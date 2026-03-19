'use client';

import dynamic from 'next/dynamic';
import { motion } from 'framer-motion';
import Link from 'next/link';
import { useEffect, useState } from 'react';
import LiveFeed from '@/components/LiveFeed';
import ProtocolFlow from '@/components/ProtocolFlow';

// Three.js scene — client-only, no SSR
const HeroScene = dynamic(() => import('@/components/HeroScene'), { ssr: false });

// ── Data ──────────────────────────────────────────────────────────────────────

const STATS = [
  { label: 'Agents Deployed',  value: '1,247',   delta: '+12 today'  },
  { label: 'Total Requests',   value: '89,432',   delta: '+2.1k today' },
  { label: 'XLM Earned',       value: '12,450',   delta: '+341 today' },
  { label: 'Active Builders',  value: '342',      delta: '+8 today'   },
];

const AGENT_TEMPLATES = [
  {
    icon:  '⚡',
    title: 'MEV Bot',
    desc:  'Front-running & sandwich detection on Stellar DEX order books with sub-500ms latency.',
    tag:   'HIGH FREQ',
    color: 'from-[#00FFE5]/10 to-transparent',
    border:'border-[rgba(0,255,229,0.15)]',
    href:  '/build?template=mev_bot',
  },
  {
    icon:  '🔺',
    title: 'Arbitrage Tracker',
    desc:  'Triangular & cross-path arbitrage across Stellar DEX with automated execution.',
    tag:   'ARBITRAGE',
    color: 'from-[#7b61ff]/10 to-transparent',
    border:'border-[rgba(123,97,255,0.15)]',
    href:  '/build?template=arb_tracker',
  },
  {
    icon:  '👁',
    title: 'Mempool Monitor',
    desc:  'Real-time Stellar transaction stream analysis via Horizon SSE with configurable alerts.',
    tag:   'MONITORING',
    color: 'from-[#f59e0b]/10 to-transparent',
    border:'border-[rgba(245,158,11,0.15)]',
    href:  '/build?template=mempool_monitor',
  },
  {
    icon:  '📈',
    title: 'Trading Bot',
    desc:  'Buy / sell / short strategies with grid & DCA modes, stop-loss, and take-profit.',
    tag:   'TRADING',
    color: 'from-[#4ade80]/10 to-transparent',
    border:'border-[rgba(74,222,128,0.15)]',
    href:  '/build?template=trading_bot',
  },
  {
    icon:  '🔀',
    title: 'Relayer',
    desc:  'Fee-bump transaction relay with 0x402 micropayment charging. Gasless UX for end users.',
    tag:   'RELAY',
    color: 'from-[#f87171]/10 to-transparent',
    border:'border-[rgba(248,113,113,0.15)]',
    href:  '/build?template=relayer',
  },
  {
    icon:  '💧',
    title: 'Liquidity Tracker',
    desc:  'Order-book depth analysis with real-time slippage simulation across configurable pairs.',
    tag:   'DEFI',
    color: 'from-[#38bdf8]/10 to-transparent',
    border:'border-[rgba(56,189,248,0.15)]',
    href:  '/build?template=liquidity_tracker',
  },
];

const PROTOCOL_FEATURES = [
  {
    icon:  '💳',
    title: '0x402 Protocol',
    desc:  'HTTP 402 pay-per-request. Every agent API call automatically handles the challenge → Stellar payment → retry dance.',
    color: 'text-[#f59e0b]',
  },
  {
    icon:  '🔀',
    title: 'Kafka Pub-Sub',
    desc:  'All agent events stream through 8 Upstash Kafka topics — payments, trades, chain events, billing.',
    color: 'text-[#4ade80]',
  },
  {
    icon:  '⛓️',
    title: 'Soroban On-Chain',
    desc:  'Agent identities and earnings are anchored on Stellar via Soroban smart contracts.',
    color: 'text-[#7b61ff]',
  },
  {
    icon:  '🦀',
    title: 'Rust SDK',
    desc:  'Six production-grade agent templates in Rust — gas-optimised, async, fully typed.',
    color: 'text-[#00FFE5]',
  },
];

// ── Animation variants ────────────────────────────────────────────────────────

const fadeUp = {
  hidden:  { opacity: 0, y: 28 },
  visible: (i: number) => ({
    opacity:    1,
    y:          0,
    transition: { delay: i * 0.09, duration: 0.55, ease: 'easeOut' as const },
  }),
};

// ── Page ──────────────────────────────────────────────────────────────────────

export default function HomePage() {
  const [typedText, setTypedText] = useState('');
  const fullText = 'Build · Deploy · Monetize';

  useEffect(() => {
    let i = 0;
    const timer = setInterval(() => {
      if (i < fullText.length) { setTypedText(fullText.slice(0, i + 1)); i++; }
      else clearInterval(timer);
    }, 55);
    return () => clearInterval(timer);
  }, []);

  return (
    <div className="min-h-screen bg-[#050508] overflow-x-hidden">

      {/* ── HERO ─────────────────────────────────────────────────────────── */}
      <section className="relative min-h-[100svh] flex flex-col items-center justify-center overflow-hidden">

        {/* Three.js canvas — fills the whole hero */}
        <div className="absolute inset-0 z-0">
          <HeroScene />
        </div>

        {/* Radial glow overlays */}
        <div className="absolute inset-0 z-[1] pointer-events-none">
          <div className="absolute top-1/3 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[600px] h-[600px] rounded-full bg-[#00FFE5] opacity-[0.04] blur-[100px]" />
          <div className="absolute top-2/3 left-1/4 w-[400px] h-[400px] rounded-full bg-[#7b61ff] opacity-[0.05] blur-[80px]" />
        </div>

        {/* Hero content */}
        <div className="relative z-[2] max-w-5xl mx-auto px-4 text-center">
          <motion.div
            initial={{ opacity: 0, y: 24 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.7, ease: 'easeOut' as const }}
          >
            {/* Status pill */}
            <div className="inline-flex items-center gap-2.5 px-3.5 py-1.5 rounded-full border border-[rgba(0,255,229,0.2)] bg-[rgba(0,255,229,0.04)] text-[#00FFE5] text-xs font-mono mb-8">
              <span className="w-1.5 h-1.5 rounded-full bg-[#00FFE5] animate-pulse" />
              Live on Stellar Testnet · 0x402 Protocol Active
            </div>

            <h1 className="font-syne text-6xl md:text-8xl font-extrabold tracking-tight text-white mb-5 leading-[1.05]">
              Agent<span className="text-transparent bg-clip-text bg-gradient-to-r from-[#00FFE5] to-[#7b61ff]">Forge</span>
            </h1>

            <h2 className="font-mono text-xl md:text-2xl text-[#00FFE5] mb-7 h-8 tracking-wide">
              {typedText}<span className="animate-pulse opacity-70">_</span>
            </h2>

            <p className="text-white/50 text-lg md:text-xl max-w-2xl mx-auto mb-10 leading-relaxed font-light">
              The Web3-native AI agent marketplace on Stellar. Every API call
              monetized via the <span className="text-[#f59e0b] font-medium">0x402 protocol</span>,
              every event streaming through <span className="text-[#4ade80] font-medium">Kafka pub-sub</span>.
            </p>

            <div className="flex flex-wrap items-center justify-center gap-4">
              <Link
                href="/build"
                className="group relative px-8 py-3.5 font-mono text-sm font-bold rounded-xl overflow-hidden text-black"
              >
                <span className="absolute inset-0 bg-gradient-to-r from-[#00FFE5] to-[#00ccb8] transition-opacity" />
                <span className="absolute inset-0 bg-white opacity-0 group-hover:opacity-10 transition-opacity" />
                <span className="relative">Build Your Agent →</span>
              </Link>
              <Link
                href="/agents"
                className="px-8 py-3.5 font-mono text-sm font-medium border border-[rgba(0,255,229,0.25)] text-[#00FFE5] rounded-xl hover:bg-[rgba(0,255,229,0.06)] transition-all"
              >
                Browse Marketplace
              </Link>
              <Link
                href="/docs"
                className="px-8 py-3.5 font-mono text-sm text-white/40 hover:text-white/70 transition-colors"
              >
                Read Docs
              </Link>
            </div>
          </motion.div>
        </div>

        {/* Scroll indicator */}
        <motion.div
          className="absolute bottom-8 left-1/2 -translate-x-1/2 z-[2] flex flex-col items-center gap-2"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ delay: 1.5 }}
        >
          <span className="font-mono text-[10px] text-white/20 tracking-widest uppercase">scroll</span>
          <motion.div
            className="w-px h-10 bg-gradient-to-b from-white/20 to-transparent"
            animate={{ scaleY: [1, 0.4, 1] }}
            transition={{ duration: 1.6, repeat: Infinity }}
          />
        </motion.div>
      </section>

      {/* ── STATS BAR ────────────────────────────────────────────────────── */}
      <section className="border-y border-white/[0.05] bg-[rgba(0,0,0,0.5)] backdrop-blur-sm py-6">
        <div className="max-w-6xl mx-auto px-4">
          <div className="grid grid-cols-2 md:grid-cols-4 gap-6">
            {STATS.map((stat, i) => (
              <motion.div
                key={stat.label}
                custom={i}
                variants={fadeUp}
                initial="hidden"
                whileInView="visible"
                viewport={{ once: true }}
                className="text-center"
              >
                <div className="font-syne text-3xl font-extrabold text-[#00FFE5] tabular-nums">
                  {stat.value}
                </div>
                <div className="font-mono text-xs text-white/40 mt-1">{stat.label}</div>
                <div className="font-mono text-[10px] text-[#4ade80] mt-0.5">{stat.delta}</div>
              </motion.div>
            ))}
          </div>
        </div>
      </section>

      {/* ── PROTOCOL ARCHITECTURE ────────────────────────────────────────── */}
      <section className="max-w-6xl mx-auto px-4 py-24">
        <motion.div
          initial={{ opacity: 0, y: 24 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          className="text-center mb-14"
        >
          <span className="font-mono text-[11px] text-[#00FFE5] tracking-[0.25em] uppercase">
            Core Architecture
          </span>
          <h2 className="font-syne text-4xl md:text-5xl font-bold text-white mt-3">
            0x402 × Kafka × Stellar
          </h2>
          <p className="text-white/40 font-mono text-sm mt-4 max-w-xl mx-auto">
            The complete payment-to-execution pipeline powering every agent interaction
          </p>
        </motion.div>

        {/* Flow diagram */}
        <div className="mb-16">
          <ProtocolFlow />
        </div>

        {/* Feature cards */}
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
          {PROTOCOL_FEATURES.map((feat, i) => (
            <motion.div
              key={feat.title}
              custom={i}
              variants={fadeUp}
              initial="hidden"
              whileInView="visible"
              viewport={{ once: true }}
              className="group p-5 rounded-2xl border border-white/[0.06] bg-[rgba(255,255,255,0.02)] hover:border-white/[0.12] hover:bg-[rgba(255,255,255,0.04)] transition-all duration-300"
            >
              <span className="text-3xl">{feat.icon}</span>
              <h3 className={`font-mono text-sm font-bold mt-3 ${feat.color}`}>{feat.title}</h3>
              <p className="text-white/40 text-xs leading-relaxed mt-2">{feat.desc}</p>
            </motion.div>
          ))}
        </div>
      </section>

      {/* ── AGENT TEMPLATES ──────────────────────────────────────────────── */}
      <section className="max-w-6xl mx-auto px-4 pb-24">
        <motion.div
          initial={{ opacity: 0, y: 24 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          className="text-center mb-14"
        >
          <span className="font-mono text-[11px] text-[#7b61ff] tracking-[0.25em] uppercase">
            Agent SDK Templates
          </span>
          <h2 className="font-syne text-4xl md:text-5xl font-bold text-white mt-3">
            Production-Grade Rust Agents
          </h2>
          <p className="text-white/40 font-mono text-sm mt-4 max-w-xl mx-auto">
            Battle-hardened templates with 0x402 billing, Kafka events, gas optimisation,
            and Stellar wallet signing baked in.
          </p>
        </motion.div>

        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-5">
          {AGENT_TEMPLATES.map((tmpl, i) => (
            <motion.div
              key={tmpl.title}
              custom={i}
              variants={fadeUp}
              initial="hidden"
              whileInView="visible"
              viewport={{ once: true }}
            >
              <Link href={tmpl.href} className="block h-full group">
                <div
                  className={`h-full p-6 rounded-2xl border ${tmpl.border} bg-gradient-to-br ${tmpl.color} hover:brightness-110 transition-all duration-300 relative overflow-hidden`}
                >
                  {/* Top row */}
                  <div className="flex items-start justify-between mb-4">
                    <span className="text-4xl">{tmpl.icon}</span>
                    <span className="font-mono text-[9px] text-white/30 border border-white/10 px-1.5 py-0.5 rounded tracking-widest">
                      {tmpl.tag}
                    </span>
                  </div>

                  <h3 className="font-syne text-lg font-bold text-white mb-2">{tmpl.title}</h3>
                  <p className="text-white/40 text-sm leading-relaxed">{tmpl.desc}</p>

                  <div className="mt-5 flex items-center gap-1.5 font-mono text-xs text-[#00FFE5] group-hover:gap-3 transition-all">
                    Use template <span>→</span>
                  </div>

                  {/* Hover shimmer */}
                  <div className="absolute inset-0 opacity-0 group-hover:opacity-100 transition-opacity pointer-events-none bg-gradient-to-br from-white/[0.03] to-transparent" />
                </div>
              </Link>
            </motion.div>
          ))}
        </div>
      </section>

      {/* ── LIVE FEED + CODE PREVIEW ──────────────────────────────────────── */}
      <section className="max-w-6xl mx-auto px-4 pb-24">
        <div className="grid grid-cols-1 lg:grid-cols-5 gap-6">

          {/* Live feed — 2 cols */}
          <div className="lg:col-span-2">
            <motion.div
              initial={{ opacity: 0, x: -24 }}
              whileInView={{ opacity: 1, x: 0 }}
              viewport={{ once: true }}
              className="mb-4"
            >
              <span className="font-mono text-[11px] text-[#4ade80] tracking-[0.2em] uppercase">
                Live Network
              </span>
              <h3 className="font-syne text-2xl font-bold text-white mt-1">
                Real-Time Activity
              </h3>
            </motion.div>
            <motion.div
              initial={{ opacity: 0 }}
              whileInView={{ opacity: 1 }}
              viewport={{ once: true }}
              transition={{ delay: 0.2 }}
            >
              <LiveFeed />
            </motion.div>
          </div>

          {/* Code preview — 3 cols */}
          <div className="lg:col-span-3">
            <motion.div
              initial={{ opacity: 0, x: 24 }}
              whileInView={{ opacity: 1, x: 0 }}
              viewport={{ once: true }}
              className="mb-4"
            >
              <span className="font-mono text-[11px] text-[#7b61ff] tracking-[0.2em] uppercase">
                SDK Preview
              </span>
              <h3 className="font-syne text-2xl font-bold text-white mt-1">
                Three Lines to Go Live
              </h3>
            </motion.div>

            <motion.div
              initial={{ opacity: 0 }}
              whileInView={{ opacity: 1 }}
              viewport={{ once: true }}
              transition={{ delay: 0.25 }}
              className="rounded-2xl border border-white/[0.07] bg-[rgba(5,5,8,0.9)] backdrop-blur overflow-hidden font-mono text-sm"
            >
              {/* Window chrome */}
              <div className="flex items-center gap-1.5 px-4 py-3 border-b border-white/[0.05]">
                <span className="w-2.5 h-2.5 rounded-full bg-[#f87171] opacity-70" />
                <span className="w-2.5 h-2.5 rounded-full bg-[#f59e0b] opacity-70" />
                <span className="w-2.5 h-2.5 rounded-full bg-[#4ade80] opacity-70" />
                <span className="ml-3 text-[11px] text-white/20">mev_bot / src / main.rs</span>
              </div>

              <pre className="p-5 overflow-x-auto text-[13px] leading-7">
<code><span className="text-[#7b61ff]">use</span> <span className="text-white/80">common::</span><span className="text-[#00FFE5]">{"{"}</span><span className="text-white">HorizonClient, Keypair, PaymentClient, KafkaPublisher</span><span className="text-[#00FFE5]">{"}"}</span><span className="text-white/40">;</span>{"\n"}
<span className="text-white/40">{"// "}</span><span className="text-white/30">Load wallet from secret key (Strkey S...)</span>{"\n"}
<span className="text-[#7b61ff]">let</span> <span className="text-white">keypair</span> <span className="text-white/40">=</span> <span className="text-[#4ade80]">Keypair::from_secret</span><span className="text-[#00FFE5]">(</span><span className="text-white/50">&cfg.agent_secret</span><span className="text-[#00FFE5]">)</span><span className="text-white/40">?;</span>{"\n\n"}
<span className="text-white/40">{"// "}</span><span className="text-white/30">0x402 client — auto-handles HTTP 402 ↔ Stellar payment</span>{"\n"}
<span className="text-[#7b61ff]">let</span> <span className="text-white">pay</span> <span className="text-white/40">=</span> <span className="text-[#4ade80]">PaymentClient::new</span><span className="text-[#00FFE5]">(</span><span className="text-white/80">keypair.clone(), &horizon_url, &passphrase</span><span className="text-[#00FFE5]">)</span><span className="text-white/40">?;</span>{"\n\n"}
<span className="text-white/40">{"// "}</span><span className="text-white/30">Kafka publisher — fire-and-forget trade events</span>{"\n"}
<span className="text-[#7b61ff]">let</span> <span className="text-white">kafka</span> <span className="text-white/40">=</span> <span className="text-[#4ade80]">KafkaPublisher::from_env</span><span className="text-[#00FFE5]">()</span><span className="text-white/40">;</span>{"\n\n"}
<span className="text-white/40">{"// "}</span><span className="text-white/30">Run — scans order books, executes MEV, publishes events</span>{"\n"}
<span className="text-[#4ade80]">strategy::scan_loop</span><span className="text-[#00FFE5]">(&cfg, &horizon, &keypair, &pay, &kafka)</span><span className="text-white/40">.</span><span className="text-[#7b61ff]">await</span></code>
              </pre>

              <div className="px-5 pb-4 flex items-center gap-3">
                <Link href="/docs/sdk" className="font-mono text-[11px] text-[#00FFE5] hover:underline">
                  Full SDK docs →
                </Link>
                <span className="text-white/10">·</span>
                <Link href="/build" className="font-mono text-[11px] text-white/30 hover:text-white/60 transition-colors">
                  Use template →
                </Link>
              </div>
            </motion.div>
          </div>
        </div>
      </section>

      {/* ── CTA BANNER ───────────────────────────────────────────────────── */}
      <section className="max-w-6xl mx-auto px-4 pb-28">
        <motion.div
          initial={{ opacity: 0, y: 24 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          className="relative rounded-3xl border border-white/[0.08] overflow-hidden"
        >
          {/* Background gradient */}
          <div className="absolute inset-0 bg-gradient-to-br from-[rgba(0,255,229,0.05)] via-[rgba(123,97,255,0.05)] to-[rgba(0,0,0,0.8)]" />
          <div className="absolute top-0 left-1/2 -translate-x-1/2 w-[500px] h-[1px] bg-gradient-to-r from-transparent via-[#00FFE5]/40 to-transparent" />

          <div className="relative px-8 py-14 text-center">
            <span className="font-mono text-[11px] text-[#00FFE5] tracking-[0.25em] uppercase">
              Ship Today
            </span>
            <h2 className="font-syne text-4xl md:text-5xl font-extrabold text-white mt-4 mb-4">
              From idea to on-chain revenue<br className="hidden md:block" /> in minutes.
            </h2>
            <p className="text-white/40 font-mono text-sm mb-10 max-w-lg mx-auto">
              Connect your Freighter wallet, pick a Rust template, configure strategy params,
              deploy on Soroban — and start earning XLM per request automatically via 0x402.
            </p>

            <div className="flex flex-wrap items-center justify-center gap-4">
              <Link
                href="/build"
                className="group relative px-10 py-4 font-mono text-sm font-bold rounded-xl overflow-hidden text-black"
              >
                <span className="absolute inset-0 bg-gradient-to-r from-[#00FFE5] to-[#00ccb8]" />
                <span className="absolute inset-0 bg-white opacity-0 group-hover:opacity-10 transition-opacity" />
                <span className="relative">Start Building →</span>
              </Link>
              <Link
                href="/agents"
                className="px-10 py-4 font-mono text-sm font-medium border border-white/10 text-white/50 rounded-xl hover:text-white hover:border-white/20 transition-all"
              >
                Browse Agents
              </Link>
            </div>
          </div>
        </motion.div>
      </section>

    </div>
  );
}
