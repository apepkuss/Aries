import { create } from 'zustand';
import { getSessions, getSessionDetail, deleteSession } from '@/api/session';
import type { SessionMeta, UIMessage } from '@/api/types';

interface SessionsState {
  // Data
  sessions: SessionMeta[];
  currentId: string | null;

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
}

export const useSessionsStore = create<SessionsState>((set, get) => ({
  // Initial state
  sessions: [],
  currentId: null,
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
}));
