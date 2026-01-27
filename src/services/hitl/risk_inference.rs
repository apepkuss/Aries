//! L2 系统推断规则
//!
//! 基于工具名称和参数自动推断风险级别，作为兜底机制。
//!
//! # 推断规则
//!
//! L2 系统推断只产生两种结果：
//! - `Low`: 默认级别，适用于大多数读取操作
//! - `High`: 匹配高风险关键词的操作
//!
//! 注意：L2 推断**不会产生 Safe 级别**，Safe 只能通过 L3 运行时信任获得。

use super::types::RiskLevel;

/// 高风险关键词列表
///
/// 包含可能导致数据丢失、外发数据、系统变更的操作
const HIGH_RISK_KEYWORDS: &[&str] = &[
    // 删除相关
    "delete",
    "remove",
    "drop",
    "truncate",
    "purge",
    "clear",
    "erase",
    "destroy",
    // 发送相关
    "send",
    "post",
    "submit",
    "publish",
    "broadcast",
    "emit",
    "notify",
    "email",
    "mail",
    "message",
    "sms",
    // 执行相关
    "execute",
    "run",
    "exec",
    "shell",
    "command",
    "bash",
    "script",
    "eval",
    "spawn",
    // 写入相关
    "write",
    "create",
    "insert",
    "update",
    "modify",
    "edit",
    "save",
    "put",
    "set",
    "store",
    // 部署相关
    "push",
    "deploy",
    "release",
    "publish",
    "upload",
    "commit",
    "merge",
    // 支付相关
    "pay",
    "transfer",
    "charge",
    "refund",
    "purchase",
    "buy",
    "sell",
    "invoice",
    // 权限相关
    "grant",
    "revoke",
    "permission",
    "role",
    "admin",
    // 网络相关
    "request",
    "fetch",
    "download",
    "api",
    // 数据库相关
    "query",
    "sql",
    "database",
    "db",
    "migrate",
];

/// 基于工具名称推断风险级别
///
/// # 参数
/// - `tool_name`: 工具名称（支持 MCP 格式如 `mcp__server__tool`）
///
/// # 返回
/// - `RiskLevel::High`: 如果工具名称包含高风险关键词
/// - `RiskLevel::Low`: 默认情况
pub fn infer_risk_from_name(tool_name: &str) -> RiskLevel {
    let name = tool_name.to_lowercase();

    // 提取实际的工具名称（处理 MCP 格式）
    let actual_name = extract_tool_name(&name);

    // 检查高风险关键词
    for keyword in HIGH_RISK_KEYWORDS {
        if actual_name.contains(keyword) {
            return RiskLevel::High;
        }
    }

    RiskLevel::Low
}

/// 从完整工具名称中提取实际名称
///
/// 处理 MCP 格式：`mcp__server__tool_name` -> `tool_name`
fn extract_tool_name(full_name: &str) -> &str {
    // 处理 MCP 格式
    if full_name.starts_with("mcp__") {
        // 找到最后一个 __ 后的部分
        if let Some(pos) = full_name.rfind("__") {
            return &full_name[pos + 2..];
        }
    }

    // 处理 skill 格式：`skill__name__tool` -> `tool`
    if full_name.starts_with("skill__")
        && let Some(pos) = full_name.rfind("__")
    {
        return &full_name[pos + 2..];
    }

    full_name
}

/// 基于参数值推断额外的风险因素
///
/// # 参数
/// - `tool_name`: 工具名称
/// - `args`: 工具参数
///
/// # 返回
/// 推断出的风险因素列表
pub fn infer_risk_factors(tool_name: &str, args: &serde_json::Value) -> Vec<InferredRiskFactor> {
    let mut factors = Vec::new();

    // 检查是否涉及外部资源
    if has_external_resource(args) {
        factors.push(InferredRiskFactor {
            code: "external_resource".to_string(),
            description: "操作涉及外部资源".to_string(),
            risk_increase: false, // 仅作为信息提示
        });
    }

    // 检查是否涉及敏感路径
    if has_sensitive_path(args) {
        factors.push(InferredRiskFactor {
            code: "sensitive_path".to_string(),
            description: "操作涉及敏感系统路径".to_string(),
            risk_increase: true,
        });
    }

    // 检查是否有大量数据
    if has_large_data(args) {
        factors.push(InferredRiskFactor {
            code: "large_data".to_string(),
            description: "操作涉及大量数据".to_string(),
            risk_increase: false,
        });
    }

    // 检查邮件相关
    if (tool_name.contains("email") || tool_name.contains("mail"))
        && let Some(recipients) = extract_email_recipients(args)
        && has_external_email(&recipients)
    {
        factors.push(InferredRiskFactor {
            code: "external_recipient".to_string(),
            description: "收件人包含外部邮箱".to_string(),
            risk_increase: true,
        });
    }

    factors
}

/// 推断出的风险因素
#[derive(Debug, Clone)]
pub struct InferredRiskFactor {
    /// 因素代码
    pub code: String,
    /// 描述
    pub description: String,
    /// 是否增加风险
    pub risk_increase: bool,
}

/// 检查参数是否包含外部资源引用
fn has_external_resource(args: &serde_json::Value) -> bool {
    let json_str = args.to_string().to_lowercase();

    // 检查 URL
    json_str.contains("http://")
        || json_str.contains("https://")
        || json_str.contains("ftp://")
        || json_str.contains("s3://")
}

/// 检查参数是否包含敏感路径
fn has_sensitive_path(args: &serde_json::Value) -> bool {
    let json_str = args.to_string().to_lowercase();

    let sensitive_patterns = [
        "/etc/",
        "/var/",
        "/usr/",
        "/root/",
        "/home/",
        "~/.ssh",
        "~/.aws",
        "~/.config",
        ".env",
        "credentials",
        "secrets",
        "private",
        "password",
        "token",
        "apikey",
        "api_key",
    ];

    sensitive_patterns
        .iter()
        .any(|pattern| json_str.contains(pattern))
}

/// 检查是否涉及大量数据
fn has_large_data(args: &serde_json::Value) -> bool {
    // 检查 JSON 字符串长度
    let json_str = args.to_string();
    if json_str.len() > 10000 {
        return true;
    }

    // 检查数组长度
    if let Some(arr) = args.as_array()
        && arr.len() > 100
    {
        return true;
    }

    // 递归检查对象中的数组
    if let Some(obj) = args.as_object() {
        for (_, value) in obj {
            if let Some(arr) = value.as_array()
                && arr.len() > 100
            {
                return true;
            }
        }
    }

    false
}

/// 提取邮件收件人
fn extract_email_recipients(args: &serde_json::Value) -> Option<Vec<String>> {
    let mut recipients = Vec::new();

    // 检查常见的收件人字段
    let recipient_fields = ["to", "cc", "bcc", "recipients", "email", "emails"];

    if let Some(obj) = args.as_object() {
        for field in recipient_fields {
            if let Some(value) = obj.get(field) {
                if let Some(s) = value.as_str() {
                    recipients.push(s.to_string());
                } else if let Some(arr) = value.as_array() {
                    for item in arr {
                        if let Some(s) = item.as_str() {
                            recipients.push(s.to_string());
                        }
                    }
                }
            }
        }
    }

    if recipients.is_empty() {
        None
    } else {
        Some(recipients)
    }
}

/// 检查是否包含外部邮箱
fn has_external_email(recipients: &[String]) -> bool {
    // 定义内部邮箱域名列表（可以从配置中读取）
    let internal_domains = ["company.com", "internal.com", "corp.local"];

    for recipient in recipients {
        if let Some(domain) = recipient.split('@').next_back() {
            let domain_lower = domain.to_lowercase();
            if !internal_domains.iter().any(|d| domain_lower.ends_with(d)) {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_infer_risk_from_name_high_risk() {
        // 删除操作
        assert_eq!(infer_risk_from_name("delete_file"), RiskLevel::High);
        assert_eq!(infer_risk_from_name("remove_user"), RiskLevel::High);
        assert_eq!(infer_risk_from_name("drop_database"), RiskLevel::High);

        // 发送操作
        assert_eq!(infer_risk_from_name("send_email"), RiskLevel::High);
        assert_eq!(infer_risk_from_name("post_message"), RiskLevel::High);
        assert_eq!(infer_risk_from_name("publish_article"), RiskLevel::High);

        // 执行操作
        assert_eq!(infer_risk_from_name("execute_command"), RiskLevel::High);
        assert_eq!(infer_risk_from_name("run_script"), RiskLevel::High);
        assert_eq!(infer_risk_from_name("shell_exec"), RiskLevel::High);

        // 写入操作
        assert_eq!(infer_risk_from_name("write_file"), RiskLevel::High);
        assert_eq!(infer_risk_from_name("create_user"), RiskLevel::High);
        assert_eq!(infer_risk_from_name("update_config"), RiskLevel::High);
    }

    #[test]
    fn test_infer_risk_from_name_low_risk() {
        // 读取操作
        assert_eq!(infer_risk_from_name("read_file"), RiskLevel::Low);
        assert_eq!(infer_risk_from_name("get_user"), RiskLevel::Low);
        assert_eq!(infer_risk_from_name("list_items"), RiskLevel::Low);
        assert_eq!(infer_risk_from_name("find_records"), RiskLevel::Low);
        assert_eq!(infer_risk_from_name("search"), RiskLevel::Low);
    }

    #[test]
    fn test_infer_risk_from_name_mcp_format() {
        // MCP 格式工具名
        assert_eq!(
            infer_risk_from_name("mcp__email__send_email"),
            RiskLevel::High
        );
        assert_eq!(
            infer_risk_from_name("mcp__github__delete_branch"),
            RiskLevel::High
        );
        assert_eq!(
            infer_risk_from_name("mcp__filesystem__read_file"),
            RiskLevel::Low
        );
    }

    #[test]
    fn test_has_external_resource() {
        assert!(has_external_resource(&json!({
            "url": "https://example.com/api"
        })));

        assert!(has_external_resource(&json!({
            "source": "s3://bucket/key"
        })));

        assert!(!has_external_resource(&json!({
            "path": "/local/file.txt"
        })));
    }

    #[test]
    fn test_has_sensitive_path() {
        assert!(has_sensitive_path(&json!({
            "path": "/etc/passwd"
        })));

        assert!(has_sensitive_path(&json!({
            "file": "~/.ssh/id_rsa"
        })));

        assert!(has_sensitive_path(&json!({
            "config": ".env.production"
        })));

        assert!(!has_sensitive_path(&json!({
            "path": "/tmp/test.txt"
        })));
    }

    #[test]
    fn test_has_large_data() {
        // 大数组
        let large_array: Vec<i32> = (0..200).collect();
        assert!(has_large_data(&json!({ "items": large_array })));

        // 小数组
        assert!(!has_large_data(&json!({ "items": [1, 2, 3] })));
    }

    #[test]
    fn test_extract_email_recipients() {
        let args = json!({
            "to": ["alice@example.com", "bob@example.com"],
            "cc": "charlie@company.com",
            "subject": "Test"
        });

        let recipients = extract_email_recipients(&args).unwrap();
        assert_eq!(recipients.len(), 3);
        assert!(recipients.contains(&"alice@example.com".to_string()));
    }

    #[test]
    fn test_has_external_email() {
        // 外部邮箱
        assert!(has_external_email(&["external@gmail.com".to_string()]));

        // 内部邮箱
        assert!(!has_external_email(&["user@company.com".to_string()]));

        // 混合
        assert!(has_external_email(&[
            "internal@company.com".to_string(),
            "external@gmail.com".to_string(),
        ]));
    }

    #[test]
    fn test_infer_risk_factors() {
        let factors = infer_risk_factors(
            "send_email",
            &json!({
                "to": ["external@gmail.com"],
                "subject": "Test"
            }),
        );

        assert!(!factors.is_empty());
        assert!(factors.iter().any(|f| f.code == "external_recipient"));
    }
}
