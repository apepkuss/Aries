// Core module - foundational types and utilities
//
// This module re-exports core functionality that is fundamental to the application.
// These are low-level modules with minimal dependencies.

// Re-export from parent module for gradual migration
// Re-export commonly used types
pub use crate::{
    config,
    config::Config,
    error,
    error::{ServerError, ServerResult},
};
