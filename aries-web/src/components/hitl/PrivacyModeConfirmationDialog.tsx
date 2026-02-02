import { useState, useEffect, useCallback } from 'react';
import { cn } from '@/lib/utils';
import type { UIHitlRequest, HitlPrivacyModeConfirmationRequest } from '@/api/types';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import { Label } from '@/components/ui/label';
import {
  ShieldIcon,
  ShieldAlertIcon,
  MessageSquareIcon,
  ClockIcon,
  Loader2Icon,
} from 'lucide-react';

interface PrivacyModeConfirmationDialogProps {
  request: UIHitlRequest | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onChoosePrivacyMode: (requestId: string, usePrivacyMode: boolean, rememberChoice: boolean) => Promise<void>;
  className?: string;
}

export function PrivacyModeConfirmationDialog({
  request,
  open,
  onOpenChange,
  onChoosePrivacyMode,
  className,
}: PrivacyModeConfirmationDialogProps) {
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [rememberChoice, setRememberChoice] = useState(false);

  // Reset state when dialog opens/closes or request changes
  useEffect(() => {
    if (open) {
      setIsSubmitting(false);
      setRememberChoice(false);
    }
  }, [open, request?.id]);

  const handleChoosePrivacyMode = useCallback(async (usePrivacyMode: boolean) => {
    if (!request) return;
    setIsSubmitting(true);
    try {
      await onChoosePrivacyMode(request.id, usePrivacyMode, rememberChoice);
      onOpenChange(false);
    } finally {
      setIsSubmitting(false);
    }
  }, [request, onChoosePrivacyMode, onOpenChange, rememberChoice]);

  if (!request) return null;

  // Extract privacy mode confirmation data
  const privacyData =
    request.request_type.type === 'privacy_mode_confirmation'
      ? (request.request_type.data as HitlPrivacyModeConfirmationRequest)
      : null;

  if (!privacyData) return null;

  const { query_summary, detected_patterns, confidence, recommendation } = privacyData;

  // Format confidence as percentage
  const confidencePercent = Math.round(confidence * 100);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className={cn('max-w-lg max-h-[90vh] overflow-y-auto', className)}
        showCloseButton={!isSubmitting}
      >
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <ShieldAlertIcon className="h-5 w-5 text-amber-500" />
            Privacy Content Detected
          </DialogTitle>
          <DialogDescription>
            Your message may contain sensitive information. Please choose how to proceed.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
          {/* Countdown Timer */}
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
            </div>
          )}

          {/* Query Summary */}
          <div className="rounded-md bg-muted/50 p-3">
            <p className="text-sm text-muted-foreground mb-1">Query Preview</p>
            <p className="text-sm">{query_summary}</p>
          </div>

          {/* Detected Patterns */}
          {detected_patterns.length > 0 && (
            <div className="space-y-2">
              <p className="text-sm font-medium">Detected Sensitive Content:</p>
              <div className="flex flex-wrap gap-2">
                {detected_patterns.map((pattern, index) => (
                  <span
                    key={index}
                    className="inline-flex items-center gap-1 px-2 py-1 rounded-full text-xs bg-amber-100 text-amber-800 dark:bg-amber-900/30 dark:text-amber-300"
                    title={pattern.description}
                  >
                    <ShieldAlertIcon className="h-3 w-3" />
                    {pattern.category}
                  </span>
                ))}
              </div>
            </div>
          )}

          {/* Confidence */}
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <span>Detection Confidence:</span>
            <div className="flex-1 h-2 bg-muted rounded-full overflow-hidden">
              <div
                className={cn(
                  'h-full transition-all',
                  confidencePercent >= 80 ? 'bg-red-500' :
                  confidencePercent >= 60 ? 'bg-amber-500' : 'bg-green-500'
                )}
                style={{ width: `${confidencePercent}%` }}
              />
            </div>
            <span className="font-medium">{confidencePercent}%</span>
          </div>

          {/* Recommendation */}
          {recommendation && (
            <div className="rounded-md bg-blue-50 dark:bg-blue-900/20 p-3 text-sm text-blue-700 dark:text-blue-300">
              <p className="font-medium mb-1">Recommendation</p>
              <p>{recommendation}</p>
            </div>
          )}

          {/* Mode Explanations */}
          <div className="grid grid-cols-2 gap-3">
            <div className="rounded-md border border-emerald-200 dark:border-emerald-800 p-3 space-y-2">
              <div className="flex items-center gap-2 text-emerald-600 dark:text-emerald-400">
                <ShieldIcon className="h-4 w-4" />
                <span className="font-medium text-sm">Privacy Mode</span>
              </div>
              <p className="text-xs text-muted-foreground">
                Uses a local or on-premise AI model. Your data stays private and is not sent to external services.
              </p>
            </div>
            <div className="rounded-md border border-gray-200 dark:border-gray-700 p-3 space-y-2">
              <div className="flex items-center gap-2 text-gray-600 dark:text-gray-400">
                <MessageSquareIcon className="h-4 w-4" />
                <span className="font-medium text-sm">Normal Mode</span>
              </div>
              <p className="text-xs text-muted-foreground">
                Uses cloud-based AI for best performance. Data may be processed by external services.
              </p>
            </div>
          </div>

          {/* Remember Choice Switch */}
          <div className="flex items-center space-x-2">
            <Switch
              id="remember-choice"
              checked={rememberChoice}
              onCheckedChange={setRememberChoice}
              disabled={isSubmitting}
            />
            <Label
              htmlFor="remember-choice"
              className="text-sm text-muted-foreground cursor-pointer"
            >
              Remember my choice for this session
            </Label>
          </div>
        </div>

        <DialogFooter className="flex-col gap-2 sm:flex-row">
          {/* Privacy Mode Button */}
          <Button
            variant="default"
            onClick={() => handleChoosePrivacyMode(true)}
            disabled={isSubmitting}
            className="gap-2 bg-emerald-600 hover:bg-emerald-700 text-white"
          >
            {isSubmitting ? (
              <Loader2Icon className="h-4 w-4 animate-spin" />
            ) : (
              <ShieldIcon className="h-4 w-4" />
            )}
            Use Privacy Mode
          </Button>

          {/* Normal Mode Button */}
          <Button
            variant="outline"
            onClick={() => handleChoosePrivacyMode(false)}
            disabled={isSubmitting}
            className="gap-2"
          >
            <MessageSquareIcon className="h-4 w-4" />
            Continue Normally
          </Button>
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
