import { create } from 'zustand';
import { persist } from 'zustand/middleware';

type Theme = 'light' | 'dark' | 'system';
type ActiveView = 'chat' | 'skills' | 'mcp';

interface UIState {
  // Sidebar
  sidebarOpen: boolean;
  toggleSidebar: () => void;
  setSidebarOpen: (open: boolean) => void;

  // Active view
  activeView: ActiveView;
  setActiveView: (view: ActiveView) => void;

  // Activity bar click handler (VSCode-style toggle)
  handleActivityBarClick: (view: ActiveView) => void;

  // Artifacts panel
  artifactsPanelOpen: boolean;
  toggleArtifactsPanel: () => void;
  setArtifactsPanelOpen: (open: boolean) => void;

  // Theme
  theme: Theme;
  setTheme: (theme: Theme) => void;

  // Thinking process collapse state
  thinkingCollapsed: boolean;
  setThinkingCollapsed: (collapsed: boolean) => void;

  // Settings dialog
  settingsOpen: boolean;
  setSettingsOpen: (open: boolean) => void;
}

export const useUIStore = create<UIState>()(
  persist(
    (set) => ({
      // Sidebar (default collapsed, not persisted)
      sidebarOpen: false,
      toggleSidebar: () => set((state) => ({ sidebarOpen: !state.sidebarOpen })),
      setSidebarOpen: (open) => set({ sidebarOpen: open }),

      // Active view
      activeView: 'chat',
      setActiveView: (view) => set({ activeView: view }),

      // Activity bar click: chat toggles panel, skills/mcp switch view only
      handleActivityBarClick: (view) =>
        set((state) => {
          if (view === 'chat') {
            if (state.activeView === 'chat' && state.sidebarOpen) {
              return { sidebarOpen: false };
            }
            return { activeView: 'chat', sidebarOpen: true };
          }
          // Skills/MCP: switch view, close panel
          return { activeView: view, sidebarOpen: false };
        }),

      // Artifacts panel
      artifactsPanelOpen: false,
      toggleArtifactsPanel: () =>
        set((state) => ({ artifactsPanelOpen: !state.artifactsPanelOpen })),
      setArtifactsPanelOpen: (open) => set({ artifactsPanelOpen: open }),

      // Theme
      theme: 'system',
      setTheme: (theme) => set({ theme }),

      // Thinking process
      thinkingCollapsed: true,
      setThinkingCollapsed: (collapsed) => set({ thinkingCollapsed: collapsed }),

      // Settings dialog
      settingsOpen: false,
      setSettingsOpen: (open) => set({ settingsOpen: open }),
    }),
    {
      name: 'moss-ui-storage',
      partialize: (state) => ({
        // sidebarOpen is intentionally not persisted - always starts collapsed
        artifactsPanelOpen: state.artifactsPanelOpen,
        theme: state.theme,
        thinkingCollapsed: state.thinkingCollapsed,
      }),
    }
  )
);

// Helper to apply theme to document
export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  const systemDark = window.matchMedia('(prefers-color-scheme: dark)').matches;

  if (theme === 'dark' || (theme === 'system' && systemDark)) {
    root.classList.add('dark');
  } else {
    root.classList.remove('dark');
  }
}
