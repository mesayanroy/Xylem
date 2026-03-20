# Xylem - pubsub architecture

**Xylem** is a Web3-native AI agent marketplace and builder platform on the Stellar blockchain.

## Overview

Users connect their Freighter wallet, build custom AI agents, monetize them on-chain, and every agent API request is metered and paid using the 0x402 AI-to-AI payment protocol.

## Tech Stack

- **Frontend**: Next.js 14 (App Router), TypeScript, TailwindCSS, Framer Motion
- **Wallet**: Stellar Freighter Wallet SDK (`@stellar/freighter-api`)
- **Blockchain**: Stellar network — smart contracts in Soroban (Rust)
- **Database**: Supabase (PostgreSQL)
- **AI Backends**: OpenAI GPT-4o-mini + Anthropic Claude Haiku
- **Payments**: 0x402 protocol for per-request payments in XLM

## Getting Started

### 1. Install dependencies

```bash
npm install
```

### 2. Configure environment variables

Copy `.env.example` to `.env.local` and fill in your values:

```bash
cp .env.example .env.local
```

### 3. Set up Supabase

Run `supabase-schema.sql` in your Supabase SQL editor.

### 4. Run the development server

```bash
npm run dev
```

Open [http://localhost:3000](http://localhost:3000).

## Pages

| Route | Description |
|-------|-------------|
| `/` | Landing page with hero, stats, features |
| `/agents` | Browse all deployed agents (marketplace grid) |
| `/agents/[id]` | Agent detail with API docs, try it live, fork |
| `/build` | 3-step agent builder wizard |
| `/dashboard` | User dashboard: my agents, earnings in XLM |
| `/marketplace` | Featured + trending agents |
| `/docs` | Developer documentation + 0x402 guide |
| `/devs` | Developer hub: SDK snippets, webhooks |
| `/about` | About the project and architecture |

## 0x402 Payment Flow

1. Client calls `POST /api/agents/{id}/run`
2. Server returns HTTP 402 with payment headers:
   - `X-Payment-Required: xlm`
   - `X-Payment-Amount: {price_xlm}`
   - `X-Payment-Address: {agent_owner_address}`
   - `X-Payment-Network: stellar`
   - `X-Payment-Memo: agent:{id}:req:{nonce}`
3. Client signs XLM payment transaction via Freighter
4. Client retries with `X-Payment-Tx-Hash` header
5. Server verifies via Stellar Horizon API and runs agent

## Smart Contract

The `AgentRegistry` Soroban contract is in `contracts/agent_registry/`. Deploy with:

```bash
chmod +x contracts/deploy.sh
./contracts/deploy.sh
```

## API Routes

- `POST /api/agents/create` — Deploy a new agent
- `GET /api/agents/list` — List public agents
- `GET /api/agents/[id]` — Get agent by ID
- `POST /api/agents/[id]/run` — Run agent (0x402)
- `POST /api/payment/verify` — Verify Stellar tx
