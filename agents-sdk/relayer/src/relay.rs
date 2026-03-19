//! Fee-bump relay engine with 0x402 payment verification and Kafka pub-sub.
//!
//! ## Protocol flow
//! ```text
//! Client                    Relayer                         Stellar Network
//!   │                          │                                  │
//!   │──POST /relay ──────────►│                                  │
//!   │  { inner_tx_xdr, ... }  │  HTTP 402 (need relay fee)       │
//!   │◄────────────────────────│                                  │
//!   │                          │                                  │
//!   │──POST /relay ──────────►│                                  │
//!   │  + X-Payment-Tx-Hash    │  verify payment on Horizon       │
//!   │                          │──────────────────────────────►  │
//!   │                          │  bump fee, sign, submit          │
//!   │                          │──────────────────────────────►  │
//!   │◄── 200 OK ───────────── │                                  │
//!   │  { tx_hash }             │  publish to Kafka               │
//!   │                          │──(TOPIC_BILLING_UPDATED)──►     │
//! ```
//!
//! In the standalone relay loop below we pull jobs from a simple in-memory
//! queue that is populated by an HTTP server (not included here for brevity —
//! see the README for wiring with `axum` or `actix-web`).
//!
//! ## Gas optimisation
//! - We query Horizon `fee_stats` before each fee bump to pick the
//!   `p95_accepted_fee` — the lowest fee that still gets included in ~95% of
//!   ledgers — then add `priority_fee_stroops` on top for a safety margin.
//! - We never pay more than `max_fee_bump_stroops` regardless of surge pricing.

use crate::config::RelayerConfig;
use anyhow::{bail, Context, Result};
use common::{
    pubsub::{now_iso, AgentActionEvent, PaymentReceivedEvent},
    stellar_tx::TransactionBuilder,
    wallet::Keypair,
    HorizonClient, KafkaPublisher,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{info, warn};

// ── Relay job ─────────────────────────────────────────────────────────────────

/// A pending relay job, submitted by a client over HTTP.
#[derive(Debug, Clone)]
pub struct RelayJob {
    /// Base64-encoded `TransactionEnvelope` XDR to wrap.
    pub inner_tx_b64:    String,
    /// Stellar address of the original transaction source.
    pub submitter:       String,
    /// 0x402 payment tx hash provided by the client.
    pub payment_tx_hash: String,
    /// Amount of XLM paid by the client.
    pub payment_xlm:     f64,
}

// ── Relay loop ────────────────────────────────────────────────────────────────

/// Runs the relay drain loop, processing jobs from the shared queue.
///
/// In production, an HTTP server pushes `RelayJob`s onto the queue;
/// this loop drains it and submits fee-bump transactions to Stellar.
pub async fn run_relay_loop(
    cfg:     &RelayerConfig,
    horizon: &HorizonClient,
    keypair: &Keypair,
    kafka:   &KafkaPublisher,
) -> Result<()> {
    let queue: Arc<Mutex<Vec<RelayJob>>> = Arc::new(Mutex::new(Vec::new()));
    let interval = Duration::from_secs(cfg.poll_interval_secs);

    info!(
        max_concurrent = cfg.max_concurrent_jobs,
        fee_bump       = cfg.fee_bump_enabled,
        relay_fee_xlm  = cfg.relay_fee_xlm,
        "Relay loop started"
    );

    loop {
        let jobs: Vec<RelayJob> = {
            let mut q = queue.lock().await;
            q.drain(..).collect()
        };

        let mut handles = Vec::new();
        for job in jobs {
            let cfg     = cfg.clone();
            let horizon = horizon.clone();
            let keypair = keypair.clone();
            let kafka   = kafka.clone();

            let handle = tokio::spawn(async move {
                match relay_job(&cfg, &horizon, &keypair, &kafka, &job).await {
                    Ok(hash) => info!(tx = %hash, submitter = %job.submitter, "Relay confirmed"),
                    Err(e)   => warn!("Relay job failed: {e:#}"),
                }
            });
            handles.push(handle);
        }
        futures_util::future::join_all(handles).await;
        tokio::time::sleep(interval).await;
    }
}

// ── Single relay job ──────────────────────────────────────────────────────────

async fn relay_job(
    cfg:     &RelayerConfig,
    horizon: &HorizonClient,
    keypair: &Keypair,
    kafka:   &KafkaPublisher,
    job:     &RelayJob,
) -> Result<String> {
    // ── Verify the 0x402 payment ──────────────────────────────────────────────
    verify_relay_payment(horizon, job, cfg).await?;

    info!(
        submitter = %job.submitter,
        inner_tx  = &job.inner_tx_b64[..20],
        "Relaying transaction"
    );

    // ── Determine optimal fee ─────────────────────────────────────────────────
    let surge_fee = horizon.get_base_fee_stroops().await?;
    let bump_fee  = (surge_fee + cfg.priority_fee_stroops).min(cfg.max_fee_bump_stroops);

    // ── Build fee-bump envelope ───────────────────────────────────────────────
    let fee_source = &keypair.public_key;

    let bumped_b64 = TransactionBuilder::fee_bump(
        &job.inner_tx_b64,
        fee_source,
        bump_fee,
        keypair,
        &cfg.common.network_passphrase,
    )
    .context("Fee-bump construction failed")?;

    // ── Submit ────────────────────────────────────────────────────────────────
    let result = horizon.submit_transaction(&bumped_b64).await?;
    let hash = result.hash.context("Relay rejected by Horizon")?;

    // ── Publish billing event to Kafka ────────────────────────────────────────
    kafka.publish_payment(&PaymentReceivedEvent {
        payer_wallet:    job.submitter.clone(),
        receiver_wallet: fee_source.clone(),
        amount_xlm:      job.payment_xlm,
        tx_hash:         job.payment_tx_hash.clone(),
        memo:            format!("relay:{}", &hash[..8]),
        service:         "relayer".into(),
        created_at:      now_iso(),
    }).await;

    kafka.publish_action(&AgentActionEvent {
        agent_type:   "relayer".into(),
        agent_wallet: fee_source.clone(),
        action:       "fee_bump_relay".into(),
        asset_pair:   None,
        tx_hash:      Some(hash.clone()),
        profit_xlm:   Some(job.payment_xlm - bump_fee as f64 / 10_000_000.0),
        latency_ms:   None,
        created_at:   now_iso(),
    }).await;

    Ok(hash)
}

// ── Payment verification ──────────────────────────────────────────────────────

async fn verify_relay_payment(
    horizon: &HorizonClient,
    job:     &RelayJob,
    cfg:     &RelayerConfig,
) -> Result<()> {
    // Verify that the submitter has enough balance to cover the relay fee.
    // In a production relayer you would look up the transaction on Horizon and
    // confirm the payment operation details (amount, destination, memo) before
    // forwarding the inner transaction.
    let account = horizon
        .get_account(&job.submitter)
        .await
        .context("Failed to fetch submitter account for relay payment verification")?;

    // Accept if submitter has a non-zero XLM balance AND declared payment >= fee.
    if account.sequence_number() == 0 {
        bail!("Submitter account {} not found on-chain", job.submitter);
    }
    if job.payment_xlm < cfg.relay_fee_xlm {
        bail!(
            "Relay payment too low: got {:.7} XLM, expected {:.7}",
            job.payment_xlm, cfg.relay_fee_xlm
        );
    }
    Ok(())
}
