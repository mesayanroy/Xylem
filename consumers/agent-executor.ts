/**
 * consumers/agent-executor.ts
 *
 * Listens on `agentforge.payment.confirmed`.
 *
 * For each confirmed payment it:
 *  1. Fetches the agent record from Supabase.
 *  2. Calls the appropriate AI model (OpenAI / Anthropic).
 *  3. Writes the request record to `agent_requests` in Supabase.
 *  4. Publishes `agentforge.agent.completed` for downstream consumers.
 */

import { createClient } from '@supabase/supabase-js';
import { createConsumer, publish, TOPICS } from '../lib/kafka';
import type { PaymentConfirmedEvent, AgentCompletedEvent } from '../types/events';

const CONSUMER_GROUP = 'agentforge-agent-executor';

const supabaseUrl = process.env.NEXT_PUBLIC_SUPABASE_URL || '';
const supabaseServiceKey =
  process.env.SUPABASE_SERVICE_ROLE_KEY || process.env.NEXT_PUBLIC_SUPABASE_ANON_KEY || '';

function getSupabase() {
  if (!supabaseUrl || !supabaseServiceKey) {
    throw new Error('Supabase is not configured for the agent executor.');
  }
  return createClient(supabaseUrl, supabaseServiceKey);
}

async function fetchAgent(agentId: string) {
  const sb = getSupabase();
  const { data, error } = await sb.from('agents').select('*').eq('id', agentId).single();
  if (error || !data) throw new Error(`Agent ${agentId} not found`);
  return data as {
    id: string;
    name: string;
    owner_wallet: string;
    model: string;
    system_prompt: string;
    price_xlm: number;
    total_requests: number;
    total_earned_xlm: number;
  };
}

async function runModel(model: string, systemPrompt: string, input: string): Promise<string> {
  if (model === 'openai-gpt4o-mini') {
    // Dynamic import keeps the consumer file lightweight
    const { default: OpenAI } = await import('openai');
    const client = new OpenAI({ apiKey: process.env.OPENAI_API_KEY });
    const res = await client.chat.completions.create({
      model: 'gpt-4o-mini',
      messages: [
        { role: 'system', content: systemPrompt },
        { role: 'user', content: input },
      ],
      max_tokens: 1024,
    });
    return res.choices[0]?.message?.content ?? '';
  }

  if (model === 'anthropic-claude-haiku') {
    const Anthropic = (await import('@anthropic-ai/sdk')).default;
    const client = new Anthropic({ apiKey: process.env.ANTHROPIC_API_KEY });
    const res = await client.messages.create({
      model: 'claude-haiku-20240307',
      max_tokens: 1024,
      system: systemPrompt,
      messages: [{ role: 'user', content: input }],
    });
    const block = res.content[0];
    return block?.type === 'text' ? block.text : '';
  }

  throw new Error(`Unknown model: ${model}`);
}

const consumer = createConsumer<PaymentConfirmedEvent>(
  CONSUMER_GROUP,
  TOPICS.PAYMENT_CONFIRMED,
  async (event) => {
    const { requestId, agentId, txHash, callerWallet, ownerWallet, priceXlm, input, confirmedAt } =
      event;

    console.log(`[AgentExecutor] Executing agent ${agentId} for request ${requestId}`);
    const startTime = Date.now();

    let agent;
    try {
      agent = await fetchAgent(agentId);
    } catch (err) {
      console.error(`[AgentExecutor] Cannot fetch agent ${agentId}:`, err);
      return;
    }

    let output: string;
    try {
      output = await runModel(agent.model, agent.system_prompt, input);
    } catch (err) {
      console.error(`[AgentExecutor] Model error for request ${requestId}:`, err);
      return;
    }

    const latencyMs = Date.now() - startTime;

    // Persist request record
    try {
      const sb = getSupabase();
      await sb.from('agent_requests').insert({
        id: requestId,
        agent_id: agentId,
        caller_wallet: callerWallet || null,
        input_payload: { input },
        output_response: { output },
        payment_tx_hash: txHash,
        payment_amount_xlm: priceXlm,
        protocol: '0x402',
        status: 'success',
        latency_ms: latencyMs,
        created_at: confirmedAt,
      });
    } catch (err) {
      console.error(`[AgentExecutor] DB insert error for ${requestId}:`, err);
      // Continue – billing + feed should still fire
    }

    const completed: AgentCompletedEvent = {
      requestId,
      agentId,
      model: agent.model,
      callerWallet,
      ownerWallet,
      priceXlm,
      input,
      output,
      latencyMs,
      txHash,
      completedAt: new Date().toISOString(),
    };

    await publish(TOPICS.AGENT_COMPLETED, completed);
    console.log(`[AgentExecutor] Published agent.completed for request ${requestId} (${latencyMs}ms)`);
  }
);

export default consumer;
