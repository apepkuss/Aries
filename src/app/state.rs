// Application State
//
// This module defines the core application state shared across the application.

use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::Arc,
    time::Instant,
};

use futures_util::stream::{self, StreamExt};
use tokio::sync::RwLock;

use crate::{
    HEALTH_CHECK_INTERVAL,
    config::Config,
    error::{ServerError, ServerResult},
    handlers, info, memory, server,
};

/// Application state
pub struct AppState {
    pub(crate) server_group: Arc<RwLock<HashMap<server::ServerKind, server::ServerGroup>>>,
    pub(crate) config: Arc<RwLock<Config>>,
    /// Path to the configuration file (for persistence)
    pub(crate) config_path: Option<std::path::PathBuf>,
    pub(crate) server_info: Arc<RwLock<info::ServerInfo>>,
    pub(crate) models: Arc<RwLock<HashMap<server::ServerId, Vec<endpoints::models::Model>>>>,
    pub(crate) memory: Option<Arc<memory::CompleteChatMemory>>,
    /// Timestamp of the last API-based config update (for conflict detection with file watcher)
    pub(crate) last_config_update_time: RwLock<Option<Instant>>,
}

impl AppState {
    pub fn new(config: Config, server_info: info::ServerInfo) -> Self {
        Self {
            server_group: Arc::new(RwLock::new(HashMap::new())),
            config: Arc::new(RwLock::new(config)),
            config_path: None,
            server_info: Arc::new(RwLock::new(server_info)),
            models: Arc::new(RwLock::new(HashMap::new())),
            memory: None,
            last_config_update_time: RwLock::new(None),
        }
    }

    /// Record a config update timestamp (used for conflict detection with file watcher)
    pub async fn record_config_update_time(&self) {
        let mut last_update = self.last_config_update_time.write().await;
        *last_update = Some(Instant::now());
    }

    /// Get the last config update timestamp
    pub async fn get_last_config_update_time(&self) -> Option<Instant> {
        *self.last_config_update_time.read().await
    }

    pub fn with_config_path(mut self, path: std::path::PathBuf) -> Self {
        self.config_path = Some(path);
        self
    }

    pub fn with_memory(mut self, memory: Arc<memory::CompleteChatMemory>) -> Self {
        self.memory = Some(memory);
        self
    }

    /// Get the configuration file path if set
    pub fn get_config_path(&self) -> Option<&std::path::Path> {
        self.config_path.as_deref()
    }

    /// Check if memory system is enabled
    pub fn has_memory(&self) -> bool {
        self.memory.is_some()
    }

    pub async fn register_downstream_server(&self, server: server::Server) -> ServerResult<()> {
        use server::ServerKind;

        if server.kind.contains(ServerKind::chat) {
            self.server_group
                .write()
                .await
                .entry(ServerKind::chat)
                .or_insert(server::ServerGroup::new(ServerKind::chat))
                .register(server.clone())
                .await?;
        }
        if server.kind.contains(ServerKind::embeddings) {
            self.server_group
                .write()
                .await
                .entry(ServerKind::embeddings)
                .or_insert(server::ServerGroup::new(ServerKind::embeddings))
                .register(server.clone())
                .await?;
        }
        if server.kind.contains(ServerKind::image) {
            self.server_group
                .write()
                .await
                .entry(ServerKind::image)
                .or_insert(server::ServerGroup::new(ServerKind::image))
                .register(server.clone())
                .await?;
        }
        if server.kind.contains(ServerKind::tts) {
            self.server_group
                .write()
                .await
                .entry(ServerKind::tts)
                .or_insert(server::ServerGroup::new(ServerKind::tts))
                .register(server.clone())
                .await?;
        }
        if server.kind.contains(ServerKind::translate) {
            self.server_group
                .write()
                .await
                .entry(ServerKind::translate)
                .or_insert(server::ServerGroup::new(ServerKind::translate))
                .register(server.clone())
                .await?;
        }
        if server.kind.contains(ServerKind::transcribe) {
            self.server_group
                .write()
                .await
                .entry(ServerKind::transcribe)
                .or_insert(server::ServerGroup::new(ServerKind::transcribe))
                .register(server.clone())
                .await?;
        }
        if server.kind.contains(ServerKind::privacy_chat) {
            self.server_group
                .write()
                .await
                .entry(ServerKind::privacy_chat)
                .or_insert(server::ServerGroup::new(ServerKind::privacy_chat))
                .register(server.clone())
                .await?;
        }

        Ok(())
    }

    pub async fn unregister_downstream_server(
        &self,
        server_id: impl AsRef<str>,
    ) -> ServerResult<()> {
        use server::ServerKind;

        let mut found = false;

        // unregister the server from the servers
        {
            // parse server kind from server id
            let kinds = server_id
                .as_ref()
                .split("-server-")
                .next()
                .unwrap()
                .split("-")
                .collect::<Vec<&str>>();

            let group_map = self.server_group.read().await;

            for kind in kinds {
                let kind = ServerKind::from_str(kind).unwrap();
                if let Some(group) = group_map.get(&kind) {
                    group.unregister(server_id.as_ref()).await?;
                    crate::dual_info!("Unregistered {} server: {}", &kind, server_id.as_ref());

                    if !found {
                        found = true;
                    }
                }
            }
        }

        if found {
            // remove the server info from the server_info
            let mut server_info = self.server_info.write().await;
            server_info.servers.remove(server_id.as_ref());

            // remove the server from the models
            let mut models = self.models.write().await;
            models.remove(server_id.as_ref());
        }

        if !found {
            return Err(ServerError::Operation(format!(
                "Server {} not found",
                server_id.as_ref()
            )));
        }

        Ok(())
    }

    pub async fn list_downstream_servers(
        &self,
    ) -> ServerResult<HashMap<server::ServerKind, Vec<server::Server>>> {
        let servers = self.server_group.read().await;

        let mut server_groups = HashMap::new();
        for (kind, group) in servers.iter() {
            if !group.is_empty().await {
                let servers = group.servers.read().await;

                // Create a new Vec with cloned Server instances using async stream
                let server_vec = stream::iter(servers.iter())
                    .then(|server_lock| async move {
                        let server = server_lock.read().await;
                        server.clone()
                    })
                    .collect::<Vec<_>>()
                    .await;

                server_groups.insert(*kind, server_vec);
            }
        }

        Ok(server_groups)
    }

    pub async fn check_server_health(&self) -> ServerResult<()> {
        use server::ServerKind;

        if !self.server_group.read().await.is_empty() {
            let mut unhealthy_servers = Vec::new();

            // Check health status of downstream servers
            {
                let group_map = self.server_group.read().await;

                // check health of unique servers
                let mut unique_server_ids = HashSet::new();
                for (kind, group) in group_map.iter() {
                    if !group.is_empty().await {
                        let servers = group.servers.read().await;
                        for server_lock in servers.iter() {
                            let mut server = server_lock.write().await;

                            if !unique_server_ids.contains(&server.id)
                                && unique_server_ids.contains(&server.url)
                            {
                                crate::dual_info!("Checking health of {}", &server.id);

                                unique_server_ids.insert(server.id.clone());
                                unique_server_ids.insert(server.url.clone());

                                let is_healthy = server.check_health().await;
                                if !is_healthy {
                                    crate::dual_warn!(
                                        "{} server {} is unhealthy",
                                        kind,
                                        &server.id
                                    );
                                    unhealthy_servers.push(server.id.clone());
                                }
                            }
                        }
                    }
                }
            }

            // Unregister unhealthy servers
            if !unhealthy_servers.is_empty() {
                for server_id in unhealthy_servers {
                    self.unregister_downstream_server(&server_id).await?;
                }
            }

            // Push the healthy servers to the external service if configured
            if let Some(push_url) = &self.config.read().await.server_health_push_url {
                // collect the healthy servers by kind
                let mut healthy_servers: HashMap<ServerKind, Vec<String>> = HashMap::new();
                {
                    let group_map = self.server_group.read().await;
                    for (kind, group) in group_map.iter() {
                        if group.is_empty().await {
                            crate::dual_warn!("No {} servers available after health check", kind);
                        }

                        healthy_servers.insert(
                            *kind,
                            group.healthy_servers.read().await.iter().cloned().collect(),
                        );
                    }
                }

                let health_status = serde_json::json!({
                    "rag": self.config.read().await.rag.as_ref().unwrap().enable,
                    "servers": healthy_servers,
                });

                crate::dual_debug!(
                    "Healthy servers:\n{}",
                    serde_json::to_string_pretty(&health_status).unwrap()
                );

                // Send the healthy servers to the external service
                reqwest::Client::new()
                    .post(push_url)
                    .json(&health_status)
                    .send()
                    .await
                    .map_err(|e| {
                        let err_msg = format!("Failed to send health check result: {e}");

                        crate::dual_error!("{}", err_msg);

                        ServerError::Operation(err_msg)
                    })?;
            }
        } else {
            crate::dual_warn!("No servers registered, skipping health check");
        }

        Ok(())
    }

    pub async fn start_health_check_task(self: Arc<Self>) {
        let check_interval = HEALTH_CHECK_INTERVAL.get().unwrap_or(&60);
        let check_interval = tokio::time::Duration::from_secs(*check_interval);

        tokio::spawn(async move {
            loop {
                crate::dual_debug!("Starting health check");

                if let Err(e) = self.check_server_health().await {
                    crate::dual_error!("Health check error: {}", e);
                }

                tokio::time::sleep(check_interval).await;
            }
        });
    }

    pub async fn register_config_servers(self: &Arc<Self>) -> ServerResult<()> {
        let config = self.config.read().await;

        // Register chat service from configuration file
        if let Some(chat_config) = &config.chat {
            crate::dual_info!("Registering chat service from config: {}", chat_config.url);
            let server = server::Server::from_chat_config(chat_config)?;

            // Update model list for the server
            let headers = axum::http::HeaderMap::new();
            if let Err(e) = handlers::update_model_list(
                axum::extract::State(Arc::clone(self)),
                &headers,
                "config-registration",
                &server,
            )
            .await
            {
                crate::dual_warn!(
                    "Failed to update model list for chat server {}: {}",
                    server.id,
                    e
                );
                // Continue with registration even if model list update fails
            }

            self.register_downstream_server(server).await?;
            crate::dual_info!("Chat service registered successfully");
        }

        // Register embedding service from configuration file
        if let Some(embedding_config) = &config.embedding {
            crate::dual_info!(
                "Registering embedding service from config: {}",
                embedding_config.url
            );
            let server = server::Server::from_embedding_config(embedding_config)?;

            // Update model list for the server
            let headers = axum::http::HeaderMap::new();
            if let Err(e) = handlers::update_model_list(
                axum::extract::State(Arc::clone(self)),
                &headers,
                "config-registration",
                &server,
            )
            .await
            {
                crate::dual_warn!(
                    "Failed to update model list for embedding server {}: {}",
                    server.id,
                    e
                );
                // Continue with registration even if model list update fails
            }

            self.register_downstream_server(server).await?;
            crate::dual_info!("Embedding service registered successfully");
        }

        Ok(())
    }
}
