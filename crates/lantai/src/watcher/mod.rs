pub mod debounce;
#[cfg(test)]
mod tests;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use notify::{EventKind, RecursiveMode, Watcher};
use tokio_util::sync::CancellationToken;

use self::debounce::FileDebouncer;
use crate::{
    Lantai,
    error::{LantaiError, LantaiResult},
};

/// 文件监视器 — 检测 .md 文件变更并触发增量索引
pub struct LantaiWatcher {
    lantai: Arc<Lantai>,
    dirs: Vec<PathBuf>,
    debounce_ms: u64,
}

impl LantaiWatcher {
    pub fn new(lantai: Arc<Lantai>, dirs: Vec<PathBuf>, debounce_ms: u64) -> Self {
        Self {
            lantai,
            dirs,
            debounce_ms,
        }
    }

    /// 启动文件监视（通过 CancellationToken 停止）
    pub async fn watch(&self, cancel: CancellationToken) -> LantaiResult<()> {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<notify::Event>(256);

        // 创建 notify watcher
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let _ = tx.blocking_send(event);
            }
        })
        .map_err(|e| LantaiError::Indexing(format!("Failed to create watcher: {e}")))?;

        // 注册监控目录
        for dir in &self.dirs {
            watcher
                .watch(dir.as_ref(), RecursiveMode::Recursive)
                .map_err(|e| {
                    LantaiError::Indexing(format!(
                        "Failed to watch directory {}: {e}",
                        dir.display()
                    ))
                })?;
        }

        let debounce_duration = Duration::from_millis(self.debounce_ms);
        let mut debouncer = FileDebouncer::new(debounce_duration);
        let tick_interval = Duration::from_millis(100);

        tracing::info!("Watching {} directories for .md changes", self.dirs.len());

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    tracing::info!("Watcher cancelled, stopping");
                    break;
                }
                event = rx.recv() => {
                    match event {
                        Some(event) => self.handle_notify_event(&mut debouncer, &event),
                        None => break, // channel closed
                    }
                }
                _ = tokio::time::sleep(tick_interval) => {
                    let ready = debouncer.drain_ready();
                    if !ready.is_empty() {
                        self.process_ready_files(&ready).await;
                    }
                }
            }
        }

        // 处理剩余的 pending 事件
        let remaining = debouncer.drain_ready();
        if !remaining.is_empty() {
            self.process_ready_files(&remaining).await;
        }

        Ok(())
    }

    fn handle_notify_event(&self, debouncer: &mut FileDebouncer, event: &notify::Event) {
        match event.kind {
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
                for path in &event.paths {
                    if is_markdown(path) {
                        debouncer.record_event(path.clone());
                    }
                }
            }
            _ => {}
        }
    }

    async fn process_ready_files(&self, files: &[PathBuf]) {
        // 收集涉及的目录，然后触发增量索引
        let dir_strs: Vec<String> = self
            .dirs
            .iter()
            .map(|d| d.to_string_lossy().to_string())
            .collect();
        let dir_refs: Vec<&str> = dir_strs.iter().map(|s| s.as_str()).collect();

        tracing::info!("Processing {} changed files", files.len());

        match self.lantai.index(&dir_refs).await {
            Ok(report) => {
                tracing::info!(
                    "Index updated: +{} ~{} -{} files, +{} -{} chunks",
                    report.files_added,
                    report.files_updated,
                    report.files_deleted,
                    report.chunks_added,
                    report.chunks_deleted,
                );
            }
            Err(e) => {
                tracing::error!("Index update failed: {e}");
            }
        }
    }
}

/// Mutex-wrapped variant of LantaiWatcher for shared access (e.g., from AppState).
///
/// Accepts `Arc<tokio::sync::Mutex<Lantai>>` so the same instance can be used
/// for both search (from tool calls) and indexing (from watcher).
pub struct LantaiMutexWatcher {
    lantai: Arc<tokio::sync::Mutex<Lantai>>,
    dirs: Vec<PathBuf>,
    debounce_ms: u64,
}

impl LantaiMutexWatcher {
    pub fn new(
        lantai: Arc<tokio::sync::Mutex<Lantai>>,
        dirs: Vec<PathBuf>,
        debounce_ms: u64,
    ) -> Self {
        Self {
            lantai,
            dirs,
            debounce_ms,
        }
    }

    pub async fn watch(&self, cancel: CancellationToken) -> LantaiResult<()> {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<notify::Event>(256);

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let _ = tx.blocking_send(event);
            }
        })
        .map_err(|e| LantaiError::Indexing(format!("Failed to create watcher: {e}")))?;

        for dir in &self.dirs {
            watcher
                .watch(dir.as_ref(), RecursiveMode::Recursive)
                .map_err(|e| {
                    LantaiError::Indexing(format!(
                        "Failed to watch directory {}: {e}",
                        dir.display()
                    ))
                })?;
        }

        let debounce_duration = Duration::from_millis(self.debounce_ms);
        let mut debouncer = FileDebouncer::new(debounce_duration);
        let tick_interval = Duration::from_millis(100);

        tracing::info!(
            "MutexWatcher: watching {} directories for .md changes",
            self.dirs.len()
        );

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    tracing::info!("MutexWatcher cancelled, stopping");
                    break;
                }
                event = rx.recv() => {
                    match event {
                        Some(event) => handle_notify_event_common(&self.dirs, &mut debouncer, &event),
                        None => break,
                    }
                }
                _ = tokio::time::sleep(tick_interval) => {
                    let ready = debouncer.drain_ready();
                    if !ready.is_empty() {
                        self.process_ready_files(&ready).await;
                    }
                }
            }
        }

        let remaining = debouncer.drain_ready();
        if !remaining.is_empty() {
            self.process_ready_files(&remaining).await;
        }

        Ok(())
    }

    async fn process_ready_files(&self, files: &[PathBuf]) {
        let dir_strs: Vec<String> = self
            .dirs
            .iter()
            .map(|d| d.to_string_lossy().to_string())
            .collect();
        let dir_refs: Vec<&str> = dir_strs.iter().map(|s| s.as_str()).collect();

        tracing::info!("MutexWatcher: processing {} changed files", files.len());

        let guard = self.lantai.lock().await;
        match guard.index(&dir_refs).await {
            Ok(report) => {
                tracing::info!(
                    "MutexWatcher index updated: +{} ~{} -{} files, +{} -{} chunks",
                    report.files_added,
                    report.files_updated,
                    report.files_deleted,
                    report.chunks_added,
                    report.chunks_deleted,
                );
            }
            Err(e) => {
                tracing::error!("MutexWatcher index update failed: {e}");
            }
        }
    }
}

/// Shared event filtering logic used by both watcher variants.
fn handle_notify_event_common(
    _dirs: &[PathBuf],
    debouncer: &mut FileDebouncer,
    event: &notify::Event,
) {
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
            for path in &event.paths {
                if is_markdown(path) {
                    debouncer.record_event(path.clone());
                }
            }
        }
        _ => {}
    }
}

fn is_markdown(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "md")
}
