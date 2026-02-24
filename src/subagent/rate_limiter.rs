//! Rate Limiter for Sub-Agent API calls
//!
//! This module provides a token bucket based rate limiter to control the rate of
//! API requests and token usage for Sub-Agents.
//!
//! # Features
//!
//! - Token bucket algorithm for smooth rate limiting
//! - Separate limits for requests per minute (RPM) and tokens per minute (TPM)
//! - Support for waiting until tokens are available
//! - Exponential backoff for retry logic
//!
//! # Example
//!
//! ```rust,ignore
//! use moss::subagent::{RateLimiter, SubAgentRateLimitConfig};
//!
//! let config = SubAgentRateLimitConfig::default();
//! let limiter = RateLimiter::new(&config);
//!
//! // Acquire a request slot
//! if limiter.acquire_request().await {
//!     // Make the API request
//! }
//!
//! // Acquire tokens for response
//! if limiter.acquire_tokens(1000).await {
//!     // Process the response
//! }
//! ```

use std::time::Duration;

use tokio::{sync::Mutex, time::Instant};

use super::SubAgentRateLimitConfig;

// ============================================================================
// TokenBucket - 令牌桶实现
// ============================================================================

/// A token bucket for rate limiting.
///
/// The token bucket algorithm allows for smooth rate limiting with burst capacity.
/// Tokens are refilled at a constant rate up to the bucket's capacity.
#[derive(Debug)]
pub struct TokenBucket {
    /// Maximum number of tokens the bucket can hold
    capacity: f64,
    /// Current number of tokens (uses mutex for async safety)
    tokens: Mutex<f64>,
    /// Rate at which tokens are refilled (tokens per second)
    refill_rate: f64,
    /// Last time tokens were refilled
    last_refill: Mutex<Instant>,
}

impl TokenBucket {
    /// Create a new token bucket.
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum number of tokens the bucket can hold
    /// * `refill_rate_per_minute` - Rate at which tokens are refilled (per minute)
    pub fn new(capacity: u64, refill_rate_per_minute: u64) -> Self {
        Self {
            capacity: capacity as f64,
            tokens: Mutex::new(capacity as f64),
            refill_rate: refill_rate_per_minute as f64 / 60.0,
            last_refill: Mutex::new(Instant::now()),
        }
    }

    /// Try to acquire a single token without waiting.
    ///
    /// Returns `true` if a token was acquired, `false` otherwise.
    pub async fn try_acquire(&self) -> bool {
        self.try_acquire_n(1.0).await
    }

    /// Try to acquire `n` tokens without waiting.
    ///
    /// Returns `true` if tokens were acquired, `false` otherwise.
    pub async fn try_acquire_n(&self, n: f64) -> bool {
        let mut tokens = self.tokens.lock().await;
        let mut last_refill = self.last_refill.lock().await;

        // Refill tokens based on elapsed time
        let now = Instant::now();
        let elapsed = now.duration_since(*last_refill).as_secs_f64();
        *tokens = (*tokens + elapsed * self.refill_rate).min(self.capacity);
        *last_refill = now;

        if *tokens >= n {
            *tokens -= n;
            true
        } else {
            false
        }
    }

    /// Acquire a single token, waiting if necessary.
    ///
    /// # Arguments
    ///
    /// * `max_wait` - Maximum time to wait for a token
    ///
    /// Returns `true` if a token was acquired within the timeout, `false` otherwise.
    pub async fn acquire(&self, max_wait: Duration) -> bool {
        self.acquire_n(1.0, max_wait).await
    }

    /// Acquire `n` tokens, waiting if necessary.
    ///
    /// # Arguments
    ///
    /// * `n` - Number of tokens to acquire
    /// * `max_wait` - Maximum time to wait for tokens
    ///
    /// Returns `true` if tokens were acquired within the timeout, `false` otherwise.
    pub async fn acquire_n(&self, n: f64, max_wait: Duration) -> bool {
        let start = Instant::now();
        let poll_interval = Duration::from_millis(50);

        loop {
            if self.try_acquire_n(n).await {
                return true;
            }

            if start.elapsed() >= max_wait {
                return false;
            }

            // Wait before retrying
            let remaining = max_wait.saturating_sub(start.elapsed());
            let sleep_time = poll_interval.min(remaining);
            tokio::time::sleep(sleep_time).await;
        }
    }

    /// Get the current number of available tokens.
    pub async fn available(&self) -> f64 {
        let mut tokens = self.tokens.lock().await;
        let mut last_refill = self.last_refill.lock().await;

        // Refill tokens based on elapsed time
        let now = Instant::now();
        let elapsed = now.duration_since(*last_refill).as_secs_f64();
        *tokens = (*tokens + elapsed * self.refill_rate).min(self.capacity);
        *last_refill = now;

        *tokens
    }

    /// Get the time until the bucket will be full.
    pub async fn time_to_full(&self) -> Duration {
        let available = self.available().await;
        let deficit = self.capacity - available;
        if deficit <= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(deficit / self.refill_rate)
        }
    }
}

// ============================================================================
// RateLimiter - 组合限流器
// ============================================================================

/// A combined rate limiter for API requests and token usage.
///
/// This limiter uses two separate token buckets:
/// - One for requests per minute (RPM)
/// - One for tokens per minute (TPM)
///
/// Both limits must be satisfied for an operation to proceed.
#[derive(Debug)]
pub struct RateLimiter {
    /// Token bucket for request rate limiting
    rpm_bucket: TokenBucket,
    /// Token bucket for token usage limiting
    tpm_bucket: TokenBucket,
    /// Configuration for retry behavior
    config: SubAgentRateLimitConfig,
}

impl RateLimiter {
    /// Create a new rate limiter from configuration.
    pub fn new(config: &SubAgentRateLimitConfig) -> Self {
        Self {
            rpm_bucket: TokenBucket::new(
                config.requests_per_minute as u64,
                config.requests_per_minute as u64,
            ),
            tpm_bucket: TokenBucket::new(config.tokens_per_minute, config.tokens_per_minute),
            config: config.clone(),
        }
    }

    /// Try to acquire a request slot without waiting.
    ///
    /// Returns `true` if a request slot was acquired, `false` otherwise.
    pub async fn try_acquire_request(&self) -> bool {
        self.rpm_bucket.try_acquire().await
    }

    /// Acquire a request slot, waiting if necessary.
    ///
    /// # Arguments
    ///
    /// * `max_wait` - Maximum time to wait for a slot
    ///
    /// Returns `true` if a slot was acquired, `false` otherwise.
    pub async fn acquire_request(&self, max_wait: Duration) -> bool {
        self.rpm_bucket.acquire(max_wait).await
    }

    /// Acquire a request slot with automatic retry using exponential backoff.
    ///
    /// Returns `true` if a slot was acquired within the retry limit, `false` otherwise.
    pub async fn acquire_request_with_retry(&self) -> bool {
        for attempt in 0..self.config.retry_max_attempts {
            if self.try_acquire_request().await {
                return true;
            }

            // Calculate exponential backoff delay
            let delay = self.config.retry_delay_for_attempt(attempt);
            tokio::time::sleep(delay).await;
        }

        // Final attempt
        self.try_acquire_request().await
    }

    /// Try to acquire token quota without waiting.
    ///
    /// # Arguments
    ///
    /// * `token_count` - Number of tokens to acquire
    ///
    /// Returns `true` if the quota was acquired, `false` otherwise.
    pub async fn try_acquire_tokens(&self, token_count: u64) -> bool {
        self.tpm_bucket.try_acquire_n(token_count as f64).await
    }

    /// Acquire token quota, waiting if necessary.
    ///
    /// # Arguments
    ///
    /// * `token_count` - Number of tokens to acquire
    /// * `max_wait` - Maximum time to wait
    ///
    /// Returns `true` if the quota was acquired, `false` otherwise.
    pub async fn acquire_tokens(&self, token_count: u64, max_wait: Duration) -> bool {
        self.tpm_bucket
            .acquire_n(token_count as f64, max_wait)
            .await
    }

    /// Try to acquire both a request slot and token quota atomically.
    ///
    /// Note: This is not truly atomic - it acquires the request slot first,
    /// then the token quota. If token acquisition fails, the request slot
    /// is still consumed.
    ///
    /// # Arguments
    ///
    /// * `token_count` - Number of tokens to acquire
    /// * `max_wait` - Maximum time to wait for both acquisitions
    ///
    /// Returns `true` if both were acquired, `false` otherwise.
    pub async fn acquire_request_and_tokens(&self, token_count: u64, max_wait: Duration) -> bool {
        let start = Instant::now();

        // First acquire request slot
        if !self.rpm_bucket.acquire(max_wait).await {
            return false;
        }

        // Then acquire token quota with remaining time
        let remaining = max_wait.saturating_sub(start.elapsed());
        self.tpm_bucket
            .acquire_n(token_count as f64, remaining)
            .await
    }

    /// Check if rate limiting is currently in effect.
    ///
    /// Returns `true` if either bucket is empty, `false` otherwise.
    pub async fn is_limited(&self) -> bool {
        let rpm_available = self.rpm_bucket.available().await;
        let tpm_available = self.tpm_bucket.available().await;
        rpm_available < 1.0 || tpm_available < 1.0
    }

    /// Get the current available request slots.
    pub async fn available_requests(&self) -> u32 {
        self.rpm_bucket.available().await as u32
    }

    /// Get the current available token quota.
    pub async fn available_tokens(&self) -> u64 {
        self.tpm_bucket.available().await as u64
    }

    /// Get estimated time until a request can be made.
    pub async fn time_until_request_available(&self) -> Duration {
        if self.rpm_bucket.available().await >= 1.0 {
            Duration::ZERO
        } else {
            // Time for one token to be refilled
            let rate = self.config.requests_per_minute as f64 / 60.0;
            if rate > 0.0 {
                Duration::from_secs_f64(1.0 / rate)
            } else {
                Duration::MAX
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_token_bucket_basic() {
        // 60 tokens per minute = 1 token per second
        let bucket = TokenBucket::new(10, 60);

        // Should be able to acquire initial tokens
        assert!(bucket.try_acquire().await);
        assert!(bucket.try_acquire().await);

        // Check available count decreased
        let available = bucket.available().await;
        assert!(available < 10.0);
    }

    #[tokio::test]
    async fn test_token_bucket_exhaustion() {
        // Small bucket for testing
        let bucket = TokenBucket::new(2, 60);

        // Exhaust all tokens
        assert!(bucket.try_acquire().await);
        assert!(bucket.try_acquire().await);

        // Should fail now
        assert!(!bucket.try_acquire().await);
    }

    #[tokio::test]
    async fn test_token_bucket_refill() {
        // 600 tokens per minute = 10 tokens per second
        let bucket = TokenBucket::new(10, 600);

        // Exhaust all tokens
        for _ in 0..10 {
            assert!(bucket.try_acquire().await);
        }

        // Should fail
        assert!(!bucket.try_acquire().await);

        // Wait a bit for refill
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Should have some tokens now
        assert!(bucket.try_acquire().await);
    }

    #[tokio::test]
    async fn test_token_bucket_acquire_with_wait() {
        // 600 tokens per minute = 10 tokens per second
        let bucket = TokenBucket::new(1, 600);

        // Use the only token
        assert!(bucket.try_acquire().await);

        // Acquire with wait should succeed after refill
        let result = bucket.acquire(Duration::from_millis(200)).await;
        assert!(result);
    }

    #[tokio::test]
    async fn test_rate_limiter_basic() {
        let config = SubAgentRateLimitConfig {
            requests_per_minute: 60,
            tokens_per_minute: 10000,
            retry_max_attempts: 3,
            retry_base_delay_ms: 100,
        };
        let limiter = RateLimiter::new(&config);

        // Should be able to acquire
        assert!(limiter.try_acquire_request().await);
        assert!(limiter.try_acquire_tokens(100).await);
    }

    #[tokio::test]
    async fn test_rate_limiter_combined() {
        let config = SubAgentRateLimitConfig {
            requests_per_minute: 10,
            tokens_per_minute: 1000,
            retry_max_attempts: 3,
            retry_base_delay_ms: 100,
        };
        let limiter = RateLimiter::new(&config);

        // Should be able to acquire both
        assert!(
            limiter
                .acquire_request_and_tokens(100, Duration::from_secs(1))
                .await
        );
    }
}
