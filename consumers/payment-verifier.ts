/**
 * consumers/payment-verifier.ts
 *
 * Listens on `agentforge.payment.pending`.
 *
 * For each pending payment it:
 *  1. Opens a Horizon SSE stream for the agent owner's wallet.
 *  2. Waits until the specific tx hash lands on the ledger (with a timeout).
 *  3. Publishes `agentforge.payment.confirmed` so the Agent Executor can proceed.
 */

import { createConsumer, publish, TOPICS } from '../lib/kafka';
import { waitForTransaction } from '../lib/stellar';
import type { PaymentPendingEvent, PaymentConfirmedEvent } from '../types/events';

const CONSUMER_GROUP = 'agentforge-payment-verifier';
const TX_TIMEOUT_MS = 120_000; // 2 minutes

const consumer = createConsumer<PaymentPendingEvent>(
  CONSUMER_GROUP,
  TOPICS.PAYMENT_PENDING,
  async (event) => {
    const {
      requestId,
      agentId,
      txHash,
      callerWallet,
      ownerWallet,
      priceXlm,
      input,
      memo,
    } = event;

    console.log(`[PaymentVerifier] Verifying tx ${txHash} for request ${requestId}`);

    try {
      // Wait until Horizon has the tx (polls every 3 s, times out after 2 min)
      const tx = await waitForTransaction(txHash, TX_TIMEOUT_MS);

      // Basic sanity: memo must start with expected prefix (e.g. "agent:<id>")
      const memoPrefix = memo.includes(':') ? memo.split(':').slice(0, 2).join(':') : memo;
      if (memo && tx.memo && !tx.memo.startsWith(memoPrefix)) {
        console.warn(`[PaymentVerifier] Memo mismatch for ${requestId}: expected prefix "${memo}", got "${tx.memo}"`);
        return;
      }

      const confirmed: PaymentConfirmedEvent = {
        requestId,
        agentId,
        txHash,
        callerWallet,
        ownerWallet,
        priceXlm,
        input,
        confirmedAt: new Date().toISOString(),
      };

      await publish(TOPICS.PAYMENT_CONFIRMED, confirmed);
      console.log(`[PaymentVerifier] Published payment.confirmed for request ${requestId}`);
    } catch (err) {
      console.error(`[PaymentVerifier] Failed to verify tx ${txHash}:`, err);
    }
  }
);

export default consumer;
