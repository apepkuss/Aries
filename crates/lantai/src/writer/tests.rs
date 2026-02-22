use tempfile::TempDir;

use super::*;

fn make_writer() -> (TempDir, MemoryWriter) {
    let dir = TempDir::new().unwrap();
    let writer = MemoryWriter::new(dir.path());
    (dir, writer)
}

// ── write tests ──

#[tokio::test]
async fn write_daily_creates_dated_file() {
    let (dir, writer) = make_writer();
    let req = MemoryWriteRequest {
        content: "test note".into(),
        category: MemoryCategory::Daily,
        heading: None,
    };
    let msg = writer.write(&req).await.unwrap();
    assert!(msg.contains("daily log"));

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let content = std::fs::read_to_string(dir.path().join(format!("{today}.md"))).unwrap();
    assert!(content.contains("test note"));
    assert!(content.starts_with("- ["));
}

#[tokio::test]
async fn write_core_has_date_prefix() {
    let (_dir, writer) = make_writer();
    let req = MemoryWriteRequest {
        content: "use REST not GraphQL".into(),
        category: MemoryCategory::Core,
        heading: Some("API Design".into()),
    };
    let msg = writer.write(&req).await.unwrap();
    assert!(msg.contains("Core"));

    let content = writer.read_core_memory().await.unwrap().unwrap();
    assert!(content.contains("## API Design"));
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    assert!(content.contains(&format!("- {today}: use REST not GraphQL")));
}

#[tokio::test]
async fn write_experience_no_date_prefix() {
    let (_dir, writer) = make_writer();
    let req = MemoryWriteRequest {
        content: "always check error codes".into(),
        category: MemoryCategory::Experience,
        heading: Some("Error Handling".into()),
    };
    writer.write(&req).await.unwrap();

    let content = writer.read_experience_memory().await.unwrap().unwrap();
    assert!(content.contains("## Error Handling"));
    assert!(content.contains("always check error codes"));
    // Should NOT have date prefix like Core
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    assert!(!content.contains(&format!("- {today}:")));
}

#[tokio::test]
async fn write_core_default_heading() {
    let (_dir, writer) = make_writer();
    let req = MemoryWriteRequest {
        content: "some info".into(),
        category: MemoryCategory::Core,
        heading: None,
    };
    writer.write(&req).await.unwrap();

    let content = writer.read_core_memory().await.unwrap().unwrap();
    assert!(content.contains("## General"));
}

// ── read tests ──

#[tokio::test]
async fn read_nonexistent_returns_none() {
    let (_dir, writer) = make_writer();
    assert!(writer.read_core_memory().await.unwrap().is_none());
    assert!(writer.read_experience_memory().await.unwrap().is_none());
    assert!(writer.read_daily_log("2099-01-01").await.unwrap().is_none());
}

// ── parse_sections + rebuild roundtrip ──

#[tokio::test]
async fn parse_and_rebuild_roundtrip() {
    let input = "\
## Users

- Alice
- Bob

## Config

key = value
";
    let sections = MemoryWriter::parse_sections(input);
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0].heading, "Users");
    assert!(sections[0].content.contains("Alice"));
    assert_eq!(sections[1].heading, "Config");

    let rebuilt = MemoryWriter::rebuild_from_sections(&sections);
    // Verify headings and content survive roundtrip
    assert!(rebuilt.contains("## Users"));
    assert!(rebuilt.contains("- Alice"));
    assert!(rebuilt.contains("## Config"));
    assert!(rebuilt.contains("key = value"));
}

#[tokio::test]
async fn parse_with_preamble() {
    let input = "# Title\nSome preamble\n\n## Section\nbody\n";
    let sections = MemoryWriter::parse_sections(input);
    assert_eq!(sections.len(), 2);
    assert!(sections[0].heading.is_empty()); // preamble
    assert_eq!(sections[1].heading, "Section");
}

// ── update_section ──

#[tokio::test]
async fn update_section_replaces_content() {
    let (_dir, writer) = make_writer();

    // Seed MEMORY.md with two sections
    let initial = "## Prefs\n\nold prefs\n\n## Decisions\n\nold decision\n";
    let path = writer.memory_dir.join(MEMORY_FILE);
    tokio::fs::write(&path, initial).await.unwrap();

    writer
        .update_section(MemoryCategory::Core, "Prefs", "new prefs here")
        .await
        .unwrap();

    let content = writer.read_core_memory().await.unwrap().unwrap();
    assert!(content.contains("new prefs here"));
    assert!(!content.contains("old prefs"));
    // Other section untouched
    assert!(content.contains("old decision"));
}

#[tokio::test]
async fn update_section_not_found_returns_error() {
    let (_dir, writer) = make_writer();
    let path = writer.memory_dir.join(MEMORY_FILE);
    tokio::fs::write(&path, "## Existing\ncontent\n")
        .await
        .unwrap();

    let result = writer
        .update_section(MemoryCategory::Core, "NonExistent", "data")
        .await;
    assert!(result.is_err());
}

// ── delete_section ──

#[tokio::test]
async fn delete_section_removes_section() {
    let (_dir, writer) = make_writer();

    let initial = "## Keep\n\nkeep this\n\n## Remove\n\nremove this\n";
    let path = writer.memory_dir.join(MEMORY_FILE);
    tokio::fs::write(&path, initial).await.unwrap();

    writer
        .delete_section(MemoryCategory::Core, "Remove")
        .await
        .unwrap();

    let content = writer.read_core_memory().await.unwrap().unwrap();
    assert!(content.contains("keep this"));
    assert!(!content.contains("Remove"));
    assert!(!content.contains("remove this"));
}

#[tokio::test]
async fn delete_section_not_found_returns_error() {
    let (_dir, writer) = make_writer();
    let path = writer.memory_dir.join(MEMORY_FILE);
    tokio::fs::write(&path, "## A\ncontent\n").await.unwrap();

    let result = writer.delete_section(MemoryCategory::Core, "B").await;
    assert!(result.is_err());
}

// ── list_sections ──

#[tokio::test]
async fn list_sections_returns_headings() {
    let (_dir, writer) = make_writer();
    let path = writer.memory_dir.join(EXPERIENCE_FILE);
    tokio::fs::write(&path, "## Tips\ncontent\n\n## Patterns\ncontent\n")
        .await
        .unwrap();

    let headings = writer
        .list_sections(MemoryCategory::Experience)
        .await
        .unwrap();
    assert_eq!(headings, vec!["Tips", "Patterns"]);
}

#[tokio::test]
async fn list_sections_empty_file() {
    let (_dir, writer) = make_writer();
    let headings = writer.list_sections(MemoryCategory::Core).await.unwrap();
    assert!(headings.is_empty());
}

// ── daily cannot be edited/deleted ──

#[tokio::test]
async fn update_daily_returns_error() {
    let (_dir, writer) = make_writer();
    let result = writer.update_section(MemoryCategory::Daily, "X", "Y").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn delete_daily_returns_error() {
    let (_dir, writer) = make_writer();
    let result = writer.delete_section(MemoryCategory::Daily, "X").await;
    assert!(result.is_err());
}

// ── rewrite_file ──

#[tokio::test]
async fn rewrite_file_replaces_entire_content() {
    let (_dir, writer) = make_writer();
    let path = writer.memory_dir.join(MEMORY_FILE);
    tokio::fs::write(&path, "old content").await.unwrap();

    writer
        .rewrite_file(MemoryCategory::Core, "## Compacted\n\nnew content\n")
        .await
        .unwrap();

    let content = writer.read_core_memory().await.unwrap().unwrap();
    assert!(!content.contains("old content"));
    assert!(content.contains("## Compacted"));
    assert!(content.contains("new content"));
}

#[tokio::test]
async fn rewrite_daily_returns_error() {
    let (_dir, writer) = make_writer();
    let result = writer.rewrite_file(MemoryCategory::Daily, "x").await;
    assert!(result.is_err());
}

// ── file_size ──

#[tokio::test]
async fn file_size_returns_zero_for_missing() {
    let (_dir, writer) = make_writer();
    assert_eq!(writer.file_size(MemoryCategory::Core).await, 0);
}

#[tokio::test]
async fn file_size_returns_correct_size() {
    let (_dir, writer) = make_writer();
    let path = writer.memory_dir.join(MEMORY_FILE);
    tokio::fs::write(&path, "hello world").await.unwrap();
    assert_eq!(writer.file_size(MemoryCategory::Core).await, 11);
}
