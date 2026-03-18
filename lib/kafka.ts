/**
 * lib/kafka.ts
 *
 * Upstash Kafka backbone for AgentForge.
 * All topic names are defined here so every producer/consumer imports from
 * a single source of truth.
 *
 * Upstash Kafka is REST-based so it works in both Node.js consumers and
 * Next.js edge/serverless API routes without a native TCP socket.
 */

import { Kafka } from '@upstash/kafka';

// ─── Kafka client singleton ───────────────────────────────────────────────────

let _kafka: Kafka | null = null;

function getKafkaClient(): Kafka {
  if (!_kafka) {
    const url = process.env.UPSTASH_KAFKA_BROKER;
    const username = process.env.UPSTASH_KAFKA_USERNAME;
    const password = process.env.UPSTASH_KAFKA_PASSWORD;

    if (!url || !username || !password) {
      throw new Error(
        'Upstash Kafka is not configured. ' +
          'Set UPSTASH_KAFKA_BROKER, UPSTASH_KAFKA_USERNAME and UPSTASH_KAFKA_PASSWORD.'
      );
    }

    _kafka = new Kafka({ url, username, password });
  }
  return _kafka;
}

// ─── Topic registry ───────────────────────────────────────────────────────────

export const TOPICS = {
  /** Emitted by the /run route once a tx hash has been supplied by the caller. */
  PAYMENT_PENDING: 'agentforge.payment.pending',

  /** Emitted by the Payment Verifier once Horizon confirms the tx. */
  PAYMENT_CONFIRMED: 'agentforge.payment.confirmed',

  /** Emitted by the Agent Executor once the AI model has returned a response. */
  AGENT_COMPLETED: 'agentforge.agent.completed',

  /** Emitted by the Billing Aggregator after updating Supabase earnings. */
  BILLING_UPDATED: 'agentforge.billing.updated',

  /** Emitted by the Marketplace Feed consumer before pushing to Ably. */
  MARKETPLACE_ACTIVITY: 'agentforge.marketplace.activity',

  /** Emitted by the Chain Syncer for any on-chain state change. */
  CHAIN_SYNCED: 'agentforge.chain.synced',

  /** Used by the A2A Router to forward requests between agents. */
  A2A_REQUEST: 'agentforge.a2a.request',

  /** Used by the A2A Router to deliver responses between agents. */
  A2A_RESPONSE: 'agentforge.a2a.response',
} as const;

export type Topic = (typeof TOPICS)[keyof typeof TOPICS];

// ─── Producer helper ──────────────────────────────────────────────────────────

/**
 * Publish a single JSON message to a Kafka topic.
 * Safe to call from Next.js API routes (REST under the hood).
 */
export async function publish<T>(topic: Topic, payload: T): Promise<void> {
  const kafka = getKafkaClient();
  const producer = kafka.producer();
  await producer.produce(topic, JSON.stringify(payload));
}

// ─── Consumer factory ─────────────────────────────────────────────────────────

export type MessageHandler<T = unknown> = (payload: T) => Promise<void>;

/**
 * Create a long-running consumer that processes every message on `topic`.
 *
 * @param groupId   Kafka consumer group – use a unique string per consumer.
 * @param topic     One of the `TOPICS` constants.
 * @param handler   Async function called with the parsed JSON payload.
 * @param options   Optional poll interval in milliseconds (default 1000 ms).
 */
export function createConsumer<T>(
  groupId: string,
  topic: Topic,
  handler: MessageHandler<T>,
  options: { pollIntervalMs?: number } = {}
): { start: () => void; stop: () => void } {
  const { pollIntervalMs = 1_000 } = options;
  let running = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  async function poll(): Promise<void> {
    if (!running) return;
    try {
      const kafka = getKafkaClient();
      const consumer = kafka.consumer();
      const messages = await consumer.consume({
        consumerGroupId: groupId,
        instanceId: `${groupId}-instance`,
        topics: [topic],
        autoOffsetReset: 'earliest',
      });

      for (const msg of messages) {
        try {
          const parsed = JSON.parse(msg.value as string) as T;
          await handler(parsed);
        } catch (err) {
          console.error(`[${groupId}] Error handling message:`, err);
        }
      }
    } catch (err) {
      console.error(`[${groupId}] Poll error:`, err);
    } finally {
      if (running) {
        timer = setTimeout(poll, pollIntervalMs);
      }
    }
  }

  return {
    start() {
      if (running) return;
      running = true;
      console.log(`[${groupId}] Starting consumer on topic "${topic}"`);
      void poll();
    },
    stop() {
      running = false;
      if (timer) clearTimeout(timer);
      console.log(`[${groupId}] Stopped.`);
    },
  };
}
