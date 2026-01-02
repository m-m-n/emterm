//! PTY Session implementation.
//!
//! This module provides the `PtySession` struct for managing individual
//! PTY sessions, including process spawning, I/O operations, and lifecycle.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex as StdMutex};

use portable_pty::{CommandBuilder, PtyPair, PtySize, native_pty_system};

use super::{PtyError, SessionId};

/// Represents a single PTY session with a connected shell process.
///
/// A `PtySession` manages the lifecycle of a PTY pair and its associated
/// shell process, providing methods for I/O operations and session control.
pub struct PtySession {
    /// Unique identifier for this session.
    pub id: SessionId,
    /// The PTY pair (master/slave).
    pair: PtyPair,
    /// The spawned child process.
    child: Box<dyn portable_pty::Child + Send + Sync>,
    /// Thread-safe writer handle for sending input to the PTY.
    /// Uses std::sync::Mutex for synchronous write operations.
    writer: Arc<StdMutex<Box<dyn Write + Send>>>,
}

impl PtySession {
    /// Creates a new PTY session with the specified shell and dimensions.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique session identifier
    /// * `shell` - Path to the shell executable
    /// * `cols` - Number of columns for the terminal
    /// * `rows` - Number of rows for the terminal
    ///
    /// # Returns
    ///
    /// A new `PtySession` instance, or a `PtyError` if creation fails.
    ///
    /// # Platform Notes
    ///
    /// - On Unix systems, the shell is spawned as a login shell (`-l` flag)
    /// - On Windows, PowerShell is used without additional flags
    pub fn new(id: SessionId, shell: &str, cols: u16, rows: u16) -> Result<Self, PtyError> {
        let pty_system = native_pty_system();

        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(shell);

        // Set login shell for Unix systems
        #[cfg(unix)]
        cmd.arg("-l");

        let child = pair.slave.spawn_command(cmd)?;
        let writer = Arc::new(StdMutex::new(pair.master.take_writer()?));

        Ok(Self {
            id,
            pair,
            child,
            writer,
        })
    }

    /// Resizes the PTY to the specified dimensions.
    ///
    /// This sends a resize signal (SIGWINCH on Unix) to the shell process.
    ///
    /// # Arguments
    ///
    /// * `cols` - New number of columns
    /// * `rows` - New number of rows
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), PtyError> {
        self.pair.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    /// Writes data to the PTY input.
    ///
    /// This sends the provided bytes to the shell's stdin.
    ///
    /// # Arguments
    ///
    /// * `data` - Bytes to write to the PTY
    pub fn write(&self, data: &[u8]) -> Result<(), PtyError> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|e| PtyError::Pty(format!("Lock poisoned: {}", e)))?;
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    /// Takes ownership of the PTY reader.
    ///
    /// Returns a cloned reader that can be used to read output from the PTY.
    /// This can be called multiple times to get additional readers.
    pub fn take_reader(&self) -> Result<Box<dyn Read + Send>, PtyError> {
        self.pair
            .master
            .try_clone_reader()
            .map_err(|e| PtyError::Io(std::io::Error::other(e.to_string())))
    }

    /// Checks if the child process has exited without blocking.
    ///
    /// # Returns
    ///
    /// * `Ok(Some(status))` - The process has exited with the given status
    /// * `Ok(None)` - The process is still running
    /// * `Err(e)` - An error occurred while checking status
    pub fn try_wait(&mut self) -> Result<Option<portable_pty::ExitStatus>, PtyError> {
        self.child
            .try_wait()
            .map_err(|e| PtyError::Pty(e.to_string()))
    }

    /// Forcefully terminates the child process.
    pub fn kill(&mut self) -> Result<(), PtyError> {
        self.child.kill().map_err(|e| PtyError::Pty(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::{detect_default_shell, generate_session_id};

    #[test]
    fn test_session_creation() {
        let id = generate_session_id();
        let shell = detect_default_shell();
        let result = PtySession::new(id.clone(), &shell, 80, 24);

        assert!(result.is_ok(), "Session creation should succeed");

        let mut session = result.unwrap();
        assert_eq!(session.id, id);

        // Cleanup: kill the process
        let _ = session.kill();
    }

    #[test]
    fn test_session_resize() {
        let id = generate_session_id();
        let shell = detect_default_shell();
        let mut session = PtySession::new(id, &shell, 80, 24).unwrap();

        let resize_result = session.resize(120, 40);
        assert!(resize_result.is_ok(), "Resize should succeed");

        // Cleanup
        let _ = session.kill();
    }

    #[test]
    fn test_session_take_reader() {
        let id = generate_session_id();
        let shell = detect_default_shell();
        let mut session = PtySession::new(id, &shell, 80, 24).unwrap();

        let reader_result = session.take_reader();
        assert!(reader_result.is_ok(), "Taking reader should succeed");

        // Cleanup
        let _ = session.kill();
    }

    #[test]
    fn test_session_kill() {
        let id = generate_session_id();
        let shell = detect_default_shell();
        let mut session = PtySession::new(id, &shell, 80, 24).unwrap();

        let kill_result = session.kill();
        assert!(kill_result.is_ok(), "Kill should succeed");

        // After killing, try_wait should return Some
        std::thread::sleep(std::time::Duration::from_millis(100));
        let status = session.try_wait();
        assert!(status.is_ok());
    }

    #[test]
    fn test_session_write() {
        let id = generate_session_id();
        let shell = detect_default_shell();
        let mut session = PtySession::new(id, &shell, 80, 24).unwrap();

        // Write some data
        let write_result = session.write(b"echo hello\n");
        assert!(write_result.is_ok(), "Write should succeed");

        // Cleanup
        let _ = session.kill();
    }
}
