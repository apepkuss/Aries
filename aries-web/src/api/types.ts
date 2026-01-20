// ============================================================================
// Chat API Types
// ============================================================================

export interface ChatMessage {
  role: 'system' | 'user' | 'assistant' | 'tool';
  content: string | ContentPart[];
  name?: string;
  tool_calls?: ToolCall[];
  tool_call_id?: string;
}

export interface ContentPart {
  type: 'text' | 'image_url';
  text?: string;
  image_url?: {
    url: string;
  };
}

export interface ToolCall {
  id: string;
  type: 'function';
  function: {
    name: string;
    arguments: string;
  };
}

export interface ChatCompletionRequest {
  model?: string;
  messages: ChatMessage[];
  stream?: boolean;
  temperature?: number;
  max_tokens?: number;
  tools?: Tool[];
  tool_choice?: 'auto' | 'none' | { type: 'function'; function: { name: string } };
}

export interface Tool {
  type: 'function';
  function: {
    name: string;
    description: string;
    parameters?: Record<string, unknown>;
  };
}

export interface ChatCompletionResponse {
  id: string;
  object: string;
  created: number;
  model: string;
  choices: ChatChoice[];
  usage?: {
    prompt_tokens: number;
    completion_tokens: number;
    total_tokens: number;
  };
}

export interface ChatChoice {
  index: number;
  message: ChatMessage;
  finish_reason: 'stop' | 'length' | 'tool_calls' | 'content_filter' | null;
}

// SSE Stream Events
export interface ChatStreamDelta {
  role?: string;
  content?: string;
  tool_calls?: ToolCallDelta[];
}

export interface ToolCallDelta {
  index: number;
  id?: string;
  type?: string;
  function?: {
    name?: string;
    arguments?: string;
  };
}

export interface ChatStreamChoice {
  index: number;
  delta: ChatStreamDelta;
  finish_reason: string | null;
}

export interface ChatStreamChunk {
  id: string;
  object: string;
  created: number;
  model: string;
  choices: ChatStreamChoice[];
}

// ============================================================================
// Config API Types
// ============================================================================

export interface SanitizedConfig {
  server: SanitizedServerConfig;
  chat?: SanitizedChatConfig;
  embedding?: SanitizedEmbeddingConfig;
  memory?: SanitizedMemoryConfig;
  rag?: SanitizedRagConfig;
  mcp?: SanitizedMcpConfig;
  skill?: SanitizedSkillConfig;
  artifacts?: SanitizedArtifactsConfig;
  updatable_fields: string[];
}

export interface SanitizedServerConfig {
  host: string;
  port: number;
  max_tools_per_iteration: number;
  tool_call_max_retries: number;
  tool_call_retry_delay_ms: number;
  max_plan_subtasks: number;
  plan_timeout_secs: number;
  subtask_max_retries: number;
  subtask_react_max_iterations: number;
  subtask_react_timeout_secs: number;
}

export interface SanitizedChatConfig {
  url: string;
  model: string;
  api_key_configured: boolean;
}

export interface SanitizedEmbeddingConfig {
  url: string;
  api_key_configured: boolean;
}

export interface SanitizedMemoryConfig {
  enable: boolean;
  database_path: string;
  context_window: number;
  auto_summarize: boolean;
  summarization_strategy: 'Incremental' | 'FullHistory';
  summarize_threshold: number;
  max_stored_messages: number;
  summary_service_api_key_configured: boolean;
}

export interface SanitizedRagConfig {
  enable: boolean;
  policy: string;
  context_window: number;
}

export interface SanitizedMcpConfig {
  server: {
    tool_servers: SanitizedMcpToolServer[];
  };
}

export interface SanitizedMcpToolServer {
  name: string;
  transport: string;
  url?: string;
  enable: boolean;
  tools_count: number;
}

export interface SanitizedSkillConfig {
  enabled: boolean;
  directories: string[];
  max_reference_size: number;
  api_key_configured: boolean;
  market_api_key_configured: boolean;
}

export interface SanitizedArtifactsConfig {
  enabled: boolean;
  database_path: string;
  storage_path?: string;
  max_content_size: number;
  max_binary_size: number;
  max_versions: number;
  retention_days: number;
  cleanup_interval_secs: number;
  soft_delete_retention_days: number;
  enable_cleanup: boolean;
}

// Config Update
export interface ConfigUpdateRequest {
  server?: ServerConfigUpdate;
  chat?: ChatConfigUpdate;
  embedding?: EmbeddingConfigUpdate;
  memory?: MemoryConfigUpdate;
  rag?: RagConfigUpdate;
}

export interface ServerConfigUpdate {
  max_tools_per_iteration?: number;
  tool_call_max_retries?: number;
  tool_call_retry_delay_ms?: number;
  max_plan_subtasks?: number;
  plan_timeout_secs?: number;
  subtask_max_retries?: number;
  subtask_react_max_iterations?: number;
  subtask_react_timeout_secs?: number;
}

export interface ChatConfigUpdate {
  url?: string;
  model?: string;
  api_key?: string;
}

export interface EmbeddingConfigUpdate {
  url?: string;
  api_key?: string;
}

export interface MemoryConfigUpdate {
  auto_summarize?: boolean;
  summarization_strategy?: 'Incremental' | 'FullHistory';
  summarize_threshold?: number;
  max_stored_messages?: number;
}

export interface RagConfigUpdate {
  enable?: boolean;
}

export interface ConfigUpdateResponse {
  success: boolean;
  updated_fields: string[];
  failed_fields?: Record<string, string>;
  requires_action?: Record<string, string>;
  message: string;
}

// Config Schema
export interface ConfigSchemaResponse {
  updatable: ConfigSchemaSection;
  readonly: string[];
}

export interface ConfigSchemaSection {
  server?: Record<string, FieldSchema>;
  chat?: Record<string, FieldSchema>;
  embedding?: Record<string, FieldSchema>;
  memory?: Record<string, FieldSchema>;
  rag?: Record<string, FieldSchema>;
}

export interface FieldSchema {
  type: string;
  minimum?: number;
  maximum?: number;
  default?: unknown;
  description: string;
  side_effect?: string;
}

// ============================================================================
// Memory API Types
// ============================================================================

export interface Conversation {
  id: string;
  user_id: string;
  title: string;
  created_at: string;
  updated_at: string;
  message_count: number;
}

export interface ConversationListResponse {
  conversations: Conversation[];
  total: number;
}

export interface ConversationMessage {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  timestamp: string;
  tool_calls?: ToolCall[];
}

export interface ConversationHistoryResponse {
  messages: ConversationMessage[];
  summary?: string;
}

// ============================================================================
// Skills API Types
// ============================================================================

export interface Skill {
  name: string;
  description: string;
  enabled: boolean;
  tools: string[];
}

export interface SkillDetail extends Skill {
  version?: string;
  author?: string;
  source_path?: string;
}

export interface SkillsListResponse {
  skills: Skill[];
}

// ============================================================================
// Artifacts API Types
// ============================================================================

export interface Artifact {
  id: string;
  conversation_id: string;
  type: string;
  title: string;
  content: string;
  language?: string;
  version: number;
  created_at: string;
  updated_at: string;
}

export interface ArtifactVersion {
  version: number;
  content: string;
  created_at: string;
}

export interface ArtifactVersionsResponse {
  versions: ArtifactVersion[];
}

// ============================================================================
// Audio API Types
// ============================================================================

export interface TranscriptionRequest {
  file: File;
  model?: string;
  language?: string;
}

export interface TranscriptionResponse {
  text: string;
}

export interface SpeechRequest {
  input: string;
  model?: string;
  voice?: string;
  response_format?: 'mp3' | 'opus' | 'aac' | 'flac';
}

// ============================================================================
// UI Message Types (internal)
// ============================================================================

export type MessageRole = 'user' | 'assistant' | 'system';

export interface UIMessage {
  id: string;
  role: MessageRole;
  content: string;
  timestamp: Date;
  isStreaming?: boolean;
  thinking?: string;
  toolCalls?: UIToolCall[];
  /** Task plan with subtask list for ToDo display */
  taskPlan?: UITaskPlan;
  /** Execution events in chronological order for timeline display */
  executionEvents?: ExecutionEvent[];
  error?: string;
}

/** Task plan for UI display */
export interface UITaskPlan {
  goal: string;
  subtasks: UISubtask[];
}

/** Subtask within a task plan */
export interface UISubtask {
  id: number;
  description: string;
  status: 'pending' | 'in_progress' | 'completed' | 'failed' | 'skipped';
}

export interface UIToolCall {
  id: string;
  name: string;
  arguments: Record<string, unknown>;
  status: 'pending' | 'running' | 'success' | 'error';
  result?: string;
  error?: string;
  durationMs?: number;
}

// Execution event types for timeline display
export type ExecutionEventType = 'thought' | 'tool_call';

export interface ExecutionEvent {
  id: string;
  type: ExecutionEventType;
  timestamp: number;
  // For thought events
  content?: string;
  // For tool_call events
  toolCall?: UIToolCall;
}

// ============================================================================
// Enhanced Stream Event Types
// ============================================================================

export type EnhancedStreamEventType =
  | 'status'
  | 'plan'
  | 'thought'
  | 'tool_call'
  | 'tool_result'
  | 'text'
  | 'finish';

export interface StatusEvent {
  phase: 'planning' | 'executing' | 'reflecting' | 'completing';
  message: string;
  subtask_id?: number;
  subtask_total?: number;
  subtask_current?: number;
}

export interface PlanEvent {
  goal: string;
  subtasks: PlanSubtask[];
}

export interface PlanSubtask {
  id: number;
  description: string;
  status: 'pending' | 'in_progress' | 'completed' | 'failed' | 'skipped';
}

export interface ThoughtEvent {
  content: string;
  status: 'start' | 'streaming' | 'done';
  subtask_id?: number;
  iteration?: number;
}

export interface ToolCallEvent {
  tool_call_id: string;
  tool_name: string;
  args: Record<string, unknown>;
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

export interface TextEvent {
  content: string;
}

export interface FinishEvent {
  stop_reason: string;
  usage?: {
    prompt_tokens: number;
    completion_tokens: number;
    total_tokens: number;
  };
  summary?: {
    subtask_count: number;
    completed_count: number;
    failed_count: number;
    tool_call_count: number;
    duration_ms: number;
  };
}

export type EnhancedStreamEvent =
  | { type: 'status'; data: StatusEvent }
  | { type: 'plan'; data: PlanEvent }
  | { type: 'thought'; data: ThoughtEvent }
  | { type: 'tool_call'; data: ToolCallEvent }
  | { type: 'tool_result'; data: ToolResultEvent }
  | { type: 'text'; data: TextEvent }
  | { type: 'finish'; data: FinishEvent };
