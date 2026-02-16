use super::{mock::MockEmbedding, traits::EmbeddingProvider};

#[tokio::test]
async fn test_mock_embedding_deterministic() {
    let mock = MockEmbedding::new(128);
    let text = "hello world";

    let v1 = mock.embed(text).await.unwrap();
    let v2 = mock.embed(text).await.unwrap();

    assert_eq!(v1, v2, "Same text should produce identical vectors");
}

#[tokio::test]
async fn test_mock_embedding_dimensions() {
    for dims in [64, 128, 384, 1536] {
        let mock = MockEmbedding::new(dims);
        let vec = mock.embed("test text").await.unwrap();
        assert_eq!(
            vec.len(),
            dims,
            "Expected {dims} dimensions, got {}",
            vec.len()
        );
    }
}

#[tokio::test]
async fn test_embed_single_delegates_to_batch() {
    let mock = MockEmbedding::new(64);

    // embed() 应与 embed_batch() 返回相同结果
    let single = mock.embed("test").await.unwrap();
    let batch = mock.embed_batch(&["test"]).await.unwrap();

    assert_eq!(batch.len(), 1);
    assert_eq!(single, batch[0]);
}

#[tokio::test]
async fn test_mock_embedding_batch() {
    let mock = MockEmbedding::new(64);
    let texts = vec!["hello", "world", "foo"];

    let results = mock.embed_batch(&texts).await.unwrap();
    assert_eq!(results.len(), 3);

    // 不同文本应产生不同向量
    assert_ne!(results[0], results[1]);
    assert_ne!(results[1], results[2]);
}

#[tokio::test]
async fn test_mock_embedding_metadata() {
    let mock = MockEmbedding::new(384);

    assert_eq!(mock.name(), "mock");
    assert_eq!(mock.model(), "mock-embedding");
    assert_eq!(mock.dimensions(), 384);
    assert_eq!(mock.max_batch_size(), 256);
}
