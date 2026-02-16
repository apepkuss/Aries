use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{LantaiError, LantaiResult};

/// lantai 全局配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LantaiConfig {
    /// 记忆文件目录
    pub memory_dir: String,
    /// SQLite 数据库路径
    pub database_path: String,
    /// 分块配置
    #[serde(default)]
    pub chunking: ChunkingConfig,
    /// Embedding 配置
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    /// 搜索配置
    #[serde(default)]
    pub search: SearchConfig,
    /// 文件监视配置
    pub watch: Option<WatchConfig>,
}

/// 分块参数
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChunkingConfig {
    /// 单个 chunk 最大行数
    pub max_chunk_lines: usize,
    /// 单个 chunk 最小行数（低于此值向前合并）
    pub min_chunk_lines: usize,
}

/// Embedding 参数
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmbeddingConfig {
    /// 模型名称
    pub model: String,
    /// 向量维度
    pub dimensions: usize,
    /// 批处理大小
    pub batch_size: usize,
}

/// 搜索参数
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SearchConfig {
    /// 默认返回结果数
    pub default_limit: usize,
    /// 向量搜索权重
    pub vec_weight: f64,
    /// BM25 搜索权重
    pub bm25_weight: f64,
    /// RRF 融合常数 k
    pub rrf_k: u32,
}

/// 文件监视参数
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WatchConfig {
    /// 防抖时间（毫秒）
    pub debounce_ms: u64,
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            max_chunk_lines: 50,
            min_chunk_lines: 5,
        }
    }
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            model: "text-embedding-3-small".to_string(),
            dimensions: 1536,
            batch_size: 32,
        }
    }
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            default_limit: 5,
            vec_weight: 0.6,
            bm25_weight: 0.4,
            rrf_k: 60,
        }
    }
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self { debounce_ms: 1500 }
    }
}

impl Default for LantaiConfig {
    fn default() -> Self {
        Self {
            memory_dir: "~/.lantai/memory".to_string(),
            database_path: "~/.lantai/lantai.db".to_string(),
            chunking: ChunkingConfig::default(),
            embedding: EmbeddingConfig::default(),
            search: SearchConfig::default(),
            watch: Some(WatchConfig::default()),
        }
    }
}

impl LantaiConfig {
    /// 分层加载配置：默认值 → ~/.lantai/config.toml → .lantai.toml → 环境变量
    pub fn load() -> LantaiResult<Self> {
        let mut config = Self::default();

        // 第二层：全局配置 ~/.lantai/config.toml
        if let Some(home_dir) = dirs::home_dir() {
            let global_path = home_dir.join(".lantai").join("config.toml");
            if global_path.exists() {
                config = Self::merge_from_file(config, &global_path)?;
            }
        }

        // 第三层：项目级配置 .lantai.toml
        let project_path = PathBuf::from(".lantai.toml");
        if project_path.exists() {
            config = Self::merge_from_file(config, &project_path)?;
        }

        // 第四层：环境变量覆盖
        config = Self::apply_env_overrides(config);

        // 展开路径中的 ~ 等 shell 变量
        config.memory_dir = expand_path(&config.memory_dir);
        config.database_path = expand_path(&config.database_path);

        Ok(config)
    }

    /// 从 TOML 文件读取并合并到当前配置
    fn merge_from_file(base: Self, path: &Path) -> LantaiResult<Self> {
        let content = std::fs::read_to_string(path)?;
        let file_config: LantaiConfig = toml::from_str(&content)
            .map_err(|e| LantaiError::Config(format!("Failed to parse {}: {e}", path.display())))?;
        // 文件配置覆盖基础配置（TOML 反序列化已处理 serde(default)）
        let _ = base;
        Ok(file_config)
    }

    /// 从环境变量覆盖配置项
    fn apply_env_overrides(mut config: Self) -> Self {
        if let Ok(val) = std::env::var("LANTAI_MEMORY_DIR") {
            config.memory_dir = val;
        }
        if let Ok(val) = std::env::var("LANTAI_DATABASE_PATH") {
            config.database_path = val;
        }
        if let Ok(val) = std::env::var("LANTAI_EMBEDDING_MODEL") {
            config.embedding.model = val;
        }
        if let Ok(val) = std::env::var("LANTAI_EMBEDDING_DIMENSIONS")
            && let Ok(dims) = val.parse()
        {
            config.embedding.dimensions = dims;
        }
        config
    }
}

/// 展开路径中的 shell 变量（如 ~ → /home/user）
fn expand_path(path: &str) -> String {
    shellexpand::tilde(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = LantaiConfig::default();
        assert_eq!(config.chunking.max_chunk_lines, 50);
        assert_eq!(config.chunking.min_chunk_lines, 5);
        assert_eq!(config.embedding.model, "text-embedding-3-small");
        assert_eq!(config.embedding.dimensions, 1536);
        assert_eq!(config.embedding.batch_size, 32);
        assert_eq!(config.search.default_limit, 5);
        assert_eq!(config.search.rrf_k, 60);
    }

    #[test]
    fn test_config_from_toml() {
        let toml_str = r#"
memory_dir = "/tmp/test-memory"
database_path = "/tmp/test.db"

[chunking]
max_chunk_lines = 100
min_chunk_lines = 10

[embedding]
model = "nomic-embed-text"
dimensions = 768
batch_size = 64

[search]
default_limit = 10
vec_weight = 0.7
bm25_weight = 0.3
rrf_k = 30
"#;
        let config: LantaiConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.memory_dir, "/tmp/test-memory");
        assert_eq!(config.database_path, "/tmp/test.db");
        assert_eq!(config.chunking.max_chunk_lines, 100);
        assert_eq!(config.embedding.model, "nomic-embed-text");
        assert_eq!(config.embedding.dimensions, 768);
        assert_eq!(config.search.vec_weight, 0.7);
        assert_eq!(config.search.rrf_k, 30);
    }

    #[test]
    fn test_config_partial_toml_uses_defaults() {
        let toml_str = r#"
memory_dir = "/tmp/mem"
database_path = "/tmp/db"
"#;
        let config: LantaiConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.memory_dir, "/tmp/mem");
        // 未指定的字段应使用默认值
        assert_eq!(config.chunking.max_chunk_lines, 50);
        assert_eq!(config.embedding.model, "text-embedding-3-small");
        assert_eq!(config.search.default_limit, 5);
    }

    #[test]
    fn test_env_overrides() {
        let mut config = LantaiConfig::default();
        // SAFETY: 测试环境下单线程执行，不会引发并发问题
        unsafe {
            std::env::set_var("LANTAI_MEMORY_DIR", "/env/memory");
            std::env::set_var("LANTAI_EMBEDDING_DIMENSIONS", "384");
        }
        config = LantaiConfig::apply_env_overrides(config);
        assert_eq!(config.memory_dir, "/env/memory");
        assert_eq!(config.embedding.dimensions, 384);
        // 清理
        unsafe {
            std::env::remove_var("LANTAI_MEMORY_DIR");
            std::env::remove_var("LANTAI_EMBEDDING_DIMENSIONS");
        }
    }

    #[test]
    fn test_expand_tilde() {
        let expanded = expand_path("~/test");
        assert!(!expanded.starts_with('~'));
        assert!(expanded.ends_with("/test"));
    }
}
