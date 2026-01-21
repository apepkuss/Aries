//! Sub-Agent 通信通道
//!
//! 此模块实现 Sub-Agent 与主 Agent 之间的双向通信机制。
//! 支持运行时消息传递，允许动态任务调整和数据共享。

use std::{collections::HashMap, time::Duration};

use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, warn};

use super::types::SubAgentId;

// ============================================================================
// Message Types
// ============================================================================

/// 通信消息类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChannelMessage {
    /// 文本消息
    Text {
        /// 消息内容
        content: String,
    },
    /// 指令消息
    Instruction {
        /// 指令类型
        instruction_type: InstructionType,
        /// 指令内容
        content: String,
    },
    /// 数据消息
    Data {
        /// 数据键
        key: String,
        /// 数据值
        value: serde_json::Value,
    },
    /// 请求确认消息
    ConfirmRequest {
        /// 请求 ID
        request_id: String,
        /// 确认问题
        question: String,
        /// 可选选项
        options: Option<Vec<String>>,
    },
    /// 确认响应消息
    ConfirmResponse {
        /// 请求 ID
        request_id: String,
        /// 是否确认
        confirmed: bool,
        /// 选择的选项（如果有）
        selected_option: Option<String>,
    },
    /// 进度报告
    Progress {
        /// 进度百分比 (0-100)
        percentage: u8,
        /// 进度描述
        description: String,
    },
    /// 错误消息
    Error {
        /// 错误代码
        code: String,
        /// 错误描述
        message: String,
    },
}

/// 指令类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstructionType {
    /// 修改任务
    ModifyTask,
    /// 暂停执行
    Pause,
    /// 恢复执行
    Resume,
    /// 停止执行
    Stop,
    /// 优先级调整
    PriorityChange,
    /// 自定义指令
    Custom,
}

/// 消息信封（包含元数据）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope {
    /// 消息 ID
    pub id: String,
    /// 发送者 ID
    pub sender_id: String,
    /// 接收者 ID
    pub receiver_id: String,
    /// 消息内容
    pub message: ChannelMessage,
    /// 时间戳（毫秒）
    pub timestamp_ms: u64,
    /// 是否需要确认
    pub requires_ack: bool,
}

impl MessageEnvelope {
    /// 创建新的消息信封
    pub fn new(
        sender_id: impl Into<String>,
        receiver_id: impl Into<String>,
        message: ChannelMessage,
    ) -> Self {
        Self {
            id: generate_message_id(),
            sender_id: sender_id.into(),
            receiver_id: receiver_id.into(),
            message,
            timestamp_ms: current_timestamp_ms(),
            requires_ack: false,
        }
    }

    /// 设置需要确认
    pub fn with_ack(mut self) -> Self {
        self.requires_ack = true;
        self
    }

    /// 创建文本消息
    pub fn text(
        sender_id: impl Into<String>,
        receiver_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self::new(
            sender_id,
            receiver_id,
            ChannelMessage::Text {
                content: content.into(),
            },
        )
    }

    /// 创建指令消息
    pub fn instruction(
        sender_id: impl Into<String>,
        receiver_id: impl Into<String>,
        instruction_type: InstructionType,
        content: impl Into<String>,
    ) -> Self {
        Self::new(
            sender_id,
            receiver_id,
            ChannelMessage::Instruction {
                instruction_type,
                content: content.into(),
            },
        )
    }

    /// 创建数据消息
    pub fn data(
        sender_id: impl Into<String>,
        receiver_id: impl Into<String>,
        key: impl Into<String>,
        value: serde_json::Value,
    ) -> Self {
        Self::new(
            sender_id,
            receiver_id,
            ChannelMessage::Data {
                key: key.into(),
                value,
            },
        )
    }
}

// ============================================================================
// SubAgentChannel
// ============================================================================

/// Sub-Agent 通信通道
///
/// 每个 Sub-Agent 拥有一个通道实例，用于与主 Agent 或父 Sub-Agent 通信。
pub struct SubAgentChannel {
    /// Sub-Agent ID
    subagent_id: SubAgentId,
    /// 父 Agent ID（"main" 表示主 Agent）
    parent_id: String,
    /// 发送到父 Agent 的通道
    to_parent: mpsc::Sender<MessageEnvelope>,
    /// 从父 Agent 接收的通道
    from_parent: mpsc::Receiver<MessageEnvelope>,
    /// 消息队列容量
    #[allow(dead_code)]
    capacity: usize,
}

impl SubAgentChannel {
    /// 创建新的通道
    pub fn new(
        subagent_id: SubAgentId,
        parent_id: impl Into<String>,
        to_parent: mpsc::Sender<MessageEnvelope>,
        from_parent: mpsc::Receiver<MessageEnvelope>,
        capacity: usize,
    ) -> Self {
        Self {
            subagent_id,
            parent_id: parent_id.into(),
            to_parent,
            from_parent,
            capacity,
        }
    }

    /// 获取 Sub-Agent ID
    pub fn subagent_id(&self) -> &SubAgentId {
        &self.subagent_id
    }

    /// 获取父 Agent ID
    pub fn parent_id(&self) -> &str {
        &self.parent_id
    }

    /// 发送消息到父 Agent
    pub async fn send_to_parent(&self, message: ChannelMessage) -> Result<(), ChannelError> {
        let envelope = MessageEnvelope::new(
            self.subagent_id.to_string(),
            self.parent_id.clone(),
            message,
        );

        self.to_parent
            .send(envelope)
            .await
            .map_err(|_| ChannelError::SendFailed {
                reason: "Parent channel closed".to_string(),
            })
    }

    /// 发送文本消息到父 Agent
    pub async fn send_text_to_parent(
        &self,
        content: impl Into<String>,
    ) -> Result<(), ChannelError> {
        self.send_to_parent(ChannelMessage::Text {
            content: content.into(),
        })
        .await
    }

    /// 发送进度报告到父 Agent
    pub async fn send_progress(
        &self,
        percentage: u8,
        description: impl Into<String>,
    ) -> Result<(), ChannelError> {
        self.send_to_parent(ChannelMessage::Progress {
            percentage: percentage.min(100),
            description: description.into(),
        })
        .await
    }

    /// 请求父 Agent 确认
    pub async fn request_confirmation(
        &self,
        question: impl Into<String>,
        options: Option<Vec<String>>,
    ) -> Result<String, ChannelError> {
        let request_id = generate_message_id();
        let envelope = MessageEnvelope::new(
            self.subagent_id.to_string(),
            self.parent_id.clone(),
            ChannelMessage::ConfirmRequest {
                request_id: request_id.clone(),
                question: question.into(),
                options,
            },
        )
        .with_ack();

        self.to_parent
            .send(envelope)
            .await
            .map_err(|_| ChannelError::SendFailed {
                reason: "Parent channel closed".to_string(),
            })?;

        Ok(request_id)
    }

    /// 从父 Agent 接收消息（非阻塞）
    pub async fn try_recv_from_parent(&mut self) -> Option<MessageEnvelope> {
        self.from_parent.try_recv().ok()
    }

    /// 从父 Agent 接收消息（阻塞）
    pub async fn recv_from_parent(&mut self) -> Option<MessageEnvelope> {
        self.from_parent.recv().await
    }

    /// 从父 Agent 接收消息（带超时）
    pub async fn recv_from_parent_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<MessageEnvelope, ChannelError> {
        tokio::time::timeout(timeout, self.from_parent.recv())
            .await
            .map_err(|_| ChannelError::Timeout)?
            .ok_or(ChannelError::ChannelClosed)
    }

    /// 检查是否有待处理的消息
    pub fn has_pending_messages(&self) -> bool {
        !self.from_parent.is_empty()
    }
}

// ============================================================================
// ChannelManager
// ============================================================================

/// 通道管理器
///
/// 管理所有 Sub-Agent 的通信通道，提供消息路由功能。
pub struct ChannelManager {
    /// 默认通道容量
    default_capacity: usize,
    /// Sub-Agent 到主 Agent 的发送端（由 Sub-Agent 持有的发送端对应的接收端）
    parent_receivers: RwLock<HashMap<SubAgentId, mpsc::Receiver<MessageEnvelope>>>,
    /// 主 Agent 到 Sub-Agent 的发送端
    child_senders: RwLock<HashMap<SubAgentId, mpsc::Sender<MessageEnvelope>>>,
    /// 待处理的确认请求
    pending_confirmations: RwLock<HashMap<String, PendingConfirmation>>,
}

/// 待处理的确认请求
struct PendingConfirmation {
    /// Sub-Agent ID
    #[allow(dead_code)]
    subagent_id: SubAgentId,
    /// 请求 ID
    #[allow(dead_code)]
    request_id: String,
    /// 响应发送端
    response_tx: tokio::sync::oneshot::Sender<ConfirmResponse>,
}

/// 确认响应
#[derive(Debug, Clone)]
pub struct ConfirmResponse {
    /// 是否确认
    pub confirmed: bool,
    /// 选择的选项
    pub selected_option: Option<String>,
}

impl Default for ChannelManager {
    fn default() -> Self {
        Self::new(32)
    }
}

impl ChannelManager {
    /// 创建新的通道管理器
    pub fn new(default_capacity: usize) -> Self {
        Self {
            default_capacity,
            parent_receivers: RwLock::new(HashMap::new()),
            child_senders: RwLock::new(HashMap::new()),
            pending_confirmations: RwLock::new(HashMap::new()),
        }
    }

    /// 为 Sub-Agent 创建通道
    pub async fn create_channel(
        &self,
        subagent_id: SubAgentId,
        parent_id: impl Into<String>,
    ) -> SubAgentChannel {
        let capacity = self.default_capacity;
        let parent_id = parent_id.into();

        // 创建双向通道
        // Sub-Agent -> Parent
        let (to_parent_tx, to_parent_rx) = mpsc::channel(capacity);
        // Parent -> Sub-Agent
        let (to_child_tx, to_child_rx) = mpsc::channel(capacity);

        // 存储接收端和发送端
        {
            let mut receivers = self.parent_receivers.write().await;
            receivers.insert(subagent_id.clone(), to_parent_rx);
        }
        {
            let mut senders = self.child_senders.write().await;
            senders.insert(subagent_id.clone(), to_child_tx);
        }

        SubAgentChannel::new(subagent_id, parent_id, to_parent_tx, to_child_rx, capacity)
    }

    /// 发送消息到 Sub-Agent
    pub async fn send_to_child(
        &self,
        subagent_id: &SubAgentId,
        message: ChannelMessage,
    ) -> Result<(), ChannelError> {
        let senders = self.child_senders.read().await;
        let sender = senders
            .get(subagent_id)
            .ok_or_else(|| ChannelError::NotFound {
                id: subagent_id.to_string(),
            })?;

        let envelope = MessageEnvelope::new("main", subagent_id.to_string(), message);

        sender
            .send(envelope)
            .await
            .map_err(|_| ChannelError::SendFailed {
                reason: "Child channel closed".to_string(),
            })
    }

    /// 发送文本消息到 Sub-Agent
    pub async fn send_text_to_child(
        &self,
        subagent_id: &SubAgentId,
        content: impl Into<String>,
    ) -> Result<(), ChannelError> {
        self.send_to_child(
            subagent_id,
            ChannelMessage::Text {
                content: content.into(),
            },
        )
        .await
    }

    /// 发送指令到 Sub-Agent
    pub async fn send_instruction_to_child(
        &self,
        subagent_id: &SubAgentId,
        instruction_type: InstructionType,
        content: impl Into<String>,
    ) -> Result<(), ChannelError> {
        self.send_to_child(
            subagent_id,
            ChannelMessage::Instruction {
                instruction_type,
                content: content.into(),
            },
        )
        .await
    }

    /// 从 Sub-Agent 接收消息（非阻塞）
    pub async fn try_recv_from_child(&self, subagent_id: &SubAgentId) -> Option<MessageEnvelope> {
        let mut receivers = self.parent_receivers.write().await;
        if let Some(receiver) = receivers.get_mut(subagent_id) {
            receiver.try_recv().ok()
        } else {
            None
        }
    }

    /// 从 Sub-Agent 接收消息（阻塞）
    pub async fn recv_from_child(
        &self,
        subagent_id: &SubAgentId,
    ) -> Result<MessageEnvelope, ChannelError> {
        let mut receivers = self.parent_receivers.write().await;
        let receiver = receivers
            .get_mut(subagent_id)
            .ok_or_else(|| ChannelError::NotFound {
                id: subagent_id.to_string(),
            })?;

        receiver.recv().await.ok_or(ChannelError::ChannelClosed)
    }

    /// 从任意 Sub-Agent 接收消息（带超时）
    pub async fn recv_any_timeout(
        &self,
        timeout: Duration,
    ) -> Result<(SubAgentId, MessageEnvelope), ChannelError> {
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            // 检查所有通道
            let mut receivers = self.parent_receivers.write().await;
            for (id, receiver) in receivers.iter_mut() {
                if let Ok(envelope) = receiver.try_recv() {
                    return Ok((id.clone(), envelope));
                }
            }
            drop(receivers);

            // 检查是否超时
            if tokio::time::Instant::now() >= deadline {
                return Err(ChannelError::Timeout);
            }

            // 短暂等待后重试
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// 响应确认请求
    pub async fn respond_confirmation(
        &self,
        request_id: &str,
        confirmed: bool,
        selected_option: Option<String>,
    ) -> Result<(), ChannelError> {
        let mut pending = self.pending_confirmations.write().await;
        let confirmation = pending
            .remove(request_id)
            .ok_or_else(|| ChannelError::NotFound {
                id: request_id.to_string(),
            })?;

        confirmation
            .response_tx
            .send(ConfirmResponse {
                confirmed,
                selected_option,
            })
            .map_err(|_| ChannelError::SendFailed {
                reason: "Confirmation receiver dropped".to_string(),
            })
    }

    /// 移除 Sub-Agent 的通道
    pub async fn remove_channel(&self, subagent_id: &SubAgentId) {
        {
            let mut receivers = self.parent_receivers.write().await;
            receivers.remove(subagent_id);
        }
        {
            let mut senders = self.child_senders.write().await;
            senders.remove(subagent_id);
        }
        debug!(subagent_id = %subagent_id, "Channel removed");
    }

    /// 获取活跃通道数量
    pub async fn active_channel_count(&self) -> usize {
        let senders = self.child_senders.read().await;
        senders.len()
    }

    /// 清理所有通道
    pub async fn clear_all(&self) {
        {
            let mut receivers = self.parent_receivers.write().await;
            receivers.clear();
        }
        {
            let mut senders = self.child_senders.write().await;
            senders.clear();
        }
        {
            let mut pending = self.pending_confirmations.write().await;
            pending.clear();
        }
        debug!("All channels cleared");
    }

    /// 检查 Sub-Agent 是否有活跃通道
    pub async fn has_channel(&self, subagent_id: &SubAgentId) -> bool {
        let senders = self.child_senders.read().await;
        senders.contains_key(subagent_id)
    }

    /// 广播消息到所有 Sub-Agent
    pub async fn broadcast(&self, message: ChannelMessage) -> Vec<SubAgentId> {
        let senders = self.child_senders.read().await;
        let mut failed = Vec::new();

        for (id, sender) in senders.iter() {
            let envelope = MessageEnvelope::new("main", id.to_string(), message.clone());
            if sender.send(envelope).await.is_err() {
                warn!(subagent_id = %id, "Failed to broadcast message");
                failed.push(id.clone());
            }
        }

        failed
    }
}

// ============================================================================
// Error Types
// ============================================================================

/// 通道错误
#[derive(Debug, Clone, thiserror::Error)]
pub enum ChannelError {
    /// 发送失败
    #[error("Failed to send message: {reason}")]
    SendFailed { reason: String },
    /// 接收超时
    #[error("Receive timeout")]
    Timeout,
    /// 通道已关闭
    #[error("Channel closed")]
    ChannelClosed,
    /// 通道未找到
    #[error("Channel not found: {id}")]
    NotFound { id: String },
    /// 消息格式错误
    #[error("Invalid message format: {reason}")]
    InvalidFormat { reason: String },
}

// ============================================================================
// Helper Functions
// ============================================================================

/// 生成消息 ID
fn generate_message_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::SeqCst);
    format!(
        "msg-{:08x}-{:04x}",
        current_timestamp_ms() as u32,
        count as u16
    )
}

/// 获取当前时间戳（毫秒）
fn current_timestamp_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn test_message_envelope_creation() {
        let envelope = MessageEnvelope::text("sender", "receiver", "Hello");
        assert_eq!(envelope.sender_id, "sender");
        assert_eq!(envelope.receiver_id, "receiver");
        assert!(!envelope.requires_ack);

        if let ChannelMessage::Text { content } = &envelope.message {
            assert_eq!(content, "Hello");
        } else {
            panic!("Expected Text message");
        }
    }

    #[test]
    fn test_message_envelope_with_ack() {
        let envelope = MessageEnvelope::text("sender", "receiver", "Hello").with_ack();
        assert!(envelope.requires_ack);
    }

    #[test]
    fn test_instruction_message() {
        let envelope = MessageEnvelope::instruction(
            "main",
            "sa-123",
            InstructionType::ModifyTask,
            "Change search keyword",
        );

        if let ChannelMessage::Instruction {
            instruction_type,
            content,
        } = &envelope.message
        {
            assert_eq!(*instruction_type, InstructionType::ModifyTask);
            assert_eq!(content, "Change search keyword");
        } else {
            panic!("Expected Instruction message");
        }
    }

    #[test]
    fn test_data_message() {
        let envelope =
            MessageEnvelope::data("sa-123", "main", "results", serde_json::json!({"count": 5}));

        if let ChannelMessage::Data { key, value } = &envelope.message {
            assert_eq!(key, "results");
            assert_eq!(value["count"], 5);
        } else {
            panic!("Expected Data message");
        }
    }

    #[test]
    fn test_message_serialization() {
        let message = ChannelMessage::Progress {
            percentage: 50,
            description: "Half done".to_string(),
        };

        let json = serde_json::to_string(&message).unwrap();
        assert!(json.contains("progress"));
        assert!(json.contains("50"));
        assert!(json.contains("Half done"));

        let deserialized: ChannelMessage = serde_json::from_str(&json).unwrap();
        if let ChannelMessage::Progress {
            percentage,
            description,
        } = deserialized
        {
            assert_eq!(percentage, 50);
            assert_eq!(description, "Half done");
        } else {
            panic!("Expected Progress message");
        }
    }

    #[tokio::test]
    async fn test_channel_manager_create_channel() {
        let manager = ChannelManager::default();
        let subagent_id = SubAgentId::new();

        let _channel = manager.create_channel(subagent_id.clone(), "main").await;

        assert!(manager.has_channel(&subagent_id).await);
        assert_eq!(manager.active_channel_count().await, 1);
    }

    #[tokio::test]
    async fn test_channel_manager_remove_channel() {
        let manager = ChannelManager::default();
        let subagent_id = SubAgentId::new();

        let _channel = manager.create_channel(subagent_id.clone(), "main").await;
        assert!(manager.has_channel(&subagent_id).await);

        manager.remove_channel(&subagent_id).await;
        assert!(!manager.has_channel(&subagent_id).await);
    }

    #[tokio::test]
    async fn test_channel_send_recv() {
        let manager = Arc::new(ChannelManager::default());
        let subagent_id = SubAgentId::new();

        let channel = manager.create_channel(subagent_id.clone(), "main").await;

        // Sub-Agent sends to parent
        channel
            .send_text_to_parent("Hello from child")
            .await
            .unwrap();

        // Parent receives
        let envelope = manager.try_recv_from_child(&subagent_id).await.unwrap();
        if let ChannelMessage::Text { content } = envelope.message {
            assert_eq!(content, "Hello from child");
        } else {
            panic!("Expected Text message");
        }
    }

    #[tokio::test]
    async fn test_channel_bidirectional() {
        let manager = Arc::new(ChannelManager::default());
        let subagent_id = SubAgentId::new();

        let mut channel = manager.create_channel(subagent_id.clone(), "main").await;

        // Parent sends to child
        manager
            .send_text_to_child(&subagent_id, "Hello from parent")
            .await
            .unwrap();

        // Child receives
        let envelope = channel.try_recv_from_parent().await.unwrap();
        if let ChannelMessage::Text { content } = envelope.message {
            assert_eq!(content, "Hello from parent");
        } else {
            panic!("Expected Text message");
        }
    }

    #[tokio::test]
    async fn test_channel_progress_report() {
        let manager = Arc::new(ChannelManager::default());
        let subagent_id = SubAgentId::new();

        let channel = manager.create_channel(subagent_id.clone(), "main").await;

        // Send progress
        channel.send_progress(50, "Processing...").await.unwrap();

        // Receive progress
        let envelope = manager.try_recv_from_child(&subagent_id).await.unwrap();
        if let ChannelMessage::Progress {
            percentage,
            description,
        } = envelope.message
        {
            assert_eq!(percentage, 50);
            assert_eq!(description, "Processing...");
        } else {
            panic!("Expected Progress message");
        }
    }

    #[tokio::test]
    async fn test_channel_broadcast() {
        let manager = Arc::new(ChannelManager::default());
        let id1 = SubAgentId::new();
        let id2 = SubAgentId::new();

        let mut channel1 = manager.create_channel(id1.clone(), "main").await;
        let mut channel2 = manager.create_channel(id2.clone(), "main").await;

        // Broadcast
        let failed = manager
            .broadcast(ChannelMessage::Text {
                content: "Broadcast message".to_string(),
            })
            .await;
        assert!(failed.is_empty());

        // Both should receive
        let msg1 = channel1.try_recv_from_parent().await.unwrap();
        let msg2 = channel2.try_recv_from_parent().await.unwrap();

        if let (ChannelMessage::Text { content: c1 }, ChannelMessage::Text { content: c2 }) =
            (&msg1.message, &msg2.message)
        {
            assert_eq!(c1, "Broadcast message");
            assert_eq!(c2, "Broadcast message");
        } else {
            panic!("Expected Text messages");
        }
    }

    #[tokio::test]
    async fn test_channel_clear_all() {
        let manager = ChannelManager::default();

        let _c1 = manager.create_channel(SubAgentId::new(), "main").await;
        let _c2 = manager.create_channel(SubAgentId::new(), "main").await;

        assert_eq!(manager.active_channel_count().await, 2);

        manager.clear_all().await;

        assert_eq!(manager.active_channel_count().await, 0);
    }

    #[test]
    fn test_generate_message_id() {
        let id1 = generate_message_id();
        let id2 = generate_message_id();

        assert!(id1.starts_with("msg-"));
        assert!(id2.starts_with("msg-"));
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_instruction_types() {
        let types = vec![
            InstructionType::ModifyTask,
            InstructionType::Pause,
            InstructionType::Resume,
            InstructionType::Stop,
            InstructionType::PriorityChange,
            InstructionType::Custom,
        ];

        for t in types {
            let json = serde_json::to_string(&t).unwrap();
            let deserialized: InstructionType = serde_json::from_str(&json).unwrap();
            assert_eq!(t, deserialized);
        }
    }
}
