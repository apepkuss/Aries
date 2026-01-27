//! Structured event types for enhanced SSE streaming.
//!
//! This module defines the event types and payload structures for real-time
//! streaming of Agent execution progress to the frontend.
//!
//! # Event Types
//!
//! - `thought`: Agent's reasoning process (Chain of Thought)
//! - `tool_call`: Agent decides to call a tool
//! - `tool_result`: Result of tool execution
//! - `text`: Final response text chunks
//! - `status`: Macro-level status updates
//! - `finish`: Task completion signal
//!
//! # Example SSE Output
//!
//! ```text
//! event: status
//! data: {"phase": "planning", "message": "Analyzing user intent..."}
//!
//! event: thought
//! data: {"content": "I need to use the weather tool.", "status": "done"}
//!
//! event: tool_call
//! data: {"tool_call_id": "call_123", "tool_name": "get_weather", "args": {"city": "Shanghai"}}
//!
//! event: tool_result
//! data: {"tool_call_id": "call_123", "result": "{\"temp\": 25}", "is_error": false, "duration_ms": 150}
//!
//! event: text
//! data: {"content": "The weather in Shanghai is sunny, 25°C."}
//!
//! event: finish
//! data: {"stop_reason": "stop", "usage": {"prompt_tokens": 100, "completion_tokens": 50, "total_tokens": 150}}
//! ```

use std::fmt;

use serde::{Deserialize, Serialize};

use super::trace::TokenUsage;

// ============================================================================
// Event Type Enumeration
// ============================================================================

/// Types of structured events that can be emitted during Agent execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamEventType {
    /// Agent's thinking process (Chain of Thought).
    Thought,
    /// Agent decides to call a tool.
    ToolCall,
    /// Result of a tool execution.
    ToolResult,
    /// Final response text chunk.
    Text,
    /// Macro-level execution status update.
    Status,
    /// Task plan with subtask list.
    Plan,
    /// Task completion signal.
    Finish,
    /// Artifact created event.
    ArtifactCreated,
    /// Artifact updated event.
    ArtifactUpdated,
    /// Artifact deleted event.
    ArtifactDeleted,
    /// Sub-Agent spawned event.
    SubAgentSpawned,
    /// Sub-Agent started execution.
    SubAgentStarted,
    /// Sub-Agent progress update.
    SubAgentProgress,
    /// Sub-Agent thought event.
    SubAgentThought,
    /// Sub-Agent tool call event.
    SubAgentToolCall,
    /// Sub-Agent completed successfully.
    SubAgentCompleted,
    /// Sub-Agent failed with error.
    SubAgentFailed,
    /// HITL request created (awaiting user response).
    HitlRequest,
    /// HITL request status changed.
    HitlStatus,
    /// HITL timeout warning.
    HitlTimeoutWarning,
}

impl fmt::Display for StreamEventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StreamEventType::Thought => write!(f, "thought"),
            StreamEventType::ToolCall => write!(f, "tool_call"),
            StreamEventType::ToolResult => write!(f, "tool_result"),
            StreamEventType::Text => write!(f, "text"),
            StreamEventType::Status => write!(f, "status"),
            StreamEventType::Plan => write!(f, "plan"),
            StreamEventType::Finish => write!(f, "finish"),
            StreamEventType::ArtifactCreated => write!(f, "artifact_created"),
            StreamEventType::ArtifactUpdated => write!(f, "artifact_updated"),
            StreamEventType::ArtifactDeleted => write!(f, "artifact_deleted"),
            StreamEventType::SubAgentSpawned => write!(f, "subagent_spawned"),
            StreamEventType::SubAgentStarted => write!(f, "subagent_started"),
            StreamEventType::SubAgentProgress => write!(f, "subagent_progress"),
            StreamEventType::SubAgentThought => write!(f, "subagent_thought"),
            StreamEventType::SubAgentToolCall => write!(f, "subagent_tool_call"),
            StreamEventType::SubAgentCompleted => write!(f, "subagent_completed"),
            StreamEventType::SubAgentFailed => write!(f, "subagent_failed"),
            StreamEventType::HitlRequest => write!(f, "hitl_request"),
            StreamEventType::HitlStatus => write!(f, "hitl_status"),
            StreamEventType::HitlTimeoutWarning => write!(f, "hitl_timeout_warning"),
        }
    }
}

// ============================================================================
// Thought Event
// ============================================================================

/// Status of a thought event during streaming.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThoughtStatus {
    /// Thought streaming has started.
    Start,
    /// Thought content is being streamed.
    Streaming,
    /// Thought streaming is complete.
    #[default]
    Done,
}

/// Event payload for Agent's thinking process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThoughtEvent {
    /// The thought content.
    pub content: String,
    /// Status of the thought (start/streaming/done).
    #[serde(default)]
    pub status: ThoughtStatus,
    /// Optional subtask ID this thought belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask_id: Option<usize>,
    /// Optional iteration number within the subtask.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iteration: Option<u32>,
}

impl ThoughtEvent {
    /// Creates a new ThoughtEvent with the given content.
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            status: ThoughtStatus::Done,
            subtask_id: None,
            iteration: None,
        }
    }

    /// Sets the thought status.
    pub fn with_status(mut self, status: ThoughtStatus) -> Self {
        self.status = status;
        self
    }

    /// Sets the subtask ID.
    pub fn with_subtask_id(mut self, subtask_id: usize) -> Self {
        self.subtask_id = Some(subtask_id);
        self
    }

    /// Sets the iteration number.
    pub fn with_iteration(mut self, iteration: u32) -> Self {
        self.iteration = Some(iteration);
        self
    }
}

// ============================================================================
// Tool Call Event
// ============================================================================

/// Event payload for tool call initiation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallEvent {
    /// Unique identifier for this tool call.
    pub tool_call_id: String,
    /// Name of the tool being called.
    pub tool_name: String,
    /// Arguments passed to the tool (as JSON value).
    pub args: serde_json::Value,
    /// Optional MCP server name hosting the tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    /// Optional subtask ID this tool call belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask_id: Option<usize>,
}

impl ToolCallEvent {
    /// Creates a new ToolCallEvent.
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        args: serde_json::Value,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            args,
            server_name: None,
            subtask_id: None,
        }
    }

    /// Sets the MCP server name.
    pub fn with_server_name(mut self, server_name: impl Into<String>) -> Self {
        self.server_name = Some(server_name.into());
        self
    }

    /// Sets the subtask ID.
    pub fn with_subtask_id(mut self, subtask_id: usize) -> Self {
        self.subtask_id = Some(subtask_id);
        self
    }
}

// ============================================================================
// Tool Result Event
// ============================================================================

/// Event payload for tool execution result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultEvent {
    /// The tool call ID this result corresponds to.
    pub tool_call_id: String,
    /// The result content (success output or error message).
    pub result: String,
    /// Whether the tool call resulted in an error.
    pub is_error: bool,
    /// Execution duration in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Optional subtask ID this result belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask_id: Option<usize>,
}

impl ToolResultEvent {
    /// Creates a new successful ToolResultEvent.
    pub fn success(tool_call_id: impl Into<String>, result: impl Into<String>) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            result: result.into(),
            is_error: false,
            duration_ms: None,
            subtask_id: None,
        }
    }

    /// Creates a new error ToolResultEvent.
    pub fn error(tool_call_id: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            result: error.into(),
            is_error: true,
            duration_ms: None,
            subtask_id: None,
        }
    }

    /// Sets the execution duration.
    pub fn with_duration_ms(mut self, duration_ms: u64) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }

    /// Sets the subtask ID.
    pub fn with_subtask_id(mut self, subtask_id: usize) -> Self {
        self.subtask_id = Some(subtask_id);
        self
    }
}

// ============================================================================
// Text Event
// ============================================================================

/// Event payload for final response text chunks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextEvent {
    /// The text content chunk.
    pub content: String,
}

impl TextEvent {
    /// Creates a new TextEvent.
    #[allow(dead_code)]
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
        }
    }
}

// ============================================================================
// Status Event
// ============================================================================

/// Execution phases for status events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPhase {
    /// Task planning phase.
    Planning,
    /// Task execution phase.
    Executing,
    /// Reflection/evaluation phase.
    Reflecting,
    /// Final response generation phase.
    Completing,
}

impl fmt::Display for ExecutionPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExecutionPhase::Planning => write!(f, "planning"),
            ExecutionPhase::Executing => write!(f, "executing"),
            ExecutionPhase::Reflecting => write!(f, "reflecting"),
            ExecutionPhase::Completing => write!(f, "completing"),
        }
    }
}

/// Event payload for macro-level status updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusEvent {
    /// Current execution phase.
    pub phase: ExecutionPhase,
    /// Human-readable status message.
    pub message: String,
    /// Optional subtask ID (for executing phase).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask_id: Option<usize>,
    /// Optional total subtask count.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask_total: Option<usize>,
    /// Optional current subtask index (1-based).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask_current: Option<usize>,
}

impl StatusEvent {
    /// Creates a new StatusEvent.
    pub fn new(phase: ExecutionPhase, message: impl Into<String>) -> Self {
        Self {
            phase,
            message: message.into(),
            subtask_id: None,
            subtask_total: None,
            subtask_current: None,
        }
    }

    /// Creates a planning phase status event.
    #[allow(dead_code)]
    pub fn planning(message: impl Into<String>) -> Self {
        Self::new(ExecutionPhase::Planning, message)
    }

    /// Creates an executing phase status event.
    #[allow(dead_code)]
    pub fn executing(message: impl Into<String>) -> Self {
        Self::new(ExecutionPhase::Executing, message)
    }

    /// Creates a reflecting phase status event.
    #[allow(dead_code)]
    pub fn reflecting(message: impl Into<String>) -> Self {
        Self::new(ExecutionPhase::Reflecting, message)
    }

    /// Creates a completing phase status event.
    #[allow(dead_code)]
    pub fn completing(message: impl Into<String>) -> Self {
        Self::new(ExecutionPhase::Completing, message)
    }

    /// Sets subtask progress information.
    pub fn with_subtask_progress(
        mut self,
        subtask_id: usize,
        current: usize,
        total: usize,
    ) -> Self {
        self.subtask_id = Some(subtask_id);
        self.subtask_current = Some(current);
        self.subtask_total = Some(total);
        self
    }
}

// ============================================================================
// Plan Event
// ============================================================================

/// Event payload for task plan with subtask list.
/// Sent after planning phase completes, contains all planned subtasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanEvent {
    /// Overall goal of the task plan.
    pub goal: String,
    /// List of planned subtasks.
    pub subtasks: Vec<PlanSubtask>,
}

/// A subtask within a plan event (simplified view for frontend).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanSubtask {
    /// Subtask ID (0-indexed).
    pub id: usize,
    /// Human-readable description of the subtask.
    pub description: String,
    /// Current status of the subtask.
    pub status: String,
}

impl PlanEvent {
    /// Creates a new PlanEvent from goal and subtasks.
    pub fn new(goal: impl Into<String>, subtasks: Vec<PlanSubtask>) -> Self {
        Self {
            goal: goal.into(),
            subtasks,
        }
    }
}

impl PlanSubtask {
    /// Creates a new PlanSubtask.
    pub fn new(id: usize, description: impl Into<String>, status: impl Into<String>) -> Self {
        Self {
            id,
            description: description.into(),
            status: status.into(),
        }
    }
}

// ============================================================================
// Finish Event
// ============================================================================

/// Event payload for task completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinishEvent {
    /// Reason for stopping (e.g., "stop", "max_tokens", "error").
    pub stop_reason: String,
    /// Token usage statistics.
    pub usage: TokenUsage,
    /// Optional error message if stop_reason is "error".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Optional execution summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<ExecutionSummary>,
}

/// Summary of the execution for the finish event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSummary {
    /// Total number of subtasks.
    pub subtask_count: usize,
    /// Number of completed subtasks.
    pub completed_count: usize,
    /// Number of failed subtasks.
    pub failed_count: usize,
    /// Total number of tool calls made.
    pub tool_call_count: usize,
    /// Total execution duration in milliseconds.
    pub duration_ms: u64,
}

impl FinishEvent {
    /// Creates a new successful FinishEvent.
    #[allow(dead_code)]
    pub fn success(usage: TokenUsage) -> Self {
        Self {
            stop_reason: "stop".to_string(),
            usage,
            error: None,
            summary: None,
        }
    }

    /// Creates a new error FinishEvent.
    #[allow(dead_code)]
    pub fn error(usage: TokenUsage, error: impl Into<String>) -> Self {
        Self {
            stop_reason: "error".to_string(),
            usage,
            error: Some(error.into()),
            summary: None,
        }
    }

    /// Creates a new FinishEvent with a custom stop reason.
    pub fn with_reason(stop_reason: impl Into<String>, usage: TokenUsage) -> Self {
        Self {
            stop_reason: stop_reason.into(),
            usage,
            error: None,
            summary: None,
        }
    }

    /// Sets the execution summary.
    pub fn with_summary(mut self, summary: ExecutionSummary) -> Self {
        self.summary = Some(summary);
        self
    }
}

// ============================================================================
// Artifact Events
// ============================================================================

/// Event payload when a new artifact is created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactCreatedEvent {
    /// Artifact ID.
    pub artifact_id: String,
    /// Artifact title (filename).
    pub title: String,
    /// Artifact type (code, markdown, etc.).
    pub artifact_type: serde_json::Value,
    /// Content preview (first 200 chars).
    pub preview: String,
    /// Content size in bytes.
    pub size: u64,
    /// Download URL.
    pub url: String,
    /// Optional subtask ID this artifact belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask_id: Option<usize>,
}

#[allow(dead_code)]
impl ArtifactCreatedEvent {
    /// Creates a new ArtifactCreatedEvent.
    pub fn new(
        artifact_id: impl Into<String>,
        title: impl Into<String>,
        artifact_type: serde_json::Value,
        content: &str,
        size: u64,
        url: impl Into<String>,
    ) -> Self {
        let preview = if content.len() > 200 {
            format!("{}...", &content[..200])
        } else {
            content.to_string()
        };

        Self {
            artifact_id: artifact_id.into(),
            title: title.into(),
            artifact_type,
            preview,
            size,
            url: url.into(),
            subtask_id: None,
        }
    }

    /// Sets the subtask ID.
    pub fn with_subtask_id(mut self, subtask_id: usize) -> Self {
        self.subtask_id = Some(subtask_id);
        self
    }
}

/// Event payload when an artifact is updated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactUpdatedEvent {
    /// Artifact ID.
    pub artifact_id: String,
    /// New version number.
    pub version: i32,
    /// Content preview (first 200 chars of new content).
    pub preview: String,
    /// Optional change description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_description: Option<String>,
    /// Optional subtask ID this update belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask_id: Option<usize>,
}

#[allow(dead_code)]
impl ArtifactUpdatedEvent {
    /// Creates a new ArtifactUpdatedEvent.
    pub fn new(artifact_id: impl Into<String>, version: i32, content: &str) -> Self {
        let preview = if content.len() > 200 {
            format!("{}...", &content[..200])
        } else {
            content.to_string()
        };

        Self {
            artifact_id: artifact_id.into(),
            version,
            preview,
            change_description: None,
            subtask_id: None,
        }
    }

    /// Sets the change description.
    pub fn with_change_description(mut self, description: impl Into<String>) -> Self {
        self.change_description = Some(description.into());
        self
    }

    /// Sets the subtask ID.
    pub fn with_subtask_id(mut self, subtask_id: usize) -> Self {
        self.subtask_id = Some(subtask_id);
        self
    }
}

/// Event payload when an artifact is deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactDeletedEvent {
    /// Artifact ID.
    pub artifact_id: String,
    /// Optional subtask ID this deletion belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask_id: Option<usize>,
}

#[allow(dead_code)]
impl ArtifactDeletedEvent {
    /// Creates a new ArtifactDeletedEvent.
    pub fn new(artifact_id: impl Into<String>) -> Self {
        Self {
            artifact_id: artifact_id.into(),
            subtask_id: None,
        }
    }

    /// Sets the subtask ID.
    pub fn with_subtask_id(mut self, subtask_id: usize) -> Self {
        self.subtask_id = Some(subtask_id);
        self
    }
}

// ============================================================================
// Sub-Agent Events
// ============================================================================

/// Event payload when a Sub-Agent is spawned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentSpawnedEvent {
    /// Sub-Agent ID.
    pub subagent_id: String,
    /// Sub-Agent name.
    pub name: String,
    /// Task assigned to the Sub-Agent.
    pub task: String,
    /// Parent Sub-Agent ID (if nested).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Nesting depth level.
    pub depth: u32,
}

impl SubAgentSpawnedEvent {
    /// Creates a new SubAgentSpawnedEvent.
    pub fn new(
        subagent_id: impl Into<String>,
        name: impl Into<String>,
        task: impl Into<String>,
    ) -> Self {
        Self {
            subagent_id: subagent_id.into(),
            name: name.into(),
            task: task.into(),
            parent_id: None,
            depth: 0,
        }
    }

    /// Sets the parent ID.
    pub fn with_parent(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_id = Some(parent_id.into());
        self
    }

    /// Sets the depth level.
    pub fn with_depth(mut self, depth: u32) -> Self {
        self.depth = depth;
        self
    }
}

/// Event payload when a Sub-Agent starts execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentStartedEvent {
    /// Sub-Agent ID.
    pub subagent_id: String,
    /// Sub-Agent name.
    pub name: String,
}

impl SubAgentStartedEvent {
    /// Creates a new SubAgentStartedEvent.
    pub fn new(subagent_id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            subagent_id: subagent_id.into(),
            name: name.into(),
        }
    }
}

/// Event payload for Sub-Agent progress updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentProgressEvent {
    /// Sub-Agent ID.
    pub subagent_id: String,
    /// Current iteration number.
    pub iteration: u32,
    /// Maximum iterations allowed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<u32>,
    /// Optional progress message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Name of the tool being called (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

impl SubAgentProgressEvent {
    /// Creates a new SubAgentProgressEvent.
    pub fn new(subagent_id: impl Into<String>, iteration: u32) -> Self {
        Self {
            subagent_id: subagent_id.into(),
            iteration,
            max_iterations: None,
            message: None,
            tool_name: None,
        }
    }

    /// Sets the maximum iterations.
    pub fn with_max_iterations(mut self, max: u32) -> Self {
        self.max_iterations = Some(max);
        self
    }

    /// Sets the progress message.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Sets the tool name.
    pub fn with_tool_name(mut self, tool_name: impl Into<String>) -> Self {
        self.tool_name = Some(tool_name.into());
        self
    }
}

/// Event payload for Sub-Agent thought (Chain of Thought).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentThoughtEvent {
    /// Sub-Agent ID.
    pub subagent_id: String,
    /// Thought content.
    pub content: String,
    /// Current iteration number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iteration: Option<u32>,
    /// Thought status.
    #[serde(default)]
    pub status: ThoughtStatus,
}

impl SubAgentThoughtEvent {
    /// Creates a new SubAgentThoughtEvent.
    pub fn new(subagent_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            subagent_id: subagent_id.into(),
            content: content.into(),
            iteration: None,
            status: ThoughtStatus::Done,
        }
    }

    /// Sets the iteration number.
    pub fn with_iteration(mut self, iteration: u32) -> Self {
        self.iteration = Some(iteration);
        self
    }

    /// Sets the thought status.
    pub fn with_status(mut self, status: ThoughtStatus) -> Self {
        self.status = status;
        self
    }
}

/// Event payload for Sub-Agent tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentToolCallEvent {
    /// Sub-Agent ID.
    pub subagent_id: String,
    /// Tool call ID.
    pub tool_call_id: String,
    /// Tool name.
    pub tool_name: String,
    /// Tool arguments.
    pub args: serde_json::Value,
    /// Current iteration number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iteration: Option<u32>,
}

impl SubAgentToolCallEvent {
    /// Creates a new SubAgentToolCallEvent.
    pub fn new(
        subagent_id: impl Into<String>,
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        args: serde_json::Value,
    ) -> Self {
        Self {
            subagent_id: subagent_id.into(),
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            args,
            iteration: None,
        }
    }

    /// Sets the iteration number.
    pub fn with_iteration(mut self, iteration: u32) -> Self {
        self.iteration = Some(iteration);
        self
    }
}

/// Event payload when a Sub-Agent completes successfully.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentCompletedEvent {
    /// Sub-Agent ID.
    pub subagent_id: String,
    /// Sub-Agent name.
    pub name: String,
    /// Final output/result.
    pub output: String,
    /// Total iterations executed.
    pub iterations: u32,
    /// Execution duration in milliseconds.
    pub duration_ms: u64,
}

impl SubAgentCompletedEvent {
    /// Creates a new SubAgentCompletedEvent.
    pub fn new(
        subagent_id: impl Into<String>,
        name: impl Into<String>,
        output: impl Into<String>,
        iterations: u32,
        duration_ms: u64,
    ) -> Self {
        Self {
            subagent_id: subagent_id.into(),
            name: name.into(),
            output: output.into(),
            iterations,
            duration_ms,
        }
    }
}

/// Event payload when a Sub-Agent fails.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentFailedEvent {
    /// Sub-Agent ID.
    pub subagent_id: String,
    /// Sub-Agent name.
    pub name: String,
    /// Error message.
    pub error: String,
    /// Total iterations executed before failure.
    pub iterations: u32,
    /// Execution duration in milliseconds.
    pub duration_ms: u64,
}

impl SubAgentFailedEvent {
    /// Creates a new SubAgentFailedEvent.
    pub fn new(
        subagent_id: impl Into<String>,
        name: impl Into<String>,
        error: impl Into<String>,
        iterations: u32,
        duration_ms: u64,
    ) -> Self {
        Self {
            subagent_id: subagent_id.into(),
            name: name.into(),
            error: error.into(),
            iterations,
            duration_ms,
        }
    }
}

// ============================================================================
// HITL Events
// ============================================================================

/// Event payload when a HITL (Human-in-the-Loop) request is created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HitlRequestEvent {
    /// HITL request ID.
    pub request_id: String,
    /// Request type (confirmation, clarification, feedback, pause).
    pub request_type: String,
    /// Summary of what needs user attention.
    pub summary: String,
    /// Risk level (for confirmation requests).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_level: Option<String>,
    /// Tool name (for confirmation requests).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Conversation ID.
    pub conversation_id: String,
    /// User ID.
    pub user_id: String,
    /// Expiration time (ISO 8601).
    pub expires_at: String,
    /// Remaining seconds until timeout.
    pub remaining_seconds: i64,
    /// Timeout behavior.
    pub timeout_behavior: String,
    /// Optional subtask ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask_id: Option<usize>,
    /// Optional Sub-Agent ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_id: Option<String>,
}

impl HitlRequestEvent {
    /// Creates a new HitlRequestEvent.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        request_id: impl Into<String>,
        request_type: impl Into<String>,
        summary: impl Into<String>,
        conversation_id: impl Into<String>,
        user_id: impl Into<String>,
        expires_at: impl Into<String>,
        remaining_seconds: i64,
        timeout_behavior: impl Into<String>,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            request_type: request_type.into(),
            summary: summary.into(),
            risk_level: None,
            tool_name: None,
            conversation_id: conversation_id.into(),
            user_id: user_id.into(),
            expires_at: expires_at.into(),
            remaining_seconds,
            timeout_behavior: timeout_behavior.into(),
            subtask_id: None,
            subagent_id: None,
        }
    }

    /// Sets the risk level.
    pub fn with_risk_level(mut self, risk_level: impl Into<String>) -> Self {
        self.risk_level = Some(risk_level.into());
        self
    }

    /// Sets the tool name.
    pub fn with_tool_name(mut self, tool_name: impl Into<String>) -> Self {
        self.tool_name = Some(tool_name.into());
        self
    }

    /// Sets the subtask ID.
    pub fn with_subtask_id(mut self, subtask_id: usize) -> Self {
        self.subtask_id = Some(subtask_id);
        self
    }

    /// Sets the Sub-Agent ID.
    pub fn with_subagent_id(mut self, subagent_id: impl Into<String>) -> Self {
        self.subagent_id = Some(subagent_id.into());
        self
    }
}

/// Event payload when a HITL request status changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HitlStatusEvent {
    /// HITL request ID.
    pub request_id: String,
    /// New status (approved, rejected, modified, timed_out, cancelled, completed).
    pub status: String,
    /// Human-readable message.
    pub message: String,
}

impl HitlStatusEvent {
    /// Creates a new HitlStatusEvent.
    pub fn new(
        request_id: impl Into<String>,
        status: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            status: status.into(),
            message: message.into(),
        }
    }
}

/// Event payload when a HITL request is about to time out.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HitlTimeoutWarningEvent {
    /// HITL request ID.
    pub request_id: String,
    /// Remaining seconds until timeout.
    pub remaining_seconds: u64,
}

impl HitlTimeoutWarningEvent {
    /// Creates a new HitlTimeoutWarningEvent.
    pub fn new(request_id: impl Into<String>, remaining_seconds: u64) -> Self {
        Self {
            request_id: request_id.into(),
            remaining_seconds,
        }
    }
}

// ============================================================================
// Unified Stream Event
// ============================================================================

/// A unified stream event that can hold any event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// Thought event.
    Thought(ThoughtEvent),
    /// Tool call event.
    ToolCall(ToolCallEvent),
    /// Tool result event.
    ToolResult(ToolResultEvent),
    /// Text event.
    Text(TextEvent),
    /// Status event.
    Status(StatusEvent),
    /// Plan event.
    Plan(PlanEvent),
    /// Finish event.
    Finish(FinishEvent),
    /// Artifact created event.
    ArtifactCreated(ArtifactCreatedEvent),
    /// Artifact updated event.
    ArtifactUpdated(ArtifactUpdatedEvent),
    /// Artifact deleted event.
    ArtifactDeleted(ArtifactDeletedEvent),
    /// Sub-Agent spawned event.
    SubAgentSpawned(SubAgentSpawnedEvent),
    /// Sub-Agent started event.
    SubAgentStarted(SubAgentStartedEvent),
    /// Sub-Agent progress event.
    SubAgentProgress(SubAgentProgressEvent),
    /// Sub-Agent thought event.
    SubAgentThought(SubAgentThoughtEvent),
    /// Sub-Agent tool call event.
    SubAgentToolCall(SubAgentToolCallEvent),
    /// Sub-Agent completed event.
    SubAgentCompleted(SubAgentCompletedEvent),
    /// Sub-Agent failed event.
    SubAgentFailed(SubAgentFailedEvent),
    /// HITL request event.
    HitlRequest(HitlRequestEvent),
    /// HITL status change event.
    HitlStatus(HitlStatusEvent),
    /// HITL timeout warning event.
    HitlTimeoutWarning(HitlTimeoutWarningEvent),
}

impl StreamEvent {
    /// Returns the event type.
    pub fn event_type(&self) -> StreamEventType {
        match self {
            StreamEvent::Thought(_) => StreamEventType::Thought,
            StreamEvent::ToolCall(_) => StreamEventType::ToolCall,
            StreamEvent::ToolResult(_) => StreamEventType::ToolResult,
            StreamEvent::Text(_) => StreamEventType::Text,
            StreamEvent::Status(_) => StreamEventType::Status,
            StreamEvent::Plan(_) => StreamEventType::Plan,
            StreamEvent::Finish(_) => StreamEventType::Finish,
            StreamEvent::ArtifactCreated(_) => StreamEventType::ArtifactCreated,
            StreamEvent::ArtifactUpdated(_) => StreamEventType::ArtifactUpdated,
            StreamEvent::ArtifactDeleted(_) => StreamEventType::ArtifactDeleted,
            StreamEvent::SubAgentSpawned(_) => StreamEventType::SubAgentSpawned,
            StreamEvent::SubAgentStarted(_) => StreamEventType::SubAgentStarted,
            StreamEvent::SubAgentProgress(_) => StreamEventType::SubAgentProgress,
            StreamEvent::SubAgentThought(_) => StreamEventType::SubAgentThought,
            StreamEvent::SubAgentToolCall(_) => StreamEventType::SubAgentToolCall,
            StreamEvent::SubAgentCompleted(_) => StreamEventType::SubAgentCompleted,
            StreamEvent::SubAgentFailed(_) => StreamEventType::SubAgentFailed,
            StreamEvent::HitlRequest(_) => StreamEventType::HitlRequest,
            StreamEvent::HitlStatus(_) => StreamEventType::HitlStatus,
            StreamEvent::HitlTimeoutWarning(_) => StreamEventType::HitlTimeoutWarning,
        }
    }
}

// ============================================================================
// Stream Configuration
// ============================================================================

/// HTTP header name for enabling enhanced streaming mode.
pub const ENHANCED_STREAM_HEADER: &str = "x-enhanced-stream";

/// Configuration for enhanced streaming mode.
#[derive(Debug, Clone, Default)]
pub struct EnhancedStreamConfig {
    /// Whether enhanced streaming is enabled.
    pub enabled: bool,
    /// Whether to include thought events.
    pub include_thoughts: bool,
    /// Whether to include tool call events.
    pub include_tool_calls: bool,
    /// Whether to include status events.
    pub include_status: bool,
    /// Whether to include artifact events.
    pub include_artifacts: bool,
    /// Whether to include Sub-Agent events.
    pub include_subagents: bool,
    /// Whether to include HITL events.
    pub include_hitl: bool,
}

impl EnhancedStreamConfig {
    /// Creates a new config with all events enabled.
    pub fn all_enabled() -> Self {
        Self {
            enabled: true,
            include_thoughts: true,
            include_tool_calls: true,
            include_status: true,
            include_artifacts: true,
            include_subagents: true,
            include_hitl: true,
        }
    }

    /// Creates a disabled config (standard OpenAI-compatible mode).
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Parses the enhanced stream configuration from HTTP headers.
    ///
    /// Supports the following header formats:
    /// - `X-Enhanced-Stream: true` - Enable all enhanced events
    /// - `X-Enhanced-Stream: false` - Disable (use standard mode)
    /// - `X-Enhanced-Stream: thoughts,tool_calls,artifacts` - Enable specific events
    pub fn from_headers(headers: &axum::http::HeaderMap) -> Self {
        let header_value = headers
            .get(ENHANCED_STREAM_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        Self::parse(header_value)
    }

    /// Parses the configuration from a string value.
    pub fn parse(value: &str) -> Self {
        let value = value.trim().to_lowercase();

        if value.is_empty() || value == "false" || value == "0" {
            return Self::disabled();
        }

        if value == "true" || value == "1" || value == "all" {
            return Self::all_enabled();
        }

        // Parse comma-separated event types
        let mut config = Self {
            enabled: true,
            include_thoughts: false,
            include_tool_calls: false,
            include_status: false,
            include_artifacts: false,
            include_subagents: false,
            include_hitl: false,
        };

        for part in value.split(',') {
            match part.trim() {
                "thoughts" | "thought" => config.include_thoughts = true,
                "tool_calls" | "tools" => config.include_tool_calls = true,
                "status" => config.include_status = true,
                "artifacts" | "artifact" => config.include_artifacts = true,
                "subagents" | "subagent" => config.include_subagents = true,
                "hitl" => config.include_hitl = true,
                "all" => {
                    config.include_thoughts = true;
                    config.include_tool_calls = true;
                    config.include_status = true;
                    config.include_artifacts = true;
                    config.include_subagents = true;
                    config.include_hitl = true;
                }
                _ => {}
            }
        }

        // If no specific events are enabled, enable all
        if !config.include_thoughts
            && !config.include_tool_calls
            && !config.include_status
            && !config.include_artifacts
            && !config.include_subagents
            && !config.include_hitl
        {
            config = Self::all_enabled();
        }

        config
    }

    /// Returns true if thought events should be emitted.
    pub fn should_emit_thought(&self) -> bool {
        self.enabled && self.include_thoughts
    }

    /// Returns true if tool call/result events should be emitted.
    pub fn should_emit_tool_events(&self) -> bool {
        self.enabled && self.include_tool_calls
    }

    /// Returns true if status events should be emitted.
    pub fn should_emit_status(&self) -> bool {
        self.enabled && self.include_status
    }

    /// Returns true if artifact events should be emitted.
    #[allow(dead_code)]
    pub fn should_emit_artifacts(&self) -> bool {
        self.enabled && self.include_artifacts
    }

    /// Returns true if Sub-Agent events should be emitted.
    pub fn should_emit_subagent_events(&self) -> bool {
        self.enabled && self.include_subagents
    }

    /// Returns true if HITL events should be emitted.
    pub fn should_emit_hitl(&self) -> bool {
        self.enabled && self.include_hitl
    }
}

// ============================================================================
// SSE Formatting Utilities
// ============================================================================

/// Formats an event as an SSE message.
///
/// # Example Output
///
/// ```text
/// event: thought
/// data: {"content": "I need to analyze this.", "status": "done"}
///
/// ```
pub fn format_sse_event<T: Serialize>(event_type: &str, data: &T) -> String {
    let json = serde_json::to_string(data)
        .unwrap_or_else(|e| format!(r#"{{"error": "Failed to serialize event: {}"}}"#, e));
    format!("event: {}\ndata: {}\n\n", event_type, json)
}

/// Formats a StreamEvent as an SSE message.
pub fn format_stream_event(event: &StreamEvent) -> String {
    let event_type = event.event_type().to_string();
    match event {
        StreamEvent::Thought(e) => format_sse_event(&event_type, e),
        StreamEvent::ToolCall(e) => format_sse_event(&event_type, e),
        StreamEvent::ToolResult(e) => format_sse_event(&event_type, e),
        StreamEvent::Text(e) => format_sse_event(&event_type, e),
        StreamEvent::Status(e) => format_sse_event(&event_type, e),
        StreamEvent::Plan(e) => format_sse_event(&event_type, e),
        StreamEvent::Finish(e) => format_sse_event(&event_type, e),
        StreamEvent::ArtifactCreated(e) => format_sse_event(&event_type, e),
        StreamEvent::ArtifactUpdated(e) => format_sse_event(&event_type, e),
        StreamEvent::ArtifactDeleted(e) => format_sse_event(&event_type, e),
        // Sub-Agent events
        StreamEvent::SubAgentSpawned(e) => format_sse_event(&event_type, e),
        StreamEvent::SubAgentStarted(e) => format_sse_event(&event_type, e),
        StreamEvent::SubAgentProgress(e) => format_sse_event(&event_type, e),
        StreamEvent::SubAgentThought(e) => format_sse_event(&event_type, e),
        StreamEvent::SubAgentToolCall(e) => format_sse_event(&event_type, e),
        StreamEvent::SubAgentCompleted(e) => format_sse_event(&event_type, e),
        StreamEvent::SubAgentFailed(e) => format_sse_event(&event_type, e),
        // HITL events
        StreamEvent::HitlRequest(e) => format_sse_event(&event_type, e),
        StreamEvent::HitlStatus(e) => format_sse_event(&event_type, e),
        StreamEvent::HitlTimeoutWarning(e) => format_sse_event(&event_type, e),
    }
}

/// Formats the SSE stream termination signal.
#[allow(dead_code)]
pub fn format_sse_done() -> &'static str {
    "data: [DONE]\n\n"
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_event_type_display() {
        assert_eq!(StreamEventType::Thought.to_string(), "thought");
        assert_eq!(StreamEventType::ToolCall.to_string(), "tool_call");
        assert_eq!(StreamEventType::ToolResult.to_string(), "tool_result");
        assert_eq!(StreamEventType::Text.to_string(), "text");
        assert_eq!(StreamEventType::Status.to_string(), "status");
        assert_eq!(StreamEventType::Finish.to_string(), "finish");
    }

    #[test]
    fn test_thought_event_serialization() {
        let event = ThoughtEvent::new("I need to search for information.")
            .with_status(ThoughtStatus::Done)
            .with_subtask_id(1)
            .with_iteration(2);

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"content\":\"I need to search for information.\""));
        assert!(json.contains("\"status\":\"done\""));
        assert!(json.contains("\"subtask_id\":1"));
        assert!(json.contains("\"iteration\":2"));
    }

    #[test]
    fn test_tool_call_event_serialization() {
        let event = ToolCallEvent::new(
            "call_123",
            "get_weather",
            serde_json::json!({"city": "Shanghai"}),
        )
        .with_server_name("weather-service");

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"tool_call_id\":\"call_123\""));
        assert!(json.contains("\"tool_name\":\"get_weather\""));
        assert!(json.contains("\"city\":\"Shanghai\""));
        assert!(json.contains("\"server_name\":\"weather-service\""));
    }

    #[test]
    fn test_tool_result_event_success() {
        let event = ToolResultEvent::success("call_123", r#"{"temp": 25}"#).with_duration_ms(150);

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"tool_call_id\":\"call_123\""));
        assert!(json.contains("\"is_error\":false"));
        assert!(json.contains("\"duration_ms\":150"));
    }

    #[test]
    fn test_tool_result_event_error() {
        let event = ToolResultEvent::error("call_456", "Connection timeout");

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"tool_call_id\":\"call_456\""));
        assert!(json.contains("\"is_error\":true"));
        assert!(json.contains("\"result\":\"Connection timeout\""));
    }

    #[test]
    fn test_status_event_with_progress() {
        let event = StatusEvent::executing("Processing subtask...").with_subtask_progress(2, 2, 5);

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"phase\":\"executing\""));
        assert!(json.contains("\"subtask_id\":2"));
        assert!(json.contains("\"subtask_current\":2"));
        assert!(json.contains("\"subtask_total\":5"));
    }

    #[test]
    fn test_finish_event_success() {
        let usage = TokenUsage::new(100, 50);
        let event = FinishEvent::success(usage);

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"stop_reason\":\"stop\""));
        assert!(json.contains("\"prompt_tokens\":100"));
        assert!(json.contains("\"completion_tokens\":50"));
        assert!(json.contains("\"total_tokens\":150"));
    }

    #[test]
    fn test_finish_event_with_summary() {
        let usage = TokenUsage::new(200, 100);
        let summary = ExecutionSummary {
            subtask_count: 3,
            completed_count: 2,
            failed_count: 1,
            tool_call_count: 5,
            duration_ms: 5000,
        };
        let event = FinishEvent::success(usage).with_summary(summary);

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"subtask_count\":3"));
        assert!(json.contains("\"completed_count\":2"));
        assert!(json.contains("\"failed_count\":1"));
        assert!(json.contains("\"tool_call_count\":5"));
        assert!(json.contains("\"duration_ms\":5000"));
    }

    #[test]
    fn test_format_sse_event() {
        let event = TextEvent::new("Hello, world!");
        let sse = format_sse_event("text", &event);

        assert!(sse.starts_with("event: text\n"));
        assert!(sse.contains("data: "));
        assert!(sse.contains("\"content\":\"Hello, world!\""));
        assert!(sse.ends_with("\n\n"));
    }

    #[test]
    fn test_format_stream_event() {
        let event = StreamEvent::Thought(ThoughtEvent::new("Thinking..."));
        let sse = format_stream_event(&event);

        assert!(sse.starts_with("event: thought\n"));
        assert!(sse.contains("\"content\":\"Thinking...\""));
    }

    #[test]
    fn test_stream_event_type_extraction() {
        let thought = StreamEvent::Thought(ThoughtEvent::new("test"));
        assert_eq!(thought.event_type(), StreamEventType::Thought);

        let tool_call =
            StreamEvent::ToolCall(ToolCallEvent::new("id", "tool", serde_json::json!({})));
        assert_eq!(tool_call.event_type(), StreamEventType::ToolCall);

        let status = StreamEvent::Status(StatusEvent::planning("test"));
        assert_eq!(status.event_type(), StreamEventType::Status);
    }

    #[test]
    fn test_execution_phase_display() {
        assert_eq!(ExecutionPhase::Planning.to_string(), "planning");
        assert_eq!(ExecutionPhase::Executing.to_string(), "executing");
        assert_eq!(ExecutionPhase::Reflecting.to_string(), "reflecting");
        assert_eq!(ExecutionPhase::Completing.to_string(), "completing");
    }

    #[test]
    fn test_enhanced_stream_config_disabled() {
        let config = EnhancedStreamConfig::parse("");
        assert!(!config.enabled);

        let config = EnhancedStreamConfig::parse("false");
        assert!(!config.enabled);

        let config = EnhancedStreamConfig::parse("0");
        assert!(!config.enabled);
    }

    #[test]
    fn test_enhanced_stream_config_all_enabled() {
        let config = EnhancedStreamConfig::parse("true");
        assert!(config.enabled);
        assert!(config.include_thoughts);
        assert!(config.include_tool_calls);
        assert!(config.include_status);

        let config = EnhancedStreamConfig::parse("1");
        assert!(config.enabled);

        let config = EnhancedStreamConfig::parse("all");
        assert!(config.enabled);
        assert!(config.include_thoughts);
        assert!(config.include_tool_calls);
        assert!(config.include_status);
    }

    #[test]
    fn test_enhanced_stream_config_selective() {
        let config = EnhancedStreamConfig::parse("thoughts");
        assert!(config.enabled);
        assert!(config.include_thoughts);
        assert!(!config.include_tool_calls);
        assert!(!config.include_status);

        let config = EnhancedStreamConfig::parse("thoughts,tool_calls");
        assert!(config.enabled);
        assert!(config.include_thoughts);
        assert!(config.include_tool_calls);
        assert!(!config.include_status);

        let config = EnhancedStreamConfig::parse("tools, status");
        assert!(config.enabled);
        assert!(!config.include_thoughts);
        assert!(config.include_tool_calls);
        assert!(config.include_status);
    }

    #[test]
    fn test_enhanced_stream_config_should_emit() {
        let config = EnhancedStreamConfig::all_enabled();
        assert!(config.should_emit_thought());
        assert!(config.should_emit_tool_events());
        assert!(config.should_emit_status());

        let config = EnhancedStreamConfig::disabled();
        assert!(!config.should_emit_thought());
        assert!(!config.should_emit_tool_events());
        assert!(!config.should_emit_status());

        let config = EnhancedStreamConfig::parse("thoughts");
        assert!(config.should_emit_thought());
        assert!(!config.should_emit_tool_events());
        assert!(!config.should_emit_status());
    }

    #[test]
    fn test_thought_status_serialization() {
        assert_eq!(
            serde_json::to_string(&ThoughtStatus::Start).unwrap(),
            "\"start\""
        );
        assert_eq!(
            serde_json::to_string(&ThoughtStatus::Streaming).unwrap(),
            "\"streaming\""
        );
        assert_eq!(
            serde_json::to_string(&ThoughtStatus::Done).unwrap(),
            "\"done\""
        );
    }

    #[test]
    fn test_text_event_serialization() {
        let event = TextEvent::new("Hello, world!");
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(json, r#"{"content":"Hello, world!"}"#);
    }

    #[test]
    fn test_status_event_all_phases() {
        let planning = StatusEvent::planning("Starting plan generation");
        assert!(
            serde_json::to_string(&planning)
                .unwrap()
                .contains("\"phase\":\"planning\"")
        );

        let executing = StatusEvent::executing("Processing subtask");
        assert!(
            serde_json::to_string(&executing)
                .unwrap()
                .contains("\"phase\":\"executing\"")
        );

        let reflecting = StatusEvent::reflecting("Analyzing result");
        assert!(
            serde_json::to_string(&reflecting)
                .unwrap()
                .contains("\"phase\":\"reflecting\"")
        );

        let completing = StatusEvent::completing("Generating final response");
        assert!(
            serde_json::to_string(&completing)
                .unwrap()
                .contains("\"phase\":\"completing\"")
        );
    }

    #[test]
    fn test_finish_event_with_error() {
        let usage = TokenUsage::new(50, 25);
        let event = FinishEvent::error(usage, "Tool execution failed");

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"stop_reason\":\"error\""));
        assert!(json.contains("\"error\":\"Tool execution failed\""));
    }

    #[test]
    fn test_all_stream_event_types() {
        // Verify all 6 event types can be wrapped in StreamEvent
        let events: Vec<StreamEvent> = vec![
            StreamEvent::Thought(ThoughtEvent::new("thinking")),
            StreamEvent::ToolCall(ToolCallEvent::new("id", "tool", serde_json::json!({}))),
            StreamEvent::ToolResult(ToolResultEvent::success("id", "result")),
            StreamEvent::Text(TextEvent::new("content")),
            StreamEvent::Status(StatusEvent::planning("message")),
            StreamEvent::Finish(FinishEvent::success(TokenUsage::default())),
        ];

        let expected_types = vec![
            StreamEventType::Thought,
            StreamEventType::ToolCall,
            StreamEventType::ToolResult,
            StreamEventType::Text,
            StreamEventType::Status,
            StreamEventType::Finish,
        ];

        for (event, expected_type) in events.iter().zip(expected_types.iter()) {
            assert_eq!(event.event_type(), *expected_type);
        }
    }

    #[test]
    fn test_sse_format_complete_event_sequence() {
        // Simulate a complete Agent execution event sequence
        let events = vec![
            format_stream_event(&StreamEvent::Status(StatusEvent::planning(
                "Generating plan",
            ))),
            format_stream_event(&StreamEvent::Status(
                StatusEvent::executing("Executing subtask 1").with_subtask_progress(1, 1, 2),
            )),
            format_stream_event(&StreamEvent::Thought(
                ThoughtEvent::new("I need to search for weather data")
                    .with_subtask_id(1)
                    .with_iteration(1),
            )),
            format_stream_event(&StreamEvent::ToolCall(
                ToolCallEvent::new(
                    "call_1",
                    "get_weather",
                    serde_json::json!({"city": "Shanghai"}),
                )
                .with_server_name("weather-api"),
            )),
            format_stream_event(&StreamEvent::ToolResult(
                ToolResultEvent::success("call_1", r#"{"temp": 25, "condition": "sunny"}"#)
                    .with_duration_ms(120),
            )),
            format_stream_event(&StreamEvent::Status(StatusEvent::completing(
                "Generating response",
            ))),
            format_stream_event(&StreamEvent::Finish(
                FinishEvent::success(TokenUsage::new(100, 50)).with_summary(ExecutionSummary {
                    subtask_count: 2,
                    completed_count: 2,
                    failed_count: 0,
                    tool_call_count: 1,
                    duration_ms: 1500,
                }),
            )),
            format_stream_event(&StreamEvent::Text(TextEvent::new(
                "The weather in Shanghai is sunny, 25°C.",
            ))),
        ];

        // Verify each event has proper SSE format
        for event in &events {
            assert!(event.starts_with("event: "));
            assert!(event.contains("\ndata: "));
            assert!(event.ends_with("\n\n"));
        }

        // Verify event type order
        assert!(events[0].starts_with("event: status\n"));
        assert!(events[1].starts_with("event: status\n"));
        assert!(events[2].starts_with("event: thought\n"));
        assert!(events[3].starts_with("event: tool_call\n"));
        assert!(events[4].starts_with("event: tool_result\n"));
        assert!(events[5].starts_with("event: status\n"));
        assert!(events[6].starts_with("event: finish\n"));
        assert!(events[7].starts_with("event: text\n"));
    }

    // ========================================================================
    // Artifact Event Tests
    // ========================================================================

    #[test]
    fn test_artifact_event_type_display() {
        assert_eq!(
            StreamEventType::ArtifactCreated.to_string(),
            "artifact_created"
        );
        assert_eq!(
            StreamEventType::ArtifactUpdated.to_string(),
            "artifact_updated"
        );
        assert_eq!(
            StreamEventType::ArtifactDeleted.to_string(),
            "artifact_deleted"
        );
    }

    #[test]
    fn test_artifact_created_event_serialization() {
        let event = ArtifactCreatedEvent::new(
            "art_123",
            "main.rs",
            serde_json::json!({"code": {"language": "rust"}}),
            "fn main() {\n    println!(\"Hello, world!\");\n}",
            42,
            "/v1/artifacts/art_123/download",
        )
        .with_subtask_id(1);

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"artifact_id\":\"art_123\""));
        assert!(json.contains("\"title\":\"main.rs\""));
        assert!(json.contains("\"size\":42"));
        assert!(json.contains("\"subtask_id\":1"));
        assert!(json.contains("\"url\":\"/v1/artifacts/art_123/download\""));
    }

    #[test]
    fn test_artifact_created_event_preview_truncation() {
        let long_content = "x".repeat(300);
        let event = ArtifactCreatedEvent::new(
            "art_456",
            "large.txt",
            serde_json::json!("text"),
            &long_content,
            300,
            "/v1/artifacts/art_456/download",
        );

        // Preview should be truncated to 200 chars + "..."
        assert_eq!(event.preview.len(), 203);
        assert!(event.preview.ends_with("..."));
    }

    #[test]
    fn test_artifact_updated_event_serialization() {
        let event = ArtifactUpdatedEvent::new("art_123", 2, "updated content here")
            .with_change_description("Fixed bug in main function")
            .with_subtask_id(1);

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"artifact_id\":\"art_123\""));
        assert!(json.contains("\"version\":2"));
        assert!(json.contains("\"preview\":\"updated content here\""));
        assert!(json.contains("\"change_description\":\"Fixed bug in main function\""));
        assert!(json.contains("\"subtask_id\":1"));
    }

    #[test]
    fn test_artifact_deleted_event_serialization() {
        let event = ArtifactDeletedEvent::new("art_789").with_subtask_id(2);

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"artifact_id\":\"art_789\""));
        assert!(json.contains("\"subtask_id\":2"));
    }

    #[test]
    fn test_stream_event_artifact_types() {
        let created = StreamEvent::ArtifactCreated(ArtifactCreatedEvent::new(
            "art_1",
            "file.txt",
            serde_json::json!("text"),
            "content",
            7,
            "/download",
        ));
        assert_eq!(created.event_type(), StreamEventType::ArtifactCreated);

        let updated =
            StreamEvent::ArtifactUpdated(ArtifactUpdatedEvent::new("art_1", 2, "new content"));
        assert_eq!(updated.event_type(), StreamEventType::ArtifactUpdated);

        let deleted = StreamEvent::ArtifactDeleted(ArtifactDeletedEvent::new("art_1"));
        assert_eq!(deleted.event_type(), StreamEventType::ArtifactDeleted);
    }

    #[test]
    fn test_format_artifact_events() {
        let created_event = StreamEvent::ArtifactCreated(ArtifactCreatedEvent::new(
            "art_001",
            "script.py",
            serde_json::json!({"code": {"language": "python"}}),
            "print('hello')",
            14,
            "/v1/artifacts/art_001/download",
        ));
        let sse = format_stream_event(&created_event);
        assert!(sse.starts_with("event: artifact_created\n"));
        assert!(sse.contains("\"artifact_id\":\"art_001\""));

        let updated_event = StreamEvent::ArtifactUpdated(ArtifactUpdatedEvent::new(
            "art_001",
            2,
            "print('hello world')",
        ));
        let sse = format_stream_event(&updated_event);
        assert!(sse.starts_with("event: artifact_updated\n"));
        assert!(sse.contains("\"version\":2"));

        let deleted_event = StreamEvent::ArtifactDeleted(ArtifactDeletedEvent::new("art_001"));
        let sse = format_stream_event(&deleted_event);
        assert!(sse.starts_with("event: artifact_deleted\n"));
        assert!(sse.contains("\"artifact_id\":\"art_001\""));
    }

    #[test]
    fn test_enhanced_stream_config_with_artifacts() {
        let config = EnhancedStreamConfig::parse("artifacts");
        assert!(config.enabled);
        assert!(config.include_artifacts);
        assert!(!config.include_thoughts);
        assert!(!config.include_tool_calls);
        assert!(!config.include_status);

        let config = EnhancedStreamConfig::parse("thoughts,artifacts");
        assert!(config.enabled);
        assert!(config.include_thoughts);
        assert!(config.include_artifacts);
        assert!(!config.include_tool_calls);

        let config = EnhancedStreamConfig::all_enabled();
        assert!(config.should_emit_artifacts());

        let config = EnhancedStreamConfig::disabled();
        assert!(!config.should_emit_artifacts());
    }

    #[test]
    fn test_artifact_event_sequence() {
        // Simulate artifact lifecycle events
        let events = vec![
            format_stream_event(&StreamEvent::ArtifactCreated(ArtifactCreatedEvent::new(
                "art_001",
                "main.rs",
                serde_json::json!({"code": {"language": "rust"}}),
                "fn main() {}",
                12,
                "/v1/artifacts/art_001/download",
            ))),
            format_stream_event(&StreamEvent::ArtifactUpdated(
                ArtifactUpdatedEvent::new("art_001", 2, "fn main() { println!(\"Hi\"); }")
                    .with_change_description("Added print statement"),
            )),
            format_stream_event(&StreamEvent::ArtifactDeleted(ArtifactDeletedEvent::new(
                "art_001",
            ))),
        ];

        assert!(events[0].starts_with("event: artifact_created\n"));
        assert!(events[1].starts_with("event: artifact_updated\n"));
        assert!(events[2].starts_with("event: artifact_deleted\n"));

        for event in &events {
            assert!(event.ends_with("\n\n"));
        }
    }
}
