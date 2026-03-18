/**
 * consumers/index.ts
 *
 * Entry point for all AgentForge background consumers.
 * Run with:  npm run consumers:all
 *
 * Starts:
 *   • Payment Verifier
 *   • Agent Executor
 *   • Billing Aggregator
 *   • Marketplace Feed (two sub-consumers)
 *   • Chain Syncer (Kafka consumer + Horizon SSE stream)
 *   • A2A Router
 */

import paymentVerifier from './payment-verifier';
import agentExecutor from './agent-executor';
import billingAggregator from './billing-aggregator';
import { completedConsumer as mfCompleted, billingConsumer as mfBilling } from './marketplace-feed';
import { chainSyncedConsumer, startChainSyncerStream } from './chain-syncer';
import a2aRouter from './a2a-router';

const consumers = [
  paymentVerifier,
  agentExecutor,
  billingAggregator,
  mfCompleted,
  mfBilling,
  chainSyncedConsumer,
  a2aRouter,
];

console.log('[Consumers] Starting AgentForge PubSub consumers...');

for (const c of consumers) {
  c.start();
}

// Start the Horizon SSE stream (returns a close fn)
const stopChainStream = startChainSyncerStream();

// Graceful shutdown
function shutdown() {
  console.log('[Consumers] Shutting down...');
  for (const c of consumers) {
    c.stop();
  }
  stopChainStream();
  process.exit(0);
}

process.on('SIGTERM', shutdown);
process.on('SIGINT', shutdown);

console.log('[Consumers] All consumers running. Press Ctrl+C to stop.');
