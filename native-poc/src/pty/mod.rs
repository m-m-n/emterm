//! PTY session + reader/writer threads.
//!
//! Phase 2 establishes the PTY pair, runs separate reader and writer threads,
//! and surfaces the lifecycle to higher layers via an event channel.
//!
//! Phase 3 replaces the raw byte sink with the parser-driven grid by
//! retargeting the `on_bytes` callback at the parser entry point.

pub mod input;

use std::io::{Read, Write};
use std::sync::Arc;
use std::thread::JoinHandle;

use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

/// Events emitted from a PTY session up to the App.
#[derive(Debug)]
pub enum PtyEvent {
    /// New bytes arrived. The receiver decides how to drive the parser /
    /// raw sink. Phase 2 dumps these into `Tab::raw_buffer` directly.
    Data(Vec<u8>),
    /// PTY EOF or unrecoverable read error.
    Exited { reason: ExitReason },
}

#[derive(Debug)]
pub enum ExitReason {
    /// Child process exited cleanly (status code if known).
    Eof,
    /// Read error (e.g. PTY descriptor closed unexpectedly).
    ReadError(String),
}

/// Owned PTY session. Dropping this triggers teardown: pending input is
/// discarded, the writer thread observes the closed channel and exits, and
/// the master PTY is closed which causes the reader thread to see EOF.
pub struct PtySession {
    /// Outbound queue consumed by the writer thread.
    input_tx: Sender<Vec<u8>>,
    /// Resize handle for the PTY master. Wrapped so the App can call
    /// `resize` without owning the session mutably.
    master: Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
    /// Child process handle for SIGHUP on teardown.
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    reader_join: Option<JoinHandle<()>>,
    writer_join: Option<JoinHandle<()>>,
}

impl PtySession {
    /// Spawn a shell. `$SHELL` is preferred; falls back to `/bin/sh`.
    /// `event_tx` receives `PtyEvent` items.
    pub fn spawn(cols: u16, rows: u16, event_tx: Sender<PtyEvent>) -> std::io::Result<Self> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| {
            // Phase 6 NFR: Linux-only PoC, but keep the fallback explicit.
            "/bin/sh".to_string()
        });

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        let cmd = CommandBuilder::new(shell);
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        // The slave handle is no longer needed; dropping it lets the child
        // own its own end.
        drop(pair.slave);

        let master = pair.master;
        let mut reader = master
            .try_clone_reader()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let mut writer = master
            .take_writer()
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        let (input_tx, input_rx) = bounded::<Vec<u8>>(256);

        let reader_event_tx = event_tx.clone();
        let reader_join = std::thread::Builder::new()
            .name("native-poc-pty-reader".into())
            .spawn(move || {
                reader_loop(&mut *reader, reader_event_tx);
            })
            .expect("spawn reader thread");

        let writer_join = std::thread::Builder::new()
            .name("native-poc-pty-writer".into())
            .spawn(move || {
                writer_loop(&mut *writer, input_rx);
            })
            .expect("spawn writer thread");

        Ok(Self {
            input_tx,
            master: Arc::new(Mutex::new(master)),
            child: Arc::new(Mutex::new(child)),
            reader_join: Some(reader_join),
            writer_join: Some(writer_join),
        })
    }

    /// Send bytes to the shell. Drops with a warn log if the queue is full.
    pub fn write(&self, bytes: Vec<u8>) {
        if let Err(e) = self.input_tx.try_send(bytes) {
            log::warn!("pty input queue full or closed: {e}");
        }
    }

    /// Update the PTY size. Called on window resize.
    pub fn resize(&self, cols: u16, rows: u16) {
        let master = self.master.lock();
        if let Err(e) = master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }) {
            log::warn!("pty resize failed: {e}");
        }
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        // Closing the input channel signals the writer thread to exit.
        // The reader thread observes EOF when the child exits or when we
        // drop the master. We send SIGHUP to the child best-effort.
        {
            let mut child = self.child.lock();
            let _ = child.kill();
        }
        // Wait briefly for threads; ignore errors so Drop never panics.
        if let Some(h) = self.reader_join.take() {
            let _ = h.join();
        }
        if let Some(h) = self.writer_join.take() {
            let _ = h.join();
        }
    }
}

fn reader_loop(reader: &mut dyn Read, event_tx: Sender<PtyEvent>) {
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => {
                let _ = event_tx.send(PtyEvent::Exited {
                    reason: ExitReason::Eof,
                });
                break;
            }
            Ok(n) => {
                let payload = buf[..n].to_vec();
                if event_tx.send(PtyEvent::Data(payload)).is_err() {
                    // Receiver dropped; nothing to do.
                    break;
                }
            }
            Err(e) => {
                let _ = event_tx.send(PtyEvent::Exited {
                    reason: ExitReason::ReadError(e.to_string()),
                });
                break;
            }
        }
    }
}

fn writer_loop(writer: &mut dyn Write, rx: Receiver<Vec<u8>>) {
    while let Ok(bytes) = rx.recv() {
        if let Err(e) = writer.write_all(&bytes) {
            log::warn!("pty writer error: {e}");
            break;
        }
        // Flush so single-byte typing is delivered immediately.
        if let Err(e) = writer.flush() {
            log::warn!("pty writer flush error: {e}");
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Spawn a real PTY running `/bin/sh -c 'echo hello'` and verify that the
    /// `hello` bytes show up in the data stream. This is an integration test
    /// that touches the OS; skipped automatically on non-unix.
    #[test]
    #[cfg(unix)]
    fn pty_round_trip_echo_hello() {
        // Override SHELL so we run a deterministic short-lived command.
        // We achieve this by setting argv via CommandBuilder. Since our
        // spawn API does not take a command yet, this test uses portable-pty
        // directly to confirm the reader thread shape works end-to-end.
        use portable_pty::{native_pty_system, CommandBuilder, PtySize};

        let pty = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.args(["-c", "echo hello"]);
        let mut child = pty.slave.spawn_command(cmd).unwrap();
        drop(pty.slave);

        let mut reader = pty.master.try_clone_reader().unwrap();
        let (tx, rx) = crossbeam_channel::bounded::<PtyEvent>(64);
        let join = std::thread::spawn(move || {
            reader_loop(&mut *reader, tx);
        });

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut collected = Vec::<u8>::new();
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(PtyEvent::Data(b)) => collected.extend_from_slice(&b),
                Ok(PtyEvent::Exited { .. }) => break,
                Err(_) => {
                    if let Ok(Some(_)) = child.try_wait() {
                        // After child exited, give the reader a moment to
                        // drain remaining bytes.
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        }
        // Best-effort: kill if still alive.
        let _ = child.kill();
        let _ = join.join();

        let text = String::from_utf8_lossy(&collected);
        assert!(
            text.contains("hello"),
            "expected 'hello' in pty output, got: {text:?}"
        );
    }
}
