import { useState } from 'react';
import { MessageSquare, Trash2, Edit2, MoreHorizontal, CheckSquare, X } from 'lucide-react';
import { Button } from '@/components/ui/button';

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { cn } from '@/lib/utils';
import {
  useUIStore,
  useConversationsStore,
  useChatStore,
  useConfigStore,
  useSessionsStore,
} from '@/stores';
import { useEffect } from 'react';
import { SessionList } from '@/components/session/SessionList';
import { SessionBatchDeleteDialog } from '@/components/session/SessionBatchDeleteDialog';

export function Sidebar() {
  const { sidebarOpen, activeView } = useUIStore();
  const {
    conversations,
    currentId: currentConvId,
    isLoading: isLoadingConv,
    fetchConversations,
    selectConversation,
    deleteConversation: removeConversation,
  } = useConversationsStore();
  const {
    sessions,
    currentId: currentSessionId,
    isLoading: isLoadingSessions,
    isSelectMode,
    selectedIds,
    fetchSessions,
    selectSession,
    deleteSession: removeSession,
    toggleSelectMode,
    toggleSelect,
    selectAll,
    batchDelete,
    deleteAll,
  } = useSessionsStore();
  const { clearMessages, loadMessages, setConversationId, setSessionId } = useChatStore();
  const { config } = useConfigStore();
  const [batchDeleteOpen, setBatchDeleteOpen] = useState(false);
  const [deleteAllOpen, setDeleteAllOpen] = useState(false);

  // Feature flags
  const memoryEnabled = config?.memory?.enable ?? false;
  const sessionEnabled = config?.session?.enable ?? false;

  // Use session history as primary source; fall back to memory conversations
  const useSessionHistory = sessionEnabled;

  // Fetch data on mount based on which feature is enabled
  useEffect(() => {
    if (useSessionHistory) {
      fetchSessions();
    } else if (memoryEnabled) {
      fetchConversations();
    }
  }, [fetchSessions, fetchConversations, useSessionHistory, memoryEnabled]);

  // --- Session history handlers ---
  const handleSelectSession = async (id: string) => {
    try {
      // Clear memory conversation selection
      useConversationsStore.getState().clearCurrent();

      // Abort any active stream and clear HITL state before loading new session
      clearMessages();

      const messages = await selectSession(id);
      loadMessages(messages);
      setConversationId(null);
      setSessionId(id); // Continue appending to this session
    } catch (err) {
      console.error('[Sidebar] Failed to load session:', err);
    } finally {
      useSessionsStore.getState().setLoadingDetail(false);
    }
  };

  const handleDeleteSession = async (id: string) => {
    await removeSession(id);
    if (currentSessionId === id) {
      clearMessages();
    }
  };

  // --- Memory conversation handlers ---
  const handleSelectConversation = async (id: string) => {
    // Clear session selection
    useSessionsStore.getState().clearCurrent();

    // Abort any active stream and clear HITL state before loading new conversation
    clearMessages();

    const messages = await selectConversation(id);
    loadMessages(messages);
    setConversationId(id);
  };

  const handleDeleteConversation = async (id: string) => {
    await removeConversation(id);
    if (currentConvId === id) {
      clearMessages();
    }
  };

  if (!sidebarOpen) {
    return null;
  }

  const historyDisabled = !useSessionHistory && !memoryEnabled;

  const handleBatchDelete = async () => {
    await batchDelete();
    clearMessages();
  };

  const handleDeleteAll = async () => {
    await deleteAll();
    clearMessages();
  };

  return (
    <aside className="w-60 border-r bg-muted/10 backdrop-blur-sm flex flex-col min-h-0 overflow-hidden transition-all duration-300">
      {/* Panel title */}
      <div className="px-3 py-2.5 border-b border-border/50">
        <span className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          {activeView === 'skills' ? 'Skills' : activeView === 'mcp' ? 'MCP Servers' : 'Chat History'}
        </span>
      </div>

      {activeView === 'chat' && (
        <>
          {/* Toolbar for session actions */}
          {useSessionHistory && sessions.length > 0 && (
            <div className="flex items-center gap-1 px-3 py-2 border-b border-border/50">
              {isSelectMode ? (
                <>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 text-xs"
                    onClick={selectAll}
                  >
                    全选
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 text-xs text-destructive hover:text-destructive"
                    onClick={() => setBatchDeleteOpen(true)}
                    disabled={selectedIds.size === 0}
                  >
                    删除 ({selectedIds.size})
                  </Button>
                  <div className="flex-1" />
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7"
                    onClick={toggleSelectMode}
                  >
                    <X className="h-3.5 w-3.5" />
                  </Button>
                </>
              ) : (
                <>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 text-xs"
                    onClick={toggleSelectMode}
                  >
                    <CheckSquare className="h-3.5 w-3.5 mr-1" />
                    选择
                  </Button>
                  <div className="flex-1" />
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 text-xs text-destructive hover:text-destructive"
                    onClick={() => setDeleteAllOpen(true)}
                  >
                    <Trash2 className="h-3.5 w-3.5 mr-1" />
                    清空
                  </Button>
                </>
              )}
            </div>
          )}

          {/* History list */}
          <div className="flex-1 min-h-0 overflow-y-auto px-2">
            <div className="space-y-1 py-2">
              {historyDisabled ? (
                <div className="text-sm text-muted-foreground text-center py-8 px-2">
                  会话历史已禁用
                </div>
              ) : useSessionHistory ? (
                <SessionList
                  sessions={sessions}
                  currentId={currentSessionId}
                  isLoading={isLoadingSessions}
                  onSelect={handleSelectSession}
                  onDelete={handleDeleteSession}
                  isSelectMode={isSelectMode}
                  selectedIds={selectedIds}
                  onToggleSelect={toggleSelect}
                />
              ) : isLoadingConv ? (
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
                    isActive={currentConvId === conv.id}
                    onSelect={() => handleSelectConversation(conv.id)}
                    onDelete={() => handleDeleteConversation(conv.id)}
                  />
                ))
              )}
            </div>
          </div>

          {/* Batch delete dialog */}
          <SessionBatchDeleteDialog
            open={batchDeleteOpen}
            onOpenChange={setBatchDeleteOpen}
            onConfirm={handleBatchDelete}
            count={selectedIds.size}
          />

          {/* Delete all dialog */}
          <SessionBatchDeleteDialog
            open={deleteAllOpen}
            onOpenChange={setDeleteAllOpen}
            onConfirm={handleDeleteAll}
            count={0}
          />
        </>
      )}

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
