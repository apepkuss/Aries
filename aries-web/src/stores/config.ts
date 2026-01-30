import { create } from 'zustand';
import {
  getConfig,
  updateConfig,
  getConfigSchema,
} from '@/api/config';
import type {
  SanitizedConfig,
  ConfigUpdateRequest,
  ConfigUpdateResponse,
  ConfigSchemaResponse,
} from '@/api/types';

interface ConfigState {
  // Data
  config: SanitizedConfig | null;
  schema: ConfigSchemaResponse | null;

  // Loading states
  isLoading: boolean;
  isSaving: boolean;

  // Error
  error: string | null;

  // Pending changes (not yet saved)
  pendingChanges: ConfigUpdateRequest;

  // Actions
  fetchConfig: () => Promise<void>;
  fetchSchema: () => Promise<void>;
  setPendingChange: <K extends keyof ConfigUpdateRequest>(
    section: K,
    field: string,
    value: unknown
  ) => void;
  clearPendingChanges: () => void;
  saveChanges: () => Promise<ConfigUpdateResponse | null>;
  hasPendingChanges: () => boolean;
}

export const useConfigStore = create<ConfigState>((set, get) => ({
  // Initial state
  config: null,
  schema: null,
  isLoading: false,
  isSaving: false,
  error: null,
  pendingChanges: {},

  // Fetch current config
  fetchConfig: async () => {
    // Prevent duplicate requests
    if (get().isLoading) return;

    set({ isLoading: true, error: null });
    try {
      const config = await getConfig();
      set({ config, isLoading: false });
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to fetch config';
      console.error('Failed to fetch config:', message);
      set({
        error: `Failed to load configuration. Make sure the backend server is running. (${message})`,
        isLoading: false,
      });
    }
  },

  // Fetch config schema
  fetchSchema: async () => {
    // Skip if already loaded
    if (get().schema) return;

    try {
      const schema = await getConfigSchema();
      set({ schema });
    } catch (err) {
      console.error('Failed to fetch schema:', err);
      // Schema is optional, don't set error state
    }
  },

  // Set a pending change
  setPendingChange: (section, field, value) => {
    set((state) => {
      const currentSection = state.pendingChanges[section] || {};

      return {
        pendingChanges: {
          ...state.pendingChanges,
          [section]: {
            ...currentSection,
            [field]: value,
          },
        },
      };
    });
  },

  // Clear all pending changes
  clearPendingChanges: () => {
    set({ pendingChanges: {} });
  },

  // Check if there are pending changes
  hasPendingChanges: () => {
    const { pendingChanges } = get();
    return Object.keys(pendingChanges).some(
      (key) => Object.keys(pendingChanges[key as keyof ConfigUpdateRequest] || {}).length > 0
    );
  },

  // Save pending changes
  saveChanges: async () => {
    const { pendingChanges, hasPendingChanges } = get();

    if (!hasPendingChanges()) {
      return null;
    }

    set({ isSaving: true, error: null });

    try {
      const response = await updateConfig(pendingChanges);

      if (response.success) {
        // Refresh config to get updated values
        const config = await getConfig();
        set({
          config,
          pendingChanges: {},
          isSaving: false,
        });
      } else {
        set({
          error: response.message,
          isSaving: false,
        });
      }

      return response;
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to save config';
      set({
        error: errorMessage,
        isSaving: false,
      });
      return null;
    }
  },
}));
