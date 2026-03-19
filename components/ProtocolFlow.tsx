'use client';

/**
 * ProtocolFlow — animated diagram showing how the 0x402 payment protocol
 * and Kafka pub-sub backbone connect agents and the platform.
 *
 * Layout (left → right):
 *   [Agent SDK] →[0x402 Challenge]→ [Platform API] →[Kafka Topics]→ [Dashboard]
 */

import { motion } from 'framer-motion';

interface FlowStep {
  id:      number;
  label:   string;
  sub:     string;
  color:   string;
  border:  string;
  icon:    string;
}

const STEPS: FlowStep[] = [
  {
    id:    1,
    label: 'Agent SDK',
    sub:   'Rust / 0x402 client',
    color: 'text-[#00FFE5]',
    border:'border-[rgba(0,255,229,0.3)]',
    icon:  '⚙️',
  },
  {
    id:    2,
    label: '0x402 Protocol',
    sub:   'HTTP 402 → Stellar TX',
    color: 'text-[#f59e0b]',
    border:'border-[rgba(245,158,11,0.3)]',
    icon:  '💳',
  },
  {
    id:    3,
    label: 'Platform API',
    sub:   'Next.js · Soroban verify',
    color: 'text-[#7b61ff]',
    border:'border-[rgba(123,97,255,0.3)]',
    icon:  '🔗',
  },
  {
    id:    4,
    label: 'Kafka Pub-Sub',
    sub:   'Upstash · 8 topics',
    color: 'text-[#4ade80]',
    border:'border-[rgba(74,222,128,0.3)]',
    icon:  '🔀',
  },
  {
    id:    5,
    label: 'Dashboard',
    sub:   'Real-time · Ably SSE',
    color: 'text-[#00FFE5]',
    border:'border-[rgba(0,255,229,0.3)]',
    icon:  '📊',
  },
];

const ARROW_LABELS = ['HTTP 402', 'XLM tx', 'verify', 'events'];

export default function ProtocolFlow() {
  return (
    <div className="overflow-x-auto pb-2">
      <div className="flex items-center gap-0 min-w-max mx-auto">
        {STEPS.map((step, idx) => (
          <div key={step.id} className="flex items-center">
            {/* Node */}
            <motion.div
              initial={{ opacity: 0, y: 20 }}
              whileInView={{ opacity: 1, y: 0 }}
              viewport={{ once: true }}
              transition={{ delay: idx * 0.12 }}
              className={`flex flex-col items-center text-center w-32 px-3 py-4 rounded-xl border ${step.border} bg-[rgba(255,255,255,0.02)]`}
            >
              <span className="text-2xl mb-2">{step.icon}</span>
              <span className={`font-mono text-xs font-bold ${step.color}`}>{step.label}</span>
              <span className="font-mono text-[9px] text-white/30 mt-1 leading-tight">{step.sub}</span>
            </motion.div>

            {/* Arrow between nodes */}
            {idx < STEPS.length - 1 && (
              <motion.div
                initial={{ opacity: 0, scaleX: 0 }}
                whileInView={{ opacity: 1, scaleX: 1 }}
                viewport={{ once: true }}
                transition={{ delay: idx * 0.12 + 0.1 }}
                className="flex flex-col items-center w-14 shrink-0"
              >
                {/* Arrow line with animated pulse */}
                <div className="relative w-full flex items-center">
                  <div className="flex-1 h-px bg-gradient-to-r from-white/10 to-white/20" />
                  {/* Animated dot */}
                  <motion.div
                    className="absolute left-0 w-2 h-2 rounded-full bg-[#00FFE5]"
                    animate={{ left: ['0%', '100%'] }}
                    transition={{ duration: 1.8, repeat: Infinity, delay: idx * 0.4, ease: 'linear' }}
                    style={{ top: '-3px' }}
                  />
                  <svg className="w-3 h-3 text-white/20 ml-0.5" viewBox="0 0 8 8" fill="currentColor">
                    <path d="M0 3h6l-2-2 1-1 3 4-3 4-1-1 2-2H0z"/>
                  </svg>
                </div>
                <span className="font-mono text-[8px] text-white/20 mt-1">
                  {ARROW_LABELS[idx]}
                </span>
              </motion.div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
