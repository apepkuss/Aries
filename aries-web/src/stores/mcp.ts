import { create } from 'zustand';
import { getMcpServers, toggleMcpServer, updateMcpApiKey } from '@/api/mcp';
import type { SanitizedMcpToolServer } from '@/api/types';

interface McpState {
  servers: SanitizedMcpToolServer[];
  isLoading: boolean;
  /** Per-server error messages (keyed by server name) */
  serverErrors: Record<string, string>;
  /** Name of the server currently being toggled */
  togglingServer: string | null;
  fetchServers: () => Promise<void>;
  toggleServer: (name: string, enable: boolean) => Promise<void>;
  updateApiKey: (name: string, apiKey: string, apiKeyParam?: string) => Promise<void>;
}

export const useMcpStore = create<McpState>((set, get) => ({
  servers: [],
  isLoading: false,
  serverErrors: {},
  togglingServer: null,

  fetchServers: async () => {
    if (get().isLoading) return;

    set({ isLoading: true, serverErrors: {} });
    try {
      const response = await getMcpServers();
      set({ servers: response.servers, isLoading: false });
    } catch (err) {
      console.warn('Failed to fetch MCP servers:', err instanceof Error ? err.message : err);
      set({ servers: [], isLoading: false });
    }
  },

  toggleServer: async (name: string, enable: boolean) => {
    set((state) => ({
      togglingServer: name,
      serverErrors: { ...state.serverErrors, [name]: undefined as unknown as string },
    }));
    try {
      const response = await toggleMcpServer(name, enable);
      // Update the specific server in local state
      set((state) => {
        const { [name]: _, ...restErrors } = state.serverErrors;
        return {
          servers: state.servers.map((s) =>
            s.name === name ? response.server : s
          ),
          togglingServer: null,
          serverErrors: restErrors,
        };
      });
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Toggle failed';
      console.error(`Failed to toggle MCP server '${name}':`, err);
      set((state) => ({
        togglingServer: null,
        serverErrors: { ...state.serverErrors, [name]: message || 'Toggle failed' },
      }));
    }
  },

  updateApiKey: async (name: string, apiKey: string, apiKeyParam?: string) => {
    try {
      await updateMcpApiKey(name, apiKey, apiKeyParam);
      // Refresh server list to get updated state
      await get().fetchServers();
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Update failed';
      set((state) => ({
        serverErrors: { ...state.serverErrors, [name]: message },
      }));
      throw err;
    }
  },
}));
