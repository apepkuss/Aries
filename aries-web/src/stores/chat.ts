import { create } from 'zustand';
import { streamChatCompletionEnhanced, buildChatRequest } from '@/api/chat';
import type {
  ChatMessage,
  UIMessage,
  UIToolCall,
  ExecutionEvent,
  UITaskPlan,
  UISubtask,
  UISubAgent,
} from '@/api/types';
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

  // Sub-Agents state (keyed by subagent_id)
  subAgents: Map<string, UISubAgent>;

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

  // Sub-Agent actions
  getSubAgent: (id: string) => UISubAgent | undefined;
  getActiveSubAgents: () => UISubAgent[];
  getRootSubAgents: () => UISubAgent[];
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

// Helper function to extract subtask ID from Sub-Agent name
// Format: "subtask-1", "Subtask-1", etc.
function extractSubtaskIdFromName(name: string): number | null {
  const match = name.match(/^[Ss]ubtask-(\d+)$/);
  return match ? parseInt(match[1], 10) : null;
}

// Helper function to update a subtask in a task plan
function updateSubtaskInPlan(
  taskPlan: UITaskPlan | undefined,
  subtaskId: number,
  update: Partial<UISubtask>
): UITaskPlan | undefined {
  if (!taskPlan) return undefined;
  return {
    ...taskPlan,
    subtasks: taskPlan.subtasks.map((s) =>
      s.id === subtaskId ? { ...s, ...update } : s
    ),
  };
}

export const useChatStore = create<ChatState>((set, get) => ({
  // Initial state
  messages: [],
  isStreaming: false,
  abortController: null,
  executionStatus: initialExecutionStatus,
  subAgents: new Map(),
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

          case 'tool_call': {
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
          }

          case 'tool_result': {
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
          }

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

          // Sub-Agent events
          case 'subagent_spawned':
            {
              const { subagent_id, name, task, depth, parent_id } = event.data;
              console.log('[Chat Store] Sub-Agent spawned:', name, subagent_id);
              const newSubAgent: UISubAgent = {
                id: subagent_id,
                name,
                task,
                state: 'pending',
                depth,
                parentId: parent_id,
                childIds: [],
                createdAt: Date.now(),
              };

              // Check if this Sub-Agent is associated with a Subtask
              const subtaskId = extractSubtaskIdFromName(name);

              set((state) => {
                const newSubAgents = new Map(state.subAgents);
                newSubAgents.set(subagent_id, newSubAgent);
                // Update parent's childIds if exists
                if (parent_id && newSubAgents.has(parent_id)) {
                  const parent = newSubAgents.get(parent_id)!;
                  newSubAgents.set(parent_id, {
                    ...parent,
                    childIds: [...parent.childIds, subagent_id],
                  });
                }

                // If associated with a Subtask, update the task plan
                let updatedMessages = state.messages;
                if (subtaskId !== null && taskPlan) {
                  const updatedTaskPlan = updateSubtaskInPlan(taskPlan, subtaskId, {
                    subAgentId: subagent_id,
                  });
                  if (updatedTaskPlan) {
                    taskPlan = updatedTaskPlan;
                    updatedMessages = state.messages.map((msg) =>
                      msg.id === assistantMessage.id
                        ? { ...msg, taskPlan: updatedTaskPlan }
                        : msg
                    );
                  }
                }

                return { subAgents: newSubAgents, messages: updatedMessages };
              });
            }
            break;

          case 'subagent_started':
            {
              const { subagent_id, timestamp } = event.data;
              console.log('[Chat Store] Sub-Agent started:', subagent_id);
              set((state) => {
                const newSubAgents = new Map(state.subAgents);
                const agent = newSubAgents.get(subagent_id);
                if (agent) {
                  newSubAgents.set(subagent_id, {
                    ...agent,
                    state: 'running',
                    startedAt: timestamp,
                  });
                }
                return { subAgents: newSubAgents };
              });
            }
            break;

          case 'subagent_progress':
            {
              const { subagent_id, iteration, message, tool_name } = event.data;
              console.log('[Chat Store] Sub-Agent progress:', subagent_id, iteration);
              set((state) => {
                const newSubAgents = new Map(state.subAgents);
                const agent = newSubAgents.get(subagent_id);

                // Find associated subtask ID from the agent name
                const subtaskId = agent ? extractSubtaskIdFromName(agent.name) : null;

                if (agent) {
                  newSubAgents.set(subagent_id, {
                    ...agent,
                    progress: {
                      iteration,
                      message,
                      lastToolName: tool_name,
                    },
                  });
                }

                // Update task plan if associated with a subtask
                let updatedMessages = state.messages;
                if (subtaskId !== null && taskPlan) {
                  const updatedTaskPlan = updateSubtaskInPlan(taskPlan, subtaskId, {
                    progress: {
                      iteration,
                      maxIterations: 20, // TODO: get from config
                      lastToolName: tool_name,
                      message,
                    },
                  });
                  if (updatedTaskPlan) {
                    taskPlan = updatedTaskPlan;
                    updatedMessages = state.messages.map((msg) =>
                      msg.id === assistantMessage.id
                        ? { ...msg, taskPlan: updatedTaskPlan }
                        : msg
                    );
                  }
                }

                return { subAgents: newSubAgents, messages: updatedMessages };
              });
            }
            break;

          case 'subagent_completed':
            {
              const { subagent_id, output, metrics } = event.data;
              console.log('[Chat Store] Sub-Agent completed:', subagent_id);
              set((state) => {
                const newSubAgents = new Map(state.subAgents);
                const agent = newSubAgents.get(subagent_id);

                // Find associated subtask ID from the agent name
                const subtaskId = agent ? extractSubtaskIdFromName(agent.name) : null;

                if (agent) {
                  newSubAgents.set(subagent_id, {
                    ...agent,
                    state: 'completed',
                    completedAt: Date.now(),
                    result: { output, metrics },
                  });
                }

                // Update task plan if associated with a subtask
                let updatedMessages = state.messages;
                if (subtaskId !== null && taskPlan) {
                  const updatedTaskPlan = updateSubtaskInPlan(taskPlan, subtaskId, {
                    status: 'completed',
                    result: { output, metrics },
                    progress: undefined, // Clear progress when completed
                  });
                  if (updatedTaskPlan) {
                    taskPlan = updatedTaskPlan;
                    updatedMessages = state.messages.map((msg) =>
                      msg.id === assistantMessage.id
                        ? { ...msg, taskPlan: updatedTaskPlan }
                        : msg
                    );
                  }
                }

                return { subAgents: newSubAgents, messages: updatedMessages };
              });
            }
            break;

          case 'subagent_failed':
            {
              const { subagent_id, error: errorMsg, metrics } = event.data;
              console.log('[Chat Store] Sub-Agent failed:', subagent_id, errorMsg);
              set((state) => {
                const newSubAgents = new Map(state.subAgents);
                const agent = newSubAgents.get(subagent_id);

                // Find associated subtask ID from the agent name
                const subtaskId = agent ? extractSubtaskIdFromName(agent.name) : null;

                if (agent) {
                  newSubAgents.set(subagent_id, {
                    ...agent,
                    state: 'failed',
                    completedAt: Date.now(),
                    error: errorMsg,
                    result: metrics ? { output: '', metrics } : undefined,
                  });
                }

                // Update task plan if associated with a subtask
                let updatedMessages = state.messages;
                if (subtaskId !== null && taskPlan) {
                  const updatedTaskPlan = updateSubtaskInPlan(taskPlan, subtaskId, {
                    status: 'failed',
                    error: errorMsg,
                    result: metrics ? { output: '', metrics } : undefined,
                    progress: undefined, // Clear progress when failed
                  });
                  if (updatedTaskPlan) {
                    taskPlan = updatedTaskPlan;
                    updatedMessages = state.messages.map((msg) =>
                      msg.id === assistantMessage.id
                        ? { ...msg, taskPlan: updatedTaskPlan }
                        : msg
                    );
                  }
                }

                return { subAgents: newSubAgents, messages: updatedMessages };
              });
            }
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
      subAgents: new Map(),
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

  // Get a specific Sub-Agent by ID
  getSubAgent: (id) => {
    return get().subAgents.get(id);
  },

  // Get all active (running) Sub-Agents
  getActiveSubAgents: () => {
    const { subAgents } = get();
    return Array.from(subAgents.values()).filter(
      (agent) => agent.state === 'running' || agent.state === 'pending'
    );
  },

  // Get root-level Sub-Agents (no parent)
  getRootSubAgents: () => {
    const { subAgents } = get();
    return Array.from(subAgents.values()).filter((agent) => !agent.parentId);
  },
}));
