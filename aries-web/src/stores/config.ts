import { create } from 'zustand';
import {
  getConfig,
  updateConfig,
  getConfigSchema,
  testChatService,
} from '@/api/config';
import type {
  SanitizedConfig,
  ConfigUpdateRequest,
  ConfigUpdateResponse,
  ConfigSchemaResponse,
} from '@/api/types';

type UrlTestState = 'idle' | 'testing' | 'passed' | 'failed';

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

  // URL test state
  urlTestState: UrlTestState;
  urlTestError: string | null;

  // Privacy chat URL test state
  privacyUrlTestState: UrlTestState;
  privacyUrlTestError: string | null;

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
  hasUrlChange: () => boolean;
  hasPrivacyUrlChange: () => boolean;
  testChatUrl: () => Promise<boolean>;
  testPrivacyChatUrl: () => Promise<boolean>;
  resetUrlTest: () => void;
}

export const useConfigStore = create<ConfigState>((set, get) => ({
  // Initial state
  config: null,
  schema: null,
  isLoading: false,
  isSaving: false,
  error: null,
  pendingChanges: {},
  urlTestState: 'idle',
  urlTestError: null,
  privacyUrlTestState: 'idle',
  privacyUrlTestError: null,

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

      // Reset URL test state when URL changes
      const newUrlTestState =
        section === 'chat' && field === 'url' ? 'idle' : state.urlTestState;
      const newUrlTestError =
        section === 'chat' && field === 'url' ? null : state.urlTestError;
      const newPrivacyUrlTestState =
        section === 'privacy_chat' && field === 'url' ? 'idle' : state.privacyUrlTestState;
      const newPrivacyUrlTestError =
        section === 'privacy_chat' && field === 'url' ? null : state.privacyUrlTestError;

      return {
        pendingChanges: {
          ...state.pendingChanges,
          [section]: {
            ...currentSection,
            [field]: value,
          },
        },
        urlTestState: newUrlTestState as UrlTestState,
        urlTestError: newUrlTestError,
        privacyUrlTestState: newPrivacyUrlTestState as UrlTestState,
        privacyUrlTestError: newPrivacyUrlTestError,
      };
    });
  },

  // Clear all pending changes
  clearPendingChanges: () => {
    set({ pendingChanges: {}, urlTestState: 'idle', urlTestError: null, privacyUrlTestState: 'idle', privacyUrlTestError: null });
  },

  // Check if there are pending changes
  hasPendingChanges: () => {
    const { pendingChanges } = get();
    return Object.keys(pendingChanges).some(
      (key) => Object.keys(pendingChanges[key as keyof ConfigUpdateRequest] || {}).length > 0
    );
  },

  // Check if URL has changed
  hasUrlChange: () => {
    const { config, pendingChanges } = get();
    const pendingUrl = pendingChanges.chat?.url;
    const savedUrl = config?.chat?.url;

    // URL has changed if there's a pending URL that differs from saved
    return pendingUrl !== undefined && pendingUrl !== savedUrl;
  },

  // Check if privacy chat URL has changed
  hasPrivacyUrlChange: () => {
    const { config, pendingChanges } = get();
    const pendingUrl = pendingChanges.privacy_chat?.url;
    const savedUrl = config?.privacy_chat?.url;

    return pendingUrl !== undefined && pendingUrl !== savedUrl;
  },

  // Test chat service URL connectivity
  testChatUrl: async () => {
    const { pendingChanges } = get();
    const url = pendingChanges.chat?.url;

    if (!url) {
      set({ urlTestError: 'URL is required' });
      return false;
    }

    set({ urlTestState: 'testing', urlTestError: null });

    try {
      const response = await testChatService({
        url,
        api_key: pendingChanges.chat?.api_key || undefined,
      });

      if (response.success) {
        set({ urlTestState: 'passed', urlTestError: null });
        return true;
      } else {
        set({ urlTestState: 'failed', urlTestError: response.error || 'Connection test failed' });
        return false;
      }
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to test connection';
      set({ urlTestState: 'failed', urlTestError: errorMessage });
      return false;
    }
  },

  // Test privacy chat service URL connectivity
  testPrivacyChatUrl: async () => {
    const { pendingChanges } = get();
    const url = pendingChanges.privacy_chat?.url;

    if (!url) {
      set({ privacyUrlTestError: 'URL is required' });
      return false;
    }

    set({ privacyUrlTestState: 'testing', privacyUrlTestError: null });

    try {
      const response = await testChatService({
        url,
        api_key: pendingChanges.privacy_chat?.api_key || undefined,
      });

      if (response.success) {
        set({ privacyUrlTestState: 'passed', privacyUrlTestError: null });
        return true;
      } else {
        set({ privacyUrlTestState: 'failed', privacyUrlTestError: response.error || 'Connection test failed' });
        return false;
      }
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to test connection';
      set({ privacyUrlTestState: 'failed', privacyUrlTestError: errorMessage });
      return false;
    }
  },

  // Reset URL test state
  resetUrlTest: () => {
    set({ urlTestState: 'idle', urlTestError: null, privacyUrlTestState: 'idle', privacyUrlTestError: null });
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
          urlTestState: 'idle',
          urlTestError: null,
          privacyUrlTestState: 'idle',
          privacyUrlTestError: null,
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
