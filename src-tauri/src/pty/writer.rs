//! PTY Writer Channel module.
//!
//! Provides a channel-based write architecture for PTY sessions.
//! Each session gets a dedicated writer thread that consumes from an MPSC channel,
//! eliminating per-keystroke lock contention on the write path.

use std::collections::HashMap;
use std::io::Write;
use std::sync::RwLock;

use tokio::sync::mpsc;

use super::{PtyError, SessionId};

/// Registry that maps session IDs to write channel senders.
///
/// Uses `std::sync::RwLock` (not tokio) so that the Tauri command handler
/// can be synchronous, avoiding async overhead on the hot path.
pub struct WriterRegistry {
    senders: RwLock<HashMap<SessionId, mpsc::UnboundedSender<Vec<u8>>>>,
}

impl Default for WriterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl WriterRegistry {
    /// Creates a new empty writer registry.
    pub fn new() -> Self {
        Self {
            senders: RwLock::new(HashMap::new()),
        }
    }

    /// Registers a write channel sender for a session.
    ///
    /// # Arguments
    ///
    /// * `session_id` - The session ID to register
    /// * `sender` - The unbounded sender for the write channel
    pub fn register(
        &self,
        session_id: SessionId,
        sender: mpsc::UnboundedSender<Vec<u8>>,
    ) -> Result<(), PtyError> {
        let mut senders = self
            .senders
            .write()
            .map_err(|_| PtyError::Pty("WriterRegistry lock poisoned".into()))?;
        senders.insert(session_id, sender);
        Ok(())
    }

    /// Sends data to a session's write channel.
    ///
    /// This is the hot-path method called on every keystroke.
    /// It acquires a single read lock and performs a lock-free channel send.
    ///
    /// # Arguments
    ///
    /// * `session_id` - The target session ID
    /// * `data` - Bytes to write to the PTY
    pub fn send(&self, session_id: &str, data: Vec<u8>) -> Result<(), PtyError> {
        let senders = self
            .senders
            .read()
            .map_err(|_| PtyError::Pty("WriterRegistry lock poisoned".into()))?;
        let sender = senders
            .get(session_id)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.to_string()))?;
        sender
            .send(data)
            .map_err(|_| PtyError::Pty("Writer channel closed".to_string()))
    }

    /// Removes and returns the sender for a session.
    ///
    /// Dropping the sender causes the writer thread to exit naturally
    /// when it detects the channel is closed.
    ///
    /// # Arguments
    ///
    /// * `session_id` - The session ID to remove
    pub fn remove(
        &self,
        session_id: &str,
    ) -> Result<Option<mpsc::UnboundedSender<Vec<u8>>>, PtyError> {
        let mut senders = self
            .senders
            .write()
            .map_err(|_| PtyError::Pty("WriterRegistry lock poisoned".into()))?;
        Ok(senders.remove(session_id))
    }

    /// Returns the number of registered writers.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        let senders = self.senders.read().expect("WriterRegistry lock poisoned");
        senders.len()
    }
}

/// Spawns a dedicated writer thread for a PTY session.
///
/// The thread owns the PTY writer handle exclusively, eliminating the need
/// for any locks on the write path. It blocks on the channel receiver and
/// writes each received message to the PTY, then flushes.
///
/// The thread exits naturally when the channel is closed (all senders dropped).
///
/// # Arguments
///
/// * `session_id` - Session ID for logging
/// * `writer` - The PTY writer handle (exclusive ownership)
/// * `receiver` - The receiving end of the write channel
pub fn spawn_writer_thread(
    session_id: SessionId,
    mut writer: Box<dyn Write + Send>,
    mut receiver: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    std::thread::spawn(move || {
        log::trace!("PTY writer: starting for session {}", session_id);

        // Use blocking_recv() to wait for data on a std thread.
        // After writing the first message, drain any pending messages
        // via try_recv() before flushing once (batch optimization).
        while let Some(data) = receiver.blocking_recv() {
            if let Err(e) = writer.write_all(&data) {
                log::debug!("PTY writer: write error for session {}: {}", session_id, e);
                break;
            }
            // Drain pending messages to batch writes before flush
            while let Ok(data) = receiver.try_recv() {
                if let Err(e) = writer.write_all(&data) {
                    log::debug!("PTY writer: write error for session {}: {}", session_id, e);
                    // Flush what we have so far and exit
                    let _ = writer.flush();
                    return;
                }
            }
            if let Err(e) = writer.flush() {
                log::debug!("PTY writer: flush error for session {}: {}", session_id, e);
                break;
            }
        }

        log::trace!("PTY writer: exiting for session {}", session_id);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_register_and_send() {
        let registry = WriterRegistry::new();
        let (tx, mut rx) = mpsc::unbounded_channel();

        registry.register("session-1".to_string(), tx).unwrap();

        // Send data through registry
        let result = registry.send("session-1", vec![0x61]); // 'a'
        assert!(result.is_ok());

        // Verify data received
        let received = rx.try_recv();
        assert!(received.is_ok());
        assert_eq!(received.unwrap(), vec![0x61]);
    }

    #[test]
    fn test_registry_send_nonexistent_session() {
        let registry = WriterRegistry::new();

        let result = registry.send("nonexistent", vec![0x61]);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Session not found"),
            "Should return SessionNotFound error"
        );
    }

    #[test]
    fn test_registry_remove() {
        let registry = WriterRegistry::new();
        let (tx, _rx) = mpsc::unbounded_channel();

        registry.register("session-1".to_string(), tx).unwrap();
        assert_eq!(registry.len(), 1);

        let removed = registry.remove("session-1").unwrap();
        assert!(removed.is_some());
        assert_eq!(registry.len(), 0);

        // Send should now fail
        let result = registry.send("session-1", vec![0x61]);
        assert!(result.is_err());
    }

    #[test]
    fn test_registry_remove_nonexistent() {
        let registry = WriterRegistry::new();

        let removed = registry.remove("nonexistent").unwrap();
        assert!(removed.is_none());
    }

    #[test]
    fn test_registry_multiple_sessions() {
        let registry = WriterRegistry::new();
        let (tx1, mut rx1) = mpsc::unbounded_channel();
        let (tx2, mut rx2) = mpsc::unbounded_channel();

        registry.register("session-1".to_string(), tx1).unwrap();
        registry.register("session-2".to_string(), tx2).unwrap();
        assert_eq!(registry.len(), 2);

        // Send to session-1
        registry.send("session-1", vec![0x61]).unwrap();
        // Send to session-2
        registry.send("session-2", vec![0x62]).unwrap();

        assert_eq!(rx1.try_recv().unwrap(), vec![0x61]);
        assert_eq!(rx2.try_recv().unwrap(), vec![0x62]);
    }

    #[test]
    fn test_registry_send_after_receiver_dropped() {
        let registry = WriterRegistry::new();
        let (tx, rx) = mpsc::unbounded_channel::<Vec<u8>>();

        registry.register("session-1".to_string(), tx).unwrap();

        // Drop receiver to close channel
        drop(rx);

        let result = registry.send("session-1", vec![0x61]);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Writer channel closed"),);
    }

    /// Mock writer that records all written data for test verification.
    struct MockWriter {
        data: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    }

    impl MockWriter {
        fn new() -> (Self, std::sync::Arc<std::sync::Mutex<Vec<u8>>>) {
            let data = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            (Self { data: data.clone() }, data)
        }
    }

    impl Write for MockWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.data.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Mock writer that always fails on write.
    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "mock write error",
            ))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_writer_thread_writes_data() {
        let (tx, rx) = mpsc::unbounded_channel();
        let (writer, data) = MockWriter::new();

        spawn_writer_thread("test-session".to_string(), Box::new(writer), rx);

        // Send data
        tx.send(vec![0x68, 0x65, 0x6c, 0x6c, 0x6f]).unwrap(); // "hello"

        // Drop sender to close channel (writer thread will exit)
        drop(tx);

        // Wait for writer thread to process
        std::thread::sleep(std::time::Duration::from_millis(100));

        let written = data.lock().unwrap();
        assert_eq!(&*written, b"hello");
    }

    #[test]
    fn test_writer_thread_preserves_order() {
        let (tx, rx) = mpsc::unbounded_channel();
        let (writer, data) = MockWriter::new();

        spawn_writer_thread("test-session".to_string(), Box::new(writer), rx);

        // Send multiple messages
        tx.send(vec![0x61]).unwrap(); // 'a'
        tx.send(vec![0x62]).unwrap(); // 'b'
        tx.send(vec![0x63]).unwrap(); // 'c'

        drop(tx);
        std::thread::sleep(std::time::Duration::from_millis(100));

        let written = data.lock().unwrap();
        assert_eq!(&*written, b"abc");
    }

    #[test]
    fn test_writer_thread_exits_on_channel_close() {
        let (tx, rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (writer, _data) = MockWriter::new();

        spawn_writer_thread("test-session".to_string(), Box::new(writer), rx);

        // Drop sender immediately - writer thread should exit
        drop(tx);

        // Give thread time to exit
        std::thread::sleep(std::time::Duration::from_millis(100));
        // No assertion needed - thread exits cleanly without panic
    }

    #[test]
    fn test_writer_thread_handles_write_error() {
        let (tx, rx) = mpsc::unbounded_channel();

        spawn_writer_thread("test-session".to_string(), Box::new(FailingWriter), rx);

        // Send data - should cause write error in thread
        let _ = tx.send(vec![0x61]);

        // Give thread time to handle error and exit
        std::thread::sleep(std::time::Duration::from_millis(100));
        // Thread should exit gracefully without panic
    }
}
