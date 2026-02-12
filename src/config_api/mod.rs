//! Configuration Management API
//!
//! Provides runtime configuration introspection and modification endpoints.
//!
//! # Endpoints
//!
//! - `GET /v1/config` - Get current configuration (sanitized)
//! - `POST /v1/config` - Update configuration fields
//! - `GET /v1/config/schema` - Get updatable fields schema
//!
//! # Features
//!
//! - **Sensitive field sanitization**: API keys and secrets are never exposed
//! - **Hot update support**: Some fields can be updated without restart
//! - **Validation**: Field values are validated before applying
//! - **Side effects**: Service reloading is handled automatically when needed
//! - **Persistence**: Configuration changes can be persisted to disk
//!
//! # Example
//!
//! ```json
//! // GET /v1/config response
//! {
//!   "server": {
//!     "host": "127.0.0.1",
//!     "port": 3389,
//!     "max_tools_per_iteration": 5
//!   },
//!   "chat": {
//!     "url": "http://localhost:8080/v1",
//!     "api_key_configured": true
//!   },
//!   "updatable_fields": [
//!     "server.max_tools_per_iteration",
//!     "chat.url"
//!   ]
//! }
//! ```

pub mod diff;
mod handlers;
pub mod persist;
mod reload;
pub mod sanitize;
pub mod types;
mod update;
mod validate;
pub mod watcher;

pub use handlers::{
    get_config_handler, get_config_schema_handler, test_chat_service_handler, update_config_handler,
};
pub use watcher::ConfigWatcher;
