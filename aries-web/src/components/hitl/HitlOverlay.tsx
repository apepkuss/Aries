import { useState, useEffect, useCallback } from 'react';
import { useHitlStore, useActiveHitlRequest, usePendingHitlRequests } from '@/stores';
import { HitlConfirmationDialog } from './HitlConfirmationDialog';
import { PrivacyModeConfirmationDialog } from './PrivacyModeConfirmationDialog';
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
  const { approve, reject, abort, privacyModeChoice, setActiveRequest } = useHitlStore();

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

  // Debug logging
  console.log('[HitlOverlay] Pending requests:', pendingRequests.length);
  console.log('[HitlOverlay] Relevant requests:', relevantRequests.length);
  console.log('[HitlOverlay] Request types:', relevantRequests.map(r => r.request_type.type));

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

  const handlePrivacyModeChoice = useCallback(
    async (requestId: string, usePrivacyMode: boolean, rememberChoice: boolean) => {
      await privacyModeChoice(requestId, usePrivacyMode, rememberChoice);
      setDialogOpenForRequest(null);
    },
    [privacyModeChoice]
  );

  // No pending requests, don't render anything
  if (relevantRequests.length === 0) {
    return null;
  }

  // Separate privacy confirmation requests from regular requests
  const pendingRegularRequests = relevantRequests.filter(
    (r) => r.status === 'pending' && r.request_type.type !== 'privacy_mode_confirmation'
  );
  const pendingPrivacyRequest = relevantRequests.find(
    (r) => r.status === 'pending' && r.request_type.type === 'privacy_mode_confirmation'
  );

  // Privacy confirmation dialog should auto-open (no banner needed)
  const privacyDialogOpen = pendingPrivacyRequest !== undefined;

  console.log('[HitlOverlay] Regular requests:', pendingRegularRequests.length);
  console.log('[HitlOverlay] Privacy request found:', !!pendingPrivacyRequest);
  console.log('[HitlOverlay] Privacy dialog should be open:', privacyDialogOpen);

  return (
    <>
      {/* Notification banners for regular pending requests only */}
      {pendingRegularRequests.length > 0 && (
        <div className={cn('space-y-2', className)}>
          {pendingRegularRequests.map((request) => (
            <HitlNotificationBanner
              key={request.id}
              request={request}
              onApprove={() => handleApprove(request.id)}
              onReject={() => handleReject(request.id)}
              onViewDetails={() => handleViewDetails(request.id)}
            />
          ))}
        </div>
      )}

      {/* Privacy mode confirmation dialog - auto-opens when there's a pending request */}
      {pendingPrivacyRequest && (
        <PrivacyModeConfirmationDialog
          request={pendingPrivacyRequest}
          open={privacyDialogOpen}
          onOpenChange={(open) => {
            // If user closes dialog without choosing, treat as "continue normally"
            if (!open && pendingPrivacyRequest) {
              handlePrivacyModeChoice(pendingPrivacyRequest.id, false, false);
            }
          }}
          onChoosePrivacyMode={handlePrivacyModeChoice}
        />
      )}

      {/* Regular confirmation dialog - only for non-privacy requests */}
      {activeRequest && activeRequest.request_type.type !== 'privacy_mode_confirmation' && (
        <HitlConfirmationDialog
          request={activeRequest}
          open={dialogOpen}
          onOpenChange={handleDialogOpenChange}
          onApprove={handleApprove}
          onReject={handleReject}
          onAbort={handleAbort}
        />
      )}
    </>
  );
}
