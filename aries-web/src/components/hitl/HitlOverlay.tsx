import { useState, useEffect, useCallback } from 'react';
import { useHitlStore, useActiveHitlRequest, usePendingHitlRequests } from '@/stores';
import { HitlConfirmationDialog } from './HitlConfirmationDialog';
import { HitlNotificationBanner } from './HitlNotificationBanner';
import { cn } from '@/lib/utils';

interface HitlOverlayProps {
  conversationId?: string;
  className?: string;
}

/**
 * HitlOverlay - Manages HITL UI overlay for the chat interface
 *
 * This component:
 * 1. Displays notification banners for pending HITL requests
 * 2. Shows a confirmation dialog for the active request
 * 3. Updates countdown timers in real-time
 */
export function HitlOverlay({ conversationId, className }: HitlOverlayProps) {
  // Track whether user has manually opened the dialog to view details
  const [dialogOpenForRequest, setDialogOpenForRequest] = useState<string | null>(null);

  const activeRequest = useActiveHitlRequest();
  const pendingRequests = usePendingHitlRequests();
  const { approve, reject, abort, setActiveRequest } = useHitlStore();

  // Dialog is open only when user explicitly clicks "View Details"
  // This allows users to quickly approve/reject from the banner without extra clicks
  const dialogOpen =
    activeRequest !== undefined &&
    activeRequest.status === 'pending' &&
    dialogOpenForRequest === activeRequest.id;

  // Countdown timer effect
  useEffect(() => {
    if (pendingRequests.length === 0) return;

    const interval = setInterval(() => {
      // Update remaining seconds for all pending requests
      const { pendingRequests: currentRequests } = useHitlStore.getState();
      const newRequests = new Map(currentRequests);
      let hasChanges = false;

      for (const [id, request] of newRequests) {
        if ((request.remainingSeconds ?? 0) > 0) {
          newRequests.set(id, {
            ...request,
            remainingSeconds: (request.remainingSeconds ?? 0) - 1,
          });
          hasChanges = true;
        }
      }

      if (hasChanges) {
        useHitlStore.setState({ pendingRequests: newRequests });
      }
    }, 1000);

    return () => clearInterval(interval);
  }, [pendingRequests.length]);

  // Filter requests for current conversation
  const relevantRequests = conversationId
    ? pendingRequests.filter((r) => r.conversation_id === conversationId)
    : pendingRequests;

  const handleDialogOpenChange = useCallback(
    (open: boolean) => {
      if (!open) {
        // User closed the dialog
        setDialogOpenForRequest(null);
      }
    },
    []
  );

  const handleApprove = useCallback(
    async (requestId: string) => {
      await approve(requestId);
      setDialogOpenForRequest(null);
    },
    [approve]
  );

  const handleReject = useCallback(
    async (requestId: string) => {
      await reject(requestId);
      setDialogOpenForRequest(null);
    },
    [reject]
  );

  const handleAbort = useCallback(
    async (requestId: string) => {
      await abort(requestId);
      setDialogOpenForRequest(null);
    },
    [abort]
  );

  const handleViewDetails = useCallback(
    (requestId: string) => {
      setActiveRequest(requestId);
      // Open dialog when user clicks "View Details"
      setDialogOpenForRequest(requestId);
    },
    [setActiveRequest]
  );

  // No pending requests, don't render anything
  if (relevantRequests.length === 0) {
    return null;
  }

  return (
    <>
      {/* Notification banners for pending requests */}
      <div className={cn('space-y-2', className)}>
        {relevantRequests
          .filter((r) => r.status === 'pending')
          .map((request) => (
            <HitlNotificationBanner
              key={request.id}
              request={request}
              onApprove={() => handleApprove(request.id)}
              onReject={() => handleReject(request.id)}
              onViewDetails={() => handleViewDetails(request.id)}
            />
          ))}
      </div>

      {/* Confirmation dialog */}
      <HitlConfirmationDialog
        request={activeRequest || null}
        open={dialogOpen}
        onOpenChange={handleDialogOpenChange}
        onApprove={handleApprove}
        onReject={handleReject}
        onAbort={handleAbort}
      />
    </>
  );
}
