//! Shared utilities for chat modes
//!
//! This module contains shared components used across different chat modes,
//! such as time budget management for Plan mode and HTTP request utilities.

use std::time::{Duration, Instant};

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};

use crate::error::{ServerError, ServerResult};

// ============================================================================
// HTTP Request Utilities
// ============================================================================

/// Sends an HTTP POST request to an LLM chat completions endpoint.
///
/// This function uses reqwest with `no_proxy()` configuration to work correctly
/// with local LLM servers like LlamaEdge. The `no_proxy()` setting prevents
/// reqwest from using system proxy settings that can cause 502 Bad Gateway errors.
///
/// # Arguments
///
/// * `url` - The full URL of the chat completions endpoint
/// * `api_key` - Optional API key for authorization
/// * `request_body` - The JSON request body as a serde_json::Value
///
/// # Returns
///
/// The parsed JSON response from the server, or an error if the request failed.
pub fn send_llm_request(
    url: &str,
    api_key: Option<&str>,
    request_body: &serde_json::Value,
) -> ServerResult<serde_json::Value> {
    // Use a blocking runtime for sync context
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| ServerError::Operation(format!("Failed to create runtime: {}", e)))?;

    rt.block_on(async {
        let client = crate::utils::create_http_client();

        let mut request = client
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .json(request_body);

        // Add authorization header if API key is present
        if let Some(key) = api_key {
            if !key.is_empty() {
                let auth = if key.starts_with("Bearer ") {
                    key.to_string()
                } else {
                    format!("Bearer {}", key)
                };
                request = request.header(AUTHORIZATION, auth);
            }
        }

        let response = request
            .send()
            .await
            .map_err(|e| ServerError::Operation(format!("Request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ServerError::Operation(format!(
                "Server returned error {}: {}",
                status, body
            )));
        }

        response
            .json()
            .await
            .map_err(|e| ServerError::Operation(format!("Failed to parse LLM response: {}", e)))
    })
}

/// Extracts the message content from a chat completion response.
///
/// # Arguments
///
/// * `response` - The JSON response from the LLM
///
/// # Returns
///
/// The content string, or an error if the response format is invalid.
pub fn extract_chat_content(response: &serde_json::Value) -> ServerResult<String> {
    response["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| ServerError::Operation("Invalid response format from LLM".to_string()))
}

/// Sends an HTTP POST request to an LLM chat completions endpoint (async version).
///
/// This function uses reqwest with `no_proxy()` configuration to work correctly
/// with local LLM servers like LlamaEdge. The `no_proxy()` setting prevents
/// reqwest from using system proxy settings that can cause 502 Bad Gateway errors.
///
/// # Arguments
///
/// * `url` - The full URL of the chat completions endpoint
/// * `api_key` - Optional API key for authorization
/// * `request_body` - The JSON request body as a serde_json::Value
///
/// # Returns
///
/// The raw response text from the server, or an error if the request failed.
pub async fn send_llm_request_async(
    url: String,
    api_key: Option<String>,
    request_body: serde_json::Value,
) -> ServerResult<String> {
    let client = crate::utils::create_http_client();

    let mut request = client
        .post(&url)
        .header(CONTENT_TYPE, "application/json")
        .json(&request_body);

    // Add authorization header if API key is present
    if let Some(key) = &api_key {
        if !key.is_empty() {
            let auth = if key.starts_with("Bearer ") {
                key.clone()
            } else {
                format!("Bearer {}", key)
            };
            request = request.header(AUTHORIZATION, auth);
        }
    }

    let response = request
        .send()
        .await
        .map_err(|e| ServerError::Operation(format!("Request failed: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(ServerError::Operation(format!(
            "Server returned error {}: {}",
            status, body
        )));
    }

    response
        .text()
        .await
        .map_err(|e| ServerError::Operation(format!("Failed to read response: {}", e)))
}

/// Time budget manager for allocating execution time across subtasks.
///
/// `TimeBudget` tracks the total time budget for a plan execution and
/// provides methods to query remaining time and allocate time slices
/// for pending subtasks.
///
/// # Example
///
/// ```rust
/// use aries::chat::shared::TimeBudget;
///
/// let budget = TimeBudget::new(600); // 10 minutes total budget
///
/// // Check remaining time
/// let remaining = budget.remaining();
///
/// // Allocate time for 5 pending subtasks
/// let per_task = budget.allocate(5);
///
/// // Check if budget is exhausted
/// if budget.is_exhausted() {
///     println!("Time budget exhausted!");
/// }
/// ```
#[derive(Debug)]
pub struct TimeBudget {
    total_budget: Duration,
    start_time: Instant,
}

impl TimeBudget {
    /// Creates a new time budget with the specified total seconds.
    ///
    /// # Arguments
    ///
    /// * `total_secs` - Total time budget in seconds
    ///
    /// # Returns
    ///
    /// A new `TimeBudget` instance with the clock started immediately.
    pub fn new(total_secs: u64) -> Self {
        Self {
            total_budget: Duration::from_secs(total_secs),
            start_time: Instant::now(),
        }
    }

    /// Returns the remaining time in the budget.
    ///
    /// If the elapsed time exceeds the total budget, returns `Duration::ZERO`.
    pub fn remaining(&self) -> Duration {
        let elapsed = self.start_time.elapsed();
        if elapsed >= self.total_budget {
            Duration::ZERO
        } else {
            self.total_budget - elapsed
        }
    }

    /// Allocates time for pending subtasks.
    ///
    /// Divides the remaining time (minus a 10% buffer) equally among
    /// the specified number of pending tasks.
    ///
    /// # Arguments
    ///
    /// * `pending_count` - Number of subtasks still pending execution
    ///
    /// # Returns
    ///
    /// The allocated time per subtask. Returns `Duration::ZERO` if
    /// `pending_count` is 0.
    pub fn allocate(&self, pending_count: usize) -> Duration {
        if pending_count == 0 {
            return Duration::ZERO;
        }
        let remaining = self.remaining();
        // Reserve 10% as buffer
        let allocatable = remaining.mul_f64(0.9);
        allocatable / pending_count as u32
    }

    /// Checks if the time budget is exhausted.
    ///
    /// # Returns
    ///
    /// `true` if the elapsed time has reached or exceeded the total budget.
    pub fn is_exhausted(&self) -> bool {
        self.start_time.elapsed() >= self.total_budget
    }

    /// Returns the elapsed time since the budget was created.
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Returns the total budget duration.
    #[allow(dead_code)] // Used in tests and as public API
    pub fn total(&self) -> Duration {
        self.total_budget
    }
}

#[cfg(test)]
mod tests {
    use std::thread::sleep;

    use super::*;

    #[test]
    fn test_new_budget() {
        let budget = TimeBudget::new(600);
        assert_eq!(budget.total(), Duration::from_secs(600));
        assert!(!budget.is_exhausted());
    }

    #[test]
    fn test_remaining_time() {
        let budget = TimeBudget::new(10);

        // Initially, remaining should be close to total
        let remaining = budget.remaining();
        assert!(remaining <= Duration::from_secs(10));
        assert!(remaining > Duration::from_secs(9));
    }

    #[test]
    fn test_allocate_zero_pending() {
        let budget = TimeBudget::new(100);
        let allocated = budget.allocate(0);
        assert_eq!(allocated, Duration::ZERO);
    }

    #[test]
    fn test_allocate_multiple_tasks() {
        let budget = TimeBudget::new(100);
        let allocated = budget.allocate(5);

        // Should allocate approximately 90% / 5 = 18 seconds per task
        // (with some tolerance for elapsed time)
        assert!(allocated.as_secs() >= 15);
        assert!(allocated.as_secs() <= 20);
    }

    #[test]
    fn test_is_exhausted() {
        // Create a very short budget
        let budget = TimeBudget::new(0);
        assert!(budget.is_exhausted());

        // Create a longer budget
        let budget = TimeBudget::new(100);
        assert!(!budget.is_exhausted());
    }

    #[test]
    fn test_elapsed_time() {
        let budget = TimeBudget::new(100);
        sleep(Duration::from_millis(50));
        let elapsed = budget.elapsed();
        assert!(elapsed >= Duration::from_millis(50));
    }
}
