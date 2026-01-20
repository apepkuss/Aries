import { create } from 'zustand';
import {
  getConversations,
  getConversationHistory,
  deleteConversation,
  renameConversation,
} from '@/api/memory';
import type { Conversation, UIMessage } from '@/api/types';

interface ConversationsState {
  // Data
  conversations: Conversation[];
  currentId: string | null;

  // Loading states
  isLoading: boolean;
  isLoadingHistory: boolean;

  // Error
  error: string | null;

  // Actions
  fetchConversations: () => Promise<void>;
  selectConversation: (id: string) => Promise<UIMessage[]>;
  deleteConversation: (id: string) => Promise<void>;
  renameConversation: (id: string, title: string) => Promise<void>;
  setCurrentId: (id: string | null) => void;
  clearCurrent: () => void;
}

export const useConversationsStore = create<ConversationsState>((set, get) => ({
  // Initial state
  conversations: [],
  currentId: null,
  isLoading: false,
  isLoadingHistory: false,
  error: null,

  // Fetch all conversations
  fetchConversations: async () => {
    // Prevent duplicate requests
    if (get().isLoading) return;

    set({ isLoading: true, error: null });
    try {
      const response = await getConversations();
      set({
        conversations: response.conversations,
        isLoading: false,
      });
    } catch (err) {
      // Don't show error for connection issues - backend may not be running
      const message = err instanceof Error ? err.message : 'Failed to fetch conversations';
      console.warn('Failed to fetch conversations:', message);
      set({
        conversations: [],
        isLoading: false,
        // Only set error for non-network issues
        error: null,
      });
    }
  },

  // Select a conversation and load its history
  selectConversation: async (id: string) => {
    set({ isLoadingHistory: true, currentId: id, error: null });
    try {
      const response = await getConversationHistory(id);

      // Convert to UI messages
      const messages: UIMessage[] = response.messages.map((msg) => ({
        id: msg.id,
        role: msg.role,
        content: msg.content,
        timestamp: new Date(msg.timestamp),
        toolCalls: msg.tool_calls?.map((tc) => ({
          id: tc.id,
          name: tc.function.name,
          arguments: JSON.parse(tc.function.arguments || '{}'),
          status: 'success' as const,
        })),
      }));

      set({ isLoadingHistory: false });
      return messages;
    } catch (err) {
      set({
        error: err instanceof Error ? err.message : 'Failed to load conversation',
        isLoadingHistory: false,
      });
      return [];
    }
  },

  // Delete a conversation
  deleteConversation: async (id: string) => {
    const { conversations, currentId } = get();
    try {
      await deleteConversation(id);
      set({
        conversations: conversations.filter((c) => c.id !== id),
        currentId: currentId === id ? null : currentId,
      });
    } catch (err) {
      set({
        error: err instanceof Error ? err.message : 'Failed to delete conversation',
      });
    }
  },

  // Rename a conversation
  renameConversation: async (id: string, title: string) => {
    const { conversations } = get();
    try {
      const updated = await renameConversation(id, title);
      set({
        conversations: conversations.map((c) => (c.id === id ? updated : c)),
      });
    } catch (err) {
      set({
        error: err instanceof Error ? err.message : 'Failed to rename conversation',
      });
    }
  },

  // Set current conversation ID without loading
  setCurrentId: (id) => {
    set({ currentId: id });
  },

  // Clear current selection
  clearCurrent: () => {
    set({ currentId: null });
  },
}));
