//! Graceful shutdown mechanism for PTY sessions.
//!
//! This module provides a 3-stage graceful shutdown sequence for PTY sessions:
//! 1. Send "exit\n" command and wait (Stage 1: 5 seconds)
//! 2. Send EOF (Ctrl+D) and wait (Stage 2: 2 seconds)
//! 3. Force kill the process (Stage 3)

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

use super::{PtyManager, PtySession};

/// Default timeout for Stage 1 (exit command).
const DEFAULT_STAGE1_TIMEOUT: Duration = Duration::from_secs(5);

/// Default timeout for Stage 2 (EOF).
const DEFAULT_STAGE2_TIMEOUT: Duration = Duration::from_secs(2);

/// Configuration for graceful shutdown timeouts.
#[derive(Clone, Copy, Debug)]
pub struct ShutdownConfig {
    /// Timeout for Stage 1 (exit command).
    pub stage1_timeout: Duration,
    /// Timeout for Stage 2 (EOF).
    pub stage2_timeout: Duration,
}

impl Default for ShutdownConfig {
    fn default() -> Self {
        Self {
            stage1_timeout: DEFAULT_STAGE1_TIMEOUT,
            stage2_timeout: DEFAULT_STAGE2_TIMEOUT,
        }
    }
}

impl ShutdownConfig {
    /// Creates a new ShutdownConfig from a total timeout in milliseconds.
    ///
    /// Distributes the timeout proportionally: 5/7 for Stage 1, 2/7 for Stage 2.
    /// Minimum Stage 1: 1 second, Minimum Stage 2: 500ms.
    pub fn from_total_ms(total_ms: u64) -> Self {
        if total_ms == 0 {
            return Self::default();
        }

        // Distribute: 5/7 for stage 1, 2/7 for stage 2
        let stage1_ms = (total_ms * 5 / 7).max(1000);
        let stage2_ms = (total_ms * 2 / 7).max(500);

        Self {
            stage1_timeout: Duration::from_millis(stage1_ms),
            stage2_timeout: Duration::from_millis(stage2_ms),
        }
    }
}

/// Executes a 3-stage graceful shutdown sequence for a PTY session with default timeouts.
///
/// This is a convenience wrapper around `shutdown_with_config` using default timeouts.
///
/// # Arguments
///
/// * `pty_manager` - The PTY manager instance
/// * `session_id` - The session ID to shut down
///
/// # Returns
///
/// `Ok(())` if shutdown completed successfully, or an error message.
pub async fn shutdown(pty_manager: &PtyManager, session_id: &str) -> Result<(), String> {
    shutdown_with_config(pty_manager, session_id, ShutdownConfig::default()).await
}

/// Executes a 3-stage graceful shutdown sequence for a PTY session with custom timeouts.
///
/// # Stages
///
/// 1. **Stage 1**: Send `"exit\n"` command and wait for configured timeout
/// 2. **Stage 2**: Send EOF (`0x04`) and wait for configured timeout
/// 3. **Stage 3**: Force kill the process
///
/// # Arguments
///
/// * `pty_manager` - The PTY manager instance
/// * `session_id` - The session ID to shut down
/// * `config` - Timeout configuration
///
/// # Returns
///
/// `Ok(())` if shutdown completed successfully, or an error message.
pub async fn shutdown_with_config(
    pty_manager: &PtyManager,
    session_id: &str,
    config: ShutdownConfig,
) -> Result<(), String> {
    let session = pty_manager
        .get_session(session_id)
        .await
        .ok_or("Session not found")?;

    let registry = pty_manager.writer_registry();

    // Stage 1: Send exit command via writer channel
    eprintln!(
        "Graceful shutdown stage 1: sending 'exit' command (timeout: {:?})",
        config.stage1_timeout
    );
    registry
        .send(session_id, b"exit\n".to_vec())
        .map_err(|e| e.to_string())?;

    if wait_for_exit(&session, config.stage1_timeout).await {
        eprintln!("Graceful shutdown: process exited in stage 1");
        return Ok(());
    }

    // Stage 2: Send EOF via writer channel
    eprintln!(
        "Graceful shutdown stage 2: sending EOF (timeout: {:?})",
        config.stage2_timeout
    );
    registry
        .send(session_id, vec![0x04])
        .map_err(|e| e.to_string())?;

    if wait_for_exit(&session, config.stage2_timeout).await {
        eprintln!("Graceful shutdown: process exited in stage 2");
        return Ok(());
    }

    // Stage 3: Force kill
    eprintln!("Graceful shutdown stage 3: force killing process");
    {
        let mut session = session.lock().await;
        session.kill().map_err(|e| e.to_string())?;
    }

    eprintln!("Graceful shutdown: process killed in stage 3");
    Ok(())
}

/// Waits for a PTY session to exit within the specified timeout.
///
/// Polls the process status every 100ms until it exits or the timeout is reached.
///
/// # Arguments
///
/// * `session` - The PTY session to monitor
/// * `timeout_duration` - Maximum time to wait
///
/// # Returns
///
/// `true` if the process exited within the timeout, `false` otherwise.
async fn wait_for_exit(session: &Arc<Mutex<PtySession>>, timeout_duration: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout_duration {
        let mut s = session.lock().await;
        match s.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => {
                // Process still running, continue waiting
            }
            Err(e) => {
                eprintln!("wait_for_exit: try_wait error: {}", e);
                // Treat error as terminal condition
                return false;
            }
        }
        drop(s);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_shutdown_stage1_success() {
        // Create a manager and session (atomic sets up writer channel)
        let manager = PtyManager::new();
        let result = manager
            .create_session_atomic(None, None, 80, 24)
            .await
            .unwrap();
        let session_id = result.session_id;

        // Normal shell should exit on "exit" command (Stage 1)
        let result = shutdown(&manager, &session_id).await;
        assert!(result.is_ok(), "Shutdown should succeed");

        // Session should be gone after exit
        // (The reader thread will remove it automatically)
    }

    #[tokio::test]
    async fn test_shutdown_nonexistent_session() {
        let manager = PtyManager::new();

        // Try to shutdown a non-existent session
        let result = shutdown(&manager, "nonexistent-id").await;
        assert!(result.is_err(), "Should fail for non-existent session");
        assert_eq!(result.unwrap_err(), "Session not found");
    }

    #[tokio::test]
    async fn test_shutdown_stage3_force_kill() {
        // Create a manager and session with a long-running command (atomic sets up writer channel)
        let manager = PtyManager::new();
        let created = manager
            .create_session_atomic(None, None, 80, 24)
            .await
            .unwrap();
        let session_id = created.session_id;

        // Send a command that won't exit on "exit" or EOF via writer channel
        let registry = manager.writer_registry();
        let _ = registry.send(&session_id, b"sleep 999 & disown\n".to_vec());
        // Wait a bit for the command to start
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Shutdown should eventually kill it in stage 3
        let result = shutdown(&manager, &session_id).await;
        assert!(result.is_ok(), "Shutdown should eventually succeed");

        // Cleanup
        if let Some(session) = manager.remove_session(&session_id).await {
            let mut s = session.lock().await;
            let _ = s.kill();
        }
    }

    #[tokio::test]
    async fn test_wait_for_exit_timeout() {
        let manager = PtyManager::new();
        let created = manager
            .create_session_atomic(None, None, 80, 24)
            .await
            .unwrap();
        let session_id = created.session_id;

        let session = manager.get_session(&session_id).await.unwrap();

        // Send a long-running command via writer channel
        let _ = manager
            .writer_registry()
            .send(&session_id, b"sleep 10\n".to_vec());

        // Wait for exit with very short timeout
        let exited = wait_for_exit(&session, Duration::from_millis(100)).await;
        assert!(!exited, "Should timeout waiting for sleep command");

        // Cleanup
        if let Some(session) = manager.remove_session(&session_id).await {
            let mut s = session.lock().await;
            let _ = s.kill();
        }
    }

    #[test]
    fn test_shutdown_config_default() {
        let config = ShutdownConfig::default();
        assert_eq!(config.stage1_timeout, Duration::from_secs(5));
        assert_eq!(config.stage2_timeout, Duration::from_secs(2));
    }

    #[test]
    fn test_shutdown_config_from_total_ms() {
        // 7 seconds (same as default total)
        let config = ShutdownConfig::from_total_ms(7000);
        assert_eq!(config.stage1_timeout, Duration::from_millis(5000));
        assert_eq!(config.stage2_timeout, Duration::from_millis(2000));

        // 14 seconds
        let config = ShutdownConfig::from_total_ms(14000);
        assert_eq!(config.stage1_timeout, Duration::from_millis(10000));
        assert_eq!(config.stage2_timeout, Duration::from_millis(4000));

        // Very short timeout (should use minimums)
        let config = ShutdownConfig::from_total_ms(500);
        assert_eq!(config.stage1_timeout, Duration::from_millis(1000)); // min 1s
        assert_eq!(config.stage2_timeout, Duration::from_millis(500)); // min 500ms

        // Zero timeout should use defaults
        let config = ShutdownConfig::from_total_ms(0);
        assert_eq!(config.stage1_timeout, Duration::from_secs(5));
        assert_eq!(config.stage2_timeout, Duration::from_secs(2));
    }

    #[tokio::test]
    async fn test_shutdown_with_custom_config() {
        let manager = PtyManager::new();
        let created = manager
            .create_session_atomic(None, None, 80, 24)
            .await
            .unwrap();
        let session_id = created.session_id;

        // Use custom short timeouts
        let config = ShutdownConfig {
            stage1_timeout: Duration::from_millis(1000),
            stage2_timeout: Duration::from_millis(500),
        };

        let result = shutdown_with_config(&manager, &session_id, config).await;
        assert!(result.is_ok(), "Shutdown with custom config should succeed");
    }
}
