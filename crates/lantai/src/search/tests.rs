use std::io::Write;

use super::{hybrid::rrf_merge, types::SearchQuery};
use crate::{Lantai, LantaiConfig, MockEmbedding};

const DIMS: usize = 64;

fn test_config() -> LantaiConfig {
    LantaiConfig {
        chunking: crate::config::ChunkingConfig {
            max_chunk_lines: 50,
            min_chunk_lines: 1,
        },
        ..LantaiConfig::default()
    }
}

fn write_md(dir: &std::path::Path, name: &str, content: &str) {
    let path = dir.join(name);
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(content.as_bytes()).unwrap();
}

#[tokio::test]
async fn test_index_then_search() {
    let tmp = tempfile::tempdir().unwrap();
    write_md(
        tmp.path(),
        "guide.md",
        "# Installation Guide\n\n\
         Install the software using the package manager.\n\n\
         ## Configuration\n\n\
         Edit the configuration file to set up your preferences.\n",
    );
    write_md(
        tmp.path(),
        "api.md",
        "# API Reference\n\n\
         The REST API provides endpoints for data access.\n\n\
         ## Authentication\n\n\
         Use Bearer tokens for authentication.\n",
    );

    let config = test_config();
    let mock = Box::new(MockEmbedding::new(DIMS));
    let lantai = Lantai::new_in_memory(config, Some(mock)).unwrap();

    // 索引
    let report = lantai.index(&[tmp.path().to_str().unwrap()]).await.unwrap();
    assert_eq!(report.files_added, 2);

    // 搜索
    let results = lantai.search("configuration").await.unwrap();
    assert!(!results.is_empty(), "Expected search results, got none");
}

#[test]
fn test_rrf_merge_basic() {
    let vec_results = vec![
        ("a".to_string(), 0.9),
        ("b".to_string(), 0.8),
        ("c".to_string(), 0.7),
    ];
    let fts_results = vec![
        ("b".to_string(), -1.0),
        ("a".to_string(), -2.0),
        ("d".to_string(), -3.0),
    ];

    let merged = rrf_merge(&vec_results, &fts_results, 0.6, 0.4, 60);

    // a: vec rank 0, fts rank 1 → 0.6/(60+0+1) + 0.4/(60+1+1) = 0.6/61 + 0.4/62
    // b: vec rank 1, fts rank 0 → 0.6/(60+1+1) + 0.4/(60+0+1) = 0.6/62 + 0.4/61
    // a 和 b 的分数应大于单侧出现的 c 和 d
    assert_eq!(merged[0].0, "a");
    assert_eq!(merged[1].0, "b");
    assert!(merged[0].1 > merged[2].1);
}

#[test]
fn test_rrf_merge_disjoint() {
    let vec_results = vec![("a".to_string(), 0.9), ("b".to_string(), 0.8)];
    let fts_results = vec![("c".to_string(), -1.0), ("d".to_string(), -2.0)];

    let merged = rrf_merge(&vec_results, &fts_results, 0.6, 0.4, 60);

    // 所有 4 个结果都应包含
    assert_eq!(merged.len(), 4);
    let ids: Vec<&str> = merged.iter().map(|(id, _)| id.as_str()).collect();
    assert!(ids.contains(&"a"));
    assert!(ids.contains(&"b"));
    assert!(ids.contains(&"c"));
    assert!(ids.contains(&"d"));
}

#[test]
fn test_rrf_merge_identical() {
    let vec_results = vec![("a".to_string(), 0.9), ("b".to_string(), 0.8)];
    let fts_results = vec![("a".to_string(), -1.0), ("b".to_string(), -2.0)];

    let merged = rrf_merge(&vec_results, &fts_results, 0.6, 0.4, 60);

    // 只有 2 个结果（重叠）
    assert_eq!(merged.len(), 2);

    // 分数应是两侧叠加
    let a_score = merged.iter().find(|(id, _)| id == "a").unwrap().1;
    let expected_a = 0.6 / (60.0 + 0.0 + 1.0) + 0.4 / (60.0 + 0.0 + 1.0);
    assert!((a_score - expected_a).abs() < 1e-10);
}

#[tokio::test]
async fn test_search_empty_index() {
    let config = test_config();
    let mock = Box::new(MockEmbedding::new(DIMS));
    let lantai = Lantai::new_in_memory(config, Some(mock)).unwrap();

    let results = lantai.search("anything").await.unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_search_limit() {
    let tmp = tempfile::tempdir().unwrap();
    // 创建多个 section 的文件
    let mut content = String::new();
    for i in 0..10 {
        content.push_str(&format!("## Section {i}\n\nContent for section {i}.\n\n"));
    }
    write_md(tmp.path(), "many.md", &content);

    let config = test_config();
    let mock = Box::new(MockEmbedding::new(DIMS));
    let lantai = Lantai::new_in_memory(config, Some(mock)).unwrap();

    lantai.index(&[tmp.path().to_str().unwrap()]).await.unwrap();

    // 搜索限制为 3
    let q = SearchQuery::new("section", 3);
    let results = lantai.search_with_options(&q).await.unwrap();
    assert!(
        results.len() <= 3,
        "Expected <= 3 results, got {}",
        results.len()
    );
}

#[tokio::test]
async fn test_search_weight_override() {
    let tmp = tempfile::tempdir().unwrap();
    write_md(tmp.path(), "doc.md", "# Test\n\nSome test content.\n");

    let config = test_config();
    let mock = Box::new(MockEmbedding::new(DIMS));
    let lantai = Lantai::new_in_memory(config, Some(mock)).unwrap();

    lantai.index(&[tmp.path().to_str().unwrap()]).await.unwrap();

    // 纯向量搜索
    let mut q = SearchQuery::new("test", 5);
    q.vec_weight = Some(1.0);
    q.bm25_weight = Some(0.0);
    let vec_only = lantai.search_with_options(&q).await.unwrap();

    // 纯 BM25 搜索
    q.vec_weight = Some(0.0);
    q.bm25_weight = Some(1.0);
    let bm25_only = lantai.search_with_options(&q).await.unwrap();

    // 两种模式都应能返回结果（具体结果可能不同）
    assert!(!vec_only.is_empty() || !bm25_only.is_empty());
}

// ─── BM25-only 模式测试 ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_bm25_only_index_and_search() {
    let tmp = tempfile::tempdir().unwrap();
    write_md(
        tmp.path(),
        "guide.md",
        "# Installation Guide\n\n\
         Install the software using the package manager.\n\n\
         ## Configuration\n\n\
         Edit the configuration file to set up your preferences.\n",
    );

    let config = test_config();
    // 不传入 embedding — BM25-only 模式
    let lantai = Lantai::new_in_memory(config, None).unwrap();

    assert!(!lantai.has_embedding());

    // 索引应正常工作
    let report = lantai.index(&[tmp.path().to_str().unwrap()]).await.unwrap();
    assert_eq!(report.files_added, 1);
    assert!(report.chunks_added >= 1);

    // BM25 搜索应返回结果
    let results = lantai.search("configuration").await.unwrap();
    assert!(
        !results.is_empty(),
        "BM25-only search should return results"
    );
}

#[tokio::test]
async fn test_bm25_only_empty_index() {
    let config = test_config();
    let lantai = Lantai::new_in_memory(config, None).unwrap();

    let results = lantai.search("anything").await.unwrap();
    assert!(results.is_empty());
}
