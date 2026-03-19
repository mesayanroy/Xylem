//! Stellar transaction envelope builder.
//!
//! Constructs base64-encoded XDR transaction envelopes by encoding the
//! canonical Stellar transaction structure manually.  Each operation is
//! serialised according to the Stellar XDR specification (XDR RFC 4506).
//!
//! ## Gas / fee optimisation
//! - Operations are batched: up to 100 operations per transaction envelope.
//! - The fee is computed as `base_fee_stroops × operation_count`.
//! - Fee-bump transactions (`FeeBumpTransaction`) wrap pre-signed inner
//!   transactions to increase their fee without requiring re-signing by the
//!   original source account.

use crate::horizon::{Asset, HorizonClient};
use crate::wallet::Keypair;
use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use std::str::FromStr;
use tracing::debug;

// ── XDR primitive helpers ─────────────────────────────────────────────────────
//
// Stellar XDR is big-endian, fixed-size or length-prefixed.

fn xdr_u32(n: u32) -> [u8; 4] { n.to_be_bytes() }
fn xdr_i32(n: i32) -> [u8; 4] { n.to_be_bytes() }
fn xdr_u64(n: u64) -> [u8; 8] { n.to_be_bytes() }
fn xdr_i64(n: i64) -> [u8; 8] { n.to_be_bytes() }

/// XDR opaque bytes: 4-byte big-endian length followed by the bytes,
/// padded to a 4-byte boundary.
fn xdr_opaque(data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(data);
    let pad = (4 - (data.len() % 4)) % 4;
    out.extend(std::iter::repeat(0u8).take(pad));
    out
}

/// XDR variable-length string (same encoding as opaque).
fn xdr_string(s: &str) -> Vec<u8> { xdr_opaque(s.as_bytes()) }

// ── Asset XDR ─────────────────────────────────────────────────────────────────

fn asset_xdr(asset: &Asset) -> Vec<u8> {
    match asset {
        Asset::Native => xdr_u32(0).to_vec(), // ASSET_TYPE_NATIVE = 0
        Asset::Credit { asset_code, asset_issuer } => {
            let mut out = Vec::new();
            if asset_code.len() <= 4 {
                out.extend_from_slice(&xdr_u32(1)); // ASSET_TYPE_CREDIT_ALPHANUM4
                let mut code = [0u8; 4];
                code[..asset_code.len()].copy_from_slice(asset_code.as_bytes());
                out.extend_from_slice(&code);
            } else {
                out.extend_from_slice(&xdr_u32(2)); // ASSET_TYPE_CREDIT_ALPHANUM12
                let mut code = [0u8; 12];
                code[..asset_code.len().min(12)].copy_from_slice(&asset_code.as_bytes()[..asset_code.len().min(12)]);
                out.extend_from_slice(&code);
            }
            // Account ID (Ed25519 public key)
            out.extend_from_slice(&account_id_xdr(asset_issuer));
            out
        }
    }
}

fn account_id_xdr(address: &str) -> Vec<u8> {
    // Decode G... Strkey to raw 32-byte public key
    let raw = crate::wallet::decode_raw_public_key(address).unwrap_or([0u8; 32]);
    let mut out = Vec::new();
    out.extend_from_slice(&xdr_u32(0)); // PUBLIC_KEY_TYPE_ED25519 = 0
    out.extend_from_slice(&raw);
    out
}

// ── Operation types ───────────────────────────────────────────────────────────

/// Supported Stellar operations.
#[derive(Debug, Clone)]
pub enum OperationBody {
    /// ManageSellOffer: place or cancel a sell order on the SDEX.
    ManageSellOffer {
        selling:  Asset,
        buying:   Asset,
        /// Amount in stroops (0 = cancel offer).
        amount:   i64,
        /// Price as N/D fraction (selling/buying).
        price_n:  i32,
        price_d:  i32,
        offer_id: i64, // 0 = new offer
    },
    /// ManageBuyOffer: place or cancel a buy order on the SDEX.
    ManageBuyOffer {
        selling:    Asset,
        buying:     Asset,
        buy_amount: i64,
        price_n:    i32,
        price_d:    i32,
        offer_id:   i64,
    },
    /// PathPaymentStrictSend: send exact amount, receive at least `dest_min`.
    PathPaymentStrictSend {
        send_asset:   Asset,
        send_amount:  i64,
        destination:  String,
        dest_asset:   Asset,
        dest_min:     i64,
        path:         Vec<Asset>,
    },
    /// FeeBumpTransaction inner: bump fee of an existing transaction.
    Payment {
        destination: String,
        asset:       Asset,
        amount:      i64,
    },
}

fn op_xdr(op: &OperationBody) -> Vec<u8> {
    let mut out = Vec::new();
    // No source account on operation (use transaction-level source)
    out.extend_from_slice(&xdr_u32(0)); // sourceAccount present = 0 (absent)

    match op {
        OperationBody::ManageSellOffer {
            selling, buying, amount, price_n, price_d, offer_id,
        } => {
            out.extend_from_slice(&xdr_u32(3)); // MANAGE_SELL_OFFER
            out.extend_from_slice(&asset_xdr(selling));
            out.extend_from_slice(&asset_xdr(buying));
            out.extend_from_slice(&xdr_i64(*amount));
            out.extend_from_slice(&xdr_i32(*price_n));
            out.extend_from_slice(&xdr_i32(*price_d));
            out.extend_from_slice(&xdr_i64(*offer_id));
        }
        OperationBody::ManageBuyOffer {
            selling, buying, buy_amount, price_n, price_d, offer_id,
        } => {
            out.extend_from_slice(&xdr_u32(12)); // MANAGE_BUY_OFFER
            out.extend_from_slice(&asset_xdr(selling));
            out.extend_from_slice(&asset_xdr(buying));
            out.extend_from_slice(&xdr_i64(*buy_amount));
            out.extend_from_slice(&xdr_i32(*price_n));
            out.extend_from_slice(&xdr_i32(*price_d));
            out.extend_from_slice(&xdr_i64(*offer_id));
        }
        OperationBody::PathPaymentStrictSend {
            send_asset, send_amount, destination, dest_asset, dest_min, path,
        } => {
            out.extend_from_slice(&xdr_u32(13)); // PATH_PAYMENT_STRICT_SEND
            out.extend_from_slice(&asset_xdr(send_asset));
            out.extend_from_slice(&xdr_i64(*send_amount));
            out.extend_from_slice(&account_id_xdr(destination));
            out.extend_from_slice(&asset_xdr(dest_asset));
            out.extend_from_slice(&xdr_i64(*dest_min));
            out.extend_from_slice(&xdr_u32(path.len() as u32));
            for a in path { out.extend_from_slice(&asset_xdr(a)); }
        }
        OperationBody::Payment { destination, asset, amount } => {
            out.extend_from_slice(&xdr_u32(1)); // PAYMENT
            out.extend_from_slice(&account_id_xdr(destination));
            out.extend_from_slice(&asset_xdr(asset));
            out.extend_from_slice(&xdr_i64(*amount));
        }
    }
    out
}

// ── Transaction builder ───────────────────────────────────────────────────────

/// Stellar `TransactionV1` builder.
///
/// Constructs the minimal XDR needed for a valid signed `TransactionEnvelope`.
///
/// ```text
/// TransactionEnvelope
///   └─ TransactionV1Envelope
///        ├─ Transaction
///        │    ├─ sourceAccount (MuxedAccount / Ed25519 public key)
///        │    ├─ fee           (u32 stroops)
///        │    ├─ seqNum        (i64)
///        │    ├─ preconditions (timebounds)
///        │    ├─ memo          (MemoNone)
///        │    └─ operations[]
///        └─ signatures[]
/// ```
pub struct TransactionBuilder {
    source:      String,
    sequence:    i64,
    base_fee:    u32,
    timebounds:  Option<(u64, u64)>, // (min_time, max_time) in Unix seconds
    operations:  Vec<OperationBody>,
    memo_text:   Option<String>,
}

impl TransactionBuilder {
    pub fn new(source: impl Into<String>, sequence: i64, base_fee_stroops: u32) -> Self {
        Self {
            source:     source.into(),
            sequence,
            base_fee:   base_fee_stroops,
            timebounds: None,
            operations: Vec::new(),
            memo_text:  None,
        }
    }

    /// Convenience: fetch sequence number and current fee from Horizon.
    pub async fn from_horizon(horizon: &HorizonClient, source: &str) -> Result<Self> {
        let acct = horizon.get_account(source).await?;
        let fee  = horizon.get_base_fee_stroops().await?;
        Ok(Self::new(source, acct.sequence_number() + 1, fee.max(100)))
    }

    /// Add a [`TimeBounds`] condition (ISO-8601 seconds).
    ///
    /// Setting `max_time` prevents a transaction from being submitted after
    /// a deadline — critical for MEV and arbitrage bots.
    pub fn with_timebounds(mut self, min_secs: u64, max_secs: u64) -> Self {
        self.timebounds = Some((min_secs, max_secs));
        self
    }

    /// Add an optional text memo (≤ 28 bytes).
    pub fn with_memo(mut self, memo: impl Into<String>) -> Self {
        self.memo_text = Some(memo.into());
        self
    }

    /// Append an operation.
    pub fn add_op(mut self, op: OperationBody) -> Self {
        self.operations.push(op);
        self
    }

    /// Build the unsigned transaction XDR bytes.
    ///
    /// Returns the raw bytes that must be signed (the `tx_xdr` passed to
    /// [`Keypair::sign_transaction`]).
    pub fn build_tx_xdr(&self) -> Result<Vec<u8>> {
        if self.operations.is_empty() {
            bail!("Transaction must have at least one operation");
        }
        if self.operations.len() > 100 {
            bail!("Stellar transactions support at most 100 operations");
        }

        let fee = self.base_fee * self.operations.len() as u32;
        let mut tx = Vec::new();

        // sourceAccount: MuxedAccount (KEY_TYPE_ED25519 = 0)
        tx.extend_from_slice(&xdr_u32(0));
        tx.extend_from_slice(&account_id_xdr_inner(&self.source));

        tx.extend_from_slice(&xdr_u32(fee));
        tx.extend_from_slice(&xdr_i64(self.sequence));

        // preconditions: PRECOND_TIME = 2 if timebounds set, else PRECOND_NONE = 0
        match self.timebounds {
            None => {
                tx.extend_from_slice(&xdr_u32(0)); // PRECOND_NONE
            }
            Some((min, max)) => {
                tx.extend_from_slice(&xdr_u32(1)); // PRECOND_TIME
                tx.extend_from_slice(&xdr_u64(min));
                tx.extend_from_slice(&xdr_u64(max));
            }
        }

        // memo
        match &self.memo_text {
            None => tx.extend_from_slice(&xdr_u32(0)), // MEMO_NONE
            Some(text) => {
                tx.extend_from_slice(&xdr_u32(1)); // MEMO_TEXT
                tx.extend_from_slice(&xdr_string(text));
            }
        }

        // operations
        tx.extend_from_slice(&xdr_u32(self.operations.len() as u32));
        for op in &self.operations {
            tx.extend_from_slice(&op_xdr(op));
        }

        // ext = 0 (no extensions)
        tx.extend_from_slice(&xdr_u32(0));

        Ok(tx)
    }

    /// Build, sign, and serialise a complete `TransactionEnvelope` as base64.
    ///
    /// The returned string can be submitted directly to Horizon's
    /// `POST /transactions` endpoint (form field `tx`).
    pub fn sign_and_encode(
        &self,
        keypair: &Keypair,
        network_passphrase: &str,
    ) -> Result<String> {
        let tx_xdr = self.build_tx_xdr()?;
        let (hint, signature) = keypair.sign_transaction(network_passphrase, &tx_xdr);

        // TransactionEnvelope: ENVELOPE_TYPE_TX = 2
        let mut envelope = Vec::new();
        envelope.extend_from_slice(&xdr_u32(2)); // ENVELOPE_TYPE_TX

        // TransactionV1Envelope = {tx, signatures}
        envelope.extend_from_slice(&tx_xdr);

        // signatures: array of DecoratedSignature
        envelope.extend_from_slice(&xdr_u32(1)); // 1 signature
        // DecoratedSignature.hint (4 bytes)
        envelope.extend_from_slice(&hint);
        // DecoratedSignature.signature (opaque, 64 bytes)
        envelope.extend_from_slice(&xdr_opaque(&signature));

        debug!(ops = self.operations.len(), "Built signed transaction");
        Ok(B64.encode(&envelope))
    }

    /// Build and sign a **Fee Bump** transaction that wraps an existing
    /// signed inner transaction to increase its fee.
    ///
    /// `inner_tx_xdr_b64` is the base64-encoded `TransactionEnvelope` to bump.
    /// `new_fee_stroops` must be ≥ original_fee + 100 per operation.
    pub fn fee_bump(
        inner_tx_xdr_b64: &str,
        fee_source: &str,
        new_total_fee: u32,
        keypair: &Keypair,
        network_passphrase: &str,
    ) -> Result<String> {
        let inner_bytes = B64.decode(inner_tx_xdr_b64).map_err(|e| anyhow::anyhow!("{e}"))?;

        let mut envelope = Vec::new();
        // ENVELOPE_TYPE_TX_FEE_BUMP = 5
        envelope.extend_from_slice(&xdr_u32(5));

        // FeeBumpTransactionEnvelope.tx (FeeBumpTransaction)
        // feeSource (MuxedAccount)
        envelope.extend_from_slice(&xdr_u32(0));
        envelope.extend_from_slice(&account_id_xdr_inner(fee_source));
        // fee
        envelope.extend_from_slice(&xdr_i64(new_total_fee as i64));
        // innerTx (FeeBumpInnerTx, ENVELOPE_TYPE_TX = 2)
        envelope.extend_from_slice(&xdr_u32(2));
        // length-prefixed inner envelope bytes
        envelope.extend_from_slice(&xdr_opaque(&inner_bytes));
        // ext = 0
        envelope.extend_from_slice(&xdr_u32(0));

        // Sign the fee bump
        let (hint, sig) = keypair.sign_transaction(network_passphrase, &envelope);
        envelope.extend_from_slice(&xdr_u32(1));
        envelope.extend_from_slice(&hint);
        envelope.extend_from_slice(&xdr_opaque(&sig));

        Ok(B64.encode(&envelope))
    }
}

// Helper that returns the 32-byte key bytes for an account ID (no wrapping).
fn account_id_xdr_inner(address: &str) -> Vec<u8> {
    crate::wallet::decode_raw_public_key(address)
        .map(|b| b.to_vec())
        .unwrap_or_else(|_| vec![0u8; 32])
}

// ── Price helpers ─────────────────────────────────────────────────────────────

/// Convert a floating-point price to an integer (n, d) fraction suitable
/// for the XDR Price struct.
///
/// Uses a continued-fraction approximation capped at 2^31 − 1.
pub fn price_to_fraction(price: f64) -> (i32, i32) {
    if price <= 0.0 { return (1, 1); }
    // Simple rational approximation: multiply by 10_000_000 and reduce by GCD
    let scale = 10_000_000i64;
    let n = (price * scale as f64).round() as i64;
    let d = scale;
    let g = gcd(n.unsigned_abs(), d.unsigned_abs()) as i64;
    let n = (n / g).min(i32::MAX as i64) as i32;
    let d = (d / g).min(i32::MAX as i64) as i32;
    (n.max(1), d.max(1))
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 { let t = b; b = a % b; a = t; }
    a
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_fraction_round_trip() {
        let (n, d) = price_to_fraction(1.5);
        let recomputed = n as f64 / d as f64;
        assert!((recomputed - 1.5).abs() < 0.0001);
    }

    #[test]
    fn xdr_opaque_padding() {
        let bytes = xdr_opaque(b"hello"); // 5 bytes → padded to 8
        assert_eq!(bytes.len(), 4 + 8); // 4-byte length + 8 bytes
    }
}
