//! Shared utilities for chat modes
//!
//! This module contains shared components used across different chat modes,
//! such as time budget management for Plan mode.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// Time budget manager for allocating execution time across subtasks.
///
/// `TimeBudget` tracks the total time budget for a plan execution and
/// provides methods to query remaining time and allocate time slices
/// for pending subtasks.
///
/// # Example
///
/// ```rust
/// use moss::chat::shared::TimeBudget;
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
    /// Accumulated pause time (e.g. HITL wait) excluded from the budget.
    /// Uses `Arc<AtomicU64>` for thread-safe interior mutability so that
    /// parallel subtask executors can report pause durations concurrently.
    pause_nanos: Arc<AtomicU64>,
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
            pause_nanos: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Returns the remaining time in the budget (excluding paused time).
    ///
    /// If the effective elapsed time exceeds the total budget, returns `Duration::ZERO`.
    pub fn remaining(&self) -> Duration {
        let effective = self.effective_elapsed();
        if effective >= self.total_budget {
            Duration::ZERO
        } else {
            self.total_budget - effective
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
    /// Returns `true` if the effective elapsed time (excluding paused time)
    /// has reached or exceeded the total budget.
    pub fn is_exhausted(&self) -> bool {
        self.effective_elapsed() >= self.total_budget
    }

    /// Returns the effective elapsed time (wall-clock minus accumulated pause).
    pub fn elapsed(&self) -> Duration {
        self.effective_elapsed()
    }

    /// Adds pause duration that should not count against the budget.
    ///
    /// This is used to exclude time spent waiting for human-in-the-loop
    /// approval from the plan execution timeout. Thread-safe: can be called
    /// concurrently from parallel subtask executors.
    pub fn add_pause(&self, duration: Duration) {
        self.pause_nanos
            .fetch_add(duration.as_nanos() as u64, Ordering::Relaxed);
    }

    /// Returns a cloneable handle to the pause tracker.
    ///
    /// Subtask executors can use this to report their HITL wait time back
    /// to the plan-level time budget. The `Arc` allows the handle to be
    /// sent into `tokio::spawn` tasks for parallel execution.
    pub fn pause_tracker(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.pause_nanos)
    }

    /// Returns the effective elapsed time (wall-clock minus accumulated pause).
    fn effective_elapsed(&self) -> Duration {
        let wall_elapsed = self.start_time.elapsed();
        let pause = Duration::from_nanos(self.pause_nanos.load(Ordering::Relaxed));
        wall_elapsed.saturating_sub(pause)
    }

    /// Returns the raw wall-clock elapsed time (without subtracting pause).
    pub fn wall_elapsed(&self) -> Duration {
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

    #[test]
    fn test_add_pause_extends_budget() {
        // Create a short budget (1 second)
        let budget = TimeBudget::new(1);

        // Sleep to consume most of the budget
        sleep(Duration::from_millis(800));

        // Add pause that covers the elapsed time so budget is not exhausted
        budget.add_pause(Duration::from_secs(5));
        assert!(!budget.is_exhausted());
        // Effective elapsed ≈ 800ms - 5s = 0 (saturating_sub), remaining ≈ 1s
        assert!(budget.remaining() > Duration::from_millis(500));
    }

    #[test]
    fn test_pause_tracker_shared() {
        let budget = TimeBudget::new(100);
        let tracker = budget.pause_tracker();

        // Adding via tracker should be reflected in budget
        tracker.fetch_add(Duration::from_secs(5).as_nanos() as u64, Ordering::Relaxed);

        // wall_elapsed should be close to 0, effective elapsed should be 0 (saturating_sub)
        let wall = budget.wall_elapsed();
        let effective = budget.elapsed();
        assert!(effective <= wall);
    }

    #[test]
    fn test_remaining_accounts_for_pause() {
        let budget = TimeBudget::new(10);
        sleep(Duration::from_millis(100));

        let remaining_before = budget.remaining();
        // Simulate a 5-second pause
        budget.add_pause(Duration::from_secs(5));
        let remaining_after = budget.remaining();

        // After adding pause, remaining should be larger
        assert!(remaining_after > remaining_before);
    }
}
