import { apiClient, getCurrentUserId } from './client';
import type {
  ChatCompletionRequest,
  ChatCompletionResponse,
  ChatStreamChunk,
  ChatMessage,
  EnhancedStreamEvent,
  StatusEvent,
  PlanEvent,
  ThoughtEvent,
  ToolCallEvent,
  ToolResultEvent,
  TextEvent,
  FinishEvent,
  SubAgentSpawnedEvent,
  SubAgentStartedEvent,
  SubAgentProgressEvent,
  SubAgentToolCallEvent,
  SubAgentCompletedEvent,
  SubAgentFailedEvent,
} from './types';

/**
 * Send a chat completion request (non-streaming)
 */
export async function sendChatCompletion(
  request: ChatCompletionRequest
): Promise<ChatCompletionResponse> {
  return apiClient
    .post('v1/chat/completions', {
      json: { ...request, stream: false },
    })
    .json<ChatCompletionResponse>();
}

/**
 * Send a chat completion request with streaming
 * Returns an async generator that yields parsed SSE chunks
 * Also handles non-streaming responses (e.g., direct-answer mode)
 */
export async function* streamChatCompletion(
  request: ChatCompletionRequest,
  signal?: AbortSignal
): AsyncGenerator<ChatStreamChunk, void, unknown> {
  const response = await fetch('/v1/chat/completions', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-User-ID': getCurrentUserId(),
      'X-Request-ID': `req_${Date.now()}_${Math.random().toString(36).substring(2, 9)}`,
    },
    body: JSON.stringify({ ...request, stream: true }),
    signal,
  });

  if (!response.ok) {
    const error = await response.json().catch(() => ({}));
    throw new Error((error as { message?: string }).message || response.statusText);
  }

  const contentType = response.headers.get('content-type') || '';

  // Check if response is SSE (streaming) or JSON (non-streaming fallback)
  if (contentType.includes('text/event-stream')) {
    // Handle SSE streaming response
    const reader = response.body?.getReader();
    if (!reader) {
      throw new Error('Response body is not readable');
    }

    const decoder = new TextDecoder();
    let buffer = '';

    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split('\n');
        buffer = lines.pop() || '';

        for (const line of lines) {
          const trimmed = line.trim();
          if (!trimmed || trimmed === 'data: [DONE]') continue;

          if (trimmed.startsWith('data: ')) {
            try {
              const chunk = JSON.parse(trimmed.slice(6)) as ChatStreamChunk;
              yield chunk;
            } catch {
              // Skip invalid JSON
            }
          }
        }
      }
    } finally {
      reader.releaseLock();
    }
  } else {
    // Handle JSON response (non-streaming, e.g., direct-answer mode)
    const data = (await response.json()) as ChatCompletionResponse;
    const message = data.choices[0]?.message;

    // Extract content as string
    let content: string | undefined;
    if (typeof message?.content === 'string') {
      content = message.content;
    } else if (Array.isArray(message?.content)) {
      // Handle ContentPart[] - extract text parts
      content = message.content
        .filter((part): part is { type: 'text'; text: string } => part.type === 'text')
        .map((part) => part.text)
        .join('');
    }

    // Convert tool_calls to delta format
    const toolCallDeltas = message?.tool_calls?.map((tc, index) => ({
      index,
      id: tc.id,
      type: tc.type,
      function: tc.function,
    }));

    // Convert to stream chunk format
    const chunk: ChatStreamChunk = {
      id: data.id,
      object: 'chat.completion.chunk',
      created: data.created,
      model: data.model,
      choices: [
        {
          index: 0,
          delta: {
            role: message?.role,
            content,
            tool_calls: toolCallDeltas,
          },
          finish_reason: data.choices[0]?.finish_reason,
        },
      ],
    };
    yield chunk;
  }
}

/**
 * Send a chat completion request with enhanced streaming
 * Returns an async generator that yields enhanced stream events
 * Supports: status, thought, tool_call, tool_result, text, finish events
 */
export async function* streamChatCompletionEnhanced(
  request: ChatCompletionRequest,
  signal?: AbortSignal
): AsyncGenerator<EnhancedStreamEvent, void, unknown> {
  const response = await fetch('/v1/chat/completions', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-User-ID': getCurrentUserId(),
      'X-Request-ID': `req_${Date.now()}_${Math.random().toString(36).substring(2, 9)}`,
      'X-Enhanced-Stream': 'all', // Enable all enhanced stream events
    },
    body: JSON.stringify({ ...request, stream: true }),
    signal,
  });

  if (!response.ok) {
    const error = await response.json().catch(() => ({}));
    throw new Error((error as { message?: string }).message || response.statusText);
  }

  const reader = response.body?.getReader();
  if (!reader) {
    throw new Error('Response body is not readable');
  }

  const decoder = new TextDecoder();
  let buffer = '';
  let currentEventType: string | null = null;

  // Check if enhanced stream is enabled in response
  const isEnhanced = response.headers.get('X-Enhanced-Stream') === 'true';
  console.log('[Enhanced Stream] Response header X-Enhanced-Stream:', isEnhanced);

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split('\n');
      buffer = lines.pop() || '';

      for (const line of lines) {
        const trimmed = line.trim();
        if (!trimmed) continue;
        if (trimmed === 'data: [DONE]') return;

        // Parse event type
        if (trimmed.startsWith('event: ')) {
          currentEventType = trimmed.slice(7);
          console.log('[Enhanced Stream] Event type:', currentEventType);
          continue;
        }

        // Parse event data
        if (trimmed.startsWith('data: ')) {
          if (currentEventType) {
            try {
              const data = JSON.parse(trimmed.slice(6));
              const event = parseEnhancedEvent(currentEventType, data);
              if (event) {
                console.log('[Enhanced Stream] Parsed event:', event.type);
                yield event;
              }
            } catch {
              // Skip invalid JSON
            }
            currentEventType = null;
          } else {
            // Fallback: try to parse as OpenAI format chunk and convert to text event
            console.log('[Enhanced Stream] Data without event type:', trimmed.slice(0, 100));
          }
        }
      }
    }
  } finally {
    reader.releaseLock();
  }
}

/**
 * Parse enhanced stream event data
 */
function parseEnhancedEvent(
  eventType: string,
  data: unknown
): EnhancedStreamEvent | null {
  switch (eventType) {
    case 'status':
      return { type: 'status', data: data as StatusEvent };
    case 'plan':
      return { type: 'plan', data: data as PlanEvent };
    case 'thought':
      return { type: 'thought', data: data as ThoughtEvent };
    case 'tool_call':
      return { type: 'tool_call', data: data as ToolCallEvent };
    case 'tool_result':
      return { type: 'tool_result', data: data as ToolResultEvent };
    case 'text':
      return { type: 'text', data: data as TextEvent };
    case 'finish':
      return { type: 'finish', data: data as FinishEvent };
    // Sub-Agent events
    case 'subagent_spawned':
      return { type: 'subagent_spawned', data: data as SubAgentSpawnedEvent };
    case 'subagent_started':
      return { type: 'subagent_started', data: data as SubAgentStartedEvent };
    case 'subagent_progress':
      return { type: 'subagent_progress', data: data as SubAgentProgressEvent };
    case 'subagent_tool_call':
      return { type: 'subagent_tool_call', data: data as SubAgentToolCallEvent };
    case 'subagent_completed':
      return { type: 'subagent_completed', data: data as SubAgentCompletedEvent };
    case 'subagent_failed':
      return { type: 'subagent_failed', data: data as SubAgentFailedEvent };
    default:
      console.log('[Enhanced Stream] Unknown event type:', eventType);
      return null;
  }
}

/**
 * Helper to build a chat request with conversation context
 */
export function buildChatRequest(
  messages: ChatMessage[],
  options?: Partial<ChatCompletionRequest>
): ChatCompletionRequest {
  return {
    model: options?.model,
    messages,
    stream: options?.stream ?? true,
    temperature: options?.temperature,
    max_tokens: options?.max_tokens,
    tools: options?.tools,
    tool_choice: options?.tool_choice,
  };
}
