import { create } from 'zustand';
import {
  getSessions,
  getSessionDetail,
  deleteSession,
  batchDeleteSessions,
  deleteAllSessions,
} from '@/api/session';
import type { SessionMeta, UIMessage } from '@/api/types';

interface SessionsState {
  // Data
  sessions: SessionMeta[];
  currentId: string | null;

  // Multi-select
  isSelectMode: boolean;
  selectedIds: Set<string>;

  // Loading states
  isLoading: boolean;
  isLoadingDetail: boolean;

  // Error
  error: string | null;

  // Actions
  fetchSessions: () => Promise<void>;
  selectSession: (id: string) => Promise<UIMessage[]>;
  deleteSession: (id: string) => Promise<void>;
  setCurrentId: (id: string | null) => void;
  clearCurrent: () => void;

  // Multi-select actions
  toggleSelectMode: () => void;
  toggleSelect: (id: string) => void;
  selectAll: () => void;
  deselectAll: () => void;
  batchDelete: () => Promise<void>;
  deleteAll: () => Promise<void>;
}

export const useSessionsStore = create<SessionsState>((set, get) => ({
  // Initial state
  sessions: [],
  currentId: null,
  isSelectMode: false,
  selectedIds: new Set(),
  isLoading: false,
  isLoadingDetail: false,
  error: null,

  // Fetch all sessions
  fetchSessions: async () => {
    if (get().isLoading) return;

    set({ isLoading: true, error: null });
    try {
      const response = await getSessions();
      set({
        sessions: response.sessions,
        isLoading: false,
      });
    } catch (err) {
      console.warn('Failed to fetch sessions:', err instanceof Error ? err.message : err);
      set({
        sessions: [],
        isLoading: false,
        error: null,
      });
    }
  },

  // Select a session and load its records
  selectSession: async (id: string) => {
    set({ isLoadingDetail: true, currentId: id, error: null });
    try {
      const response = await getSessionDetail(id);

      // Convert session records to UI messages (skip session_start records)
      const messages: UIMessage[] = response.records
        .filter((r): r is Extract<typeof r, { type: 'message' }> => r.type === 'message')
        .filter((r) => r.role === 'user' || r.role === 'assistant')
        .map((r) => ({
          id: r.message_id,
          role: r.role as 'user' | 'assistant',
          content: r.content,
          timestamp: new Date(r.timestamp),
          privacyMode: r.privacy_mode === true ? true : undefined,
        }));

      set({ isLoadingDetail: false });
      return messages;
    } catch (err) {
      set({
        error: err instanceof Error ? err.message : 'Failed to load session',
        isLoadingDetail: false,
      });
      return [];
    }
  },

  // Delete a session
  deleteSession: async (id: string) => {
    const { sessions, currentId } = get();
    try {
      await deleteSession(id);
      set({
        sessions: sessions.filter((s) => s.session_id !== id),
        currentId: currentId === id ? null : currentId,
      });
    } catch (err) {
      set({
        error: err instanceof Error ? err.message : 'Failed to delete session',
      });
    }
  },

  // Set current session ID without loading
  setCurrentId: (id) => {
    set({ currentId: id });
  },

  // Clear current selection
  clearCurrent: () => {
    set({ currentId: null });
  },

  // Toggle multi-select mode
  toggleSelectMode: () => {
    const { isSelectMode } = get();
    set({
      isSelectMode: !isSelectMode,
      selectedIds: new Set(),
    });
  },

  // Toggle selection of a single item
  toggleSelect: (id: string) => {
    const { selectedIds } = get();
    const next = new Set(selectedIds);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    set({ selectedIds: next });
  },

  // Select all sessions
  selectAll: () => {
    const { sessions } = get();
    set({ selectedIds: new Set(sessions.map((s) => s.session_id)) });
  },

  // Deselect all
  deselectAll: () => {
    set({ selectedIds: new Set() });
  },

  // Batch delete selected sessions
  batchDelete: async () => {
    const { selectedIds, sessions, currentId } = get();
    const ids = Array.from(selectedIds);
    if (ids.length === 0) return;

    try {
      await batchDeleteSessions(ids);
      const remaining = sessions.filter((s) => !selectedIds.has(s.session_id));
      set({
        sessions: remaining,
        selectedIds: new Set(),
        isSelectMode: false,
        currentId: selectedIds.has(currentId ?? '') ? null : currentId,
      });
    } catch (err) {
      set({
        error: err instanceof Error ? err.message : 'Failed to batch delete sessions',
      });
    }
  },

  // Delete all sessions
  deleteAll: async () => {
    try {
      await deleteAllSessions();
      set({
        sessions: [],
        selectedIds: new Set(),
        isSelectMode: false,
        currentId: null,
      });
    } catch (err) {
      set({
        error: err instanceof Error ? err.message : 'Failed to delete all sessions',
      });
    }
  },
}));
