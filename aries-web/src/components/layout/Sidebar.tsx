import { Plus, MessageSquare, Trash2, Edit2, MoreHorizontal } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { cn } from '@/lib/utils';
import { useUIStore, useConversationsStore, useChatStore } from '@/stores';
import { useEffect } from 'react';

export function Sidebar() {
  const { sidebarOpen } = useUIStore();
  const {
    conversations,
    currentId,
    isLoading,
    fetchConversations,
    selectConversation,
    deleteConversation: removeConversation,
  } = useConversationsStore();
  const { clearMessages, loadMessages, setConversationId } = useChatStore();

  // Fetch conversations on mount
  useEffect(() => {
    fetchConversations();
  }, [fetchConversations]);

  const handleNewChat = () => {
    clearMessages();
  };

  const handleSelectConversation = async (id: string) => {
    const messages = await selectConversation(id);
    loadMessages(messages);
    setConversationId(id);
  };

  const handleDeleteConversation = async (id: string) => {
    await removeConversation(id);
    if (currentId === id) {
      clearMessages();
    }
  };

  if (!sidebarOpen) {
    return null;
  }

  return (
    <aside className="w-64 border-r bg-muted/10 backdrop-blur-sm flex flex-col transition-all duration-300">
      {/* New chat button */}
      <div className="p-4">
        <Button
          onClick={handleNewChat}
          className="w-full justify-start gap-2 shadow-sm hover:shadow-md transition-all active:scale-[0.98]"
        >
          <Plus className="h-4 w-4" />
          <span>新对话</span>
        </Button>
      </div>

      {/* Conversations list */}
      <ScrollArea className="flex-1 px-2">
        <div className="space-y-1 py-2">
          {isLoading ? (
            <div className="text-sm text-muted-foreground text-center py-8">
              加载中...
            </div>
          ) : conversations.length === 0 ? (
            <div className="text-sm text-muted-foreground text-center py-8">
              暂无对话
            </div>
          ) : (
            conversations.map((conv) => (
              <ConversationItem
                key={conv.id}
                id={conv.id}
                title={conv.title}
                isActive={currentId === conv.id}
                onSelect={() => handleSelectConversation(conv.id)}
                onDelete={() => handleDeleteConversation(conv.id)}
              />
            ))
          )}
        </div>
      </ScrollArea>
    </aside>
  );
}

interface ConversationItemProps {
  id: string;
  title: string;
  isActive: boolean;
  onSelect: () => void;
  onDelete: () => void;
}

function ConversationItem({
  title,
  isActive,
  onSelect,
  onDelete,
}: ConversationItemProps) {
  return (
    <div
      className={cn(
        'group flex items-center gap-2 px-3 py-2 rounded-lg cursor-pointer transition-all duration-200',
        isActive
          ? 'bg-secondary text-secondary-foreground shadow-sm ring-1 ring-border'
          : 'text-muted-foreground hover:bg-muted hover:text-foreground'
      )}
      onClick={onSelect}
    >
      <MessageSquare className={cn("h-4 w-4 shrink-0 transition-colors", isActive ? "text-primary" : "text-muted-foreground/60Group-hover:text-foreground")} />
      <span className="flex-1 truncate text-xs font-medium">{title || '新对话'}</span>

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
          <DropdownMenuItem disabled>
            <Edit2 className="mr-2 h-4 w-4" />
            重命名
          </DropdownMenuItem>
          <DropdownMenuItem
            className="text-destructive focus:bg-destructive/10 focus:text-destructive"
            onClick={(e) => {
              e.stopPropagation();
              onDelete();
            }}
          >
            <Trash2 className="mr-2 h-4 w-4" />
            删除
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
