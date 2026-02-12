import { create } from 'zustand';
import { getMcpServers, toggleMcpServer, updateMcpApiKey } from '@/api/mcp';
import type { SanitizedMcpToolServer } from '@/api/types';

interface McpState {
  servers: SanitizedMcpToolServer[];
  isLoading: boolean;
  error: string | null;
  /** Name of the server currently being toggled */
  togglingServer: string | null;
  fetchServers: () => Promise<void>;
  toggleServer: (name: string, enable: boolean) => Promise<void>;
  updateApiKey: (name: string, apiKey: string, apiKeyParam?: string) => Promise<void>;
}

export const useMcpStore = create<McpState>((set, get) => ({
  servers: [],
  isLoading: false,
  error: null,
  togglingServer: null,

  fetchServers: async () => {
    if (get().isLoading) return;

    set({ isLoading: true, error: null });
    try {
      const response = await getMcpServers();
      set({ servers: response.servers, isLoading: false });
    } catch (err) {
      console.warn('Failed to fetch MCP servers:', err instanceof Error ? err.message : err);
      set({ servers: [], isLoading: false, error: null });
    }
  },

  toggleServer: async (name: string, enable: boolean) => {
    set({ togglingServer: name, error: null });
    try {
      const response = await toggleMcpServer(name, enable);
      // Update the specific server in local state
      set((state) => ({
        servers: state.servers.map((s) =>
          s.name === name ? response.server : s
        ),
        togglingServer: null,
      }));
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Toggle failed';
      set({ togglingServer: null, error: message });
      throw err;
    }
  },

  updateApiKey: async (name: string, apiKey: string, apiKeyParam?: string) => {
    set({ error: null });
    try {
      await updateMcpApiKey(name, apiKey, apiKeyParam);
      // Refresh server list to get updated state
      await get().fetchServers();
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Update failed';
      set({ error: message });
      throw err;
    }
  },
}));
