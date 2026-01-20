import { create } from 'zustand';
import { streamChatCompletionEnhanced, buildChatRequest } from '@/api/chat';
import type { ChatMessage, UIMessage, UIToolCall, ExecutionEvent, UITaskPlan, UISubtask } from '@/api/types';
import { useConfigStore } from './config';

// Execution status for task planning mode
export interface ExecutionStatus {
  phase: 'idle' | 'planning' | 'executing' | 'reflecting' | 'completing';
  message: string;
  subtaskId?: number;
  subtaskTotal?: number;
  subtaskCurrent?: number;
}

interface ChatState {
  // Messages
  messages: UIMessage[];

  // Streaming state
  isStreaming: boolean;
  abortController: AbortController | null;

  // Execution status (for task planning mode)
  executionStatus: ExecutionStatus;

  // Current conversation
  currentConversationId: string | null;

  // Error
  error: string | null;

  // Actions
  sendMessage: (content: string) => Promise<void>;
  stopGeneration: () => void;
  clearMessages: () => void;
  setConversationId: (id: string | null) => void;
  loadMessages: (messages: UIMessage[]) => void;
}

// Generate unique message ID
function generateMessageId(): string {
  return `msg_${Date.now()}_${Math.random().toString(36).substring(2, 9)}`;
}

// Convert UI messages to API format
function toApiMessages(messages: UIMessage[]): ChatMessage[] {
  return messages.map((msg) => ({
    role: msg.role,
    content: msg.content,
  }));
}

// Initial execution status
const initialExecutionStatus: ExecutionStatus = {
  phase: 'idle',
  message: '',
};

export const useChatStore = create<ChatState>((set, get) => ({
  // Initial state
  messages: [],
  isStreaming: false,
  abortController: null,
  executionStatus: initialExecutionStatus,
  currentConversationId: null,
  error: null,

  // Send a message
  sendMessage: async (content: string) => {
    const { messages, isStreaming } = get();

    if (isStreaming || !content.trim()) {
      return;
    }

    // Create user message
    const userMessage: UIMessage = {
      id: generateMessageId(),
      role: 'user',
      content: content.trim(),
      timestamp: new Date(),
    };

    // Create placeholder assistant message
    const assistantMessage: UIMessage = {
      id: generateMessageId(),
      role: 'assistant',
      content: '',
      timestamp: new Date(),
      isStreaming: true,
      toolCalls: [],
    };

    // Create abort controller
    const abortController = new AbortController();

    set({
      messages: [...messages, userMessage, assistantMessage],
      isStreaming: true,
      abortController,
      executionStatus: initialExecutionStatus,
      error: null,
    });

    try {
      // Build API request with model from config
      const apiMessages = toApiMessages([...messages, userMessage]);
      const model = useConfigStore.getState().config?.chat?.model;
      const request = buildChatRequest(apiMessages, { stream: true, model });

      // Track state during streaming
      let fullContent = '';
      let thinking = '';
      const toolCalls: UIToolCall[] = [];
      const executionEvents: ExecutionEvent[] = [];
      let eventSeq = 0;
      let taskPlan: UITaskPlan | undefined = undefined;

      // Use enhanced streaming to get detailed events
      for await (const event of streamChatCompletionEnhanced(
        request,
        abortController.signal
      )) {
        console.log('[Chat Store] Processing event:', event.type, event.data);
        switch (event.type) {
          case 'status':
            // Update execution status
            console.log('[Chat Store] Setting execution status:', event.data);
            // Update subtask status in task plan if we have subtask info
            if (taskPlan && event.data.subtask_id !== undefined) {
              const subtaskIndex = taskPlan.subtasks.findIndex(
                (s) => s.id === event.data.subtask_id
              );
              if (subtaskIndex >= 0) {
                // Mark current subtask as in_progress, mark completed subtasks
                taskPlan.subtasks = taskPlan.subtasks.map((s, idx) => {
                  if (s.id === event.data.subtask_id) {
                    return { ...s, status: 'in_progress' as const };
                  } else if (idx < subtaskIndex && s.status === 'in_progress') {
                    return { ...s, status: 'completed' as const };
                  }
                  return s;
                });
              }
            }
            set((state) => ({
              executionStatus: {
                phase: event.data.phase,
                message: event.data.message,
                subtaskId: event.data.subtask_id,
                subtaskTotal: event.data.subtask_total,
                subtaskCurrent: event.data.subtask_current,
              },
              // Also update task plan in message
              messages: taskPlan
                ? state.messages.map((msg) =>
                    msg.id === assistantMessage.id
                      ? { ...msg, taskPlan: { ...taskPlan! } }
                      : msg
                  )
                : state.messages,
            }));
            break;

          case 'plan':
            // Store task plan with subtask list
            console.log('[Chat Store] Received plan event:', event.data.goal);
            taskPlan = {
              goal: event.data.goal,
              subtasks: event.data.subtasks.map((s) => ({
                id: s.id,
                description: s.description,
                status: s.status as UISubtask['status'],
              })),
            };
            // Update message with task plan
            set((state) => ({
              messages: state.messages.map((msg) =>
                msg.id === assistantMessage.id
                  ? { ...msg, taskPlan }
                  : msg
              ),
            }));
            break;

          case 'thought':
            // Accumulate thought content
            if (event.data.content) {
              thinking += (thinking ? '\n' : '') + event.data.content;
              console.log('[Chat Store] Updated thinking:', thinking.slice(0, 100));
              // Add thought event to execution timeline
              executionEvents.push({
                id: `thought_${eventSeq++}`,
                type: 'thought',
                timestamp: Date.now(),
                content: event.data.content,
              });
              // Update message with thinking and execution events
              set((state) => ({
                messages: state.messages.map((msg) =>
                  msg.id === assistantMessage.id
                    ? { ...msg, thinking, executionEvents: [...executionEvents] }
                    : msg
                ),
              }));
            }
            break;

          case 'tool_call':
            // Add new tool call
            console.log('[Chat Store] Adding tool call:', event.data.tool_name);
            const newToolCall: UIToolCall = {
              id: event.data.tool_call_id,
              name: event.data.tool_name,
              arguments: event.data.args as Record<string, unknown>,
              status: 'running',
            };
            toolCalls.push(newToolCall);
            // Add tool_call event to execution timeline
            executionEvents.push({
              id: `toolcall_${eventSeq++}`,
              type: 'tool_call',
              timestamp: Date.now(),
              toolCall: newToolCall,
            });
            // Update message with tool calls and execution events
            set((state) => ({
              messages: state.messages.map((msg) =>
                msg.id === assistantMessage.id
                  ? { ...msg, toolCalls: [...toolCalls], executionEvents: [...executionEvents] }
                  : msg
              ),
            }));
            break;

          case 'tool_result':
            // Update tool call with result
            const tcIndex = toolCalls.findIndex(
              (tc) => tc.id === event.data.tool_call_id
            );
            console.log('[Chat Store] Tool result for:', event.data.tool_call_id, 'index:', tcIndex);
            if (tcIndex >= 0) {
              const updatedToolCall = {
                ...toolCalls[tcIndex],
                status: (event.data.is_error ? 'error' : 'success') as UIToolCall['status'],
                result: event.data.result,
                error: event.data.is_error ? event.data.result : undefined,
                durationMs: event.data.duration_ms,
              };
              toolCalls[tcIndex] = updatedToolCall;
              // Also update the tool call in execution events
              const eventIndex = executionEvents.findIndex(
                (e) => e.type === 'tool_call' && e.toolCall?.id === event.data.tool_call_id
              );
              if (eventIndex >= 0) {
                executionEvents[eventIndex] = {
                  ...executionEvents[eventIndex],
                  toolCall: updatedToolCall,
                };
              }
              // Update message
              set((state) => ({
                messages: state.messages.map((msg) =>
                  msg.id === assistantMessage.id
                    ? { ...msg, toolCalls: [...toolCalls], executionEvents: [...executionEvents] }
                    : msg
                ),
              }));
            }
            break;

          case 'text':
            // Accumulate final text content
            fullContent += event.data.content;
            console.log('[Chat Store] Text content length:', fullContent.length);
            // Update message
            set((state) => ({
              messages: state.messages.map((msg) =>
                msg.id === assistantMessage.id
                  ? { ...msg, content: fullContent }
                  : msg
              ),
            }));
            break;

          case 'finish':
            // Streaming complete
            console.log('[Chat Store] Finish event received');
            break;
        }
      }

      // Mark streaming as complete - update tool call statuses in both arrays
      const finalToolCalls = toolCalls.length > 0
        ? toolCalls.map((tc) => ({
            ...tc,
            status: (tc.status === 'running' ? 'success' : tc.status) as UIToolCall['status'],
          }))
        : undefined;

      const finalExecutionEvents = executionEvents.length > 0
        ? executionEvents.map((e) =>
            e.type === 'tool_call' && e.toolCall?.status === 'running'
              ? { ...e, toolCall: { ...e.toolCall, status: 'success' as UIToolCall['status'] } }
              : e
          )
        : undefined;

      // Mark all subtasks as completed when streaming finishes
      const finalTaskPlan = taskPlan
        ? {
            ...taskPlan,
            subtasks: taskPlan.subtasks.map((s) =>
              s.status === 'pending' || s.status === 'in_progress'
                ? { ...s, status: 'completed' as const }
                : s
            ),
          }
        : undefined;

      set((state) => ({
        messages: state.messages.map((msg) =>
          msg.id === assistantMessage.id
            ? {
                ...msg,
                isStreaming: false,
                content: fullContent || msg.content,
                thinking: thinking || msg.thinking,
                toolCalls: finalToolCalls || msg.toolCalls,
                executionEvents: finalExecutionEvents || msg.executionEvents,
                taskPlan: finalTaskPlan || msg.taskPlan,
              }
            : msg
        ),
        isStreaming: false,
        abortController: null,
        executionStatus: initialExecutionStatus,
      }));
    } catch (err) {
      if (err instanceof Error && err.name === 'AbortError') {
        // User cancelled
        set((state) => ({
          messages: state.messages.map((msg) =>
            msg.id === assistantMessage.id
              ? { ...msg, isStreaming: false, content: msg.content || '(cancelled)' }
              : msg
          ),
          isStreaming: false,
          abortController: null,
          executionStatus: initialExecutionStatus,
        }));
      } else {
        // Error
        const errorMessage = err instanceof Error ? err.message : 'Failed to send message';
        set((state) => ({
          messages: state.messages.map((msg) =>
            msg.id === assistantMessage.id
              ? { ...msg, isStreaming: false, error: errorMessage }
              : msg
          ),
          isStreaming: false,
          abortController: null,
          executionStatus: initialExecutionStatus,
          error: errorMessage,
        }));
      }
    }
  },

  // Stop generation
  stopGeneration: () => {
    const { abortController } = get();
    if (abortController) {
      abortController.abort();
    }
  },

  // Clear all messages
  clearMessages: () => {
    set({
      messages: [],
      currentConversationId: null,
      executionStatus: initialExecutionStatus,
      error: null,
    });
  },

  // Set current conversation ID
  setConversationId: (id) => {
    set({ currentConversationId: id });
  },

  // Load messages (e.g., from history)
  loadMessages: (messages) => {
    set({ messages, error: null });
  },
}));
