//! Tests for stdio MCP transport module.

use super::types::*;

#[test]
fn test_stdio_config_default() {
    let config = StdioConfig::default();
    assert_eq!(config.health_check_interval_secs, 30);
    assert_eq!(config.health_check_timeout_secs, 5);
    assert_eq!(config.health_check_failure_threshold, 3);
    assert!(config.restart_on_failure);
    assert_eq!(config.max_restart_attempts, 3);
    assert_eq!(config.restart_backoff_secs, 5);
}

#[test]
fn test_process_status_equality() {
    assert_eq!(ProcessStatus::Running, ProcessStatus::Running);
    assert_eq!(ProcessStatus::Stopped, ProcessStatus::Stopped);
    assert_ne!(ProcessStatus::Running, ProcessStatus::Stopped);
    assert_eq!(
        ProcessStatus::Failed("error".to_string()),
        ProcessStatus::Failed("error".to_string())
    );
}

#[test]
fn test_stdio_config_serialization() {
    let config = StdioConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let deserialized: StdioConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(
        config.health_check_interval_secs,
        deserialized.health_check_interval_secs
    );
    assert_eq!(config.restart_on_failure, deserialized.restart_on_failure);
}

#[test]
fn test_stdio_config_deserialization_with_defaults() {
    // Empty JSON object should use all defaults
    let config: StdioConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(config.health_check_interval_secs, 30);
    assert!(config.restart_on_failure);
}

#[test]
fn test_stdio_config_partial_override() {
    let json = r#"{"health_check_interval_secs": 60, "restart_on_failure": false}"#;
    let config: StdioConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.health_check_interval_secs, 60);
    assert!(!config.restart_on_failure);
    // Other fields should use defaults
    assert_eq!(config.health_check_timeout_secs, 5);
    assert_eq!(config.max_restart_attempts, 3);
}

#[test]
fn test_process_config_toml_deserialization() {
    let toml_str = r#"
name = "actionbook"
command = "npx"
args = ["-y", "@actionbookdev/mcp@latest"]
enable = true

[stdio]
health_check_interval_secs = 60
restart_on_failure = true
max_restart_attempts = 5
"#;
    let config: StdioProcessConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.name, "actionbook");
    assert_eq!(config.command, "npx");
    assert_eq!(config.args, vec!["-y", "@actionbookdev/mcp@latest"]);
    assert!(config.enable);
    assert!(config.env.is_empty());
    assert!(config.working_dir.is_none());
    // stdio section
    assert_eq!(config.stdio.health_check_interval_secs, 60);
    assert!(config.stdio.restart_on_failure);
    assert_eq!(config.stdio.max_restart_attempts, 5);
    // defaults for unspecified fields
    assert_eq!(config.stdio.health_check_timeout_secs, 5);
    assert_eq!(config.stdio.restart_backoff_secs, 5);
}

#[test]
fn test_process_config_toml_minimal() {
    let toml_str = r#"
name = "test-server"
command = "/usr/bin/echo"
"#;
    let config: StdioProcessConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.name, "test-server");
    assert_eq!(config.command, "/usr/bin/echo");
    assert!(config.args.is_empty());
    assert!(config.enable); // default true
    // stdio section defaults
    assert_eq!(config.stdio.health_check_interval_secs, 30);
    assert!(config.stdio.restart_on_failure);
}

#[test]
fn test_process_config_toml_with_env() {
    let toml_str = r#"
name = "my-server"
command = "node"
args = ["server.js"]
working_dir = "/opt/mcp"

[env]
NODE_ENV = "production"
PORT = "3000"

[stdio]
health_check_interval_secs = 0
restart_on_failure = false
"#;
    let config: StdioProcessConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.working_dir.as_deref(), Some("/opt/mcp"));
    assert_eq!(config.env.get("NODE_ENV").unwrap(), "production");
    assert_eq!(config.env.get("PORT").unwrap(), "3000");
    assert_eq!(config.stdio.health_check_interval_secs, 0); // disabled
    assert!(!config.stdio.restart_on_failure);
}

#[test]
fn test_stdio_request_serialization() {
    let req = StdioRequest {
        id: "req-1".to_string(),
        method: "tools/call".to_string(),
        params: serde_json::json!({"name": "search", "arguments": {}}),
    };
    let json = serde_json::to_string(&req).unwrap();
    let deserialized: StdioRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.id, "req-1");
    assert_eq!(deserialized.method, "tools/call");
}

#[test]
fn test_stdio_response_with_result() {
    let json = r#"{"id": "req-1", "result": {"content": "hello"}, "error": null}"#;
    let resp: StdioResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.id, "req-1");
    assert!(resp.result.is_some());
    assert!(resp.error.is_none());
}

#[test]
fn test_stdio_response_with_error() {
    let json = r#"{"id": "req-2", "result": null, "error": {"code": -32600, "message": "Invalid Request", "data": null}}"#;
    let resp: StdioResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.id, "req-2");
    assert!(resp.result.is_none());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32600);
    assert_eq!(err.message, "Invalid Request");
}

#[test]
fn test_process_metadata_new() {
    let meta = ProcessMetadata::new("test-server", Some(12345));
    assert_eq!(meta.name, "test-server");
    assert_eq!(meta.pid, Some(12345));
    assert_eq!(meta.status, ProcessStatus::Starting);
    assert!(meta.start_time.is_some());
    assert_eq!(meta.restart_count, 0);
    assert!(meta.last_health_check.is_none());
    assert_eq!(meta.consecutive_failures, 0);
}

#[test]
fn test_process_status_display() {
    assert_eq!(ProcessStatus::Running.to_string(), "Running");
    assert_eq!(ProcessStatus::Stopped.to_string(), "Stopped");
    assert_eq!(
        ProcessStatus::Failed("timeout".to_string()).to_string(),
        "Failed: timeout"
    );
}
