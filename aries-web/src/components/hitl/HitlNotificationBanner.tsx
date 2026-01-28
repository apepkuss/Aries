import { cn } from '@/lib/utils';
import type { UIHitlRequest, HitlRiskLevel } from '@/api/types';
import { Button } from '@/components/ui/button';
import { RiskBadge } from './RiskBadge';
import {
  AlertTriangleIcon,
  CheckIcon,
  XIcon,
  ClockIcon,
  ChevronRightIcon,
  Loader2Icon,
} from 'lucide-react';

interface HitlNotificationBannerProps {
  request: UIHitlRequest;
  onApprove?: () => void;
  onReject?: () => void;
  onViewDetails?: () => void;
  className?: string;
}

export function HitlNotificationBanner({
  request,
  onApprove,
  onReject,
  onViewDetails,
  className,
}: HitlNotificationBannerProps) {
  const confirmData =
    request.request_type.type === 'confirmation' ? request.request_type.data : null;

  const riskLevel = confirmData?.risk_level || 'medium';
  const description = confirmData?.summary;

  return (
    <div
      className={cn(
        'rounded-lg border p-4',
        getBannerStyles(riskLevel),
        request.isResponding && 'opacity-70',
        className
      )}
    >
      <div className="flex items-start gap-3">
        {/* Icon */}
        <div className={cn('shrink-0 mt-0.5', getIconColor(riskLevel))}>
          <AlertTriangleIcon className="h-5 w-5" />
        </div>

        {/* Content */}
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 flex-wrap">
            {/* Subtask identifier badge */}
            {request.subtask_id !== undefined && (
              <span className="text-xs font-medium bg-blue-100 text-blue-700 dark:bg-blue-900/50 dark:text-blue-300 px-1.5 py-0.5 rounded">
                Subtask-{request.subtask_id}
              </span>
            )}
            {description && <span className="font-medium">{description}</span>}
            <RiskBadge level={riskLevel} size="sm" />
          </div>

          {/* Timer - only show when not in 'wait' mode */}
          {request.timeout_behavior !== 'wait' && (request.remainingSeconds ?? 0) > 0 && (
            <div
              className={cn(
                'flex items-center gap-1.5 text-xs mt-2',
                (request.remainingSeconds ?? 0) <= 10
                  ? 'text-red-600 dark:text-red-400 font-medium'
                  : 'text-muted-foreground'
              )}
            >
              <ClockIcon className="h-3 w-3" />
              <span>
                {(request.remainingSeconds ?? 0) <= 10 ? 'Expiring: ' : ''}
                {formatTime(request.remainingSeconds ?? 0)}
              </span>
              {request.timeout_behavior && (
                <span className="opacity-70">
                  ({formatTimeoutBehavior(request.timeout_behavior)})
                </span>
              )}
            </div>
          )}
        </div>

        {/* Actions */}
        <div className="flex items-center gap-2 shrink-0">
          {onApprove && (
            <Button
              size="sm"
              variant={riskLevel === 'critical' ? 'destructive' : 'default'}
              onClick={onApprove}
              disabled={request.isResponding}
              className="gap-1"
            >
              {request.isResponding ? (
                <Loader2Icon className="h-3 w-3 animate-spin" />
              ) : (
                <CheckIcon className="h-3 w-3" />
              )}
              Approve
            </Button>
          )}

          {onReject && (
            <Button
              size="sm"
              variant="outline"
              onClick={onReject}
              disabled={request.isResponding}
              className="gap-1"
            >
              <XIcon className="h-3 w-3" />
              Reject
            </Button>
          )}

          {onViewDetails && (
            <Button
              size="sm"
              variant="ghost"
              onClick={onViewDetails}
              disabled={request.isResponding}
            >
              <ChevronRightIcon className="h-4 w-4" />
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}

// Get banner background/border styles based on risk level
function getBannerStyles(riskLevel: HitlRiskLevel): string {
  switch (riskLevel) {
    case 'critical':
      return 'bg-red-50 border-red-200 dark:bg-red-950/30 dark:border-red-900';
    case 'high':
      return 'bg-orange-50 border-orange-200 dark:bg-orange-950/30 dark:border-orange-900';
    case 'medium':
      return 'bg-yellow-50 border-yellow-200 dark:bg-yellow-950/30 dark:border-yellow-900';
    case 'low':
    default:
      return 'bg-blue-50 border-blue-200 dark:bg-blue-950/30 dark:border-blue-900';
  }
}

// Get icon color based on risk level
function getIconColor(riskLevel: HitlRiskLevel): string {
  switch (riskLevel) {
    case 'critical':
      return 'text-red-600 dark:text-red-400';
    case 'high':
      return 'text-orange-600 dark:text-orange-400';
    case 'medium':
      return 'text-yellow-600 dark:text-yellow-400';
    case 'low':
    default:
      return 'text-blue-600 dark:text-blue-400';
  }
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
      return 'auto-approve on timeout';
    case 'reject':
      return 'auto-reject on timeout';
    case 'skip':
      return 'skip on timeout';
    case 'wait':
      return 'wait indefinitely';
    default:
      return behavior;
  }
}
