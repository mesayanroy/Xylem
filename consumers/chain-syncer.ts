/**
 * consumers/chain-syncer.ts
 *
 * Listens for on-chain state changes from the Soroban contract and publishes
 * `agentforge.chain.synced` events.
 *
 * In a full implementation this would open a Soroban RPC subscription.
 * For now it polls the Horizon account transactions stream for the contract's
 * deployment account and emits a ChainSyncedEvent for each new ledger entry.
 */

import { createConsumer, publish, TOPICS } from '../lib/kafka';
import { watchAccountTransactions } from '../lib/stellar';
import type { ChainSyncedEvent } from '../types/events';

const CONSUMER_GROUP = 'agentforge-chain-syncer';
const CONTRACT_ID = process.env.NEXT_PUBLIC_SOROBAN_CONTRACT_ID || '';

/**
 * Start the Horizon SSE watcher for the Soroban contract account and re-publish
 * to Kafka so downstream services can react without coupling to Horizon directly.
 */
export function startChainSyncerStream(): () => void {
  if (!CONTRACT_ID) {
    console.warn('[ChainSyncer] NEXT_PUBLIC_SOROBAN_CONTRACT_ID is not set – chain syncer disabled.');
    return () => undefined;
  }

  console.log(`[ChainSyncer] Watching Horizon for contract account ${CONTRACT_ID}`);

  const close = watchAccountTransactions(CONTRACT_ID, async (tx) => {
    const event: ChainSyncedEvent = {
      contractId: CONTRACT_ID,
      nodeId: tx.id,
      ledgerSequence: tx.ledger_attr ?? 0,
      data: {
        hash: tx.hash,
        memo: tx.memo,
        fee: tx.fee_charged,
        operationCount: tx.operation_count,
        successful: tx.successful,
        createdAt: tx.created_at,
      },
      syncedAt: new Date().toISOString(),
    };

    try {
      await publish(TOPICS.CHAIN_SYNCED, event);
      console.log(`[ChainSyncer] Synced ledger op ${tx.hash} for contract ${CONTRACT_ID}`);
    } catch (err) {
      console.error('[ChainSyncer] Publish error:', err);
    }
  });

  return close;
}

/**
 * Kafka consumer for chain.synced events (so other microservices can react
 * without connecting to Horizon themselves).
 */
export const chainSyncedConsumer = createConsumer<ChainSyncedEvent>(
  CONSUMER_GROUP,
  TOPICS.CHAIN_SYNCED,
  async (event) => {
    console.log(
      `[ChainSyncer] Received chain.synced: contract=${event.contractId} ledger=${event.ledgerSequence}`
    );
    // Extend here: update on-chain agent registry in Supabase, invalidate caches, etc.
  }
);
