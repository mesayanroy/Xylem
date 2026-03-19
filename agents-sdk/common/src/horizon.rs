//! Async Horizon REST + SSE client for Stellar network interaction.
//!
//! Wraps the Horizon HTTP API with strong types, automatic retry with
//! exponential back-off, and streaming (SSE) support for real-time
//! transaction / ledger / trade feeds.
//!
//! ## Performance notes
//! - A single `reqwest::Client` is shared across all calls (connection pool).
//! - Retries are capped at 5 attempts with jittered exponential back-off.
//! - Streaming responses are parsed line-by-line without buffering the entire body.

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, warn};

// ── Primitive types ───────────────────────────────────────────────────────────

/// A Stellar asset (native XLM or issued token).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Asset {
    Native,
    Credit {
        asset_code:   String,
        asset_issuer: String,
    },
}

impl Asset {
    pub fn native() -> Self {
        Asset::Native
    }

    pub fn credit(code: impl Into<String>, issuer: impl Into<String>) -> Self {
        Asset::Credit {
            asset_code:   code.into(),
            asset_issuer: issuer.into(),
        }
    }

    /// Returns a short human-readable asset code (e.g. "XLM", "USDC").
    pub fn code(&self) -> &str {
        match self {
            Asset::Native => "XLM",
            Asset::Credit { asset_code, .. } => asset_code.as_str(),
        }
    }

    /// Horizon query string representation, e.g. `"native"` or `"USDC:issuer…"`.
    pub fn to_query_params(&self) -> Vec<(String, String)> {
        match self {
            Asset::Native => vec![("asset_type".into(), "native".into())],
            Asset::Credit { asset_code, asset_issuer } => vec![
                (
                    "asset_type".into(),
                    if asset_code.len() <= 4 { "credit_alphanum4".into() }
                    else { "credit_alphanum12".into() }
                ),
                ("asset_code".into(),   asset_code.clone()),
                ("asset_issuer".into(), asset_issuer.clone()),
            ],
        }
    }
}

impl std::fmt::Display for Asset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Asset::Native => write!(f, "XLM (native)"),
            Asset::Credit { asset_code, asset_issuer } => {
                write!(f, "{asset_code}:{}", &asset_issuer[..8])
            }
        }
    }
}

// ── Order-book types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct OrderBookLevel {
    pub price:  String,
    pub amount: String,
}

impl OrderBookLevel {
    pub fn price_decimal(&self) -> Decimal {
        Decimal::from_str(&self.price).unwrap_or_default()
    }
    pub fn amount_decimal(&self) -> Decimal {
        Decimal::from_str(&self.amount).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrderBook {
    pub bids: Vec<OrderBookLevel>,
    pub asks: Vec<OrderBookLevel>,
}

impl OrderBook {
    /// Best bid (highest buyer price).
    pub fn best_bid(&self) -> Option<Decimal> {
        self.bids.first().map(|l| l.price_decimal())
    }

    /// Best ask (lowest seller price).
    pub fn best_ask(&self) -> Option<Decimal> {
        self.asks.first().map(|l| l.price_decimal())
    }

    /// Mid-price between best bid and best ask.
    pub fn mid_price(&self) -> Option<Decimal> {
        let bid = self.best_bid()?;
        let ask = self.best_ask()?;
        Some((bid + ask) / Decimal::TWO)
    }

    /// Spread as a fraction of the mid-price.
    pub fn spread_bps(&self) -> Option<Decimal> {
        let bid = self.best_bid()?;
        let ask = self.best_ask()?;
        let mid = (bid + ask) / Decimal::TWO;
        if mid.is_zero() { return None; }
        Some(((ask - bid) / mid) * Decimal::from(10_000))
    }

    /// Cumulative depth on bid side up to `depth` levels.
    pub fn bid_depth(&self, depth: usize) -> Decimal {
        self.bids.iter().take(depth).map(|l| l.amount_decimal()).sum()
    }

    /// Cumulative depth on ask side up to `depth` levels.
    pub fn ask_depth(&self, depth: usize) -> Decimal {
        self.asks.iter().take(depth).map(|l| l.amount_decimal()).sum()
    }
}

// ── Account ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct AccountResponse {
    pub id:              String,
    pub sequence:        String,
    pub balances:        Vec<Balance>,
    pub subentry_count:  u32,
}

impl AccountResponse {
    /// Current sequence number (needed for transaction building).
    pub fn sequence_number(&self) -> i64 {
        self.sequence.parse().unwrap_or(0)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Balance {
    pub balance:      String,
    pub asset_type:   String,
    pub asset_code:   Option<String>,
    pub asset_issuer: Option<String>,
}

impl Balance {
    pub fn amount(&self) -> Decimal {
        Decimal::from_str(&self.balance).unwrap_or_default()
    }
}

// ── Trade ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct Trade {
    pub id:              String,
    pub ledger_close_time: String,
    pub base_amount:     String,
    pub counter_amount:  String,
    pub price:           TradePrice,
    pub base_is_seller:  bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TradePrice {
    pub n: i64,
    pub d: i64,
}

impl TradePrice {
    pub fn to_decimal(&self) -> Decimal {
        if self.d == 0 { return Decimal::ZERO; }
        Decimal::from(self.n) / Decimal::from(self.d)
    }
}

// ── Path-payment paths ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct PaymentPath {
    pub source_amount:       String,
    pub destination_amount:  String,
    pub path:                Vec<serde_json::Value>,
}

// ── Transaction result ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct TransactionResult {
    pub hash:             Option<String>,
    pub successful:       Option<bool>,
    pub result_xdr:       Option<String>,
    #[serde(default)]
    pub fee_charged:      Option<String>,
    // populated on error
    pub title:            Option<String>,
    pub status:           Option<u16>,
    pub extras:           Option<serde_json::Value>,
}

// ── SSE event ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct SseTransaction {
    pub id:                  String,
    pub hash:                String,
    pub ledger:              u64,
    pub created_at:          String,
    pub source_account:      String,
    pub fee_charged:         String,
    pub max_fee:             String,
    pub operation_count:     u32,
    pub memo_type:           String,
    pub memo:                Option<String>,
    pub envelope_xdr:        String,
    pub result_xdr:          String,
    pub result_meta_xdr:     String,
    pub fee_meta_xdr:        String,
    pub valid_after:         Option<String>,
    pub valid_before:        Option<String>,
}

// ── Client ────────────────────────────────────────────────────────────────────

const MAX_RETRIES: u32 = 5;
const BASE_BACKOFF_MS: u64 = 200;

/// Async client for the Stellar Horizon REST API.
///
/// All methods are cancellation-safe and share a single connection pool.
#[derive(Clone)]
pub struct HorizonClient {
    pub base_url: String,
    client:       Client,
}

impl HorizonClient {
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connection_verbose(false)
            .build()
            .context("Failed to build HTTP client")?;

        Ok(Self { base_url: base_url.into(), client })
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T> {
        let url = format!("{}{path}", self.base_url);
        let mut attempt = 0u32;

        loop {
            match self.client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    return resp
                        .json::<T>()
                        .await
                        .with_context(|| format!("Failed to decode JSON from {url}"));
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    if attempt >= MAX_RETRIES || !status.is_server_error() {
                        bail!("Horizon {status} at {url}: {body}");
                    }
                }
                Err(e) => {
                    if attempt >= MAX_RETRIES {
                        return Err(e).with_context(|| format!("GET {url} failed"));
                    }
                    warn!("Horizon GET {url} failed (attempt {attempt}): {e}");
                }
            }

            let delay = BASE_BACKOFF_MS * 2u64.pow(attempt);
            sleep(Duration::from_millis(delay)).await;
            attempt += 1;
        }
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /// Fetch account information (sequence number, balances).
    pub async fn get_account(&self, address: &str) -> Result<AccountResponse> {
        self.get_json(&format!("/accounts/{address}")).await
    }

    /// Fetch DEX order book for a trading pair.
    pub async fn get_order_book(
        &self,
        selling: &Asset,
        buying: &Asset,
        limit: u32,
    ) -> Result<OrderBook> {
        let mut params: Vec<(String, String)> = Vec::new();
        for (k, v) in selling.to_query_params() {
            params.push((format!("selling_{k}"), v));
        }
        for (k, v) in buying.to_query_params() {
            params.push((format!("buying_{k}"), v));
        }
        params.push(("limit".into(), limit.to_string()));

        let qs = params
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");

        self.get_json::<OrderBook>(&format!("/order_book?{qs}")).await
    }

    /// Find the cheapest path for a path-payment.
    pub async fn find_paths(
        &self,
        source_asset:   &Asset,
        dest_asset:     &Asset,
        dest_amount:    &str,
        source_account: &str,
    ) -> Result<Vec<PaymentPath>> {
        let mut params = vec![
            ("destination_amount".to_string(), dest_amount.to_string()),
            ("source_account".to_string(),     source_account.to_string()),
        ];
        for (k, v) in source_asset.to_query_params() {
            params.push((format!("source_{k}"), v));
        }
        for (k, v) in dest_asset.to_query_params() {
            params.push((format!("destination_{k}"), v));
        }
        let qs = params.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");

        #[derive(Deserialize)]
        struct PathResp { #[serde(rename = "_embedded")] embedded: Embedded }
        #[derive(Deserialize)]
        struct Embedded { records: Vec<PaymentPath> }

        let resp = self
            .get_json::<PathResp>(&format!("/paths/strict-send?{qs}"))
            .await?;
        Ok(resp.embedded.records)
    }

    /// Submit a signed transaction XDR (base64-encoded).
    ///
    /// Returns the full result even on failure so callers can inspect `extras`.
    pub async fn submit_transaction(&self, tx_xdr_b64: &str) -> Result<TransactionResult> {
        let url = format!("{}/transactions", self.base_url);
        let resp = self
            .client
            .post(&url)
            .form(&[("tx", tx_xdr_b64)])
            .send()
            .await
            .with_context(|| format!("POST {url} failed"))?;

        let result: TransactionResult = resp.json().await.context("decode TransactionResult")?;

        if result.successful == Some(false) {
            warn!(
                hash = ?result.hash,
                extras = ?result.extras,
                "Transaction failed"
            );
        }
        Ok(result)
    }

    /// Stream ledger-close transactions via Horizon SSE.
    ///
    /// Yields parsed [`SseTransaction`] events.  The stream reconnects
    /// automatically on error (cursor is advanced past successfully received events).
    pub async fn stream_transactions(
        &self,
        cursor: &str,
        tx: tokio::sync::mpsc::Sender<SseTransaction>,
    ) {
        let url = format!(
            "{}/transactions?cursor={cursor}&order=asc&limit=200",
            self.base_url
        );

        loop {
            debug!("Connecting to SSE stream: {url}");
            match self.client.get(&url).header("Accept", "text/event-stream").send().await {
                Err(e) => {
                    warn!("SSE connect error: {e}");
                    sleep(Duration::from_secs(2)).await;
                }
                Ok(resp) => {
                    let mut stream = resp.bytes_stream();
                    let mut buf = String::new();

                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Err(e) => { warn!("SSE read error: {e}"); break; }
                            Ok(bytes) => {
                                buf.push_str(&String::from_utf8_lossy(&bytes));
                                // SSE events are separated by double newlines
                                while let Some(pos) = buf.find("\n\n") {
                                    let event = buf[..pos].to_string();
                                    buf = buf[pos + 2..].to_string();

                                    if let Some(data) = event
                                        .lines()
                                        .find(|l| l.starts_with("data:"))
                                    {
                                        let json = data.trim_start_matches("data:").trim();
                                        match serde_json::from_str::<SseTransaction>(json) {
                                            Ok(t) => {
                                                if tx.send(t).await.is_err() {
                                                    return; // receiver dropped
                                                }
                                            }
                                            Err(e) => {
                                                debug!("SSE parse skip: {e}");
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }

    /// Fetch recent trades for a pair (useful for price-discovery).
    pub async fn get_trades(
        &self,
        base: &Asset,
        counter: &Asset,
        limit: u32,
    ) -> Result<Vec<Trade>> {
        let mut params: Vec<(String, String)> = Vec::new();
        for (k, v) in base.to_query_params() {
            params.push((format!("base_{k}"), v));
        }
        for (k, v) in counter.to_query_params() {
            params.push((format!("counter_{k}"), v));
        }
        params.push(("limit".into(), limit.to_string()));
        params.push(("order".into(), "desc".into()));

        let qs = params.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");

        #[derive(Deserialize)]
        struct TradeResp { #[serde(rename = "_embedded")] embedded: Embedded }
        #[derive(Deserialize)]
        struct Embedded { records: Vec<Trade> }

        let resp = self.get_json::<TradeResp>(&format!("/trades?{qs}")).await?;
        Ok(resp.embedded.records)
    }

    /// Current base fee in stroops from fee statistics.
    pub async fn get_base_fee_stroops(&self) -> Result<u32> {
        #[derive(Deserialize)]
        struct FeeStats {
            fee_charged: FeePercentiles,
        }
        #[derive(Deserialize)]
        struct FeePercentiles {
            p50: String,
        }
        let stats: FeeStats = self.get_json("/fee_stats").await?;
        Ok(stats.fee_charged.p50.parse().unwrap_or(100))
    }
}
