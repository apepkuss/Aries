//! Artifacts Management Module
//!
//! Provides storage, versioning, and management for Agent-generated artifacts
//! such as code files, documents, charts, and other outputs.
//!
//! # Architecture
//!
//! - `types.rs` - Core data structures (Artifact, ArtifactType, etc.)
//! - `store.rs` - Database operations and storage abstraction
//! - `handlers.rs` - HTTP API handlers
//! - `storage/` - Storage backend implementations (filesystem, S3)
//!
//! # Example
//!
//! ```rust,ignore
//! use artifacts::{ArtifactStore, CreateArtifactRequest, ArtifactType};
//!
//! let store = ArtifactStore::new(pool, config).await?;
//! let artifact = store.create(CreateArtifactRequest {
//!     conversation_id: "conv_123".to_string(),
//!     title: "main.rs".to_string(),
//!     artifact_type: ArtifactType::Code { language: "rust".into() },
//!     content: "fn main() {}".to_string(),
//!     ..Default::default()
//! }, None).await?;
//! ```

mod handlers;
mod service;
mod storage;
mod store;
mod types;

pub use handlers::{
    ArtifactsState, create_artifact_handler, delete_artifact_handler, download_artifact_handler,
    get_artifact_handler, get_version_content_handler, list_artifacts_by_conversation_handler,
    list_versions_handler, restore_version_handler, update_artifact_handler,
};
#[allow(unused_imports)]
pub use service::ArtifactService;
#[allow(unused_imports)]
pub use store::{ArtifactConfig, ArtifactError, ArtifactResult, ArtifactStore};
#[allow(unused_imports)]
pub use types::*;
