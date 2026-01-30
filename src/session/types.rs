use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// JSONL format version
pub const JSONL_FORMAT_VERSION: u8 = 1;

/// JSONL line record (tagged enum, distinguished by `type` field)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SessionRecord {
    /// Session start marker (first line of each file)
    #[serde(rename = "session_start")]
    SessionStart {
        version: u8,
        session_id: String,
        user_id: String,
        model: String,
        created_at: DateTime<Utc>,
    },

    /// Message record
    #[serde(rename = "message")]
    Message {
        version: u8,
        role: String,
        content: String,
        timestamp: DateTime<Utc>,
        message_id: String,
        sequence: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        tokens: Option<TokenUsage>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<serde_json::Value>>,
    },
}

/// Token usage for a message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt: u64,
    pub completion: u64,
}

/// Session metadata extracted from JSONL file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub session_id: String,
    pub user_id: String,
    pub model: String,
    pub created_at: DateTime<Utc>,
    /// Derived from file modification time
    pub updated_at: DateTime<Utc>,
    /// Count of message records (type=message lines)
    pub message_count: usize,
}

/// Session error types
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Session not found: {0}")]
    NotFound(String),

    #[error("Invalid session file: {0}")]
    InvalidFile(String),
}

pub type SessionResult<T> = Result<T, SessionError>;
