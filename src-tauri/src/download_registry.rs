//! Download file handle registry for streaming file writes.
//!
//! Manages open file handles for download sessions. Each session corresponds
//! to a single file download initiated via OSC sequences. The registry
//! enforces a maximum concurrent session limit and an idle timeout.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

/// Maximum number of concurrent download sessions.
const MAX_SESSIONS: usize = 10;

/// Idle timeout in seconds. Sessions idle longer than this are cleaned up.
const IDLE_TIMEOUT_SECS: u64 = 120;

/// An open file handle with metadata for timeout tracking.
struct OpenFileHandle {
    file: File,
    path: PathBuf,
    last_activity: Instant,
}

/// Registry of active download file handles.
///
/// Thread-safe via internal `Mutex`. Designed for low contention
/// (at most 10 concurrent sessions, infrequent access).
pub struct DownloadRegistry {
    sessions: Mutex<HashMap<String, OpenFileHandle>>,
}

impl Default for DownloadRegistry {
    fn default() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

impl DownloadRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Lock the sessions mutex, recovering from poison if a thread panicked.
    fn lock(&self) -> MutexGuard<'_, HashMap<String, OpenFileHandle>> {
        self.sessions.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Insert a new session. Returns error if max sessions reached.
    pub fn insert(&self, id: String, file: File, path: PathBuf) -> Result<(), String> {
        let mut sessions = self.lock();
        if sessions.len() >= MAX_SESSIONS {
            return Err(format!(
                "Maximum concurrent download sessions ({}) reached",
                MAX_SESSIONS
            ));
        }
        sessions.insert(
            id,
            OpenFileHandle {
                file,
                path,
                last_activity: Instant::now(),
            },
        );
        Ok(())
    }

    /// Write data to a session's file. Updates last-activity timestamp.
    /// On I/O error, closes and deletes the partial file, removes from registry.
    pub fn write(&self, id: &str, data: &[u8]) -> Result<(), String> {
        let mut sessions = self.lock();
        let handle = sessions
            .get_mut(id)
            .ok_or_else(|| format!("Download session not found: {}", id))?;

        match handle.file.write_all(data) {
            Ok(()) => {
                handle.last_activity = Instant::now();
                Ok(())
            }
            Err(e) => {
                let path = handle.path.clone();
                sessions.remove(id);
                if let Err(re) = fs::remove_file(&path) {
                    log::warn!("Failed to delete partial download file {:?}: {}", path, re);
                }
                Err(format!("Write failed (partial file deleted): {}", e))
            }
        }
    }

    /// Finish a session: flush, close, and remove from registry.
    pub fn finish(&self, id: &str) -> Result<PathBuf, String> {
        let mut sessions = self.lock();
        let mut handle = sessions
            .remove(id)
            .ok_or_else(|| format!("Download session not found: {}", id))?;

        handle
            .file
            .flush()
            .map_err(|e| format!("Flush failed: {}", e))?;

        Ok(handle.path)
    }

    /// Cancel a session: close handle, delete partial file, remove from registry.
    pub fn cancel(&self, id: &str) -> Result<(), String> {
        let mut sessions = self.lock();
        if let Some(handle) = sessions.remove(id) {
            drop(handle.file);
            if let Err(e) = fs::remove_file(&handle.path) {
                log::warn!(
                    "Failed to delete partial download file {:?}: {}",
                    handle.path,
                    e
                );
            }
        }
        Ok(())
    }

    /// Remove sessions that have been idle longer than the timeout.
    /// Returns the number of sessions cleaned up.
    pub fn cleanup_expired(&self) -> usize {
        let mut sessions = self.lock();
        let now = Instant::now();
        let timeout = std::time::Duration::from_secs(IDLE_TIMEOUT_SECS);

        let expired: Vec<String> = sessions
            .iter()
            .filter(|(_, h)| now.duration_since(h.last_activity) > timeout)
            .map(|(id, _)| id.clone())
            .collect();

        let count = expired.len();
        for id in expired {
            if let Some(handle) = sessions.remove(&id) {
                drop(handle.file);
                if let Err(e) = fs::remove_file(&handle.path) {
                    log::warn!(
                        "Failed to delete expired download file {:?}: {}",
                        handle.path,
                        e
                    );
                }
            }
        }
        count
    }

    /// Get the number of active sessions.
    pub fn session_count(&self) -> usize {
        self.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use tempfile::TempDir;

    fn create_test_file(dir: &TempDir, name: &str) -> (File, PathBuf) {
        let path = dir.path().join(name);
        let file = File::create(&path).unwrap();
        (file, path)
    }

    #[test]
    fn test_insert_and_session_count() {
        let registry = DownloadRegistry::new();
        let dir = TempDir::new().unwrap();

        let (file, path) = create_test_file(&dir, "test1.bin");
        registry.insert("s1".into(), file, path).unwrap();
        assert_eq!(registry.session_count(), 1);

        let (file, path) = create_test_file(&dir, "test2.bin");
        registry.insert("s2".into(), file, path).unwrap();
        assert_eq!(registry.session_count(), 2);
    }

    #[test]
    fn test_max_sessions_limit() {
        let registry = DownloadRegistry::new();
        let dir = TempDir::new().unwrap();

        for i in 0..MAX_SESSIONS {
            let (file, path) = create_test_file(&dir, &format!("test{}.bin", i));
            registry.insert(format!("s{}", i), file, path).unwrap();
        }

        let (file, path) = create_test_file(&dir, "overflow.bin");
        let result = registry.insert("overflow".into(), file, path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Maximum"));
    }

    #[test]
    fn test_write_and_finish() {
        let registry = DownloadRegistry::new();
        let dir = TempDir::new().unwrap();

        let (file, path) = create_test_file(&dir, "write_test.bin");
        let path_clone = path.clone();
        registry.insert("w1".into(), file, path).unwrap();

        registry.write("w1", b"Hello ").unwrap();
        registry.write("w1", b"World").unwrap();
        let finished_path = registry.finish("w1").unwrap();

        assert_eq!(finished_path, path_clone);
        assert_eq!(registry.session_count(), 0);

        // Verify file content
        let mut content = Vec::new();
        File::open(&finished_path)
            .unwrap()
            .read_to_end(&mut content)
            .unwrap();
        assert_eq!(content, b"Hello World");
    }

    #[test]
    fn test_write_unknown_session() {
        let registry = DownloadRegistry::new();
        let result = registry.write("nonexistent", b"data");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_finish_unknown_session() {
        let registry = DownloadRegistry::new();
        let result = registry.finish("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_cancel_deletes_file() {
        let registry = DownloadRegistry::new();
        let dir = TempDir::new().unwrap();

        let (file, path) = create_test_file(&dir, "cancel_test.bin");
        let path_clone = path.clone();
        registry.insert("c1".into(), file, path).unwrap();
        registry.write("c1", b"partial data").unwrap();

        registry.cancel("c1").unwrap();
        assert_eq!(registry.session_count(), 0);
        assert!(!path_clone.exists(), "Partial file should be deleted");
    }

    #[test]
    fn test_cancel_nonexistent_is_ok() {
        let registry = DownloadRegistry::new();
        assert!(registry.cancel("nonexistent").is_ok());
    }

    #[test]
    fn test_cleanup_expired() {
        let registry = DownloadRegistry::new();
        let dir = TempDir::new().unwrap();

        let (file, path) = create_test_file(&dir, "expired.bin");
        let path_clone = path.clone();
        registry.insert("exp".into(), file, path).unwrap();

        // Manually set last_activity to the past
        {
            let mut sessions = registry.sessions.lock().unwrap();
            let handle = sessions.get_mut("exp").unwrap();
            handle.last_activity =
                Instant::now() - std::time::Duration::from_secs(IDLE_TIMEOUT_SECS + 10);
        }

        let cleaned = registry.cleanup_expired();
        assert_eq!(cleaned, 1);
        assert_eq!(registry.session_count(), 0);
        assert!(!path_clone.exists(), "Expired file should be deleted");
    }

    #[test]
    fn test_cleanup_does_not_remove_active() {
        let registry = DownloadRegistry::new();
        let dir = TempDir::new().unwrap();

        let (file, path) = create_test_file(&dir, "active.bin");
        registry.insert("act".into(), file, path).unwrap();

        let cleaned = registry.cleanup_expired();
        assert_eq!(cleaned, 0);
        assert_eq!(registry.session_count(), 1);
    }
}
