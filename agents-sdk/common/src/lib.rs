//! Xylem AgentForge — shared SDK utilities
//!
//! Provides the building blocks used by every agent:
//!
//! - [`wallet`]      — Stellar keypair management, Strkey decode/encode, ed25519 signing
//! - [`horizon`]     — Async Horizon REST + SSE client (order-books, accounts, offers, paths …)
//! - [`stellar_tx`]  — Transaction envelope builder & fee-bump helpers
//! - [`config`]      — Common environment-variable configuration
//! - [`payment`]     — 0x402 protocol client: pay-per-request HTTP with auto payment dance
//! - [`pubsub`]      — Upstash Kafka REST producer for publishing agent events to the platform

pub mod config;
pub mod horizon;
pub mod payment;
pub mod pubsub;
pub mod stellar_tx;
pub mod wallet;

// Re-export the most commonly used types at crate root for ergonomic imports.
pub use config::CommonConfig;
pub use horizon::{Asset, HorizonClient, OrderBook, OrderBookLevel};
pub use payment::PaymentClient;
pub use pubsub::{
    now_iso, AgentActionEvent, ChainEvent, KafkaPublisher, PaymentReceivedEvent,
    TOPIC_AGENT_COMPLETED, TOPIC_MARKETPLACE_ACTIVITY,
};
pub use stellar_tx::{OperationBody, TransactionBuilder};
pub use wallet::{Keypair, WalletError};
