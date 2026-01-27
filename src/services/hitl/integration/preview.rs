//! 操作预览构建器
//!
//! 根据工具类型和参数构建适当的操作预览，
//! 用于在 HITL 请求中向用户展示即将执行的操作。

use std::collections::HashMap;

use crate::services::hitl::types::{
    FileOperationPreview, FileOperationType, GenericPreview, HttpRequestPreview, OperationPreview,
    ShellCommandPreview,
};

/// 操作预览构建器
pub struct PreviewBuilder;

impl PreviewBuilder {
    /// 根据工具名称和参数构建预览
    ///
    /// 自动检测工具类型并构建相应的预览格式
    pub fn build(tool_name: &str, args: &serde_json::Value) -> OperationPreview {
        // 尝试识别特定类型的工具
        if Self::is_file_operation(tool_name)
            && let Some(preview) = Self::build_file_preview(tool_name, args)
        {
            return preview;
        }

        if Self::is_shell_command(tool_name)
            && let Some(preview) = Self::build_shell_preview(tool_name, args)
        {
            return preview;
        }

        if Self::is_http_request(tool_name)
            && let Some(preview) = Self::build_http_preview(tool_name, args)
        {
            return preview;
        }

        // 默认使用通用预览
        Self::build_generic_preview(tool_name, args)
    }

    /// 检查是否是文件操作工具
    fn is_file_operation(tool_name: &str) -> bool {
        let file_keywords = [
            "file",
            "write",
            "read",
            "delete",
            "remove",
            "copy",
            "move",
            "rename",
            "mkdir",
            "rmdir",
            "create_file",
            "edit_file",
            "filesystem",
        ];

        let lower_name = tool_name.to_lowercase();
        file_keywords.iter().any(|k| lower_name.contains(k))
    }

    /// 检查是否是 Shell 命令工具
    fn is_shell_command(tool_name: &str) -> bool {
        let shell_keywords = [
            "shell",
            "bash",
            "exec",
            "execute",
            "run_command",
            "command",
            "terminal",
            "sh",
        ];

        let lower_name = tool_name.to_lowercase();
        shell_keywords.iter().any(|k| lower_name.contains(k))
    }

    /// 检查是否是 HTTP 请求工具
    fn is_http_request(tool_name: &str) -> bool {
        let http_keywords = [
            "http", "fetch", "request", "api", "curl", "get", "post", "put", "patch",
        ];

        let lower_name = tool_name.to_lowercase();
        // 避免匹配 "delete" 到文件删除
        http_keywords.iter().any(|k| lower_name.contains(k))
    }

    /// 构建文件操作预览
    fn build_file_preview(tool_name: &str, args: &serde_json::Value) -> Option<OperationPreview> {
        let lower_name = tool_name.to_lowercase();

        // 确定操作类型
        let operation = if lower_name.contains("write") || lower_name.contains("create") {
            FileOperationType::Create
        } else if lower_name.contains("edit") || lower_name.contains("modify") {
            FileOperationType::Modify
        } else if lower_name.contains("delete") || lower_name.contains("remove") {
            FileOperationType::Delete
        } else if lower_name.contains("copy") {
            FileOperationType::Copy
        } else if lower_name.contains("move") || lower_name.contains("rename") {
            FileOperationType::Move
        } else {
            // 对于 read 操作，使用 Modify 作为默认值（或者我们可以返回 None）
            return None;
        };

        // 提取路径
        let path = args
            .get("path")
            .or_else(|| args.get("file_path"))
            .or_else(|| args.get("file"))
            .or_else(|| args.get("target"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        // 提取内容预览（用于写入）
        let content_preview = args
            .get("content")
            .or_else(|| args.get("data"))
            .and_then(|v| v.as_str())
            .map(|s| {
                if s.len() > 200 {
                    format!("{}... (截断)", &s[..200])
                } else {
                    s.to_string()
                }
            });

        // 计算内容大小
        let size_bytes = args
            .get("content")
            .or_else(|| args.get("data"))
            .and_then(|v| v.as_str())
            .map(|s| s.len() as u64);

        Some(OperationPreview::FileOperation(FileOperationPreview {
            operation,
            path,
            content_preview,
            size_bytes,
            affected_files_count: Some(1),
        }))
    }

    /// 构建 Shell 命令预览
    fn build_shell_preview(_tool_name: &str, args: &serde_json::Value) -> Option<OperationPreview> {
        // 提取命令
        let command = args
            .get("command")
            .or_else(|| args.get("cmd"))
            .or_else(|| args.get("script"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        // 提取工作目录
        let working_directory = args
            .get("cwd")
            .or_else(|| args.get("working_dir"))
            .or_else(|| args.get("directory"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // 提取环境变量
        let environment = args.get("env").and_then(|v| {
            if let Some(obj) = v.as_object() {
                let mut env = HashMap::new();
                for (k, v) in obj {
                    if let Some(s) = v.as_str() {
                        env.insert(k.clone(), s.to_string());
                    }
                }
                if env.is_empty() { None } else { Some(env) }
            } else {
                None
            }
        });

        // 分析估计影响
        let estimated_impact = Self::analyze_shell_impact(&command);

        Some(OperationPreview::ShellCommand(ShellCommandPreview {
            command,
            working_directory,
            environment,
            estimated_impact,
        }))
    }

    /// 分析 Shell 命令的潜在影响
    fn analyze_shell_impact(command: &str) -> String {
        let mut impacts = Vec::new();
        let lower_cmd = command.to_lowercase();

        // 检测危险命令
        if lower_cmd.contains("rm ") || lower_cmd.contains("rmdir") {
            impacts.push("删除文件/目录");
        }
        if lower_cmd.contains("chmod") {
            impacts.push("修改文件权限");
        }
        if lower_cmd.contains("chown") {
            impacts.push("修改文件所有者");
        }
        if lower_cmd.contains("sudo") {
            impacts.push("管理员权限");
        }
        if lower_cmd.contains("mv ") {
            impacts.push("移动/重命名");
        }
        if lower_cmd.contains("cp ") {
            impacts.push("复制文件");
        }
        if lower_cmd.contains("kill") || lower_cmd.contains("pkill") {
            impacts.push("终止进程");
        }
        if lower_cmd.contains("curl") || lower_cmd.contains("wget") || lower_cmd.contains("fetch") {
            impacts.push("网络请求");
        }
        if lower_cmd.contains("> ") || lower_cmd.contains(">>") {
            impacts.push("写入文件");
        }
        if lower_cmd.contains("| ") {
            impacts.push("管道操作");
        }
        if lower_cmd.contains("git push") {
            impacts.push("推送代码");
        }
        if lower_cmd.contains("npm publish") || lower_cmd.contains("cargo publish") {
            impacts.push("发布包");
        }
        if lower_cmd.contains("docker") {
            impacts.push("Docker 操作");
        }

        if impacts.is_empty() {
            "执行 Shell 命令".to_string()
        } else {
            impacts.join(", ")
        }
    }

    /// 构建 HTTP 请求预览
    fn build_http_preview(tool_name: &str, args: &serde_json::Value) -> Option<OperationPreview> {
        // 提取 URL
        let url = args
            .get("url")
            .or_else(|| args.get("endpoint"))
            .or_else(|| args.get("uri"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        // 确定 HTTP 方法
        let method = args
            .get("method")
            .and_then(|v| v.as_str())
            .map(|s| s.to_uppercase())
            .unwrap_or_else(|| {
                // 从工具名称推断
                let lower_name = tool_name.to_lowercase();
                if lower_name.contains("post") {
                    "POST".to_string()
                } else if lower_name.contains("put") {
                    "PUT".to_string()
                } else if lower_name.contains("delete") {
                    "DELETE".to_string()
                } else if lower_name.contains("patch") {
                    "PATCH".to_string()
                } else {
                    "GET".to_string()
                }
            });

        // 提取请求头
        let headers = args.get("headers").and_then(|v| {
            if let Some(obj) = v.as_object() {
                let mut headers = HashMap::new();
                for (k, v) in obj {
                    if let Some(s) = v.as_str() {
                        headers.insert(k.clone(), s.to_string());
                    }
                }
                if headers.is_empty() {
                    None
                } else {
                    Some(headers)
                }
            } else {
                None
            }
        });

        // 提取请求体（转换为字符串）
        let body = args.get("body").or_else(|| args.get("data")).map(|v| {
            if let Some(s) = v.as_str() {
                s.to_string()
            } else {
                serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
            }
        });

        // 检查是否为外部请求
        let is_external = !url.contains("localhost") && !url.contains("127.0.0.1");

        Some(OperationPreview::HttpRequest(HttpRequestPreview {
            method,
            url,
            headers,
            body,
            is_external,
        }))
    }

    /// 构建通用预览
    fn build_generic_preview(tool_name: &str, args: &serde_json::Value) -> OperationPreview {
        // 从工具名称生成描述
        let description = Self::tool_name_to_description(tool_name);

        // 转换参数为 details
        let details = if let Some(obj) = args.as_object() {
            obj.iter()
                .map(|(k, v)| {
                    let value = if let Some(s) = v.as_str() {
                        if s.len() > 200 {
                            serde_json::json!(format!("{}... (truncated)", &s[..200]))
                        } else {
                            v.clone()
                        }
                    } else {
                        v.clone()
                    };
                    (k.clone(), value)
                })
                .collect()
        } else {
            HashMap::new()
        };

        OperationPreview::Generic(GenericPreview {
            title: Self::format_tool_name(tool_name),
            description,
            details,
        })
    }

    /// 将工具名称转换为描述
    fn tool_name_to_description(tool_name: &str) -> String {
        // 解析 MCP 工具名称格式: mcp__server__tool
        if tool_name.starts_with("mcp__") {
            let parts: Vec<&str> = tool_name.splitn(3, "__").collect();
            if parts.len() >= 3 {
                return format!("通过 {} 服务调用 {} 工具", parts[1], parts[2]);
            }
        }

        // 解析内部工具名称格式: internal__tool
        if tool_name.starts_with("internal__") {
            let tool = tool_name.trim_start_matches("internal__");
            return format!("执行内部工具: {}", tool);
        }

        format!("执行工具: {}", tool_name)
    }

    /// 格式化工具名称为标题
    fn format_tool_name(tool_name: &str) -> String {
        // 解析 MCP 工具名称
        if tool_name.starts_with("mcp__") {
            let parts: Vec<&str> = tool_name.splitn(3, "__").collect();
            if parts.len() >= 3 {
                return format!("[{}] {}", parts[1], parts[2]);
            }
        }

        // 解析内部工具名称
        if tool_name.starts_with("internal__") {
            return tool_name.trim_start_matches("internal__").to_string();
        }

        tool_name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_build_file_preview_write() {
        let preview = PreviewBuilder::build(
            "mcp__filesystem__write_file",
            &json!({
                "path": "/tmp/test.txt",
                "content": "Hello, World!"
            }),
        );

        if let OperationPreview::FileOperation(file_preview) = preview {
            assert_eq!(file_preview.operation, FileOperationType::Create);
            assert_eq!(file_preview.path, "/tmp/test.txt");
            assert_eq!(
                file_preview.content_preview,
                Some("Hello, World!".to_string())
            );
        } else {
            panic!("Expected FileOperation preview");
        }
    }

    #[test]
    fn test_build_file_preview_delete() {
        let preview = PreviewBuilder::build(
            "mcp__filesystem__delete_file",
            &json!({
                "path": "/tmp/test.txt"
            }),
        );

        if let OperationPreview::FileOperation(file_preview) = preview {
            assert_eq!(file_preview.operation, FileOperationType::Delete);
            assert_eq!(file_preview.path, "/tmp/test.txt");
        } else {
            panic!("Expected FileOperation preview");
        }
    }

    #[test]
    fn test_build_shell_preview() {
        let preview = PreviewBuilder::build(
            "mcp__shell__execute",
            &json!({
                "command": "ls -la",
                "cwd": "/home/user"
            }),
        );

        if let OperationPreview::ShellCommand(shell_preview) = preview {
            assert_eq!(shell_preview.command, "ls -la");
            assert_eq!(
                shell_preview.working_directory,
                Some("/home/user".to_string())
            );
        } else {
            panic!("Expected ShellCommand preview");
        }
    }

    #[test]
    fn test_build_shell_preview_with_sudo() {
        let preview = PreviewBuilder::build(
            "mcp__shell__execute",
            &json!({
                "command": "sudo rm -rf /tmp/test"
            }),
        );

        if let OperationPreview::ShellCommand(shell_preview) = preview {
            assert!(shell_preview.estimated_impact.contains("管理员权限"));
            assert!(shell_preview.estimated_impact.contains("删除"));
        } else {
            panic!("Expected ShellCommand preview");
        }
    }

    #[test]
    fn test_build_http_preview() {
        let preview = PreviewBuilder::build(
            "mcp__http__fetch",
            &json!({
                "url": "https://api.example.com/users",
                "method": "POST",
                "body": {"name": "test"}
            }),
        );

        if let OperationPreview::HttpRequest(http_preview) = preview {
            assert_eq!(http_preview.method, "POST");
            assert_eq!(http_preview.url, "https://api.example.com/users");
            assert!(http_preview.is_external);
        } else {
            panic!("Expected HttpRequest preview");
        }
    }

    #[test]
    fn test_build_http_preview_localhost() {
        let preview = PreviewBuilder::build(
            "mcp__http__get",
            &json!({
                "url": "http://localhost:8080/api"
            }),
        );

        if let OperationPreview::HttpRequest(http_preview) = preview {
            assert_eq!(http_preview.method, "GET");
            assert!(!http_preview.is_external);
        } else {
            panic!("Expected HttpRequest preview");
        }
    }

    #[test]
    fn test_build_generic_preview() {
        let preview = PreviewBuilder::build(
            "mcp__custom__some_tool",
            &json!({
                "arg1": "value1",
                "arg2": 123
            }),
        );

        if let OperationPreview::Generic(generic_preview) = preview {
            assert_eq!(generic_preview.title, "[custom] some_tool");
            assert!(generic_preview.description.contains("custom"));
            assert!(generic_preview.details.contains_key("arg1"));
        } else {
            panic!("Expected Generic preview");
        }
    }

    #[test]
    fn test_analyze_shell_impact() {
        let impact = PreviewBuilder::analyze_shell_impact("rm -rf /tmp/test");
        assert!(impact.contains("删除"));

        let impact = PreviewBuilder::analyze_shell_impact("git push origin main");
        assert!(impact.contains("推送"));

        let impact = PreviewBuilder::analyze_shell_impact("echo hello");
        assert_eq!(impact, "执行 Shell 命令");
    }

    #[test]
    fn test_tool_name_to_description() {
        assert!(
            PreviewBuilder::tool_name_to_description("mcp__filesystem__write_file")
                .contains("filesystem")
        );
        assert!(
            PreviewBuilder::tool_name_to_description("internal__spawn_sub_agent")
                .contains("内部工具")
        );
    }
}
