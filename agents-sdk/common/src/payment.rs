//! 0x402 payment protocol client for agent-to-agent (A2A) calls.
//!
//! The 0x402 protocol flow:
//! 1. Agent A calls Agent B's API endpoint.
//! 2. Agent B responds with `HTTP 402 Payment Required` + payment details.
//! 3. Agent A constructs and signs a Stellar payment transaction.
//! 4. Agent A retries the request with the tx hash in `X-Payment-Tx-Hash`.
//! 5. Agent B verifies the on-chain payment and serves the response.
//!
//! This module encapsulates the full client-side flow so any agent template
//! can pay for another agent's service with a single function call.

use crate::wallet::Keypair;
use anyhow::{bail, Context, Result};
use reqwest::{header, Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, info};

// ── Payment challenge (from a 402 response) ───────────────────────────────────

/// The payment details returned inside a 402 response body.
#[derive(Debug, Clone, Deserialize)]
pub struct PaymentChallenge {
    pub amount_xlm:  f64,
    pub address:     String,
    pub network:     String,
    /// Stellar transaction memo (max 28 chars, as enforced by the platform).
    pub memo:        String,
}

/// Full 402 body as returned by the AgentForge `/run` endpoint.
#[derive(Debug, Deserialize)]
struct ChallengeBody {
    error:           String,
    payment_details: Option<PaymentChallenge>,
}

// ── Successful response ───────────────────────────────────────────────────────

/// Successful API response from an agent endpoint.
#[derive(Debug, Deserialize)]
pub struct AgentResponse {
    pub output:     Option<String>,
    pub request_id: Option<String>,
    /// `"pending"` when the request entered the Kafka async pipeline.
    pub status:     Option<String>,
    pub latency_ms: Option<u64>,
}

// ── Pub-sub receipt ───────────────────────────────────────────────────────────

/// Published to the `agentforge.payment.pending` Kafka topic so the platform
/// knows this agent has initiated a paid A2A call.
#[derive(Debug, Clone, Serialize)]
pub struct PaymentPendingEvent {
    pub request_id:   String,
    pub agent_id:     String,
    pub caller_agent: String,
    pub tx_hash:      String,
    pub price_xlm:    f64,
    pub input:        String,
    pub created_at:   String,
}

// ── Client ────────────────────────────────────────────────────────────────────

/// 0x402-aware HTTP client.
///
/// Automatically handles the 402 payment dance:
/// 1. Initial unauthenticated request.
/// 2. Parse `PaymentChallenge` from 402 body.
/// 3. Build & sign a Stellar payment transaction.
/// 4. Retry with payment proof in headers.
#[derive(Clone)]
pub struct PaymentClient {
    http:               Client,
    keypair:            Keypair,
    horizon_url:        String,
    network_passphrase: String,
}

impl PaymentClient {
    pub fn new(
        keypair:            Keypair,
        horizon_url:        impl Into<String>,
        network_passphrase: impl Into<String>,
    ) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .context("Failed to build PaymentClient HTTP client")?;

        Ok(Self {
            http,
            keypair,
            horizon_url:        horizon_url.into(),
            network_passphrase: network_passphrase.into(),
        })
    }

    /// Call an agent endpoint, handling the 0x402 payment dance automatically.
    ///
    /// `agent_url` — full URL, e.g. `https://agentforge.xyz/api/agents/123/run`
    /// `input`     — JSON-serialisable request body `{ "input": "..." }`
    ///
    /// Returns the [`AgentResponse`] on success.
    pub async fn call_agent(
        &self,
        agent_url: &str,
        input:     &str,
    ) -> Result<AgentResponse> {
        let body = serde_json::json!({ "input": input });

        // ── Step 1: Initial request (no payment header) ───────────────────────
        debug!(url = %agent_url, "Initial A2A request");
        let resp = self
            .http
            .post(agent_url)
            .header(header::CONTENT_TYPE, "application/json")
            .header("X-Payment-Wallet", &self.keypair.public_key)
            .json(&body)
            .send()
            .await
            .context("A2A initial request failed")?;

        // If the agent is free, we're done.
        if resp.status().is_success() {
            return parse_agent_response(resp).await;
        }

        // ── Step 2: Handle 402 ────────────────────────────────────────────────
        if resp.status() != StatusCode::PAYMENT_REQUIRED {
            let status = resp.status();
            let text   = resp.text().await.unwrap_or_default();
            bail!("Agent returned {status}: {text}");
        }

        let challenge = parse_challenge(resp).await?;
        info!(
            amount = challenge.amount_xlm,
            dest   = %challenge.address,
            memo   = %challenge.memo,
            "Received 402 — initiating Stellar payment"
        );

        // ── Step 3: Submit Stellar payment ────────────────────────────────────
        let tx_hash = self
            .submit_payment(&challenge)
            .await
            .context("0x402 payment submission failed")?;

        info!(tx = %tx_hash, "Payment submitted — retrying agent call");

        // ── Step 4: Retry with payment proof ─────────────────────────────────
        let resp2 = self
            .http
            .post(agent_url)
            .header(header::CONTENT_TYPE, "application/json")
            .header("X-Payment-Tx-Hash", &tx_hash)
            .header("X-Payment-Wallet",  &self.keypair.public_key)
            .json(&body)
            .send()
            .await
            .context("A2A paid retry failed")?;

        if !resp2.status().is_success() && resp2.status() != StatusCode::ACCEPTED {
            let status = resp2.status();
            let text   = resp2.text().await.unwrap_or_default();
            bail!("Agent rejected payment: {status} — {text}");
        }

        parse_agent_response(resp2).await
    }

    // ── Internal: build and submit a Stellar payment transaction ─────────────

    async fn submit_payment(&self, challenge: &PaymentChallenge) -> Result<String> {
        use crate::horizon::HorizonClient;
        use crate::stellar_tx::{OperationBody, TransactionBuilder};
        use crate::wallet::xlm_to_stroops;
        use std::time::{SystemTime, UNIX_EPOCH};

        let horizon  = HorizonClient::new(&self.horizon_url)?;
        let account  = horizon.get_account(&self.keypair.public_key).await?;
        let live_fee = horizon.get_base_fee_stroops().await?.max(100);

        let now      = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let max_time = now + 30; // 30-second expiry

        let amount_stroops = xlm_to_stroops(challenge.amount_xlm);
        if amount_stroops <= 0 { bail!("Payment amount too small"); }

        let tx_b64 = TransactionBuilder::new(
                &self.keypair.public_key,
                account.sequence_number() + 1,
                live_fee,
            )
            .with_timebounds(now, max_time)
            .with_memo(&challenge.memo[..challenge.memo.len().min(28)])
            .add_op(OperationBody::Payment {
                destination: challenge.address.clone(),
                asset:       crate::horizon::Asset::native(),
                amount:      amount_stroops,
            })
            .sign_and_encode(&self.keypair, &self.network_passphrase)?;

        let result = horizon.submit_transaction(&tx_b64).await?;
        result.hash.context("Payment transaction rejected by Horizon")
    }
}

// ── Parsing helpers ───────────────────────────────────────────────────────────

async fn parse_challenge(resp: Response) -> Result<PaymentChallenge> {
    let body: ChallengeBody = resp.json().await.context("Failed to parse 402 body")?;
    body.payment_details.context("402 response missing payment_details field")
}

async fn parse_agent_response(resp: Response) -> Result<AgentResponse> {
    resp.json::<AgentResponse>()
        .await
        .context("Failed to parse agent response")
}
