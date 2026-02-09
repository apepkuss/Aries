//! stdio transport layer.
//!
//! Handles stdin/stdout communication with MCP Server child processes.
//! Self-implemented instead of using rmcp's `TokioChildProcess` for:
//! - Lazy Loading lifecycle separation
//! - Health check integration
//! - Flexible error recovery

// TODO: Phase 3 implementation

/// stdio transport layer.
pub struct StdioTransport;
