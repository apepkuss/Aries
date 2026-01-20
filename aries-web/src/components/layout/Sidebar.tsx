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
    <aside className="w-64 border-r bg-muted/30 flex flex-col">
      {/* New chat button */}
      <div className="p-3">
        <Button onClick={handleNewChat} className="w-full justify-start gap-2">
          <Plus className="h-4 w-4" />
          New Chat
        </Button>
      </div>

      {/* Conversations list */}
      <ScrollArea className="flex-1">
        <div className="p-2 space-y-1">
          {isLoading ? (
            <div className="text-sm text-muted-foreground text-center py-4">
              Loading...
            </div>
          ) : conversations.length === 0 ? (
            <div className="text-sm text-muted-foreground text-center py-4">
              No conversations yet
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
        'group flex items-center gap-2 px-2 py-2 rounded-md cursor-pointer hover:bg-muted',
        isActive && 'bg-muted'
      )}
      onClick={onSelect}
    >
      <MessageSquare className="h-4 w-4 shrink-0 text-muted-foreground" />
      <span className="flex-1 truncate text-sm">{title || 'New Chat'}</span>

      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6 opacity-0 group-hover:opacity-100"
            onClick={(e) => e.stopPropagation()}
          >
            <MoreHorizontal className="h-4 w-4" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem disabled>
            <Edit2 className="mr-2 h-4 w-4" />
            Rename
          </DropdownMenuItem>
          <DropdownMenuItem
            className="text-destructive"
            onClick={(e) => {
              e.stopPropagation();
              onDelete();
            }}
          >
            <Trash2 className="mr-2 h-4 w-4" />
            Delete
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
