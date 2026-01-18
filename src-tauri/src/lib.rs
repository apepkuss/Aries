use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use aries::chat::emitter::EventEmitter;
use aries::chat::events::{
    ExecutionPhase, ExecutionSummary, FinishEvent, StatusEvent, StreamEvent, TextEvent,
    ThoughtEvent, ThoughtStatus, ToolCallEvent, ToolResultEvent,
};
use aries::chat::trace::TokenUsage;
use aries::{AppState, AriesEngine};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutState};
use tokio::sync::RwLock;

// ============================================================================
// TauriEventEmitter - Bridges aries EventEmitter to Tauri IPC events
// ============================================================================

/// Event emitter that sends execution events to the Tauri frontend via IPC.
///
/// This struct implements the `EventEmitter` trait from the aries crate,
/// converting structured events into Tauri events that the frontend can listen to.
pub struct TauriEventEmitter {
    /// Handle to the Tauri application for emitting events.
    app_handle: AppHandle,
    /// Unique request ID for correlating events.
    request_id: String,
}

impl TauriEventEmitter {
    /// Creates a new TauriEventEmitter.
    pub fn new(app_handle: AppHandle, request_id: String) -> Self {
        Self {
            app_handle,
            request_id,
        }
    }

    /// Emits a StreamEvent to the frontend.
    fn emit_stream_event(&self, event: StreamEvent) {
        let event_name = format!("plan-event-{}", self.request_id);
        let _ = self.app_handle.emit(&event_name, &event);
    }
}

#[async_trait]
impl EventEmitter for TauriEventEmitter {
    async fn emit_thought(
        &self,
        content: &str,
        status: ThoughtStatus,
        subtask_id: Option<usize>,
        iteration: Option<u32>,
    ) {
        let mut event = ThoughtEvent::new(content).with_status(status);
        if let Some(id) = subtask_id {
            event = event.with_subtask_id(id);
        }
        if let Some(iter) = iteration {
            event = event.with_iteration(iter);
        }
        self.emit_stream_event(StreamEvent::Thought(event));
    }

    async fn emit_tool_call(
        &self,
        call_id: &str,
        tool_name: &str,
        args: &serde_json::Value,
        server_name: Option<&str>,
        subtask_id: Option<usize>,
    ) {
        let mut event = ToolCallEvent::new(call_id, tool_name, args.clone());
        if let Some(server) = server_name {
            event = event.with_server_name(server);
        }
        if let Some(id) = subtask_id {
            event = event.with_subtask_id(id);
        }
        self.emit_stream_event(StreamEvent::ToolCall(event));
    }

    async fn emit_tool_result(
        &self,
        call_id: &str,
        result: &str,
        is_error: bool,
        duration: Option<Duration>,
        subtask_id: Option<usize>,
    ) {
        let mut event = if is_error {
            ToolResultEvent::error(call_id, result)
        } else {
            ToolResultEvent::success(call_id, result)
        };

        if let Some(dur) = duration {
            event = event.with_duration_ms(dur.as_millis() as u64);
        }
        if let Some(id) = subtask_id {
            event = event.with_subtask_id(id);
        }
        self.emit_stream_event(StreamEvent::ToolResult(event));
    }

    async fn emit_status(
        &self,
        phase: ExecutionPhase,
        message: &str,
        subtask_id: Option<usize>,
        subtask_current: Option<usize>,
        subtask_total: Option<usize>,
    ) {
        let mut event = StatusEvent::new(phase, message);
        if let Some(id) = subtask_id {
            if let (Some(current), Some(total)) = (subtask_current, subtask_total) {
                event = event.with_subtask_progress(id, current, total);
            } else {
                event.subtask_id = Some(id);
            }
        }
        self.emit_stream_event(StreamEvent::Status(event));
    }

    async fn emit_text(&self, content: &str) {
        let event = TextEvent::new(content);
        self.emit_stream_event(StreamEvent::Text(event));
    }

    async fn emit_finish(
        &self,
        usage: &TokenUsage,
        stop_reason: &str,
        error: Option<&str>,
        summary: Option<ExecutionSummary>,
    ) {
        let mut event = if let Some(err) = error {
            FinishEvent::error(usage.clone(), err)
        } else {
            FinishEvent::with_reason(stop_reason, usage.clone())
        };

        if let Some(sum) = summary {
            event = event.with_summary(sum);
        }
        self.emit_stream_event(StreamEvent::Finish(event));
    }

    async fn emit_artifact_created(
        &self,
        artifact_id: &str,
        title: &str,
        artifact_type: &serde_json::Value,
        content: &str,
        size: u64,
        url: &str,
        subtask_id: Option<usize>,
    ) {
        use aries::chat::events::ArtifactCreatedEvent;
        let mut event =
            ArtifactCreatedEvent::new(artifact_id, title, artifact_type.clone(), content, size, url);
        if let Some(id) = subtask_id {
            event = event.with_subtask_id(id);
        }
        self.emit_stream_event(StreamEvent::ArtifactCreated(event));
    }

    async fn emit_artifact_updated(
        &self,
        artifact_id: &str,
        version: i32,
        content: &str,
        change_description: Option<&str>,
        subtask_id: Option<usize>,
    ) {
        use aries::chat::events::ArtifactUpdatedEvent;
        let mut event = ArtifactUpdatedEvent::new(artifact_id, version, content);
        if let Some(desc) = change_description {
            event = event.with_change_description(desc);
        }
        if let Some(id) = subtask_id {
            event = event.with_subtask_id(id);
        }
        self.emit_stream_event(StreamEvent::ArtifactUpdated(event));
    }

    async fn emit_artifact_deleted(&self, artifact_id: &str, subtask_id: Option<usize>) {
        use aries::chat::events::ArtifactDeletedEvent;
        let mut event = ArtifactDeletedEvent::new(artifact_id);
        if let Some(id) = subtask_id {
            event = event.with_subtask_id(id);
        }
        self.emit_stream_event(StreamEvent::ArtifactDeleted(event));
    }

    fn is_active(&self) -> bool {
        true
    }
}

// ============================================================================
// Configuration Types
// ============================================================================

/// Chat configuration for frontend
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatConfigDto {
    pub url: String,
    pub api_key: String,
    pub model: String,
}

/// State to hold the config file path
pub struct ConfigState {
    pub config_path: RwLock<Option<PathBuf>>,
}

/// Get the config file path with cross-platform support.
/// Priority: User config dir > Bundled resource (copied on first run)
fn get_config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("Failed to get app config dir: {}", e))?;

    let user_config_path = config_dir.join("config.toml");

    // If user config exists, use it
    if user_config_path.exists() {
        println!("Using user config: {:?}", user_config_path);
        return Ok(user_config_path);
    }

    // Otherwise, copy bundled config to user config dir
    let resource_path = app
        .path()
        .resource_dir()
        .map_err(|e| format!("Failed to get resource dir: {}", e))?
        .join("config.toml");

    if resource_path.exists() {
        // Create config directory if it doesn't exist
        std::fs::create_dir_all(&config_dir)
            .map_err(|e| format!("Failed to create config dir: {}", e))?;

        // Copy bundled config to user config dir
        std::fs::copy(&resource_path, &user_config_path)
            .map_err(|e| format!("Failed to copy config: {}", e))?;

        println!(
            "Copied default config from {:?} to {:?}",
            resource_path, user_config_path
        );
        return Ok(user_config_path);
    }

    // Fallback for development mode: use project root config
    let dev_config_path = PathBuf::from("config.toml");
    if dev_config_path.exists() {
        println!("Using development config: {:?}", dev_config_path);
        return Ok(dev_config_path);
    }

    Err("No config.toml found".to_string())
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
async fn get_server_info(state: tauri::State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let _info = state.server_info.read().await;
    Ok(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "status": "Running",
        "memory_enabled": state.has_memory(),
    }))
}

/// Get chat configuration from config.toml
#[tauri::command]
async fn get_chat_config(
    config_state: tauri::State<'_, ConfigState>,
) -> Result<ChatConfigDto, String> {
    let config_path = config_state.config_path.read().await;
    let path = config_path
        .as_ref()
        .ok_or_else(|| "Config path not initialized".to_string())?;

    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read config file: {}", e))?;

    let config: toml::Value =
        toml::from_str(&content).map_err(|e| format!("Failed to parse config: {}", e))?;

    let chat_config = if let Some(chat) = config.get("chat") {
        ChatConfigDto {
            url: chat
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            api_key: chat
                .get("api_key")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            model: chat
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        }
    } else {
        ChatConfigDto::default()
    };

    Ok(chat_config)
}

/// Update chat configuration in config.toml and re-register the chat server
#[tauri::command]
async fn update_chat_config(
    config_state: tauri::State<'_, ConfigState>,
    state: tauri::State<'_, Arc<AppState>>,
    chat_config: ChatConfigDto,
) -> Result<(), String> {
    println!("📝 update_chat_config called: url={}, model={}", chat_config.url, chat_config.model);

    let config_path = config_state.config_path.read().await;
    let path = config_path
        .as_ref()
        .ok_or_else(|| "Config path not initialized".to_string())?;

    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read config file: {}", e))?;

    let mut config: toml::Value =
        toml::from_str(&content).map_err(|e| format!("Failed to parse config: {}", e))?;

    // Create or update [chat] section
    let chat_table = toml::value::Table::from_iter([
        ("url".to_string(), toml::Value::String(chat_config.url.clone())),
        (
            "api_key".to_string(),
            toml::Value::String(chat_config.api_key.clone()),
        ),
        ("model".to_string(), toml::Value::String(chat_config.model.clone())),
    ]);

    if let Some(table) = config.as_table_mut() {
        table.insert("chat".to_string(), toml::Value::Table(chat_table));
    }

    let new_content =
        toml::to_string_pretty(&config).map_err(|e| format!("Failed to serialize config: {}", e))?;

    std::fs::write(path, new_content)
        .map_err(|e| format!("Failed to write config file: {}", e))?;

    println!("📝 Config file written successfully");

    // Update the in-memory AppState config and re-register the chat server
    {
        use aries::config::ChatConfig;
        use aries::server::{Server, ServerKind};

        // Create ChatConfig from DTO using the new constructor
        let new_chat_config = ChatConfig::new(
            chat_config.url.clone(),
            chat_config.api_key.clone(),
        );

        // Update the config in AppState
        {
            let mut app_config = state.config.write().await;
            app_config.chat = Some(new_chat_config.clone());
        }
        println!("📝 AppState config updated");

        // Create and register the new chat server
        let server = Server::from_chat_config(&new_chat_config)
            .map_err(|e| format!("Failed to create server from config: {}", e))?;
        println!("📝 Server created: {}", server.id);

        // First, unregister any existing chat servers
        let server_ids_to_remove: Vec<String> = {
            let servers = state.server_group.read().await;
            if let Some(chat_group) = servers.get(&ServerKind::chat) {
                chat_group.server_ids().await
            } else {
                Vec::new()
            }
        };
        println!("📝 Found {} existing chat servers to unregister", server_ids_to_remove.len());

        for server_id in server_ids_to_remove {
            println!("📝 Unregistering server: {}", server_id);
            let _ = state.unregister_downstream_server(&server_id).await;
        }

        // Register the new chat server
        println!("📝 Registering new chat server...");
        state
            .register_downstream_server(server)
            .await
            .map_err(|e| format!("Failed to register chat server: {}", e))?;

        println!("✅ Chat server registered: {}", chat_config.url);
    }

    println!("✅ Chat config updated successfully");
    Ok(())
}

/// Chat message request from frontend
#[derive(Debug, Clone, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub model: String,
}

/// Chat stream request from frontend (Plan mode with execution transparency)
#[derive(Debug, Clone, Deserialize)]
pub struct ChatStreamRequest {
    pub message: String,
    pub model: Option<String>,
    pub conversation_id: Option<String>,
}

/// Response from starting a chat stream
#[derive(Debug, Clone, Serialize)]
pub struct ChatStreamStartResponse {
    /// Unique request ID for listening to events
    pub request_id: String,
}

/// Chat message response to frontend
#[derive(Debug, Clone, Serialize)]
pub struct ChatResponse {
    pub content: String,
}

/// Send a chat message and get a response from the LLM
#[tauri::command]
async fn chat(
    state: tauri::State<'_, Arc<AppState>>,
    request: ChatRequest,
) -> Result<ChatResponse, String> {
    // Get the chat configuration from AppState
    let config = state.config.read().await;
    let chat_config = config
        .chat
        .as_ref()
        .ok_or_else(|| "No chat service configured. Please configure a chat service in Settings.".to_string())?;

    // Build the chat completion request URL
    let chat_url = format!(
        "{}/chat/completions",
        chat_config.url.trim_end_matches('/')
    );

    // Get API key (may be empty for local servers)
    let api_key = chat_config.get_api_key();

    // Use minimal request body for maximum compatibility with local LLM servers
    // Note: Some local LLM servers may not handle system messages well
    let request_body = serde_json::json!({
        "model": request.model,
        "messages": [
            {
                "role": "user",
                "content": request.message
            }
        ],
        "stream": false
    });

    println!("📤 Chat request to: {}", chat_url);

    // Release the config lock before making the HTTP request
    drop(config);

    // Use reqwest with no_proxy() to work correctly with local LLM servers
    // The no_proxy() setting prevents reqwest from using system proxy settings
    // which can cause 502 Bad Gateway errors with local servers like LlamaEdge
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let mut request_builder = client
        .post(&chat_url)
        .header("Content-Type", "application/json")
        .json(&request_body);

    // Add Authorization header if API key is present
    if let Some(key) = &api_key {
        if !key.is_empty() {
            let auth = if key.starts_with("Bearer ") {
                key.clone()
            } else {
                format!("Bearer {}", key)
            };
            request_builder = request_builder.header("Authorization", auth);
        }
    }

    let response = request_builder
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Server returned error {}: {}", status, body));
    }

    let response_json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    // Extract the assistant's message content
    let content = response_json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("Sorry, I couldn't generate a response.")
        .to_string();

    println!("✅ Chat response received");

    Ok(ChatResponse { content })
}

/// Start a chat stream with Plan mode execution transparency.
///
/// This command initiates a Plan mode execution and emits events to the frontend
/// as the execution progresses. The frontend should listen to events on the
/// channel `plan-event-{request_id}`.
///
/// # Returns
/// Returns the request_id which can be used to subscribe to execution events.
#[tauri::command]
async fn chat_stream(
    app_handle: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    request: ChatStreamRequest,
) -> Result<ChatStreamStartResponse, String> {
    use aries::chat::api::{execute_plan, ExecutePlanRequest};
    use tokio_util::sync::CancellationToken;

    // Generate a unique request ID
    let request_id = format!("stream-{}", uuid::Uuid::new_v4());

    // Create the TauriEventEmitter
    let emitter = Arc::new(TauriEventEmitter::new(
        app_handle.clone(),
        request_id.clone(),
    ));

    // Create cancellation token for this request
    let cancel_token = CancellationToken::new();

    // Get the state for the async task
    let state_clone = state.inner().clone();
    let request_id_clone = request_id.clone();

    // Build the execute plan request
    let plan_request = ExecutePlanRequest {
        message: request.message,
        model: request.model,
        conversation_id: request.conversation_id,
        system_message: None,
    };

    // Spawn the execution task in the background
    tokio::spawn(async move {
        match execute_plan(state_clone, plan_request, emitter.clone(), cancel_token).await {
            Ok(response) => {
                println!(
                    "Plan execution completed successfully - request_id: {}",
                    request_id_clone
                );
                // The emitter has already sent the finish event
                let _ = response;
            }
            Err(e) => {
                eprintln!(
                    "Plan execution failed: {} - request_id: {}",
                    e, request_id_clone
                );
                // Emit an error finish event
                use aries::chat::events::ExecutionPhase;
                emitter
                    .emit_status(
                        ExecutionPhase::Completing,
                        &format!("Execution failed: {}", e),
                        None,
                        None,
                        None,
                    )
                    .await;
                emitter
                    .emit_finish(
                        &TokenUsage::default(),
                        "error",
                        Some(&e.to_string()),
                        None,
                    )
                    .await;
            }
        }
    });

    Ok(ChatStreamStartResponse { request_id })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Define the toggle shortcut: Option + Space
    let toggle_shortcut = Shortcut::new(Some(Modifiers::ALT), Code::Space);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_shortcut(toggle_shortcut)
                .unwrap()
                .with_handler(move |app, shortcut, event| {
                    if shortcut == &toggle_shortcut && event.state() == ShortcutState::Pressed {
                        if let Some(window) = app.get_webview_window("main") {
                            let is_visible = window.is_visible().unwrap_or(false);
                            if is_visible {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(),
        )
        .setup(|app| {
            let handle = app.handle().clone();

            // Create Tray Menu
            let quit_i = MenuItem::with_id(app, "quit", "Quit Aries", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "Open Aries", true, None::<&str>)?;
            let hide_i = MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &hide_i, &quit_i])?;

            // Build Tray Icon
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "hide" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.hide();
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let is_visible = window.is_visible().unwrap_or(false);
                            if is_visible {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            // Initialize AriesEngine
            let config_path = match get_config_path(&handle) {
                Ok(path) => path,
                Err(e) => {
                    eprintln!("Failed to get config path: {}", e);
                    return Ok(());
                }
            };

            // Store config path for later use
            let config_state = ConfigState {
                config_path: RwLock::new(Some(config_path.clone())),
            };
            handle.manage(config_state);

            tauri::async_runtime::block_on(async move {
                match AriesEngine::init(config_path).await {
                    Ok(engine) => {
                        println!("Aries Engine initialized successfully");
                        engine.start_health_checks().await;
                        handle.manage(engine.state);
                    }
                    Err(e) => {
                        eprintln!("Failed to initialize Aries Engine: {}", e);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            get_server_info,
            get_chat_config,
            update_chat_config,
            chat,
            chat_stream
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
