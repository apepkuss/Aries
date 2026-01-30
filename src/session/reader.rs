use std::path::PathBuf;

use chrono::{DateTime, Utc};
use tokio::fs;

use super::types::*;

/// JSONL session reader for listing, reading, and deleting session files.
pub struct SessionReader {
    /// Root directory for session files (same as SessionWriter)
    base_dir: PathBuf,
}

impl SessionReader {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    /// List all sessions for a given user.
    ///
    /// Scans the user's directory for `.jsonl` files and extracts metadata
    /// from the `session_start` line and file modification time.
    pub async fn list_sessions(&self, user_id: &str) -> SessionResult<Vec<SessionMeta>> {
        let user_dir = self.base_dir.join(user_id);
        if !user_dir.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();
        let mut entries = fs::read_dir(&user_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }

            match self.extract_meta(&path).await {
                Ok(meta) => sessions.push(meta),
                Err(e) => {
                    // Log but skip malformed files
                    eprintln!("Warning: skipping malformed session file {:?}: {}", path, e);
                }
            }
        }

        // Sort by updated_at descending (most recent first)
        sessions.sort_by_key(|b| std::cmp::Reverse(b.updated_at));

        Ok(sessions)
    }

    /// Read all records from a session file.
    pub async fn read_session(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> SessionResult<Vec<SessionRecord>> {
        let path = self.session_file_path(user_id, session_id);
        if !path.exists() {
            return Err(SessionError::NotFound(session_id.to_string()));
        }

        let content = fs::read_to_string(&path).await?;
        let mut records = Vec::new();

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let record: SessionRecord = serde_json::from_str(line)?;
            records.push(record);
        }

        Ok(records)
    }

    /// Delete a session file.
    pub async fn delete_session(&self, user_id: &str, session_id: &str) -> SessionResult<()> {
        let path = self.session_file_path(user_id, session_id);
        if !path.exists() {
            return Err(SessionError::NotFound(session_id.to_string()));
        }

        fs::remove_file(&path).await?;
        Ok(())
    }

    /// Extract metadata from a session JSONL file.
    ///
    /// Reads the first line for `session_start` data, counts message lines,
    /// and uses file modification time for `updated_at`.
    async fn extract_meta(&self, path: &std::path::Path) -> SessionResult<SessionMeta> {
        let content = fs::read_to_string(path).await?;
        let metadata = fs::metadata(path).await?;

        let updated_at: DateTime<Utc> = metadata
            .modified()
            .map(|t| t.into())
            .unwrap_or_else(|_| Utc::now());

        let mut session_id = String::new();
        let mut user_id = String::new();
        let mut model = String::new();
        let mut created_at = Utc::now();
        let mut message_count = 0usize;
        let mut found_start = false;

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let record: SessionRecord = serde_json::from_str(line)?;
            match record {
                SessionRecord::SessionStart {
                    session_id: sid,
                    user_id: uid,
                    model: m,
                    created_at: ca,
                    ..
                } => {
                    session_id = sid;
                    user_id = uid;
                    model = m;
                    created_at = ca;
                    found_start = true;
                }
                SessionRecord::Message { .. } => {
                    message_count += 1;
                }
            }
        }

        if !found_start {
            return Err(SessionError::InvalidFile(format!(
                "Missing session_start record in {:?}",
                path
            )));
        }

        Ok(SessionMeta {
            session_id,
            user_id,
            model,
            created_at,
            updated_at,
            message_count,
        })
    }

    fn session_file_path(&self, user_id: &str, session_id: &str) -> PathBuf {
        self.base_dir
            .join(user_id)
            .join(format!("{session_id}.jsonl"))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::session::writer::SessionWriter;

    /// Helper: write a session with N messages and return (session_id, TempDir)
    async fn setup_session(msg_count: usize) -> (String, TempDir) {
        let tmp = TempDir::new().unwrap();
        let writer = SessionWriter::new(tmp.path());

        let session_id = writer.get_or_create_session_id("user_1").await;

        for i in 1..=msg_count {
            let seq = writer.next_sequence("user_1").await;
            let record = SessionRecord::Message {
                version: JSONL_FORMAT_VERSION,
                role: if i % 2 == 1 { "user" } else { "assistant" }.to_string(),
                content: format!("Message {i}"),
                timestamp: Utc::now(),
                message_id: format!("msg_{i:03}"),
                sequence: seq,
                tokens: None,
                tool_calls: None,
            };
            writer
                .append_message("user_1", &session_id, "test-model", record)
                .await
                .unwrap();
        }

        (session_id, tmp)
    }

    #[tokio::test]
    async fn test_list_sessions_empty_user() {
        let tmp = TempDir::new().unwrap();
        let reader = SessionReader::new(tmp.path());

        let sessions = reader.list_sessions("nonexistent").await.unwrap();
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_list_sessions_returns_metadata() {
        let (session_id, tmp) = setup_session(3).await;
        let reader = SessionReader::new(tmp.path());

        let sessions = reader.list_sessions("user_1").await.unwrap();
        assert_eq!(sessions.len(), 1);

        let meta = &sessions[0];
        assert_eq!(meta.session_id, session_id);
        assert_eq!(meta.user_id, "user_1");
        assert_eq!(meta.model, "test-model");
        assert_eq!(meta.message_count, 3);
    }

    #[tokio::test]
    async fn test_read_session_returns_all_records() {
        let (session_id, tmp) = setup_session(2).await;
        let reader = SessionReader::new(tmp.path());

        let records = reader.read_session("user_1", &session_id).await.unwrap();

        // 1 session_start + 2 messages
        assert_eq!(records.len(), 3);
        assert!(matches!(&records[0], SessionRecord::SessionStart { .. }));
        assert!(matches!(&records[1], SessionRecord::Message { .. }));
        assert!(matches!(&records[2], SessionRecord::Message { .. }));
    }

    #[tokio::test]
    async fn test_read_session_not_found() {
        let tmp = TempDir::new().unwrap();
        let reader = SessionReader::new(tmp.path());

        let result = reader.read_session("user_1", "nonexistent").await;
        assert!(matches!(result, Err(SessionError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_delete_session() {
        let (session_id, tmp) = setup_session(1).await;
        let reader = SessionReader::new(tmp.path());

        // File should exist before delete
        let sessions = reader.list_sessions("user_1").await.unwrap();
        assert_eq!(sessions.len(), 1);

        // Delete
        reader.delete_session("user_1", &session_id).await.unwrap();

        // File should be gone
        let sessions = reader.list_sessions("user_1").await.unwrap();
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_delete_session_not_found() {
        let tmp = TempDir::new().unwrap();
        let reader = SessionReader::new(tmp.path());

        let result = reader.delete_session("user_1", "nonexistent").await;
        assert!(matches!(result, Err(SessionError::NotFound(_))));
    }
}
