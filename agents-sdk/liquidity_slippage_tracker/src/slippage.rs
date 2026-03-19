//! Slippage calculation: price impact for a given trade size on a real order book.
//!
//! ## Model
//! We simulate a market-order walk through the order book.  For a buy of
//! size S in XLM, we consume ask levels from best to worst until the
//! cumulative XLM spent equals S.  The **effective price** is the
//! weighted-average fill price; **slippage** is:
//!
//! ```text
//! slippage_bps = (effective_price / best_ask_price - 1) × 10_000
//! ```
//!
//! A negative slippage is impossible for a buy (you always pay ≥ best ask).

use common::{OrderBook, OrderBookLevel};
use rust_decimal::prelude::*;
use rust_decimal::Decimal;

/// Result of a slippage simulation.
#[derive(Debug, Clone)]
pub struct SlippageResult {
    /// Nominal size of the simulated trade in XLM.
    pub trade_size_xlm: f64,
    /// Best ask/bid price (reference price before impact).
    pub reference_price: f64,
    /// Effective (volume-weighted) fill price.
    pub effective_price: f64,
    /// Price impact in basis points (positive = adverse).
    pub slippage_bps: f64,
    /// Whether the order book had enough depth to fill the full trade.
    pub fully_filled: bool,
    /// Amount actually filled (may be < trade_size_xlm if book is thin).
    pub filled_xlm: f64,
}

/// Compute slippage for a **buy** of `trade_xlm` XLM on the given order book.
///
/// We walk the ask side; each ask level is expressed as:
/// - `price`  = XLM per 1 unit of the asset
/// - `amount` = units of the asset available at this price
pub fn compute_buy_slippage(ob: &OrderBook, trade_xlm: f64) -> Option<SlippageResult> {
    if ob.asks.is_empty() { return None; }

    let best_ask = parse_level_price(&ob.asks[0])?;
    let trade_xlm_d = Decimal::from_f64(trade_xlm)?;
    let mut xlm_remaining = trade_xlm_d;
    let mut total_xlm_spent = Decimal::ZERO;
    let mut total_asset_bought = Decimal::ZERO;

    for level in &ob.asks {
        if xlm_remaining <= Decimal::ZERO { break; }

        let level_price  = parse_level_price(level)?;
        let level_amount = parse_level_amount(level)?; // units of asset
        let level_xlm    = level_amount * level_price; // XLM value of this level

        let xlm_to_consume = xlm_remaining.min(level_xlm);
        let asset_bought   = xlm_to_consume / level_price;

        total_xlm_spent    += xlm_to_consume;
        total_asset_bought += asset_bought;
        xlm_remaining      -= xlm_to_consume;
    }

    let fully_filled  = xlm_remaining == Decimal::ZERO;
    let filled_xlm    = total_xlm_spent.to_f64()?;

    // Effective price = total XLM spent / total asset units received
    let effective_price = if total_asset_bought > Decimal::ZERO {
        (total_xlm_spent / total_asset_bought).to_f64()?
    } else {
        return None;
    };

    let ref_price = best_ask.to_f64()?;
    let slippage_bps = (effective_price / ref_price - 1.0) * 10_000.0;

    Some(SlippageResult {
        trade_size_xlm:  trade_xlm,
        reference_price: ref_price,
        effective_price,
        slippage_bps:    slippage_bps.max(0.0),
        fully_filled,
        filled_xlm,
    })
}

/// Compute slippage for a **sell** of `trade_xlm` XLM equivalent on the bid side.
pub fn compute_sell_slippage(ob: &OrderBook, trade_xlm: f64) -> Option<SlippageResult> {
    if ob.bids.is_empty() { return None; }

    let best_bid = parse_level_price(&ob.bids[0])?;
    let trade_xlm_d = Decimal::from_f64(trade_xlm)?;
    let mut xlm_remaining = trade_xlm_d;
    let mut total_xlm_received = Decimal::ZERO;
    let mut total_asset_sold   = Decimal::ZERO;

    for level in &ob.bids {
        if xlm_remaining <= Decimal::ZERO { break; }

        let level_price  = parse_level_price(level)?;
        let level_amount = parse_level_amount(level)?;
        let level_xlm    = level_amount * level_price;

        let xlm_to_fill  = xlm_remaining.min(level_xlm);
        let asset_sold   = xlm_to_fill / level_price;

        total_xlm_received += xlm_to_fill;
        total_asset_sold   += asset_sold;
        xlm_remaining      -= xlm_to_fill;
    }

    let fully_filled  = xlm_remaining == Decimal::ZERO;
    let filled_xlm    = total_xlm_received.to_f64()?;

    let effective_price = if total_asset_sold > Decimal::ZERO {
        (total_xlm_received / total_asset_sold).to_f64()?
    } else {
        return None;
    };

    let ref_price = best_bid.to_f64()?;
    // For a sell, adverse slippage = effective_price < reference_price
    let slippage_bps = (1.0 - effective_price / ref_price) * 10_000.0;

    Some(SlippageResult {
        trade_size_xlm:  trade_xlm,
        reference_price: ref_price,
        effective_price,
        slippage_bps:    slippage_bps.max(0.0),
        fully_filled,
        filled_xlm,
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_level_price(level: &OrderBookLevel) -> Option<Decimal> {
    Decimal::from_str(&level.price).ok()
}

fn parse_level_amount(level: &OrderBookLevel) -> Option<Decimal> {
    Decimal::from_str(&level.amount).ok()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use common::OrderBook;

    fn make_book(asks: &[(&str, &str)], bids: &[(&str, &str)]) -> OrderBook {
        OrderBook {
            asks: asks.iter().map(|(p, a)| common::OrderBookLevel {
                price: p.to_string(), amount: a.to_string()
            }).collect(),
            bids: bids.iter().map(|(p, a)| common::OrderBookLevel {
                price: p.to_string(), amount: a.to_string()
            }).collect(),
        }
    }

    #[test]
    fn zero_slippage_single_level() {
        // 100 XLM available at price 1.0 → buying 10 XLM should have zero slippage
        let ob = make_book(&[("1.0", "100")], &[]);
        let result = compute_buy_slippage(&ob, 10.0).unwrap();
        assert!((result.slippage_bps - 0.0).abs() < 0.01);
        assert!(result.fully_filled);
    }

    #[test]
    fn slippage_multi_level() {
        // 5 XLM at 1.0, next 5 XLM at 1.05 — buying 10 XLM
        let ob = make_book(&[("1.0", "5"), ("1.05", "100")], &[]);
        let result = compute_buy_slippage(&ob, 10.0).unwrap();
        // Effective price = (5 + 5*1.05) / 10 = 1.025 → slippage = 250 bps
        assert!(result.slippage_bps > 0.0);
        assert!((result.slippage_bps - 250.0).abs() < 1.0);
    }

    #[test]
    fn partial_fill_when_insufficient_depth() {
        let ob = make_book(&[("1.0", "5")], &[]);
        let result = compute_buy_slippage(&ob, 100.0).unwrap();
        assert!(!result.fully_filled);
        assert!((result.filled_xlm - 5.0).abs() < 0.01);
    }
}
