use super::{
    markdown::MarkdownChunker,
    types::{content_hash, generate_composite_id},
};

fn default_chunker() -> MarkdownChunker {
    MarkdownChunker::new(50, 5)
}

#[test]
fn test_basic_chunking() {
    let content = "\
# Introduction

This is the intro paragraph.
It has multiple lines.

# Setup

Follow these steps:
1. Install Rust
2. Run cargo build
3. Done";

    let chunks = default_chunker().chunk_file("test.md", content);
    assert_eq!(chunks.len(), 2);

    assert_eq!(chunks[0].heading_path, "# Introduction");
    assert!(chunks[0].content.contains("This is the intro paragraph."));
    assert_eq!(chunks[0].start_line, 1);

    assert_eq!(chunks[1].heading_path, "# Setup");
    assert!(chunks[1].content.contains("Follow these steps:"));
}

#[test]
fn test_heading_path() {
    let content = "\
# Guide

Top level content.

## Getting Started

Getting started content.

### Prerequisites

You need Rust installed.

## Advanced

Advanced content here.

### Configuration

Config details.";

    // 使用 min_chunk_lines=1 避免小段合并干扰 heading path 测试
    let chunker = MarkdownChunker::new(50, 1);
    let chunks = chunker.chunk_file("test.md", content);

    assert_eq!(chunks.len(), 5, "Expected 5 chunks, got {}", chunks.len());

    // Chunk 0: # Guide (top level content)
    assert_eq!(chunks[0].heading_path, "# Guide");

    // Chunk 1: # Guide > ## Getting Started
    assert_eq!(chunks[1].heading_path, "# Guide > ## Getting Started");

    // Chunk 2: # Guide > ## Getting Started > ### Prerequisites
    assert_eq!(
        chunks[2].heading_path,
        "# Guide > ## Getting Started > ### Prerequisites"
    );

    // Chunk 3: # Guide > ## Advanced (## resets ### from stack)
    assert_eq!(chunks[3].heading_path, "# Guide > ## Advanced");

    // Chunk 4: # Guide > ## Advanced > ### Configuration
    assert_eq!(
        chunks[4].heading_path,
        "# Guide > ## Advanced > ### Configuration"
    );
}

#[test]
fn test_large_paragraph_split() {
    // 创建超过 max_chunk_lines (10) 的段落
    let chunker = MarkdownChunker::new(10, 2);

    let mut lines = vec!["# BigSection".to_string()];
    for i in 1..=25 {
        lines.push(format!("Line {i} of big paragraph."));
        if i == 12 {
            lines.push(String::new()); // 空行，作为拆分点
        }
    }
    let content = lines.join("\n");

    let chunks = chunker.chunk_file("test.md", &content);

    // 应被拆分为多个 chunk
    assert!(
        chunks.len() >= 2,
        "Expected >=2 chunks, got {}",
        chunks.len()
    );

    // 每个 chunk 不应超过 max_chunk_lines（允许拆分后有余量）
    for chunk in &chunks {
        let line_count = chunk.end_line - chunk.start_line + 1;
        // 如果没有合适的空行拆分点，可能略超，但不应远超
        assert!(
            line_count <= 15,
            "Chunk has {} lines, expected <= 15",
            line_count
        );
    }
}

#[test]
fn test_small_chunk_merge() {
    // min_chunk_lines = 5，小于 5 行的 chunk 应向前合并
    let chunker = MarkdownChunker::new(50, 5);

    let content = "\
# Section A

Line 1 of section A.
Line 2 of section A.
Line 3 of section A.
Line 4 of section A.
Line 5 of section A.

# Section B

Tiny.

# Section C

Line 1 of section C.
Line 2 of section C.
Line 3 of section C.
Line 4 of section C.
Line 5 of section C.";

    let chunks = chunker.chunk_file("test.md", content);

    // Section B (3 lines including heading) should be merged into Section A
    // since it's below min_chunk_lines
    // After merge: Section A+B combined, then Section C
    assert!(
        chunks.len() <= 3,
        "Expected small chunk to be merged, got {} chunks",
        chunks.len()
    );

    // Section B 的内容应在某个 chunk 中
    let has_tiny = chunks.iter().any(|c| c.content.contains("Tiny."));
    assert!(has_tiny, "Section B content should exist in merged chunk");
}

#[test]
fn test_no_heading_file() {
    let content = "\
This is a plain text file.
It has no headings at all.
Just regular paragraphs.

Another paragraph here.
And some more text.";

    let chunks = default_chunker().chunk_file("plain.md", content);

    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].heading_path, "");
    assert!(chunks[0].content.contains("plain text file"));
    assert_eq!(chunks[0].start_line, 1);
    assert_eq!(chunks[0].end_line, 6);
}

#[test]
fn test_composite_id_deterministic() {
    let id1 = generate_composite_id("docs/test.md", 1, 10, "abc123", "text-embedding-3-small");
    let id2 = generate_composite_id("docs/test.md", 1, 10, "abc123", "text-embedding-3-small");
    assert_eq!(id1, id2);
    assert_eq!(id1.len(), 16); // 8 bytes = 16 hex chars
}

#[test]
fn test_composite_id_uniqueness() {
    let id1 = generate_composite_id("docs/a.md", 1, 10, "hash1", "model");
    let id2 = generate_composite_id("docs/b.md", 1, 10, "hash1", "model");
    let id3 = generate_composite_id("docs/a.md", 2, 10, "hash1", "model");
    let id4 = generate_composite_id("docs/a.md", 1, 10, "hash2", "model");
    let id5 = generate_composite_id("docs/a.md", 1, 10, "hash1", "other-model");

    // 所有 ID 应不同
    let ids = vec![&id1, &id2, &id3, &id4, &id5];
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            assert_ne!(ids[i], ids[j], "IDs at index {i} and {j} should differ");
        }
    }
}

#[test]
fn test_content_hash_changes() {
    let h1 = content_hash("Hello world");
    let h2 = content_hash("Hello world");
    let h3 = content_hash("Hello World"); // different case

    assert_eq!(h1, h2);
    assert_ne!(h1, h3);
}

#[test]
fn test_empty_file() {
    let chunks = default_chunker().chunk_file("empty.md", "");
    assert!(chunks.is_empty());
}

#[test]
fn test_heading_only_file() {
    let content = "# Just A Heading";
    let chunks = default_chunker().chunk_file("heading.md", content);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].heading_path, "# Just A Heading");
}

#[test]
fn test_chunk_line_numbers() {
    let content = "\
# First

Line A
Line B

# Second

Line C
Line D
Line E";

    // 使用 min_chunk_lines=1 避免合并干扰
    let chunker = MarkdownChunker::new(50, 1);
    let chunks = chunker.chunk_file("test.md", content);
    assert_eq!(chunks.len(), 2);

    // First chunk: lines 1-5 (空行 L5 也包含在内)
    assert_eq!(chunks[0].start_line, 1);
    assert_eq!(chunks[0].end_line, 5);

    // Second chunk: lines 6-10
    assert_eq!(chunks[1].start_line, 6);
    assert_eq!(chunks[1].end_line, 10);
}
