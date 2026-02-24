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
  type: 'text' | 'image_url' | 'input_file';
  text?: string;
  image_url?: {
    url: string;
  };
  input_file?: {
    file_id?: string;
    filename?: string;
    file_data?: string;
  };
}

export interface FileAttachment {
  path: string;
  name: string;
  size: number;
  mimeType: string;
  extension: string;
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
  privacy_chat?: SanitizedChatConfig;
  embedding?: SanitizedEmbeddingConfig;
  memory?: SanitizedMemoryConfig;
  rag?: SanitizedRagConfig;
  mcp?: SanitizedMcpConfig;
  skill?: SanitizedSkillConfig;
  artifacts?: SanitizedArtifactsConfig;
  subagent?: SanitizedSubagentConfig;
  session?: SanitizedSessionConfig;
  lantai_auto_memory?: SanitizedLantaiAutoMemoryConfig;
  updatable_fields: string[];
}

export interface SanitizedSessionConfig {
  enable: boolean;
  storage_path: string;
}

export interface SanitizedLantaiAutoMemoryConfig {
  auto_summary: boolean;
  checkpoint_token_ratio: number;
  embedding_model: string;
  embedding_dimensions: number;
  embedding_batch_size: number;
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
  model_context_size: number;
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
  command?: string;
  enable: boolean;
  tools_count: number;
  api_key_configured: boolean;
  api_key_param?: string;
}

// ============================================================================
// MCP Management API Types
// ============================================================================

export interface McpServerListResponse {
  servers: SanitizedMcpToolServer[];
  total: number;
  enabled_count: number;
}

export interface ToggleMcpServerResponse {
  success: boolean;
  message: string;
  server: SanitizedMcpToolServer;
}

export interface UpdateApiKeyResponse {
  success: boolean;
  message: string;
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
  retention_days: number;
  cleanup_interval_secs: number;
  soft_delete_retention_days: number;
  enable_cleanup: boolean;
}

export interface SanitizedSubagentConfig {
  enabled: boolean;
  execution_mode: 'direct' | 'subagent';
  parallel_mode: 'auto' | 'sequential' | 'manual';
  max_concurrent: number;
  default_timeout_secs: number;
  default_max_iterations: number;
}

// Config Update
export interface ConfigUpdateRequest {
  server?: ServerConfigUpdate;
  chat?: ChatConfigUpdate;
  embedding?: EmbeddingConfigUpdate;
  memory?: MemoryConfigUpdate;
  rag?: RagConfigUpdate;
  subagent?: SubagentConfigUpdate;
  lantai_auto_memory?: LantaiAutoMemoryConfigUpdate;
}

export interface ChatConfigUpdate {
  model_context_size?: number;
}

export interface LantaiAutoMemoryConfigUpdate {
  auto_summary?: boolean;
  checkpoint_token_ratio?: number;
  embedding_model?: string;
  embedding_dimensions?: number;
  embedding_batch_size?: number;
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

export interface SubagentConfigUpdate {
  execution_mode?: 'direct' | 'subagent';
  parallel_mode?: 'auto' | 'sequential' | 'manual';
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
// Session History API Types
// ============================================================================

export interface SessionMeta {
  session_id: string;
  user_id: string;
  model: string;
  title: string;
  created_at: string;
  updated_at: string;
  message_count: number;
}

export interface SessionListResponse {
  sessions: SessionMeta[];
  total: number;
}

export interface SessionTokenUsage {
  prompt: number;
  completion: number;
}

export type SessionRecordType = 'session_start' | 'message';

export interface SessionStartRecord {
  type: 'session_start';
  version: number;
  session_id: string;
  user_id: string;
  model: string;
  created_at: string;
}

export interface SessionMessageRecord {
  type: 'message';
  version: number;
  role: 'user' | 'assistant' | 'system' | 'tool';
  content: string;
  timestamp: string;
  message_id: string;
  sequence: number;
  tokens?: SessionTokenUsage;
  tool_calls?: unknown[];
  privacy_mode?: boolean;
}

export type SessionRecord = SessionStartRecord | SessionMessageRecord;

export interface SessionDetailResponse {
  records: SessionRecord[];
}

export interface SessionDeleteResponse {
  success: boolean;
  session_id: string;
  message: string;
}

export interface BatchDeleteSessionsResponse {
  success: boolean;
  deleted_count: number;
  deleted_ids: string[];
  failed_ids: string[];
  message: string;
}

// ============================================================================
// Skills API Types
// ============================================================================

export interface SkillSummary {
  name: string;
  description: string;
  allowed_tools: string[];
  parameters?: Record<string, unknown>;
}

export interface SkillDetail {
  name: string;
  description: string;
  enabled: boolean;
  license?: string;
  allowed_tools: string[];
  allowed_scripts?: string[];
  scripts: string[];
  content: string;
  parameters?: Record<string, unknown>;
}

export interface SkillListResponse {
  skills: SkillSummary[];
  total: number;
}

export interface InstallSkillResponse {
  success: boolean;
  message: string;
  skill_name?: string;
}

export interface SkillEnvResponse {
  skill_name: string;
  env_vars: Record<string, string>;
}

export interface UpdateSkillEnvResponse {
  success: boolean;
  message: string;
  skill_name: string;
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
  created_at: string;
  updated_at: string;
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
  /** Whether this message was processed via privacy mode */
  privacyMode?: boolean;
  /** File attachments included with this message */
  attachments?: FileAttachment[];
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
  status: 'pending' | 'in_progress' | 'completed' | 'failed' | 'skipped' | 'interrupted';

  // Sub-Agent association (for subagent execution mode)
  subAgentId?: string;

  // Execution progress details
  progress?: {
    iteration: number;
    maxIterations: number;
    lastToolName?: string;
    message?: string;
  };

  // Tool calls made during execution
  toolCalls?: SubAgentToolCall[];

  // Result when completed
  result?: {
    output: string;
    metrics?: SubAgentMetrics;
  };

  // Error information
  error?: string;
  retryCount?: number;

  // UI state
  expanded?: boolean;
}

/** Tool call made by a Sub-Agent */
export interface SubAgentToolCall {
  id: string;
  name: string;
  arguments: Record<string, unknown>;
  iteration?: number;
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

export interface ErrorEvent {
  message: string;
  type?: 'user_interrupted' | 'timeout' | 'error';
}

export type EnhancedStreamEvent =
  | { type: 'status'; data: StatusEvent }
  | { type: 'plan'; data: PlanEvent }
  | { type: 'thought'; data: ThoughtEvent }
  | { type: 'tool_call'; data: ToolCallEvent }
  | { type: 'tool_result'; data: ToolResultEvent }
  | { type: 'text'; data: TextEvent }
  | { type: 'finish'; data: FinishEvent }
  | { type: 'error'; data: ErrorEvent }
  | { type: 'subagent_spawned'; data: SubAgentSpawnedEvent }
  | { type: 'subagent_started'; data: SubAgentStartedEvent }
  | { type: 'subagent_progress'; data: SubAgentProgressEvent }
  | { type: 'subagent_tool_call'; data: SubAgentToolCallEvent }
  | { type: 'subagent_completed'; data: SubAgentCompletedEvent }
  | { type: 'subagent_failed'; data: SubAgentFailedEvent }
  | { type: 'hitl_request'; data: HitlRequestEvent }
  | { type: 'hitl_status'; data: HitlStatusEvent }
  | { type: 'hitl_timeout_warning'; data: HitlTimeoutWarningEvent };

// ============================================================================
// Sub-Agent Event Types
// ============================================================================

/** Sub-Agent state enum */
export type SubAgentState = 'pending' | 'running' | 'completed' | 'failed' | 'cancelled' | 'interrupted';

/** Base event data for Sub-Agent events */
interface SubAgentEventBase {
  subagent_id: string;
  name: string;
  parent_id?: string;
}

/** Event emitted when a Sub-Agent is spawned */
export interface SubAgentSpawnedEvent extends SubAgentEventBase {
  task: string;
  depth: number;
}

/** Event emitted when a Sub-Agent starts execution */
export interface SubAgentStartedEvent extends SubAgentEventBase {
  timestamp: number;
}

/** Event emitted for Sub-Agent progress updates */
export interface SubAgentProgressEvent extends SubAgentEventBase {
  iteration: number;
  message?: string;
  tool_name?: string;
}

/** Event emitted when a Sub-Agent makes a tool call */
export interface SubAgentToolCallEvent {
  subagent_id: string;
  tool_call_id: string;
  tool_name: string;
  args: Record<string, unknown>;
  iteration?: number;
}

/** Event emitted when a Sub-Agent completes successfully */
export interface SubAgentCompletedEvent extends SubAgentEventBase {
  output: string;
  metrics: SubAgentMetrics;
}

/** Event emitted when a Sub-Agent fails */
export interface SubAgentFailedEvent extends SubAgentEventBase {
  error: string;
  metrics?: SubAgentMetrics;
}

/** Sub-Agent execution metrics */
export interface SubAgentMetrics {
  total_iterations: number;
  tool_calls: number;
  prompt_tokens: number;
  completion_tokens: number;
  duration_ms: number;
}

// ============================================================================
// Sub-Agent UI Types
// ============================================================================

/** UI representation of a Sub-Agent */
export interface UISubAgent {
  id: string;
  name: string;
  task: string;
  state: SubAgentState;
  depth: number;
  parentId?: string;
  /** Child Sub-Agent IDs */
  childIds: string[];
  /** Progress information */
  progress?: {
    iteration: number;
    message?: string;
    lastToolName?: string;
  };
  /** Result when completed */
  result?: {
    output: string;
    metrics: SubAgentMetrics;
  };
  /** Error message when failed */
  error?: string;
  /** Timestamps */
  createdAt: number;
  startedAt?: number;
  completedAt?: number;
}

// ============================================================================
// HITL (Human-in-the-Loop) API Types
// ============================================================================

/** Risk level for HITL operations */
export type HitlRiskLevel = 'safe' | 'low' | 'medium' | 'high' | 'critical';

/** HITL request status */
export type HitlRequestStatus =
  | 'pending'
  | 'approved'
  | 'rejected'
  | 'modified'
  | 'expired'
  | 'cancelled'
  | 'completed';

/** Timeout behavior for HITL requests */
export type HitlTimeoutBehavior = 'approve' | 'reject' | 'skip' | 'wait';

/** File operation type */
export type FileOperationType = 'create' | 'modify' | 'delete' | 'move' | 'copy';

/** Risk factor for HITL operations */
export interface HitlRiskFactor {
  category: string;
  description: string;
  severity: HitlRiskLevel;
}

/** File operation preview */
export interface FileOperationPreview {
  type: 'file';
  operation: FileOperationType;
  path: string;
  content_preview?: string;
  original_content?: string;
  size_bytes?: number;
  is_binary?: boolean;
}

/** Shell command preview */
export interface ShellCommandPreview {
  type: 'shell';
  command: string;
  working_directory?: string;
  environment?: Record<string, string>;
  estimated_impact?: string[];
}

/** HTTP request preview */
export interface HttpRequestPreview {
  type: 'http';
  method: string;
  url: string;
  headers?: Record<string, string>;
  body_preview?: string;
}

/** Generic operation preview */
export interface GenericPreview {
  type: 'generic';
  title: string;
  description: string;
  details?: Record<string, string>;
}

/** Operation preview union type */
export type HitlOperationPreview =
  | FileOperationPreview
  | ShellCommandPreview
  | HttpRequestPreview
  | GenericPreview;

/** Confirmation request details */
export interface HitlConfirmationRequest {
  summary: string;
  risk_level: HitlRiskLevel;
  tool_name: string;
  tool_args: Record<string, unknown>;
  preview: HitlOperationPreview;
  risk_factors: HitlRiskFactor[];
  allow_modification: boolean;
  modifiable_fields: string[];
}

/** Clarification request details */
export interface HitlClarificationRequest {
  question: string;
  context?: string;
  options?: string[];
  allow_free_input: boolean;
}

/** Feedback request details */
export interface HitlFeedbackRequest {
  summary: string;
  context?: string;
  rating_requested: boolean;
  comment_requested: boolean;
}

/** Pause request details */
export interface HitlPauseRequest {
  reason: 'user_requested' | 'error_threshold' | 'checkpoint' | 'resource_limit';
  current_state: string;
  completed_steps: string[];
  pending_steps: string[];
}

/** Privacy mode confirmation request details */
export interface HitlPrivacyModeConfirmationRequest {
  query_summary: string;
  detected_patterns: DetectedPrivacyPattern[];
  confidence: number;
  recommendation: string;
}

/** Detected privacy pattern */
export interface DetectedPrivacyPattern {
  category: string;
  description: string;
}

/** HITL request type union */
export type HitlRequestType =
  | { type: 'confirmation'; data: HitlConfirmationRequest }
  | { type: 'clarification'; data: HitlClarificationRequest }
  | { type: 'feedback'; data: HitlFeedbackRequest }
  | { type: 'pause'; data: HitlPauseRequest }
  | { type: 'privacy_mode_confirmation'; data: HitlPrivacyModeConfirmationRequest };

/** HITL request from API (matches backend HitlRequestDetailResponse) */
export interface HitlRequestFromApi {
  id: string;
  conversation_id: string;
  user_id: string;
  request_type: string; // "confirmation" | "clarification" | "feedback" | "pause" | "privacy_mode_confirmation"
  details: HitlConfirmationRequest | HitlClarificationRequest | HitlFeedbackRequest | HitlPauseRequest | HitlPrivacyModeConfirmationRequest;
  status: HitlRequestStatus;
  created_at: string;
  updated_at?: string;
  expires_at: string;
  remaining_seconds?: number;
  timeout_behavior: string;
  metadata?: Record<string, unknown>;
  response?: unknown;
  /** Optional subtask ID (1-based) */
  subtask_id?: number;
  /** Optional Sub-Agent ID */
  subagent_id?: string;
}

/** HITL request (normalized for frontend use) */
export interface HitlRequest {
  id: string;
  conversation_id: string;
  user_id: string;
  request_type: HitlRequestType;
  status: HitlRequestStatus;
  created_at: string;
  expires_at: string;
  timeout_behavior: HitlTimeoutBehavior;
  responded_at?: string;
  /** Optional subtask ID (1-based) */
  subtask_id?: number;
  /** Optional Sub-Agent ID */
  subagent_id?: string;
}

/** HITL response types */
export type HitlResponseAction =
  | { action: 'approve' }
  | { action: 'reject'; reason?: string }
  | { action: 'modify'; modifications: Record<string, unknown> }
  | { action: 'abort'; reason?: string }
  | { action: 'clarify'; selected_option?: number; input?: string }
  | { action: 'provide_feedback'; rating?: number; comment?: string }
  | { action: 'resume' }
  | { action: 'privacy_mode_choice'; use_privacy_mode: boolean; remember_choice?: boolean };

/** Request to respond to a HITL request */
export interface HitlRespondRequest {
  response: HitlResponseAction;
}

/** Response from HITL respond endpoint */
export interface HitlRespondResponse {
  success: boolean;
  request_id: string;
  status: HitlRequestStatus;
  message?: string;
}

/** Response from list pending HITL requests */
export interface HitlPendingListResponse {
  requests: HitlRequest[];
  total: number;
}

/** Response from get HITL request history */
export interface HitlHistoryResponse {
  requests: HitlRequest[];
  total: number;
  page: number;
  page_size: number;
}

// ============================================================================
// HITL SSE Event Types
// ============================================================================

/** HITL request event - new request created */
export interface HitlRequestEvent {
  request_id: string;
  request_type: string;
  summary: string;
  risk_level?: HitlRiskLevel;
  tool_name?: string;
  conversation_id: string;
  expires_at: string;
  timeout_behavior: HitlTimeoutBehavior;
  /** Optional subtask ID (1-based) */
  subtask_id?: number;
  /** Optional Sub-Agent ID */
  subagent_id?: string;
}

/** HITL status event - request status changed */
export interface HitlStatusEvent {
  request_id: string;
  status: HitlRequestStatus;
  message: string;
}

/** HITL timeout warning event */
export interface HitlTimeoutWarningEvent {
  request_id: string;
  remaining_seconds: number;
}

// ============================================================================
// HITL UI Types
// ============================================================================

/** UI representation of a HITL request */
export interface UIHitlRequest extends HitlRequest {
  /** Time remaining before expiration (seconds) */
  remainingSeconds?: number;
  /** Whether the request is expanded in UI */
  expanded?: boolean;
  /** Loading state for responding */
  isResponding?: boolean;
}
