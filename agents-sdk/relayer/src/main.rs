//! Relayer — entry point.
//!
//! The relayer serves two purposes:
//! 1. **Fee-bump relay** — accepts a pre-signed transaction XDR from a
//!    third party and wraps it in a FeeBumpTransaction, paying the fee on
//!    their behalf.  This lets gasless clients use the network.
//! 2. **Sequence-managed relay** — manages sequence numbers for a pool of
//!    accounts so callers don't need to track state.
//!
//! The relayer charges callers via the **0x402 payment protocol**: the
//! caller must include a valid Stellar payment tx hash in their request,
//! and the payment is confirmed before the transaction is relayed.
//!
//! ## Usage
//! ```
//! cp .env.template .env
//! cargo run --release --bin relayer
//! ```

mod config;
mod relay;

use anyhow::Result;
use common::{HorizonClient, KafkaPublisher, Keypair};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cfg = config::RelayerConfig::from_env()?;

    tracing_subscriber::fmt()
        .with_env_filter(&cfg.common.log_level)
        .with_target(false)
        .compact()
        .init();

    info!(
        horizon  = %cfg.common.horizon_url,
        fee_bump = cfg.fee_bump_enabled,
        "Relayer starting"
    );

    let horizon = HorizonClient::new(&cfg.common.horizon_url)?;
    let keypair = Keypair::from_secret(&cfg.common.agent_secret)?;
    let kafka   = KafkaPublisher::from_env();

    info!(address = %keypair.public_key, "Fee-payer wallet loaded");

    relay::run_relay_loop(&cfg, &horizon, &keypair, &kafka).await
}
