/**
 * app/api/ably/token/route.ts
 *
 * Ably token request endpoint.
 *
 * Frontend clients call this endpoint to obtain a short-lived Ably token so
 * the full ABLY_API_KEY never has to be sent to the browser.
 *
 * Usage in client hooks:
 *   const ably = new Ably.Realtime({ authUrl: '/api/ably/token' });
 */

import { NextResponse } from 'next/server';
import Ably from 'ably';

export async function GET() {
  const apiKey = process.env.ABLY_API_KEY;
  if (!apiKey) {
    return NextResponse.json(
      { error: 'Ably is not configured on this server.' },
      { status: 503 }
    );
  }

  try {
    const client = new Ably.Rest({ key: apiKey });

    // Scope the token to subscribe-only on the marketplace channel so the
    // browser cannot accidentally publish server-side events.
    const tokenRequest = await client.auth.createTokenRequest({
      capability: { marketplace: ['subscribe'], 'marketplace:*': ['subscribe'] },
      ttl: 3_600_000, // 1 hour in ms
    });

    return NextResponse.json(tokenRequest);
  } catch (err) {
    console.error('[Ably token] Error generating token request:', err);
    return NextResponse.json(
      { error: 'Failed to generate Ably token.' },
      { status: 500 }
    );
  }
}
