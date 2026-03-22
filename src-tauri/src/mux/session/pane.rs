//! Pane state: owns a PTY, reader thread, and bounded channel.

use std::io::Write;
use std::sync::{Arc, Mutex as StdMutex};

use portable_pty::MasterPty;
use tokio::sync::mpsc;

use crate::mux::ring_buffer::DetachRingBuffer;

/// Pane identifier.
pub type PaneId = u32;

/// Thread-safe shared reference to a shadow VT100 parser.
pub type SharedShadowParser = Arc<StdMutex<vt100::Parser>>;

/// PTY output chunk sent from the reader thread to the mux writer.
pub struct PtyOutputChunk {
    pub pane_id: PaneId,
    pub data: Vec<u8>,
}

/// Bounded channel capacity for PTY output per pane.
pub const PTY_CHANNEL_CAPACITY: usize = 256;

/// Where the PTY reader thread sends output data.
///
/// When a GUI client is connected, output goes directly to the channel.
/// When disconnected, output accumulates in a ring buffer for later replay.
pub enum PaneOutputTarget {
    /// Connected: send output to the GUI via channel.
    Connected(mpsc::Sender<PtyOutputChunk>),
    /// Detached: buffer output in a ring buffer for replay on reattach.
    Detached(DetachRingBuffer),
}

/// Thread-safe shared reference to a pane's output target.
pub type SharedOutputTarget = Arc<StdMutex<PaneOutputTarget>>;

/// A single terminal pane with its PTY and communication channels.
pub struct MuxPane {
    pub id: PaneId,
    pub cols: u16,
    pub rows: u16,
    /// Shared output target (reader thread writes here, connection handler swaps on detach/reattach).
    pub output_target: SharedOutputTarget,
    /// Writer handle for sending input to the PTY.
    writer: Option<Arc<StdMutex<Box<dyn Write + Send>>>>,
    /// Master PTY handle, retained after reader/writer extraction for resize support.
    master: Option<Box<dyn MasterPty + Send>>,
    /// Whether this pane's PTY has exited.
    pub exited: bool,
    /// Shadow VT100 parser for screen state tracking (used for reattach restoration).
    pub shadow_parser: SharedShadowParser,
}

impl MuxPane {
    /// Create a new pane (PTY spawn handled by caller).
    pub fn new(
        id: PaneId,
        cols: u16,
        rows: u16,
        output_target: SharedOutputTarget,
        writer: Box<dyn Write + Send>,
        master: Box<dyn MasterPty + Send>,
    ) -> Self {
        Self {
            id,
            cols,
            rows,
            output_target,
            writer: Some(Arc::new(StdMutex::new(writer))),
            master: Some(master),
            exited: false,
            shadow_parser: Arc::new(StdMutex::new(vt100::Parser::new(rows, cols, 0))),
        }
    }

    /// Write input data to the PTY.
    pub fn write_input(&self, data: &[u8]) -> std::io::Result<()> {
        let writer = self
            .writer
            .as_ref()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "writer closed"))?;
        let mut w = writer
            .lock()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        w.write_all(data)?;
        w.flush()
    }

    /// Resize the PTY to the given dimensions.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), String> {
        let master = self
            .master
            .as_ref()
            .ok_or_else(|| "PTY master closed".to_string())?;
        master
            .resize(portable_pty::PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("PTY resize failed: {}", e))?;
        self.cols = cols;
        self.rows = rows;
        self.shadow_parser.lock().unwrap().set_size(rows, cols);
        Ok(())
    }

    /// Mark PTY as exited.
    pub fn mark_exited(&mut self) {
        self.exited = true;
        self.writer = None;
        self.master = None;
    }

    /// Create a pane without a PTY master, for testing only.
    #[cfg(test)]
    pub fn new_test(id: PaneId, cols: u16, rows: u16, output_target: SharedOutputTarget) -> Self {
        let writer: Box<dyn Write + Send> = Box::new(std::io::sink());
        Self {
            id,
            cols,
            rows,
            output_target,
            writer: Some(Arc::new(StdMutex::new(writer))),
            master: None,
            exited: false,
            shadow_parser: Arc::new(StdMutex::new(vt100::Parser::new(rows, cols, 0))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_output_target() -> SharedOutputTarget {
        let (tx, _rx) = mpsc::channel(1);
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx)))
    }

    #[test]
    fn test_resize_fails_without_master() {
        let target = make_output_target();
        let mut pane = MuxPane::new_test(1, 80, 24, target);
        let result = pane.resize(120, 40);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("PTY master closed"));
        // Dimensions should not change on error
        assert_eq!(pane.cols, 80);
        assert_eq!(pane.rows, 24);
    }

    #[test]
    fn test_mark_exited_clears_writer_and_master() {
        let target = make_output_target();
        let mut pane = MuxPane::new_test(1, 80, 24, target);
        assert!(!pane.exited);

        pane.mark_exited();
        assert!(pane.exited);

        // Writing should fail after exit
        let result = pane.write_input(b"hello");
        assert!(result.is_err());
    }

    #[test]
    fn test_write_input_to_sink() {
        let target = make_output_target();
        let pane = MuxPane::new_test(1, 80, 24, target);
        // sink() writer always succeeds
        assert!(pane.write_input(b"hello world").is_ok());
    }

    #[test]
    fn test_channel_backpressure_full() {
        // Channel capacity 1: second send should fail with Full
        let (tx, _rx) = mpsc::channel::<PtyOutputChunk>(1);
        // First send succeeds
        assert!(
            tx.try_send(PtyOutputChunk {
                pane_id: 1,
                data: vec![1]
            })
            .is_ok()
        );
        // Second send hits backpressure (channel full)
        let result = tx.try_send(PtyOutputChunk {
            pane_id: 1,
            data: vec![2],
        });
        assert!(result.is_err());
        match result {
            Err(mpsc::error::TrySendError::Full(_)) => {} // expected
            _ => panic!("Expected Full error"),
        }
    }

    #[test]
    fn test_channel_closed_detection() {
        let (tx, rx) = mpsc::channel::<PtyOutputChunk>(PTY_CHANNEL_CAPACITY);
        drop(rx); // Close receiver
        let result = tx.try_send(PtyOutputChunk {
            pane_id: 1,
            data: vec![1],
        });
        assert!(result.is_err());
        match result {
            Err(mpsc::error::TrySendError::Closed(_)) => {} // expected
            _ => panic!("Expected Closed error"),
        }
    }

    #[test]
    fn test_bounded_channel_capacity_constant() {
        // Verify the constant is reasonable (not too small, not too large)
        assert!(PTY_CHANNEL_CAPACITY >= 64);
        assert!(PTY_CHANNEL_CAPACITY <= 4096);
    }

    #[cfg(unix)]
    #[test]
    fn test_resize_with_real_pty() {
        let pty_system = portable_pty::native_pty_system();
        let size = portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size).unwrap();
        let writer = pair.master.take_writer().unwrap();

        let target = make_output_target();
        let mut pane = MuxPane::new(1, 80, 24, target, writer, pair.master);

        let result = pane.resize(120, 40);
        assert!(result.is_ok());
        assert_eq!(pane.cols, 120);
        assert_eq!(pane.rows, 40);
    }
}
