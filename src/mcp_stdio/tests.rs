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
