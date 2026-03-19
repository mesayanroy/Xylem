//! Liquidity tracker configuration.

use anyhow::{Context, Result};
use common::{config::CommonConfig, Asset};

/// A single asset pair to track.
#[derive(Debug, Clone)]
pub struct TrackedPair {
    pub base:  Asset,
    pub quote: Asset,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct LiquidityConfig {
    pub common:                CommonConfig,
    /// Asset pairs to monitor.
    pub pairs:                 Vec<TrackedPair>,
    /// Hypothetical trade sizes (in XLM) to compute slippage for.
    pub trade_sizes_xlm:       Vec<f64>,
    /// Alert when total ask depth (XLM) drops below this value.
    pub min_depth_xlm:         f64,
    /// Alert when slippage for the smallest trade size exceeds this (in bps).
    pub max_slippage_alert_bps: u32,
    /// Number of order-book levels to request from Horizon.
    pub depth_levels:          u32,
    /// Milliseconds between scans.
    pub poll_interval_ms:      u64,
    /// If true, log all metrics; otherwise only log alerts.
    pub verbose:               bool,
}

impl LiquidityConfig {
    pub fn from_env() -> Result<Self> {
        let common = CommonConfig::from_env()?;

        let pairs_str = std::env::var("TRACK_PAIRS")
            .unwrap_or_else(|_| {
                "XLM/USDC:native:USDC:GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5"
                    .to_string()
            });

        let pairs = parse_pairs(&pairs_str)
            .context("TRACK_PAIRS: format 'LABEL:sell_code:sell_issuer:buy_code:buy_issuer'")?;

        let sizes_str = std::env::var("TRADE_SIZES_XLM")
            .unwrap_or_else(|_| "10,100,500,1000".to_string());
        let trade_sizes_xlm: Vec<f64> = sizes_str
            .split(',')
            .filter_map(|v| v.trim().parse().ok())
            .collect();

        let min_depth_xlm: f64 = std::env::var("MIN_DEPTH_XLM")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(1000.0);

        let max_slippage_alert_bps: u32 = std::env::var("MAX_SLIPPAGE_ALERT_BPS")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(200);

        let depth_levels: u32 = std::env::var("DEPTH_LEVELS")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(20);

        let poll_interval_ms: u64 = std::env::var("POLL_INTERVAL_MS")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(2000);

        let verbose = std::env::var("VERBOSE")
            .map(|v| v == "true" || v == "1").unwrap_or(false);

        Ok(Self {
            common,
            pairs,
            trade_sizes_xlm,
            min_depth_xlm,
            max_slippage_alert_bps,
            depth_levels,
            poll_interval_ms,
            verbose,
        })
    }
}

fn parse_pairs(raw: &str) -> Result<Vec<TrackedPair>> {
    raw.split(';')
       .filter(|s| !s.is_empty())
       .map(|entry| {
           let parts: Vec<&str> = entry.trim().split(':').collect();
           // Format: LABEL:codeA:issuerA_or_native:codeB:issuerB_or_native
           // Minimum length: LABEL + 2 asset specs (native = 1 part, credit = 2 parts)
           if parts.len() < 3 {
               anyhow::bail!("Invalid pair: {entry}");
           }
           let label = parts[0].to_string();
           let rest  = &parts[1..];
           let (base,  rest)  = consume_asset(rest)?;
           let (quote, _)     = consume_asset(rest)?;
           Ok(TrackedPair { base, quote, label })
       })
       .collect()
}

fn consume_asset<'a>(parts: &'a [&'a str]) -> Result<(Asset, &'a [&'a str])> {
    if parts.is_empty() { anyhow::bail!("Missing asset parts"); }
    if parts[0].eq_ignore_ascii_case("native") || parts[0].eq_ignore_ascii_case("xlm") {
        Ok((Asset::native(), &parts[1..]))
    } else if parts.len() >= 2 {
        Ok((Asset::credit(parts[0], parts[1]), &parts[2..]))
    } else {
        anyhow::bail!("Non-native asset needs issuer")
    }
}
