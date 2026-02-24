// Moss - AI Agent Framework
//
// This library provides core functionality for building AI agent applications.

use once_cell::sync::OnceCell;

// =============================================================================
// Module Organization
// =============================================================================
//
// The crate is organized into logical groups:
//
// - core/        : Foundational types (config, error)
// - services/    : Business logic (chat, memory, skills, executor, etc.)
// - integration/ : External integrations (mcp, server)
// - app/         : Application state and coordination
//
// For backward compatibility, modules are also available at the crate root.
// =============================================================================

// =============================================================================
// Public API for External Integration (Desktop/Frontend Applications)
// =============================================================================
//
// Recommended types for external consumers:
//
// Configuration:
//   - `Config`              : Main configuration structure
//   - `ServerError`         : Error type for operations
//   - `ServerResult<T>`     : Result alias
//
// Application State:
//   - `AppState`            : Core application state
//
// Chat & Streaming:
//   - `chat::events::*`     : Stream events for real-time updates
//   - `chat::EventEmitter`  : Trait for custom event handling
//
// Memory System:
//   - `memory::CompleteChatMemory` : Conversation memory management
//   - `memory::types::*`           : Message and conversation types
//
// Skills System:
//   - `skills::SkillRegistry`      : Global skill registry
//   - `skills::SkillLoader`        : Skill loading utilities
//   - `skills::LoadedSkill`        : Loaded skill representation
//
// Script Execution:
//   - `executor::ScriptExecutorManager` : Script execution management
//   - `executor::Executor`              : Executor trait
//
// =============================================================================

// Organized module groups (new structure)
pub mod core;
pub mod integration;
pub mod services;

// Application layer
pub mod app;

// Core modules (also available via crate::core::*)
pub mod config;
pub mod error;

// Service modules (also available via crate::services::*)
pub mod artifacts;
pub mod chat;
pub mod executor;
pub mod memory;
pub mod reflection;
pub mod responses;
pub mod skills;
pub mod subagent;

// Re-export hitl from services
pub use services::hitl;

// Integration modules (also available via crate::integration::*)
pub mod mcp;
pub mod mcp_handlers;
pub mod mcp_stdio;
pub mod server;

// Other modules
pub mod capabilities;
pub mod cli;
pub mod config_api;
pub mod handlers;
pub mod info;
pub mod session;
pub mod utils;

// Re-export commonly used types for convenience
pub use app::AppState;
pub use config::Config;
pub use error::{ServerError, ServerResult};

// Global health check interval for downstream servers in seconds
pub static HEALTH_CHECK_INTERVAL: OnceCell<u64> = OnceCell::new();
