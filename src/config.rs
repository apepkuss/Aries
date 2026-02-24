use std::{collections::HashMap, env, net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{
    Router,
    extract::{Query, State},
    response::Html,
    routing::get,
};
use chat_prompts::MergeRagContextPolicy;
use clap::ValueEnum;
use endpoints::chat::McpTransport;
use rmcp::{
    model::{ClientCapabilities, ClientInfo, Implementation, Tool as RmcpTool},
    service::ServiceExt,
    transport::{
        SseClientTransport, StreamableHttpClientTransport, TokioChildProcess,
        auth::{AuthClient, OAuthState},
        sse_client::SseClientConfig,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncWriteExt, BufWriter},
    sync::{Mutex, RwLock as TokioRwLock, oneshot},
};
use url::Url;

use crate::{
    dual_debug, dual_error, dual_info, dual_warn,
    error::{ServerError, ServerResult},
    executor::{DenoConfig, DockerConfig, ResourceLimits, WasmtimeConfig},
    mcp::{MCP_SERVICES, McpService},
    mcp_stdio::{self, StdioConfig, StdioProcessConfig},
};

/// Application home directory name (under user's home directory)
pub const APP_HOME_DIR: &str = ".moss";

/// Default server port
pub const DEFAULT_PORT: u16 = 3389;

/// Returns the application home directory path, e.g. ~/.moss
pub fn app_home_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Failed to get home directory")
        .join(APP_HOME_DIR)
}

/// Returns a tilde-prefixed path under the app home directory, e.g. ~/.moss/skills
pub fn app_home_path(sub: &str) -> String {
    format!("~/{APP_HOME_DIR}/{sub}")
}

const MCP_REDIRECT_URI: &str = "http://localhost:8080/callback";
const CALLBACK_PORT: u16 = 8080;
const CALLBACK_HTML: &str = include_str!("auth/callback.html");

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat: Option<ChatConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding: Option<EmbeddingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rag: Option<RagConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_info_push_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_health_push_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp: Option<McpConfig>,
    /// Skills configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill: Option<SkillConfig>,
    /// Reflection system configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reflection: Option<crate::reflection::ReflectionConfig>,
    /// Dynamic replanning configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replan: Option<crate::reflection::ReplanConfig>,
    /// Artifacts management configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<ArtifactsConfig>,
    /// Configuration API settings (hot reload, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_api: Option<ConfigApiSettings>,
    /// Sub-Agent system configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent: Option<crate::subagent::SubAgentSystemConfig>,
    /// HITL (Human-in-the-Loop) configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hitl: Option<crate::services::hitl::HitlConfig>,
    /// Session history configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionConfig>,
    /// Privacy detection configuration for smart privacy mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privacy_detection: Option<crate::services::privacy::PrivacyDetectorConfig>,
    /// Lantai knowledge base configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lantai: Option<MossLantaiConfig>,
}
impl Config {
    pub async fn load(path: impl AsRef<std::path::Path>) -> ServerResult<Self> {
        let config = config::Config::builder()
            .add_source(config::File::with_name(path.as_ref().to_str().unwrap()))
            .build()
            .map_err(|e| {
                let err_msg = format!("Failed to load config file: {e}");
                dual_error!("{}", &err_msg);
                ServerError::FailedToLoadConfig(err_msg)
            })?;

        let mut config = config.try_deserialize::<Self>().map_err(|e| {
            let err_msg = format!("Failed to deserialize config: {e}");
            dual_error!("{}", &err_msg);
            ServerError::FailedToLoadConfig(err_msg)
        })?;

        if let Some(mcp_config) = config.mcp.as_mut()
            && !mcp_config.server.tool_servers.is_empty()
        {
            let total = mcp_config.server.tool_servers.len();
            let mut connected = 0;
            let mut failed_names = Vec::new();

            for server_config in mcp_config.server.tool_servers.iter_mut() {
                if !server_config.enable {
                    continue;
                }
                match server_config.connect_mcp_server().await {
                    Ok(()) => {
                        connected += 1;
                    }
                    Err(e) => {
                        dual_warn!(
                            "Failed to connect MCP server '{}': {}. Disabling.",
                            server_config.name,
                            e
                        );
                        server_config.enable = false;
                        failed_names.push(server_config.name.clone());
                    }
                }
            }

            if !failed_names.is_empty() {
                dual_warn!(
                    "MCP servers: {}/{} connected, {} failed ({}). Failed servers have been disabled.",
                    connected,
                    total,
                    failed_names.len(),
                    failed_names.join(", ")
                );

                // Persist the disabled state to config file
                let config_path = path.as_ref();
                let result = crate::config_api::persist::persist_config(&config, config_path).await;
                if !result.success {
                    dual_warn!(
                        "Failed to persist config after disabling failed MCP servers: {:?}",
                        result.error
                    );
                }
            }
        }

        dual_debug!("config:\n{:#?}", config);

        Ok(config)
    }
}

// Add Default implementation for Config
impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: DEFAULT_PORT,
                max_tools_per_iteration: default_max_tools_per_iteration(),
                tool_call_max_retries: default_tool_call_max_retries(),
                tool_call_retry_delay_ms: default_tool_call_retry_delay_ms(),
                max_plan_subtasks: default_max_plan_subtasks(),
                plan_timeout_secs: default_plan_timeout_secs(),
                subtask_max_retries: default_subtask_max_retries(),
                subtask_react_max_iterations: default_subtask_react_max_iterations(),
                subtask_react_timeout_secs: default_subtask_react_timeout_secs(),
            },
            chat: None,
            embedding: None,
            memory: None,
            rag: None,
            server_info_push_url: None,
            server_health_push_url: None,
            mcp: None,
            skill: None,
            reflection: None,
            replan: None,
            artifacts: None,
            config_api: None,
            subagent: None,
            hitl: None,
            session: None,
            privacy_detection: None,
            lantai: None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    /// Maximum number of tool calls allowed per iteration.
    /// Prevents excessive tool execution in a single loop iteration.
    #[serde(default = "default_max_tools_per_iteration")]
    pub max_tools_per_iteration: usize,
    /// Maximum number of retries for a failed tool call.
    /// Allows automatic recovery from transient failures.
    #[serde(default = "default_tool_call_max_retries")]
    pub tool_call_max_retries: u32,
    /// Delay in milliseconds between tool call retries.
    /// Provides backoff time for transient failures to resolve.
    #[serde(default = "default_tool_call_retry_delay_ms")]
    pub tool_call_retry_delay_ms: u64,
    /// Maximum number of subtasks allowed in a plan.
    /// Prevents overly complex plans that could be difficult to execute.
    #[serde(default = "default_max_plan_subtasks")]
    pub max_plan_subtasks: usize,
    /// Timeout in seconds for the entire plan execution.
    /// Prevents long-running plans from blocking indefinitely.
    #[serde(default = "default_plan_timeout_secs")]
    pub plan_timeout_secs: u64,
    /// Maximum number of retries for a failed subtask.
    /// Allows automatic recovery from transient failures during plan execution.
    #[serde(default = "default_subtask_max_retries")]
    pub subtask_max_retries: u32,
    /// Maximum number of React iterations allowed per subtask in Plan mode.
    /// Controls how many reasoning loops a subtask can perform before completing.
    #[serde(default = "default_subtask_react_max_iterations")]
    pub subtask_react_max_iterations: u32,
    /// Timeout in seconds for each subtask's React loop in Plan mode.
    /// Prevents individual subtasks from blocking the entire plan execution.
    #[serde(default = "default_subtask_react_timeout_secs")]
    pub subtask_react_timeout_secs: u64,
}

fn default_max_tools_per_iteration() -> usize {
    5 // Default maximum 5 tools per iteration
}

fn default_tool_call_max_retries() -> u32 {
    2 // Default maximum 2 retries
}

fn default_tool_call_retry_delay_ms() -> u64 {
    500 // Default 500ms delay between retries
}

fn default_max_plan_subtasks() -> usize {
    10 // Default maximum 10 subtasks per plan
}

fn default_plan_timeout_secs() -> u64 {
    600 // Default 10 minutes timeout for plan execution
}

fn default_subtask_max_retries() -> u32 {
    2 // Default maximum 2 retries per subtask
}

fn default_subtask_react_max_iterations() -> u32 {
    5 // Default maximum 5 React iterations per subtask
}

fn default_subtask_react_timeout_secs() -> u64 {
    60 // Default 60 seconds timeout per subtask
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChatConfig {
    pub url: String,
    api_key: String,
    /// Model to use for chat completions (runtime only, not persisted)
    #[serde(default, skip_serializing)]
    pub model: String,
    /// Model context window size (tokens). Used for token-aware features.
    /// Set to 0 to disable all token-aware features (default: 128000).
    #[serde(default = "default_model_context_size")]
    pub model_context_size: u64,
}

fn default_model_context_size() -> u64 {
    128000
}

impl ChatConfig {
    pub fn get_api_key(&self) -> Option<String> {
        if !self.api_key.is_empty() {
            Some(self.api_key.clone())
        } else {
            std::env::var("DEFAULT_CHAT_SERVICE_API_KEY").ok()
        }
    }

    pub fn set_api_key(&mut self, api_key: String) {
        self.api_key = api_key;
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct EmbeddingConfig {
    pub url: String,
    api_key: String,
}

impl EmbeddingConfig {
    /// Create a new EmbeddingConfig with a URL and empty API key
    pub fn new_with_url(url: String) -> Self {
        Self {
            url,
            api_key: String::new(),
        }
    }

    pub fn get_api_key(&self) -> Option<String> {
        if !self.api_key.is_empty() {
            Some(self.api_key.clone())
        } else {
            std::env::var("DEFAULT_EMBEDDING_SERVICE_API_KEY").ok()
        }
    }

    pub fn set_api_key(&mut self, api_key: String) {
        self.api_key = api_key;
    }
}

/// Summarization strategy for handling conversation history
#[derive(Debug, Default, Copy, Deserialize, Serialize, Clone, PartialEq)]
pub enum SummarizationStrategy {
    /// Incremental summarization: Use existing summary + new messages
    /// This is more efficient but may lose some context over time
    #[default]
    Incremental,
    /// Full history summarization: Re-summarize all historical messages
    /// This provides better context but is more computationally expensive
    FullHistory,
}

impl std::fmt::Display for SummarizationStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            SummarizationStrategy::Incremental => write!(f, "Incremental"),
            SummarizationStrategy::FullHistory => write!(f, "FullHistory"),
        }
    }
}

/// Memory system configuration
///
/// Controls the behavior of conversation memory management including
/// automatic summarization and message retention policies.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MemoryConfig {
    /// Enable or disable memory functionality
    pub enable: bool,
    /// Path to SQLite database file for storing conversation history
    pub database_path: String,
    /// Maximum context window size in tokens
    pub context_window: u64,
    /// Enable automatic message summarization when limits are reached
    pub auto_summarize: bool,

    /// Summarization strategy to use when generating summaries
    /// - Incremental: Use existing summary + new messages (default, more efficient)
    /// - FullHistory: Re-summarize all historical messages (better context, more expensive)
    pub summarization_strategy: SummarizationStrategy,

    /// Base number for calculating minimum messages to keep after summarization.
    /// Actual kept messages = summarize_threshold / 2
    /// This should be LESS than max_stored_messages to allow effective summarization.
    ///
    /// Note: The name "summarize_threshold" is somewhat misleading - it's not the threshold
    /// for triggering summarization, but rather the base for calculating retention count.
    /// A more descriptive name would be "min_keep_messages_base" or similar.
    pub summarize_threshold: u32,

    /// Maximum number of messages to store before triggering summarization.
    /// When message count reaches this limit, summarization is automatically triggered.
    /// This should be GREATER than summarize_threshold for proper operation.
    ///
    /// This name accurately reflects its purpose as the trigger point for summarization.
    pub max_stored_messages: u32,

    /// Base URL for the summary service used to generate conversation summaries.
    /// This service is called when automatic summarization is triggered.
    pub summary_service_base_url: String,

    /// API key for authenticating with the summary service.
    /// Leave empty if the summary service doesn't require authentication.
    pub summary_service_api_key: String,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enable: false,
            database_path: "data/memory.db".to_string(),
            context_window: 8192,
            auto_summarize: true,
            summarization_strategy: SummarizationStrategy::default(),
            // Default configuration follows the principle: max_stored_messages > summarize_threshold
            // This allows for effective summarization: 20 messages trigger → keep 6 → summarize 14
            summarize_threshold: 12, // Keep 6 messages minimum (12/2)
            max_stored_messages: 20, // Trigger summarization at 20 messages
            summary_service_base_url: "http://localhost:10086/v1".to_string(),
            summary_service_api_key: String::new(),
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct RagConfig {
    pub enable: bool,
    pub prompt: Option<String>,
    pub policy: MergeRagContextPolicy,
    pub context_window: u64,
}
impl<'de> Deserialize<'de> for RagConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RagConfigHelper {
            enable: bool,
            policy: String,
            context_window: u64,
        }

        let helper = RagConfigHelper::deserialize(deserializer)?;

        let policy = MergeRagContextPolicy::from_str(&helper.policy, true)
            .map_err(|e| serde::de::Error::custom(e.to_string()))?;

        Ok(RagConfig {
            enable: helper.enable,
            prompt: None,
            policy,
            context_window: helper.context_window,
        })
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct McpConfig {
    #[serde(rename = "server")]
    pub server: McpServerConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct McpServerConfig {
    #[serde(rename = "tool")]
    pub tool_servers: Vec<McpToolServerConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpToolServerConfig {
    pub name: String,
    pub transport: McpTransport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_url: Option<String>,
    /// Command to run for stdio transport
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Arguments for the stdio command
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// Environment variables for the stdio process
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
    /// Working directory for the stdio process
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    /// stdio-specific configuration (health check, restart, etc.)
    #[serde(default, skip_serializing_if = "StdioConfig::is_default")]
    pub stdio: StdioConfig,
    pub enable: bool,
    /// API key (stored separately, dynamically appended to URL at connection time)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// URL query parameter name for the API key (e.g., "tavilyApiKey")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_param: Option<String>,
    #[serde(skip)]
    pub server_name: Option<String>,
    #[serde(skip)]
    pub tools: Option<Vec<RmcpTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_message: Option<String>,
}
impl McpToolServerConfig {
    /// Connect the mcp server if it is enabled
    pub async fn connect_mcp_server(&mut self) -> ServerResult<()> {
        if self.enable {
            // Handle stdio transport separately (no URL needed)
            if self.transport == McpTransport::Stdio {
                return self.connect_stdio().await;
            }

            // Validate URL configuration: exactly one must be non-empty
            let mut use_oauth = false;
            let server_url = match (&self.url, &self.oauth_url) {
                (Some(url), None) => url,
                (None, Some(oauth_url)) => {
                    use_oauth = true;
                    oauth_url
                }
                (Some(_), Some(_)) => {
                    let err_msg = format!(
                        "Invalid configuration for mcp server '{}': Both url and oauth_url cannot be set at the same time",
                        self.name
                    );
                    dual_error!("{}", err_msg);
                    return Err(ServerError::Operation(err_msg));
                }
                (None, None) => {
                    let err_msg = format!(
                        "Invalid configuration for mcp server '{}': Either url or oauth_url must be set",
                        self.name
                    );
                    dual_error!("{}", err_msg);
                    return Err(ServerError::Operation(err_msg));
                }
            };

            match self.transport {
                McpTransport::Sse => {
                    // Dynamically append API key to URL if configured
                    let url = if let (Some(api_key), Some(param_name)) =
                        (&self.api_key, &self.api_key_param)
                    {
                        if !api_key.is_empty() {
                            let base = server_url.trim_end_matches('/');
                            let separator = if base.contains('?') { "&" } else { "?" };
                            format!(
                                "{}{}{}={}",
                                base,
                                separator,
                                param_name,
                                urlencoding::encode(api_key)
                            )
                        } else {
                            server_url.trim_end_matches('/').to_string()
                        }
                    } else {
                        server_url.trim_end_matches('/').to_string()
                    };

                    let service = match use_oauth {
                        false => {
                            Url::parse(&url).map_err(|e| {
                                let err_msg = format!("Invalid mcp tools sse URL: {url}. {e}",);
                                dual_error!("{}", err_msg);
                                ServerError::Operation(err_msg)
                            })?;
                            dual_debug!("Sync mcp tools from mcp server: {}", url);

                            // create a sse transport
                            let transport =
                                SseClientTransport::start(url.as_str()).await.map_err(|e| {
                                    let err_msg = format!("Failed to create sse transport: {e}");
                                    dual_error!("{}", &err_msg);
                                    ServerError::McpOperation(err_msg)
                                })?;

                            // create a mcp client
                            let client_info = ClientInfo {
                                protocol_version: Default::default(),
                                capabilities: ClientCapabilities::default(),
                                client_info: Implementation {
                                    name: env!("CARGO_PKG_NAME").to_string(),
                                    version: env!("CARGO_PKG_VERSION").to_string(),
                                    title: None,
                                    icons: None,
                                    website_url: None,
                                },
                            };
                            client_info.into_dyn().serve(transport).await.map_err(|e| {
                                let err_msg = format!(
                                    "Failed to connect to mcp server (name: {}, url: {}, transport: {}). {e}. Please check if the mcp server is running.",
                                    self.name, server_url, self.transport
                                );
                                dual_error!("{}", &err_msg);
                                ServerError::McpOperation(err_msg)
                            })?
                        }
                        true => {
                            // it is a http server for handling callback
                            // Create channel for receiving authorization code
                            let (code_sender, code_receiver) = oneshot::channel::<String>();

                            // Create app state
                            let app_state = AppState {
                                code_receiver: Arc::new(Mutex::new(Some(code_sender))),
                            };

                            // Start HTTP server for handling callbacks
                            let app = Router::new()
                                .route("/callback", get(callback_handler))
                                .with_state(app_state);

                            let addr = SocketAddr::from(([127, 0, 0, 1], CALLBACK_PORT));
                            tracing::info!("Starting callback server at: http://{}", addr);

                            // Start server in a separate task
                            tokio::spawn(async move {
                                let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
                                let result = axum::serve(listener, app).await;

                                if let Err(e) = result {
                                    tracing::error!("Callback server error: {}", e);
                                }
                            });

                            // Get server URL
                            tracing::info!("Using MCP server OAuth URL: {}", url);

                            // Initialize oauth state machine
                            let mut oauth_state =
                                OAuthState::new(&url, None).await.map_err(|e| {
                                    let err_msg =
                                        format!("Failed to initialize oauth state machine: {e}");
                                    dual_error!("{}", err_msg);
                                    ServerError::McpOperation(err_msg)
                                })?;

                            // Get metadata to view supported scopes
                            if let OAuthState::Unauthorized(manager) = &mut oauth_state {
                                let metadata = manager.discover_metadata().await.map_err(|e| {
                                    let err_msg = format!("Failed to discover metadata: {e}");
                                    dual_error!("{}", err_msg);
                                    ServerError::McpOperation(err_msg.to_string())
                                })?;
                                if let Some(supported_scopes) = metadata.scopes_supported {
                                    dual_debug!("Server supported scopes: {:?}", supported_scopes);
                                    // Use server supported scopes
                                    oauth_state
                                        .start_authorization(
                                            &supported_scopes
                                                .iter()
                                                .map(|s| s.as_str())
                                                .collect::<Vec<_>>(),
                                            MCP_REDIRECT_URI,
                                        )
                                        .await
                                        .map_err(|e| {
                                            let err_msg =
                                                format!("Failed to start authorization: {e}");
                                            dual_error!("{}", err_msg);
                                            ServerError::McpOperation(err_msg)
                                        })?;
                                } else {
                                    let err_msg = "Failed to get supported scopes from mcp server";
                                    dual_error!("{}", err_msg);
                                    return Err(ServerError::McpOperation(err_msg.to_string()));
                                }
                            }

                            // Output authorization URL to user
                            let mut output = BufWriter::new(tokio::io::stdout());
                            output
                                .write_all(b"\n=== MCP OAuth Client ===\n\n")
                                .await
                                .map_err(|e| {
                                    let err_msg = format!("Failed to write to stdout: {e}");
                                    dual_error!("{}", err_msg);
                                    ServerError::McpOperation(err_msg)
                                })?;
                            output.write_all(b"Please open the following URL in your browser to authorize:\n\n")
                            .await.map_err(|e| {
                                let err_msg = format!("Failed to write to stdout: {e}");
                                dual_error!("{}", err_msg);
                                ServerError::McpOperation(err_msg)
                            })?;

                            output
                                .write_all(
                                    oauth_state
                                        .get_authorization_url()
                                        .await
                                        .map_err(|e| {
                                            let err_msg =
                                                format!("Failed to get authorization url: {e}");
                                            dual_error!("{}", err_msg);
                                            ServerError::McpOperation(err_msg)
                                        })?
                                        .as_bytes(),
                                )
                                .await
                                .map_err(|e| {
                                    let err_msg = format!("Failed to write to stdout: {e}");
                                    dual_error!("{}", err_msg);
                                    ServerError::McpOperation(err_msg)
                                })?;
                            output
                                .write_all(b"\n\nWaiting for browser callback, please do not close this window...\n")
                                .await.map_err(|e| {
                                    let err_msg = format!("Failed to write to stdout: {e}");
                                    dual_error!("{}", err_msg);
                                    ServerError::McpOperation(err_msg)
                                })?;
                            output.flush().await.map_err(|e| {
                                let err_msg = format!("Failed to flush stdout: {e}");
                                dual_error!("{}", err_msg);
                                ServerError::McpOperation(err_msg)
                            })?;

                            // Wait for authorization code
                            tracing::info!("Waiting for authorization code...");
                            let auth_code = code_receiver.await.map_err(|e| {
                                let err_msg = format!("Failed to get authorization code: {e}");
                                dual_error!("{}", err_msg);
                                ServerError::McpOperation(err_msg)
                            })?;
                            tracing::info!("Received authorization code: {}", auth_code);
                            // Exchange code for access token
                            tracing::info!("Exchanging authorization code for access token...");
                            oauth_state.handle_callback(&auth_code).await.map_err(|e| {
                                let err_msg = format!("Failed to handle callback: {e}");
                                dual_error!("{}", err_msg);
                                ServerError::McpOperation(err_msg)
                            })?;
                            tracing::info!("Successfully obtained access token");

                            output
                                .write_all(
                                    b"\nAuthorization successful! Access token obtained.\n\n",
                                )
                                .await
                                .map_err(|e| {
                                    let err_msg = format!("Failed to write to stdout: {e}");
                                    dual_error!("{}", err_msg);
                                    ServerError::McpOperation(err_msg)
                                })?;
                            output.flush().await.map_err(|e| {
                                let err_msg = format!("Failed to flush stdout: {e}");
                                dual_error!("{}", err_msg);
                                ServerError::McpOperation(err_msg)
                            })?;

                            // Create authorized transport, this transport is authorized by the oauth state machine
                            tracing::info!("Establishing authorized connection to MCP server...");
                            let am = oauth_state.into_authorization_manager().ok_or_else(|| {
                                let err_msg = "Failed to get authorization manager";
                                dual_error!("{}", err_msg);
                                ServerError::McpOperation(err_msg.to_string())
                            })?;
                            let client = AuthClient::new(reqwest::Client::default(), am);
                            let transport = SseClientTransport::start_with_client(
                                client,
                                SseClientConfig {
                                    sse_endpoint: url.into(),
                                    ..Default::default()
                                },
                            )
                            .await
                            .map_err(|e| {
                                let err_msg = format!("Failed to create authorized transport: {e}");
                                dual_error!("{}", err_msg);
                                ServerError::McpOperation(err_msg)
                            })?;

                            // Create client and connect to MCP server
                            let client_info = ClientInfo {
                                protocol_version: Default::default(),
                                capabilities: ClientCapabilities::default(),
                                client_info: Implementation {
                                    name: env!("CARGO_PKG_NAME").to_string(),
                                    version: env!("CARGO_PKG_VERSION").to_string(),
                                    title: None,
                                    icons: None,
                                    website_url: None,
                                },
                            };
                            let service =
                                client_info.into_dyn().serve(transport).await.map_err(|e| {
                                    let err_msg = format!(
                                        "Failed to connect to mcp server (name: {}, url: {}, transport: {}). {e}. Please check if the mcp server is running.",
                                        self.name, server_url, self.transport
                                    );
                                    dual_error!("{}", &err_msg);
                                    ServerError::McpOperation(err_msg)
                                })?;
                            tracing::info!("Successfully connected to MCP server");

                            service
                        }
                    };

                    // get mcp service name
                    let service_name = match service.peer_info() {
                        Some(peer_info) => peer_info.server_info.name.clone(),
                        None => self.name.clone(),
                    };
                    // update server name
                    self.server_name = Some(service_name.clone());

                    // list tools
                    let tools = service.list_all_tools().await.map_err(|e| {
                        let err_msg = format!("Failed to list tools: {e}");
                        dual_error!("{}", &err_msg);
                        ServerError::McpOperation(err_msg)
                    })?;
                    dual_info!("Found {} tools from {} mcp server", tools.len(), self.name,);

                    dual_debug!(
                        "Retrieved mcp tools: {}",
                        serde_json::to_string_pretty(&tools).unwrap()
                    );

                    // update tools
                    self.tools = Some(tools.clone());

                    let mut client = McpService::new(&service_name, service);
                    client.tools = tools.iter().map(|tool| tool.name.to_string()).collect();
                    client.fallback_message = self.fallback_message.clone();

                    // print name of all tools
                    for (idx, tool) in tools.iter().enumerate() {
                        dual_debug!(
                            "Tool {} - name: {}, description: {}",
                            idx,
                            tool.name,
                            tool.description.as_deref().unwrap_or("No description"),
                        );
                    }

                    // add mcp client to MCP_CLIENTS
                    match MCP_SERVICES.get() {
                        Some(clients) => {
                            let mut clients = clients.write().await;
                            if clients.contains_key(&service_name) {
                                let err_msg = format!("Mcp client {service_name} already exists");
                                dual_error!("{}", err_msg);
                                return Err(ServerError::Operation(err_msg));
                            }
                            clients.insert(service_name, TokioRwLock::new(client));
                        }
                        None => {
                            MCP_SERVICES
                                .set(TokioRwLock::new(HashMap::from([(
                                    service_name,
                                    TokioRwLock::new(client),
                                )])))
                                .map_err(|_| {
                                    let err_msg = "Failed to set MCP_CLIENTS";
                                    dual_error!("{}", err_msg);
                                    ServerError::Operation(err_msg.to_string())
                                })?;
                        }
                    }
                }
                McpTransport::StreamHttp => {
                    // Dynamically append API key to URL if configured
                    let url = if let (Some(api_key), Some(param_name)) =
                        (&self.api_key, &self.api_key_param)
                    {
                        if !api_key.is_empty() {
                            let base = server_url.trim_end_matches('/');
                            let separator = if base.contains('?') { "&" } else { "?" };
                            format!(
                                "{}{}{}={}",
                                base,
                                separator,
                                param_name,
                                urlencoding::encode(api_key)
                            )
                        } else {
                            server_url.trim_end_matches('/').to_string()
                        }
                    } else {
                        server_url.trim_end_matches('/').to_string()
                    };

                    let service = match use_oauth {
                        false => {
                            Url::parse(&url).map_err(|e| {
                                let err_msg =
                                    format!("Invalid mcp tools stream-http URL: {url}. {e}",);
                                dual_error!("{}", err_msg);
                                ServerError::Operation(err_msg)
                            })?;
                            dual_debug!("Sync mcp tools from mcp server: {}", url);

                            // create a stream-http transport
                            let transport = StreamableHttpClientTransport::from_uri(url.as_str());

                            // create a mcp client
                            let client_info = ClientInfo {
                                protocol_version: Default::default(),
                                capabilities: ClientCapabilities::default(),
                                client_info: Implementation {
                                    name: env!("CARGO_PKG_NAME").to_string(),
                                    version: env!("CARGO_PKG_VERSION").to_string(),
                                    title: None,
                                    icons: None,
                                    website_url: None,
                                },
                            };
                            client_info.into_dyn().serve(transport).await.map_err(|e| {
                                let err_msg = format!(
                                    "Failed to connect to mcp server (name: {}, url: {}, transport: {}). {e}. Please check if the mcp server is running.",
                                    self.name, server_url, self.transport
                                );
                                dual_error!("{}", &err_msg);
                                ServerError::McpOperation(err_msg)
                            })?
                        }
                        true => {
                            // it is a http server for handling callback
                            // Create channel for receiving authorization code
                            let (code_sender, code_receiver) = oneshot::channel::<String>();

                            // Create app state

                            let app_state = AppState {
                                code_receiver: Arc::new(Mutex::new(Some(code_sender))),
                            };

                            // Start HTTP server for handling callbacks
                            let app = Router::new()
                                .route("/callback", get(callback_handler))
                                .with_state(app_state);

                            let addr = SocketAddr::from(([127, 0, 0, 1], CALLBACK_PORT));
                            tracing::info!("Starting callback server at: http://{}", addr);

                            // Start server in a separate task
                            tokio::spawn(async move {
                                let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
                                let result = axum::serve(listener, app).await;

                                if let Err(e) = result {
                                    tracing::error!("Callback server error: {}", e);
                                }
                            });

                            // Get server URL
                            tracing::info!("Using MCP server OAuth URL: {}", url);

                            // Initialize oauth state machine
                            let mut oauth_state =
                                OAuthState::new(&url, None).await.map_err(|e| {
                                    let err_msg =
                                        format!("Failed to initialize oauth state machine: {e}");
                                    dual_error!("{}", err_msg);
                                    ServerError::McpOperation(err_msg)
                                })?;

                            // Get metadata to view supported scopes
                            if let OAuthState::Unauthorized(manager) = &mut oauth_state {
                                let metadata = manager.discover_metadata().await.map_err(|e| {
                                    let err_msg = format!("Failed to discover metadata: {e}");
                                    dual_error!("{}", err_msg);
                                    ServerError::McpOperation(err_msg.to_string())
                                })?;
                                if let Some(supported_scopes) = metadata.scopes_supported {
                                    dual_debug!("Server supported scopes: {:?}", supported_scopes);
                                    // Use server supported scopes
                                    oauth_state
                                        .start_authorization(
                                            &supported_scopes
                                                .iter()
                                                .map(|s| s.as_str())
                                                .collect::<Vec<_>>(),
                                            MCP_REDIRECT_URI,
                                        )
                                        .await
                                        .map_err(|e| {
                                            let err_msg =
                                                format!("Failed to start authorization: {e}");
                                            dual_error!("{}", err_msg);
                                            ServerError::McpOperation(err_msg)
                                        })?;
                                } else {
                                    let err_msg = "Failed to get supported scopes from mcp server";
                                    dual_error!("{}", err_msg);
                                    return Err(ServerError::McpOperation(err_msg.to_string()));
                                }
                            }

                            // Output authorization URL to user
                            let mut output = BufWriter::new(tokio::io::stdout());
                            output
                                .write_all(b"\n=== MCP OAuth Client ===\n\n")
                                .await
                                .map_err(|e| {
                                    let err_msg = format!("Failed to write to stdout: {e}");
                                    dual_error!("{}", err_msg);
                                    ServerError::McpOperation(err_msg)
                                })?;
                            output.write_all(b"Please open the following URL in your browser to authorize:\n\n")
                            .await.map_err(|e| {
                                let err_msg = format!("Failed to write to stdout: {e}");
                                dual_error!("{}", err_msg);
                                ServerError::McpOperation(err_msg)
                            })?;

                            output
                                .write_all(
                                    oauth_state
                                        .get_authorization_url()
                                        .await
                                        .map_err(|e| {
                                            let err_msg =
                                                format!("Failed to get authorization url: {e}");
                                            dual_error!("{}", err_msg);
                                            ServerError::McpOperation(err_msg)
                                        })?
                                        .as_bytes(),
                                )
                                .await
                                .map_err(|e| {
                                    let err_msg = format!("Failed to write to stdout: {e}");
                                    dual_error!("{}", err_msg);
                                    ServerError::McpOperation(err_msg)
                                })?;
                            output
                                .write_all(b"\n\nWaiting for browser callback, please do not close this window...\n")
                                .await.map_err(|e| {
                                    let err_msg = format!("Failed to write to stdout: {e}");
                                    dual_error!("{}", err_msg);
                                    ServerError::McpOperation(err_msg)
                                })?;
                            output.flush().await.map_err(|e| {
                                let err_msg = format!("Failed to flush stdout: {e}");
                                dual_error!("{}", err_msg);
                                ServerError::McpOperation(err_msg)
                            })?;

                            // Wait for authorization code
                            tracing::info!("Waiting for authorization code...");
                            let auth_code = code_receiver.await.map_err(|e| {
                                let err_msg = format!("Failed to get authorization code: {e}");
                                dual_error!("{}", err_msg);
                                ServerError::McpOperation(err_msg)
                            })?;
                            tracing::info!("Received authorization code: {}", auth_code);
                            // Exchange code for access token
                            tracing::info!("Exchanging authorization code for access token...");
                            oauth_state.handle_callback(&auth_code).await.map_err(|e| {
                                let err_msg = format!("Failed to handle callback: {e}");
                                dual_error!("{}", err_msg);
                                ServerError::McpOperation(err_msg)
                            })?;
                            tracing::info!("Successfully obtained access token");

                            output
                                .write_all(
                                    b"\nAuthorization successful! Access token obtained.\n\n",
                                )
                                .await
                                .map_err(|e| {
                                    let err_msg = format!("Failed to write to stdout: {e}");
                                    dual_error!("{}", err_msg);
                                    ServerError::McpOperation(err_msg)
                                })?;
                            output.flush().await.map_err(|e| {
                                let err_msg = format!("Failed to flush stdout: {e}");
                                dual_error!("{}", err_msg);
                                ServerError::McpOperation(err_msg)
                            })?;

                            // Create authorized transport, this transport is authorized by the oauth state machine
                            tracing::info!("Establishing authorized connection to MCP server...");
                            let am = oauth_state.into_authorization_manager().ok_or_else(|| {
                                let err_msg = "Failed to get authorization manager";
                                dual_error!("{}", err_msg);
                                ServerError::McpOperation(err_msg.to_string())
                            })?;
                            let client = AuthClient::new(reqwest::Client::default(), am);

                            // Use StreamableHttpClientTransport
                            let transport = StreamableHttpClientTransport::with_client(
                                client,
                                StreamableHttpClientTransportConfig {
                                    uri: url.into(),
                                    ..Default::default()
                                },
                            );

                            // Create client and connect to MCP server
                            let client_info = ClientInfo {
                                protocol_version: Default::default(),
                                capabilities: ClientCapabilities::default(),
                                client_info: Implementation {
                                    name: env!("CARGO_PKG_NAME").to_string(),
                                    version: env!("CARGO_PKG_VERSION").to_string(),
                                    title: None,
                                    icons: None,
                                    website_url: None,
                                },
                            };
                            client_info.into_dyn().serve(transport).await.map_err(|e| {
                                let err_msg = format!(
                                    "Failed to connect to mcp server (name: {}, url: {}, transport: {}). {e}. Please check if the mcp server is running.",
                                    self.name, server_url, self.transport
                                );
                                dual_error!("{}", &err_msg);
                                ServerError::McpOperation(err_msg)
                            })?
                        }
                    };

                    // get mcp service name
                    let service_name = match service.peer_info() {
                        Some(peer_info) => peer_info.server_info.name.clone(),
                        None => self.name.clone(),
                    };
                    // update server name
                    self.server_name = Some(service_name.clone());

                    // list tools
                    let tools = service.list_all_tools().await.map_err(|e| {
                        let err_msg = format!("Failed to list tools: {e}");
                        dual_error!("{}", &err_msg);
                        ServerError::McpOperation(err_msg)
                    })?;
                    dual_info!("Found {} tools from {} mcp server", tools.len(), self.name,);

                    dual_debug!(
                        "Retrieved mcp tools: {}",
                        serde_json::to_string_pretty(&tools).unwrap()
                    );

                    // update tools
                    self.tools = Some(tools.clone());

                    // create mcp client
                    let mut client = McpService::new(&service_name, service);
                    client.tools = tools.iter().map(|tool| tool.name.to_string()).collect();
                    client.fallback_message = self.fallback_message.clone();

                    // print name of all tools
                    for (idx, tool) in tools.iter().enumerate() {
                        dual_debug!(
                            "Tool {} - name: {}, description: {}",
                            idx,
                            tool.name,
                            tool.description.as_deref().unwrap_or("No description"),
                        );
                    }

                    // add mcp client to MCP_CLIENTS
                    match MCP_SERVICES.get() {
                        Some(clients) => {
                            let mut clients = clients.write().await;
                            if clients.contains_key(&service_name) {
                                let err_msg = format!("Mcp client {service_name} already exists");
                                dual_error!("{}", err_msg);
                                return Err(ServerError::Operation(err_msg));
                            }
                            clients.insert(service_name, TokioRwLock::new(client));
                        }
                        None => {
                            MCP_SERVICES
                                .set(TokioRwLock::new(HashMap::from([(
                                    service_name,
                                    TokioRwLock::new(client),
                                )])))
                                .map_err(|_| {
                                    let err_msg = "Failed to set MCP_CLIENTS";
                                    dual_error!("{}", err_msg);
                                    ServerError::Operation(err_msg.to_string())
                                })?;
                        }
                    }
                }
                // Stdio is handled by connect_stdio() above, before URL validation
                McpTransport::Stdio => unreachable!("stdio handled before URL validation"),
            }
        }

        Ok(())
    }

    /// Connect a stdio MCP server using rmcp's TokioChildProcess.
    ///
    /// Spawns the child process, performs MCP handshake, discovers tools,
    /// and registers the resulting `RunningService` into `MCP_SERVICES`
    /// (same path as SSE/StreamHTTP).
    async fn connect_stdio(&mut self) -> ServerResult<()> {
        let command = self.command.as_ref().ok_or_else(|| {
            let err_msg = format!(
                "Invalid configuration for stdio mcp server '{}': 'command' is required",
                self.name
            );
            dual_error!("{}", err_msg);
            ServerError::Operation(err_msg)
        })?;

        dual_warn!(
            "[mcp-stdio:{}] Starting child process '{}' with Moss Agent privileges. \
             Ensure you trust this MCP server.",
            self.name,
            command
        );

        // Build command
        let mut cmd = tokio::process::Command::new(command);
        cmd.args(self.args.as_deref().unwrap_or_default());
        if let Some(ref env_vars) = self.env {
            for (key, value) in env_vars {
                cmd.env(key, value);
            }
        }
        if let Some(ref dir) = self.working_dir {
            cmd.current_dir(dir);
        }

        // Spawn process via TokioChildProcess (captures stderr for logging)
        let (child_process, stderr) = TokioChildProcess::builder(cmd)
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                let err_msg = format!(
                    "Failed to start stdio MCP server '{}' (command: {}): {e}",
                    self.name, command
                );
                dual_error!("{}", &err_msg);
                ServerError::McpOperation(err_msg)
            })?;

        let pid = child_process.id();
        dual_info!("[mcp-stdio:{}] Process started (PID: {:?})", self.name, pid);

        // Spawn stderr logger
        if let Some(stderr) = stderr {
            let name = self.name.clone();
            tokio::spawn(async move {
                mcp_stdio::log_stderr(name, stderr).await;
            });
        }

        // Create MCP client and perform handshake
        let client_info = ClientInfo {
            protocol_version: Default::default(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: env!("CARGO_PKG_NAME").to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                icons: None,
                website_url: None,
            },
        };

        let service = client_info
            .into_dyn()
            .serve(child_process)
            .await
            .map_err(|e| {
                let err_msg = format!(
                    "Failed to connect to stdio MCP server (name: {}, command: {}). {e}. \
                     Please check if the command is valid and the MCP server starts correctly.",
                    self.name, command
                );
                dual_error!("{}", &err_msg);
                ServerError::McpOperation(err_msg)
            })?;

        // Get service name from server
        let service_name = match service.peer_info() {
            Some(peer_info) => peer_info.server_info.name.clone(),
            None => self.name.clone(),
        };
        self.server_name = Some(service_name.clone());

        // Discover tools
        let tools = service.list_all_tools().await.map_err(|e| {
            let err_msg = format!(
                "Failed to list tools from stdio server '{}': {e}",
                self.name
            );
            dual_error!("{}", &err_msg);
            ServerError::McpOperation(err_msg)
        })?;
        dual_info!(
            "Found {} tools from {} stdio mcp server",
            tools.len(),
            self.name,
        );

        dual_debug!(
            "Retrieved mcp tools: {}",
            serde_json::to_string_pretty(&tools).unwrap()
        );

        // Update tools
        self.tools = Some(tools.clone());

        let mut client = McpService::new(&service_name, service);
        client.tools = tools.iter().map(|tool| tool.name.to_string()).collect();
        client.fallback_message = self.fallback_message.clone();

        // Log all discovered tools
        for (idx, tool) in tools.iter().enumerate() {
            dual_debug!(
                "Tool {} - name: {}, description: {}",
                idx,
                tool.name,
                tool.description.as_deref().unwrap_or("No description"),
            );
        }

        // Register to MCP_SERVICES (same as SSE/StreamHTTP path)
        match MCP_SERVICES.get() {
            Some(clients) => {
                let mut clients = clients.write().await;
                if clients.contains_key(&service_name) {
                    let err_msg = format!("Mcp client {service_name} already exists");
                    dual_error!("{}", err_msg);
                    return Err(ServerError::Operation(err_msg));
                }
                clients.insert(service_name.clone(), TokioRwLock::new(client));
            }
            None => {
                MCP_SERVICES
                    .set(TokioRwLock::new(HashMap::from([(
                        service_name.clone(),
                        TokioRwLock::new(client),
                    )])))
                    .map_err(|_| {
                        let err_msg = "Failed to set MCP_CLIENTS";
                        dual_error!("{}", err_msg);
                        ServerError::Operation(err_msg.to_string())
                    })?;
            }
        }

        // Register config and process to StdioProcessManager for health check & recovery
        let stdio_config = StdioProcessConfig {
            name: self.name.clone(),
            command: command.clone(),
            args: self.args.clone().unwrap_or_default(),
            env: self.env.clone().unwrap_or_default(),
            working_dir: self.working_dir.clone(),
            enable: self.enable,
            stdio: self.stdio.clone(),
        };
        let manager = mcp_stdio::get_stdio_process_manager();
        manager.register_config(stdio_config).await;
        manager
            .register_process(
                &self.name,
                service_name.clone(),
                pid,
                self.fallback_message.clone(),
            )
            .await;

        dual_info!(
            "[mcp-stdio:{}] Stdio MCP server connected and registered (service_name: {})",
            self.name,
            service_name
        );

        Ok(())
    }
}

#[derive(Debug, Clone)]
struct AppState {
    code_receiver: Arc<Mutex<Option<oneshot::Sender<String>>>>,
}

#[derive(Debug, Deserialize)]
struct CallbackParams {
    code: String,
    #[allow(dead_code)]
    state: Option<String>,
}

async fn callback_handler(
    Query(params): Query<CallbackParams>,
    State(state): State<AppState>,
) -> Html<String> {
    tracing::info!("Received callback with code: {}", params.code);

    // Send the code to the main thread
    if let Some(sender) = state.code_receiver.lock().await.take() {
        let _ = sender.send(params.code);
    }
    // Return success page
    Html(CALLBACK_HTML.to_string())
}

/// Artifacts management configuration
///
/// Controls the behavior of the Artifacts storage and management system.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ArtifactsConfig {
    /// Enable or disable Artifacts functionality
    #[serde(default = "default_artifacts_enabled")]
    pub enabled: bool,

    /// Path to SQLite database file for storing artifact metadata
    #[serde(default = "default_artifacts_database_path")]
    pub database_path: String,

    /// Path to storage directory for artifact content
    /// If not specified, uses system default data directory
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_path: Option<String>,

    /// Maximum content size for text artifacts in bytes (default: 1MB)
    #[serde(default = "default_artifacts_max_size")]
    pub max_content_size: u64,

    /// Maximum content size for binary artifacts in bytes (default: 100MB)
    #[serde(default = "default_artifacts_max_binary_size")]
    pub max_binary_size: u64,

    // ========== Lifecycle Configuration ==========
    /// Artifact retention days (0 = never expire, default: 30)
    #[serde(default = "default_artifacts_retention_days")]
    pub retention_days: u32,

    /// Cleanup interval in seconds (default: 3600 = 1 hour)
    #[serde(default = "default_artifacts_cleanup_interval")]
    pub cleanup_interval_secs: u64,

    /// Days to keep soft-deleted artifacts before physical deletion (default: 7)
    #[serde(default = "default_artifacts_soft_delete_retention")]
    pub soft_delete_retention_days: u32,

    /// Enable automatic cleanup (default: true)
    #[serde(default = "default_artifacts_enable_cleanup")]
    pub enable_cleanup: bool,
}

fn default_artifacts_enabled() -> bool {
    false
}

fn default_artifacts_database_path() -> String {
    "data/artifacts.db".to_string()
}

fn default_artifacts_max_size() -> u64 {
    1024 * 1024 // 1MB
}

fn default_artifacts_max_binary_size() -> u64 {
    100 * 1024 * 1024 // 100MB
}

fn default_artifacts_retention_days() -> u32 {
    30
}

fn default_artifacts_cleanup_interval() -> u64 {
    3600 // 1 hour
}

fn default_artifacts_soft_delete_retention() -> u32 {
    7
}

fn default_artifacts_enable_cleanup() -> bool {
    true
}

impl Default for ArtifactsConfig {
    fn default() -> Self {
        Self {
            enabled: default_artifacts_enabled(),
            database_path: default_artifacts_database_path(),
            storage_path: None,
            max_content_size: default_artifacts_max_size(),
            max_binary_size: default_artifacts_max_binary_size(),
            retention_days: default_artifacts_retention_days(),
            cleanup_interval_secs: default_artifacts_cleanup_interval(),
            soft_delete_retention_days: default_artifacts_soft_delete_retention(),
            enable_cleanup: default_artifacts_enable_cleanup(),
        }
    }
}

/// Skills configuration
///
/// Controls the behavior of Agent Skills support.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SkillConfig {
    /// Enable or disable Skills functionality
    #[serde(default = "default_skills_enabled")]
    pub enabled: bool,

    /// List of directories to scan for Skills
    /// Each directory should contain skill subdirectories with SKILL.md files
    #[serde(default = "default_skills_directories")]
    pub directories: Vec<String>,

    /// Script execution configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution: Option<ExecutionConfig>,

    /// Maximum total size of reference documents to load per skill (in bytes)
    /// Set to 0 for no limit. Default: 102400 (100KB)
    #[serde(default = "default_max_reference_size")]
    pub max_reference_size: usize,

    /// API configuration for Skills management endpoints
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<SkillApiConfig>,

    /// Skills marketplace configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market: Option<SkillMarketConfig>,
}

impl SkillConfig {
    /// Get the primary skills directory (first in the list)
    ///
    /// Returns the first directory from `directories`, expanding `~`.
    /// Falls back to the default app home skills directory if empty.
    pub fn directory(&self) -> String {
        self.directories
            .first()
            .cloned()
            .unwrap_or_else(|| app_home_path("skills"))
    }
}

/// Skills API configuration for authentication and rate limiting
///
/// Controls access to Skills management API endpoints.
///
/// # Example Configuration
///
/// ```toml
/// [skill.api]
/// api_key = "sk-your-secret-key"
/// rate_limit_requests = 100
/// rate_limit_window_secs = 60
/// ```
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SkillApiConfig {
    /// API key for authenticating Skills API requests.
    /// If empty, no authentication is required.
    /// Can also be set via SKILLS_API_KEY environment variable.
    #[serde(default)]
    api_key: String,

    /// Maximum number of requests allowed within the rate limit window.
    /// Set to 0 to disable rate limiting. Default: 100
    #[serde(default = "default_rate_limit_requests")]
    pub rate_limit_requests: u32,

    /// Rate limit window duration in seconds. Default: 60
    #[serde(default = "default_rate_limit_window_secs")]
    pub rate_limit_window_secs: u64,
}

impl SkillApiConfig {
    /// Get the API key, checking environment variable as fallback
    pub fn get_api_key(&self) -> Option<String> {
        if !self.api_key.is_empty() {
            Some(self.api_key.clone())
        } else {
            std::env::var("SKILLS_API_KEY").ok()
        }
    }

    /// Check if rate limiting is enabled
    pub fn rate_limiting_enabled(&self) -> bool {
        self.rate_limit_requests > 0
    }
}

impl Default for SkillApiConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            rate_limit_requests: default_rate_limit_requests(),
            rate_limit_window_secs: default_rate_limit_window_secs(),
        }
    }
}

fn default_rate_limit_requests() -> u32 {
    100 // 100 requests per window
}

fn default_rate_limit_window_secs() -> u64 {
    60 // 60 seconds window
}

fn default_skills_enabled() -> bool {
    true
}

fn default_skills_directories() -> Vec<String> {
    vec![app_home_path("skills")]
}

fn default_max_reference_size() -> usize {
    102400 // 100KB
}

impl Default for SkillConfig {
    fn default() -> Self {
        Self {
            enabled: default_skills_enabled(),
            directories: default_skills_directories(),
            execution: None,
            max_reference_size: default_max_reference_size(),
            api: None,
            market: None,
        }
    }
}

/// Skills marketplace configuration
///
/// Configures access to skills marketplace (skillsmp.com) for remote skill installation.
///
/// # Example Configuration
///
/// ```toml
/// [skill.market]
/// url = "https://skillsmp.com/api/v1"
/// api_key = "sk_live_your_api_key"
/// ```
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SkillMarketConfig {
    /// Base URL for the skills marketplace API
    /// Default: https://skillsmp.com/api/v1
    #[serde(default = "default_market_url")]
    pub url: String,

    /// API key for authenticating marketplace requests
    /// Can also be set via SKILLSMP_API_KEY environment variable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Cache directory for downloaded skills
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_dir: Option<String>,
}

impl SkillMarketConfig {
    /// Returns the cache directory with tilde expanded, if configured.
    pub fn expanded_cache_dir(&self) -> Option<String> {
        self.cache_dir
            .as_ref()
            .map(|dir| shellexpand::tilde(dir).to_string())
    }
}

impl Default for SkillMarketConfig {
    fn default() -> Self {
        Self {
            url: default_market_url(),
            api_key: None,
            cache_dir: None,
        }
    }
}

fn default_market_url() -> String {
    "https://skillsmp.com/api/v1".to_string()
}

/// Script execution configuration
///
/// Configures script executors for Skills that include executable scripts.
/// Multiple executor backends are supported (Docker, Deno, etc.).
///
/// # Example Configuration
///
/// ```toml
/// [skill.execution]
/// enabled = true
/// preferred_executor = "deno"
///
/// [skill.execution.limits]
/// max_memory_bytes = 268435456  # 256MB
/// timeout = "30s"
/// network_access = false
///
/// [skill.execution.deno]
/// deno_path = "/usr/local/bin/deno"
/// allow_net = false
/// allow_env = true
///
/// [skill.execution.docker]
/// default_image = "python:3.11-slim"
/// auto_remove = true
/// ```
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ExecutionConfig {
    /// Enable or disable script execution (default: true)
    #[serde(default = "default_execution_enabled")]
    pub enabled: bool,

    /// Preferred executor backend: "deno", "docker", or "auto"
    /// When set to "auto", selects executor based on file extension
    #[serde(default = "default_preferred_executor")]
    pub preferred_executor: String,

    /// Default resource limits for all executors
    #[serde(default)]
    pub limits: ResourceLimits,

    /// Deno executor configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deno: Option<DenoConfig>,

    /// Docker executor configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docker: Option<DockerConfig>,

    /// Wasmtime executor configuration (WebAssembly sandbox)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wasmtime: Option<WasmtimeConfig>,
}

fn default_execution_enabled() -> bool {
    true
}

fn default_preferred_executor() -> String {
    "auto".to_string()
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            enabled: default_execution_enabled(),
            preferred_executor: default_preferred_executor(),
            limits: ResourceLimits::default(),
            deno: None,
            docker: None,
            wasmtime: None,
        }
    }
}

// ============================================================================
// Config API Settings (Hot Reload)
// ============================================================================

/// Configuration API settings
///
/// Controls hot-reload behavior and other Config API features.
///
/// # Example Configuration
///
/// ```toml
/// [config_api]
/// hot_reload_enabled = true
/// hot_reload_debounce_ms = 500
/// hot_reload_keep_on_invalid = true
/// hot_reload_audit = false
/// ```
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConfigApiSettings {
    /// Enable configuration hot-reload file watching
    #[serde(default)]
    pub hot_reload_enabled: bool,

    /// Debounce delay in milliseconds for file change events
    /// Prevents multiple triggers when file is being saved
    #[serde(default = "default_hot_reload_debounce_ms")]
    pub hot_reload_debounce_ms: u64,

    /// Keep current configuration when new config is invalid
    /// If true, invalid configurations are logged but not applied
    #[serde(default = "default_hot_reload_keep_on_invalid")]
    pub hot_reload_keep_on_invalid: bool,

    /// Log hot-reload changes to audit log
    #[serde(default)]
    pub hot_reload_audit: bool,
}

fn default_hot_reload_debounce_ms() -> u64 {
    500 // 500ms debounce delay
}

fn default_hot_reload_keep_on_invalid() -> bool {
    true // Keep current config on invalid
}

impl Default for ConfigApiSettings {
    fn default() -> Self {
        Self {
            hot_reload_enabled: false,
            hot_reload_debounce_ms: default_hot_reload_debounce_ms(),
            hot_reload_keep_on_invalid: default_hot_reload_keep_on_invalid(),
            hot_reload_audit: false,
        }
    }
}

// ============================================================================
// Session History Configuration
// ============================================================================

/// Session history configuration
///
/// Controls JSONL-based chat history persistence for normal (non-privacy) sessions.
///
/// # Example Configuration
///
/// ```toml
/// [session]
/// enable = true
/// storage_path = "~/.moss/sessions"
/// ```
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SessionConfig {
    /// Enable or disable session history recording
    #[serde(default = "default_session_enabled")]
    pub enable: bool,

    /// Directory for storing session JSONL files
    /// Supports `~` for home directory expansion.
    /// Layout: {storage_path}/{user_id}/{session_id}.jsonl
    #[serde(default = "default_session_storage_path")]
    pub storage_path: String,
}

fn default_session_enabled() -> bool {
    true
}

fn default_session_storage_path() -> String {
    app_home_path("sessions")
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            enable: default_session_enabled(),
            storage_path: default_session_storage_path(),
        }
    }
}

// ─── Lantai knowledge base configuration ─────────────────────────────────────

/// Lantai knowledge base configuration for Moss integration.
///
/// Requires `[embedding]` section to be configured for generating embeddings.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MossLantaiConfig {
    /// Enable or disable Lantai knowledge base
    #[serde(default)]
    pub enabled: bool,
    /// Directory containing markdown files to index
    #[serde(default = "default_lantai_memory_dir")]
    pub memory_dir: String,
    /// Path to SQLite database file
    #[serde(default = "default_lantai_database_path")]
    pub database_path: String,
    /// Embedding model configuration (URL and API key come from [embedding] section)
    #[serde(default)]
    pub embedding: LantaiEmbeddingSubConfig,
    /// Chunking configuration
    #[serde(default)]
    pub chunking: LantaiChunkingSubConfig,
    /// Search configuration
    #[serde(default)]
    pub search: LantaiSearchSubConfig,
    /// File watcher configuration
    #[serde(default)]
    pub watch: LantaiWatchSubConfig,
    /// Auto-memory configuration
    #[serde(default)]
    pub auto_memory: LantaiAutoMemoryConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LantaiEmbeddingSubConfig {
    #[serde(default = "default_lantai_embedding_model")]
    pub model: String,
    #[serde(default = "default_lantai_embedding_dimensions")]
    pub dimensions: usize,
    #[serde(default = "default_lantai_embedding_batch_size")]
    pub batch_size: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LantaiChunkingSubConfig {
    #[serde(default = "default_lantai_max_chunk_lines")]
    pub max_chunk_lines: usize,
    #[serde(default = "default_lantai_min_chunk_lines")]
    pub min_chunk_lines: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LantaiSearchSubConfig {
    #[serde(default = "default_lantai_search_limit")]
    pub default_limit: usize,
    #[serde(default = "default_lantai_vec_weight")]
    pub vec_weight: f64,
    #[serde(default = "default_lantai_bm25_weight")]
    pub bm25_weight: f64,
    #[serde(default = "default_lantai_rrf_k")]
    pub rrf_k: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LantaiWatchSubConfig {
    #[serde(default = "default_lantai_watch_enabled")]
    pub enabled: bool,
    #[serde(default = "default_lantai_watch_debounce_ms")]
    pub debounce_ms: u64,
}

// Default value functions
fn default_lantai_memory_dir() -> String {
    "~/.lantai/memory".to_string()
}
fn default_lantai_database_path() -> String {
    "~/.lantai/lantai.db".to_string()
}
fn default_lantai_embedding_model() -> String {
    "text-embedding-3-small".to_string()
}
fn default_lantai_embedding_dimensions() -> usize {
    1536
}
fn default_lantai_embedding_batch_size() -> usize {
    32
}
fn default_lantai_max_chunk_lines() -> usize {
    50
}
fn default_lantai_min_chunk_lines() -> usize {
    5
}
fn default_lantai_search_limit() -> usize {
    5
}
fn default_lantai_vec_weight() -> f64 {
    0.6
}
fn default_lantai_bm25_weight() -> f64 {
    0.4
}
fn default_lantai_rrf_k() -> u32 {
    60
}
fn default_lantai_watch_enabled() -> bool {
    true
}
fn default_lantai_watch_debounce_ms() -> u64 {
    1500
}

impl Default for MossLantaiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            memory_dir: default_lantai_memory_dir(),
            database_path: default_lantai_database_path(),
            embedding: LantaiEmbeddingSubConfig::default(),
            chunking: LantaiChunkingSubConfig::default(),
            search: LantaiSearchSubConfig::default(),
            watch: LantaiWatchSubConfig::default(),
            auto_memory: LantaiAutoMemoryConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LantaiAutoMemoryConfig {
    /// Enable context injection at session start (default: true)
    #[serde(default = "default_true")]
    pub context_injection: bool,
    /// Enable auto-summary of conversations to daily log (default: true)
    #[serde(default = "default_true")]
    pub auto_summary: bool,
    /// Max characters for injected memory context (default: 2000)
    #[serde(default = "default_max_context_chars")]
    pub max_context_chars: usize,
    /// Checkpoint trigger ratio: prompt_tokens >= model_context_size * ratio (default: 0.75)
    #[serde(default = "default_checkpoint_token_ratio")]
    pub checkpoint_token_ratio: f32,
    /// Enable auto-compaction of MEMORY.md / EXPERIENCE.md (default: true)
    #[serde(default = "default_true")]
    pub compaction_enabled: bool,
    /// File size threshold (chars) to trigger compaction (default: 4000)
    #[serde(default = "default_compaction_threshold")]
    pub compaction_threshold: usize,
}

fn default_true() -> bool {
    true
}
fn default_max_context_chars() -> usize {
    2000
}
fn default_checkpoint_token_ratio() -> f32 {
    0.75
}
fn default_compaction_threshold() -> usize {
    4000
}

impl Default for LantaiAutoMemoryConfig {
    fn default() -> Self {
        Self {
            context_injection: true,
            auto_summary: true,
            max_context_chars: default_max_context_chars(),
            checkpoint_token_ratio: default_checkpoint_token_ratio(),
            compaction_enabled: true,
            compaction_threshold: default_compaction_threshold(),
        }
    }
}

impl Default for LantaiEmbeddingSubConfig {
    fn default() -> Self {
        Self {
            model: default_lantai_embedding_model(),
            dimensions: default_lantai_embedding_dimensions(),
            batch_size: default_lantai_embedding_batch_size(),
        }
    }
}

impl Default for LantaiChunkingSubConfig {
    fn default() -> Self {
        Self {
            max_chunk_lines: default_lantai_max_chunk_lines(),
            min_chunk_lines: default_lantai_min_chunk_lines(),
        }
    }
}

impl Default for LantaiSearchSubConfig {
    fn default() -> Self {
        Self {
            default_limit: default_lantai_search_limit(),
            vec_weight: default_lantai_vec_weight(),
            bm25_weight: default_lantai_bm25_weight(),
            rrf_k: default_lantai_rrf_k(),
        }
    }
}

impl Default for LantaiWatchSubConfig {
    fn default() -> Self {
        Self {
            enabled: default_lantai_watch_enabled(),
            debounce_ms: default_lantai_watch_debounce_ms(),
        }
    }
}

impl MossLantaiConfig {
    /// Convert to lantai crate's LantaiConfig, with expanded paths.
    pub fn to_lantai_config(&self, memory_dir: &str, database_path: &str) -> lantai::LantaiConfig {
        lantai::LantaiConfig {
            memory_dir: memory_dir.to_string(),
            database_path: database_path.to_string(),
            chunking: lantai::config::ChunkingConfig {
                max_chunk_lines: self.chunking.max_chunk_lines,
                min_chunk_lines: self.chunking.min_chunk_lines,
            },
            embedding: lantai::config::EmbeddingConfig {
                model: self.embedding.model.clone(),
                dimensions: self.embedding.dimensions,
                batch_size: self.embedding.batch_size,
            },
            search: lantai::config::SearchConfig {
                default_limit: self.search.default_limit,
                vec_weight: self.search.vec_weight,
                bm25_weight: self.search.bm25_weight,
                rrf_k: self.search.rrf_k,
            },
            watch: Some(lantai::config::WatchConfig {
                debounce_ms: self.watch.debounce_ms,
            }),
        }
    }
}
