import { create } from 'zustand';
import {
  browseClawHub,
  searchClawHub,
  installClawHubSkill,
} from '@/api/clawhub';
import type { ClawHubSkill } from '@/api/types';

interface ClawHubState {
  skills: ClawHubSkill[];
  isLoading: boolean;
  isSearching: boolean;
  installingSlug: string | null;
  error: string | null;
  installError: string | null;
  cursor: string | null;
  hasMore: boolean;
  sortBy: string;
  searchQuery: string;

  browse: (reset?: boolean) => Promise<void>;
  search: (query: string) => Promise<void>;
  clearSearch: () => void;
  install: (slug: string, version?: string) => Promise<boolean>;
  setSortBy: (sort: string) => void;
  clearInstallError: () => void;
}

let lastBrowseTime = 0;

export const useClawHubStore = create<ClawHubState>((set, get) => ({
  skills: [],
  isLoading: false,
  isSearching: false,
  installingSlug: null,
  error: null,
  installError: null,
  cursor: null,
  hasMore: true,
  sortBy: 'trending',
  searchQuery: '',

  browse: async (reset = false) => {
    const state = get();
    if (state.isLoading) return;

    // Throttle: minimum 1s between browse requests to avoid 429
    const now = Date.now();
    const elapsed = now - lastBrowseTime;
    if (!reset && elapsed < 1000) {
      await new Promise((r) => setTimeout(r, 1000 - elapsed));
    }
    lastBrowseTime = Date.now();
    if (!reset && !state.hasMore) return;

    set({
      isLoading: true,
      error: null,
      ...(reset ? { skills: [], cursor: null, hasMore: true } : {}),
    });

    try {
      const result = await browseClawHub({
        limit: 20,
        cursor: reset ? undefined : state.cursor ?? undefined,
        sort: state.sortBy,
      });

      set((s) => ({
        skills: reset ? result.skills : [...s.skills, ...result.skills],
        cursor: result.cursor ?? null,
        hasMore: !!result.cursor && result.skills.length > 0,
        isLoading: false,
      }));
    } catch (err) {
      set({
        isLoading: false,
        error: err instanceof Error ? err.message : 'Failed to load skills from ClawHub',
      });
    }
  },

  search: async (query: string) => {
    if (!query.trim()) {
      get().clearSearch();
      return;
    }

    set({ isSearching: true, error: null, searchQuery: query });

    try {
      const result = await searchClawHub(query, 50);
      set({
        skills: result.skills,
        isSearching: false,
        hasMore: false,
        cursor: null,
      });
    } catch (err) {
      set({
        isSearching: false,
        error: err instanceof Error ? err.message : 'Search failed',
      });
    }
  },

  clearSearch: () => {
    set({ searchQuery: '', skills: [], cursor: null, hasMore: true });
    get().browse(true);
  },

  install: async (slug: string, version?: string) => {
    set({ installingSlug: slug, installError: null });
    try {
      const result = await installClawHubSkill({ slug, version });
      set({ installingSlug: null });
      return result.success;
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Installation failed';
      set({ installingSlug: null, installError: message });
      return false;
    }
  },

  setSortBy: (sort: string) => {
    set({ sortBy: sort });
    get().browse(true);
  },

  clearInstallError: () => set({ installError: null }),
}));
