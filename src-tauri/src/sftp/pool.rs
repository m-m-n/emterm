//! Thread-safe concurrent upload pool with configurable slot limit.
//!
//! Limits the number of simultaneous SFTP upload subprocesses.
//! Threads calling `acquire_slot` block via Condvar until a slot is available.

use std::collections::HashSet;
use std::sync::{Condvar, Mutex};

/// Internal state protected by Mutex.
struct PoolState {
    max_concurrent: u16,
    active: HashSet<String>,
}

/// Thread-safe concurrent upload pool for use as Tauri managed state.
///
/// Each `sftp_upload` command thread calls `acquire_slot` before spawning
/// the sftp subprocess, and `release_slot` when done (success, failure, or cancel).
pub struct ConcurrentUploadPool {
    state: Mutex<PoolState>,
    slot_available: Condvar,
}

impl ConcurrentUploadPool {
    /// Create a new pool with the specified concurrency limit (minimum 1).
    pub fn new(max_concurrent: u16) -> Self {
        Self {
            state: Mutex::new(PoolState {
                max_concurrent: max_concurrent.max(1),
                active: HashSet::new(),
            }),
            slot_available: Condvar::new(),
        }
    }

    /// Block until a slot is available, then mark this session as active.
    pub fn acquire_slot(&self, session_id: &str) {
        let mut state = self.state.lock().unwrap();
        while state.active.len() >= state.max_concurrent as usize {
            state = self.slot_available.wait(state).unwrap();
        }
        state.active.insert(session_id.to_string());
    }

    /// Release a slot and wake one waiting thread.
    pub fn release_slot(&self, session_id: &str) {
        let mut state = self.state.lock().unwrap();
        state.active.remove(session_id);
        self.slot_available.notify_one();
    }

    /// Check if a session is currently active.
    pub fn is_active(&self, session_id: &str) -> bool {
        let state = self.state.lock().unwrap();
        state.active.contains(session_id)
    }

    /// Check if there are any active uploads.
    pub fn has_active_uploads(&self) -> bool {
        let state = self.state.lock().unwrap();
        !state.active.is_empty()
    }

    /// Get the number of currently active uploads.
    pub fn active_count(&self) -> usize {
        let state = self.state.lock().unwrap();
        state.active.len()
    }

    /// Update the maximum concurrent uploads limit (minimum 1).
    pub fn set_max_concurrent(&self, max: u16) {
        let mut state = self.state.lock().unwrap();
        state.max_concurrent = max.max(1);
        // Wake all waiters so they can re-check with new limit
        self.slot_available.notify_all();
    }

    /// Get the current maximum concurrent uploads limit.
    pub fn max_concurrent(&self) -> u16 {
        let state = self.state.lock().unwrap();
        state.max_concurrent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_new_pool_defaults() {
        let pool = ConcurrentUploadPool::new(4);
        assert_eq!(pool.max_concurrent(), 4);
        assert_eq!(pool.active_count(), 0);
        assert!(!pool.has_active_uploads());
    }

    #[test]
    fn test_new_pool_min_concurrent_is_one() {
        let pool = ConcurrentUploadPool::new(0);
        assert_eq!(pool.max_concurrent(), 1);
    }

    #[test]
    fn test_acquire_and_release() {
        let pool = ConcurrentUploadPool::new(2);
        pool.acquire_slot("upload-1");
        assert_eq!(pool.active_count(), 1);
        assert!(pool.is_active("upload-1"));

        pool.acquire_slot("upload-2");
        assert_eq!(pool.active_count(), 2);

        pool.release_slot("upload-1");
        assert_eq!(pool.active_count(), 1);
        assert!(!pool.is_active("upload-1"));
        assert!(pool.is_active("upload-2"));

        pool.release_slot("upload-2");
        assert_eq!(pool.active_count(), 0);
    }

    #[test]
    fn test_acquire_blocks_when_full() {
        let pool = Arc::new(ConcurrentUploadPool::new(1));
        pool.acquire_slot("upload-1");

        let pool2 = Arc::clone(&pool);
        let handle = std::thread::spawn(move || {
            pool2.acquire_slot("upload-2");
            pool2.active_count()
        });

        // Give thread time to block
        std::thread::sleep(std::time::Duration::from_millis(50));
        // upload-2 should still be waiting
        assert_eq!(pool.active_count(), 1);

        // Release slot -> unblocks the waiting thread
        pool.release_slot("upload-1");
        let count = handle.join().unwrap();
        // upload-2 is now active
        assert!(count >= 1);

        pool.release_slot("upload-2");
        assert_eq!(pool.active_count(), 0);
    }

    #[test]
    fn test_release_nonexistent_is_noop() {
        let pool = ConcurrentUploadPool::new(2);
        pool.release_slot("nonexistent"); // Should not panic
        assert_eq!(pool.active_count(), 0);
    }

    #[test]
    fn test_set_max_concurrent() {
        let pool = ConcurrentUploadPool::new(4);
        pool.set_max_concurrent(2);
        assert_eq!(pool.max_concurrent(), 2);
    }

    #[test]
    fn test_set_max_concurrent_minimum_one() {
        let pool = ConcurrentUploadPool::new(4);
        pool.set_max_concurrent(0);
        assert_eq!(pool.max_concurrent(), 1);
    }

    #[test]
    fn test_has_active_uploads() {
        let pool = ConcurrentUploadPool::new(2);
        assert!(!pool.has_active_uploads());
        pool.acquire_slot("a");
        assert!(pool.has_active_uploads());
        pool.release_slot("a");
        assert!(!pool.has_active_uploads());
    }

    #[test]
    fn test_set_max_concurrent_unblocks_waiters() {
        let pool = Arc::new(ConcurrentUploadPool::new(1));
        pool.acquire_slot("upload-1");

        let pool2 = Arc::clone(&pool);
        let handle = std::thread::spawn(move || {
            pool2.acquire_slot("upload-2");
        });

        std::thread::sleep(std::time::Duration::from_millis(50));
        // Increase limit -> should unblock waiting thread
        pool.set_max_concurrent(2);
        handle.join().unwrap();

        assert_eq!(pool.active_count(), 2);
        pool.release_slot("upload-1");
        pool.release_slot("upload-2");
    }
}
