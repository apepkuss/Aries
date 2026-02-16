use std::{collections::HashMap, path::PathBuf, time::Duration};

use tokio::time::Instant;

/// Per-file 防抖器
///
/// 同一文件在 debounce_duration 内的多次变更只触发一次索引
pub struct FileDebouncer {
    pending: HashMap<PathBuf, Instant>,
    debounce_duration: Duration,
}

impl FileDebouncer {
    pub fn new(debounce_duration: Duration) -> Self {
        Self {
            pending: HashMap::new(),
            debounce_duration,
        }
    }

    /// 记录文件变更事件（更新该文件的最后变更时间戳）
    pub fn record_event(&mut self, path: PathBuf) {
        self.pending.insert(path, Instant::now());
    }

    /// 返回已超过防抖时间的文件列表，并从 pending 中移除
    pub fn drain_ready(&mut self) -> Vec<PathBuf> {
        let now = Instant::now();
        let ready: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, ts)| now.duration_since(**ts) >= self.debounce_duration)
            .map(|(path, _)| path.clone())
            .collect();

        for path in &ready {
            self.pending.remove(path);
        }

        ready
    }

    /// 是否有待处理的事件
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
}
