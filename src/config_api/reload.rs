//! Service reload logic
//!
//! This module provides functions for reloading services after configuration updates.
//! When chat or embedding service configuration is updated, the corresponding
//! downstream server needs to be unregistered and re-registered with the new settings.

use std::sync::Arc;

use crate::{
    AppState, dual_error, dual_info, dual_warn,
    server::{Server, ServerKind},
};

/// Result of a service reload operation
#[derive(Debug)]
pub struct ReloadResult {
    /// Whether the reload was successful
    pub success: bool,
    /// The service that was reloaded
    #[allow(dead_code)]
    pub service: String,
    /// Error message if reload failed
    pub error: Option<String>,
}

impl ReloadResult {
    pub fn success(service: &str) -> Self {
        Self {
            success: true,
            service: service.to_string(),
            error: None,
        }
    }

    pub fn failure(service: &str, error: String) -> Self {
        Self {
            success: false,
            service: service.to_string(),
            error: Some(error),
        }
    }
}

/// Reload the chat service with updated configuration
///
/// This function:
/// 1. Finds and unregisters any existing chat servers that were registered from config
/// 2. Creates a new server from the updated chat configuration
/// 3. Registers the new server
///
/// # Arguments
///
/// * `state` - The application state containing server groups and configuration
///
/// # Returns
///
/// A `ReloadResult` indicating success or failure
pub async fn reload_chat_service(state: &Arc<AppState>) -> ReloadResult {
    dual_info!("Reloading chat service...");

    // Step 1: Find and unregister existing config-based chat servers
    let servers_to_remove =
        find_config_servers(state, ServerKind::chat, "chat-server-config-").await;

    for server_id in servers_to_remove {
        dual_info!("Unregistering old chat server: {}", server_id);
        if let Err(e) = state.unregister_downstream_server(&server_id).await {
            dual_warn!("Failed to unregister chat server {}: {}", server_id, e);
            // Continue anyway - the old server might already be gone
        }
    }

    // Step 2: Read the updated configuration and create new server
    let config = state.config.read().await;

    let Some(chat_config) = &config.chat else {
        dual_warn!("Chat configuration not found, skipping reload");
        return ReloadResult::failure("chat", "Chat configuration not found".to_string());
    };

    let server = match Server::from_chat_config(chat_config) {
        Ok(s) => s,
        Err(e) => {
            let error_msg = format!("Failed to create chat server from config: {}", e);
            dual_error!("{}", error_msg);
            return ReloadResult::failure("chat", error_msg);
        }
    };

    // Drop the config read lock before registering
    drop(config);

    // Step 3: Update model list for the new server
    let headers = axum::http::HeaderMap::new();
    if let Err(e) = crate::handlers::update_model_list(
        axum::extract::State(Arc::clone(state)),
        &headers,
        "config-reload",
        &server,
    )
    .await
    {
        dual_warn!(
            "Failed to update model list for chat server {}: {}",
            server.id,
            e
        );
        // Continue with registration even if model list update fails
    }

    // Step 4: Register the new server
    if let Err(e) = state.register_downstream_server(server).await {
        let error_msg = format!("Failed to register chat server: {}", e);
        dual_error!("{}", error_msg);
        return ReloadResult::failure("chat", error_msg);
    }

    dual_info!("Chat service reloaded successfully");
    ReloadResult::success("chat")
}

/// Reload the embedding service with updated configuration
///
/// This function:
/// 1. Finds and unregisters any existing embedding servers that were registered from config
/// 2. Creates a new server from the updated embedding configuration
/// 3. Registers the new server
///
/// # Arguments
///
/// * `state` - The application state containing server groups and configuration
///
/// # Returns
///
/// A `ReloadResult` indicating success or failure
pub async fn reload_embedding_service(state: &Arc<AppState>) -> ReloadResult {
    dual_info!("Reloading embedding service...");

    // Step 1: Find and unregister existing config-based embedding servers
    let servers_to_remove =
        find_config_servers(state, ServerKind::embeddings, "embeddings-server-config-").await;

    for server_id in servers_to_remove {
        dual_info!("Unregistering old embedding server: {}", server_id);
        if let Err(e) = state.unregister_downstream_server(&server_id).await {
            dual_warn!("Failed to unregister embedding server {}: {}", server_id, e);
            // Continue anyway - the old server might already be gone
        }
    }

    // Step 2: Read the updated configuration and create new server
    let config = state.config.read().await;

    let Some(embedding_config) = &config.embedding else {
        dual_warn!("Embedding configuration not found, skipping reload");
        return ReloadResult::failure("embedding", "Embedding configuration not found".to_string());
    };

    let server = match Server::from_embedding_config(embedding_config) {
        Ok(s) => s,
        Err(e) => {
            let error_msg = format!("Failed to create embedding server from config: {}", e);
            dual_error!("{}", error_msg);
            return ReloadResult::failure("embedding", error_msg);
        }
    };

    // Drop the config read lock before registering
    drop(config);

    // Step 3: Update model list for the new server
    let headers = axum::http::HeaderMap::new();
    if let Err(e) = crate::handlers::update_model_list(
        axum::extract::State(Arc::clone(state)),
        &headers,
        "config-reload",
        &server,
    )
    .await
    {
        dual_warn!(
            "Failed to update model list for embedding server {}: {}",
            server.id,
            e
        );
        // Continue with registration even if model list update fails
    }

    // Step 4: Register the new server
    if let Err(e) = state.register_downstream_server(server).await {
        let error_msg = format!("Failed to register embedding server: {}", e);
        dual_error!("{}", error_msg);
        return ReloadResult::failure("embedding", error_msg);
    }

    dual_info!("Embedding service reloaded successfully");
    ReloadResult::success("embedding")
}

/// Find servers that were registered from configuration
///
/// This helper function searches for servers with IDs that start with the given prefix.
///
/// # Arguments
///
/// * `state` - The application state
/// * `kind` - The server kind to search for
/// * `prefix` - The ID prefix to match (e.g., "chat-server-config-")
///
/// # Returns
///
/// A vector of server IDs that match the criteria
async fn find_config_servers(state: &Arc<AppState>, kind: ServerKind, prefix: &str) -> Vec<String> {
    let mut server_ids = Vec::new();

    // Get the server group for the specified kind
    let servers = match state.list_downstream_servers().await {
        Ok(s) => s,
        Err(e) => {
            dual_warn!("Failed to list downstream servers: {}", e);
            return server_ids;
        }
    };

    // Find servers with matching prefix
    if let Some(kind_servers) = servers.get(&kind) {
        for server in kind_servers {
            if server.id.starts_with(prefix) {
                server_ids.push(server.id.clone());
            }
        }
    }

    server_ids
}

/// Reload the Lantai embedding provider with updated model/dimensions/batch_size
///
/// Reads the current embedding service URL/API key from `[embedding]` config
/// and model/dimensions from `[lantai.embedding]` config, then hot-replaces
/// the embedding provider inside the running Lantai instance.
pub async fn reload_lantai_embedding(state: &Arc<AppState>) -> ReloadResult {
    dual_info!("Reloading Lantai embedding provider...");

    let Some(lantai_arc) = state.lantai() else {
        dual_warn!("Lantai not initialized, skipping embedding provider reload");
        return ReloadResult::failure("lantai_embedding", "Lantai not initialized".to_string());
    };

    // Read config to get embedding service URL/API key and lantai embedding settings
    let config = state.config.read().await;

    let Some(ref embedding_cfg) = config.embedding else {
        dual_warn!("No [embedding] config, setting Lantai to BM25-only mode");
        drop(config);
        let mut lantai = lantai_arc.lock().await;
        if let Err(e) = lantai.replace_embedding(None) {
            let msg = format!("Failed to clear Lantai embedding: {e}");
            dual_error!("{}", msg);
            return ReloadResult::failure("lantai_embedding", msg);
        }
        dual_info!("Lantai switched to BM25-only mode");
        return ReloadResult::success("lantai_embedding");
    };

    let Some(ref lantai_cfg) = config.lantai else {
        dual_warn!("No [lantai] config, skipping embedding provider reload");
        return ReloadResult::failure(
            "lantai_embedding",
            "Lantai configuration not found".to_string(),
        );
    };

    let url = embedding_cfg.url.clone();
    let api_key = embedding_cfg.get_api_key().unwrap_or_default();
    let model = lantai_cfg.embedding.model.clone();
    let dimensions = lantai_cfg.embedding.dimensions;

    drop(config);

    // Create new embedding provider
    let new_embedding: Box<dyn lantai::EmbeddingProvider> = Box::new(
        lantai::embedding::OpenAIEmbedding::new(&url, &api_key, &model, dimensions),
    );

    dual_info!(
        "Replacing Lantai embedding provider: model={}, dimensions={}",
        model,
        dimensions,
    );

    // Hot-replace the embedding provider
    let mut lantai = lantai_arc.lock().await;
    if let Err(e) = lantai.replace_embedding(Some(new_embedding)) {
        let msg = format!("Failed to replace Lantai embedding provider: {e}");
        dual_error!("{}", msg);
        return ReloadResult::failure("lantai_embedding", msg);
    }

    dual_info!("Lantai embedding provider reloaded successfully");
    ReloadResult::success("lantai_embedding")
}

/// Determine which services need to be reloaded based on updated fields
///
/// # Returns
///
/// A tuple of (reload_chat, reload_embedding, reload_lantai_embedding) booleans
pub fn determine_services_to_reload(side_effect_fields: &[String]) -> (bool, bool, bool) {
    let mut reload_chat = false;
    let mut reload_embedding = false;
    let mut reload_lantai_embedding = false;

    for field in side_effect_fields {
        if field.starts_with("chat.") {
            reload_chat = true;
        }
        if field.starts_with("embedding.") {
            reload_embedding = true;
        }
        if field.starts_with("lantai_auto_memory.embedding_") {
            reload_lantai_embedding = true;
        }
    }

    (reload_chat, reload_embedding, reload_lantai_embedding)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reload_result_success() {
        let result = ReloadResult::success("chat");
        assert!(result.success);
        assert_eq!(result.service, "chat");
        assert!(result.error.is_none());
    }

    #[test]
    fn test_reload_result_failure() {
        let result = ReloadResult::failure("embedding", "Connection failed".to_string());
        assert!(!result.success);
        assert_eq!(result.service, "embedding");
        assert_eq!(result.error, Some("Connection failed".to_string()));
    }

    #[test]
    fn test_determine_services_to_reload_chat() {
        let fields = vec!["chat.url".to_string()];
        let (reload_chat, reload_embedding, reload_lantai) = determine_services_to_reload(&fields);
        assert!(reload_chat);
        assert!(!reload_embedding);
        assert!(!reload_lantai);
    }

    #[test]
    fn test_determine_services_to_reload_embedding() {
        let fields = vec!["embedding.api_key".to_string()];
        let (reload_chat, reload_embedding, reload_lantai) = determine_services_to_reload(&fields);
        assert!(!reload_chat);
        assert!(reload_embedding);
        assert!(!reload_lantai);
    }

    #[test]
    fn test_determine_services_to_reload_both() {
        let fields = vec![
            "chat.url".to_string(),
            "embedding.url".to_string(),
            "chat.api_key".to_string(),
        ];
        let (reload_chat, reload_embedding, reload_lantai) = determine_services_to_reload(&fields);
        assert!(reload_chat);
        assert!(reload_embedding);
        assert!(!reload_lantai);
    }

    #[test]
    fn test_determine_services_to_reload_none() {
        let fields = vec!["server.max_tools_per_iteration".to_string()];
        let (reload_chat, reload_embedding, reload_lantai) = determine_services_to_reload(&fields);
        assert!(!reload_chat);
        assert!(!reload_embedding);
        assert!(!reload_lantai);
    }

    #[test]
    fn test_determine_services_to_reload_lantai_embedding() {
        let fields = vec!["lantai_auto_memory.embedding_model".to_string()];
        let (reload_chat, reload_embedding, reload_lantai) = determine_services_to_reload(&fields);
        assert!(!reload_chat);
        assert!(!reload_embedding);
        assert!(reload_lantai);
    }

    #[test]
    fn test_determine_services_to_reload_lantai_dimensions() {
        let fields = vec![
            "lantai_auto_memory.embedding_dimensions".to_string(),
            "lantai_auto_memory.embedding_batch_size".to_string(),
        ];
        let (reload_chat, reload_embedding, reload_lantai) = determine_services_to_reload(&fields);
        assert!(!reload_chat);
        assert!(!reload_embedding);
        assert!(reload_lantai);
    }
}
