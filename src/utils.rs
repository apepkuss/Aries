use once_cell::sync::OnceCell;
use std::time::Duration;

// Global log configuration
pub static LOG_DESTINATION: OnceCell<String> = OnceCell::new();

// =============================================================================
// HTTP Client Utilities
// =============================================================================

/// Creates a new reqwest HTTP client with proper configuration for LlamaEdge compatibility.
///
/// This function creates a client configured to work correctly with local LLM servers
/// like LlamaEdge. The key configuration is `no_proxy()` which prevents the client
/// from using system proxy settings that can cause 502 Bad Gateway errors with
/// local servers.
///
/// # Returns
///
/// A configured `reqwest::Client` instance.
///
/// # Example
///
/// ```rust,no_run
/// use aries::utils::create_http_client;
///
/// let client = create_http_client();
/// // Use client for HTTP requests...
/// ```
pub fn create_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// Creates a new reqwest HTTP client with custom timeout settings.
///
/// Similar to `create_http_client()` but allows specifying custom timeout values.
///
/// # Arguments
///
/// * `timeout_secs` - Request timeout in seconds
/// * `connect_timeout_secs` - Connection timeout in seconds
///
/// # Returns
///
/// A configured `reqwest::Client` instance with the specified timeouts.
pub fn create_http_client_with_timeout(timeout_secs: u64, connect_timeout_secs: u64) -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(timeout_secs))
        .connect_timeout(Duration::from_secs(connect_timeout_secs))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

// Helper macro for dual logging (to both stdout and log file)
#[macro_export]
macro_rules! dual_log {
    ($level:expr, $($arg:tt)+) => {{
        let msg = format!($($arg)+);
        if $crate::utils::LOG_DESTINATION.get().map_or(false, |d| d == "both") {
            println!("{}: {}", $level, msg);
        }
        match $level {
            "INFO" => tracing::info!("{}", msg),
            "WARN" => tracing::warn!("{}", msg),
            "ERROR" => tracing::error!("{}", msg),
            "DEBUG" => tracing::debug!("{}", msg),
            _ => tracing::trace!("{}", msg),
        }
    }};
}

// Convenience macros for each log level
#[macro_export]
macro_rules! dual_info {
    ($($arg:tt)+) => { $crate::dual_log!("INFO", $($arg)+) };
}

#[macro_export]
macro_rules! dual_warn {
    ($($arg:tt)+) => { $crate::dual_log!("WARN", $($arg)+) };
}

#[macro_export]
macro_rules! dual_error {
    ($($arg:tt)+) => { $crate::dual_log!("ERROR", $($arg)+) };
}

#[macro_export]
macro_rules! dual_debug {
    ($($arg:tt)+) => { $crate::dual_log!("DEBUG", $($arg)+) };
}
