//! MEV Bot — entry point.
//!
//! Connects to Horizon, watches for large order-book imbalances on configured
//! trading pairs, and submits front-run / sandwich orders when opportunities
//! exceed the configured profit threshold.
//!
//! ## 0x402 Protocol
//! The MEV bot can optionally call AgentForge AI agents for real-time market
//! intelligence (e.g. sentiment analysis, token risk scores). Those calls use
//! the 0x402 payment protocol — the bot automatically handles the pay-per-
//! request dance via [`common::PaymentClient`].
//!
//! ## Pub-Sub
//! Every executed trade is published to the Kafka backbone via
//! [`common::KafkaPublisher`] so the AgentForge dashboard and billing system
//! can reflect earnings in real time.
//!
//! ## Usage
//! ```
//! cp .env.template .env
//! # fill in AGENT_SECRET_KEY, pairs, thresholds …
//! cargo run --release --bin mev_bot
//! ```

mod config;
mod executor;
mod strategy;

use anyhow::Result;
use common::{HorizonClient, KafkaPublisher, Keypair, PaymentClient};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cfg = config::MevBotConfig::from_env()?;

    tracing_subscriber::fmt()
        .with_env_filter(&cfg.common.log_level)
        .with_target(false)
        .compact()
        .init();

    info!(
        network  = if cfg.common.is_mainnet() { "mainnet" } else { "testnet" },
        horizon  = %cfg.common.horizon_url,
        pairs    = cfg.pairs.len(),
        "MEV Bot starting"
    );

    let horizon         = HorizonClient::new(&cfg.common.horizon_url)?;
    let keypair         = Keypair::from_secret(&cfg.common.agent_secret)?;

    // 0x402 client — used for paid A2A calls to AgentForge intelligence agents.
    let payment_client  = PaymentClient::new(
        keypair.clone(),
        &cfg.common.horizon_url,
        &cfg.common.network_passphrase,
    )?;

    // Kafka publisher — publishes trade events to the AgentForge platform.
    let kafka           = KafkaPublisher::from_env();

    info!(address = %keypair.public_key, "Loaded wallet");
    info!("0x402 payment client ready — A2A calls will auto-pay via Stellar");
    info!("Kafka publisher ready — trades will stream to AgentForge dashboard");

    // Run the main scan loop — never returns unless a fatal error occurs.
    strategy::scan_loop(&cfg, &horizon, &keypair, &payment_client, &kafka).await
}
