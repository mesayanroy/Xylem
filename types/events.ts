/**
 * types/events.ts
 *
 * Strongly-typed event payloads for every Kafka topic used by AgentForge.
 * Import from here to ensure producers and consumers agree on the schema.
 */

// ─── payment.pending ─────────────────────────────────────────────────────────

/** Published by the /run API route when the caller submits a tx hash. */
export interface PaymentPendingEvent {
  /** Unique request identifier (UUID v4). */
  requestId: string;
  /** The agent being invoked. */
  agentId: string;
  /** Stellar transaction hash provided by the caller. */
  txHash: string;
  /** Caller Stellar wallet address. */
  callerWallet: string;
  /** Expected payout destination (agent owner wallet). */
  ownerWallet: string;
  /** Required payment amount in XLM. */
  priceXlm: number;
  /** Memo that must be present on the tx (up to 28 chars). */
  memo: string;
  /** The user's prompt / input payload. */
  input: string;
  /** ISO-8601 timestamp. */
  createdAt: string;
}

// ─── payment.confirmed ───────────────────────────────────────────────────────

/** Published by the Payment Verifier after Horizon confirms the tx. */
export interface PaymentConfirmedEvent {
  requestId: string;
  agentId: string;
  txHash: string;
  callerWallet: string;
  ownerWallet: string;
  priceXlm: number;
  input: string;
  /** ISO-8601 timestamp when Horizon confirmed the ledger inclusion. */
  confirmedAt: string;
}

// ─── agent.completed ─────────────────────────────────────────────────────────

/** Published by the Agent Executor once the AI model returns a response. */
export interface AgentCompletedEvent {
  requestId: string;
  agentId: string;
  /** AI model identifier, e.g. "openai-gpt4o-mini". */
  model: string;
  callerWallet: string;
  ownerWallet: string;
  priceXlm: number;
  input: string;
  output: string;
  latencyMs: number;
  txHash: string;
  /** ISO-8601 timestamp. */
  completedAt: string;
}

// ─── billing.updated ─────────────────────────────────────────────────────────

/** Published by the Billing Aggregator after updating agent earnings. */
export interface BillingUpdatedEvent {
  agentId: string;
  ownerWallet: string;
  /** Amount added in this billing cycle (XLM). */
  earnedXlm: number;
  /** Running total earned by the agent (XLM). */
  totalEarnedXlm: number;
  /** Running total number of successful requests. */
  totalRequests: number;
  /** ISO-8601 timestamp. */
  updatedAt: string;
}

// ─── marketplace.activity ────────────────────────────────────────────────────

/** Published by the Marketplace Feed to be forwarded to Ably. */
export interface MarketplaceActivityEvent {
  /** One of: "agent_run" | "payment_received" | "new_agent" */
  eventType: 'agent_run' | 'payment_received' | 'new_agent';
  agentId: string;
  /** Agent display name. */
  agentName: string;
  callerWallet?: string;
  ownerWallet: string;
  priceXlm?: number;
  totalEarnedXlm?: number;
  totalRequests?: number;
  /** ISO-8601 timestamp. */
  timestamp: string;
}

// ─── chain.synced ─────────────────────────────────────────────────────────────

/** Published by the Chain Syncer after a Soroban state change is detected. */
export interface ChainSyncedEvent {
  /** Soroban contract ID. */
  contractId: string;
  /** The on-chain node / agent ID. */
  nodeId: string;
  /** Block / ledger sequence number. */
  ledgerSequence: number;
  /** Arbitrary on-chain data payload. */
  data: Record<string, unknown>;
  /** ISO-8601 timestamp. */
  syncedAt: string;
}

// ─── a2a.request ──────────────────────────────────────────────────────────────

/** Published by a caller agent that wants to delegate to another agent. */
export interface A2ARequestEvent {
  /** Correlation ID that ties request ↔ response. */
  correlationId: string;
  /** The originating (caller) agent ID. */
  fromAgentId: string;
  /** The target (callee) agent ID. */
  toAgentId: string;
  /** Input passed to the target agent. */
  input: string;
  /** Wallet that will cover the target agent's price. */
  callerWallet: string;
  /** Pre-authorised payment tx hash (may be empty for free agents). */
  paymentTxHash?: string;
  /** ISO-8601 timestamp. */
  createdAt: string;
}

// ─── a2a.response ─────────────────────────────────────────────────────────────

/** Published by the A2A Router after the target agent replies. */
export interface A2AResponseEvent {
  correlationId: string;
  fromAgentId: string;
  toAgentId: string;
  output: string;
  latencyMs: number;
  success: boolean;
  error?: string;
  /** ISO-8601 timestamp. */
  completedAt: string;
}
