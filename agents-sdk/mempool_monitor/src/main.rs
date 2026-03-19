//! Mempool Monitor — entry point.
//!
//! Subscribes to the Horizon transaction SSE stream and routes each
//! event through configurable alert rules (high-fee spike, large payment,
//! watched address activity, offer creation, etc.).
//!
//! ## 0x402 Protocol
//! Optionally forwards high-value alert events to a paid intelligence
//! agent via [`common::PaymentClient`].
//!
//! ## Pub-Sub
//! All matched alerts are published to the `agentforge.chain.synced`
//! Kafka topic so dashboards and other agents can react in real time.
//!
//! ## Usage
//! ```
//! cp .env.template .env
//! cargo run --release --bin mempool_monitor
//! ```

mod alerts;
mod config;
mod monitor;

use anyhow::Result;
use common::{HorizonClient, KafkaPublisher, Keypair, PaymentClient};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cfg = config::MonitorConfig::from_env()?;

    tracing_subscriber::fmt()
        .with_env_filter(&cfg.common.log_level)
        .with_target(false)
        .compact()
        .init();

    info!(
        horizon    = %cfg.common.horizon_url,
        rules      = cfg.alert_rules.len(),
        "Mempool Monitor starting"
    );

    let horizon  = HorizonClient::new(&cfg.common.horizon_url)?;
    let keypair  = Keypair::from_secret(&cfg.common.agent_secret)?;
    let _payment = PaymentClient::new(
        keypair.clone(),
        &cfg.common.horizon_url,
        &cfg.common.network_passphrase,
    )?;
    let kafka    = KafkaPublisher::from_env();

    info!(address = %keypair.public_key, "Wallet loaded");

    monitor::run(&cfg, &horizon, &kafka).await
}
