//! Trading Bot — entry point.
//!
//! Implements three strategies on the Stellar DEX:
//! - **Buy**   — market/limit buy using ManageBuyOffer
//! - **Sell**  — market/limit sell using ManageSellOffer
//! - **Short** — synthetic short via borrow-equivalent: sell an asset you
//!               hold for XLM, wait for price drop, repurchase cheaper.
//!
//! ## 0x402 Protocol
//! Enriches price feeds with paid off-chain data via [`common::PaymentClient`]
//! when `PRICE_FEED_AGENT_URL` is set.
//!
//! ## Pub-Sub
//! Every order fill publishes to `agentforge.agent.completed` so billing
//! and the dashboard are updated in real time.
//!
//! ## Usage
//! ```
//! cp .env.template .env
//! # Set STRATEGY=buy|sell|short, ASSET, AMOUNT, etc.
//! cargo run --release --bin trading_bot
//! ```

mod config;
mod orders;
mod strategy;

use anyhow::Result;
use common::{HorizonClient, KafkaPublisher, Keypair, PaymentClient};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cfg = config::TradingBotConfig::from_env()?;

    tracing_subscriber::fmt()
        .with_env_filter(&cfg.common.log_level)
        .with_target(false)
        .compact()
        .init();

    info!(
        strategy   = ?cfg.active_strategy,
        horizon    = %cfg.common.horizon_url,
        "Trading Bot starting"
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
    info!("0x402 + Kafka enabled for real-time billing and price intelligence");

    strategy::run(&cfg, &horizon, &keypair, &kafka).await
}
