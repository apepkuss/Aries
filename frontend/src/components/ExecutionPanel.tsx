import { useState, useEffect } from 'react';
import {
  Wrench,
  CheckCircle2,
  XCircle,
  Loader2,
  ChevronRight,
  ChevronDown,
  Clock,
  Zap,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { ThoughtBubble } from './ThoughtBubble';
import type {
  ExecutionStep,
  ThoughtEvent,
  ToolCallEvent,
  ToolResultEvent,
  StatusEvent,
  ExecutionPhase,
} from '../types/execution';

interface ExecutionPanelProps {
  steps: ExecutionStep[];
  phase: ExecutionPhase | null;
  statusMessage: string | null;
  subtaskProgress: { current: number; total: number } | null;
  isExecuting: boolean;
  defaultCollapsed?: boolean;
}

/**
 * Panel showing execution transparency information.
 * Displays thoughts, tool calls, and status updates during Plan mode execution.
 */
export function ExecutionPanel({
  steps,
  phase,
  statusMessage,
  subtaskProgress,
  isExecuting,
  defaultCollapsed = false,
}: ExecutionPanelProps) {
  const [isCollapsed, setIsCollapsed] = useState(defaultCollapsed);

  // Auto-collapse when execution finishes
  useEffect(() => {
    if (!isExecuting && steps.length > 0) {
      setIsCollapsed(true);
    } else if (isExecuting) {
      setIsCollapsed(false);
    }
  }, [isExecuting, steps.length]);

  if (steps.length === 0 && !isExecuting) {
    return null;
  }

  return (
    <div className={cn(
      "w-full bg-card/40 border border-border rounded-xl overflow-hidden shadow-sm backdrop-blur-sm transition-all duration-300",
      isCollapsed ? "max-h-12" : "max-h-[800px]"
    )}>
      {/* Header - Clickable Toggle */}
      <button
        onClick={() => setIsCollapsed(!isCollapsed)}
        className="w-full flex items-center justify-between px-4 py-3 hover:bg-muted/30 transition-colors text-left border-none outline-none group"
      >
        <div className="flex items-center gap-2">
          <Zap className={cn("w-4 h-4", isExecuting ? "text-primary animate-pulse" : "text-muted-foreground")} />
          <span className="text-sm font-medium text-foreground/80 flex items-center gap-2">
            Execution Details
            {isCollapsed && !isExecuting && steps.length > 0 && (
              <span className="text-[10px] text-muted-foreground font-normal lowercase opacity-70">
                — {steps.length} {steps.length === 1 ? 'step' : 'steps'} processed
              </span>
            )}
          </span>
        </div>
        <div className="flex items-center gap-3">
          {isExecuting && (
            <div className="flex items-center gap-2 px-2 py-0.5 rounded-full bg-primary/10 border border-primary/20">
              <Loader2 className="w-3 h-3 animate-spin text-primary" />
              <span className="text-[10px] font-medium text-primary hidden sm:inline">
                {phase === 'planning'
                  ? 'Planning...'
                  : phase === 'executing'
                  ? `Executing${subtaskProgress ? ` (${subtaskProgress.current}/${subtaskProgress.total})` : ''}...`
                  : phase === 'reflecting'
                  ? 'Reflecting...'
                  : 'Completing...'}
              </span>
            </div>
          )}
          <div className="p-1 rounded-md group-hover:bg-muted/50 transition-colors">
            {isCollapsed ? (
              <ChevronRight className="w-4 h-4 text-muted-foreground" />
            ) : (
              <ChevronDown className="w-4 h-4 text-muted-foreground" />
            )}
          </div>
        </div>
      </button>

      {/* Content Area */}
      <div className={cn(
        "transition-all duration-300 ease-in-out",
        isCollapsed ? "h-0 opacity-0 overflow-hidden" : "h-auto opacity-100 border-t border-border"
      )}>
        {/* Status Message */}
        {statusMessage && (
          <div className="px-4 py-2 bg-primary/5 border-b border-border">
            <p className="text-xs text-primary font-medium">{statusMessage}</p>
          </div>
        )}

        <div className="max-h-[450px] overflow-y-auto">
          <div className="p-4 space-y-3">
            {steps.map((step, index) => (
              <ExecutionStepItem
                key={step.id}
                step={step}
                isLatest={index === steps.length - 1}
                isExecuting={isExecuting}
              />
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}

interface ExecutionStepItemProps {
  step: ExecutionStep;
  isLatest: boolean;
  isExecuting: boolean;
}

function ExecutionStepItem({ step, isLatest, isExecuting }: ExecutionStepItemProps) {
  switch (step.type) {
    case 'thought':
      return <ThoughtStepItem step={step} isStreaming={isLatest && isExecuting} />;
    case 'tool_call':
      return <ToolCallStepItem step={step} />;
    case 'tool_result':
      return <ToolResultStepItem step={step} />;
    case 'status':
      return <StatusStepItem step={step} isLatest={isLatest} isExecuting={isExecuting} />;
    default:
      return null;
  }
}

function ThoughtStepItem({
  step,
  isStreaming = false,
}: {
  step: ExecutionStep;
  isStreaming?: boolean;
}) {
  const data = step.data as ThoughtEvent;
  const isComplete = data.status === 'done';

  return (
    <ThoughtBubble
      content={data.content}
      isStreaming={isStreaming && !isComplete}
      isComplete={isComplete}
      iteration={step.iteration}
      enableTypewriter={isStreaming}
    />
  );
}

function ToolCallStepItem({ step }: { step: ExecutionStep }) {
  const data = step.data as ToolCallEvent;
  const args =
    typeof data.args === 'object' ? JSON.stringify(data.args, null, 2) : String(data.args);

  return (
    <div className="flex gap-3 group">
      <div className="flex-shrink-0 w-6 h-6 rounded-full bg-blue-500/10 flex items-center justify-center">
        <Wrench className="w-3.5 h-3.5 text-blue-500" />
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 mb-1">
          <span className="text-xs font-medium text-blue-500">Tool Call</span>
          <ChevronRight className="w-3 h-3 text-muted-foreground" />
          <code className="text-xs font-mono text-foreground bg-muted px-1.5 py-0.5 rounded">
            {data.tool_name}
          </code>
          {data.server_name && (
            <span className="text-xs text-muted-foreground">
              @ {data.server_name}
            </span>
          )}
        </div>
        {args && args !== '{}' && (
          <pre className="text-xs text-muted-foreground bg-muted/50 p-2 rounded-lg overflow-x-auto max-h-[100px]">
            {args}
          </pre>
        )}
      </div>
    </div>
  );
}

function ToolResultStepItem({ step }: { step: ExecutionStep }) {
  const data = step.data as ToolResultEvent;
  const isError = data.is_error;
  const truncatedResult =
    data.result.length > 200 ? data.result.slice(0, 200) + '...' : data.result;

  return (
    <div className="flex gap-3 group">
      <div
        className={cn(
          'flex-shrink-0 w-6 h-6 rounded-full flex items-center justify-center',
          isError ? 'bg-red-500/10' : 'bg-green-500/10'
        )}
      >
        {isError ? (
          <XCircle className="w-3.5 h-3.5 text-red-500" />
        ) : (
          <CheckCircle2 className="w-3.5 h-3.5 text-green-500" />
        )}
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 mb-1">
          <span
            className={cn(
              'text-xs font-medium',
              isError ? 'text-red-500' : 'text-green-500'
            )}
          >
            {isError ? 'Error' : 'Result'}
          </span>
          {data.duration_ms && (
            <div className="flex items-center gap-1 text-xs text-muted-foreground">
              <Clock className="w-3 h-3" />
              {data.duration_ms}ms
            </div>
          )}
        </div>
        <pre
          className={cn(
            'text-xs p-2 rounded-lg overflow-x-auto max-h-[100px]',
            isError
              ? 'text-red-500/80 bg-red-500/5'
              : 'text-muted-foreground bg-muted/50'
          )}
        >
          {truncatedResult}
        </pre>
      </div>
    </div>
  );
}

function StatusStepItem({
  step,
  isLatest,
  isExecuting,
}: {
  step: ExecutionStep;
  isLatest: boolean;
  isExecuting: boolean;
}) {
  const data = step.data as StatusEvent;
  const isActive = isLatest && isExecuting;

  const phaseColors: Record<ExecutionPhase, string> = {
    planning: 'text-amber-500 bg-amber-500/10',
    executing: 'text-blue-500 bg-blue-500/10',
    reflecting: 'text-purple-500 bg-purple-500/10',
    completing: 'text-green-500 bg-green-500/10',
  };

  return (
    <div className="flex gap-3 group">
      <div
        className={cn(
          'flex-shrink-0 w-6 h-6 rounded-full flex items-center justify-center',
          phaseColors[data.phase]
        )}
      >
        {isActive ? (
          <Loader2 className="w-3.5 h-3.5 animate-spin" />
        ) : (
          <CheckCircle2 className="w-3.5 h-3.5" />
        )}
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className={cn('text-xs font-medium capitalize', phaseColors[data.phase].split(' ')[0])}>
            {data.phase}
          </span>
          {data.subtask_current && data.subtask_total && (
            <span className="text-xs text-muted-foreground">
              ({data.subtask_current}/{data.subtask_total})
            </span>
          )}
        </div>
        <p className="text-sm text-foreground/60">{data.message}</p>
      </div>
    </div>
  );
}

export default ExecutionPanel;
