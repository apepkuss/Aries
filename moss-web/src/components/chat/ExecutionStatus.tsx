import { Loader2, Brain, Cog, RefreshCw, CheckCircle2 } from 'lucide-react';
import { useChatStore, type ExecutionStatus as ExecutionStatusType } from '@/stores';
import { cn } from '@/lib/utils';

const phaseConfig: Record<
  ExecutionStatusType['phase'],
  { icon: React.ElementType; label: string; color: string }
> = {
  idle: { icon: Loader2, label: '', color: '' },
  planning: { icon: Brain, label: '规划中', color: 'text-purple-500' },
  executing: { icon: Cog, label: '执行中', color: 'text-blue-500' },
  reflecting: { icon: RefreshCw, label: '反思中', color: 'text-amber-500' },
  completing: { icon: CheckCircle2, label: '完成中', color: 'text-green-500' },
};

export function ExecutionStatus() {
  const { executionStatus, isStreaming } = useChatStore();

  // Don't show if idle or not streaming
  if (executionStatus.phase === 'idle' || !isStreaming) {
    return null;
  }

  const config = phaseConfig[executionStatus.phase];
  const Icon = config.icon;

  return (
    <div className="border-t bg-muted/30 px-4 py-2">
      <div className="max-w-4xl mx-auto flex items-center gap-3">
        {/* Phase indicator */}
        <div className={cn('flex items-center gap-2', config.color)}>
          <Icon className={cn('h-4 w-4', executionStatus.phase !== 'completing' && 'animate-spin')} />
          <span className="text-sm font-medium">{config.label}</span>
        </div>

        {/* Subtask progress */}
        {executionStatus.subtaskTotal !== undefined && executionStatus.subtaskTotal > 0 && (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <span>|</span>
            <span>
              子任务 {executionStatus.subtaskCurrent ?? 1}/{executionStatus.subtaskTotal}
            </span>
          </div>
        )}

        {/* Status message */}
        {executionStatus.message && (
          <div className="flex-1 text-sm text-muted-foreground truncate">
            {executionStatus.message}
          </div>
        )}
      </div>
    </div>
  );
}
