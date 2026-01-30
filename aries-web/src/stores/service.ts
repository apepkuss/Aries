import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import { registerServer, unregisterServer } from '@/api/admin';

export interface ServiceConfig {
  url: string;
  apiKey: string;
  model: string;
  serverId?: string;
}

type TestState = 'idle' | 'testing' | 'passed' | 'failed';

interface ServiceState {
  // Saved service configs (persisted to localStorage)
  chat: ServiceConfig | null;
  privacyChat: ServiceConfig | null;

  // Registration status (not persisted — re-registered on startup)
  chatRegistered: boolean;
  privacyChatRegistered: boolean;

  // Pending edits (form values not yet registered)
  pendingChat: { url?: string; apiKey?: string };
  pendingPrivacyChat: { url?: string; apiKey?: string };

  // Connection test state
  chatTestState: TestState;
  chatTestError: string | null;
  privacyChatTestState: TestState;
  privacyChatTestError: string | null;

  // Pending edit actions
  setPendingChat: (field: 'url' | 'apiKey', value: string) => void;
  setPendingPrivacyChat: (field: 'url' | 'apiKey', value: string) => void;
  clearPending: () => void;

  // Query helpers
  hasChatUrlChange: () => boolean;
  hasPrivacyChatUrlChange: () => boolean;
  hasServicePendingChanges: () => boolean;

  // Connect = register + save to localStorage
  connectChat: () => Promise<boolean>;
  connectPrivacyChat: () => Promise<boolean>;

  // Model management (no re-registration needed)
  setChatModel: (model: string) => void;
  setPrivacyChatModel: (model: string) => void;

  // Auto-register saved configs on app startup
  autoRegister: () => Promise<void>;
}

export const useServiceStore = create<ServiceState>()(
  persist(
    (set, get) => ({
      // Initial state
      chat: null,
      privacyChat: null,
      chatRegistered: false,
      privacyChatRegistered: false,
      pendingChat: {},
      pendingPrivacyChat: {},
      chatTestState: 'idle',
      chatTestError: null,
      privacyChatTestState: 'idle',
      privacyChatTestError: null,

      // Set pending edit for chat service
      setPendingChat: (field, value) => {
        set((state) => ({
          pendingChat: { ...state.pendingChat, [field]: value },
          // Reset test state when URL changes
          ...(field === 'url'
            ? { chatTestState: 'idle' as TestState, chatTestError: null }
            : {}),
        }));
      },

      // Set pending edit for privacy chat service
      setPendingPrivacyChat: (field, value) => {
        set((state) => ({
          pendingPrivacyChat: { ...state.pendingPrivacyChat, [field]: value },
          ...(field === 'url'
            ? {
                privacyChatTestState: 'idle' as TestState,
                privacyChatTestError: null,
              }
            : {}),
        }));
      },

      // Clear all pending edits
      clearPending: () => {
        set({
          pendingChat: {},
          pendingPrivacyChat: {},
          chatTestState: 'idle',
          chatTestError: null,
          privacyChatTestState: 'idle',
          privacyChatTestError: null,
        });
      },

      // Check if chat URL has pending changes
      hasChatUrlChange: () => {
        const { chat, pendingChat } = get();
        const pendingUrl = pendingChat.url;
        const savedUrl = chat?.url ?? '';
        return pendingUrl !== undefined && pendingUrl !== savedUrl;
      },

      // Check if privacy chat URL has pending changes
      hasPrivacyChatUrlChange: () => {
        const { privacyChat, pendingPrivacyChat } = get();
        const pendingUrl = pendingPrivacyChat.url;
        const savedUrl = privacyChat?.url ?? '';
        return pendingUrl !== undefined && pendingUrl !== savedUrl;
      },

      // Check if there are any pending service changes
      hasServicePendingChanges: () => {
        const { pendingChat, pendingPrivacyChat } = get();
        return (
          Object.keys(pendingChat).length > 0 ||
          Object.keys(pendingPrivacyChat).length > 0
        );
      },

      // Connect chat service: unregister old + register new + save
      connectChat: async () => {
        const { chat, pendingChat } = get();
        const url = pendingChat.url ?? chat?.url;
        if (!url) {
          set({ chatTestError: 'URL is required' });
          return false;
        }

        set({ chatTestState: 'testing', chatTestError: null });

        try {
          // Unregister old server if exists
          if (chat?.serverId) {
            try {
              await unregisterServer(chat.serverId);
            } catch {
              // Ignore unregister errors (server may have restarted)
            }
          }

          // Register new server
          const apiKey =
            pendingChat.apiKey !== undefined
              ? pendingChat.apiKey
              : (chat?.apiKey ?? '');
          const result = await registerServer({
            url,
            kind: 'chat',
            ...(apiKey ? { api_key: apiKey } : {}),
          });

          // Save to store (persisted to localStorage)
          set({
            chat: {
              url,
              apiKey,
              model: chat?.model ?? '',
              serverId: result.id,
            },
            chatRegistered: true,
            chatTestState: 'passed',
            chatTestError: null,
            pendingChat: {},
          });

          return true;
        } catch (err) {
          const errorMessage =
            err instanceof Error ? err.message : 'Failed to connect';
          set({ chatTestState: 'failed', chatTestError: errorMessage });
          return false;
        }
      },

      // Connect privacy chat service: unregister old + register new + save
      connectPrivacyChat: async () => {
        const { privacyChat, pendingPrivacyChat } = get();
        const url = pendingPrivacyChat.url ?? privacyChat?.url;
        if (!url) {
          set({ privacyChatTestError: 'URL is required' });
          return false;
        }

        set({ privacyChatTestState: 'testing', privacyChatTestError: null });

        try {
          // Unregister old server if exists
          if (privacyChat?.serverId) {
            try {
              await unregisterServer(privacyChat.serverId);
            } catch {
              // Ignore unregister errors
            }
          }

          // Register new server
          const apiKey =
            pendingPrivacyChat.apiKey !== undefined
              ? pendingPrivacyChat.apiKey
              : (privacyChat?.apiKey ?? '');
          const result = await registerServer({
            url,
            kind: 'privacy_chat',
            ...(apiKey ? { api_key: apiKey } : {}),
          });

          set({
            privacyChat: {
              url,
              apiKey,
              model: privacyChat?.model ?? '',
              serverId: result.id,
            },
            privacyChatRegistered: true,
            privacyChatTestState: 'passed',
            privacyChatTestError: null,
            pendingPrivacyChat: {},
          });

          return true;
        } catch (err) {
          const errorMessage =
            err instanceof Error ? err.message : 'Failed to connect';
          set({
            privacyChatTestState: 'failed',
            privacyChatTestError: errorMessage,
          });
          return false;
        }
      },

      // Set chat model (no re-registration needed)
      setChatModel: (model) => {
        set((state) => ({
          chat: state.chat ? { ...state.chat, model } : null,
        }));
      },

      // Set privacy chat model (no re-registration needed)
      setPrivacyChatModel: (model) => {
        set((state) => ({
          privacyChat: state.privacyChat
            ? { ...state.privacyChat, model }
            : null,
        }));
      },

      // Auto-register saved configs on app startup.
      // Does NOT unregister first — after backend restart the old IDs are
      // stale and unregister returns 500, which the browser always logs
      // regardless of try/catch.  Duplicate models from multiple
      // registrations are handled by ModelSelector's dedup logic.
      autoRegister: async () => {
        const { chat, privacyChat } = get();

        if (chat?.url) {
          try {
            const result = await registerServer({
              url: chat.url,
              kind: 'chat',
              ...(chat.apiKey ? { api_key: chat.apiKey } : {}),
            });

            set((state) => ({
              chat: state.chat
                ? { ...state.chat, serverId: result.id }
                : null,
              chatRegistered: true,
            }));
          } catch (err) {
            console.error('Failed to auto-register chat service:', err);
            set({ chatRegistered: false });
          }
        }

        if (privacyChat?.url) {
          try {
            const result = await registerServer({
              url: privacyChat.url,
              kind: 'privacy_chat',
              ...(privacyChat.apiKey ? { api_key: privacyChat.apiKey } : {}),
            });

            set((state) => ({
              privacyChat: state.privacyChat
                ? { ...state.privacyChat, serverId: result.id }
                : null,
              privacyChatRegistered: true,
            }));
          } catch (err) {
            console.error(
              'Failed to auto-register privacy chat service:',
              err
            );
            set({ privacyChatRegistered: false });
          }
        }
      },
    }),
    {
      name: 'aries-service-storage',
      partialize: (state) => ({
        // Only persist saved configs, not runtime state
        chat: state.chat,
        privacyChat: state.privacyChat,
      }),
    }
  )
);
