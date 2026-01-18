/**
 * Execution transparency types for Plan mode streaming.
 *
 * These types mirror the Rust types in aries::chat::events
 */

// ============================================================================
// Event Types
// ============================================================================

export type StreamEventType =
  | 'thought'
  | 'tool_call'
  | 'tool_result'
  | 'text'
  | 'status'
  | 'finish'
  | 'artifact_created'
  | 'artifact_updated'
  | 'artifact_deleted';

export type ThoughtStatus = 'start' | 'streaming' | 'done';

export type ExecutionPhase = 'planning' | 'executing' | 'reflecting' | 'completing';

// ============================================================================
// Event Payloads
// ============================================================================

export interface ThoughtEvent {
  content: string;
  status: ThoughtStatus;
  subtask_id?: number;
  iteration?: number;
}

export interface ToolCallEvent {
  tool_call_id: string;
  tool_name: string;
  args: unknown;
  server_name?: string;
  subtask_id?: number;
}

export interface ToolResultEvent {
  tool_call_id: string;
  result: string;
  is_error: boolean;
  duration_ms?: number;
  subtask_id?: number;
}

export interface StatusEvent {
  phase: ExecutionPhase;
  message: string;
  subtask_id?: number;
  subtask_total?: number;
  subtask_current?: number;
}

export interface TextEvent {
  content: string;
}

export interface TokenUsage {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
}

export interface ExecutionSummary {
  subtask_count: number;
  completed_count: number;
  failed_count: number;
  tool_call_count: number;
  duration_ms: number;
}

export interface FinishEvent {
  stop_reason: string;
  usage: TokenUsage;
  error?: string;
  summary?: ExecutionSummary;
}

export interface ArtifactCreatedEvent {
  artifact_id: string;
  title: string;
  artifact_type: unknown;
  content: string;
  size: number;
  url: string;
  subtask_id?: number;
}

export interface ArtifactUpdatedEvent {
  artifact_id: string;
  version: number;
  content: string;
  change_description?: string;
  subtask_id?: number;
}

export interface ArtifactDeletedEvent {
  artifact_id: string;
  subtask_id?: number;
}

// ============================================================================
// Stream Event Union (matches Rust StreamEvent enum with #[serde(tag = "type")])
// ============================================================================

export type StreamEvent =
  | ({ type: 'thought' } & ThoughtEvent)
  | ({ type: 'tool_call' } & ToolCallEvent)
  | ({ type: 'tool_result' } & ToolResultEvent)
  | ({ type: 'text' } & TextEvent)
  | ({ type: 'status' } & StatusEvent)
  | ({ type: 'finish' } & FinishEvent)
  | ({ type: 'artifact_created' } & ArtifactCreatedEvent)
  | ({ type: 'artifact_updated' } & ArtifactUpdatedEvent)
  | ({ type: 'artifact_deleted' } & ArtifactDeletedEvent);

// ============================================================================
// UI State Types
// ============================================================================

/**
 * Represents a single execution step for UI display
 */
export interface ExecutionStep {
  id: string;
  type: StreamEventType;
  timestamp: number;
  subtaskId?: number;
  iteration?: number;
  data:
    | ThoughtEvent
    | ToolCallEvent
    | ToolResultEvent
    | StatusEvent
    | TextEvent
    | FinishEvent
    | ArtifactCreatedEvent
    | ArtifactUpdatedEvent
    | ArtifactDeletedEvent;
}

/**
 * Current execution state
 */
export interface ExecutionState {
  /** Current request ID for event subscription */
  requestId: string | null;
  /** Whether execution is in progress */
  isExecuting: boolean;
  /** Current execution phase */
  phase: ExecutionPhase | null;
  /** All execution steps received */
  steps: ExecutionStep[];
  /** Current status message */
  statusMessage: string | null;
  /** Subtask progress */
  subtaskProgress: {
    current: number;
    total: number;
  } | null;
  /** Final result (if execution completed) */
  result: {
    content: string;
    usage: TokenUsage;
    summary?: ExecutionSummary;
  } | null;
  /** Error message (if execution failed) */
  error: string | null;
}

// ============================================================================
// Helper Functions
// ============================================================================

/**
 * Extracts the event type from a StreamEvent
 */
export function getEventType(event: StreamEvent): StreamEventType {
  return event.type;
}

/**
 * Extracts the event data from a StreamEvent
 */
export function getEventData(event: StreamEvent): ExecutionStep['data'] {
  return event;
}

/**
 * Creates an ExecutionStep from a StreamEvent
 */
export function createExecutionStep(event: StreamEvent): ExecutionStep {
  const type = getEventType(event);
  const data = getEventData(event);

  let subtaskId: number | undefined;
  let iteration: number | undefined;

  // Extract subtask_id and iteration if present
  if ('subtask_id' in data && data.subtask_id !== undefined) {
    subtaskId = data.subtask_id;
  }
  if ('iteration' in data && data.iteration !== undefined) {
    iteration = data.iteration;
  }

  return {
    id: `${type}-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
    type,
    timestamp: Date.now(),
    subtaskId,
    iteration,
    data,
  };
}
