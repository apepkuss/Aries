import { useState, useEffect } from 'react';
import {
  ChevronDown,
  ChevronRight,
  Brain,
  Cog,
  RefreshCw,
  CheckCircle2,
  Loader2,
  Check,
  AlertCircle,
  Clock,
  Circle,
  ListTodo,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import type { UIToolCall, ExecutionEvent, UITaskPlan } from '@/api/types';
import type { ExecutionStatus } from '@/stores';

interface ThinkingProcessProps {
  thinking?: string;
  toolCalls?: UIToolCall[];
  /** Execution events in chronological order for timeline display */
  executionEvents?: ExecutionEvent[];
  /** Task plan with subtask list */
  taskPlan?: UITaskPlan;
  executionStatus?: ExecutionStatus;
  isStreaming?: boolean;
  /** Force expanded state (overrides auto-collapse) */
  forceExpanded?: boolean;
}

const phaseConfig: Record<
  ExecutionStatus['phase'],
  { icon: React.ElementType; label: string; color: string; bgColor: string }
> = {
  idle: { icon: Loader2, label: '', color: '', bgColor: '' },
  planning: {
    icon: Brain,
    label: 'Planning',
    color: 'text-purple-600 dark:text-purple-400',
    bgColor: 'bg-purple-50 dark:bg-purple-950/30',
  },
  executing: {
    icon: Cog,
    label: 'Executing',
    color: 'text-blue-600 dark:text-blue-400',
    bgColor: 'bg-blue-50 dark:bg-blue-950/30',
  },
  reflecting: {
    icon: RefreshCw,
    label: 'Reflecting',
    color: 'text-amber-600 dark:text-amber-400',
    bgColor: 'bg-amber-50 dark:bg-amber-950/30',
  },
  completing: {
    icon: CheckCircle2,
    label: 'Completing',
    color: 'text-green-600 dark:text-green-400',
    bgColor: 'bg-green-50 dark:bg-green-950/30',
  },
};

export function ThinkingProcess({
  thinking,
  toolCalls,
  executionEvents,
  taskPlan,
  executionStatus,
  isStreaming,
  forceExpanded,
}: ThinkingProcessProps) {
  // Auto-expanded during streaming, collapsed when done
  const [isExpanded, setIsExpanded] = useState(true);
  const [wasStreaming, setWasStreaming] = useState(false);

  // Track streaming state changes to auto-collapse
  useEffect(() => {
    if (isStreaming) {
      setWasStreaming(true);
      setIsExpanded(true);
    } else if (wasStreaming && !forceExpanded) {
      // Stream just ended, collapse after a short delay
      const timer = setTimeout(() => {
        setIsExpanded(false);
      }, 500);
      return () => clearTimeout(timer);
    }
  }, [isStreaming, wasStreaming, forceExpanded]);

  // Override with forceExpanded if set
  useEffect(() => {
    if (forceExpanded !== undefined) {
      setIsExpanded(forceExpanded);
    }
  }, [forceExpanded]);

  // Don't render if nothing to show
  const hasThinking = thinking && thinking.trim().length > 0;
  const hasToolCalls = toolCalls && toolCalls.length > 0;
  const hasExecutionEvents = executionEvents && executionEvents.length > 0;
  const hasTaskPlan = taskPlan && taskPlan.subtasks.length > 0;
  const hasExecutionStatus =
    executionStatus && executionStatus.phase !== 'idle';

  if (!hasThinking && !hasToolCalls && !hasExecutionEvents && !hasTaskPlan && !hasExecutionStatus) {
    return null;
  }

  const phase = executionStatus?.phase || 'idle';
  const config = phaseConfig[phase];
  const Icon = config.icon;

  // Calculate summary stats
  const completedTools = toolCalls?.filter(
    (tc) => tc.status === 'success'
  ).length || 0;
  const totalTools = toolCalls?.length || 0;
  const hasErrors = toolCalls?.some((tc) => tc.status === 'error');

  return (
    <div
      className={cn(
        'rounded-lg border transition-all duration-200',
        isStreaming
          ? 'border-primary/30 bg-primary/5'
          : 'border-muted bg-muted/30'
      )}
    >
      {/* Header - always visible */}
      <button
        onClick={() => setIsExpanded(!isExpanded)}
        className={cn(
          'w-full flex items-center gap-2 px-3 py-2 text-sm',
          'hover:bg-muted/50 transition-colors rounded-t-lg',
          !isExpanded && 'rounded-b-lg'
        )}
      >
        {/* Expand/collapse icon */}
        {isExpanded ? (
          <ChevronDown className="h-4 w-4 text-muted-foreground shrink-0" />
        ) : (
          <ChevronRight className="h-4 w-4 text-muted-foreground shrink-0" />
        )}

        {/* Status icon and label */}
        {isStreaming && hasExecutionStatus ? (
          <div className={cn('flex items-center gap-1.5', config.color)}>
            <Icon
              className={cn(
                'h-4 w-4',
                phase !== 'completing' && 'animate-spin'
              )}
            />
            <span className="font-medium">{config.label}</span>
          </div>
        ) : (
          <div className="flex items-center gap-1.5 text-muted-foreground">
            <Brain className="h-4 w-4" />
            <span className="font-medium">Thinking Process</span>
          </div>
        )}

        {/* Subtask progress during streaming */}
        {isStreaming &&
          executionStatus?.subtaskTotal !== undefined &&
          executionStatus.subtaskTotal > 0 && (
            <span className="text-xs text-muted-foreground">
              Task {executionStatus.subtaskCurrent ?? 1}/
              {executionStatus.subtaskTotal}
            </span>
          )}

        {/* Summary when collapsed */}
        {!isExpanded && (
          <div className="flex items-center gap-2 ml-auto text-xs text-muted-foreground">
            {totalTools > 0 && (
              <span className="flex items-center gap-1">
                <Cog className="h-3 w-3" />
                {completedTools}/{totalTools} tools
                {hasErrors && (
                  <AlertCircle className="h-3 w-3 text-destructive" />
                )}
              </span>
            )}
            {hasThinking && (
              <span className="flex items-center gap-1">
                <Brain className="h-3 w-3" />
                thoughts
              </span>
            )}
          </div>
        )}

        {/* Status indicator - green pulsing when live, gray when idle */}
        <div className="ml-auto flex items-center gap-1.5">
          {isStreaming ? (
            <>
              <span className="relative flex h-2 w-2">
                <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-green-500 opacity-75"></span>
                <span className="relative inline-flex rounded-full h-2 w-2 bg-green-500"></span>
              </span>
              <span className="text-xs text-muted-foreground">Live</span>
            </>
          ) : (
            <span className="relative inline-flex rounded-full h-2 w-2 bg-gray-400"></span>
          )}
        </div>
      </button>

      {/* Content - collapsible */}
      {isExpanded && (
        <div className="px-3 pb-3 space-y-3">
          {/* Current status message */}
          {isStreaming && executionStatus?.message && (
            <div
              className={cn(
                'text-sm px-3 py-2 rounded-md',
                config.bgColor,
                config.color
              )}
            >
              {executionStatus.message}
            </div>
          )}

          {/* Task plan ToDo list */}
          {hasTaskPlan && (
            <div className="space-y-2">
              <div className="flex items-center gap-1.5 text-xs font-medium text-emerald-600 dark:text-emerald-400">
                <ListTodo className="h-3 w-3" />
                <span>Task Plan</span>
                <span className="text-muted-foreground font-normal">
                  ({taskPlan!.subtasks.filter((s) => s.status === 'completed').length}/
                  {taskPlan!.subtasks.length})
                </span>
              </div>
              <div className="space-y-1">
                {taskPlan!.subtasks.map((subtask) => (
                  <SubtaskItem key={subtask.id} subtask={subtask} />
                ))}
              </div>
            </div>
          )}

          {/* Timeline view - shows thoughts and tool calls in chronological order */}
          {hasExecutionEvents ? (
            <div className="space-y-2">
              {executionEvents!.map((event) => (
                event.type === 'thought' ? (
                  <ThoughtItem key={event.id} content={event.content || ''} />
                ) : (
                  <ToolCallItem key={event.id} toolCall={event.toolCall!} />
                )
              ))}
            </div>
          ) : (
            /* Fallback to legacy display when no execution events */
            <>
              {/* Thinking content */}
              {hasThinking && (
                <div className="space-y-1">
                  <div className="flex items-center gap-1.5 text-xs font-medium text-purple-600 dark:text-purple-400">
                    <Brain className="h-3 w-3" />
                    <span>Thoughts</span>
                  </div>
                  <div className="text-sm text-muted-foreground whitespace-pre-wrap bg-muted/50 rounded-md p-2 max-h-40 overflow-y-auto">
                    {thinking}
                  </div>
                </div>
              )}

              {/* Tool calls */}
              {hasToolCalls && (
                <div className="space-y-2">
                  <div className="flex items-center gap-1.5 text-xs font-medium text-blue-600 dark:text-blue-400">
                    <Cog className="h-3 w-3" />
                    <span>Tool Calls</span>
                    <span className="text-muted-foreground font-normal">
                      ({completedTools}/{totalTools})
                    </span>
                  </div>
                  <div className="space-y-1.5">
                    {toolCalls!.map((tc) => (
                      <ToolCallItem key={tc.id} toolCall={tc} />
                    ))}
                  </div>
                </div>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}

interface ThoughtItemProps {
  content: string;
}

function ThoughtItem({ content }: ThoughtItemProps) {
  return (
    <div className="text-sm bg-background rounded-md border border-purple-200 dark:border-purple-800/50">
      <div className="flex items-start gap-2 px-2 py-1.5">
        <Brain className="h-3.5 w-3.5 text-purple-500 shrink-0 mt-0.5" />
        <div className="text-muted-foreground whitespace-pre-wrap text-xs">
          {content}
        </div>
      </div>
    </div>
  );
}

interface SubtaskItemProps {
  subtask: {
    id: number;
    description: string;
    status: 'pending' | 'in_progress' | 'completed' | 'failed' | 'skipped';
  };
}

function SubtaskItem({ subtask }: SubtaskItemProps) {
  return (
    <div className="flex items-start gap-2 px-2 py-1 text-sm">
      {/* Status indicator */}
      {subtask.status === 'pending' && (
        <Circle className="h-4 w-4 text-muted-foreground shrink-0 mt-0.5" />
      )}
      {subtask.status === 'in_progress' && (
        <Loader2 className="h-4 w-4 animate-spin text-blue-500 shrink-0 mt-0.5" />
      )}
      {subtask.status === 'completed' && (
        <CheckCircle2 className="h-4 w-4 text-green-500 shrink-0 mt-0.5" />
      )}
      {subtask.status === 'failed' && (
        <AlertCircle className="h-4 w-4 text-destructive shrink-0 mt-0.5" />
      )}
      {subtask.status === 'skipped' && (
        <Circle className="h-4 w-4 text-muted-foreground/50 shrink-0 mt-0.5" />
      )}
      {/* Description */}
      <span
        className={cn(
          'text-xs',
          subtask.status === 'completed' && 'text-muted-foreground line-through',
          subtask.status === 'skipped' && 'text-muted-foreground/50 line-through',
          subtask.status === 'failed' && 'text-destructive',
          subtask.status === 'in_progress' && 'text-foreground font-medium',
          subtask.status === 'pending' && 'text-muted-foreground'
        )}
      >
        {subtask.description}
      </span>
    </div>
  );
}

interface ToolCallItemProps {
  toolCall: UIToolCall;
}

function ToolCallItem({ toolCall }: ToolCallItemProps) {
  const [showDetails, setShowDetails] = useState(false);

  return (
    <div className="text-sm bg-background rounded-md border">
      {/* Tool call header */}
      <button
        onClick={() => setShowDetails(!showDetails)}
        className="w-full flex items-center gap-2 px-2 py-1.5 hover:bg-muted/50 transition-colors rounded-md"
      >
        {/* Status indicator */}
        {toolCall.status === 'running' && (
          <Loader2 className="h-3.5 w-3.5 animate-spin text-blue-500 shrink-0" />
        )}
        {toolCall.status === 'success' && (
          <Check className="h-3.5 w-3.5 text-green-500 shrink-0" />
        )}
        {toolCall.status === 'error' && (
          <AlertCircle className="h-3.5 w-3.5 text-destructive shrink-0" />
        )}
        {toolCall.status === 'pending' && (
          <div className="h-3.5 w-3.5 rounded-full border-2 border-muted-foreground/30 shrink-0" />
        )}

        {/* Tool name */}
        <span className="font-mono text-xs truncate">{toolCall.name}</span>

        {/* Duration */}
        {toolCall.durationMs !== undefined && (
          <span className="flex items-center gap-0.5 text-xs text-muted-foreground ml-auto">
            <Clock className="h-3 w-3" />
            {toolCall.durationMs}ms
          </span>
        )}

        {/* Expand indicator */}
        {(toolCall.arguments ||
          toolCall.result ||
          toolCall.error) && (
          <ChevronRight
            className={cn(
              'h-3 w-3 text-muted-foreground transition-transform shrink-0',
              showDetails && 'rotate-90'
            )}
          />
        )}
      </button>

      {/* Details */}
      {showDetails && (
        <div className="px-2 pb-2 space-y-2">
          {/* Arguments */}
          {toolCall.arguments && Object.keys(toolCall.arguments).length > 0 && (
            <div>
              <div className="text-xs text-muted-foreground mb-1">
                Arguments
              </div>
              <pre className="text-xs bg-muted p-2 rounded overflow-x-auto max-h-24">
                {JSON.stringify(toolCall.arguments, null, 2)}
              </pre>
            </div>
          )}

          {/* Result */}
          {toolCall.result && toolCall.status === 'success' && (
            <div>
              <div className="text-xs text-muted-foreground mb-1">Result</div>
              <pre className="text-xs bg-muted p-2 rounded overflow-x-auto max-h-24">
                {toolCall.result.length > 500
                  ? toolCall.result.slice(0, 500) + '...'
                  : toolCall.result}
              </pre>
            </div>
          )}

          {/* Error */}
          {toolCall.error && toolCall.status === 'error' && (
            <div>
              <div className="text-xs text-destructive mb-1">Error</div>
              <div className="text-xs bg-destructive/10 text-destructive p-2 rounded">
                {toolCall.error}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
