//! Shared constants and utilities for internal tool naming.
//!
//! Internal tools use a `{prefix}__{tool_name}` naming convention
//! (e.g., `internal__skill_run_script`). These constants are shared
//! across `plan.rs`, `injector.rs`, and `subagent/executor.rs`.

/// Internal tool prefix for non-MCP tools
pub const INTERNAL_TOOL_PREFIX: &str = "internal";

/// Skill script execution tool name (without prefix)
pub const SKILL_RUN_SCRIPT_TOOL: &str = "skill_run_script";

/// Skill asset loading tool name (without prefix)
pub const SKILL_LOAD_ASSET_TOOL: &str = "skill_load_asset";

/// Lantai knowledge base search tool name (without prefix)
pub const LANTAI_SEARCH_TOOL: &str = "lantai_search";

/// Lantai knowledge base stats tool name (without prefix)
pub const LANTAI_STATS_TOOL: &str = "lantai_stats";

/// Generate full internal tool name from short name.
///
/// # Example
/// ```ignore
/// assert_eq!(internal_tool_name("skill_run_script"), "internal__skill_run_script");
/// ```
pub fn internal_tool_name(tool_name: &str) -> String {
    format!("{INTERNAL_TOOL_PREFIX}__{tool_name}")
}

/// Check if a tool name is an internal tool.
pub fn is_internal_tool(tool_name: &str) -> bool {
    tool_name.starts_with(&format!("{INTERNAL_TOOL_PREFIX}__"))
}

/// Parse an internal tool name, returning the tool name without prefix if valid.
pub fn parse_internal_tool_name(full_name: &str) -> Option<&str> {
    full_name.strip_prefix(&format!("{INTERNAL_TOOL_PREFIX}__"))
}
