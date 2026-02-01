import { useState } from 'react';
import { MessageSquare, Trash2, MoreHorizontal } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { cn } from '@/lib/utils';
import type { SessionMeta } from '@/api/types';
import { SessionDeleteDialog } from './SessionDeleteDialog';

interface SessionListProps {
  sessions: SessionMeta[];
  currentId: string | null;
  isLoading: boolean;
  onSelect: (id: string) => void;
  onDelete: (id: string) => void;
}

export function SessionList({
  sessions,
  currentId,
  isLoading,
  onSelect,
  onDelete,
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
}

function SessionItem({ session, isActive, onSelect, onDelete }: SessionItemProps) {
  const [deleteOpen, setDeleteOpen] = useState(false);

  const displayTitle = session.title || session.model || session.session_id.slice(0, 8);
  const timeStr = formatRelativeTime(session.updated_at);
  const countStr = `${session.message_count} 条消息`;

  return (
    <>
      <div
        className={cn(
          'group flex items-center gap-2 px-3 py-2 rounded-lg cursor-pointer transition-all duration-200',
          isActive
            ? 'bg-secondary text-secondary-foreground shadow-sm ring-1 ring-border'
            : 'text-muted-foreground hover:bg-muted hover:text-foreground'
        )}
        onClick={onSelect}
      >
        <MessageSquare
          className={cn(
            'h-4 w-4 shrink-0 transition-colors',
            isActive ? 'text-primary' : 'text-muted-foreground/60'
          )}
        />
        <div className="flex-1 min-w-0">
          <span className="block truncate text-xs font-medium">{displayTitle}</span>
          <span className="block truncate text-[10px] text-muted-foreground/70">
            {countStr} · {timeStr}
          </span>
        </div>

        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7 opacity-0 group-hover:opacity-100 transition-opacity"
              onClick={(e) => e.stopPropagation()}
            >
              <MoreHorizontal className="h-4 w-4" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="glass">
            <DropdownMenuItem
              className="text-destructive focus:bg-destructive/10 focus:text-destructive"
              onClick={(e) => {
                e.stopPropagation();
                setDeleteOpen(true);
              }}
            >
              <Trash2 className="mr-2 h-4 w-4" />
              删除
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

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
