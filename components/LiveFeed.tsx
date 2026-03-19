'use client';

/**
 * LiveFeed — real-time Kafka activity ticker.
 *
 * Simulates (or connects to) the Kafka `agentforge.marketplace.activity`
 * topic and displays a scrolling live feed of agent events.
 *
 * In production, swap the mock interval for an Ably subscription that
 * receives events forwarded from the Kafka consumer in `lib/kafka.ts`.
 */

import { motion, AnimatePresence } from 'framer-motion';
import { useEffect, useState } from 'react';

interface FeedEvent {
  id:         number;
  type:       'payment' | 'trade' | 'arb' | 'mev' | 'relay';
  agentType:  string;
  action:     string;
  profit:     string | null;
  txHash:     string;
  ts:         string;
}

const MOCK_AGENTS = [
  { type: 'payment',  agent: 'MEV Bot',           actions: ['front_buy', 'front_sell', 'sandwich']   },
  { type: 'trade',    agent: 'Trading Bot',        actions: ['grid_fill', 'dca_buy', 'limit_sell']    },
  { type: 'arb',      agent: 'Arbitrage Tracker',  actions: ['tri_arb', 'path_arb', 'fill_detected']  },
  { type: 'relay',    agent: 'Relayer',            actions: ['fee_bump', 'relay_confirmed']           },
  { type: 'mev',      agent: 'Mempool Monitor',    actions: ['large_tx_alert', 'fee_surge_alert']     },
];

function randomHex(len: number) {
  return Array.from({ length: len }, () =>
    Math.floor(Math.random() * 16).toString(16)
  ).join('');
}

function randProfit() {
  return (Math.random() * 2).toFixed(4) + ' XLM';
}

const TYPE_STYLES: Record<string, string> = {
  payment: 'text-[#00FFE5]',
  trade:   'text-[#7b61ff]',
  arb:     'text-[#f59e0b]',
  mev:     'text-[#f87171]',
  relay:   'text-[#4ade80]',
};

const TYPE_BADGES: Record<string, string> = {
  payment: 'bg-[rgba(0,255,229,0.1)] border-[rgba(0,255,229,0.3)] text-[#00FFE5]',
  trade:   'bg-[rgba(123,97,255,0.1)] border-[rgba(123,97,255,0.3)] text-[#7b61ff]',
  arb:     'bg-[rgba(245,158,11,0.1)] border-[rgba(245,158,11,0.3)] text-[#f59e0b]',
  mev:     'bg-[rgba(248,113,113,0.1)] border-[rgba(248,113,113,0.3)] text-[#f87171]',
  relay:   'bg-[rgba(74,222,128,0.1)] border-[rgba(74,222,128,0.3)] text-[#4ade80]',
};

let _uid = 0;

export default function LiveFeed() {
  const [events, setEvents] = useState<FeedEvent[]>([]);

  useEffect(() => {
    // Seed 4 initial events immediately
    const seed: FeedEvent[] = Array.from({ length: 4 }, () => generateEvent());
    setEvents(seed);

    const timer = setInterval(() => {
      setEvents(prev => [generateEvent(), ...prev].slice(0, 10));
    }, 1800);

    return () => clearInterval(timer);
  }, []);

  return (
    <div className="rounded-2xl border border-white/[0.06] bg-[rgba(5,5,8,0.85)] backdrop-blur-xl overflow-hidden">
      {/* Header */}
      <div className="flex items-center justify-between px-5 py-3 border-b border-white/[0.06]">
        <div className="flex items-center gap-2">
          <span className="w-2 h-2 rounded-full bg-[#00FFE5] animate-pulse" />
          <span className="font-mono text-xs text-white/70">Live Activity</span>
          <span className="font-mono text-[10px] text-white/30 ml-1">kafka://agentforge.agent.completed</span>
        </div>
        <span className="font-mono text-[10px] text-white/30">0x402 · Stellar · Upstash</span>
      </div>

      {/* Feed rows */}
      <div className="divide-y divide-white/[0.03]">
        <AnimatePresence initial={false}>
          {events.map(ev => (
            <motion.div
              key={ev.id}
              initial={{ opacity: 0, y: -16 }}
              animate={{ opacity: 1,  y: 0 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.3 }}
              className="flex items-start gap-3 px-5 py-3 hover:bg-white/[0.02] transition-colors"
            >
              {/* Badge */}
              <span className={`mt-0.5 shrink-0 px-1.5 py-0.5 rounded text-[9px] font-mono border uppercase tracking-wide ${TYPE_BADGES[ev.type]}`}>
                {ev.agentType.replace(' ', '\u00A0')}
              </span>

              {/* Content */}
              <div className="flex-1 min-w-0">
                <div className="flex items-baseline gap-2 flex-wrap">
                  <span className={`font-mono text-xs font-semibold ${TYPE_STYLES[ev.type]}`}>
                    {ev.action}
                  </span>
                  {ev.profit && (
                    <span className="font-mono text-[10px] text-[#4ade80]">
                      +{ev.profit}
                    </span>
                  )}
                </div>
                <div className="flex items-center gap-2 mt-0.5">
                  <span className="font-mono text-[10px] text-white/20">
                    tx:{ev.txHash}
                  </span>
                  <span className="text-white/10">·</span>
                  <span className="font-mono text-[10px] text-white/20">{ev.ts}</span>
                </div>
              </div>
            </motion.div>
          ))}
        </AnimatePresence>
      </div>
    </div>
  );
}

function generateEvent(): FeedEvent {
  const template = MOCK_AGENTS[Math.floor(Math.random() * MOCK_AGENTS.length)];
  const action   = template.actions[Math.floor(Math.random() * template.actions.length)];
  const hasProfit = ['payment', 'trade', 'arb', 'mev'].includes(template.type);

  return {
    id:        ++_uid,
    type:      template.type as FeedEvent['type'],
    agentType: template.agent,
    action,
    profit:    hasProfit && Math.random() > 0.3 ? randProfit() : null,
    txHash:    randomHex(8) + '…',
    ts:        new Date().toLocaleTimeString('en-US', { hour12: false }),
  };
}
