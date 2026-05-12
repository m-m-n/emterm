//! Rate limiting and DoS prevention for image processing.
//!
//! Provides mechanisms to prevent denial-of-service attacks through excessive
//! image commands.
//!
//! # Limits
//!
//! - Maximum concurrent decode operations: 4
//! - Maximum image commands per second: 100
//! - Timeout for incomplete transfers: 30 seconds

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Maximum concurrent decode operations.
pub const MAX_CONCURRENT_DECODES: usize = 4;

/// Maximum image commands per second.
pub const MAX_COMMANDS_PER_SECOND: u32 = 100;

/// Timeout for incomplete transfers in seconds.
pub const TRANSFER_TIMEOUT_SECS: u64 = 30;

/// Rate limiter for image commands.
pub struct RateLimiter {
    /// Timestamps of recent commands.
    command_times: VecDeque<Instant>,

    /// Maximum commands per window.
    max_commands: u32,

    /// Window duration.
    window: Duration,

    /// Current active decode count.
    active_decodes: usize,

    /// Maximum concurrent decodes.
    max_concurrent: usize,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiter {
    /// Create a new rate limiter with default settings.
    pub fn new() -> Self {
        Self {
            command_times: VecDeque::new(),
            max_commands: MAX_COMMANDS_PER_SECOND,
            window: Duration::from_secs(1),
            active_decodes: 0,
            max_concurrent: MAX_CONCURRENT_DECODES,
        }
    }

    /// Create a rate limiter with custom settings.
    pub fn with_limits(max_commands_per_second: u32, max_concurrent_decodes: usize) -> Self {
        Self {
            command_times: VecDeque::new(),
            max_commands: max_commands_per_second,
            window: Duration::from_secs(1),
            active_decodes: 0,
            max_concurrent: max_concurrent_decodes,
        }
    }

    /// Check if a new command is allowed.
    ///
    /// Returns `true` if the command is allowed, `false` if rate limited.
    pub fn check_command(&mut self) -> bool {
        let now = Instant::now();

        // Remove old timestamps outside the window
        while let Some(front) = self.command_times.front() {
            if now.duration_since(*front) > self.window {
                self.command_times.pop_front();
            } else {
                break;
            }
        }

        // Check if under limit
        if self.command_times.len() >= self.max_commands as usize {
            log::warn!(
                "Rate limit exceeded: {} commands in {:?}",
                self.command_times.len(),
                self.window
            );
            return false;
        }

        // Record this command
        self.command_times.push_back(now);
        true
    }

    /// Try to start a decode operation.
    ///
    /// Returns `true` if a decode slot is available, `false` if at capacity.
    pub fn try_start_decode(&mut self) -> bool {
        if self.active_decodes >= self.max_concurrent {
            log::warn!(
                "Concurrent decode limit reached: {}/{}",
                self.active_decodes,
                self.max_concurrent
            );
            return false;
        }

        self.active_decodes += 1;
        true
    }

    /// Complete a decode operation.
    pub fn finish_decode(&mut self) {
        self.active_decodes = self.active_decodes.saturating_sub(1);
    }

    /// Get the current number of active decode operations.
    pub fn active_decode_count(&self) -> usize {
        self.active_decodes
    }

    /// Get the command rate (commands in current window).
    pub fn current_rate(&self) -> usize {
        let now = Instant::now();

        self.command_times
            .iter()
            .filter(|t| now.duration_since(**t) <= self.window)
            .count()
    }

    /// Reset the limiter state.
    pub fn reset(&mut self) {
        self.command_times.clear();
        self.active_decodes = 0;
    }
}

/// Transfer timeout tracker for incomplete chunked transfers.
pub struct TransferTimeoutTracker {
    /// Transfers with their start times.
    transfers: Vec<(u32, Instant)>,

    /// Timeout duration.
    timeout: Duration,
}

impl Default for TransferTimeoutTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl TransferTimeoutTracker {
    /// Create a new tracker with default timeout.
    pub fn new() -> Self {
        Self {
            transfers: Vec::new(),
            timeout: Duration::from_secs(TRANSFER_TIMEOUT_SECS),
        }
    }

    /// Create a tracker with custom timeout.
    pub fn with_timeout(timeout_secs: u64) -> Self {
        Self {
            transfers: Vec::new(),
            timeout: Duration::from_secs(timeout_secs),
        }
    }

    /// Start tracking a transfer.
    pub fn start_transfer(&mut self, id: u32) {
        // Remove any existing entry for this ID
        self.transfers.retain(|(i, _)| *i != id);
        self.transfers.push((id, Instant::now()));
    }

    /// Complete a transfer (stop tracking).
    pub fn complete_transfer(&mut self, id: u32) {
        self.transfers.retain(|(i, _)| *i != id);
    }

    /// Get IDs of timed-out transfers and remove them.
    pub fn get_and_clear_timeouts(&mut self) -> Vec<u32> {
        let now = Instant::now();

        let (timed_out, remaining): (Vec<_>, Vec<_>) = self
            .transfers
            .drain(..)
            .partition(|(_, start)| now.duration_since(*start) > self.timeout);

        self.transfers = remaining;

        timed_out.into_iter().map(|(id, _)| id).collect()
    }

    /// Check if a transfer has timed out.
    pub fn is_timed_out(&self, id: u32) -> bool {
        let now = Instant::now();

        self.transfers
            .iter()
            .any(|(i, start)| *i == id && now.duration_since(*start) > self.timeout)
    }

    /// Reset the tracker.
    pub fn reset(&mut self) {
        self.transfers.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    // =========================================================================
    // RateLimiter Tests
    // =========================================================================

    #[test]
    fn test_rate_limiter_creation() {
        let limiter = RateLimiter::new();
        assert_eq!(limiter.active_decode_count(), 0);
        assert_eq!(limiter.current_rate(), 0);
    }

    #[test]
    fn test_rate_limiter_custom() {
        let limiter = RateLimiter::with_limits(50, 2);
        assert_eq!(limiter.max_commands, 50);
        assert_eq!(limiter.max_concurrent, 2);
    }

    #[test]
    fn test_rate_limiter_allows_commands() {
        let mut limiter = RateLimiter::with_limits(10, 4);

        for _ in 0..10 {
            assert!(limiter.check_command());
        }
    }

    #[test]
    fn test_rate_limiter_blocks_excess() {
        let mut limiter = RateLimiter::with_limits(5, 4);

        for _ in 0..5 {
            assert!(limiter.check_command());
        }

        // 6th command should be blocked
        assert!(!limiter.check_command());
    }

    #[test]
    fn test_rate_limiter_decode_slots() {
        let mut limiter = RateLimiter::with_limits(100, 2);

        // Start two decodes
        assert!(limiter.try_start_decode());
        assert!(limiter.try_start_decode());

        // Third should fail
        assert!(!limiter.try_start_decode());
        assert_eq!(limiter.active_decode_count(), 2);

        // Finish one
        limiter.finish_decode();
        assert_eq!(limiter.active_decode_count(), 1);

        // Now we can start another
        assert!(limiter.try_start_decode());
    }

    #[test]
    fn test_rate_limiter_reset() {
        let mut limiter = RateLimiter::with_limits(5, 2);

        for _ in 0..5 {
            limiter.check_command();
        }
        limiter.try_start_decode();

        limiter.reset();

        assert_eq!(limiter.current_rate(), 0);
        assert_eq!(limiter.active_decode_count(), 0);
    }

    // =========================================================================
    // TransferTimeoutTracker Tests
    // =========================================================================

    #[test]
    fn test_timeout_tracker_creation() {
        let tracker = TransferTimeoutTracker::new();
        assert_eq!(tracker.timeout, Duration::from_secs(30));
    }

    #[test]
    fn test_timeout_tracker_custom() {
        let tracker = TransferTimeoutTracker::with_timeout(10);
        assert_eq!(tracker.timeout, Duration::from_secs(10));
    }

    #[test]
    fn test_timeout_tracker_start_complete() {
        let mut tracker = TransferTimeoutTracker::new();

        tracker.start_transfer(1);
        tracker.start_transfer(2);

        tracker.complete_transfer(1);

        // 1 should be removed, 2 should remain tracked
        assert_eq!(tracker.transfers.len(), 1);
        assert_eq!(tracker.transfers[0].0, 2);
    }

    #[test]
    fn test_timeout_tracker_timeout_detection() {
        let mut tracker = TransferTimeoutTracker::with_timeout(0); // Immediate timeout

        tracker.start_transfer(1);
        sleep(Duration::from_millis(10)); // Small delay to ensure timeout

        assert!(tracker.is_timed_out(1));
    }

    #[test]
    fn test_timeout_tracker_get_and_clear() {
        let mut tracker = TransferTimeoutTracker::with_timeout(0);

        tracker.start_transfer(1);
        tracker.start_transfer(2);
        sleep(Duration::from_millis(10));

        let timed_out = tracker.get_and_clear_timeouts();

        assert!(timed_out.contains(&1));
        assert!(timed_out.contains(&2));
        assert!(tracker.transfers.is_empty());
    }

    #[test]
    fn test_timeout_tracker_reset() {
        let mut tracker = TransferTimeoutTracker::new();

        tracker.start_transfer(1);
        tracker.start_transfer(2);

        tracker.reset();

        assert!(tracker.transfers.is_empty());
    }
}
