use std::{path::PathBuf, time::Duration};

use super::debounce::FileDebouncer;

#[tokio::test]
async fn test_debouncer_basic() {
    let mut debouncer = FileDebouncer::new(Duration::from_millis(50));

    debouncer.record_event(PathBuf::from("test.md"));
    assert!(debouncer.has_pending());

    // 还没到防抖时间，应返回空
    let ready = debouncer.drain_ready();
    assert!(ready.is_empty());

    // 等待超过防抖时间
    tokio::time::sleep(Duration::from_millis(60)).await;

    let ready = debouncer.drain_ready();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0], PathBuf::from("test.md"));

    // drain 后应该没有 pending
    assert!(!debouncer.has_pending());
}

#[tokio::test]
async fn test_debouncer_multiple_rapid() {
    let mut debouncer = FileDebouncer::new(Duration::from_millis(100));

    // 快速连续记录同一文件
    debouncer.record_event(PathBuf::from("doc.md"));
    tokio::time::sleep(Duration::from_millis(20)).await;
    debouncer.record_event(PathBuf::from("doc.md"));
    tokio::time::sleep(Duration::from_millis(20)).await;
    debouncer.record_event(PathBuf::from("doc.md"));

    // 此时距最后一次事件还不到 100ms
    let ready = debouncer.drain_ready();
    assert!(ready.is_empty());

    // 等待超过防抖时间
    tokio::time::sleep(Duration::from_millis(110)).await;

    let ready = debouncer.drain_ready();
    // 应只返回一次
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0], PathBuf::from("doc.md"));
}

#[tokio::test]
async fn test_debouncer_different_files() {
    let mut debouncer = FileDebouncer::new(Duration::from_millis(50));

    debouncer.record_event(PathBuf::from("a.md"));
    debouncer.record_event(PathBuf::from("b.md"));

    // 等待超过防抖时间
    tokio::time::sleep(Duration::from_millis(60)).await;

    let ready = debouncer.drain_ready();
    assert_eq!(ready.len(), 2);

    let paths: Vec<String> = ready
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    assert!(paths.contains(&"a.md".to_string()));
    assert!(paths.contains(&"b.md".to_string()));
}
