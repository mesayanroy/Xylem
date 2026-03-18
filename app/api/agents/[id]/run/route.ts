import { NextRequest, NextResponse } from 'next/server';
import { v4 as uuidv4 } from 'uuid';
import { createClient } from '@supabase/supabase-js';

const supabaseUrl = process.env.NEXT_PUBLIC_SUPABASE_URL || '';
const supabaseServiceKey = process.env.SUPABASE_SERVICE_ROLE_KEY || process.env.NEXT_PUBLIC_SUPABASE_ANON_KEY || '';

// Mock agent data for demo (used when Supabase is not configured)
const DEMO_AGENTS: Record<string, {
  id: string;
  owner_wallet: string;
  name: string;
  model: string;
  system_prompt: string;
  price_xlm: number;
  is_active: boolean;
}> = {
  '1': {
    id: '1',
    owner_wallet: 'GABC1234567890ABCDEF1234567890ABCDEF1234567890ABCDEF1234XYZ1',
    name: 'DeFi Analyst',
    model: 'openai-gpt4o-mini',
    system_prompt: 'You are a DeFi analyst. Provide concise, insightful analysis.',
    price_xlm: 0.05,
    is_active: true,
  },
};

async function getAgent(agentId: string) {
  if (supabaseUrl && supabaseServiceKey) {
    const supabase = createClient(supabaseUrl, supabaseServiceKey);
    const { data } = await supabase
      .from('agents')
      .select('*')
      .eq('id', agentId)
      .single();
    return data;
  }
  return DEMO_AGENTS[agentId] || null;
}

async function runAgentModel(model: string, systemPrompt: string, userInput: string): Promise<string> {
  if (model === 'openai-gpt4o-mini') {
    const { runOpenAIAgent } = await import('@/lib/openai');
    return runOpenAIAgent(systemPrompt, userInput);
  }
  if (model === 'anthropic-claude-haiku') {
    const { runAnthropicAgent } = await import('@/lib/anthropic');
    return runAnthropicAgent(systemPrompt, userInput);
  }
  return 'Unknown model';
}

async function verifyPayment(
  txHash: string,
  ownerWallet: string,
  priceXlm: number,
  agentId: string
): Promise<boolean> {
  try {
    const { verifyPaymentTransaction } = await import('@/lib/stellar');
    const result = await verifyPaymentTransaction(txHash, ownerWallet, priceXlm, `agent:${agentId}`);
    return result.valid;
  } catch {
    return false;
  }
}

export async function POST(
  req: NextRequest,
  { params }: { params: { id: string } }
) {
  const { id: agentId } = params;
  const startTime = Date.now();

  try {
    const agent = await getAgent(agentId);
    if (!agent) {
      return NextResponse.json({ error: 'Agent not found' }, { status: 404 });
    }
    if (!agent.is_active) {
      return NextResponse.json({ error: 'Agent is not active' }, { status: 403 });
    }

    const body = await req.json();
    const { input } = body;

    if (!input || typeof input !== 'string') {
      return NextResponse.json({ error: 'Missing input field' }, { status: 400 });
    }

    // Check for existing payment
    const paymentTxHash = req.headers.get('X-Payment-Tx-Hash');
    const callerWallet = req.headers.get('X-Payment-Wallet') || '';

    if (agent.price_xlm > 0 && !paymentTxHash) {
      // Issue 402 payment challenge
      const requestNonce = Math.random().toString(36).slice(2, 10);
      // Memo is capped at 28 bytes to match Stellar's limit (same cap applied in PaymentModal)
      const memo = `agent:${agentId}:req:${requestNonce}`.slice(0, 28);

      return NextResponse.json(
        {
          error: 'Payment required',
          payment_details: {
            amount_xlm: agent.price_xlm,
            address: agent.owner_wallet,
            network: 'stellar',
            memo,
          },
        },
        {
          status: 402,
          headers: {
            'X-Payment-Required': 'xlm',
            'X-Payment-Amount': String(agent.price_xlm),
            'X-Payment-Address': agent.owner_wallet,
            'X-Payment-Network': 'stellar',
            'X-Payment-Memo': memo,
          },
        }
      );
    }

    // Verify payment if hash provided
    let paymentVerified = agent.price_xlm === 0;
    if (paymentTxHash && agent.price_xlm > 0) {
      paymentVerified = await verifyPayment(paymentTxHash, agent.owner_wallet, agent.price_xlm, agentId);
      if (!paymentVerified) {
        return NextResponse.json({ error: 'Payment verification failed' }, { status: 402 });
      }
    }

    // Run the agent
    const output = await runAgentModel(agent.model, agent.system_prompt, input);
    const latencyMs = Date.now() - startTime;
    const requestId = uuidv4();

    // Log to database
    if (supabaseUrl && supabaseServiceKey) {
      const supabase = createClient(supabaseUrl, supabaseServiceKey);
      await supabase.from('agent_requests').insert({
        id: requestId,
        agent_id: agentId,
        caller_wallet: callerWallet || null,
        caller_ip: req.headers.get('x-forwarded-for') || null,
        input_payload: { input },
        output_response: { output },
        payment_tx_hash: paymentTxHash,
        payment_amount_xlm: paymentTxHash ? agent.price_xlm : 0,
        protocol: '0x402',
        status: 'success',
        latency_ms: latencyMs,
      });

      // Update agent stats
      await supabase
        .from('agents')
        .update({
          total_requests: agent.total_requests ? agent.total_requests + 1 : 1,
          total_earned_xlm: paymentTxHash
            ? (agent.total_earned_xlm || 0) + agent.price_xlm
            : agent.total_earned_xlm || 0,
          updated_at: new Date().toISOString(),
        })
        .eq('id', agentId);
    }

    return NextResponse.json({
      output,
      request_id: requestId,
      latency_ms: latencyMs,
    });
  } catch (err) {
    console.error('Agent run error:', err);
    return NextResponse.json({ error: 'Internal server error' }, { status: 500 });
  }
}
