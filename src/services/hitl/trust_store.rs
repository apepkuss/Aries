//! L3 信任存储
//!
//! 存储用户批准记录，用于运行时风险调整。
//!
//! # 核心概念
//!
//! - `TrustKey`: 调用级信任追踪键，区分不同"行为模式"
//! - `TrustRecord`: 信任记录，包含批准次数和最后批准时间
//! - `TrustStore`: 信任存储，管理所有信任记录

use std::hash::{Hash, Hasher};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{config::RuntimeLearningConfig, types::RiskLevel};

/// L3 信任键：用于调用级信任追踪
///
/// 核心思想：区分不同"行为模式"，避免工具级信任"溢出"
///
/// 例如：`delete_file("/tmp/cache")` vs `delete_file("/etc/passwd")`
/// 虽然是同一工具，但行为模式完全不同，信任不应传递
#[derive(Debug, Clone, Eq, Serialize, Deserialize)]
pub struct TrustKey {
    /// 工具名称
    pub tool_name: String,
    /// 执行环境
    pub environment: Environment,
    /// 风险指纹：hash(subset(args) + scope + target)
    /// 只包含影响风险判断的参数子集
    pub risk_fingerprint: String,
}

impl TrustKey {
    /// 创建新的信任键
    pub fn new(tool_name: String, environment: Environment, risk_fingerprint: String) -> Self {
        Self {
            tool_name,
            environment,
            risk_fingerprint,
        }
    }

    /// 从工具调用构建信任键
    ///
    /// # 参数提取策略
    /// 不同工具类型提取不同的参数子集：
    /// - 文件操作：提取路径前缀（如 `/tmp/` vs `/etc/`）
    /// - 邮件发送：提取收件人域名（如 `@internal.com` vs `@external.com`）
    /// - Shell 命令：提取命令类型（如 `ls` vs `rm -rf`）
    pub fn from_tool_call(tool_name: &str, args: &serde_json::Value) -> Self {
        let environment = Self::detect_environment(tool_name, args);
        let risk_fingerprint = Self::compute_fingerprint(tool_name, args);

        Self {
            tool_name: tool_name.to_string(),
            environment,
            risk_fingerprint,
        }
    }

    /// 检测执行环境
    fn detect_environment(tool_name: &str, args: &serde_json::Value) -> Environment {
        let args_str = args.to_string().to_lowercase();
        let tool_lower = tool_name.to_lowercase();

        // 外部服务检测
        if tool_lower.contains("api")
            || tool_lower.contains("webhook")
            || tool_lower.contains("payment")
            || tool_lower.contains("stripe")
            || tool_lower.contains("paypal")
        {
            return Environment::External;
        }

        // 网络资源检测
        if args_str.contains("http://")
            || args_str.contains("https://")
            || args_str.contains("ftp://")
            || tool_lower.contains("request")
            || tool_lower.contains("fetch")
            || tool_lower.contains("download")
            || tool_lower.contains("upload")
        {
            return Environment::Network;
        }

        // 默认为本地
        Environment::Local
    }

    /// 计算风险指纹
    fn compute_fingerprint(tool_name: &str, args: &serde_json::Value) -> String {
        let mut hasher = Sha256::new();

        // 添加工具名称
        hasher.update(tool_name.as_bytes());

        // 提取关键参数并排序
        let key_params = Self::extract_key_params(tool_name, args);
        for (key, value) in key_params {
            hasher.update(key.as_bytes());
            hasher.update(value.as_bytes());
        }

        // 返回前 16 位十六进制
        let result = hasher.finalize();
        hex::encode(&result[..8])
    }

    /// 提取影响风险判断的关键参数
    fn extract_key_params(tool_name: &str, args: &serde_json::Value) -> Vec<(String, String)> {
        let mut params = Vec::new();
        let tool_lower = tool_name.to_lowercase();

        if let Some(obj) = args.as_object() {
            // 文件操作：提取路径前缀
            if (tool_lower.contains("file")
                || tool_lower.contains("write")
                || tool_lower.contains("delete")
                || tool_lower.contains("read"))
                && let Some(path) = obj.get("path").and_then(|v| v.as_str())
            {
                let prefix = Self::extract_path_prefix(path);
                params.push(("path_prefix".to_string(), prefix));
            }

            // 邮件：提取收件人域名
            if (tool_lower.contains("email") || tool_lower.contains("mail"))
                && let Some(to) = obj.get("to")
            {
                let domains = Self::extract_email_domains(to);
                params.push(("recipient_domains".to_string(), domains.join(",")));
            }

            // Shell 命令：提取命令名
            if (tool_lower.contains("shell")
                || tool_lower.contains("exec")
                || tool_lower.contains("command"))
                && let Some(cmd) = obj.get("command").and_then(|v| v.as_str())
            {
                let cmd_name = Self::extract_command_name(cmd);
                params.push(("command_name".to_string(), cmd_name));
            }

            // HTTP 请求：提取方法和域名
            if tool_lower.contains("request") || tool_lower.contains("fetch") {
                if let Some(method) = obj.get("method").and_then(|v| v.as_str()) {
                    params.push(("method".to_string(), method.to_uppercase()));
                }
                if let Some(url) = obj.get("url").and_then(|v| v.as_str())
                    && let Some(domain) = Self::extract_domain_from_url(url)
                {
                    params.push(("domain".to_string(), domain));
                }
            }
        }

        // 排序以确保一致性
        params.sort_by(|a, b| a.0.cmp(&b.0));
        params
    }

    /// 提取路径前缀（第一级目录）
    fn extract_path_prefix(path: &str) -> String {
        // 提取第一级目录作为前缀
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if !parts.is_empty() {
            format!("/{}", parts[0])
        } else {
            "/".to_string()
        }
    }

    /// 提取邮件域名
    fn extract_email_domains(to: &serde_json::Value) -> Vec<String> {
        let mut domains = Vec::new();

        let emails: Vec<&str> = if let Some(s) = to.as_str() {
            vec![s]
        } else if let Some(arr) = to.as_array() {
            arr.iter().filter_map(|v| v.as_str()).collect()
        } else {
            vec![]
        };

        for email in emails {
            if let Some(domain) = email.split('@').next_back() {
                domains.push(domain.to_lowercase());
            }
        }

        domains.sort();
        domains.dedup();
        domains
    }

    /// 提取命令名称
    fn extract_command_name(cmd: &str) -> String {
        cmd.split_whitespace()
            .next()
            .map(|s| s.split('/').next_back().unwrap_or(s))
            .unwrap_or("")
            .to_string()
    }

    /// 从 URL 提取域名
    fn extract_domain_from_url(url: &str) -> Option<String> {
        let url = url
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        url.split('/').next().map(|s| s.to_lowercase())
    }
}

impl PartialEq for TrustKey {
    fn eq(&self, other: &Self) -> bool {
        self.tool_name == other.tool_name
            && self.environment == other.environment
            && self.risk_fingerprint == other.risk_fingerprint
    }
}

impl Hash for TrustKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.tool_name.hash(state);
        self.environment.hash(state);
        self.risk_fingerprint.hash(state);
    }
}

/// 执行环境分类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Environment {
    /// 本地资源（文件系统、进程等）
    Local,
    /// 网络资源（HTTP 请求、数据库等）
    Network,
    /// 外部服务（第三方 API、支付网关等）
    External,
}

/// 信任记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustRecord {
    /// 累计批准次数
    pub approval_count: usize,
    /// 首次批准时间
    pub first_approved_at: DateTime<Utc>,
    /// 最后批准时间
    pub last_approved_at: DateTime<Utc>,
    /// 信任过期时间（None 表示永久）
    pub expires_at: Option<DateTime<Utc>>,
}

impl TrustRecord {
    /// 创建新的信任记录
    pub fn new(trust_duration_secs: u64) -> Self {
        let now = Utc::now();
        let expires_at = if trust_duration_secs == 0 {
            None
        } else {
            Some(now + chrono::Duration::seconds(trust_duration_secs as i64))
        };

        Self {
            approval_count: 1,
            first_approved_at: now,
            last_approved_at: now,
            expires_at,
        }
    }

    /// 记录新的批准
    pub fn record_approval(&mut self, trust_duration_secs: u64) {
        self.approval_count += 1;
        self.last_approved_at = Utc::now();

        // 延长过期时间
        if trust_duration_secs > 0 {
            self.expires_at =
                Some(self.last_approved_at + chrono::Duration::seconds(trust_duration_secs as i64));
        }
    }

    /// 检查是否已过期
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            Utc::now() > expires_at
        } else {
            false
        }
    }
}

/// 信任存储
#[derive(Debug)]
pub struct TrustStore {
    /// TrustKey -> TrustRecord
    records: DashMap<TrustKey, TrustRecord>,
}

impl TrustStore {
    /// 创建新的信任存储
    pub fn new() -> Self {
        Self {
            records: DashMap::new(),
        }
    }

    /// 获取批准次数
    pub fn get_approval_count(&self, key: &TrustKey) -> usize {
        self.records
            .get(key)
            .filter(|r| !r.is_expired())
            .map(|r| r.approval_count)
            .unwrap_or(0)
    }

    /// 获取信任记录
    pub fn get_record(&self, key: &TrustKey) -> Option<TrustRecord> {
        self.records
            .get(key)
            .filter(|r| !r.is_expired())
            .map(|r| r.clone())
    }

    /// 记录批准
    pub fn record_approval(&self, key: TrustKey, config: &RuntimeLearningConfig) {
        let trust_duration = config.trust_duration_secs;

        self.records
            .entry(key)
            .and_modify(|r| r.record_approval(trust_duration))
            .or_insert_with(|| TrustRecord::new(trust_duration));
    }

    /// 计算运行时风险调整
    ///
    /// 根据批准次数计算可以降低的风险级别数
    pub fn compute_adjustment(
        &self,
        key: &TrustKey,
        base_risk: RiskLevel,
        config: &RuntimeLearningConfig,
    ) -> RiskLevel {
        let approval_count = self.get_approval_count(key);
        let levels_to_reduce = config.levels_to_reduce(approval_count);

        // 逐级降低风险
        let mut current = base_risk;
        for _ in 0..levels_to_reduce {
            let next = current.decrease_one_level();
            // 不能降低到 min_level 以下
            if next < config.min_level {
                break;
            }
            current = next;
        }

        current
    }

    /// 清理过期记录
    pub fn cleanup_expired(&self) -> usize {
        let expired_keys: Vec<_> = self
            .records
            .iter()
            .filter(|r| r.is_expired())
            .map(|r| r.key().clone())
            .collect();

        let count = expired_keys.len();
        for key in expired_keys {
            self.records.remove(&key);
        }

        count
    }

    /// 获取记录总数
    pub fn count(&self) -> usize {
        self.records.len()
    }

    /// 清空所有记录
    pub fn clear(&self) {
        self.records.clear();
    }
}

impl Default for TrustStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_trust_key_from_tool_call() {
        let key = TrustKey::from_tool_call("delete_file", &json!({ "path": "/tmp/test.txt" }));

        assert_eq!(key.tool_name, "delete_file");
        assert_eq!(key.environment, Environment::Local);
        assert!(!key.risk_fingerprint.is_empty());
    }

    #[test]
    fn test_trust_key_different_paths() {
        let key1 = TrustKey::from_tool_call("delete_file", &json!({ "path": "/tmp/test.txt" }));
        let key2 = TrustKey::from_tool_call("delete_file", &json!({ "path": "/etc/passwd" }));

        // 不同路径前缀应该产生不同的指纹
        assert_ne!(key1.risk_fingerprint, key2.risk_fingerprint);
    }

    #[test]
    fn test_trust_key_same_prefix() {
        let key1 = TrustKey::from_tool_call("delete_file", &json!({ "path": "/tmp/test1.txt" }));
        let key2 = TrustKey::from_tool_call("delete_file", &json!({ "path": "/tmp/test2.txt" }));

        // 相同路径前缀应该产生相同的指纹
        assert_eq!(key1.risk_fingerprint, key2.risk_fingerprint);
    }

    #[test]
    fn test_environment_detection() {
        // 本地操作
        let key = TrustKey::from_tool_call("read_file", &json!({ "path": "/local/file.txt" }));
        assert_eq!(key.environment, Environment::Local);

        // 网络操作
        let key = TrustKey::from_tool_call("fetch", &json!({ "url": "https://example.com/api" }));
        assert_eq!(key.environment, Environment::Network);

        // 外部服务
        let key = TrustKey::from_tool_call("stripe_payment", &json!({ "amount": 100 }));
        assert_eq!(key.environment, Environment::External);
    }

    #[test]
    fn test_trust_record_creation() {
        let record = TrustRecord::new(3600);

        assert_eq!(record.approval_count, 1);
        assert!(record.expires_at.is_some());
        assert!(!record.is_expired());
    }

    #[test]
    fn test_trust_record_permanent() {
        let record = TrustRecord::new(0); // 0 表示永久

        assert_eq!(record.approval_count, 1);
        assert!(record.expires_at.is_none());
        assert!(!record.is_expired());
    }

    #[test]
    fn test_trust_store_record_approval() {
        let store = TrustStore::new();
        let key = TrustKey::from_tool_call("test_tool", &json!({}));
        let config = RuntimeLearningConfig::default();

        // 首次批准
        store.record_approval(key.clone(), &config);
        assert_eq!(store.get_approval_count(&key), 1);

        // 再次批准
        store.record_approval(key.clone(), &config);
        assert_eq!(store.get_approval_count(&key), 2);
    }

    #[test]
    fn test_trust_store_compute_adjustment() {
        let store = TrustStore::new();
        let key = TrustKey::from_tool_call("test_tool", &json!({}));
        let config = RuntimeLearningConfig {
            approvals_per_level: 3,
            trust_duration_secs: 604800,
            min_level: RiskLevel::Low,
        };

        // 0 次批准，不降级
        assert_eq!(
            store.compute_adjustment(&key, RiskLevel::High, &config),
            RiskLevel::High
        );

        // 记录 3 次批准
        for _ in 0..3 {
            store.record_approval(key.clone(), &config);
        }

        // 3 次批准，降低一级
        assert_eq!(
            store.compute_adjustment(&key, RiskLevel::High, &config),
            RiskLevel::Medium
        );

        // 再记录 3 次
        for _ in 0..3 {
            store.record_approval(key.clone(), &config);
        }

        // 6 次批准，降低两级（但受 min_level 限制）
        assert_eq!(
            store.compute_adjustment(&key, RiskLevel::High, &config),
            RiskLevel::Low
        );
    }

    #[test]
    fn test_trust_store_min_level_limit() {
        let store = TrustStore::new();
        let key = TrustKey::from_tool_call("critical_tool", &json!({}));
        let config = RuntimeLearningConfig {
            approvals_per_level: 1,
            trust_duration_secs: 604800,
            min_level: RiskLevel::Medium, // 最低只能降到 Medium
        };

        // 记录很多批准
        for _ in 0..100 {
            store.record_approval(key.clone(), &config);
        }

        // 即使批准很多次，也不能低于 min_level
        assert_eq!(
            store.compute_adjustment(&key, RiskLevel::Critical, &config),
            RiskLevel::Medium
        );
    }

    #[test]
    fn test_extract_path_prefix() {
        // 只提取第一级目录
        assert_eq!(TrustKey::extract_path_prefix("/tmp/test.txt"), "/tmp");
        assert_eq!(
            TrustKey::extract_path_prefix("/etc/nginx/nginx.conf"),
            "/etc"
        );
        assert_eq!(TrustKey::extract_path_prefix("/root"), "/root");
        assert_eq!(TrustKey::extract_path_prefix("/"), "/");
    }

    #[test]
    fn test_extract_email_domains() {
        let domains =
            TrustKey::extract_email_domains(&json!(["alice@example.com", "bob@company.com"]));
        assert_eq!(domains, vec!["company.com", "example.com"]);
    }

    #[test]
    fn test_extract_command_name() {
        assert_eq!(TrustKey::extract_command_name("ls -la"), "ls");
        assert_eq!(TrustKey::extract_command_name("/usr/bin/cat file"), "cat");
        assert_eq!(TrustKey::extract_command_name("rm -rf /"), "rm");
    }

    #[test]
    fn test_extract_domain_from_url() {
        assert_eq!(
            TrustKey::extract_domain_from_url("https://example.com/api"),
            Some("example.com".to_string())
        );
        assert_eq!(
            TrustKey::extract_domain_from_url("http://api.service.com:8080/v1"),
            Some("api.service.com:8080".to_string())
        );
    }
}
