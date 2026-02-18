//! PTY Manager implementation.
//!
//! This module provides the `PtyManager` struct for managing multiple
//! PTY sessions concurrently with thread-safe access.

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::{Mutex, RwLock};

use super::{
    PtyError, PtySession, SessionId, WriterRegistry, detect_default_shell, generate_session_id,
};

/// Result of session creation with atomic count.
#[derive(Clone, Serialize)]
pub struct SessionCreatedResult {
    /// The created session ID.
    pub session_id: SessionId,
    /// The session count after creation (captured inside lock).
    pub count: usize,
}

/// Result of session removal with atomic count.
#[derive(Clone, Serialize)]
pub struct SessionRemovedResult {
    /// The session count after removal (captured inside lock).
    pub count: usize,
}

/// Manages multiple PTY sessions with thread-safe access.
///
/// `PtyManager` maintains a registry of active PTY sessions and provides
/// methods for creating, accessing, and removing sessions.
///
/// Also manages a `WriterRegistry` for fast, lock-minimal PTY write operations
/// via dedicated per-session writer threads.
#[derive(Clone)]
pub struct PtyManager {
    /// Thread-safe map of session ID to session instance.
    sessions: Arc<RwLock<HashMap<SessionId, Arc<Mutex<PtySession>>>>>,
    /// Registry of write channel senders for fast PTY writes.
    writer_registry: Arc<WriterRegistry>,
}

impl Default for PtyManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PtyManager {
    /// Creates a new PTY manager with an empty session registry.
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            writer_registry: Arc::new(WriterRegistry::new()),
        }
    }

    /// Returns a reference to the writer registry for fast PTY writes.
    pub fn writer_registry(&self) -> &WriterRegistry {
        &self.writer_registry
    }

    /// Creates a new PTY session with the specified parameters.
    ///
    /// **Deprecated:** Use `create_session_atomic` instead, which also sets up
    /// the dedicated writer thread and registers the write channel in the
    /// WriterRegistry. Sessions created with this method will not have a
    /// writer channel and `pty_write` will fail with `SessionNotFound`.
    ///
    /// # Arguments
    ///
    /// * `shell` - Optional shell path. If `None`, the default shell is used.
    /// * `cols` - Number of terminal columns
    /// * `rows` - Number of terminal rows
    ///
    /// # Returns
    ///
    /// The session ID of the newly created session, or an error if creation fails.
    #[deprecated(note = "Use create_session_atomic instead, which sets up the writer channel")]
    pub async fn create_session(
        &self,
        shell: Option<String>,
        args: Option<Vec<String>>,
        cols: u16,
        rows: u16,
    ) -> Result<SessionId, PtyError> {
        let shell = shell.unwrap_or_else(detect_default_shell);
        let id = generate_session_id();
        let session = PtySession::new(id.clone(), &shell, args, cols, rows)?;

        let mut sessions = self.sessions.write().await;
        sessions.insert(id.clone(), Arc::new(Mutex::new(session)));

        Ok(id)
    }

    /// Retrieves a session by its ID.
    ///
    /// # Arguments
    ///
    /// * `id` - The session ID to look up
    ///
    /// # Returns
    ///
    /// An `Arc<Mutex<PtySession>>` if found, or `None` if the session doesn't exist.
    pub async fn get_session(&self, id: &str) -> Option<Arc<Mutex<PtySession>>> {
        let sessions = self.sessions.read().await;
        sessions.get(id).cloned()
    }

    /// Removes and returns a session by its ID.
    ///
    /// Also cleans up the write channel from the WriterRegistry.
    ///
    /// # Arguments
    ///
    /// * `id` - The session ID to remove
    ///
    /// # Returns
    ///
    /// The removed session if found, or `None` if it didn't exist.
    pub async fn remove_session(&self, id: &str) -> Option<Arc<Mutex<PtySession>>> {
        if let Err(e) = self.writer_registry.remove(id) {
            log::warn!("Failed to remove writer for session {}: {}", id, e);
        }
        let mut sessions = self.sessions.write().await;
        sessions.remove(id)
    }

    /// Returns the number of active sessions.
    #[allow(dead_code)]
    pub async fn session_count(&self) -> usize {
        let sessions = self.sessions.read().await;
        sessions.len()
    }

    /// Creates a new session and returns both the session ID and the count atomically.
    ///
    /// This ensures the count is captured inside the write lock, preventing race conditions
    /// between session creation and count emission (NFR2 compliance).
    ///
    /// Also sets up the dedicated writer thread and registers the write channel
    /// in the WriterRegistry for fast, lock-minimal PTY writes.
    pub async fn create_session_atomic(
        &self,
        shell: Option<String>,
        args: Option<Vec<String>>,
        cols: u16,
        rows: u16,
    ) -> Result<SessionCreatedResult, PtyError> {
        let shell = shell.unwrap_or_else(detect_default_shell);
        let id = generate_session_id();
        let mut session = PtySession::new(id.clone(), &shell, args, cols, rows)?;

        // Extract writer handle and set up dedicated writer thread
        let writer_handle = session
            .take_writer_handle()
            .ok_or_else(|| PtyError::Pty("Failed to take writer handle".to_string()))?;

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        super::writer::spawn_writer_thread(id.clone(), writer_handle, rx);
        self.writer_registry.register(id.clone(), tx)?;

        let mut sessions = self.sessions.write().await;
        sessions.insert(id.clone(), Arc::new(Mutex::new(session)));
        let count = sessions.len(); // Captured inside lock

        Ok(SessionCreatedResult {
            session_id: id,
            count,
        })
    }

    /// Removes a session and returns the session along with the new count atomically.
    ///
    /// This ensures the count is captured inside the write lock, preventing race conditions
    /// between session removal and count emission (NFR2 compliance).
    ///
    /// Also removes the write channel from the WriterRegistry, which causes
    /// the dedicated writer thread to exit naturally.
    pub async fn remove_session_atomic(
        &self,
        id: &str,
    ) -> Option<(Arc<Mutex<PtySession>>, SessionRemovedResult)> {
        // Remove writer channel first (drops sender, writer thread will exit)
        if let Err(e) = self.writer_registry.remove(id) {
            log::warn!("Failed to remove writer for session {}: {}", id, e);
        }

        let mut sessions = self.sessions.write().await;
        let session = sessions.remove(id)?;
        let count = sessions.len(); // Captured inside lock

        Some((session, SessionRemovedResult { count }))
    }
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_manager_creation() {
        let manager = PtyManager::new();
        assert_eq!(manager.session_count().await, 0);
    }

    #[tokio::test]
    async fn test_create_session() {
        let manager = PtyManager::new();
        let result = manager.create_session(None, None, 80, 24).await;

        assert!(result.is_ok(), "Session creation should succeed");
        assert_eq!(manager.session_count().await, 1);

        // Cleanup
        let session_id = result.unwrap();
        if let Some(session) = manager.remove_session(&session_id).await {
            let mut s = session.lock().await;
            let _ = s.kill();
        }
    }

    #[tokio::test]
    async fn test_get_session() {
        let manager = PtyManager::new();
        let session_id = manager.create_session(None, None, 80, 24).await.unwrap();

        let session = manager.get_session(&session_id).await;
        assert!(session.is_some(), "Session should be retrievable");

        let nonexistent = manager.get_session("nonexistent-id").await;
        assert!(
            nonexistent.is_none(),
            "Nonexistent session should return None"
        );

        // Cleanup
        if let Some(session) = manager.remove_session(&session_id).await {
            let mut s = session.lock().await;
            let _ = s.kill();
        }
    }

    #[tokio::test]
    async fn test_remove_session() {
        let manager = PtyManager::new();
        let session_id = manager.create_session(None, None, 80, 24).await.unwrap();

        assert_eq!(manager.session_count().await, 1);

        let removed = manager.remove_session(&session_id).await;
        assert!(removed.is_some(), "Removed session should be returned");
        assert_eq!(manager.session_count().await, 0);

        // Cleanup
        if let Some(session) = removed {
            let mut s = session.lock().await;
            let _ = s.kill();
        }
    }

    #[tokio::test]
    async fn test_multiple_sessions() {
        let manager = PtyManager::new();

        let id1 = manager.create_session(None, None, 80, 24).await.unwrap();
        let id2 = manager.create_session(None, None, 120, 40).await.unwrap();

        assert_eq!(manager.session_count().await, 2);
        assert_ne!(id1, id2, "Session IDs should be unique");

        // Both sessions should be accessible
        assert!(manager.get_session(&id1).await.is_some());
        assert!(manager.get_session(&id2).await.is_some());

        // Cleanup
        for id in [id1, id2] {
            if let Some(session) = manager.remove_session(&id).await {
                let mut s = session.lock().await;
                let _ = s.kill();
            }
        }
    }

    #[tokio::test]
    async fn test_create_session_atomic() {
        let manager = PtyManager::new();
        let result = manager.create_session_atomic(None, None, 80, 24).await;

        assert!(result.is_ok(), "Atomic session creation should succeed");
        let result = result.unwrap();

        // Count should be 1 (captured inside lock)
        assert_eq!(result.count, 1);
        assert!(!result.session_id.is_empty());

        // Verify the session exists
        assert!(manager.get_session(&result.session_id).await.is_some());
        assert_eq!(manager.session_count().await, 1);

        // Cleanup
        if let Some(session) = manager.remove_session(&result.session_id).await {
            let mut s = session.lock().await;
            let _ = s.kill();
        }
    }

    #[tokio::test]
    async fn test_remove_session_atomic() {
        let manager = PtyManager::new();

        // Create two sessions
        let result1 = manager
            .create_session_atomic(None, None, 80, 24)
            .await
            .unwrap();
        let result2 = manager
            .create_session_atomic(None, None, 80, 24)
            .await
            .unwrap();

        assert_eq!(result1.count, 1);
        assert_eq!(result2.count, 2);

        // Remove first session atomically
        let removed1 = manager.remove_session_atomic(&result1.session_id).await;
        assert!(removed1.is_some(), "First session should be removable");
        let (session1, removal_result1) = removed1.unwrap();
        assert_eq!(removal_result1.count, 1); // One session remains

        // Remove second session atomically
        let removed2 = manager.remove_session_atomic(&result2.session_id).await;
        assert!(removed2.is_some(), "Second session should be removable");
        let (session2, removal_result2) = removed2.unwrap();
        assert_eq!(removal_result2.count, 0); // No sessions remain

        // Removing nonexistent session should return None
        let removed_none = manager.remove_session_atomic("nonexistent").await;
        assert!(removed_none.is_none());

        // Cleanup
        {
            let mut s = session1.lock().await;
            let _ = s.kill();
        }
        {
            let mut s = session2.lock().await;
            let _ = s.kill();
        }
    }
}
