import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import type {
  ExecutionState,
  ExecutionStep,
  StreamEvent,
  StatusEvent,
  TokenUsage,
} from './types/execution';
import { createExecutionStep, getEventType } from './types/execution';

interface Message {
  role: 'user' | 'assistant';
  content: string;
  id: string;
  /** If true, this message is being streamed with execution transparency */
  isStreaming?: boolean;
  /** Execution steps for this message (if using Plan mode) */
  executionSteps?: ExecutionStep[];
}

interface ChatConfig {
  url: string;
  api_key: string;
  model: string;
}

// Initial execution state
const initialExecutionState: ExecutionState = {
  requestId: null,
  isExecuting: false,
  phase: null,
  steps: [],
  statusMessage: null,
  subtaskProgress: null,
  result: null,
  error: null,
};

interface AriesState {
  // Existing state
  messages: Message[];
  isConfigured: boolean;
  isLoading: boolean;
  isConfigLoading: boolean;
  serverInfo: unknown | null;
  chatConfig: ChatConfig | null;

  // Execution transparency state
  execution: ExecutionState;

  // Existing actions
  addMessage: (content: string, role: 'user' | 'assistant') => void;
  fetchServerInfo: () => Promise<void>;
  sendMessage: (content: string) => Promise<void>;
  fetchChatConfig: () => Promise<void>;
  updateChatConfig: (config: ChatConfig) => Promise<void>;

  // Execution transparency actions
  startExecution: (requestId: string) => void;
  addExecutionStep: (event: StreamEvent) => void;
  appendStreamingText: (text: string) => void;
  completeExecution: (result: { content: string; usage: TokenUsage }) => void;
  failExecution: (error: string) => void;
  resetExecution: () => void;

  // Send message with Plan mode (execution transparency)
  sendMessageWithPlan: (content: string) => Promise<string>;
}

export const useAriesStore = create<AriesState>((set, get) => ({
  messages: [],
  isConfigured: false,
  isLoading: false,
  isConfigLoading: false,
  serverInfo: null,
  chatConfig: null,
  execution: initialExecutionState,

  addMessage: (content, role) => {
    const newMessage: Message = {
      content,
      role,
      id: Math.random().toString(36).substring(7),
    };
    set((state) => ({ messages: [...state.messages, newMessage] }));
  },

  fetchServerInfo: async () => {
    try {
      const info = await invoke('get_server_info');
      set({ serverInfo: info, isConfigured: true });
    } catch (error) {
      console.error('Failed to fetch server info:', error);
      set({ isConfigured: false });
    }
  },

  sendMessage: async (content) => {
    const { chatConfig } = get();

    // Check if model is configured
    if (!chatConfig?.model) {
      get().addMessage(content, 'user');
      get().addMessage('Error: Model is not configured. Please configure a model in Settings.', 'assistant');
      return;
    }

    set({ isLoading: true });
    get().addMessage(content, 'user');

    try {
      const response = await invoke<{ content: string }>('chat', {
        request: { message: content, model: chatConfig.model }
      });
      get().addMessage(response.content, 'assistant');
    } catch (error) {
      console.error('Failed to send message:', error);
      // Show error message to user
      const errorMessage = error instanceof Error ? error.message : String(error);
      get().addMessage(`Error: ${errorMessage}`, 'assistant');
    } finally {
      set({ isLoading: false });
    }
  },

  fetchChatConfig: async () => {
    set({ isConfigLoading: true });
    try {
      const config = await invoke<ChatConfig>('get_chat_config');
      set({ chatConfig: config });
    } catch (error) {
      console.error('Failed to fetch chat config:', error);
    } finally {
      set({ isConfigLoading: false });
    }
  },

  updateChatConfig: async (config: ChatConfig) => {
    try {
      await invoke('update_chat_config', { chatConfig: config });
      set({ chatConfig: config });
    } catch (error) {
      console.error('Failed to update chat config:', error);
      throw error;
    }
  },

  // ============================================================================
  // Execution Transparency Actions
  // ============================================================================

  startExecution: (requestId: string) => {
    set({
      execution: {
        ...initialExecutionState,
        requestId,
        isExecuting: true,
      },
      isLoading: true,
    });
  },

  addExecutionStep: (event: StreamEvent) => {
    const step = createExecutionStep(event);
    const eventType = getEventType(event);

    set((state) => {
      const newSteps = [...state.execution.steps, step];

      // Update execution state based on event type
      let updates: Partial<ExecutionState> = { steps: newSteps };

      if (eventType === 'status') {
         // status is now part of the event (internally tagged), but createExecutionStep already extracted it
        const statusEvent = step.data as StatusEvent;
        updates = {
          ...updates,
          phase: statusEvent.phase,
          statusMessage: statusEvent.message,
        };

        // Update subtask progress if available
        if (statusEvent.subtask_current !== undefined && statusEvent.subtask_total !== undefined) {
          updates.subtaskProgress = {
            current: statusEvent.subtask_current,
            total: statusEvent.subtask_total,
          };
        }
      }

      return {
        execution: {
          ...state.execution,
          ...updates,
        },
      };
    });
  },

  appendStreamingText: (text: string) => {
    set((state) => {
      const lastMessage = state.messages[state.messages.length - 1];
      if (lastMessage && lastMessage.role === 'assistant') {
        const updatedMessages = [...state.messages];
        updatedMessages[updatedMessages.length - 1] = {
          ...lastMessage,
          content: lastMessage.content + text,
          isStreaming: true,
        };
        return { messages: updatedMessages };
      } else {
        const newMessage: Message = {
          content: text,
          role: 'assistant',
          id: Math.random().toString(36).substring(7),
          isStreaming: true,
        };
        return { messages: [...state.messages, newMessage] };
      }
    });
  },

  completeExecution: (result) => {
    set((state) => {
      const updatedMessages = [...state.messages];
      const lastMessageIndex = updatedMessages.length - 1;
      const lastMessage = updatedMessages[lastMessageIndex];

      if (lastMessage && lastMessage.role === 'assistant') {
        // Update existing assistant message with execution steps
        updatedMessages[lastMessageIndex] = {
          ...lastMessage,
          content: result.content || lastMessage.content,
          isStreaming: false,
          executionSteps: state.execution.steps,
        };
      } else {
        // Create new assistant message
        const assistantMessage: Message = {
          content: result.content,
          role: 'assistant',
          id: Math.random().toString(36).substring(7),
          executionSteps: state.execution.steps,
          isStreaming: false,
        };
        updatedMessages.push(assistantMessage);
      }

      return {
        messages: updatedMessages,
        execution: {
          ...state.execution,
          isExecuting: false,
          result: {
            content: result.content,
            usage: result.usage,
          },
        },
        isLoading: false,
      };
    });
  },

  failExecution: (error: string) => {
    set((state) => {
      // Create error message
      const errorMessage: Message = {
        content: `Error: ${error}`,
        role: 'assistant',
        id: Math.random().toString(36).substring(7),
        executionSteps: state.execution.steps,
      };

      return {
        messages: [...state.messages, errorMessage],
        execution: {
          ...state.execution,
          isExecuting: false,
          error,
        },
        isLoading: false,
      };
    });
  },

  resetExecution: () => {
    set({ execution: initialExecutionState });
  },

  sendMessageWithPlan: async (content: string) => {
    const { chatConfig } = get();

    // Check if model is configured
    if (!chatConfig?.model) {
      get().addMessage(content, 'user');
      get().addMessage('Error: Model is not configured. Please configure a model in Settings.', 'assistant');
      return '';
    }

    // Add user message
    get().addMessage(content, 'user');

    try {
      // Call chat_stream command
      const response = await invoke<{ request_id: string }>('chat_stream', {
        request: {
          message: content,
          model: chatConfig.model,
          conversation_id: null,
        }
      });

      // Start execution tracking
      get().startExecution(response.request_id);

      return response.request_id;
    } catch (error) {
      console.error('Failed to start chat stream:', error);
      const errorMessage = error instanceof Error ? error.message : String(error);
      get().addMessage(`Error: ${errorMessage}`, 'assistant');
      return '';
    }
  },
}));
