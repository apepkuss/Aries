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
  Users,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import type { UIToolCall, ExecutionEvent, UITaskPlan, UISubAgent, UISubtask, SubAgentMetrics } from '@/api/types';
import type { ExecutionStatus } from '@/stores';
import { SubAgentList } from './SubAgentCard';

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
  /** Root-level Sub-Agents to display */
  subAgents?: UISubAgent[];
  /** Get a Sub-Agent by ID (for nested Sub-Agents) */
  getSubAgent?: (id: string) => UISubAgent | undefined;
}

const phaseConfig: Record<
  ExecutionStatus['phase'],
  { icon: React.ElementType; label: string; color: string; bgColor: string }
> = {
  idle: { icon: Loader2, label: '', color: '', bgColor: '' },
  planning: {
    icon: Brain,
    label: '正在规划',
    color: 'text-purple-600 dark:text-purple-400',
    bgColor: 'bg-purple-500/10 dark:bg-purple-950/30',
  },
  executing: {
    icon: Cog,
    label: '正在执行',
    color: 'text-blue-600 dark:text-blue-400',
    bgColor: 'bg-blue-500/10 dark:bg-blue-950/30',
  },
  reflecting: {
    icon: RefreshCw,
    label: '正在审视',
    color: 'text-amber-600 dark:text-amber-400',
    bgColor: 'bg-amber-500/10 dark:bg-amber-950/30',
  },
  completing: {
    icon: CheckCircle2,
    label: '即将完成',
    color: 'text-green-600 dark:text-green-400',
    bgColor: 'bg-green-500/10 dark:bg-green-950/30',
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
  subAgents,
  getSubAgent,
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
  const hasSubAgents = subAgents && subAgents.length > 0;

  if (!hasThinking && !hasToolCalls && !hasExecutionEvents && !hasTaskPlan && !hasExecutionStatus && !hasSubAgents) {
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
          <div className="flex items-center gap-1.5 text-muted-foreground group-hover:text-foreground transition-colors">
            <Brain className="h-4 w-4" />
            <span className="font-medium">思考过程</span>
          </div>
        )}

        {/* Subtask progress during streaming */}
        {isStreaming &&
          executionStatus?.subtaskTotal !== undefined &&
          executionStatus.subtaskTotal > 0 && (
            <span className="text-xs text-muted-foreground bg-muted/50 px-2 py-0.5 rounded-full border border-border/50">
              任务 {executionStatus.subtaskCurrent ?? 1}/
              {executionStatus.subtaskTotal}
            </span>
          )}

        {/* Summary when collapsed */}
        {!isExpanded && (
          <div className="flex items-center gap-2 ml-auto text-xs text-muted-foreground">
            {totalTools > 0 && (
              <span className="flex items-center gap-1 bg-blue-500/10 text-blue-600 dark:text-blue-400 px-1.5 py-0.5 rounded">
                <Cog className="h-3 w-3" />
                {completedTools}/{totalTools} 工具
                {hasErrors && (
                  <AlertCircle className="h-3 w-3 text-destructive" />
                )}
              </span>
            )}
            {hasSubAgents && (
              <span className="flex items-center gap-1 bg-cyan-500/10 text-cyan-600 dark:text-cyan-400 px-1.5 py-0.5 rounded">
                <Users className="h-3 w-3" />
                {subAgents!.filter((a) => a.state === 'completed').length}/{subAgents!.length} Sub-Agent
              </span>
            )}
            {hasThinking && (
              <span className="flex items-center gap-1 bg-purple-500/10 text-purple-600 dark:text-purple-400 px-1.5 py-0.5 rounded">
                <Brain className="h-3 w-3" />
                想法
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
              <span className="text-[10px] font-bold uppercase tracking-wider text-green-600 dark:text-green-400">运行中</span>
            </>
          ) : (
            <span className="relative inline-flex rounded-full h-2 w-2 bg-muted-foreground/30"></span>
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
              <div className="flex items-center gap-1.5 text-xs font-bold text-emerald-600 dark:text-emerald-400 uppercase tracking-tight">
                <ListTodo className="h-3.5 w-3.5" />
                <span>任务规划</span>
                <span className="text-muted-foreground font-medium ml-1">
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
                  <div className="flex items-center gap-1.5 text-xs font-bold text-purple-600 dark:text-purple-400 uppercase tracking-tight">
                    <Brain className="h-3.5 w-3.5" />
                    <span>详细想法</span>
                  </div>
                  <div className="text-[13px] text-muted-foreground leading-relaxed whitespace-pre-wrap bg-purple-500/5 border border-purple-500/10 rounded-xl p-3 max-h-48 overflow-y-auto">
                    {thinking}
                  </div>
                </div>
              )}

              {/* Tool calls */}
              {hasToolCalls && (
                <div className="space-y-2">
                  <div className="flex items-center gap-1.5 text-xs font-bold text-blue-600 dark:text-blue-400 uppercase tracking-tight">
                    <Cog className="h-3.5 w-3.5" />
                    <span>工具调用</span>
                    <span className="text-muted-foreground font-medium ml-1">
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

          {/* Sub-Agents */}
          {hasSubAgents && getSubAgent && (
            <SubAgentList
              agents={subAgents!}
              getSubAgent={getSubAgent}
            />
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
  subtask: UISubtask;
  /** Callback to toggle expanded state */
  onToggleExpand?: (id: number) => void;
}

function SubtaskItem({ subtask, onToggleExpand }: SubtaskItemProps) {
  // Local expanded state if no callback provided
  const [localExpanded, setLocalExpanded] = useState(false);
  const isExpanded = subtask.expanded ?? localExpanded;

  // Determine if the subtask is expandable (has progress, error, or result)
  const hasDetails = !!(subtask.progress || subtask.error || subtask.result || subtask.subAgentId);
  const isExpandable = hasDetails && (subtask.status === 'in_progress' || subtask.status === 'failed' || subtask.status === 'completed');

  // Auto-expand when in_progress or failed
  useEffect(() => {
    if ((subtask.status === 'in_progress' || subtask.status === 'failed') && hasDetails) {
      if (onToggleExpand) {
        onToggleExpand(subtask.id);
      } else {
        setLocalExpanded(true);
      }
    }
  }, [subtask.status, hasDetails, subtask.id, onToggleExpand]);

  const handleToggle = () => {
    if (isExpandable) {
      if (onToggleExpand) {
        onToggleExpand(subtask.id);
      } else {
        setLocalExpanded(!localExpanded);
      }
    }
  };

  return (
    <div className="text-sm">
      {/* Header row - always visible */}
      <div
        className={cn(
          'flex items-start gap-2 px-2 py-1',
          isExpandable && 'cursor-pointer hover:bg-muted/50 rounded-md transition-colors'
        )}
        onClick={handleToggle}
      >
        {/* Expand/collapse indicator for expandable items */}
        {isExpandable ? (
          isExpanded ? (
            <ChevronDown className="h-4 w-4 text-muted-foreground shrink-0 mt-0.5" />
          ) : (
            <ChevronRight className="h-4 w-4 text-muted-foreground shrink-0 mt-0.5" />
          )
        ) : (
          /* Status indicator for non-expandable items */
          <>
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
          </>
        )}

        {/* Status icon for expandable items */}
        {isExpandable && (
          <>
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
          </>
        )}

        {/* Description */}
        <span
          className={cn(
            'text-xs flex-1',
            subtask.status === 'completed' && 'text-muted-foreground line-through',
            subtask.status === 'skipped' && 'text-muted-foreground/50 line-through',
            subtask.status === 'failed' && 'text-destructive',
            subtask.status === 'in_progress' && 'text-foreground font-medium',
            subtask.status === 'pending' && 'text-muted-foreground'
          )}
        >
          {subtask.description}
        </span>

        {/* Progress indicator */}
        {subtask.progress && subtask.status === 'in_progress' && (
          <span className="text-[10px] text-muted-foreground bg-muted/50 px-1.5 py-0.5 rounded">
            {subtask.progress.iteration}/{subtask.progress.maxIterations}
          </span>
        )}

        {/* Retry count indicator */}
        {subtask.retryCount !== undefined && subtask.retryCount > 0 && (
          <span className="text-[10px] text-amber-600 dark:text-amber-400 bg-amber-500/10 px-1.5 py-0.5 rounded">
            重试 {subtask.retryCount}
          </span>
        )}
      </div>

      {/* Expanded details */}
      {isExpanded && (
        <div className="ml-8 mt-1 space-y-2 pb-2">
          {/* Progress details */}
          {subtask.progress && (
            <div className="text-xs bg-blue-500/5 border border-blue-500/20 rounded-lg p-2">
              <div className="flex items-center gap-2 text-blue-600 dark:text-blue-400">
                <Cog className="h-3 w-3 animate-spin" />
                <span>
                  迭代 {subtask.progress.iteration}/{subtask.progress.maxIterations}
                </span>
              </div>
              {subtask.progress.lastToolName && (
                <div className="mt-1 text-muted-foreground flex items-center gap-1.5">
                  <Cog className="h-3 w-3" />
                  <span>工具: {subtask.progress.lastToolName}</span>
                </div>
              )}
              {subtask.progress.message && (
                <div className="mt-1 text-muted-foreground">
                  {subtask.progress.message}
                </div>
              )}
            </div>
          )}

          {/* Error message */}
          {subtask.error && (
            <div className="text-xs bg-destructive/5 border border-destructive/20 rounded-lg p-2 text-destructive">
              <div className="flex items-center gap-1.5 font-medium mb-1">
                <AlertCircle className="h-3 w-3" />
                <span>错误</span>
              </div>
              <div>{subtask.error}</div>
            </div>
          )}

          {/* Result summary */}
          {subtask.result && (
            <div className="text-xs bg-green-500/5 border border-green-500/20 rounded-lg p-2">
              <div className="flex items-center gap-1.5 text-green-600 dark:text-green-400 font-medium mb-1">
                <CheckCircle2 className="h-3 w-3" />
                <span>完成</span>
              </div>
              {subtask.result.metrics && (
                <SubAgentMetricsSummary metrics={subtask.result.metrics} />
              )}
              {subtask.result.output && (
                <div className="mt-1 text-muted-foreground max-h-20 overflow-y-auto">
                  {subtask.result.output.length > 200
                    ? subtask.result.output.slice(0, 200) + '...'
                    : subtask.result.output}
                </div>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/** Compact metrics summary display */
function SubAgentMetricsSummary({ metrics }: { metrics: SubAgentMetrics }) {
  return (
    <div className="flex flex-wrap gap-2 text-[10px] text-muted-foreground">
      <span className="flex items-center gap-1 bg-muted/50 px-1.5 py-0.5 rounded">
        <RefreshCw className="h-2.5 w-2.5" />
        {metrics.total_iterations} 迭代
      </span>
      <span className="flex items-center gap-1 bg-muted/50 px-1.5 py-0.5 rounded">
        <Cog className="h-2.5 w-2.5" />
        {metrics.tool_calls} 工具
      </span>
      <span className="flex items-center gap-1 bg-muted/50 px-1.5 py-0.5 rounded">
        <Clock className="h-2.5 w-2.5" />
        {(metrics.duration_ms / 1000).toFixed(1)}s
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
              <div className="text-[10px] font-bold text-muted-foreground mb-1.5 uppercase tracking-wider">
                输入参数
              </div>
              <pre className="text-xs bg-muted/50 p-2.5 rounded-lg border border-border/30 max-h-48 font-mono leading-tight">
                {JSON.stringify(toolCall.arguments, null, 2)}
              </pre>
            </div>
          )}

          {/* Result */}
          {toolCall.result && toolCall.status === 'success' && (
            <div>
              <div className="text-[10px] font-bold text-muted-foreground mb-1.5 uppercase tracking-wider">输出结果</div>
              <pre className="text-xs bg-muted/50 p-2.5 rounded-lg border border-border/30 max-h-48 font-mono leading-tight">
                {toolCall.result.length > 1000
                  ? toolCall.result.slice(0, 1000) + '...'
                  : toolCall.result}
              </pre>
            </div>
          )}

          {/* Error */}
          {toolCall.error && toolCall.status === 'error' && (
            <div>
              <div className="text-[10px] font-bold text-destructive mb-1.5 uppercase tracking-wider">错误信息</div>
              <div className="text-xs bg-destructive/5 text-destructive p-2.5 rounded-lg border border-destructive/20 leading-normal">
                {toolCall.error}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
