use std::path::PathBuf;
use std::sync::Arc;

use crate::{
    AppState, Config, ServerResult, ServerError,
    HEALTH_CHECK_INTERVAL,
    info::ServerInfo,
    memory::CompleteChatMemory,
    skills::SkillRegistry,
    executor::ScriptExecutorManager,
};

/// AriesEngine provides a unified interface to initialize and manage the Agent core.
pub struct AriesEngine {
    pub state: Arc<AppState>,
}

impl AriesEngine {
    /// Initialize the engine from a configuration path.
    pub async fn init(config_path: PathBuf) -> ServerResult<Self> {
        // Load config
        let config = Config::load(&config_path).await?;

        // Initialize memory
        let memory = if let Some(ref memory_config) = config.memory {
            if memory_config.enable {
                let memory_system = CompleteChatMemory::new(memory_config.clone()).await.map_err(|e| {
                    ServerError::Operation(format!("Failed to initialize memory system: {e}"))
                })?;
                Some(Arc::new(memory_system))
            } else {
                None
            }
        } else {
            None
        };

        // Initialize skills if enabled
        if let Some(ref skill_config) = config.skill {
            if skill_config.enabled {
                // Find valid skills dir
                let mut skills_dir = None;
                for dir in &skill_config.directories {
                    let expanded = shellexpand::tilde(dir).to_string();
                    if std::path::Path::new(&expanded).is_dir() {
                        skills_dir = Some(expanded);
                        break;
                    }
                }

                if let Some(dir) = skills_dir {
                    if let Ok(registry) = SkillRegistry::init_global(PathBuf::from(&dir)) {
                        let _ = registry.load_all().await;
                    }
                }

                // Initialize executors
                if let Some(ref exec_config) = skill_config.execution {
                    if exec_config.enabled {
                        let _ = ScriptExecutorManager::init_global(exec_config.clone()).await;
                    }
                }
            }
        }

        // Initialize health check interval
        // Note: In a lib context, we might want to let the consumer set this.
        // For now, we'll use a default if not set by CLI.
        let _ = HEALTH_CHECK_INTERVAL.get_or_init(|| 60);

        // Create AppState
        let mut state = AppState::new(config, ServerInfo::default()).with_config_path(config_path);
        if let Some(mem) = memory {
            state = state.with_memory(mem);
        }

        let state = Arc::new(state);

        // Register configured servers
        state.register_config_servers().await?;

        Ok(Self { state })
    }

    /// Start the health check task.
    pub async fn start_health_checks(&self) {
        Arc::clone(&self.state).start_health_check_task().await;
    }
}
