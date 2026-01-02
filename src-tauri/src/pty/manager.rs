//! PTY Manager implementation.
//!
//! This module provides the `PtyManager` struct for managing multiple
//! PTY sessions concurrently with thread-safe access.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};

use super::{PtyError, PtySession, SessionId, detect_default_shell, generate_session_id};

/// Manages multiple PTY sessions with thread-safe access.
///
/// `PtyManager` maintains a registry of active PTY sessions and provides
/// methods for creating, accessing, and removing sessions.
#[derive(Clone)]
pub struct PtyManager {
    /// Thread-safe map of session ID to session instance.
    sessions: Arc<RwLock<HashMap<SessionId, Arc<Mutex<PtySession>>>>>,
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
        }
    }

    /// Creates a new PTY session with the specified parameters.
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
    pub async fn create_session(
        &self,
        shell: Option<String>,
        cols: u16,
        rows: u16,
    ) -> Result<SessionId, PtyError> {
        let shell = shell.unwrap_or_else(detect_default_shell);
        let id = generate_session_id();
        let session = PtySession::new(id.clone(), &shell, cols, rows)?;

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
    /// # Arguments
    ///
    /// * `id` - The session ID to remove
    ///
    /// # Returns
    ///
    /// The removed session if found, or `None` if it didn't exist.
    pub async fn remove_session(&self, id: &str) -> Option<Arc<Mutex<PtySession>>> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(id)
    }

    /// Returns the number of active sessions.
    #[allow(dead_code)]
    pub async fn session_count(&self) -> usize {
        let sessions = self.sessions.read().await;
        sessions.len()
    }
}

#[cfg(test)]
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
        let result = manager.create_session(None, 80, 24).await;

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
        let session_id = manager.create_session(None, 80, 24).await.unwrap();

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
        let session_id = manager.create_session(None, 80, 24).await.unwrap();

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

        let id1 = manager.create_session(None, 80, 24).await.unwrap();
        let id2 = manager.create_session(None, 120, 40).await.unwrap();

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
}
