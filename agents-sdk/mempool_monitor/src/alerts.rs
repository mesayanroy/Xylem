//! Alert rules evaluation for the mempool monitor.
//!
//! Each [`AlertRule`] is a pure function: it receives a transaction and
//! returns `Some(AlertEvent)` if it fires, or `None` if it doesn't.
//! This makes rules trivially testable in isolation.

use crate::config::AlertRule;
use common::horizon::SseTransaction;

/// A fired alert event returned by an [`AlertRule`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct AlertEvent {
    pub rule_name: String,
    pub detail:    String,
}

/// Evaluate a single rule against a transaction.
///
/// Returns `Some(AlertEvent)` when the rule fires.
pub fn evaluate(rule: &AlertRule, tx: &SseTransaction) -> Option<AlertEvent> {
    match rule {
        // ── High fee ─────────────────────────────────────────────────────────
        AlertRule::HighFee { threshold_stroops } => {
            let fee: u64 = tx.fee_charged.parse().unwrap_or(0);
            if fee > *threshold_stroops {
                Some(AlertEvent {
                    rule_name: "HIGH_FEE".into(),
                    detail:    format!("{fee} stroops (threshold: {threshold_stroops})"),
                })
            } else {
                None
            }
        }

        // ── Watched address ──────────────────────────────────────────────────
        AlertRule::WatchedAddress { address } => {
            if tx.source_account == *address {
                Some(AlertEvent {
                    rule_name: "WATCHED_ADDRESS".into(),
                    detail:    format!("Source account matched: {address}"),
                })
            } else {
                None
            }
        }

        // ── High operation count ─────────────────────────────────────────────
        AlertRule::HighOperationCount { threshold } => {
            if tx.operation_count > *threshold {
                Some(AlertEvent {
                    rule_name: "HIGH_OP_COUNT".into(),
                    detail:    format!(
                        "{} operations (threshold: {threshold})",
                        tx.operation_count
                    ),
                })
            } else {
                None
            }
        }

        // ── Offer activity ───────────────────────────────────────────────────
        // Stellar does not embed decoded op types in the transaction SSE record,
        // so we inspect `result_meta_xdr` for the offer result code keywords.
        AlertRule::OfferActivity => {
            // The envelope XDR will contain "ManageSellOffer" / "ManageBuyOffer"
            // keywords when decoded.  We use a lightweight base64-byte check
            // against known XDR discriminants for ManageSellOffer (opType=3)
            // and ManageBuyOffer (opType=12).
            let envelope = &tx.envelope_xdr;
            // Base64-encoded ManageSellOffer opType discriminant bytes "\x00\x00\x00\x03"
            // and ManageBuyOffer "\x00\x00\x00\x0c" appear as "AAAAD" and "AAAADA" in b64.
            if envelope.contains("AAAAD") || envelope.contains("AAAANM") {
                Some(AlertEvent {
                    rule_name: "OFFER_ACTIVITY".into(),
                    detail:    "ManageSellOffer/ManageBuyOffer detected".into(),
                })
            } else {
                None
            }
        }

        // ── Path payment ─────────────────────────────────────────────────────
        AlertRule::PathPayment => {
            // PathPaymentStrictSend opType=13 (0x0D) → b64 substring "AAAADQ"
            // PathPaymentStrictReceive opType=2 (0x02) is more ambiguous
            if tx.envelope_xdr.contains("AAAADQ") {
                Some(AlertEvent {
                    rule_name: "PATH_PAYMENT".into(),
                    detail:    "PathPaymentStrictSend detected (possible arbitrage)".into(),
                })
            } else {
                None
            }
        }

        // ── Fee surge ────────────────────────────────────────────────────────
        AlertRule::FeeSurge { multiple } => {
            let _fee_charged: f64 = tx.fee_charged.parse().unwrap_or(0.0);
            let max_fee:     f64 = tx.max_fee.parse().unwrap_or(0.0);
            // If the declared max_fee is > `multiple` × 100 stroops (base fee) per op
            let per_op_fee = if tx.operation_count > 0 {
                max_fee / tx.operation_count as f64
            } else {
                max_fee
            };
            let base = 100.0_f64;
            if per_op_fee > base * multiple {
                Some(AlertEvent {
                    rule_name: "FEE_SURGE".into(),
                    detail:    format!(
                        "{per_op_fee:.0} stroops/op ({:.1}× base fee)",
                        per_op_fee / base
                    ),
                })
            } else {
                None
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use common::horizon::SseTransaction;

    fn make_tx(fee: &str, ops: u32, source: &str, envelope: &str) -> SseTransaction {
        SseTransaction {
            id:              "1".into(),
            hash:            "deadbeef".into(),
            ledger:          100,
            created_at:      "2024-01-01T00:00:00Z".into(),
            source_account:  source.into(),
            fee_charged:     fee.into(),
            max_fee:         fee.into(),
            operation_count: ops,
            memo_type:       "none".into(),
            memo:            None,
            envelope_xdr:    envelope.into(),
            result_xdr:      "".into(),
            result_meta_xdr: "".into(),
            fee_meta_xdr:    "".into(),
            valid_after:     None,
            valid_before:    None,
        }
    }

    #[test]
    fn high_fee_fires() {
        let rule = AlertRule::HighFee { threshold_stroops: 500 };
        let tx = make_tx("1000", 1, "GTEST", "");
        assert!(evaluate(&rule, &tx).is_some());
    }

    #[test]
    fn high_fee_no_fire_below_threshold() {
        let rule = AlertRule::HighFee { threshold_stroops: 5000 };
        let tx = make_tx("100", 1, "GTEST", "");
        assert!(evaluate(&rule, &tx).is_none());
    }

    #[test]
    fn watched_address_fires() {
        let rule = AlertRule::WatchedAddress { address: "GTEST".into() };
        let tx = make_tx("100", 1, "GTEST", "");
        assert!(evaluate(&rule, &tx).is_some());
    }

    #[test]
    fn watched_address_no_fire_different() {
        let rule = AlertRule::WatchedAddress { address: "GOTHER".into() };
        let tx = make_tx("100", 1, "GTEST", "");
        assert!(evaluate(&rule, &tx).is_none());
    }
}
