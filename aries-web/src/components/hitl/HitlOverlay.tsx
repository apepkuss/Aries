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
  // Track whether user has manually closed the dialog
  const [userClosedDialog, setUserClosedDialog] = useState<string | null>(null);

  const activeRequest = useActiveHitlRequest();
  const pendingRequests = usePendingHitlRequests();
  const { approve, reject, modify, abort, setActiveRequest } = useHitlStore();

  // Dialog is open when there's an active pending request, unless user manually closed it
  // When the active request changes to a new one, the dialog should reopen
  const dialogOpen =
    activeRequest !== undefined &&
    activeRequest.status === 'pending' &&
    userClosedDialog !== activeRequest.id;

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
      if (!open && activeRequest) {
        // User manually closed the dialog
        setUserClosedDialog(activeRequest.id);
      }
    },
    [activeRequest]
  );

  const handleApprove = useCallback(
    async (requestId: string) => {
      await approve(requestId);
      setUserClosedDialog(null);
    },
    [approve]
  );

  const handleReject = useCallback(
    async (requestId: string, reason?: string) => {
      await reject(requestId, reason);
      setUserClosedDialog(null);
    },
    [reject]
  );

  const handleModify = useCallback(
    async (requestId: string, modifications: Record<string, unknown>) => {
      await modify(requestId, modifications);
      setUserClosedDialog(null);
    },
    [modify]
  );

  const handleAbort = useCallback(
    async (requestId: string, reason?: string) => {
      await abort(requestId, reason);
      setUserClosedDialog(null);
    },
    [abort]
  );

  const handleViewDetails = useCallback(
    (requestId: string) => {
      setActiveRequest(requestId);
      setUserClosedDialog(null);
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
        onModify={handleModify}
        onAbort={handleAbort}
      />
    </>
  );
}
