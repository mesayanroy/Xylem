//! Trading strategy execution loop.
//!
//! Dispatches to the appropriate strategy handler based on [`TradingBotConfig::active_strategy`].
//! Each strategy runs in a loop, polling the order book and managing open offers.

use crate::{
    config::{Strategy, TradingBotConfig},
    orders,
};
use anyhow::Result;
use common::{AgentActionEvent, HorizonClient, KafkaPublisher, Keypair};
use rust_decimal::prelude::ToPrimitive;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

pub async fn run(
    cfg:     &TradingBotConfig,
    horizon: &HorizonClient,
    keypair: &Keypair,
    kafka:   &KafkaPublisher,
) -> Result<()> {
    match cfg.active_strategy {
        Strategy::Buy   => run_buy(cfg, horizon, keypair, kafka).await,
        Strategy::Sell  => run_sell(cfg, horizon, keypair, kafka).await,
        Strategy::Short => run_short(cfg, horizon, keypair, kafka).await,
        Strategy::Grid  => run_grid(cfg, horizon, keypair, kafka).await,
        Strategy::Dca   => run_dca(cfg, horizon, keypair, kafka).await,
    }
}

// ── Buy strategy ──────────────────────────────────────────────────────────────

/// Place a limit or market buy order.
///
/// If `limit_price` is set, places a passive GTC ManageBuyOffer.
/// If not, uses the current best ask for immediate fill.
async fn run_buy(
    cfg:     &TradingBotConfig,
    horizon: &HorizonClient,
    keypair: &Keypair,
    kafka:   &KafkaPublisher,
) -> Result<()> {
    let ob = horizon.get_order_book(&cfg.trade_asset, &common::Asset::native(), 5).await?;

    // Use limit price if set, otherwise take the best ask (market order).
    let price = match cfg.limit_price {
        Some(p) => p,
        None => ob.best_ask()
            .and_then(|p| p.to_f64())
            .ok_or_else(|| anyhow::anyhow!("No ask price available"))?,
    };

    info!(
        strategy = "BUY",
        asset    = %cfg.trade_asset,
        amount   = cfg.amount_xlm,
        price    = price,
        dry_run  = cfg.dry_run,
        "Placing buy order"
    );

    if cfg.dry_run {
        info!("[DRY RUN] Would buy {} XLM of {} at {:.7}", cfg.amount_xlm, cfg.trade_asset, price);
        return Ok(());
    }

    let hash = orders::place_buy_offer(cfg, horizon, keypair, price).await?;
    info!(tx = %hash, "Buy order placed");
    kafka.publish_action(&AgentActionEvent {
        agent_type:   "trading_bot".into(),
        agent_wallet: keypair.public_key.clone(),
        action:       "Buy_order".into(),
        asset_pair:   Some(format!("{}", cfg.trade_asset)),
        tx_hash:      Some(hash),
        profit_xlm:   None,
        latency_ms:   None,
        created_at:   common::pubsub::now_iso(),
    }).await;
    Ok(())
}

// ── Sell strategy ─────────────────────────────────────────────────────────────

async fn run_sell(
    cfg:     &TradingBotConfig,
    horizon: &HorizonClient,
    keypair: &Keypair,
    kafka:   &KafkaPublisher,
) -> Result<()> {
    let ob = horizon.get_order_book(&cfg.trade_asset, &common::Asset::native(), 5).await?;

    let price = match cfg.limit_price {
        Some(p) => p,
        None => ob.best_bid()
            .and_then(|p| p.to_f64())
            .ok_or_else(|| anyhow::anyhow!("No bid price available"))?,
    };

    info!(
        strategy = "SELL",
        asset    = %cfg.trade_asset,
        amount   = cfg.amount_xlm,
        price    = price,
        "Placing sell order"
    );

    if cfg.dry_run {
        info!("[DRY RUN] Would sell {} of {} at {:.7}", cfg.amount_xlm, cfg.trade_asset, price);
        return Ok(());
    }

    let hash = orders::place_sell_offer(cfg, horizon, keypair, price).await?;
    info!(tx = %hash, "Sell order placed");
    kafka.publish_action(&AgentActionEvent {
        agent_type:   "trading_bot".into(),
        agent_wallet: keypair.public_key.clone(),
        action:       "Sell_order".into(),
        asset_pair:   Some(format!("{}", cfg.trade_asset)),
        tx_hash:      Some(hash),
        profit_xlm:   None,
        latency_ms:   None,
        created_at:   common::pubsub::now_iso(),
    }).await;
    Ok(())
}

// ── Short strategy ────────────────────────────────────────────────────────────

/// Synthetic short: sell the asset now, monitor for price drop, repurchase.
///
/// Implementation:
/// 1. Place a sell offer at current market price (or limit).
/// 2. Poll until price drops below `stop_loss` (or `take_profit`).
/// 3. Place a buy offer at the lower price to close the position.
async fn run_short(
    cfg:     &TradingBotConfig,
    horizon: &HorizonClient,
    keypair: &Keypair,
    _kafka:   &KafkaPublisher,
) -> Result<()> {
    let ob = horizon.get_order_book(&cfg.trade_asset, &common::Asset::native(), 5).await?;

    let entry_price = match cfg.limit_price {
        Some(p) => p,
        None => ob.best_bid()
            .and_then(|p| p.to_f64())
            .ok_or_else(|| anyhow::anyhow!("No bid available for short entry"))?,
    };

    info!(
        strategy    = "SHORT",
        entry_price = entry_price,
        stop_loss   = ?cfg.trigger.stop_loss,
        take_profit = ?cfg.trigger.take_profit,
        "Opening short position"
    );

    if !cfg.dry_run {
        let hash = orders::place_sell_offer(cfg, horizon, keypair, entry_price).await?;
        info!(tx = %hash, entry_price, "Short entry placed");
    }

    // Poll for cover condition.
    let poll = Duration::from_millis(cfg.poll_interval_ms);
    loop {
        sleep(poll).await;

        match horizon.get_order_book(&cfg.trade_asset, &common::Asset::native(), 1).await {
            Err(e) => warn!("Price poll error: {e}"),
            Ok(ob) => {
                let current = ob.best_ask().and_then(|p| p.to_f64());

                if let Some(price) = current {
                    let cover = matches!(
                        (cfg.trigger.stop_loss, cfg.trigger.take_profit),
                        (Some(sl), _) if price >= sl  // stop-loss triggered (price rose)
                    ) || matches!(
                        (cfg.trigger.stop_loss, cfg.trigger.take_profit),
                        (_, Some(tp)) if price <= tp  // take-profit triggered (price fell)
                    );

                    if cover {
                        info!(current_price = price, "Cover condition met — closing short");
                        if !cfg.dry_run {
                            let hash = orders::place_buy_offer(cfg, horizon, keypair, price).await?;
                            info!(tx = %hash, "Short covered");
                        } else {
                            info!("[DRY RUN] Would cover short at {price:.7}");
                        }
                        return Ok(());
                    } else {
                        info!(current_price = price, entry_price, "Monitoring short position");
                    }
                }
            }
        }
    }
}

// ── Grid strategy ─────────────────────────────────────────────────────────────

/// Grid trading: place `grid_levels` buy offers below mid-price and
/// `grid_levels` sell offers above mid-price at `grid_spacing` intervals.
///
/// This passively captures spread as price oscillates within the grid.
async fn run_grid(
    cfg:     &TradingBotConfig,
    horizon: &HorizonClient,
    keypair: &Keypair,
    _kafka:   &KafkaPublisher,
) -> Result<()> {
    let ob = horizon.get_order_book(&cfg.trade_asset, &common::Asset::native(), 3).await?;

    let mid = ob.mid_price()
        .and_then(|p| p.to_f64())
        .ok_or_else(|| anyhow::anyhow!("Cannot determine mid-price for grid"))?;

    let level_amount = cfg.amount_xlm / (cfg.grid_levels as f64 * 2.0);

    info!(
        mid_price    = mid,
        levels       = cfg.grid_levels,
        spacing_pct  = cfg.grid_spacing * 100.0,
        amount_level = level_amount,
        "Setting up grid"
    );

    if cfg.dry_run {
        for i in 1..=cfg.grid_levels {
            let buy_price  = mid * (1.0 - cfg.grid_spacing * i as f64);
            let sell_price = mid * (1.0 + cfg.grid_spacing * i as f64);
            info!("[DRY RUN] Grid L{i}: buy @ {buy_price:.7}, sell @ {sell_price:.7}");
        }
        return Ok(());
    }

    // Place all grid orders in sequence (could be parallelised but risks
    // sequence number collisions — safer to batch into one transaction per level).
    for i in 1..=(cfg.grid_levels as i64) {
        let buy_price  = mid * (1.0 - cfg.grid_spacing * i as f64);
        let sell_price = mid * (1.0 + cfg.grid_spacing * i as f64);

        // Create a temporary config copy with the adjusted amount.
        let mut level_cfg = cfg.clone();
        level_cfg.amount_xlm = level_amount;
        level_cfg.existing_offer_id = 0;

        let buy_hash  = orders::place_buy_offer(&level_cfg, horizon, keypair, buy_price).await?;
        let sell_hash = orders::place_sell_offer(&level_cfg, horizon, keypair, sell_price).await?;

        info!(
            level      = i,
            buy_tx     = %buy_hash,
            sell_tx    = %sell_hash,
            buy_price  = buy_price,
            sell_price = sell_price,
            "Grid level placed"
        );
    }

    info!("Grid setup complete — monitoring passively");
    // The offers are now live on-chain; no active monitoring needed.
    // In production you'd watch for fills and rebalance here.
    Ok(())
}

// ── DCA strategy ─────────────────────────────────────────────────────────────

/// Dollar-cost averaging: buy a fixed amount every `dca_interval_secs`.
async fn run_dca(
    cfg:     &TradingBotConfig,
    horizon: &HorizonClient,
    keypair: &Keypair,
    _kafka:   &KafkaPublisher,
) -> Result<()> {
    let interval = Duration::from_secs(cfg.dca_interval_secs);
    info!(
        asset         = %cfg.trade_asset,
        amount_xlm    = cfg.amount_xlm,
        interval_secs = cfg.dca_interval_secs,
        "DCA strategy started"
    );

    loop {
        match horizon.get_order_book(&cfg.trade_asset, &common::Asset::native(), 1).await {
            Err(e) => warn!("DCA price fetch error: {e}"),
            Ok(ob) => {
                if let Some(ask) = ob.best_ask().and_then(|p| p.to_f64()) {
                    info!(price = ask, amount = cfg.amount_xlm, "DCA buy");
                    if cfg.dry_run {
                        info!("[DRY RUN] DCA: would buy {} XLM worth at {ask:.7}", cfg.amount_xlm);
                    } else {
                        match orders::place_buy_offer(cfg, horizon, keypair, ask).await {
                            Ok(h)  => info!(tx = %h, "DCA buy executed"),
                            Err(e) => warn!("DCA buy failed: {e:#}"),
                        }
                    }
                }
            }
        }
        sleep(interval).await;
    }
}
