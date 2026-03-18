/**
 * consumers/a2a-router.ts
 *
 * Agent-to-Agent (A2A) router.
 *
 * Listens on `agentforge.a2a.request`.
 * Calls the target agent's /run endpoint (internal HTTP) and publishes the
 * result to `agentforge.a2a.response`.
 *
 * This enables composable agent pipelines where one agent can delegate
 * sub-tasks to another with payment handled automatically.
 */

import { createConsumer, publish, TOPICS } from '../lib/kafka';
import type { A2ARequestEvent, A2AResponseEvent } from '../types/events';

const CONSUMER_GROUP = 'agentforge-a2a-router';

/** Base URL for internal API calls. Defaults to localhost in development. */
const API_BASE = process.env.NEXT_PUBLIC_APP_URL || 'http://localhost:3000';

const consumer = createConsumer<A2ARequestEvent>(
  CONSUMER_GROUP,
  TOPICS.A2A_REQUEST,
  async (event) => {
    const { correlationId, fromAgentId, toAgentId, input, callerWallet, paymentTxHash } =
      event;

    console.log(
      `[A2ARouter] Routing from agent ${fromAgentId} → agent ${toAgentId} (correlationId=${correlationId})`
    );

    const startTime = Date.now();

    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
      'X-Payment-Wallet': callerWallet,
    };

    if (paymentTxHash) {
      headers['X-Payment-Tx-Hash'] = paymentTxHash;
    }

    let response: A2AResponseEvent;

    try {
      const res = await fetch(`${API_BASE}/api/agents/${toAgentId}/run`, {
        method: 'POST',
        headers,
        body: JSON.stringify({ input }),
      });

      const latencyMs = Date.now() - startTime;

      if (!res.ok) {
        const body = (await res.json().catch(() => ({}))) as Record<string, unknown>;
        response = {
          correlationId,
          fromAgentId,
          toAgentId,
          output: '',
          latencyMs,
          success: false,
          error: (body.error as string) || `HTTP ${res.status}`,
          completedAt: new Date().toISOString(),
        };
      } else {
        const body = (await res.json()) as { output?: string; request_id?: string };
        response = {
          correlationId,
          fromAgentId,
          toAgentId,
          output: body.output ?? '',
          latencyMs,
          success: true,
          completedAt: new Date().toISOString(),
        };
      }
    } catch (err) {
      response = {
        correlationId,
        fromAgentId,
        toAgentId,
        output: '',
        latencyMs: Date.now() - startTime,
        success: false,
        error: String(err),
        completedAt: new Date().toISOString(),
      };
    }

    await publish(TOPICS.A2A_RESPONSE, response);
    console.log(
      `[A2ARouter] Published a2a.response for correlationId=${correlationId} success=${response.success}`
    );
  }
);

export default consumer;
