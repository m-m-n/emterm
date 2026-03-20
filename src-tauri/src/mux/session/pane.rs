//! Pane state: owns a PTY, reader thread, and bounded channel.

use std::io::Write;
use std::sync::{Arc, Mutex as StdMutex};

use tokio::sync::mpsc;

use crate::mux::ring_buffer::DetachRingBuffer;

/// Pane identifier.
pub type PaneId = u32;

/// PTY output chunk sent from the reader thread to the mux writer.
pub struct PtyOutputChunk {
    pub pane_id: PaneId,
    pub data: Vec<u8>,
}

/// Bounded channel capacity for PTY output per pane.
pub const PTY_CHANNEL_CAPACITY: usize = 256;

/// A single terminal pane with its PTY and communication channels.
pub struct MuxPane {
    pub id: PaneId,
    pub cols: u16,
    pub rows: u16,
    /// Sender for PTY output (reader thread → mux writer).
    pub output_tx: mpsc::Sender<PtyOutputChunk>,
    /// Writer handle for sending input to the PTY.
    writer: Option<Arc<StdMutex<Box<dyn Write + Send>>>>,
    /// Ring buffer for detached PTY output. Lazily allocated on first detach.
    pub ring_buffer: Option<DetachRingBuffer>,
    /// Whether this pane's PTY has exited.
    pub exited: bool,
}

impl MuxPane {
    /// Create a new pane (PTY spawn handled by caller).
    /// Ring buffer is NOT allocated until detach (lazy allocation).
    pub fn new(
        id: PaneId,
        cols: u16,
        rows: u16,
        output_tx: mpsc::Sender<PtyOutputChunk>,
        writer: Box<dyn Write + Send>,
    ) -> Self {
        Self {
            id,
            cols,
            rows,
            output_tx,
            writer: Some(Arc::new(StdMutex::new(writer))),
            ring_buffer: None, // Lazy: allocated on first detach
            exited: false,
        }
    }

    /// Get or create the ring buffer (lazy allocation).
    /// Called when entering detached state to start accumulating PTY output.
    pub fn ensure_ring_buffer(&mut self) -> &mut DetachRingBuffer {
        self.ring_buffer.get_or_insert_with(|| {
            DetachRingBuffer::new(crate::mux::ring_buffer::DEFAULT_RING_CAPACITY)
        })
    }

    /// Write input data to the PTY.
    pub fn write_input(&self, data: &[u8]) -> std::io::Result<()> {
        let writer = self
            .writer
            .as_ref()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "writer closed"))?;
        let mut w = writer
            .lock()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        w.write_all(data)?;
        w.flush()
    }

    /// Mark PTY as exited.
    pub fn mark_exited(&mut self) {
        self.exited = true;
        self.writer = None;
    }
}
