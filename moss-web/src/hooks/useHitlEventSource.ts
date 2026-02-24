import { useEffect, useRef } from 'react';
import { useHitlStore, useChatStore } from '@/stores';

const POLL_INTERVAL_MS = 1000; // Poll every 1 second

/**
 * Hook to poll for HITL pending requests.
 * Only polls while the chat is actively streaming to avoid unnecessary requests.
 * Should be called once at app root level.
 */
export function useHitlEventSource() {
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const { fetchPendingRequests } = useHitlStore();
  const isStreaming = useChatStore((s) => s.isStreaming);

  useEffect(() => {
    if (!isStreaming) {
      // Not streaming — stop polling if running
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
        intervalRef.current = null;
      }
      return;
    }

    // Streaming started — begin polling
    let mounted = true;

    const poll = async () => {
      if (!mounted) return;
      try {
        await fetchPendingRequests();
      } catch {
        // Silently ignore polling errors (server might be restarting, etc.)
      }
    };

    // Initial fetch
    poll();

    // Set up polling interval
    intervalRef.current = setInterval(poll, POLL_INTERVAL_MS);

    return () => {
      mounted = false;
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
        intervalRef.current = null;
      }
    };
  }, [isStreaming, fetchPendingRequests]);
}
