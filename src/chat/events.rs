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
    /// Task completion signal.
    Finish,
}

impl fmt::Display for StreamEventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StreamEventType::Thought => write!(f, "thought"),
            StreamEventType::ToolCall => write!(f, "tool_call"),
            StreamEventType::ToolResult => write!(f, "tool_result"),
            StreamEventType::Text => write!(f, "text"),
            StreamEventType::Status => write!(f, "status"),
            StreamEventType::Finish => write!(f, "finish"),
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
    pub fn planning(message: impl Into<String>) -> Self {
        Self::new(ExecutionPhase::Planning, message)
    }

    /// Creates an executing phase status event.
    pub fn executing(message: impl Into<String>) -> Self {
        Self::new(ExecutionPhase::Executing, message)
    }

    /// Creates a reflecting phase status event.
    pub fn reflecting(message: impl Into<String>) -> Self {
        Self::new(ExecutionPhase::Reflecting, message)
    }

    /// Creates a completing phase status event.
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
    pub fn success(usage: TokenUsage) -> Self {
        Self {
            stop_reason: "stop".to_string(),
            usage,
            error: None,
            summary: None,
        }
    }

    /// Creates a new error FinishEvent.
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
    /// Finish event.
    Finish(FinishEvent),
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
            StreamEvent::Finish(_) => StreamEventType::Finish,
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
}

impl EnhancedStreamConfig {
    /// Creates a new config with all events enabled.
    pub fn all_enabled() -> Self {
        Self {
            enabled: true,
            include_thoughts: true,
            include_tool_calls: true,
            include_status: true,
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
    /// - `X-Enhanced-Stream: thoughts,tool_calls` - Enable specific events
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
        };

        for part in value.split(',') {
            match part.trim() {
                "thoughts" | "thought" => config.include_thoughts = true,
                "tool_calls" | "tools" => config.include_tool_calls = true,
                "status" => config.include_status = true,
                "all" => {
                    config.include_thoughts = true;
                    config.include_tool_calls = true;
                    config.include_status = true;
                }
                _ => {}
            }
        }

        // If no specific events are enabled, enable all
        if !config.include_thoughts && !config.include_tool_calls && !config.include_status {
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
        StreamEvent::Finish(e) => format_sse_event(&event_type, e),
    }
}

/// Formats the SSE stream termination signal.
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
}
