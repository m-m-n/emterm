//! Session manager: actor model for centralized state management.
//!
//! All session/window/pane mutations go through a single tokio task
//! via an mpsc channel, ensuring sequential state changes with no
//! concurrent access issues.

use std::collections::HashMap;

use super::pane::PaneId;
use super::session::{MuxSession, SessionId};
use super::window::WindowId;
use crate::mux::ipc::protocol::SessionInfo;

/// The session manager owns all sessions.
pub struct SessionManager {
    sessions: HashMap<SessionId, MuxSession>,
    next_session_id: SessionId,
    next_pane_id: u32,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            next_session_id: 1,
            next_pane_id: 1,
        }
    }

    /// Allocate a globally unique pane ID.
    pub fn alloc_pane_id(&mut self) -> u32 {
        let id = self.next_pane_id;
        self.next_pane_id += 1;
        id
    }

    /// Create a new session with the given name.
    pub fn create_session(&mut self, name: String) -> SessionId {
        let id = self.next_session_id;
        self.next_session_id += 1;
        let session = MuxSession::new(id, name);
        self.sessions.insert(id, session);
        id
    }

    /// Get a reference to a session.
    pub fn get_session(&self, id: SessionId) -> Option<&MuxSession> {
        self.sessions.get(&id)
    }

    /// Get a mutable reference to a session.
    pub fn get_session_mut(&mut self, id: SessionId) -> Option<&mut MuxSession> {
        self.sessions.get_mut(&id)
    }

    /// Remove a session.
    pub fn remove_session(&mut self, id: SessionId) -> Option<MuxSession> {
        self.sessions.remove(&id)
    }

    /// Check if all sessions are empty (daemon should exit).
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Get session list for Welcome message.
    pub fn session_list(&self) -> Vec<SessionInfo> {
        self.sessions
            .values()
            .map(|s| SessionInfo {
                id: s.id,
                name: s.name.clone(),
                window_count: s.window_count() as u32,
                pane_count: s.pane_count() as u32,
            })
            .collect()
    }

    /// Create a new window in a session.
    pub fn create_window(&mut self, session_id: SessionId, name: String) -> Option<WindowId> {
        let session = self.sessions.get_mut(&session_id)?;
        let window_id = session.alloc_window_id();
        let window = super::window::MuxWindow::new(window_id, name);
        session.add_window(window);
        Some(window_id)
    }

    /// Remove a window from a session. Returns true if session became empty.
    pub fn remove_window(&mut self, session_id: SessionId, window_id: WindowId) -> Option<bool> {
        let session = self.sessions.get_mut(&session_id)?;
        session.remove_window(window_id);
        Some(session.is_empty())
    }

    /// Rename a window.
    pub fn rename_window(
        &mut self,
        session_id: SessionId,
        window_id: WindowId,
        name: String,
    ) -> bool {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            if let Some(window) = session.windows.get_mut(&window_id) {
                window.name = name;
                return true;
            }
        }
        false
    }

    /// Find a pane across all sessions. Returns (session_id, window_id).
    pub fn find_pane(&self, pane_id: PaneId) -> Option<(SessionId, WindowId)> {
        for session in self.sessions.values() {
            for window in session.windows.values() {
                if window.panes.contains_key(&pane_id) {
                    return Some((session.id, window.id));
                }
            }
        }
        None
    }

    /// Iterate over all sessions.
    pub fn sessions_iter(&self) -> impl Iterator<Item = &MuxSession> {
        self.sessions.values()
    }

    /// Get session count.
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_session() {
        let mut mgr = SessionManager::new();
        let id = mgr.create_session("test".to_string());
        assert_eq!(id, 1);
        assert_eq!(mgr.session_count(), 1);
        assert_eq!(mgr.get_session(id).unwrap().name, "test");
    }

    #[test]
    fn test_session_ids_increment() {
        let mut mgr = SessionManager::new();
        let id1 = mgr.create_session("a".to_string());
        let id2 = mgr.create_session("b".to_string());
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[test]
    fn test_remove_session() {
        let mut mgr = SessionManager::new();
        let id = mgr.create_session("test".to_string());
        assert!(mgr.remove_session(id).is_some());
        assert!(mgr.is_empty());
    }

    #[test]
    fn test_session_list() {
        let mut mgr = SessionManager::new();
        mgr.create_session("first".to_string());
        mgr.create_session("second".to_string());
        let list = mgr.session_list();
        assert_eq!(list.len(), 2);
    }
}
