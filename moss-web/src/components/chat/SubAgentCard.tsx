import { useState } from 'react';
import {
  ChevronDown,
  ChevronRight,
  Users,
  Loader2,
  CheckCircle2,
  AlertCircle,
  Clock,
  Cog,
  XCircle,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import type { UISubAgent } from '@/api/types';

interface SubAgentCardProps {
  /** The Sub-Agent to display */
  agent: UISubAgent;
  /** Get a Sub-Agent by ID (for rendering children) */
  getSubAgent?: (id: string) => UISubAgent | undefined;
  /** Whether this is a nested (child) Sub-Agent */
  isNested?: boolean;
}

/**
 * Displays a Sub-Agent's status, task, and progress
 * Supports nested Sub-Agents with collapsible children
 */
export function SubAgentCard({
  agent,
  getSubAgent,
  isNested = false,
}: SubAgentCardProps) {
  const [isExpanded, setIsExpanded] = useState(true);
  const [showResult, setShowResult] = useState(false);

  const hasChildren = agent.childIds.length > 0;
  const isActive = agent.state === 'running' || agent.state === 'pending';

  // Get status icon and colors
  const statusConfig = getStatusConfig(agent.state);

  // Format duration
  const formatDuration = (ms: number) => {
    if (ms < 1000) return `${ms}ms`;
    if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
    return `${Math.floor(ms / 60000)}m ${Math.floor((ms % 60000) / 1000)}s`;
  };

  return (
    <div
      className={cn(
        'rounded-lg border transition-all duration-200',
        isNested && 'ml-4',
        isActive
          ? 'border-blue-500/30 bg-blue-500/5'
          : agent.state === 'failed'
            ? 'border-destructive/30 bg-destructive/5'
            : agent.state === 'interrupted'
              ? 'border-orange-500/30 bg-orange-500/5'
              : agent.state === 'completed'
                ? 'border-green-500/30 bg-green-500/5'
                : 'border-muted bg-muted/30'
      )}
    >
      {/* Header */}
      <button
        onClick={() => setIsExpanded(!isExpanded)}
        className={cn(
          'w-full flex items-center gap-2 px-3 py-2 text-sm',
          'hover:bg-muted/50 transition-colors rounded-t-lg',
          !isExpanded && 'rounded-b-lg'
        )}
      >
        {/* Expand/collapse */}
        {isExpanded ? (
          <ChevronDown className="h-4 w-4 text-muted-foreground shrink-0" />
        ) : (
          <ChevronRight className="h-4 w-4 text-muted-foreground shrink-0" />
        )}

        {/* Status icon */}
        <statusConfig.Icon
          className={cn(
            'h-4 w-4 shrink-0',
            statusConfig.color,
            agent.state === 'running' && 'animate-spin'
          )}
        />

        {/* Name */}
        <span className="font-medium truncate">{agent.name}</span>

        {/* Depth indicator */}
        {agent.depth > 0 && (
          <span className="text-xs text-muted-foreground bg-muted/50 px-1.5 py-0.5 rounded">
            深度 {agent.depth}
          </span>
        )}

        {/* Progress */}
        {agent.state === 'running' && agent.progress && (
          <span className="text-xs text-muted-foreground bg-blue-500/10 text-blue-600 dark:text-blue-400 px-1.5 py-0.5 rounded animate-pulse">
            处理中
          </span>
        )}

        {/* Children count */}
        {hasChildren && (
          <span className="flex items-center gap-1 text-xs text-muted-foreground">
            <Users className="h-3 w-3" />
            {agent.childIds.length}
          </span>
        )}

        {/* Duration (when completed/failed) */}
        {agent.result?.metrics?.duration_ms && (
          <span className="flex items-center gap-0.5 text-xs text-muted-foreground ml-auto">
            <Clock className="h-3 w-3" />
            {formatDuration(agent.result.metrics.duration_ms)}
          </span>
        )}

        {/* Status indicator */}
        <div className="ml-auto flex items-center gap-1.5">
          {isActive ? (
            <>
              <span className="relative flex h-2 w-2">
                <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-blue-500 opacity-75"></span>
                <span className="relative inline-flex rounded-full h-2 w-2 bg-blue-500"></span>
              </span>
            </>
          ) : (
            <span
              className={cn(
                'relative inline-flex rounded-full h-2 w-2',
                agent.state === 'completed'
                  ? 'bg-green-500'
                  : agent.state === 'failed'
                    ? 'bg-destructive'
                    : agent.state === 'interrupted'
                      ? 'bg-orange-500'
                      : 'bg-muted-foreground/30'
              )}
            />
          )}
        </div>
      </button>

      {/* Content */}
      {isExpanded && (
        <div className="px-3 pb-3 space-y-2">
          {/* Task description */}
          <div className="text-xs text-muted-foreground bg-muted/30 rounded p-2">
            {agent.task.length > 200
              ? agent.task.slice(0, 200) + '...'
              : agent.task}
          </div>

          {/* Progress message */}
          {agent.state === 'running' && agent.progress?.message && (
            <div className="text-xs text-blue-600 dark:text-blue-400 bg-blue-500/10 rounded px-2 py-1">
              {agent.progress.message}
            </div>
          )}

          {/* Last tool */}
          {agent.state === 'running' && agent.progress?.lastToolName && (
            <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <Cog className="h-3 w-3 animate-spin text-blue-500" />
              <span className="font-mono">{agent.progress.lastToolName}</span>
            </div>
          )}

          {/* Error message */}
          {agent.state === 'failed' && agent.error && (
            <div className="text-xs text-destructive bg-destructive/10 rounded px-2 py-1.5 border border-destructive/20">
              {agent.error}
            </div>
          )}

          {/* Interrupted message */}
          {agent.state === 'interrupted' && (
            <div className="text-xs text-orange-600 dark:text-orange-400 bg-orange-500/10 rounded px-2 py-1.5 border border-orange-500/20">
              {agent.error || 'Interrupted by user'}
            </div>
          )}

          {/* Result (collapsible) */}
          {agent.state === 'completed' && agent.result && (
            <div className="space-y-1">
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  setShowResult(!showResult);
                }}
                className="flex items-center gap-1 text-xs text-green-600 dark:text-green-400 hover:underline"
              >
                {showResult ? (
                  <ChevronDown className="h-3 w-3" />
                ) : (
                  <ChevronRight className="h-3 w-3" />
                )}
                查看结果
              </button>
              {showResult && (
                <div className="text-xs bg-muted/50 rounded p-2 max-h-48 overflow-y-auto">
                  <pre className="whitespace-pre-wrap font-mono">
                    {agent.result.output.length > 500
                      ? agent.result.output.slice(0, 500) + '...'
                      : agent.result.output}
                  </pre>
                </div>
              )}
            </div>
          )}

          {/* Metrics summary */}
          {agent.result?.metrics && (
            <div className="flex flex-wrap gap-2 text-xs text-muted-foreground">
              <span className="flex items-center gap-1 bg-muted/50 px-1.5 py-0.5 rounded">
                <Cog className="h-3 w-3" />
                {agent.result.metrics.tool_calls} 工具
              </span>
              <span className="bg-muted/50 px-1.5 py-0.5 rounded">
                {agent.result.metrics.total_iterations} 步
              </span>
              <span className="bg-muted/50 px-1.5 py-0.5 rounded">
                {agent.result.metrics.prompt_tokens +
                  agent.result.metrics.completion_tokens}{' '}
                tokens
              </span>
            </div>
          )}

          {/* Nested children */}
          {hasChildren && getSubAgent && (
            <div className="space-y-2 mt-2">
              <div className="flex items-center gap-1.5 text-xs font-bold text-muted-foreground uppercase tracking-tight">
                <Users className="h-3.5 w-3.5" />
                <span>子代理</span>
              </div>
              {agent.childIds.map((childId) => {
                const child = getSubAgent(childId);
                return child ? (
                  <SubAgentCard
                    key={childId}
                    agent={child}
                    getSubAgent={getSubAgent}
                    isNested
                  />
                ) : null;
              })}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/**
 * Get status configuration based on Sub-Agent state
 */
function getStatusConfig(state: UISubAgent['state']) {
  switch (state) {
    case 'pending':
      return {
        Icon: Loader2,
        color: 'text-muted-foreground',
        label: '等待中',
      };
    case 'running':
      return {
        Icon: Loader2,
        color: 'text-blue-500',
        label: '运行中',
      };
    case 'completed':
      return {
        Icon: CheckCircle2,
        color: 'text-green-500',
        label: '已完成',
      };
    case 'failed':
      return {
        Icon: AlertCircle,
        color: 'text-destructive',
        label: '失败',
      };
    case 'cancelled':
      return {
        Icon: XCircle,
        color: 'text-muted-foreground',
        label: '已取消',
      };
    case 'interrupted':
      return {
        Icon: XCircle,
        color: 'text-orange-500',
        label: 'Interrupted',
      };
  }
}

/**
 * Container for displaying multiple Sub-Agents
 */
interface SubAgentListProps {
  /** Root-level Sub-Agents to display */
  agents: UISubAgent[];
  /** Get a Sub-Agent by ID */
  getSubAgent: (id: string) => UISubAgent | undefined;
}

export function SubAgentList({ agents, getSubAgent }: SubAgentListProps) {
  if (agents.length === 0) {
    return null;
  }

  return (
    <div className="space-y-2">
      <div className="flex items-center gap-1.5 text-xs font-bold text-blue-600 dark:text-blue-400 uppercase tracking-tight">
        <Users className="h-3.5 w-3.5" />
        <span>Sub-Agents</span>
        <span className="text-muted-foreground font-medium ml-1">
          ({agents.filter((a) => a.state === 'completed').length}/
          {agents.length})
        </span>
      </div>
      {agents.map((agent) => (
        <SubAgentCard
          key={agent.id}
          agent={agent}
          getSubAgent={getSubAgent}
        />
      ))}
    </div>
  );
}
