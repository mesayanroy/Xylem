/**
 * consumers/billing-aggregator.ts
 *
 * Listens on `agentforge.agent.completed`.
 *
 * For each completed agent run it:
 *  1. Increments `total_requests` and `total_earned_xlm` on the agent row.
 *  2. Publishes `agentforge.billing.updated` with the updated totals.
 */

import { createClient } from '@supabase/supabase-js';
import { createConsumer, publish, TOPICS } from '../lib/kafka';
import type { AgentCompletedEvent, BillingUpdatedEvent } from '../types/events';

const CONSUMER_GROUP = 'agentforge-billing-aggregator';

const supabaseUrl = process.env.NEXT_PUBLIC_SUPABASE_URL || '';
const supabaseServiceKey =
  process.env.SUPABASE_SERVICE_ROLE_KEY || process.env.NEXT_PUBLIC_SUPABASE_ANON_KEY || '';

function getSupabase() {
  if (!supabaseUrl || !supabaseServiceKey) {
    throw new Error('Supabase is not configured for the billing aggregator.');
  }
  return createClient(supabaseUrl, supabaseServiceKey);
}

const consumer = createConsumer<AgentCompletedEvent>(
  CONSUMER_GROUP,
  TOPICS.AGENT_COMPLETED,
  async (event) => {
    const { agentId, ownerWallet, priceXlm } = event;

    console.log(`[BillingAggregator] Updating earnings for agent ${agentId}`);

    const sb = getSupabase();

    // Read current totals
    const { data: agent, error: fetchErr } = await sb
      .from('agents')
      .select('total_requests, total_earned_xlm')
      .eq('id', agentId)
      .single();

    if (fetchErr || !agent) {
      console.error(`[BillingAggregator] Cannot fetch agent ${agentId}:`, fetchErr);
      return;
    }

    const newTotalRequests = (agent.total_requests ?? 0) + 1;
    const newTotalEarned = (agent.total_earned_xlm ?? 0) + priceXlm;

    const { error: updateErr } = await sb
      .from('agents')
      .update({
        total_requests: newTotalRequests,
        total_earned_xlm: newTotalEarned,
        updated_at: new Date().toISOString(),
      })
      .eq('id', agentId);

    if (updateErr) {
      console.error(`[BillingAggregator] Update error for agent ${agentId}:`, updateErr);
      return;
    }

    const billing: BillingUpdatedEvent = {
      agentId,
      ownerWallet,
      earnedXlm: priceXlm,
      totalEarnedXlm: newTotalEarned,
      totalRequests: newTotalRequests,
      updatedAt: new Date().toISOString(),
    };

    await publish(TOPICS.BILLING_UPDATED, billing);
    console.log(
      `[BillingAggregator] Agent ${agentId} | total_earned=${newTotalEarned} XLM | requests=${newTotalRequests}`
    );
  }
);

export default consumer;
