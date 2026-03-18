'use client';

/**
 * hooks/useMarketplaceFeed.ts
 *
 * React hook that subscribes to the Ably "marketplace" channel and returns
 * a live feed of marketplace activity events.
 *
 * Usage:
 *   const { events, isConnected } = useMarketplaceFeed();
 *
 * Drop this into any React page/component to get real-time agent activity
 * without polling.
 */

import { useEffect, useRef, useState, useCallback } from 'react';
import type { MarketplaceActivityEvent } from '@/types/events';

// Ably is loaded lazily (dynamic import) so it doesn't bloat the initial bundle.
const MAX_EVENTS = 50;

export interface UseMarketplaceFeedOptions {
  /** Maximum number of events to retain in memory (default: 50). */
  maxEvents?: number;
  /** Filter to a specific event type. */
  filter?: MarketplaceActivityEvent['eventType'];
  /** If true the hook won't subscribe (e.g. when the component is hidden). */
  paused?: boolean;
}

export interface UseMarketplaceFeedReturn {
  /** Most recent events, newest first. */
  events: MarketplaceActivityEvent[];
  /** Current Ably connection state. */
  connectionState: string;
  /** True when Ably is connected and receiving messages. */
  isConnected: boolean;
  /** Manually clear all buffered events. */
  clearEvents: () => void;
}

export function useMarketplaceFeed(
  options: UseMarketplaceFeedOptions = {}
): UseMarketplaceFeedReturn {
  const { maxEvents = MAX_EVENTS, filter, paused = false } = options;

  const [events, setEvents] = useState<MarketplaceActivityEvent[]>([]);
  const [connectionState, setConnectionState] = useState<string>('initialized');
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const realtimeRef = useRef<any>(null);

  const clearEvents = useCallback(() => setEvents([]), []);

  useEffect(() => {
    if (paused) return;

    let closed = false;

    async function connect() {
      // Dynamic import so this only runs in the browser
      const Ably = (await import('ably')).default;

      if (closed) return;

      const realtime = new Ably.Realtime({
        authUrl: '/api/ably/token',
        autoConnect: true,
      });

      realtimeRef.current = realtime;

      realtime.connection.on((stateChange: { current: string }) => {
        setConnectionState(stateChange.current);
      });

      const channel = realtime.channels.get('marketplace');

      // Subscribe to all events (or just the filtered type)
      const handleMessage = (msg: import('ably').InboundMessage) => {
        const activity = msg.data as MarketplaceActivityEvent;
        if (filter && activity.eventType !== filter) return;

        setEvents((prev) => {
          const next = [activity, ...prev];
          return next.length > maxEvents ? next.slice(0, maxEvents) : next;
        });
      };

      if (filter) {
        channel.subscribe(filter, handleMessage);
      } else {
        channel.subscribe(handleMessage);
      }
    }

    void connect();

    return () => {
      closed = true;
      if (realtimeRef.current) {
        realtimeRef.current.close();
        realtimeRef.current = null;
      }
    };
  }, [paused, filter, maxEvents]);

  return {
    events,
    connectionState,
    isConnected: connectionState === 'connected',
    clearEvents,
  };
}
