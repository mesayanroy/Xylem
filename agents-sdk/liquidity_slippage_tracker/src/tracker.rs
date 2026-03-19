//! Liquidity tracker loop: polls order books, computes metrics, publishes events.

use crate::{config::LiquidityConfig, slippage};
use anyhow::Result;
use common::{
    pubsub::{now_iso, AgentActionEvent, ChainEvent},
    HorizonClient, KafkaPublisher, OrderBook,
};
use rust_decimal::prelude::ToPrimitive;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, info, warn};

// ── Snapshot ──────────────────────────────────────────────────────────────────

/// Full liquidity snapshot for one asset pair at one point in time.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LiquiditySnapshot {
    pub pair:               String,
    pub timestamp:          String,
    pub bid_depth_xlm:      f64,
    pub ask_depth_xlm:      f64,
    pub spread_bps:         f64,
    pub mid_price:          f64,
    pub best_bid:           f64,
    pub best_ask:           f64,
    /// Slippage (bps) keyed by trade size (XLM).
    pub buy_slippage:       Vec<SlippageEntry>,
    pub sell_slippage:      Vec<SlippageEntry>,
    pub low_liquidity_alert: bool,
    pub high_slippage_alert: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SlippageEntry {
    pub trade_xlm:   f64,
    pub slippage_bps: f64,
    pub fully_filled: bool,
}

// ── Main loop ─────────────────────────────────────────────────────────────────

pub async fn run(
    cfg:     &LiquidityConfig,
    horizon: &HorizonClient,
    kafka:   &KafkaPublisher,
) -> Result<()> {
    let interval = Duration::from_millis(cfg.poll_interval_ms);
    let mut consecutive_errors = 0u32;

    info!(
        pairs        = cfg.pairs.len(),
        trade_sizes  = ?cfg.trade_sizes_xlm,
        "Tracker loop started"
    );

    loop {
        match scan_all_pairs(cfg, horizon, kafka).await {
            Ok(_)  => { consecutive_errors = 0; }
            Err(e) => {
                consecutive_errors += 1;
                let back = consecutive_errors.min(30);
                warn!("Scan error (backoff {back}s): {e:#}");
                sleep(Duration::from_secs(back as u64)).await;
            }
        }
        sleep(interval).await;
    }
}

async fn scan_all_pairs(
    cfg:     &LiquidityConfig,
    horizon: &HorizonClient,
    kafka:   &KafkaPublisher,
) -> Result<()> {
    // Fetch all order books in parallel.
    let futures: Vec<_> = cfg.pairs.iter().map(|pair| {
        let h = horizon.clone();
        let b = pair.base.clone();
        let q = pair.quote.clone();
        let d = cfg.depth_levels;
        async move { h.get_order_book(&b, &q, d).await }
    }).collect();

    let results = futures_util::future::join_all(futures).await;

    for (pair, ob_result) in cfg.pairs.iter().zip(results) {
        match ob_result {
            Err(e) => warn!("Order book fetch error for {}: {e}", pair.label),
            Ok(ob) => {
                match build_snapshot(pair.label.clone(), &ob, cfg) {
                    None => warn!("Could not compute snapshot for {}", pair.label),
                    Some(snap) => {
                        log_snapshot(&snap, cfg.verbose);
                        publish_snapshot(kafka, &snap).await;
                    }
                }
            }
        }
    }

    Ok(())
}

// ── Snapshot computation ──────────────────────────────────────────────────────

fn build_snapshot(
    label:   String,
    ob:      &OrderBook,
    cfg:     &LiquidityConfig,
) -> Option<LiquiditySnapshot> {
    let mid      = ob.mid_price()?.to_f64()?;
    let best_bid = ob.best_bid()?.to_f64()?;
    let best_ask = ob.best_ask()?.to_f64()?;
    let spread   = ob.spread_bps()?.to_f64()?;

    let bid_depth = ob.bid_depth(cfg.depth_levels as usize).to_f64()?;
    let ask_depth = ob.ask_depth(cfg.depth_levels as usize).to_f64()?;

    let mut buy_slippage  = Vec::new();
    let mut sell_slippage = Vec::new();
    let mut max_slippage_bps = 0.0_f64;

    for &size in &cfg.trade_sizes_xlm {
        if let Some(r) = slippage::compute_buy_slippage(ob, size) {
            max_slippage_bps = max_slippage_bps.max(r.slippage_bps);
            buy_slippage.push(SlippageEntry {
                trade_xlm:    size,
                slippage_bps: r.slippage_bps,
                fully_filled: r.fully_filled,
            });
        }
        if let Some(r) = slippage::compute_sell_slippage(ob, size) {
            max_slippage_bps = max_slippage_bps.max(r.slippage_bps);
            sell_slippage.push(SlippageEntry {
                trade_xlm:    size,
                slippage_bps: r.slippage_bps,
                fully_filled: r.fully_filled,
            });
        }
    }

    let low_liquidity_alert = ask_depth < cfg.min_depth_xlm || bid_depth < cfg.min_depth_xlm;
    let high_slippage_alert = max_slippage_bps > cfg.max_slippage_alert_bps as f64;

    Some(LiquiditySnapshot {
        pair: label,
        timestamp: now_iso(),
        bid_depth_xlm: bid_depth,
        ask_depth_xlm: ask_depth,
        spread_bps:   spread,
        mid_price:    mid,
        best_bid,
        best_ask,
        buy_slippage,
        sell_slippage,
        low_liquidity_alert,
        high_slippage_alert,
    })
}

// ── Logging & publishing ──────────────────────────────────────────────────────

fn log_snapshot(snap: &LiquiditySnapshot, verbose: bool) {
    if snap.low_liquidity_alert || snap.high_slippage_alert {
        warn!(
            pair    = %snap.pair,
            bid_xlm = snap.bid_depth_xlm,
            ask_xlm = snap.ask_depth_xlm,
            spread  = snap.spread_bps,
            low_liq = snap.low_liquidity_alert,
            hi_slip = snap.high_slippage_alert,
            "⚠️  Liquidity alert"
        );
    } else if verbose {
        debug!(
            pair       = %snap.pair,
            mid        = snap.mid_price,
            bid_depth  = snap.bid_depth_xlm,
            ask_depth  = snap.ask_depth_xlm,
            spread_bps = snap.spread_bps,
            "Liquidity snapshot"
        );
    }
}

async fn publish_snapshot(kafka: &KafkaPublisher, snap: &LiquiditySnapshot) {
    // Publish to marketplace activity feed for dashboard display.
    kafka.publish_chain_event(&ChainEvent {
        event_type: "liquidity_snapshot".into(),
        tx_hash:    "n/a".into(),
        ledger:     0,
        account:    snap.pair.clone(),
        details:    serde_json::to_value(snap).unwrap_or_default(),
        created_at: snap.timestamp.clone(),
    }).await;

    // If there are active alerts, also publish as an agent action for urgency.
    if snap.low_liquidity_alert || snap.high_slippage_alert {
        kafka.publish_action(&AgentActionEvent {
            agent_type:   "liquidity_slippage_tracker".into(),
            agent_wallet: "".into(),
            action:       if snap.low_liquidity_alert { "low_liquidity_alert" } else { "high_slippage_alert" }.into(),
            asset_pair:   Some(snap.pair.clone()),
            tx_hash:      None,
            profit_xlm:   None,
            latency_ms:   None,
            created_at:   snap.timestamp.clone(),
        }).await;
    }
}
