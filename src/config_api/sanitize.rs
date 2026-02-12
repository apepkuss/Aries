//! Configuration sanitization logic
//!
//! This module provides the `Sanitize` trait and implementations for converting
//! sensitive configuration data into safe, redacted versions.

use super::types::{
    SanitizedArtifactsConfig, SanitizedChatConfig, SanitizedConfig, SanitizedEmbeddingConfig,
    SanitizedHitlConfig, SanitizedMcpConfig, SanitizedMcpServerConfig,
    SanitizedMcpToolServerConfig, SanitizedMemoryConfig, SanitizedRagConfig, SanitizedServerConfig,
    SanitizedSessionConfig, SanitizedSkillConfig, SanitizedSubagentConfig, UPDATABLE_FIELDS,
};
use crate::{
    config::{
        ArtifactsConfig, ChatConfig, Config, EmbeddingConfig, McpConfig, McpServerConfig,
        McpToolServerConfig, MemoryConfig, RagConfig, ServerConfig, SessionConfig, SkillConfig,
    },
    services::hitl::HitlConfig,
    subagent::SubAgentSystemConfig,
};

/// Trait for converting sensitive configuration to sanitized version
pub trait Sanitize {
    /// The sanitized output type
    type Output;

    /// Convert to sanitized version with sensitive fields redacted
    fn sanitize(&self) -> Self::Output;
}

impl Sanitize for Config {
    type Output = SanitizedConfig;

    fn sanitize(&self) -> SanitizedConfig {
        SanitizedConfig {
            server: self.server.sanitize(),
            chat: self.chat.as_ref().map(|c| c.sanitize()),
            embedding: self.embedding.as_ref().map(|e| e.sanitize()),
            memory: self.memory.as_ref().map(|m| m.sanitize()),
            rag: self.rag.as_ref().map(|r| r.sanitize()),
            mcp: self.mcp.as_ref().map(|m| m.sanitize()),
            skill: self.skill.as_ref().map(|s| s.sanitize()),
            artifacts: self.artifacts.as_ref().map(|a| a.sanitize()),
            subagent: self.subagent.as_ref().map(|s| s.sanitize()),
            hitl: self.hitl.as_ref().map(|h| h.sanitize()),
            session: self.session.as_ref().map(|s| s.sanitize()),
            updatable_fields: UPDATABLE_FIELDS.iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl Sanitize for ServerConfig {
    type Output = SanitizedServerConfig;

    fn sanitize(&self) -> SanitizedServerConfig {
        SanitizedServerConfig {
            host: self.host.clone(),
            port: self.port,
            max_tools_per_iteration: self.max_tools_per_iteration,
            tool_call_max_retries: self.tool_call_max_retries,
            tool_call_retry_delay_ms: self.tool_call_retry_delay_ms,
            max_plan_subtasks: self.max_plan_subtasks,
            plan_timeout_secs: self.plan_timeout_secs,
            subtask_max_retries: self.subtask_max_retries,
            subtask_react_max_iterations: self.subtask_react_max_iterations,
            subtask_react_timeout_secs: self.subtask_react_timeout_secs,
        }
    }
}

impl Sanitize for ChatConfig {
    type Output = SanitizedChatConfig;

    fn sanitize(&self) -> SanitizedChatConfig {
        SanitizedChatConfig {
            url: self.url.clone(),
            api_key_configured: self.get_api_key().is_some(),
            model: self.model.clone(),
        }
    }
}

impl Sanitize for EmbeddingConfig {
    type Output = SanitizedEmbeddingConfig;

    fn sanitize(&self) -> SanitizedEmbeddingConfig {
        SanitizedEmbeddingConfig {
            url: self.url.clone(),
            api_key_configured: self.get_api_key().is_some(),
        }
    }
}

impl Sanitize for MemoryConfig {
    type Output = SanitizedMemoryConfig;

    fn sanitize(&self) -> SanitizedMemoryConfig {
        SanitizedMemoryConfig {
            enable: self.enable,
            database_path: self.database_path.clone(),
            context_window: self.context_window,
            auto_summarize: self.auto_summarize,
            summarization_strategy: self.summarization_strategy,
            summarize_threshold: self.summarize_threshold,
            max_stored_messages: self.max_stored_messages,
            summary_service_api_key_configured: !self.summary_service_api_key.is_empty(),
        }
    }
}

impl Sanitize for RagConfig {
    type Output = SanitizedRagConfig;

    fn sanitize(&self) -> SanitizedRagConfig {
        SanitizedRagConfig {
            enable: self.enable,
            policy: self.policy.to_string(),
            context_window: self.context_window,
        }
    }
}

impl Sanitize for McpConfig {
    type Output = SanitizedMcpConfig;

    fn sanitize(&self) -> SanitizedMcpConfig {
        SanitizedMcpConfig {
            server: self.server.sanitize(),
        }
    }
}

impl Sanitize for McpServerConfig {
    type Output = SanitizedMcpServerConfig;

    fn sanitize(&self) -> SanitizedMcpServerConfig {
        SanitizedMcpServerConfig {
            tool_servers: self.tool_servers.iter().map(|s| s.sanitize()).collect(),
        }
    }
}

impl Sanitize for McpToolServerConfig {
    type Output = SanitizedMcpToolServerConfig;

    fn sanitize(&self) -> SanitizedMcpToolServerConfig {
        SanitizedMcpToolServerConfig {
            name: self.name.clone(),
            transport: self.transport.to_string(),
            // Only expose url, hide oauth_url for security
            url: self.url.clone(),
            command: self.command.clone(),
            enable: self.enable,
            tools_count: self.tools.as_ref().map_or(0, |t| t.len()),
        }
    }
}

impl Sanitize for SkillConfig {
    type Output = SanitizedSkillConfig;

    fn sanitize(&self) -> SanitizedSkillConfig {
        SanitizedSkillConfig {
            enabled: self.enabled,
            directories: self.directories.clone(),
            max_reference_size: self.max_reference_size,
            api_key_configured: self
                .api
                .as_ref()
                .is_some_and(|api| api.get_api_key().is_some()),
            market_api_key_configured: self.market.as_ref().is_some_and(|m| m.api_key.is_some()),
        }
    }
}

impl Sanitize for ArtifactsConfig {
    type Output = SanitizedArtifactsConfig;

    fn sanitize(&self) -> SanitizedArtifactsConfig {
        SanitizedArtifactsConfig {
            enabled: self.enabled,
            database_path: self.database_path.clone(),
            storage_path: self.storage_path.clone(),
            max_content_size: self.max_content_size,
            max_binary_size: self.max_binary_size,
            retention_days: self.retention_days,
            cleanup_interval_secs: self.cleanup_interval_secs,
            soft_delete_retention_days: self.soft_delete_retention_days,
            enable_cleanup: self.enable_cleanup,
        }
    }
}

impl Sanitize for SubAgentSystemConfig {
    type Output = SanitizedSubagentConfig;

    fn sanitize(&self) -> SanitizedSubagentConfig {
        SanitizedSubagentConfig {
            enabled: self.enabled,
            execution_mode: self.execution_mode.clone(),
            parallel_mode: self.subtask_executor.parallel_mode.clone(),
            max_concurrent: self.max_concurrent,
            default_timeout_secs: self.default_timeout_secs,
            default_max_iterations: self.default_max_iterations,
        }
    }
}

impl Sanitize for HitlConfig {
    type Output = SanitizedHitlConfig;

    fn sanitize(&self) -> SanitizedHitlConfig {
        SanitizedHitlConfig {
            enabled: self.enabled,
            default_timeout_secs: self.default_timeout_secs,
            default_timeout_behavior: format!("{:?}", self.default_timeout_behavior).to_lowercase(),
            confirmation_threshold: format!("{:?}", self.confirmation_threshold).to_lowercase(),
            tool_overrides_count: self.tool_overrides.len(),
            runtime_learning_enabled: self.runtime_learning.is_some(),
        }
    }
}

impl Sanitize for SessionConfig {
    type Output = SanitizedSessionConfig;

    fn sanitize(&self) -> SanitizedSessionConfig {
        SanitizedSessionConfig {
            enable: self.enable,
            storage_path: self.storage_path.clone(),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_sanitize() {
        let config = Config::default();
        let sanitized = config.sanitize();

        assert_eq!(sanitized.server.host, "127.0.0.1");
        assert_eq!(sanitized.server.port, 3389);
        assert!(sanitized.chat.is_none());
        assert!(sanitized.embedding.is_none());
        assert!(!sanitized.updatable_fields.is_empty());
    }

    #[test]
    fn test_server_config_sanitize() {
        let server = ServerConfig {
            host: "0.0.0.0".to_string(),
            port: 8080,
            max_tools_per_iteration: 10,
            tool_call_max_retries: 3,
            tool_call_retry_delay_ms: 1000,
            max_plan_subtasks: 20,
            plan_timeout_secs: 1200,
            subtask_max_retries: 3,
            subtask_react_max_iterations: 10,
            subtask_react_timeout_secs: 120,
        };

        let sanitized = server.sanitize();
        assert_eq!(sanitized.host, "0.0.0.0");
        assert_eq!(sanitized.port, 8080);
        assert_eq!(sanitized.max_tools_per_iteration, 10);
    }

    #[test]
    fn test_memory_config_sanitize_hides_api_key() {
        let memory = MemoryConfig {
            enable: true,
            database_path: "data/memory.db".to_string(),
            context_window: 8192,
            auto_summarize: true,
            summarization_strategy: crate::config::SummarizationStrategy::Incremental,
            summarize_threshold: 12,
            max_stored_messages: 20,
            summary_service_base_url: "http://localhost:8080".to_string(),
            summary_service_api_key: "secret-api-key".to_string(),
        };

        let sanitized = memory.sanitize();
        assert!(sanitized.summary_service_api_key_configured);
        // Verify the actual key is not in the output
        let json = serde_json::to_string(&sanitized).unwrap();
        assert!(!json.contains("secret-api-key"));
    }

    #[test]
    fn test_skill_config_sanitize() {
        let skill = SkillConfig::default();
        let sanitized = skill.sanitize();

        assert!(sanitized.enabled);
        assert!(!sanitized.directories.is_empty());
        assert!(!sanitized.api_key_configured);
        assert!(!sanitized.market_api_key_configured);
    }

    #[test]
    fn test_artifacts_config_sanitize() {
        let artifacts = ArtifactsConfig::default();
        let sanitized = artifacts.sanitize();

        assert!(!sanitized.enabled);
        assert_eq!(sanitized.database_path, "data/artifacts.db");
    }

    #[test]
    fn test_updatable_fields_in_sanitized_config() {
        let config = Config::default();
        let sanitized = config.sanitize();

        assert!(
            sanitized
                .updatable_fields
                .contains(&"server.max_tools_per_iteration".to_string())
        );
        // chat.url is NOT in UPDATABLE_FIELDS (requires restart)
        assert!(!sanitized.updatable_fields.contains(&"chat.url".to_string()));
        // embedding.url IS in UPDATABLE_FIELDS
        assert!(
            sanitized
                .updatable_fields
                .contains(&"embedding.url".to_string())
        );
        assert!(
            !sanitized
                .updatable_fields
                .contains(&"server.host".to_string())
        );
    }
}
