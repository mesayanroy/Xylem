//! MEV strategy: order-book imbalance detection and opportunity scoring.
//!
//! ## Strategy overview
//! 1. Fetch the DEX order book for each configured pair every `poll_interval_ms`.
//! 2. Calculate the bid/ask imbalance ratio (`bid_depth / ask_depth`).
//! 3. If the ratio exceeds `imbalance_threshold`, compute the expected profit
//!    from placing an opposing order ahead of the imbalance.
//! 4. If profit > `min_profit_xlm`, submit a sandwich order via `executor`.
//!
//! ## Gas optimisation
//! - Transactions carry a strict 30-second time-bound; expired orders are
//!   never submitted, saving fee waste.
//! - We pre-sign the cancel leg while the trade leg is in-flight so both
//!   can be submitted back-to-back with minimal latency.

use crate::{
    config::MevBotConfig,
    executor,
};
use anyhow::Result;
use common::{HorizonClient, Keypair, OrderBook};
use rust_decimal::prelude::ToPrimitive;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, info, warn};

// ── Opportunity ───────────────────────────────────────────────────────────────

/// A detected MEV opportunity on a single pair.
#[derive(Debug)]
pub struct Opportunity {
    pub pair_index:        usize,
    /// Detected imbalance ratio (bid_depth / ask_depth or inverse).
    pub imbalance_ratio:   f64,
    /// Direction: true = buy pressure (we should front-buy then sell), false = sell pressure.
    pub buy_pressure:      bool,
    /// Size of the opportunity in XLM equivalent.
    pub opportunity_size:  f64,
    /// Estimated net profit in XLM after fees.
    pub estimated_profit:  f64,
    /// Price at time of detection (sell → buy rate).
    pub detected_price:    f64,
}

// ── Scan loop ─────────────────────────────────────────────────────────────────

/// Main scan loop — polls order books and fires trades on detected opportunities.
pub async fn scan_loop(
    cfg:            &MevBotConfig,
    horizon:        &HorizonClient,
    keypair:        &Keypair,
    _payment_client: &common::PaymentClient,
    kafka:          &common::KafkaPublisher,
) -> Result<()> {
    let interval = Duration::from_millis(cfg.poll_interval_ms);
    let mut consecutive_errors: u32 = 0;

    info!(
        interval_ms = cfg.poll_interval_ms,
        pairs        = cfg.pairs.len(),
        min_profit   = cfg.min_profit_xlm,
        "Scan loop started"
    );

    loop {
        match scan_once(cfg, horizon, keypair, kafka).await {
            Ok(opportunities_taken) => {
                if opportunities_taken > 0 {
                    info!("{opportunities_taken} MEV opportunit(ies) executed this cycle");
                }
                consecutive_errors = 0;
            }
            Err(e) => {
                consecutive_errors += 1;
                let backoff_secs = consecutive_errors.min(30);
                warn!("Scan error (backoff {backoff_secs}s): {e:#}");
                sleep(Duration::from_secs(backoff_secs as u64)).await;
            }
        }

        sleep(interval).await;
    }
}

/// Single scan pass across all configured pairs.
///
/// Returns the number of opportunities that were executed.
async fn scan_once(
    cfg:     &MevBotConfig,
    horizon: &HorizonClient,
    keypair: &Keypair,
    kafka:   &common::KafkaPublisher,
) -> Result<u32> {
    use common::pubsub::{now_iso, AgentActionEvent};

    let mut executed = 0u32;

    // Fetch all order books in parallel for minimum latency.
    let futures: Vec<_> = cfg
        .pairs
        .iter()
        .enumerate()
        .map(|(i, pair)| {
            let h = horizon.clone();
            let sell = pair.sell_asset.clone();
            let buy  = pair.buy_asset.clone();
            async move {
                h.get_order_book(&sell, &buy, cfg.depth_levels as u32 + 5)
                 .await
                 .map(|ob| (i, ob))
            }
        })
        .collect();

    let results = futures_util::future::join_all(futures).await;

    for result in results {
        match result {
            Err(e) => warn!("Order book fetch error: {e}"),
            Ok((i, ob)) => {
                if let Some(opp) = analyse_book(&ob, i, cfg) {
                    debug!(
                        pair    = i,
                        ratio   = opp.imbalance_ratio,
                        profit  = opp.estimated_profit,
                        "Opportunity detected"
                    );

                    match executor::execute(cfg, horizon, keypair, &opp, i).await {
                        Ok(hash) => {
                            info!(tx = %hash, profit = opp.estimated_profit, "MEV trade submitted");

                            // Publish the trade event to Kafka so the AgentForge
                            // dashboard and billing system reflect the earnings.
                            kafka.publish_action(&AgentActionEvent {
                                agent_type:   "mev_bot".into(),
                                agent_wallet: keypair.public_key.clone(),
                                action:       if opp.buy_pressure { "front_buy" } else { "front_sell" }.into(),
                                asset_pair:   cfg.pairs.get(i).map(|p| format!("{}/{}", p.sell_asset, p.buy_asset)),
                                tx_hash:      Some(hash),
                                profit_xlm:   Some(opp.estimated_profit),
                                latency_ms:   None,
                                created_at:   now_iso(),
                            }).await;

                            executed += 1;
                        }
                        Err(e) => warn!("Execution failed: {e:#}"),
                    }
                }
            }
        }
    }

    Ok(executed)
}

// ── Analysis ──────────────────────────────────────────────────────────────────

/// Analyse an order book and return an `Opportunity` if one exists.
fn analyse_book(ob: &OrderBook, pair_idx: usize, cfg: &MevBotConfig) -> Option<Opportunity> {
    let bid_depth = ob.bid_depth(cfg.depth_levels).to_f64()?;
    let ask_depth = ob.ask_depth(cfg.depth_levels).to_f64()?;

    if bid_depth < 1e-9 || ask_depth < 1e-9 { return None; }

    let (ratio, buy_pressure) = if bid_depth > ask_depth {
        (bid_depth / ask_depth, true)
    } else {
        (ask_depth / bid_depth, false)
    };

    if ratio < cfg.imbalance_threshold { return None; }

    let detected_price = ob.mid_price()?.to_f64()?;
    let spread = ob.spread_bps()?.to_f64()?;

    // Opportunity size: smaller of our max position and half the imbalanced depth
    let imbalanced_depth = if buy_pressure { bid_depth } else { ask_depth };
    let opportunity_size = (imbalanced_depth * 0.5).min(cfg.max_position_xlm);

    // Estimated profit: capture spread minus 2× fee (entry + exit) minus slippage buffer
    let fee_xlm = (cfg.common.base_fee_stroops as f64 * 2.0) / 10_000_000.0;
    let slippage_buffer = opportunity_size * (cfg.common.max_slippage_bps as f64 / 10_000.0);
    let gross_profit = opportunity_size * (spread / 10_000.0); // spread in bps → fraction
    let estimated_profit = gross_profit - fee_xlm * 2.0 - slippage_buffer;

    if estimated_profit < cfg.min_profit_xlm { return None; }

    Some(Opportunity {
        pair_index:       pair_idx,
        imbalance_ratio:  ratio,
        buy_pressure,
        opportunity_size,
        estimated_profit,
        detected_price,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use common::{OrderBook, OrderBookLevel};

    fn make_book(bids: &[(&str, &str)], asks: &[(&str, &str)]) -> OrderBook {
        OrderBook {
            bids: bids.iter().map(|(p, a)| OrderBookLevel {
                price: p.to_string(), amount: a.to_string()
            }).collect(),
            asks: asks.iter().map(|(p, a)| OrderBookLevel {
                price: p.to_string(), amount: a.to_string()
            }).collect(),
        }
    }

    #[test]
    fn detects_bid_imbalance() {
        let ob = make_book(
            &[("1.01", "300"), ("1.005", "200")],
            &[("1.02", "50"),  ("1.025", "30")],
        );

        use common::config::CommonConfig;
        let fake_cfg = crate::config::MevBotConfig {
            common: CommonConfig {
                horizon_url:        "https://horizon-testnet.stellar.org".into(),
                network_passphrase: common::config::TESTNET_PASSPHRASE.into(),
                soroban_rpc_url:    "".into(),
                contract_id:        "".into(),
                agent_secret:       "SCZANGBA5RLBRQ46SL6GFBFXQ3QJYR57G5YHC7VKPLLK2NNZRHIBSG".into(),
                base_fee_stroops:   100,
                max_slippage_bps:   50,
                log_level:          "debug".into(),
            },
            pairs:                vec![],
            imbalance_threshold:  3.0,
            min_profit_xlm:       0.001,
            max_position_xlm:     1000.0,
            poll_interval_ms:     500,
            depth_levels:         10,
            tx_expiry_secs:       30,
            fee_bump_stroops:     500,
        };

        let opp = analyse_book(&ob, 0, &fake_cfg);
        assert!(opp.is_some(), "Should detect bid-side imbalance");
        let o = opp.unwrap();
        assert!(o.buy_pressure);
        assert!(o.imbalance_ratio > 3.0);
    }
}
