//! SSE stream monitoring loop.
//!
//! Connects to the Horizon `/transactions` SSE feed and evaluates each
//! incoming transaction against the configured [`AlertRule`]s.

use crate::{
    alerts::{self, AlertEvent},
    config::MonitorConfig,
};
use anyhow::Result;
use common::{horizon::SseTransaction, ChainEvent, HorizonClient, KafkaPublisher};
use tokio::sync::mpsc;
use tracing::{debug, info};

/// Capacity of the channel between the SSE reader and alert processor.
const CHANNEL_CAPACITY: usize = 4096;

/// Start the monitor: spawns an SSE reader and runs the alert processor.
pub async fn run(cfg: &MonitorConfig, horizon: &HorizonClient, kafka: &KafkaPublisher) -> Result<()> {
    info!(cursor = %cfg.stream_cursor, "Subscribing to Horizon SSE stream");

    let (tx, mut rx) = mpsc::channel::<SseTransaction>(CHANNEL_CAPACITY);

    // Spawn the SSE stream reader in the background.
    let horizon_clone = horizon.clone();
    let cursor = cfg.stream_cursor.clone();
    tokio::spawn(async move {
        horizon_clone.stream_transactions(&cursor, tx).await;
    });

    let mut processed = 0u64;
    let mut alerted   = 0u64;

    // Process transactions as they arrive.
    while let Some(tx_event) = rx.recv().await {
        processed += 1;

        if cfg.verbose {
            debug!(
                hash      = %tx_event.hash,
                fee       = %tx_event.fee_charged,
                ops       = tx_event.operation_count,
                source    = %tx_event.source_account,
                "Transaction"
            );
        }

        let triggered: Vec<AlertEvent> = cfg
            .alert_rules
            .iter()
            .filter_map(|rule| alerts::evaluate(rule, &tx_event))
            .collect();

        for alert in &triggered {
            info!(
                alert_type = %alert.rule_name,
                hash       = %tx_event.hash,
                detail     = %alert.detail,
                "🚨 ALERT"
            );
            kafka.publish_chain_event(&ChainEvent {
                event_type: alert.rule_name.clone(),
                tx_hash:    tx_event.hash.clone(),
                ledger:     tx_event.ledger,
                account:    tx_event.source_account.clone(),
                details:    serde_json::json!({ "detail": alert.detail }),
                created_at: common::pubsub::now_iso(),
            }).await;
            alerted += 1;
        }

        // Fire webhook if configured and this tx triggered any rule.
        if !triggered.is_empty() {
            if let Some(ref url) = cfg.webhook_url {
                let payload = serde_json::json!({
                    "tx_hash":    tx_event.hash,
                    "source":     tx_event.source_account,
                    "fee":        tx_event.fee_charged,
                    "ops":        tx_event.operation_count,
                    "ledger":     tx_event.ledger,
                    "created_at": tx_event.created_at,
                    "alerts":     triggered.iter().map(|a| {
                        serde_json::json!({ "rule": a.rule_name, "detail": a.detail })
                    }).collect::<Vec<_>>(),
                });
                let url = url.clone();
                tokio::spawn(async move {
                    if let Ok(client) = reqwest::Client::builder().build() {
                        let _ = client.post(&url).json(&payload).send().await;
                    }
                });
            }
        }

        // Periodic progress log every 10 000 transactions.
        if processed % 10_000 == 0 {
            info!(processed, alerted, "Monitor progress checkpoint");
        }
    }

    info!(processed, alerted, "Monitor stream ended");
    Ok(())
}
