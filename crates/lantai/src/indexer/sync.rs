use std::collections::HashSet;

use crate::{chunking::types::ScannedFile, error::LantaiResult, store::Database};

/// 增量同步计划
#[derive(Debug)]
pub struct SyncPlan {
    /// 需要新增或更新的文件
    pub changed_files: Vec<ScannedFile>,
    /// 需要删除的文件路径（已从磁盘移除）
    pub deleted_files: Vec<String>,
    /// 未变化的文件数
    pub unchanged_count: usize,
}

/// 对比磁盘扫描结果和数据库记录，生成同步计划
pub fn compute_sync_plan(db: &Database, scanned_files: &[ScannedFile]) -> LantaiResult<SyncPlan> {
    // 1. 获取数据库中所有已索引文件
    let indexed_paths: HashSet<String> = db.list_indexed_files()?.into_iter().collect();

    // 2. 扫描文件路径集合
    let scanned_paths: HashSet<&str> = scanned_files.iter().map(|f| f.path.as_str()).collect();

    // 3. 找出已删除的文件（在数据库中但不在磁盘上）
    let deleted_files: Vec<String> = indexed_paths
        .iter()
        .filter(|p| !scanned_paths.contains(p.as_str()))
        .cloned()
        .collect();

    // 4. 分类变更和未变化的文件
    let mut changed_files = Vec::new();
    let mut unchanged_count = 0;

    for file in scanned_files {
        match db.get_file(&file.path)? {
            Some(record) if record.content_hash == file.content_hash => {
                unchanged_count += 1;
            }
            _ => {
                // 新文件或 content_hash 变化
                changed_files.push(file.clone());
            }
        }
    }

    Ok(SyncPlan {
        changed_files,
        deleted_files,
        unchanged_count,
    })
}
