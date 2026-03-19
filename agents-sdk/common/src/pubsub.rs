//! Kafka pub-sub client for agent event publishing.
//!
//! Agents publish structured events to the AgentForge Kafka backbone so that:
//! - The platform dashboard can display real-time activity
//! - Other agents can subscribe and react (A2A coordination)
//! - Billing and analytics consumers can process earnings
//!
//! This implementation talks to Upstash Kafka via its REST API, which
//! requires no native TCP socket — compatible with any deployment target.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, warn};

// ── Topic constants ───────────────────────────────────────────────────────────
//
// Keep in sync with `lib/kafka.ts` in the Next.js platform.

pub const TOPIC_PAYMENT_PENDING:    &str = "agentforge.payment.pending";
pub const TOPIC_PAYMENT_CONFIRMED:  &str = "agentforge.payment.confirmed";
pub const TOPIC_AGENT_COMPLETED:    &str = "agentforge.agent.completed";
pub const TOPIC_BILLING_UPDATED:    &str = "agentforge.billing.updated";
pub const TOPIC_MARKETPLACE_ACTIVITY: &str = "agentforge.marketplace.activity";
pub const TOPIC_CHAIN_SYNCED:       &str = "agentforge.chain.synced";
pub const TOPIC_A2A_REQUEST:        &str = "agentforge.a2a.request";
pub const TOPIC_A2A_RESPONSE:       &str = "agentforge.a2a.response";

// ── Event payload types ───────────────────────────────────────────────────────

/// Published by every agent whenever it executes a trade / action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentActionEvent {
    pub agent_type:    String,  // "mev_bot" | "arbitrage_tracker" | etc.
    pub agent_wallet:  String,
    pub action:        String,
    pub asset_pair:    Option<String>,
    pub tx_hash:       Option<String>,
    pub profit_xlm:    Option<f64>,
    pub latency_ms:    Option<u64>,
    pub created_at:    String,
}

/// Published when a payment was received for a service call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentReceivedEvent {
    pub payer_wallet:    String,
    pub receiver_wallet: String,
    pub amount_xlm:      f64,
    pub tx_hash:         String,
    pub memo:            String,
    pub service:         String,
    pub created_at:      String,
}

/// Published on chain events (new offer, trade fill, contract call).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainEvent {
    pub event_type:  String,
    pub tx_hash:     String,
    pub ledger:      u64,
    pub account:     String,
    pub details:     serde_json::Value,
    pub created_at:  String,
}

// ── Client ────────────────────────────────────────────────────────────────────

/// Upstash Kafka REST producer client.
///
/// Instantiate once and pass a shared reference to all agent modules.
/// All methods are cheap-to-clone thanks to the inner `Arc<Client>`.
#[derive(Clone)]
pub struct KafkaPublisher {
    http:     Client,
    url:      String,
    username: String,
    password: String,
    enabled:  bool,
}

impl KafkaPublisher {
    /// Create from environment variables.
    ///
    /// If `UPSTASH_KAFKA_BROKER` is not set, the publisher is created in
    /// **disabled** mode — `publish` calls become silent no-ops.
    pub fn from_env() -> Self {
        let url      = std::env::var("UPSTASH_KAFKA_BROKER").unwrap_or_default();
        let username = std::env::var("UPSTASH_KAFKA_USERNAME").unwrap_or_default();
        let password = std::env::var("UPSTASH_KAFKA_PASSWORD").unwrap_or_default();

        let enabled = !url.is_empty() && !username.is_empty() && !password.is_empty();

        if !enabled {
            warn!("Kafka not configured (UPSTASH_KAFKA_* vars missing) — pub-sub disabled");
        }

        let http = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("Kafka HTTP client init failed");

        Self { http, url, username, password, enabled }
    }

    /// Publish a JSON-serialisable payload to a topic.
    ///
    /// Fire-and-forget: errors are logged as warnings but do **not** propagate
    /// to the caller — a Kafka publish failure must never abort a trade.
    pub async fn publish<T: Serialize>(&self, topic: &str, payload: &T) {
        if !self.enabled { return; }

        let value = match serde_json::to_string(payload) {
            Ok(v)  => v,
            Err(e) => { warn!("Kafka serialize error: {e}"); return; }
        };

        // Upstash Kafka REST API: POST /produce/{topic}
        let url = format!("{}/produce/{topic}", self.url);
        let body = serde_json::json!({ "value": value });

        match self
            .http
            .post(&url)
            .basic_auth(&self.username, Some(&self.password))
            .json(&body)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                debug!("Published to Kafka topic {topic}");
            }
            Ok(resp) => {
                warn!("Kafka publish non-success {} for topic {topic}", resp.status());
            }
            Err(e) => {
                warn!("Kafka publish error for topic {topic}: {e}");
            }
        }
    }

    /// Convenience: publish an [`AgentActionEvent`].
    pub async fn publish_action(&self, evt: &AgentActionEvent) {
        self.publish(TOPIC_AGENT_COMPLETED, evt).await;
        self.publish(TOPIC_MARKETPLACE_ACTIVITY, evt).await;
    }

    /// Convenience: publish a [`PaymentReceivedEvent`].
    pub async fn publish_payment(&self, evt: &PaymentReceivedEvent) {
        self.publish(TOPIC_PAYMENT_CONFIRMED, evt).await;
        self.publish(TOPIC_BILLING_UPDATED, evt).await;
    }

    /// Convenience: publish a [`ChainEvent`].
    pub async fn publish_chain_event(&self, evt: &ChainEvent) {
        self.publish(TOPIC_CHAIN_SYNCED, evt).await;
    }
}

// ── Timestamp helper ──────────────────────────────────────────────────────────

/// Returns the current UTC time in ISO-8601 format.
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}
