//! PTY Session implementation.
//!
//! This module provides the `PtySession` struct for managing individual
//! PTY sessions, including process spawning, I/O operations, and lifecycle.

use std::collections::HashMap;
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
    /// When `take_writer_handle()` is called, this becomes None as ownership
    /// is transferred to the dedicated writer thread.
    writer: Option<Arc<StdMutex<Box<dyn Write + Send>>>>,
}

impl PtySession {
    /// Creates a new PTY session with the specified shell and dimensions.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique session identifier
    /// * `shell` - Path to the shell executable
    /// * `args` - Optional arguments to pass to the shell
    /// * `cols` - Number of columns for the terminal
    /// * `rows` - Number of rows for the terminal
    /// * `env_vars` - Optional environment variables to merge into the shell environment
    /// * `working_directory` - Optional working directory for the shell
    ///
    /// # Returns
    ///
    /// A new `PtySession` instance, or a `PtyError` if creation fails.
    ///
    /// # Platform Notes
    ///
    /// - Shells are spawned as non-login shells for faster startup
    /// - TERM and COLORTERM environment variables are set for compatibility
    pub fn new(
        id: SessionId,
        shell: &str,
        args: Option<Vec<String>>,
        cols: u16,
        rows: u16,
        env_vars: Option<HashMap<String, String>>,
        working_directory: Option<String>,
    ) -> Result<Self, PtyError> {
        let pty_system = native_pty_system();

        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(shell);

        // Add shell arguments if provided
        if let Some(ref shell_args) = args {
            for arg in shell_args {
                cmd.arg(arg);
            }
        }

        // Set TERM environment variable for proper terminal emulation
        // This is essential for applications like SSH, vim, htop, etc.
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        // Apply profile-specific environment variables first, then enforce
        // reserved keys so they cannot be overridden by user profiles.
        if let Some(ref vars) = env_vars {
            for (key, value) in vars {
                cmd.env(key, value);
            }
        }

        // Identify eMterm to child processes (de facto standard, used by iTerm2, WezTerm, VS Code)
        // Set AFTER env_vars to prevent profile overrides.
        cmd.env("TERM_PROGRAM", "emterm");
        cmd.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));

        // Clear tmux variables: the shell inside eMterm's PTY is directly
        // connected to eMterm, not to tmux. Inheriting TMUX causes CLI commands
        // (e.g. `emterm image`) to incorrectly apply DCS passthrough wrapping
        // and skip response handling, leading to garbage text on screen.
        cmd.env_remove("TMUX");
        cmd.env_remove("TMUX_PANE");

        // Apply profile-specific working directory
        if let Some(ref dir) = working_directory {
            if !dir.is_empty() {
                let path = std::path::Path::new(dir);
                if path.is_dir() {
                    cmd.cwd(path);
                } else {
                    log::warn!(
                        "Working directory does not exist, falling back to home: {:?}",
                        dir
                    );
                    #[cfg(unix)]
                    if let Ok(home) = std::env::var("HOME") {
                        cmd.cwd(std::path::Path::new(&home));
                    }
                    #[cfg(windows)]
                    if let Ok(home) = std::env::var("USERPROFILE") {
                        cmd.cwd(std::path::Path::new(&home));
                    }
                }
            }
        }

        let child = pair.slave.spawn_command(cmd)?;
        let writer = Arc::new(StdMutex::new(pair.master.take_writer()?));

        Ok(Self {
            id,
            pair,
            child,
            writer: Some(writer),
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
    /// Note: After `take_writer_handle()` is called, this method will return an error.
    /// Use the writer channel instead.
    ///
    /// # Arguments
    ///
    /// * `data` - Bytes to write to the PTY
    pub fn write(&self, data: &[u8]) -> Result<(), PtyError> {
        let writer_arc = self
            .writer
            .as_ref()
            .ok_or_else(|| PtyError::Pty("Writer handle already taken".to_string()))?;
        let mut writer = writer_arc
            .lock()
            .map_err(|e| PtyError::Pty(format!("Lock poisoned: {}", e)))?;
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    /// Takes ownership of the PTY writer handle.
    ///
    /// This extracts the writer so it can be transferred to a dedicated writer
    /// thread. After calling this, `write()` will return an error.
    ///
    /// Returns the inner `Box<dyn Write + Send>` if the writer is still present,
    /// consuming the `Arc<StdMutex<...>>` wrapper.
    pub fn take_writer_handle(&mut self) -> Option<Box<dyn Write + Send>> {
        let writer_arc = self.writer.take()?;
        // Try to unwrap the Arc. If there are other references, this will fail.
        match Arc::try_unwrap(writer_arc) {
            Ok(mutex) => mutex.into_inner().ok(),
            Err(_arc) => {
                // Should not happen in normal use - the Arc is only held here
                log::warn!("take_writer_handle: Arc has multiple references");
                None
            }
        }
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

    /// Gets the raw file descriptor of the PTY master.
    /// This is used to set non-blocking mode on Unix systems.
    #[cfg(unix)]
    pub fn master_fd(&self) -> Option<std::os::unix::io::RawFd> {
        self.pair.master.as_raw_fd()
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
        let result = PtySession::new(id.clone(), &shell, None, 80, 24, None, None);

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
        let mut session = PtySession::new(id, &shell, None, 80, 24, None, None).unwrap();

        let resize_result = session.resize(120, 40);
        assert!(resize_result.is_ok(), "Resize should succeed");

        // Cleanup
        let _ = session.kill();
    }

    #[test]
    fn test_session_take_reader() {
        let id = generate_session_id();
        let shell = detect_default_shell();
        let mut session = PtySession::new(id, &shell, None, 80, 24, None, None).unwrap();

        let reader_result = session.take_reader();
        assert!(reader_result.is_ok(), "Taking reader should succeed");

        // Cleanup
        let _ = session.kill();
    }

    #[test]
    fn test_session_kill() {
        let id = generate_session_id();
        let shell = detect_default_shell();
        let mut session = PtySession::new(id, &shell, None, 80, 24, None, None).unwrap();

        let kill_result = session.kill();
        assert!(kill_result.is_ok(), "Kill should succeed");

        // After killing, try_wait should return Some
        std::thread::sleep(std::time::Duration::from_millis(100));
        let status = session.try_wait();
        assert!(status.is_ok());
    }

    #[test]
    #[ignore = "portable-pty try_wait() does not reliably detect shell exit"]
    fn test_session_exit_detection() {
        use std::io::Read;

        let id = generate_session_id();
        let shell = detect_default_shell();
        let mut session = PtySession::new(id, &shell, None, 80, 24, None, None).unwrap();

        // Get a reader to drain output
        let mut reader = session.take_reader().unwrap();

        // Set non-blocking mode on reader
        #[cfg(unix)]
        if let Some(fd) = session.master_fd() {
            unsafe {
                let flags = libc::fcntl(fd, libc::F_GETFL);
                libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
        }

        // Write exit command, then send EOF (Ctrl+D)
        session.write(b"exit\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        // Send Ctrl+D (EOF) - this forces shell to exit on systems where exit alone doesn't work
        session.write(&[0x04]).unwrap();

        // Wait for shell to exit and drain output
        let mut buf = [0u8; 1024];
        let mut last_read_time = std::time::Instant::now();
        for i in 0..50 {
            // Drain any output (non-blocking)
            match reader.read(&mut buf) {
                Ok(n) if n > 0 => {
                    eprintln!(
                        "Read {} bytes: {:?}",
                        n,
                        String::from_utf8_lossy(&buf[..n.min(100)])
                    );
                    last_read_time = std::time::Instant::now();
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No data available
                }
                Err(e) => {
                    eprintln!("Read error: {}", e);
                }
                _ => {}
            }

            match session.try_wait() {
                Ok(Some(status)) => {
                    eprintln!("Shell exited with code: {}", status.exit_code());
                    return; // Test passed
                }
                Ok(None) => {
                    // If no data for 500ms after exit command, shell might be done
                    if i >= 5 && last_read_time.elapsed() > std::time::Duration::from_millis(500) {
                        eprintln!("No new data for 500ms, checking if shell exited...");
                    }
                    if i % 10 == 0 {
                        eprintln!("Shell still running (attempt {})...", i);
                    }
                }
                Err(e) => {
                    panic!("try_wait error: {}", e);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        // If we get here, try_wait didn't detect exit - this is the bug
        eprintln!("try_wait() didn't detect exit - this is a portable_pty issue");
        panic!("Shell did not exit within timeout (try_wait bug)");
    }

    #[test]
    fn test_session_write() {
        let id = generate_session_id();
        let shell = detect_default_shell();
        let mut session = PtySession::new(id, &shell, None, 80, 24, None, None).unwrap();

        // Write some data
        let write_result = session.write(b"echo hello\n");
        assert!(write_result.is_ok(), "Write should succeed");

        // Cleanup
        let _ = session.kill();
    }

    #[test]
    fn test_session_sets_term_program_env() {
        // TERM_PROGRAM=emterm and TERM_PROGRAM_VERSION should be set for all PTY sessions
        // We verify by spawning a shell that prints the env var
        let id = generate_session_id();
        let shell = detect_default_shell();
        let mut session = PtySession::new(id, &shell, None, 80, 24, None, None).unwrap();

        // Write command to print TERM_PROGRAM
        session.write(b"echo TERM_PROGRAM=$TERM_PROGRAM\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        // Read output
        let mut reader = session.take_reader().unwrap();
        #[cfg(unix)]
        if let Some(fd) = session.master_fd() {
            unsafe {
                let flags = libc::fcntl(fd, libc::F_GETFL);
                libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
        }
        let mut buf = [0u8; 4096];
        std::thread::sleep(std::time::Duration::from_millis(200));
        let mut output = String::new();
        loop {
            match std::io::Read::read(&mut reader, &mut buf) {
                Ok(0) => break,
                Ok(n) => output.push_str(&String::from_utf8_lossy(&buf[..n])),
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
        assert!(
            output.contains("TERM_PROGRAM=emterm"),
            "PTY should have TERM_PROGRAM=emterm set. Output: {}",
            output
        );

        let _ = session.kill();
    }

    #[test]
    fn test_session_creation_with_env_vars() {
        let id = generate_session_id();
        let shell = detect_default_shell();
        let mut env = HashMap::new();
        env.insert("MY_TEST_VAR".to_string(), "hello".to_string());
        env.insert("ANOTHER_VAR".to_string(), "world".to_string());

        let result = PtySession::new(id, &shell, None, 80, 24, Some(env), None);
        assert!(
            result.is_ok(),
            "Session creation with env vars should succeed"
        );

        let mut session = result.unwrap();
        let _ = session.kill();
    }

    #[test]
    fn test_session_creation_with_working_directory() {
        let id = generate_session_id();
        let shell = detect_default_shell();

        let result = PtySession::new(id, &shell, None, 80, 24, None, Some("/tmp".to_string()));
        assert!(
            result.is_ok(),
            "Session creation with valid working directory should succeed"
        );

        let mut session = result.unwrap();
        let _ = session.kill();
    }

    #[test]
    fn test_session_creation_with_invalid_working_directory() {
        let id = generate_session_id();
        let shell = detect_default_shell();

        // Non-existent directory should fall back to default (not error)
        let result = PtySession::new(
            id,
            &shell,
            None,
            80,
            24,
            None,
            Some("/nonexistent/path/that/does/not/exist".to_string()),
        );
        assert!(
            result.is_ok(),
            "Session creation with invalid working directory should succeed (fallback)"
        );

        let mut session = result.unwrap();
        let _ = session.kill();
    }

    #[test]
    fn test_session_creation_with_empty_working_directory() {
        let id = generate_session_id();
        let shell = detect_default_shell();

        let result = PtySession::new(id, &shell, None, 80, 24, None, Some(String::new()));
        assert!(
            result.is_ok(),
            "Session creation with empty working directory should succeed"
        );

        let mut session = result.unwrap();
        let _ = session.kill();
    }

    #[test]
    fn test_session_creation_with_all_profile_options() {
        let id = generate_session_id();
        let shell = detect_default_shell();
        let mut env = HashMap::new();
        env.insert("NODE_ENV".to_string(), "development".to_string());

        let result = PtySession::new(
            id,
            &shell,
            Some(vec![]),
            80,
            24,
            Some(env),
            Some("/tmp".to_string()),
        );
        assert!(
            result.is_ok(),
            "Session creation with all profile options should succeed"
        );

        let mut session = result.unwrap();
        let _ = session.kill();
    }
}
