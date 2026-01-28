//! Event emitter abstraction for enhanced SSE streaming.
//!
//! This module provides the `EventEmitter` trait and its implementations
//! for emitting structured events during Agent execution.
//!
//! # Implementations
//!
//! - `SseEventEmitter`: Emits events to an SSE stream via a channel
//! - `NoopEventEmitter`: Does nothing (for non-enhanced mode)
//!
//! # Usage
//!
//! ```ignore
//! // Create an emitter based on configuration
//! let (emitter, receiver) = if config.enabled {
//!     let (tx, rx) = tokio::sync::mpsc::channel(100);
//!     (Box::new(SseEventEmitter::new(tx)) as Box<dyn EventEmitter>, Some(rx))
//! } else {
//!     (Box::new(NoopEventEmitter) as Box<dyn EventEmitter>, None)
//! };
//!
//! // Emit events during execution
//! emitter.emit_thought("I need to search for information.", ThoughtStatus::Done).await;
//! emitter.emit_tool_call("call_123", "search", &args).await;
//! ```

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use tokio::sync::mpsc;

use super::{
    events::{
        ArtifactCreatedEvent, ArtifactDeletedEvent, ArtifactUpdatedEvent, EnhancedStreamConfig,
        ExecutionPhase, ExecutionSummary, FinishEvent, HitlRequestEvent, HitlStatusEvent,
        HitlTimeoutWarningEvent, StatusEvent, StreamEvent, SubAgentCompletedEvent,
        SubAgentFailedEvent, SubAgentProgressEvent, SubAgentSpawnedEvent, SubAgentStartedEvent,
        SubAgentThoughtEvent, SubAgentToolCallEvent, TextEvent, ThoughtEvent, ThoughtStatus,
        ToolCallEvent, ToolResultEvent, format_stream_event,
    },
    trace::TokenUsage,
};

// ============================================================================
// EventEmitter Trait
// ============================================================================

/// Trait for emitting structured events during Agent execution.
///
/// This trait abstracts the event emission mechanism, allowing different
/// implementations for enhanced streaming mode and standard mode.
#[async_trait]
pub trait EventEmitter: Send + Sync {
    /// Emits a thought event (Agent's reasoning process).
    async fn emit_thought(
        &self,
        content: &str,
        status: ThoughtStatus,
        subtask_id: Option<usize>,
        iteration: Option<u32>,
    );

    /// Emits a tool call event (before tool execution).
    async fn emit_tool_call(
        &self,
        call_id: &str,
        tool_name: &str,
        args: &serde_json::Value,
        server_name: Option<&str>,
        subtask_id: Option<usize>,
    );

    /// Emits a tool result event (after tool execution).
    async fn emit_tool_result(
        &self,
        call_id: &str,
        result: &str,
        is_error: bool,
        duration: Option<Duration>,
        subtask_id: Option<usize>,
    );

    /// Emits a status event (macro-level execution status).
    async fn emit_status(
        &self,
        phase: ExecutionPhase,
        message: &str,
        subtask_id: Option<usize>,
        subtask_current: Option<usize>,
        subtask_total: Option<usize>,
    );

    /// Emits a text event (final response text chunk).
    #[allow(dead_code)]
    async fn emit_text(&self, content: &str);

    /// Emits a finish event (task completion signal).
    async fn emit_finish(
        &self,
        usage: &TokenUsage,
        stop_reason: &str,
        error: Option<&str>,
        summary: Option<ExecutionSummary>,
    );

    /// Emits an artifact created event.
    #[allow(dead_code, clippy::too_many_arguments)]
    async fn emit_artifact_created(
        &self,
        artifact_id: &str,
        title: &str,
        artifact_type: &serde_json::Value,
        content: &str,
        size: u64,
        url: &str,
        subtask_id: Option<usize>,
    );

    /// Emits an artifact updated event.
    #[allow(dead_code)]
    async fn emit_artifact_updated(
        &self,
        artifact_id: &str,
        version: i32,
        content: &str,
        change_description: Option<&str>,
        subtask_id: Option<usize>,
    );

    /// Emits an artifact deleted event.
    #[allow(dead_code)]
    async fn emit_artifact_deleted(&self, artifact_id: &str, subtask_id: Option<usize>);

    // ========================================================================
    // Sub-Agent Events
    // ========================================================================

    /// Emits a Sub-Agent spawned event.
    async fn emit_subagent_spawned(
        &self,
        subagent_id: &str,
        name: &str,
        task: &str,
        parent_id: Option<&str>,
        depth: u32,
    );

    /// Emits a Sub-Agent started event.
    async fn emit_subagent_started(&self, subagent_id: &str, name: &str);

    /// Emits a Sub-Agent progress event.
    async fn emit_subagent_progress(
        &self,
        subagent_id: &str,
        iteration: u32,
        max_iterations: Option<u32>,
        message: Option<&str>,
        tool_name: Option<&str>,
    );

    /// Emits a Sub-Agent thought event.
    async fn emit_subagent_thought(
        &self,
        subagent_id: &str,
        content: &str,
        iteration: Option<u32>,
        status: ThoughtStatus,
    );

    /// Emits a Sub-Agent tool call event.
    async fn emit_subagent_tool_call(
        &self,
        subagent_id: &str,
        tool_call_id: &str,
        tool_name: &str,
        args: &serde_json::Value,
        iteration: Option<u32>,
    );

    /// Emits a Sub-Agent completed event.
    async fn emit_subagent_completed(
        &self,
        subagent_id: &str,
        name: &str,
        output: &str,
        iterations: u32,
        duration_ms: u64,
    );

    /// Emits a Sub-Agent failed event.
    async fn emit_subagent_failed(
        &self,
        subagent_id: &str,
        name: &str,
        error: &str,
        iterations: u32,
        duration_ms: u64,
    );

    // ========================================================================
    // HITL Events
    // ========================================================================

    /// Emits a HITL request event (awaiting user response).
    #[allow(clippy::too_many_arguments)]
    async fn emit_hitl_request(
        &self,
        request_id: &str,
        request_type: &str,
        summary: &str,
        risk_level: Option<&str>,
        tool_name: Option<&str>,
        conversation_id: &str,
        user_id: &str,
        expires_at: &str,
        remaining_seconds: i64,
        timeout_behavior: &str,
        subtask_id: Option<usize>,
        subagent_id: Option<&str>,
    );

    /// Emits a HITL status change event.
    async fn emit_hitl_status(&self, request_id: &str, status: &str, message: &str);

    /// Emits a HITL timeout warning event.
    async fn emit_hitl_timeout_warning(&self, request_id: &str, remaining_seconds: u64);

    /// Returns whether this emitter is active (will actually emit events).
    #[allow(dead_code)]
    fn is_active(&self) -> bool;
}

// ============================================================================
// SseEventEmitter Implementation
// ============================================================================

/// Event emitter that sends events through a channel for SSE streaming.
///
/// This implementation is used when enhanced streaming mode is enabled.
/// Events are serialized to SSE format and sent through the provided channel.
pub struct SseEventEmitter {
    /// Channel sender for emitting SSE-formatted events.
    sender: mpsc::Sender<String>,
    /// Configuration for which events to emit.
    config: EnhancedStreamConfig,
}

impl SseEventEmitter {
    /// Creates a new SseEventEmitter with the given sender and configuration.
    pub fn new(sender: mpsc::Sender<String>, config: EnhancedStreamConfig) -> Self {
        Self { sender, config }
    }

    /// Sends an event through the channel.
    async fn send_event(&self, event: StreamEvent) {
        let sse_message = format_stream_event(&event);
        // Ignore send errors (receiver may have been dropped)
        let _ = self.sender.send(sse_message).await;
    }
}

#[async_trait]
impl EventEmitter for SseEventEmitter {
    async fn emit_thought(
        &self,
        content: &str,
        status: ThoughtStatus,
        subtask_id: Option<usize>,
        iteration: Option<u32>,
    ) {
        if !self.config.should_emit_thought() {
            return;
        }

        let mut event = ThoughtEvent::new(content).with_status(status);
        if let Some(id) = subtask_id {
            event = event.with_subtask_id(id);
        }
        if let Some(iter) = iteration {
            event = event.with_iteration(iter);
        }

        self.send_event(StreamEvent::Thought(event)).await;
    }

    async fn emit_tool_call(
        &self,
        call_id: &str,
        tool_name: &str,
        args: &serde_json::Value,
        server_name: Option<&str>,
        subtask_id: Option<usize>,
    ) {
        if !self.config.should_emit_tool_events() {
            return;
        }

        let mut event = ToolCallEvent::new(call_id, tool_name, args.clone());
        if let Some(server) = server_name {
            event = event.with_server_name(server);
        }
        if let Some(id) = subtask_id {
            event = event.with_subtask_id(id);
        }

        self.send_event(StreamEvent::ToolCall(event)).await;
    }

    async fn emit_tool_result(
        &self,
        call_id: &str,
        result: &str,
        is_error: bool,
        duration: Option<Duration>,
        subtask_id: Option<usize>,
    ) {
        if !self.config.should_emit_tool_events() {
            return;
        }

        let mut event = if is_error {
            ToolResultEvent::error(call_id, result)
        } else {
            ToolResultEvent::success(call_id, result)
        };

        if let Some(dur) = duration {
            event = event.with_duration_ms(dur.as_millis() as u64);
        }
        if let Some(id) = subtask_id {
            event = event.with_subtask_id(id);
        }

        self.send_event(StreamEvent::ToolResult(event)).await;
    }

    async fn emit_status(
        &self,
        phase: ExecutionPhase,
        message: &str,
        subtask_id: Option<usize>,
        subtask_current: Option<usize>,
        subtask_total: Option<usize>,
    ) {
        if !self.config.should_emit_status() {
            return;
        }

        let mut event = StatusEvent::new(phase, message);
        if let Some(id) = subtask_id {
            if let (Some(current), Some(total)) = (subtask_current, subtask_total) {
                event = event.with_subtask_progress(id, current, total);
            } else {
                event.subtask_id = Some(id);
            }
        }

        self.send_event(StreamEvent::Status(event)).await;
    }

    async fn emit_text(&self, content: &str) {
        // Text events are always emitted in enhanced mode
        if !self.config.enabled {
            return;
        }

        let event = TextEvent::new(content);
        self.send_event(StreamEvent::Text(event)).await;
    }

    async fn emit_finish(
        &self,
        usage: &TokenUsage,
        stop_reason: &str,
        error: Option<&str>,
        summary: Option<ExecutionSummary>,
    ) {
        // Finish events are always emitted in enhanced mode
        if !self.config.enabled {
            return;
        }

        let mut event = if let Some(err) = error {
            FinishEvent::error(usage.clone(), err)
        } else {
            FinishEvent::with_reason(stop_reason, usage.clone())
        };

        if let Some(sum) = summary {
            event = event.with_summary(sum);
        }

        self.send_event(StreamEvent::Finish(event)).await;
    }

    async fn emit_artifact_created(
        &self,
        artifact_id: &str,
        title: &str,
        artifact_type: &serde_json::Value,
        content: &str,
        size: u64,
        url: &str,
        subtask_id: Option<usize>,
    ) {
        if !self.config.should_emit_artifacts() {
            return;
        }

        let mut event = ArtifactCreatedEvent::new(
            artifact_id,
            title,
            artifact_type.clone(),
            content,
            size,
            url,
        );
        if let Some(id) = subtask_id {
            event = event.with_subtask_id(id);
        }

        self.send_event(StreamEvent::ArtifactCreated(event)).await;
    }

    async fn emit_artifact_updated(
        &self,
        artifact_id: &str,
        version: i32,
        content: &str,
        change_description: Option<&str>,
        subtask_id: Option<usize>,
    ) {
        if !self.config.should_emit_artifacts() {
            return;
        }

        let mut event = ArtifactUpdatedEvent::new(artifact_id, version, content);
        if let Some(desc) = change_description {
            event = event.with_change_description(desc);
        }
        if let Some(id) = subtask_id {
            event = event.with_subtask_id(id);
        }

        self.send_event(StreamEvent::ArtifactUpdated(event)).await;
    }

    async fn emit_artifact_deleted(&self, artifact_id: &str, subtask_id: Option<usize>) {
        if !self.config.should_emit_artifacts() {
            return;
        }

        let mut event = ArtifactDeletedEvent::new(artifact_id);
        if let Some(id) = subtask_id {
            event = event.with_subtask_id(id);
        }

        self.send_event(StreamEvent::ArtifactDeleted(event)).await;
    }

    // ========================================================================
    // Sub-Agent Events
    // ========================================================================

    async fn emit_subagent_spawned(
        &self,
        subagent_id: &str,
        name: &str,
        task: &str,
        parent_id: Option<&str>,
        depth: u32,
    ) {
        if !self.config.should_emit_subagent_events() {
            return;
        }

        let mut event = SubAgentSpawnedEvent::new(subagent_id, name, task).with_depth(depth);
        if let Some(pid) = parent_id {
            event = event.with_parent(pid);
        }

        self.send_event(StreamEvent::SubAgentSpawned(event)).await;
    }

    async fn emit_subagent_started(&self, subagent_id: &str, name: &str) {
        if !self.config.should_emit_subagent_events() {
            return;
        }

        let event = SubAgentStartedEvent::new(subagent_id, name);
        self.send_event(StreamEvent::SubAgentStarted(event)).await;
    }

    async fn emit_subagent_progress(
        &self,
        subagent_id: &str,
        iteration: u32,
        max_iterations: Option<u32>,
        message: Option<&str>,
        tool_name: Option<&str>,
    ) {
        if !self.config.should_emit_subagent_events() {
            return;
        }

        let mut event = SubAgentProgressEvent::new(subagent_id, iteration);
        if let Some(max) = max_iterations {
            event = event.with_max_iterations(max);
        }
        if let Some(msg) = message {
            event = event.with_message(msg);
        }
        if let Some(name) = tool_name {
            event = event.with_tool_name(name);
        }

        self.send_event(StreamEvent::SubAgentProgress(event)).await;
    }

    async fn emit_subagent_thought(
        &self,
        subagent_id: &str,
        content: &str,
        iteration: Option<u32>,
        status: ThoughtStatus,
    ) {
        if !self.config.should_emit_subagent_events() {
            return;
        }

        let mut event = SubAgentThoughtEvent::new(subagent_id, content).with_status(status);
        if let Some(iter) = iteration {
            event = event.with_iteration(iter);
        }

        self.send_event(StreamEvent::SubAgentThought(event)).await;
    }

    async fn emit_subagent_tool_call(
        &self,
        subagent_id: &str,
        tool_call_id: &str,
        tool_name: &str,
        args: &serde_json::Value,
        iteration: Option<u32>,
    ) {
        if !self.config.should_emit_subagent_events() {
            return;
        }

        let mut event =
            SubAgentToolCallEvent::new(subagent_id, tool_call_id, tool_name, args.clone());
        if let Some(iter) = iteration {
            event = event.with_iteration(iter);
        }

        self.send_event(StreamEvent::SubAgentToolCall(event)).await;
    }

    async fn emit_subagent_completed(
        &self,
        subagent_id: &str,
        name: &str,
        output: &str,
        iterations: u32,
        duration_ms: u64,
    ) {
        if !self.config.should_emit_subagent_events() {
            return;
        }

        let event = SubAgentCompletedEvent::new(subagent_id, name, output, iterations, duration_ms);
        self.send_event(StreamEvent::SubAgentCompleted(event)).await;
    }

    async fn emit_subagent_failed(
        &self,
        subagent_id: &str,
        name: &str,
        error: &str,
        iterations: u32,
        duration_ms: u64,
    ) {
        if !self.config.should_emit_subagent_events() {
            return;
        }

        let event = SubAgentFailedEvent::new(subagent_id, name, error, iterations, duration_ms);
        self.send_event(StreamEvent::SubAgentFailed(event)).await;
    }

    async fn emit_hitl_request(
        &self,
        request_id: &str,
        request_type: &str,
        summary: &str,
        risk_level: Option<&str>,
        tool_name: Option<&str>,
        conversation_id: &str,
        user_id: &str,
        expires_at: &str,
        remaining_seconds: i64,
        timeout_behavior: &str,
        subtask_id: Option<usize>,
        subagent_id: Option<&str>,
    ) {
        if !self.config.should_emit_hitl() {
            return;
        }

        let mut event = HitlRequestEvent::new(
            request_id,
            request_type,
            summary,
            conversation_id,
            user_id,
            expires_at,
            remaining_seconds,
            timeout_behavior,
        );

        if let Some(level) = risk_level {
            event = event.with_risk_level(level);
        }
        if let Some(tool) = tool_name {
            event = event.with_tool_name(tool);
        }
        if let Some(id) = subtask_id {
            event = event.with_subtask_id(id);
        }
        if let Some(agent_id) = subagent_id {
            event = event.with_subagent_id(agent_id);
        }

        self.send_event(StreamEvent::HitlRequest(event)).await;
    }

    async fn emit_hitl_status(&self, request_id: &str, status: &str, message: &str) {
        if !self.config.should_emit_hitl() {
            return;
        }

        let event = HitlStatusEvent::new(request_id, status, message);
        self.send_event(StreamEvent::HitlStatus(event)).await;
    }

    async fn emit_hitl_timeout_warning(&self, request_id: &str, remaining_seconds: u64) {
        if !self.config.should_emit_hitl() {
            return;
        }

        let event = HitlTimeoutWarningEvent::new(request_id, remaining_seconds);
        self.send_event(StreamEvent::HitlTimeoutWarning(event))
            .await;
    }

    fn is_active(&self) -> bool {
        true
    }
}

// ============================================================================
// NoopEventEmitter Implementation
// ============================================================================

/// Event emitter that does nothing.
///
/// This implementation is used when enhanced streaming mode is disabled,
/// avoiding the overhead of event serialization and channel operations.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopEventEmitter;

impl NoopEventEmitter {
    /// Creates a new NoopEventEmitter.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl EventEmitter for NoopEventEmitter {
    async fn emit_thought(
        &self,
        _content: &str,
        _status: ThoughtStatus,
        _subtask_id: Option<usize>,
        _iteration: Option<u32>,
    ) {
        // No-op
    }

    async fn emit_tool_call(
        &self,
        _call_id: &str,
        _tool_name: &str,
        _args: &serde_json::Value,
        _server_name: Option<&str>,
        _subtask_id: Option<usize>,
    ) {
        // No-op
    }

    async fn emit_tool_result(
        &self,
        _call_id: &str,
        _result: &str,
        _is_error: bool,
        _duration: Option<Duration>,
        _subtask_id: Option<usize>,
    ) {
        // No-op
    }

    async fn emit_status(
        &self,
        _phase: ExecutionPhase,
        _message: &str,
        _subtask_id: Option<usize>,
        _subtask_current: Option<usize>,
        _subtask_total: Option<usize>,
    ) {
        // No-op
    }

    async fn emit_text(&self, _content: &str) {
        // No-op
    }

    async fn emit_finish(
        &self,
        _usage: &TokenUsage,
        _stop_reason: &str,
        _error: Option<&str>,
        _summary: Option<ExecutionSummary>,
    ) {
        // No-op
    }

    async fn emit_artifact_created(
        &self,
        _artifact_id: &str,
        _title: &str,
        _artifact_type: &serde_json::Value,
        _content: &str,
        _size: u64,
        _url: &str,
        _subtask_id: Option<usize>,
    ) {
        // No-op
    }

    async fn emit_artifact_updated(
        &self,
        _artifact_id: &str,
        _version: i32,
        _content: &str,
        _change_description: Option<&str>,
        _subtask_id: Option<usize>,
    ) {
        // No-op
    }

    async fn emit_artifact_deleted(&self, _artifact_id: &str, _subtask_id: Option<usize>) {
        // No-op
    }

    // ========================================================================
    // Sub-Agent Events
    // ========================================================================

    async fn emit_subagent_spawned(
        &self,
        _subagent_id: &str,
        _name: &str,
        _task: &str,
        _parent_id: Option<&str>,
        _depth: u32,
    ) {
        // No-op
    }

    async fn emit_subagent_started(&self, _subagent_id: &str, _name: &str) {
        // No-op
    }

    async fn emit_subagent_progress(
        &self,
        _subagent_id: &str,
        _iteration: u32,
        _max_iterations: Option<u32>,
        _message: Option<&str>,
        _tool_name: Option<&str>,
    ) {
        // No-op
    }

    async fn emit_subagent_thought(
        &self,
        _subagent_id: &str,
        _content: &str,
        _iteration: Option<u32>,
        _status: ThoughtStatus,
    ) {
        // No-op
    }

    async fn emit_subagent_tool_call(
        &self,
        _subagent_id: &str,
        _tool_call_id: &str,
        _tool_name: &str,
        _args: &serde_json::Value,
        _iteration: Option<u32>,
    ) {
        // No-op
    }

    async fn emit_subagent_completed(
        &self,
        _subagent_id: &str,
        _name: &str,
        _output: &str,
        _iterations: u32,
        _duration_ms: u64,
    ) {
        // No-op
    }

    async fn emit_subagent_failed(
        &self,
        _subagent_id: &str,
        _name: &str,
        _error: &str,
        _iterations: u32,
        _duration_ms: u64,
    ) {
        // No-op
    }

    async fn emit_hitl_request(
        &self,
        _request_id: &str,
        _request_type: &str,
        _summary: &str,
        _risk_level: Option<&str>,
        _tool_name: Option<&str>,
        _conversation_id: &str,
        _user_id: &str,
        _expires_at: &str,
        _remaining_seconds: i64,
        _timeout_behavior: &str,
        _subtask_id: Option<usize>,
        _subagent_id: Option<&str>,
    ) {
        // No-op
    }

    async fn emit_hitl_status(&self, _request_id: &str, _status: &str, _message: &str) {
        // No-op
    }

    async fn emit_hitl_timeout_warning(&self, _request_id: &str, _remaining_seconds: u64) {
        // No-op
    }

    fn is_active(&self) -> bool {
        false
    }
}

// ============================================================================
// Factory Functions
// ============================================================================

/// Creates an appropriate event emitter based on the configuration.
///
/// Returns a tuple of (emitter, optional receiver).
/// - If enhanced streaming is enabled, returns an `SseEventEmitter` with a receiver
/// - If disabled, returns a `NoopEventEmitter` with no receiver
///
/// The emitter is wrapped in `Arc` to allow sharing across parallel subtask executions.
///
/// If HITL is enabled globally, also spawns a HitlNotifier to bridge HITL events to the SSE stream.
pub fn create_emitter(
    config: &EnhancedStreamConfig,
    channel_capacity: usize,
) -> (Arc<dyn EventEmitter>, Option<mpsc::Receiver<String>>) {
    if config.enabled {
        let (tx, rx) = mpsc::channel(channel_capacity);
        let emitter: Arc<dyn EventEmitter> = Arc::new(SseEventEmitter::new(tx, config.clone()));

        // Spawn HitlNotifier if HITL is enabled
        if let Some(hitl_manager) = crate::services::hitl::global() {
            crate::services::hitl::HitlNotifier::spawn(
                std::sync::Arc::clone(hitl_manager),
                emitter.clone(),
            );
            tracing::debug!("HitlNotifier spawned for SSE connection");
        }

        (emitter, Some(rx))
    } else {
        (Arc::new(NoopEventEmitter::new()), None)
    }
}

/// Default channel capacity for event streaming.
#[allow(dead_code)]
pub const DEFAULT_CHANNEL_CAPACITY: usize = 100;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_noop_emitter_is_inactive() {
        let emitter = NoopEventEmitter::new();
        assert!(!emitter.is_active());
    }

    #[tokio::test]
    async fn test_noop_emitter_does_nothing() {
        let emitter = NoopEventEmitter::new();

        // These should all complete without error
        emitter
            .emit_thought("test", ThoughtStatus::Done, None, None)
            .await;
        emitter
            .emit_tool_call("id", "tool", &serde_json::json!({}), None, None)
            .await;
        emitter
            .emit_tool_result("id", "result", false, None, None)
            .await;
        emitter
            .emit_status(ExecutionPhase::Planning, "test", None, None, None)
            .await;
        emitter.emit_text("test").await;
        emitter
            .emit_finish(&TokenUsage::default(), "stop", None, None)
            .await;
    }

    #[tokio::test]
    async fn test_sse_emitter_is_active() {
        let (tx, _rx) = mpsc::channel(10);
        let emitter = SseEventEmitter::new(tx, EnhancedStreamConfig::all_enabled());
        assert!(emitter.is_active());
    }

    #[tokio::test]
    async fn test_sse_emitter_sends_thought() {
        let (tx, mut rx) = mpsc::channel(10);
        let emitter = SseEventEmitter::new(tx, EnhancedStreamConfig::all_enabled());

        emitter
            .emit_thought("I'm thinking...", ThoughtStatus::Done, Some(1), Some(2))
            .await;

        let message = rx.recv().await.unwrap();
        assert!(message.starts_with("event: thought\n"));
        assert!(message.contains("\"content\":\"I'm thinking...\""));
        assert!(message.contains("\"status\":\"done\""));
        assert!(message.contains("\"subtask_id\":1"));
        assert!(message.contains("\"iteration\":2"));
    }

    #[tokio::test]
    async fn test_sse_emitter_sends_tool_call() {
        let (tx, mut rx) = mpsc::channel(10);
        let emitter = SseEventEmitter::new(tx, EnhancedStreamConfig::all_enabled());

        emitter
            .emit_tool_call(
                "call_123",
                "get_weather",
                &serde_json::json!({"city": "Shanghai"}),
                Some("weather-server"),
                Some(1),
            )
            .await;

        let message = rx.recv().await.unwrap();
        assert!(message.starts_with("event: tool_call\n"));
        assert!(message.contains("\"tool_call_id\":\"call_123\""));
        assert!(message.contains("\"tool_name\":\"get_weather\""));
        assert!(message.contains("\"server_name\":\"weather-server\""));
    }

    #[tokio::test]
    async fn test_sse_emitter_sends_tool_result_success() {
        let (tx, mut rx) = mpsc::channel(10);
        let emitter = SseEventEmitter::new(tx, EnhancedStreamConfig::all_enabled());

        emitter
            .emit_tool_result(
                "call_123",
                r#"{"temp": 25}"#,
                false,
                Some(Duration::from_millis(150)),
                None,
            )
            .await;

        let message = rx.recv().await.unwrap();
        assert!(message.starts_with("event: tool_result\n"));
        assert!(message.contains("\"tool_call_id\":\"call_123\""));
        assert!(message.contains("\"is_error\":false"));
        assert!(message.contains("\"duration_ms\":150"));
    }

    #[tokio::test]
    async fn test_sse_emitter_sends_tool_result_error() {
        let (tx, mut rx) = mpsc::channel(10);
        let emitter = SseEventEmitter::new(tx, EnhancedStreamConfig::all_enabled());

        emitter
            .emit_tool_result("call_456", "Connection timeout", true, None, None)
            .await;

        let message = rx.recv().await.unwrap();
        assert!(message.contains("\"is_error\":true"));
        assert!(message.contains("\"result\":\"Connection timeout\""));
    }

    #[tokio::test]
    async fn test_sse_emitter_sends_status() {
        let (tx, mut rx) = mpsc::channel(10);
        let emitter = SseEventEmitter::new(tx, EnhancedStreamConfig::all_enabled());

        emitter
            .emit_status(
                ExecutionPhase::Executing,
                "Processing subtask...",
                Some(2),
                Some(2),
                Some(5),
            )
            .await;

        let message = rx.recv().await.unwrap();
        assert!(message.starts_with("event: status\n"));
        assert!(message.contains("\"phase\":\"executing\""));
        assert!(message.contains("\"subtask_id\":2"));
        assert!(message.contains("\"subtask_current\":2"));
        assert!(message.contains("\"subtask_total\":5"));
    }

    #[tokio::test]
    async fn test_sse_emitter_sends_text() {
        let (tx, mut rx) = mpsc::channel(10);
        let emitter = SseEventEmitter::new(tx, EnhancedStreamConfig::all_enabled());

        emitter.emit_text("Hello, world!").await;

        let message = rx.recv().await.unwrap();
        assert!(message.starts_with("event: text\n"));
        assert!(message.contains("\"content\":\"Hello, world!\""));
    }

    #[tokio::test]
    async fn test_sse_emitter_sends_finish() {
        let (tx, mut rx) = mpsc::channel(10);
        let emitter = SseEventEmitter::new(tx, EnhancedStreamConfig::all_enabled());

        let usage = TokenUsage::new(100, 50);
        emitter.emit_finish(&usage, "stop", None, None).await;

        let message = rx.recv().await.unwrap();
        assert!(message.starts_with("event: finish\n"));
        assert!(message.contains("\"stop_reason\":\"stop\""));
        assert!(message.contains("\"total_tokens\":150"));
    }

    #[tokio::test]
    async fn test_sse_emitter_respects_config() {
        let (tx, mut rx) = mpsc::channel(10);

        // Create config with only thoughts enabled
        let config = EnhancedStreamConfig::parse("thoughts");
        let emitter = SseEventEmitter::new(tx, config);

        // Emit thought - should be sent
        emitter
            .emit_thought("test", ThoughtStatus::Done, None, None)
            .await;
        assert!(rx.try_recv().is_ok());

        // Emit tool_call - should NOT be sent
        emitter
            .emit_tool_call("id", "tool", &serde_json::json!({}), None, None)
            .await;
        assert!(rx.try_recv().is_err());

        // Emit status - should NOT be sent
        emitter
            .emit_status(ExecutionPhase::Planning, "test", None, None, None)
            .await;
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_create_emitter_enabled() {
        let config = EnhancedStreamConfig::all_enabled();
        let (emitter, receiver) = create_emitter(&config, 10);

        assert!(emitter.is_active());
        assert!(receiver.is_some());
    }

    #[tokio::test]
    async fn test_create_emitter_disabled() {
        let config = EnhancedStreamConfig::disabled();
        let (emitter, receiver) = create_emitter(&config, 10);

        assert!(!emitter.is_active());
        assert!(receiver.is_none());
    }

    #[tokio::test]
    async fn test_sse_emitter_handles_dropped_receiver() {
        let (tx, rx) = mpsc::channel(10);
        let emitter = SseEventEmitter::new(tx, EnhancedStreamConfig::all_enabled());

        // Drop the receiver
        drop(rx);

        // This should not panic
        emitter
            .emit_thought("test", ThoughtStatus::Done, None, None)
            .await;
    }

    // ========================================================================
    // Artifact Event Emitter Tests
    // ========================================================================

    #[tokio::test]
    async fn test_sse_emitter_sends_artifact_created() {
        let (tx, mut rx) = mpsc::channel(10);
        let emitter = SseEventEmitter::new(tx, EnhancedStreamConfig::all_enabled());

        emitter
            .emit_artifact_created(
                "art_123",
                "main.rs",
                &serde_json::json!({"code": {"language": "rust"}}),
                "fn main() {}",
                12,
                "/v1/artifacts/art_123/download",
                Some(1),
            )
            .await;

        let message = rx.recv().await.unwrap();
        assert!(message.starts_with("event: artifact_created\n"));
        assert!(message.contains("\"artifact_id\":\"art_123\""));
        assert!(message.contains("\"title\":\"main.rs\""));
        assert!(message.contains("\"size\":12"));
        assert!(message.contains("\"subtask_id\":1"));
    }

    #[tokio::test]
    async fn test_sse_emitter_sends_artifact_updated() {
        let (tx, mut rx) = mpsc::channel(10);
        let emitter = SseEventEmitter::new(tx, EnhancedStreamConfig::all_enabled());

        emitter
            .emit_artifact_updated(
                "art_123",
                2,
                "fn main() { println!(\"Hi\"); }",
                Some("Added print statement"),
                Some(1),
            )
            .await;

        let message = rx.recv().await.unwrap();
        assert!(message.starts_with("event: artifact_updated\n"));
        assert!(message.contains("\"artifact_id\":\"art_123\""));
        assert!(message.contains("\"version\":2"));
        assert!(message.contains("\"change_description\":\"Added print statement\""));
    }

    #[tokio::test]
    async fn test_sse_emitter_sends_artifact_deleted() {
        let (tx, mut rx) = mpsc::channel(10);
        let emitter = SseEventEmitter::new(tx, EnhancedStreamConfig::all_enabled());

        emitter.emit_artifact_deleted("art_123", Some(2)).await;

        let message = rx.recv().await.unwrap();
        assert!(message.starts_with("event: artifact_deleted\n"));
        assert!(message.contains("\"artifact_id\":\"art_123\""));
        assert!(message.contains("\"subtask_id\":2"));
    }

    #[tokio::test]
    async fn test_sse_emitter_respects_artifacts_config() {
        let (tx, mut rx) = mpsc::channel(10);

        // Create config with only artifacts enabled
        let config = EnhancedStreamConfig::parse("artifacts");
        let emitter = SseEventEmitter::new(tx, config);

        // Emit artifact_created - should be sent
        emitter
            .emit_artifact_created(
                "art_1",
                "test.txt",
                &serde_json::json!("text"),
                "content",
                7,
                "/download",
                None,
            )
            .await;
        assert!(rx.try_recv().is_ok());

        // Emit thought - should NOT be sent
        emitter
            .emit_thought("test", ThoughtStatus::Done, None, None)
            .await;
        assert!(rx.try_recv().is_err());

        // Emit tool_call - should NOT be sent
        emitter
            .emit_tool_call("id", "tool", &serde_json::json!({}), None, None)
            .await;
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_noop_emitter_ignores_artifact_events() {
        let emitter = NoopEventEmitter::new();

        // These should all complete without error
        emitter
            .emit_artifact_created(
                "art_1",
                "test.txt",
                &serde_json::json!("text"),
                "content",
                7,
                "/download",
                None,
            )
            .await;
        emitter
            .emit_artifact_updated("art_1", 2, "new content", None, None)
            .await;
        emitter.emit_artifact_deleted("art_1", None).await;
    }
}
