import { useState, useEffect, useCallback } from 'react';
import { cn } from '@/lib/utils';
import type { UIHitlRequest } from '@/api/types';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { RiskBadge } from './RiskBadge';
import { OperationPreview } from './OperationPreview';
import {
  CheckIcon,
  XIcon,
  AlertTriangleIcon,
  ClockIcon,
  Loader2Icon,
} from 'lucide-react';

interface HitlConfirmationDialogProps {
  request: UIHitlRequest | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onApprove: (requestId: string) => Promise<void>;
  onReject: (requestId: string) => Promise<void>;
  onAbort?: (requestId: string) => Promise<void>;
  className?: string;
}

export function HitlConfirmationDialog({
  request,
  open,
  onOpenChange,
  onApprove,
  onReject,
  onAbort,
  className,
}: HitlConfirmationDialogProps) {
  const [isSubmitting, setIsSubmitting] = useState(false);

  // Reset state when dialog opens/closes or request changes
  useEffect(() => {
    if (open) {
      setIsSubmitting(false);
    }
  }, [open, request?.id]);

  const handleApprove = useCallback(async () => {
    if (!request) return;
    setIsSubmitting(true);
    try {
      await onApprove(request.id);
      onOpenChange(false);
    } finally {
      setIsSubmitting(false);
    }
  }, [request, onApprove, onOpenChange]);

  const handleReject = useCallback(async () => {
    if (!request) return;
    setIsSubmitting(true);
    try {
      await onReject(request.id);
      onOpenChange(false);
    } finally {
      setIsSubmitting(false);
    }
  }, [request, onReject, onOpenChange]);

  const handleAbort = useCallback(async () => {
    if (!request || !onAbort) return;
    setIsSubmitting(true);
    try {
      await onAbort(request.id);
      onOpenChange(false);
    } finally {
      setIsSubmitting(false);
    }
  }, [request, onAbort, onOpenChange]);

  if (!request) return null;

  // Extract confirmation data
  const confirmData =
    request.request_type.type === 'confirmation' ? request.request_type.data : null;

  const riskLevel = confirmData?.risk_level || 'medium';
  const toolName = confirmData?.tool_name || 'Unknown Tool';
  const preview = confirmData?.preview;
  const description = confirmData?.summary;

  // Determine title based on risk level
  const getTitleByRisk = () => {
    switch (riskLevel) {
      case 'critical':
        return 'Critical Operation Requires Confirmation';
      case 'high':
        return 'High-Risk Operation Requires Confirmation';
      case 'medium':
        return 'Operation Requires Confirmation';
      default:
        return 'Confirm Operation';
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className={cn('max-w-2xl max-h-[90vh] overflow-y-auto', className)}
        showCloseButton={!isSubmitting}
      >
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            {(riskLevel === 'critical' || riskLevel === 'high') && (
              <AlertTriangleIcon
                className={cn(
                  'h-5 w-5',
                  riskLevel === 'critical' ? 'text-red-500' : 'text-orange-500'
                )}
              />
            )}
            {getTitleByRisk()}
          </DialogTitle>
          <DialogDescription className="flex items-center gap-2">
            {/* Subtask identifier badge */}
            {request.subtask_id !== undefined && (
              <span className="text-xs font-medium bg-blue-100 text-blue-700 dark:bg-blue-900/50 dark:text-blue-300 px-1.5 py-0.5 rounded">
                Subtask-{request.subtask_id}
              </span>
            )}
            <span>Tool: {toolName}</span>
            <RiskBadge level={riskLevel} size="sm" />
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
          {/* Countdown Timer - only show when not in 'wait' mode */}
          {request.timeout_behavior !== 'wait' && (request.remainingSeconds ?? 0) > 0 && (
            <div
              className={cn(
                'flex items-center gap-2 rounded-md px-3 py-2 text-sm',
                (request.remainingSeconds ?? 0) <= 10
                  ? 'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400'
                  : (request.remainingSeconds ?? 0) <= 30
                    ? 'bg-yellow-100 text-yellow-700 dark:bg-yellow-900/30 dark:text-yellow-400'
                    : 'bg-muted text-muted-foreground'
              )}
            >
              <ClockIcon className="h-4 w-4" />
              <span>
                {(request.remainingSeconds ?? 0) <= 10 ? 'Expiring soon: ' : 'Time remaining: '}
                {formatTime(request.remainingSeconds ?? 0)}
              </span>
              {request.timeout_behavior && (
                <span className="ml-auto text-xs opacity-70">
                  On timeout: {formatTimeoutBehavior(request.timeout_behavior)}
                </span>
              )}
            </div>
          )}

          {/* Description */}
          {description && (
            <div className="rounded-md bg-muted/50 p-3">
              <p className="text-sm">{description}</p>
            </div>
          )}

          {/* Operation Preview */}
          {preview && <OperationPreview preview={preview} />}
        </div>

        <DialogFooter className="flex-col gap-2 sm:flex-row">
          {/* Approve Button */}
          <Button
            variant={riskLevel === 'critical' ? 'destructive' : 'default'}
            onClick={handleApprove}
            disabled={isSubmitting}
            className="gap-2"
          >
            {isSubmitting ? (
              <Loader2Icon className="h-4 w-4 animate-spin" />
            ) : (
              <CheckIcon className="h-4 w-4" />
            )}
            Approve
          </Button>

          {/* Reject Button */}
          <Button
            variant="outline"
            onClick={handleReject}
            disabled={isSubmitting}
            className="gap-2"
          >
            <XIcon className="h-4 w-4" />
            Reject
          </Button>

          {/* Abort Button (if supported) */}
          {onAbort && (
            <Button
              variant="ghost"
              onClick={handleAbort}
              disabled={isSubmitting}
              className="gap-2 text-red-600 hover:text-red-700 dark:text-red-400"
            >
              <AlertTriangleIcon className="h-4 w-4" />
              Abort Session
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// Helper function to format time
function formatTime(seconds: number): string {
  const mins = Math.floor(seconds / 60);
  const secs = seconds % 60;
  if (mins > 0) {
    return `${mins}m ${secs}s`;
  }
  return `${secs}s`;
}

// Helper function to format timeout behavior
function formatTimeoutBehavior(behavior: string): string {
  switch (behavior) {
    case 'approve':
      return 'Auto-approve';
    case 'reject':
      return 'Auto-reject';
    case 'skip':
      return 'Skip operation';
    case 'wait':
      return 'Keep waiting';
    default:
      return behavior;
  }
}
