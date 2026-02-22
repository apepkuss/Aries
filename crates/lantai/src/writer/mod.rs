#[cfg(test)]
mod tests;
pub mod types;

use std::path::{Path, PathBuf};

use chrono::Local;
use tokio::sync::RwLock;
use types::MarkdownSection;
pub use types::{MemoryCategory, MemoryWriteRequest};

use crate::error::{LantaiError, LantaiResult};

const MEMORY_FILE: &str = "MEMORY.md";
const EXPERIENCE_FILE: &str = "EXPERIENCE.md";

/// File-system memory writer for lantai.
///
/// Writes memory entries to markdown files, with file-level `RwLock` for
/// concurrent safety on MEMORY.md and EXPERIENCE.md. Daily logs use
/// O_APPEND and need no lock.
///
/// This struct is independent of [`crate::Lantai`] (which holds the SQLite
/// index). They share no mutex — file I/O here never blocks search/indexing.
pub struct MemoryWriter {
    memory_dir: PathBuf,
    /// Protects MEMORY.md read-modify-write operations
    core_lock: RwLock<()>,
    /// Protects EXPERIENCE.md read-modify-write operations
    experience_lock: RwLock<()>,
}

impl MemoryWriter {
    /// Create a new MemoryWriter. Creates `memory_dir` if it doesn't exist.
    pub fn new(memory_dir: impl Into<PathBuf>) -> Self {
        let memory_dir = memory_dir.into();
        if let Err(e) = std::fs::create_dir_all(&memory_dir) {
            tracing::warn!("Failed to create memory dir {}: {e}", memory_dir.display());
        }
        Self {
            memory_dir,
            core_lock: RwLock::new(()),
            experience_lock: RwLock::new(()),
        }
    }

    // ── Append write ──

    /// Write a new memory entry to the appropriate file.
    ///
    /// - `Daily` → append `- [HH:MM] {content}` to `YYYY-MM-DD.md`
    /// - `Core` → append `\n## {heading}\n\n- YYYY-MM-DD: {content}\n` to `MEMORY.md`
    /// - `Experience` → append `\n## {heading}\n\n{content}\n` to `EXPERIENCE.md`
    pub async fn write(&self, request: &MemoryWriteRequest) -> LantaiResult<String> {
        match request.category {
            MemoryCategory::Daily => {
                let now = Local::now();
                let date = now.format("%Y-%m-%d").to_string();
                let time = now.format("%H:%M").to_string();
                let line = format!("- [{}] {}\n", time, request.content);
                let path = self.memory_dir.join(format!("{date}.md"));
                Self::append_line(&path, &line).await?;
                Ok(format!("Saved to daily log ({date})"))
            }
            MemoryCategory::Core => {
                let heading = request.heading.as_deref().unwrap_or("General");
                let date = Local::now().format("%Y-%m-%d").to_string();
                let block = format!("\n## {heading}\n\n- {date}: {}\n", request.content);
                let path = self.memory_dir.join(MEMORY_FILE);
                Self::append_line(&path, &block).await?;
                Ok(format!("Saved to Core ({heading})"))
            }
            MemoryCategory::Experience => {
                let heading = request.heading.as_deref().unwrap_or("General");
                let block = format!("\n## {heading}\n\n{}\n", request.content);
                let path = self.memory_dir.join(EXPERIENCE_FILE);
                Self::append_line(&path, &block).await?;
                Ok(format!("Saved to Experience ({heading})"))
            }
        }
    }

    // ── Read ──

    /// Read the daily log for a given date (e.g. "2026-02-15"). No lock needed.
    pub async fn read_daily_log(&self, date: &str) -> LantaiResult<Option<String>> {
        let path = self.memory_dir.join(format!("{date}.md"));
        Self::read_file_opt(&path).await
    }

    /// Read MEMORY.md (acquires core_lock read guard).
    pub async fn read_core_memory(&self) -> LantaiResult<Option<String>> {
        let _guard = self.core_lock.read().await;
        let path = self.memory_dir.join(MEMORY_FILE);
        Self::read_file_opt(&path).await
    }

    /// Read EXPERIENCE.md (acquires experience_lock read guard).
    pub async fn read_experience_memory(&self) -> LantaiResult<Option<String>> {
        let _guard = self.experience_lock.read().await;
        let path = self.memory_dir.join(EXPERIENCE_FILE);
        Self::read_file_opt(&path).await
    }

    // ── Section-level editing (Core / Experience only) ──

    /// List all `## heading` names in the given category file.
    pub async fn list_sections(&self, category: MemoryCategory) -> LantaiResult<Vec<String>> {
        let content = self.read_category(category).await?;
        match content {
            Some(c) => Ok(Self::parse_sections(&c)
                .into_iter()
                .map(|s| s.heading)
                .collect()),
            None => Ok(vec![]),
        }
    }

    /// Replace the body of a section identified by `heading`.
    pub async fn update_section(
        &self,
        category: MemoryCategory,
        heading: &str,
        new_content: &str,
    ) -> LantaiResult<()> {
        Self::ensure_not_daily(category)?;
        let _guard = self.acquire_write_lock(category).await;
        let path = self.category_path(category);

        let content = Self::read_file_opt(&path).await?.unwrap_or_default();
        let mut sections = Self::parse_sections(&content);

        let found = sections.iter_mut().find(|s| s.heading == heading);
        match found {
            Some(section) => {
                section.content = format!("\n{new_content}\n");
            }
            None => {
                return Err(LantaiError::Indexing(format!(
                    "Section '{heading}' not found"
                )));
            }
        }

        let rebuilt = Self::rebuild_from_sections(&sections);
        Self::atomic_write(&path, &rebuilt).await
    }

    /// Delete an entire section by heading.
    pub async fn delete_section(
        &self,
        category: MemoryCategory,
        heading: &str,
    ) -> LantaiResult<()> {
        Self::ensure_not_daily(category)?;
        let _guard = self.acquire_write_lock(category).await;
        let path = self.category_path(category);

        let content = Self::read_file_opt(&path).await?.unwrap_or_default();
        let sections = Self::parse_sections(&content);
        let before_len = sections.len();
        let filtered: Vec<_> = sections
            .into_iter()
            .filter(|s| s.heading != heading)
            .collect();

        if filtered.len() == before_len {
            return Err(LantaiError::Indexing(format!(
                "Section '{heading}' not found"
            )));
        }

        let rebuilt = Self::rebuild_from_sections(&filtered);
        Self::atomic_write(&path, &rebuilt).await
    }

    /// Rewrite an entire file (used by compaction). Acquires write lock.
    pub async fn rewrite_file(&self, category: MemoryCategory, content: &str) -> LantaiResult<()> {
        Self::ensure_not_daily(category)?;
        let _guard = self.acquire_write_lock(category).await;
        let path = self.category_path(category);
        Self::atomic_write(&path, content).await
    }

    /// Return the file size in bytes for a category file. Returns 0 if file doesn't exist.
    pub async fn file_size(&self, category: MemoryCategory) -> u64 {
        let path = self.category_path(category);
        tokio::fs::metadata(&path)
            .await
            .map(|m| m.len())
            .unwrap_or(0)
    }

    // ── Internal helpers ──

    fn parse_sections(content: &str) -> Vec<MarkdownSection> {
        let mut sections = Vec::new();
        let mut current_heading: Option<String> = None;
        let mut current_body = String::new();
        let mut preamble = String::new();

        for line in content.lines() {
            if let Some(h) = line.strip_prefix("## ") {
                // Flush previous section
                if let Some(heading) = current_heading.take() {
                    sections.push(MarkdownSection {
                        heading,
                        content: current_body.clone(),
                    });
                    current_body.clear();
                } else if !preamble.is_empty() {
                    // Content before the first heading
                    sections.push(MarkdownSection {
                        heading: String::new(),
                        content: preamble.clone(),
                    });
                    preamble.clear();
                }
                current_heading = Some(h.trim().to_string());
            } else if current_heading.is_some() {
                current_body.push_str(line);
                current_body.push('\n');
            } else {
                preamble.push_str(line);
                preamble.push('\n');
            }
        }

        // Flush last section / preamble
        if let Some(heading) = current_heading {
            sections.push(MarkdownSection {
                heading,
                content: current_body,
            });
        } else if !preamble.is_empty() {
            sections.push(MarkdownSection {
                heading: String::new(),
                content: preamble,
            });
        }

        sections
    }

    fn rebuild_from_sections(sections: &[MarkdownSection]) -> String {
        let mut out = String::new();
        for (i, s) in sections.iter().enumerate() {
            if s.heading.is_empty() {
                // Preamble (content before first heading)
                out.push_str(&s.content);
            } else {
                if i > 0 {
                    out.push('\n');
                }
                out.push_str(&format!("## {}\n", s.heading));
                out.push_str(&s.content);
            }
        }
        out
    }

    async fn atomic_write(path: &Path, content: &str) -> LantaiResult<()> {
        let tmp = path.with_extension("md.tmp");
        tokio::fs::write(&tmp, content).await?;
        tokio::fs::rename(&tmp, path).await?;
        Ok(())
    }

    async fn append_line(path: &Path, line: &str) -> LantaiResult<()> {
        use tokio::{fs::OpenOptions, io::AsyncWriteExt};

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        file.write_all(line.as_bytes()).await?;
        file.flush().await?;
        Ok(())
    }

    async fn read_file_opt(path: &Path) -> LantaiResult<Option<String>> {
        match tokio::fs::read_to_string(path).await {
            Ok(s) if s.is_empty() => Ok(None),
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn ensure_not_daily(category: MemoryCategory) -> LantaiResult<()> {
        if matches!(category, MemoryCategory::Daily) {
            return Err(LantaiError::Indexing(
                "Daily logs cannot be edited or deleted".into(),
            ));
        }
        Ok(())
    }

    fn category_path(&self, category: MemoryCategory) -> PathBuf {
        match category {
            MemoryCategory::Daily => {
                let date = Local::now().format("%Y-%m-%d").to_string();
                self.memory_dir.join(format!("{date}.md"))
            }
            MemoryCategory::Core => self.memory_dir.join(MEMORY_FILE),
            MemoryCategory::Experience => self.memory_dir.join(EXPERIENCE_FILE),
        }
    }

    async fn read_category(&self, category: MemoryCategory) -> LantaiResult<Option<String>> {
        match category {
            MemoryCategory::Daily => {
                let date = Local::now().format("%Y-%m-%d").to_string();
                self.read_daily_log(&date).await
            }
            MemoryCategory::Core => self.read_core_memory().await,
            MemoryCategory::Experience => self.read_experience_memory().await,
        }
    }

    async fn acquire_write_lock(
        &self,
        category: MemoryCategory,
    ) -> tokio::sync::RwLockWriteGuard<'_, ()> {
        match category {
            MemoryCategory::Core | MemoryCategory::Daily => self.core_lock.write().await,
            MemoryCategory::Experience => self.experience_lock.write().await,
        }
    }
}
