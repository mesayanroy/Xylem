//! Relayer configuration.

use anyhow::Result;
use common::config::CommonConfig;

#[derive(Debug, Clone)]
pub struct RelayerConfig {
    pub common:                 CommonConfig,
    /// Enable fee-bump wrapping of third-party transactions.
    pub fee_bump_enabled:       bool,
    /// Fee charged to the relayer client in XLM (via 0x402).
    pub relay_fee_xlm:          f64,
    /// Wallet address of the relayer service (receives 0x402 payments).
    pub relay_fee_address:      String,
    /// Maximum fee-bump fee the relayer will pay (stroops total).
    pub max_fee_bump_stroops:   u32,
    /// Base fee added on top of current surge fee to prioritise inclusion.
    pub priority_fee_stroops:   u32,
    /// AgentForge platform API base URL (for 0x402 payment verification).
    pub platform_api_url:       String,
    /// Seconds between relay job polls (queue drain interval).
    pub poll_interval_secs:     u64,
    /// Maximum number of concurrent relay jobs.
    pub max_concurrent_jobs:    usize,
}

impl RelayerConfig {
    pub fn from_env() -> Result<Self> {
        let common = CommonConfig::from_env()?;

        let fee_bump_enabled = std::env::var("FEE_BUMP_ENABLED")
            .map(|v| v == "true" || v == "1").unwrap_or(true);

        let relay_fee_xlm: f64 = std::env::var("RELAY_FEE_XLM")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(0.001);

        // If not explicitly set, the relayer's own wallet receives the fees.
        let relay_fee_address = std::env::var("RELAY_FEE_ADDRESS")
            .unwrap_or_default();

        let max_fee_bump_stroops: u32 = std::env::var("MAX_FEE_BUMP_STROOPS")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(100_000);

        let priority_fee_stroops: u32 = std::env::var("PRIORITY_FEE_STROOPS")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(1_000);

        let platform_api_url = std::env::var("PLATFORM_API_URL")
            .unwrap_or_else(|_| "https://agentforge.xyz".to_string());

        let poll_interval_secs: u64 = std::env::var("POLL_INTERVAL_SECS")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(2);

        let max_concurrent_jobs: usize = std::env::var("MAX_CONCURRENT_JOBS")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(8);

        Ok(Self {
            common,
            fee_bump_enabled,
            relay_fee_xlm,
            relay_fee_address,
            max_fee_bump_stroops,
            priority_fee_stroops,
            platform_api_url,
            poll_interval_secs,
            max_concurrent_jobs,
        })
    }
}
