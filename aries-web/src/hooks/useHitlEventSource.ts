import { useEffect, useRef } from 'react';
import { useHitlStore } from '@/stores';

const POLL_INTERVAL_MS = 1000; // Poll every 1 second

/**
 * Hook to poll for HITL pending requests.
 * Uses polling instead of SSE since the backend doesn't expose an SSE endpoint.
 * Should be called once at app root level.
 */
export function useHitlEventSource() {
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const { fetchPendingRequests } = useHitlStore();

  useEffect(() => {
    let mounted = true;

    const poll = async () => {
      if (!mounted) return;
      try {
        await fetchPendingRequests();
      } catch (err) {
        // Silently ignore polling errors (server might be restarting, etc.)
        console.debug('[HITL Poll] Error:', err);
      }
    };

    // Initial fetch
    poll();

    // Set up polling interval
    intervalRef.current = setInterval(poll, POLL_INTERVAL_MS);
    console.log('[HITL Poll] Started polling every', POLL_INTERVAL_MS, 'ms');

    return () => {
      mounted = false;
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
        intervalRef.current = null;
        console.log('[HITL Poll] Stopped polling');
      }
    };
  }, [fetchPendingRequests]);
}
