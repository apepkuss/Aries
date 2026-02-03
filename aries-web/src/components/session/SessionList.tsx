import { useState } from 'react';
import { MessageSquare, Trash2, Check } from 'lucide-react';
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from '@/components/ui/context-menu';
import { cn } from '@/lib/utils';
import type { SessionMeta } from '@/api/types';
import { SessionDeleteDialog } from './SessionDeleteDialog';

interface SessionListProps {
  sessions: SessionMeta[];
  currentId: string | null;
  isLoading: boolean;
  onSelect: (id: string) => void;
  onDelete: (id: string) => void;
  isSelectMode?: boolean;
  selectedIds?: Set<string>;
  onToggleSelect?: (id: string) => void;
}

export function SessionList({
  sessions,
  currentId,
  isLoading,
  onSelect,
  onDelete,
  isSelectMode = false,
  selectedIds,
  onToggleSelect,
}: SessionListProps) {
  if (isLoading) {
    return (
      <div className="text-sm text-muted-foreground text-center py-8">
        加载中...
      </div>
    );
  }

  if (sessions.length === 0) {
    return (
      <div className="text-sm text-muted-foreground text-center py-8">
        暂无对话记录
      </div>
    );
  }

  return (
    <div className="space-y-1">
      {sessions.map((session) => (
        <SessionItem
          key={session.session_id}
          session={session}
          isActive={currentId === session.session_id}
          onSelect={() => onSelect(session.session_id)}
          onDelete={() => onDelete(session.session_id)}
          isSelectMode={isSelectMode}
          isSelected={selectedIds?.has(session.session_id) ?? false}
          onToggleSelect={() => onToggleSelect?.(session.session_id)}
        />
      ))}
    </div>
  );
}

interface SessionItemProps {
  session: SessionMeta;
  isActive: boolean;
  onSelect: () => void;
  onDelete: () => void;
  isSelectMode: boolean;
  isSelected: boolean;
  onToggleSelect: () => void;
}

function SessionItem({
  session,
  isActive,
  onSelect,
  onDelete,
  isSelectMode,
  isSelected,
  onToggleSelect,
}: SessionItemProps) {
  const [deleteOpen, setDeleteOpen] = useState(false);

  const displayTitle = session.title || session.model || session.session_id.slice(0, 8);
  const timeStr = formatRelativeTime(session.updated_at);
  const countStr = `${session.message_count} 条消息`;

  const handleClick = () => {
    if (isSelectMode) {
      onToggleSelect();
    } else {
      onSelect();
    }
  };

  return (
    <>
      <ContextMenu>
        <ContextMenuTrigger asChild>
          <div
            className={cn(
              'flex items-center gap-2 px-3 py-2 rounded-lg cursor-pointer transition-all duration-200',
              isSelectMode && isSelected
                ? 'bg-primary/10 text-foreground ring-1 ring-primary/30'
                : isActive && !isSelectMode
                  ? 'bg-secondary text-secondary-foreground shadow-sm ring-1 ring-border'
                  : 'text-muted-foreground hover:bg-muted hover:text-foreground'
            )}
            onClick={handleClick}
          >
            {/* Select checkbox or message icon */}
            {isSelectMode ? (
              <div
                className={cn(
                  'h-4 w-4 shrink-0 rounded border flex items-center justify-center transition-colors',
                  isSelected
                    ? 'bg-primary border-primary text-primary-foreground'
                    : 'border-muted-foreground/40'
                )}
              >
                {isSelected && <Check className="h-3 w-3" />}
              </div>
            ) : (
              <MessageSquare
                className={cn(
                  'h-4 w-4 shrink-0 transition-colors',
                  isActive ? 'text-primary' : 'text-muted-foreground/60'
                )}
              />
            )}

            <div className="flex-1 min-w-0">
              <span className="block truncate text-xs font-medium">{displayTitle}</span>
              <span className="block truncate text-[10px] text-muted-foreground/70">
                {countStr} · {timeStr}
              </span>
            </div>
          </div>
        </ContextMenuTrigger>

        {!isSelectMode && (
          <ContextMenuContent>
            <ContextMenuItem
              className="text-destructive focus:bg-destructive/10 focus:text-destructive"
              onClick={() => setDeleteOpen(true)}
            >
              <Trash2 className="mr-2 h-4 w-4" />
              删除
            </ContextMenuItem>
          </ContextMenuContent>
        )}
      </ContextMenu>

      <SessionDeleteDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        onConfirm={onDelete}
        sessionModel={session.model}
      />
    </>
  );
}

function formatRelativeTime(isoString: string): string {
  const date = new Date(isoString);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMin = Math.floor(diffMs / 60000);
  const diffHour = Math.floor(diffMs / 3600000);
  const diffDay = Math.floor(diffMs / 86400000);

  if (diffMin < 1) return '刚刚';
  if (diffMin < 60) return `${diffMin} 分钟前`;
  if (diffHour < 24) return `${diffHour} 小时前`;
  if (diffDay < 7) return `${diffDay} 天前`;

  return date.toLocaleDateString('zh-CN', { month: 'short', day: 'numeric' });
}
