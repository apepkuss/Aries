import { cn } from '@/lib/utils';
import type { UIHitlRequest } from '@/api/types';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { RiskBadge } from './RiskBadge';
import { HitlStatusIndicator } from './HitlStatusIndicator';
import {
  ChevronDownIcon,
  ChevronUpIcon,
  CheckIcon,
  XIcon,
  ClockIcon,
  TerminalIcon,
  FileIcon,
  GlobeIcon,
  HelpCircleIcon,
  MessageSquareIcon,
  PauseIcon,
} from 'lucide-react';

interface HitlRequestCardProps {
  request: UIHitlRequest;
  isActive?: boolean;
  onSelect?: () => void;
  onApprove?: () => void;
  onReject?: () => void;
  onToggleExpand?: () => void;
  className?: string;
}

export function HitlRequestCard({
  request,
  isActive = false,
  onSelect,
  onApprove,
  onReject,
  onToggleExpand,
  className,
}: HitlRequestCardProps) {
  const requestType = request.request_type.type;
  const confirmData =
    requestType === 'confirmation' ? request.request_type.data : null;

  // Get icon and title based on request type
  const getTypeInfo = () => {
    switch (requestType) {
      case 'confirmation':
        // Determine icon based on preview type
        if (confirmData?.preview) {
          switch (confirmData.preview.type) {
            case 'file':
              return { icon: FileIcon, title: 'File Operation' };
            case 'shell':
              return { icon: TerminalIcon, title: 'Shell Command' };
            case 'http':
              return { icon: GlobeIcon, title: 'HTTP Request' };
            default:
              return { icon: HelpCircleIcon, title: 'Operation' };
          }
        }
        return { icon: HelpCircleIcon, title: confirmData?.tool_name || 'Confirmation' };
      case 'clarification':
        return { icon: MessageSquareIcon, title: 'Clarification Needed' };
      case 'feedback':
        return { icon: MessageSquareIcon, title: 'Feedback Request' };
      case 'pause':
        return { icon: PauseIcon, title: 'Execution Paused' };
      default:
        return { icon: HelpCircleIcon, title: 'Request' };
    }
  };

  const { icon: TypeIcon, title } = getTypeInfo();
  const riskLevel = confirmData?.risk_level || 'medium';
  const description = confirmData?.summary || getDefaultDescription(request);

  return (
    <Card
      className={cn(
        'transition-all cursor-pointer',
        isActive && 'ring-2 ring-primary',
        request.isResponding && 'opacity-70',
        className
      )}
      onClick={onSelect}
    >
      <CardHeader className="pb-2">
        <div className="flex items-start justify-between gap-2">
          <CardTitle className="flex items-center gap-2 text-sm">
            <TypeIcon className="h-4 w-4" />
            <span className="truncate">{title}</span>
          </CardTitle>

          <div className="flex items-center gap-2 shrink-0">
            {requestType === 'confirmation' && (
              <RiskBadge level={riskLevel} size="sm" />
            )}
            <HitlStatusIndicator status={request.status} size="sm" />
          </div>
        </div>
      </CardHeader>

      <CardContent className="space-y-3">
        {/* Description */}
        {description && (
          <p className="text-sm text-muted-foreground line-clamp-2">{description}</p>
        )}

        {/* Timer */}
        {(request.remainingSeconds ?? 0) > 0 && request.status === 'pending' && (
          <div
            className={cn(
              'flex items-center gap-1.5 text-xs',
              (request.remainingSeconds ?? 0) <= 10
                ? 'text-red-600 dark:text-red-400'
                : (request.remainingSeconds ?? 0) <= 30
                  ? 'text-yellow-600 dark:text-yellow-400'
                  : 'text-muted-foreground'
            )}
          >
            <ClockIcon className="h-3 w-3" />
            <span>{formatTime(request.remainingSeconds ?? 0)}</span>
          </div>
        )}

        {/* Expanded content */}
        {request.expanded && confirmData?.tool_args && (
          <div className="mt-2 rounded-md bg-muted p-2">
            <div className="text-xs text-muted-foreground mb-1">Arguments:</div>
            <pre className="text-xs font-mono overflow-x-auto">
              {JSON.stringify(confirmData.tool_args, null, 2)}
            </pre>
          </div>
        )}

        {/* Action buttons */}
        {request.status === 'pending' && (
          <div className="flex items-center gap-2 pt-2">
            {onApprove && (
              <Button
                size="sm"
                variant={riskLevel === 'critical' ? 'destructive' : 'default'}
                onClick={(e) => {
                  e.stopPropagation();
                  onApprove();
                }}
                disabled={request.isResponding}
                className="gap-1"
              >
                <CheckIcon className="h-3 w-3" />
                Approve
              </Button>
            )}

            {onReject && (
              <Button
                size="sm"
                variant="outline"
                onClick={(e) => {
                  e.stopPropagation();
                  onReject();
                }}
                disabled={request.isResponding}
                className="gap-1"
              >
                <XIcon className="h-3 w-3" />
                Reject
              </Button>
            )}

            {onToggleExpand && (
              <Button
                size="sm"
                variant="ghost"
                onClick={(e) => {
                  e.stopPropagation();
                  onToggleExpand();
                }}
                className="ml-auto"
              >
                {request.expanded ? (
                  <ChevronUpIcon className="h-4 w-4" />
                ) : (
                  <ChevronDownIcon className="h-4 w-4" />
                )}
              </Button>
            )}
          </div>
        )}
      </CardContent>
    </Card>
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

// Helper function to get default description
function getDefaultDescription(request: UIHitlRequest): string {
  switch (request.request_type.type) {
    case 'confirmation':
      return 'Please review and confirm this operation.';
    case 'clarification':
      return 'Additional information is needed to proceed.';
    case 'feedback':
      return 'Your feedback is requested.';
    case 'pause':
      return 'Execution has been paused. Resume when ready.';
    default:
      return 'Action required.';
  }
}
