//! Arbitrage detection: triangular cycle evaluation and profit estimation.
//!
//! For each triangle `A → B → C → A` we fetch three order books and compute
//! the implied round-trip exchange rate.  A profit exists when:
//!
//! ```text
//! rate(A→B) × rate(B→C) × rate(C→A) > 1 + fees
//! ```
//!
//! We use the **best ask** for each leg (we are the buyer on every hop).

use crate::{
    config::{ArbConfig, ArbTriangle},
    executor,
};
use anyhow::Result;
use common::{AgentActionEvent, HorizonClient, KafkaPublisher, Keypair, OrderBook};
use rust_decimal::prelude::ToPrimitive;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, info, warn};

// ── Detected opportunity ──────────────────────────────────────────────────────

#[derive(Debug)]
pub struct ArbOpportunity {
    pub triangle_idx:  usize,
    /// Round-trip rate (e.g. 1.008 = 0.8% profit before fees).
    pub gross_rate:    f64,
    /// Net profit estimate in XLM.
    pub net_profit:    f64,
    /// Optimal trade size in XLM.
    pub trade_size_xlm: f64,
    /// Rates for each leg.
    pub rate_ab:       f64,
    pub rate_bc:       f64,
    pub rate_ca:       f64,
}

// ── Main detection loop ───────────────────────────────────────────────────────

pub async fn run_detection_loop(
    cfg:     &ArbConfig,
    horizon: &HorizonClient,
    keypair: &Keypair,
    kafka:   &KafkaPublisher,
) -> Result<()> {
    info!(
        triangles    = cfg.triangles.len(),
        interval_ms  = cfg.scan_interval_ms,
        dry_run      = cfg.dry_run,
        "Detection loop started"
    );

    let interval = Duration::from_millis(cfg.scan_interval_ms);
    let mut consecutive_errors = 0u32;

    loop {
        match scan_triangles(cfg, horizon, keypair, kafka).await {
            Ok(n) => {
                if n > 0 { info!("{n} arbitrage trade(s) executed"); }
                consecutive_errors = 0;
            }
            Err(e) => {
                consecutive_errors += 1;
                let backoff = consecutive_errors.min(30);
                warn!("Detection error (backoff {backoff}s): {e:#}");
                sleep(Duration::from_secs(backoff as u64)).await;
            }
        }
        sleep(interval).await;
    }
}

async fn scan_triangles(
    cfg:     &ArbConfig,
    horizon: &HorizonClient,
    keypair: &Keypair,
    kafka:   &KafkaPublisher,
) -> Result<u32> {
    let mut executed = 0u32;

    // Fetch all three order books for each triangle in parallel.
    for (tri_idx, tri) in cfg.triangles.iter().enumerate() {
        let (ob_ab, ob_bc, ob_ca) = tokio::try_join!(
            horizon.get_order_book(&tri.asset_a, &tri.asset_b, 5),
            horizon.get_order_book(&tri.asset_b, &tri.asset_c, 5),
            horizon.get_order_book(&tri.asset_c, &tri.asset_a, 5),
        )?;

        if let Some(opp) = evaluate_triangle(&ob_ab, &ob_bc, &ob_ca, tri_idx, tri, cfg) {
            debug!(
                triangle = tri_idx,
                gross    = opp.gross_rate,
                profit   = opp.net_profit,
                size_xlm = opp.trade_size_xlm,
                "Arbitrage opportunity"
            );

            if cfg.dry_run {
                info!("[DRY RUN] Would execute triangle {tri_idx}: profit ≈ {:.6} XLM",
                      opp.net_profit);
            } else {
                match executor::execute_triangle(cfg, horizon, keypair, &opp, tri).await {
                    Ok(hash) => {
                        info!(tx = %hash, profit = opp.net_profit, "Arbitrage executed");
                        kafka.publish_action(&AgentActionEvent {
                            agent_type:   "arbitrage_tracker".into(),
                            agent_wallet: keypair.public_key.clone(),
                            action:       "tri_arb".into(),
                            asset_pair:   Some(format!("{}/{}/{}", tri.asset_a.code(), tri.asset_b.code(), tri.asset_c.code())),
                            tx_hash:      Some(hash),
                            profit_xlm:   Some(opp.net_profit),
                            latency_ms:   None,
                            created_at:   common::pubsub::now_iso(),
                        }).await;
                        executed += 1;
                    }
                    Err(e) => warn!("Triangle {tri_idx} execution failed: {e:#}"),
                }
            }
        }
    }

    Ok(executed)
}

// ── Triangle evaluation ───────────────────────────────────────────────────────

fn evaluate_triangle(
    ob_ab:   &OrderBook,
    ob_bc:   &OrderBook,
    ob_ca:   &OrderBook,
    tri_idx: usize,
    _tri:     &ArbTriangle,
    cfg:     &ArbConfig,
) -> Option<ArbOpportunity> {
    // Rate for each leg = 1 / best_ask (we are the taker on every hop)
    let rate_ab = ob_ab.best_ask()?.to_f64()?;
    let rate_bc = ob_bc.best_ask()?.to_f64()?;
    let rate_ca = ob_ca.best_ask()?.to_f64()?;

    if rate_ab <= 0.0 || rate_bc <= 0.0 || rate_ca <= 0.0 { return None; }

    // Round-trip rate when buying each leg
    // A→B: pay 1A, get (1/rate_ab) B
    // B→C: pay 1B, get (1/rate_bc) C
    // C→A: pay 1C, get (1/rate_ca) A
    let gross_rate = 1.0 / (rate_ab * rate_bc * rate_ca);

    // Net after 3 × fee (2 ops per leg, sharing tx with 3 ops = 6 ops total)
    let fee_xlm = cfg.common.base_fee_stroops as f64 * 6.0 / 10_000_000.0;

    // Optimal trade size: limited by the thinnest order book depth
    let depth_a = ob_ab.ask_depth(3).to_f64().unwrap_or(0.0);
    let depth_b = ob_bc.ask_depth(3).to_f64().unwrap_or(0.0);
    let depth_c = ob_ca.ask_depth(3).to_f64().unwrap_or(0.0);
    let max_depth = depth_a.min(depth_b).min(depth_c);
    let trade_size_xlm = max_depth.min(cfg.max_trade_xlm);

    if trade_size_xlm < 0.1 { return None; }

    let net_profit = trade_size_xlm * (gross_rate - 1.0) - fee_xlm;

    if gross_rate < 1.0 + cfg.min_profit_ratio { return None; }
    if net_profit < 0.0 { return None; }

    Some(ArbOpportunity {
        triangle_idx:  tri_idx,
        gross_rate,
        net_profit,
        trade_size_xlm,
        rate_ab,
        rate_bc,
        rate_ca,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use common::{Asset, OrderBook, OrderBookLevel};

    fn book(ask_price: &str, ask_amount: &str) -> OrderBook {
        OrderBook {
            bids: vec![],
            asks: vec![OrderBookLevel {
                price:  ask_price.to_string(),
                amount: ask_amount.to_string(),
            }],
        }
    }

    #[test]
    fn profitable_triangle_detected() {
        let ob_ab = book("1.0",  "1000");
        let ob_bc = book("1.0",  "1000");
        let ob_ca = book("0.97", "1000"); // 3% cheaper on last leg

        let tri = crate::config::ArbTriangle {
            asset_a: Asset::native(),
            asset_b: Asset::credit("USDC", "GBBD"),
            asset_c: Asset::credit("yXLM", "GARD"),
        };

        use common::config::CommonConfig;
        let cfg = ArbConfig {
            common: CommonConfig {
                horizon_url: "".into(),
                network_passphrase: "".into(),
                soroban_rpc_url: "".into(),
                contract_id: "".into(),
                agent_secret: "SCZANGBA5RLBRQ46SL6GFBFXQ3QJYR57G5YHC7VKPLLK2NNZRHIBSG".into(),
                base_fee_stroops: 100,
                max_slippage_bps: 50,
                log_level: "debug".into(),
            },
            triangles: vec![],
            min_profit_ratio: 0.001,
            max_trade_xlm: 500.0,
            scan_interval_ms: 300,
            dry_run: true,
            tx_expiry_secs: 30,
        };

        let opp = evaluate_triangle(&ob_ab, &ob_bc, &ob_ca, 0, &tri, &cfg);
        assert!(opp.is_some());
        let o = opp.unwrap();
        assert!(o.gross_rate > 1.0);
    }
}
