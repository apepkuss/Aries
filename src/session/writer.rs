use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicI64, Ordering},
};

use chrono::Utc;
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncWriteExt,
    sync::RwLock,
};

use super::types::*;

/// Active session state tracked in memory
struct ActiveSession {
    session_id: String,
    /// Next sequence number for messages
    sequence: AtomicI64,
    /// Whether the session file has been initialized (session_start written)
    initialized: bool,
}

/// JSONL session writer that appends chat records to per-session files.
///
/// Storage layout:
/// ```text
/// {base_dir}/
///   {user_id}/
///     {session_id}.jsonl
/// ```
pub struct SessionWriter {
    /// Root directory for session files
    base_dir: PathBuf,
    /// Active sessions: user_id → ActiveSession
    active_sessions: RwLock<HashMap<String, ActiveSession>>,
}

impl SessionWriter {
    /// Create a new SessionWriter with the given base directory.
    ///
    /// The directory will be created on first write if it doesn't exist.
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
            active_sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Get the base directory for session files.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Get or create an active session ID for the given user.
    ///
    /// - If the user already has an active session, returns the existing ID.
    /// - Otherwise, generates a new session ID and registers it.
    pub async fn get_or_create_session_id(&self, user_id: &str) -> String {
        // Fast path: read lock
        {
            let sessions = self.active_sessions.read().await;
            if let Some(session) = sessions.get(user_id) {
                return session.session_id.clone();
            }
        }

        // Slow path: write lock + create
        let mut sessions = self.active_sessions.write().await;
        // Double-check after acquiring write lock
        if let Some(session) = sessions.get(user_id) {
            return session.session_id.clone();
        }

        let session_id = format!("sess_{}", uuid::Uuid::new_v4());
        sessions.insert(
            user_id.to_string(),
            ActiveSession {
                session_id: session_id.clone(),
                sequence: AtomicI64::new(1),
                initialized: false,
            },
        );
        session_id
    }

    /// Get the next sequence number for a session.
    ///
    /// Returns the current value and increments atomically.
    pub async fn next_sequence(&self, user_id: &str) -> i64 {
        let sessions = self.active_sessions.read().await;
        if let Some(session) = sessions.get(user_id) {
            session.sequence.fetch_add(1, Ordering::Relaxed)
        } else {
            1
        }
    }

    /// Append a message record to the session's JSONL file.
    ///
    /// If the session file doesn't exist yet, a `session_start` record is
    /// written first, followed by the message.
    ///
    /// This method is safe to call concurrently — each write opens/closes
    /// the file handle independently and uses append mode.
    pub async fn append_message(
        &self,
        user_id: &str,
        session_id: &str,
        model: &str,
        record: SessionRecord,
    ) -> SessionResult<()> {
        let file_path = self.session_file_path(user_id, session_id);

        // Ensure directory exists
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Atomically check and write session_start if needed.
        // Hold the write lock across both the check and the file write
        // to prevent concurrent tasks from each writing session_start.
        {
            let mut sessions = self.active_sessions.write().await;
            let needs_init = sessions
                .get(user_id)
                .map(|s| !s.initialized)
                .unwrap_or(true);

            if needs_init {
                let start_record = SessionRecord::SessionStart {
                    version: JSONL_FORMAT_VERSION,
                    session_id: session_id.to_string(),
                    user_id: user_id.to_string(),
                    model: model.to_string(),
                    created_at: Utc::now(),
                };
                self.append_record(&file_path, &start_record).await?;

                if let Some(session) = sessions.get_mut(user_id) {
                    session.initialized = true;
                }
            }
        }

        // Append the message record
        self.append_record(&file_path, &record).await
    }

    /// Compute the file path for a session.
    fn session_file_path(&self, user_id: &str, session_id: &str) -> PathBuf {
        self.base_dir
            .join(user_id)
            .join(format!("{session_id}.jsonl"))
    }

    /// Append a single JSON line to the given file.
    async fn append_record(&self, path: &Path, record: &SessionRecord) -> SessionResult<()> {
        let mut line = serde_json::to_string(record)?;
        line.push('\n');

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;

        file.write_all(line.as_bytes()).await?;
        file.flush().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn test_get_or_create_session_id_returns_same_id_for_same_user() {
        let tmp = TempDir::new().unwrap();
        let writer = SessionWriter::new(tmp.path());

        let id1 = writer.get_or_create_session_id("user_1").await;
        let id2 = writer.get_or_create_session_id("user_1").await;
        assert_eq!(id1, id2);
    }

    #[tokio::test]
    async fn test_get_or_create_session_id_different_users_get_different_ids() {
        let tmp = TempDir::new().unwrap();
        let writer = SessionWriter::new(tmp.path());

        let id1 = writer.get_or_create_session_id("user_1").await;
        let id2 = writer.get_or_create_session_id("user_2").await;
        assert_ne!(id1, id2);
    }

    #[tokio::test]
    async fn test_append_message_creates_session_start_and_message() {
        let tmp = TempDir::new().unwrap();
        let writer = SessionWriter::new(tmp.path());

        let session_id = writer.get_or_create_session_id("user_1").await;
        let seq = writer.next_sequence("user_1").await;

        let record = SessionRecord::Message {
            version: JSONL_FORMAT_VERSION,
            role: "user".to_string(),
            content: "Hello".to_string(),
            timestamp: Utc::now(),
            message_id: "msg_001".to_string(),
            sequence: seq,
            tokens: None,
            tool_calls: None,
            privacy_mode: false,
        };

        writer
            .append_message("user_1", &session_id, "test-model", record)
            .await
            .unwrap();

        // Read file and verify
        let file_path = tmp
            .path()
            .join("user_1")
            .join(format!("{session_id}.jsonl"));
        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();

        assert_eq!(lines.len(), 2, "Should have session_start + 1 message");

        // Verify session_start
        let start: SessionRecord = serde_json::from_str(lines[0]).unwrap();
        match start {
            SessionRecord::SessionStart {
                version,
                session_id: sid,
                user_id,
                model,
                ..
            } => {
                assert_eq!(version, JSONL_FORMAT_VERSION);
                assert_eq!(sid, session_id);
                assert_eq!(user_id, "user_1");
                assert_eq!(model, "test-model");
            }
            _ => panic!("Expected SessionStart record"),
        }

        // Verify message
        let msg: SessionRecord = serde_json::from_str(lines[1]).unwrap();
        match msg {
            SessionRecord::Message {
                role,
                content,
                sequence,
                ..
            } => {
                assert_eq!(role, "user");
                assert_eq!(content, "Hello");
                assert_eq!(sequence, 1);
            }
            _ => panic!("Expected Message record"),
        }
    }

    #[tokio::test]
    async fn test_append_multiple_messages_single_session_start() {
        let tmp = TempDir::new().unwrap();
        let writer = SessionWriter::new(tmp.path());

        let session_id = writer.get_or_create_session_id("user_1").await;

        // Write two messages
        for i in 1..=2 {
            let seq = writer.next_sequence("user_1").await;
            let record = SessionRecord::Message {
                version: JSONL_FORMAT_VERSION,
                role: "user".to_string(),
                content: format!("Message {i}"),
                timestamp: Utc::now(),
                message_id: format!("msg_{i:03}"),
                sequence: seq,
                tokens: None,
                tool_calls: None,
                privacy_mode: false,
            };
            writer
                .append_message("user_1", &session_id, "test-model", record)
                .await
                .unwrap();
        }

        let file_path = tmp
            .path()
            .join("user_1")
            .join(format!("{session_id}.jsonl"));
        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();

        assert_eq!(lines.len(), 3, "Should have 1 session_start + 2 messages");

        // Only the first line should be session_start
        let first: SessionRecord = serde_json::from_str(lines[0]).unwrap();
        assert!(matches!(first, SessionRecord::SessionStart { .. }));

        // Lines 2 and 3 should be messages
        for line in &lines[1..] {
            let rec: SessionRecord = serde_json::from_str(line).unwrap();
            assert!(matches!(rec, SessionRecord::Message { .. }));
        }
    }

    #[tokio::test]
    async fn test_next_sequence_increments() {
        let tmp = TempDir::new().unwrap();
        let writer = SessionWriter::new(tmp.path());

        let _ = writer.get_or_create_session_id("user_1").await;

        let s1 = writer.next_sequence("user_1").await;
        let s2 = writer.next_sequence("user_1").await;
        let s3 = writer.next_sequence("user_1").await;

        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
        assert_eq!(s3, 3);
    }

    #[tokio::test]
    async fn test_concurrent_writes_no_panic() {
        let tmp = TempDir::new().unwrap();
        let writer = std::sync::Arc::new(SessionWriter::new(tmp.path()));

        let session_id = writer.get_or_create_session_id("user_1").await;

        let mut handles = Vec::new();
        for i in 0..10 {
            let w = writer.clone();
            let sid = session_id.clone();
            handles.push(tokio::spawn(async move {
                let seq = w.next_sequence("user_1").await;
                let record = SessionRecord::Message {
                    version: JSONL_FORMAT_VERSION,
                    role: "user".to_string(),
                    content: format!("Concurrent message {i}"),
                    timestamp: Utc::now(),
                    message_id: format!("msg_concurrent_{i}"),
                    sequence: seq,
                    tokens: None,
                    tool_calls: None,
                    privacy_mode: false,
                };
                w.append_message("user_1", &sid, "test-model", record)
                    .await
                    .unwrap();
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        // Verify file has 11 lines (1 session_start + 10 messages)
        let file_path = tmp
            .path()
            .join("user_1")
            .join(format!("{session_id}.jsonl"));
        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();

        assert_eq!(lines.len(), 11, "Should have 1 session_start + 10 messages");

        // All lines should be valid JSON
        for line in &lines {
            let _: SessionRecord = serde_json::from_str(line).unwrap();
        }
    }
}
