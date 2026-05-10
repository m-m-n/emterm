//! PTY (Pseudo Terminal) module for eMterm.
//!
//! This module provides functionality for creating and managing PTY sessions,
//! enabling bidirectional communication between the terminal emulator and
//! shell processes.
//!
//! # Module Structure
//!
//! - `shell`: Shell detection utilities
//! - `session`: Individual PTY session management
//! - `manager`: Multi-session PTY manager

#[cfg(feature = "gui")]
pub mod backpressure;
#[cfg(feature = "gui")]
pub mod device_query_scanner;
#[cfg(feature = "gui")]
pub mod graceful_shutdown;
#[cfg(feature = "gui")]
pub mod kitty_scanner;
#[cfg(feature = "gui")]
pub mod manager;
pub mod passthrough_scanner;
#[cfg(feature = "gui")]
pub mod session;
#[cfg(feature = "gui")]
pub mod shell;
pub mod visibility;
#[cfg(feature = "gui")]
pub mod writer;

// Re-export commonly used types
#[cfg(feature = "gui")]
pub use backpressure::{BackpressureRegistry, SessionBackpressure};
#[cfg(feature = "gui")]
pub use manager::PtyManager;
#[cfg(feature = "gui")]
pub use session::PtySession;
#[cfg(feature = "gui")]
pub use shell::detect_default_shell;
pub use visibility::{
    RawPassthroughBuffer, HIDDEN_PASSTHROUGH_CAPACITY_MUX, HIDDEN_PASSTHROUGH_CAPACITY_NONMUX,
};
#[cfg(feature = "gui")]
pub use visibility::{SessionVisibilityState, VisibilityRegistry};
#[cfg(feature = "gui")]
pub use writer::WriterRegistry;

use thiserror::Error;
use uuid::Uuid;

/// Unique identifier for PTY sessions.
pub type SessionId = String;

/// Generates a new unique session identifier.
///
/// Uses UUID v4 to ensure uniqueness across all sessions.
///
/// # Returns
///
/// A new unique `SessionId` string.
///
/// # Examples
///
/// ```ignore
/// use app_lib::pty::generate_session_id;
///
/// let id1 = generate_session_id();
/// let id2 = generate_session_id();
/// assert_ne!(id1, id2);
/// ```
pub fn generate_session_id() -> SessionId {
    Uuid::new_v4().to_string()
}

/// Errors that can occur during PTY operations.
#[derive(Error, Debug)]
pub enum PtyError {
    /// Error during PTY creation or initialization.
    #[error("PTY creation failed: {0}")]
    Creation(String),

    /// I/O error during read/write operations.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Session with the specified ID was not found.
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    /// Error from the underlying PTY library.
    #[error("PTY error: {0}")]
    Pty(String),
}

// Implement conversion from anyhow::Error to PtyError
// (portable-pty uses anyhow::Error internally)
impl From<anyhow::Error> for PtyError {
    fn from(err: anyhow::Error) -> Self {
        PtyError::Pty(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_session_id_unique() {
        let id1 = generate_session_id();
        let id2 = generate_session_id();
        assert_ne!(id1, id2, "Session IDs should be unique");
    }

    #[test]
    fn test_generate_session_id_valid_uuid() {
        let id = generate_session_id();
        // UUID v4 format: xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx
        assert_eq!(id.len(), 36, "UUID should be 36 characters");
        assert!(Uuid::parse_str(&id).is_ok(), "Should be a valid UUID");
    }

    #[test]
    fn test_pty_error_display() {
        let err = PtyError::Creation("test error".to_string());
        assert_eq!(format!("{}", err), "PTY creation failed: test error");

        let err = PtyError::SessionNotFound("abc123".to_string());
        assert_eq!(format!("{}", err), "Session not found: abc123");
    }
}
