//! PTY session + reader/writer threads.
//!
//! Phase 2 establishes the PTY pair, runs separate reader and writer threads,
//! and surfaces the lifecycle to higher layers via an event channel.
//!
//! Phase 3 replaces the raw byte sink with the parser-driven grid by
//! retargeting the `on_bytes` callback at the parser entry point.

pub mod input;
pub mod ring;

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

use self::ring::RingBuffer;

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
    /// Phase 4-C: when `true`, the reader thread routes incoming PTY bytes
    /// into [`ring`] instead of emitting them as `PtyEvent::Data`. Toggled
    /// by [`set_paused`]; observed by the reader on every chunk.
    paused: Arc<AtomicBool>,
    /// Phase 4-C: drop-oldest ring buffer that absorbs PTY output while
    /// `paused` is true. Drained by the app layer on detach so the data
    /// can be replayed into `term_core`.
    ring: Arc<Mutex<RingBuffer>>,
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

        let mut cmd = CommandBuilder::new(shell);
        // Strip multiplexer-injected env vars: the shell we spawn here is a
        // fresh child of native-poc, not of any outer mux/tmux session. Leaving
        // these set would (a) make `emterm mux` refuse to start with
        // "Cannot nest mux sessions (EMTERM_MUX is set)", and (b) make tmux-
        // aware CLI commands wrap responses in DCS passthrough when they
        // shouldn't.
        cmd.env_remove("EMTERM_MUX");
        cmd.env_remove("EMTERM_MUX_SOCKET");
        cmd.env_remove("TMUX");
        cmd.env_remove("TMUX_PANE");
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

        let paused = Arc::new(AtomicBool::new(false));
        let ring = Arc::new(Mutex::new(RingBuffer::default()));

        let reader_event_tx = event_tx.clone();
        let reader_paused = paused.clone();
        let reader_ring = ring.clone();
        let reader_join = std::thread::Builder::new()
            .name("native-poc-pty-reader".into())
            .spawn(move || {
                reader_loop(&mut *reader, reader_event_tx, reader_paused, reader_ring);
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
            paused,
            ring,
        })
    }

    /// Flip the pause flag. When `true`, the reader thread routes incoming
    /// PTY bytes into the per-session ring buffer instead of emitting them
    /// as `PtyEvent::Data`. The next chunk picks up the new state — there
    /// is no per-byte handshake, so a tiny race (one chunk in flight) is
    /// possible. The caller flips this **before** swapping in the mux
    /// client so any chunk that slips through still ends up in the buffer.
    pub fn set_paused(&self, value: bool) {
        self.paused.store(value, Ordering::SeqCst);
    }

    /// Returns the current pause state. Used by tests and by the app layer
    /// to decide whether to drain on detach.
    #[allow(dead_code)] // Reserved for diagnostic UI.
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    /// Drain the ring buffer. Called on detach so the bytes can be replayed
    /// into `term_core` before the reader resumes feeding `PtyEvent::Data`.
    pub fn drain_ring(&self) -> Vec<u8> {
        self.ring.lock().drain()
    }

    /// True if the ring buffer dropped at least one byte since the last
    /// drain. Used by the app layer to surface a transient warning banner
    /// (mux session output exceeded the 256 KiB cache).
    pub fn ring_overflowed(&self) -> bool {
        self.ring.lock().overflowed()
    }

    /// Send bytes to the shell. Drops with a warn log if the queue is full.
    pub fn write(&self, bytes: Vec<u8>) {
        if let Err(e) = self.input_tx.try_send(bytes) {
            log::warn!("pty input queue full or closed: {e}");
        }
    }

    /// Paste-aware write: sanitizes embedded bracketed-paste end markers
    /// and (optionally) wraps the body in `ESC[200~ … ESC[201~` so the
    /// shell can distinguish a paste from typed input. `bracketed`
    /// reflects DECSET 2004 on the active `TerminalCore`.
    pub fn write_paste(&self, text: &str, bracketed: bool) {
        let wrapped = crate::selection::bracketed_paste(text, bracketed);
        self.write(wrapped.into_bytes());
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
        // 1. Kill the child first so the kernel closes the slave side of
        //    the PTY. Reader will then see EOF on `reader.read()`.
        {
            let mut child = self.child.lock();
            let _ = child.kill();
        }

        // 2. Force-close the input channel so `writer_loop`'s `input_rx.recv()`
        //    returns `Err(Disconnected)` and the thread exits. Without this
        //    the `Sender` lived as a struct field through the whole Drop and
        //    the writer thread sat forever on `recv()` — the symptom the
        //    user reported when clicking the X button ("応答なし").
        //
        //    We can't `drop(self.input_tx)` because moving out of `&mut self`
        //    isn't allowed in Drop; `mem::replace` swaps in a freshly-built
        //    Sender backed by an immediately-dropped Receiver, which is
        //    already disconnected, then we drop the real Sender.
        let (dummy_tx, _dummy_rx) = bounded::<Vec<u8>>(1);
        let old_tx = std::mem::replace(&mut self.input_tx, dummy_tx);
        drop(old_tx);

        // 3. Join threads. Order matters: reader is unblocked by step 1
        //    (kernel-side EOF on master read); writer is unblocked by step 2
        //    (channel disconnect).
        if let Some(h) = self.reader_join.take() {
            let _ = h.join();
        }
        if let Some(h) = self.writer_join.take() {
            let _ = h.join();
        }
    }
}

fn reader_loop(
    reader: &mut dyn Read,
    event_tx: Sender<PtyEvent>,
    paused: Arc<AtomicBool>,
    ring: Arc<Mutex<RingBuffer>>,
) {
    // 16KB read buffer (was 8KB): larger reads amortize the per-chunk
    // cost in Tab::pump (process_pty_data + flush_grapheme_buffer +
    // take_mode_actions + lock/unlock) when the producer is bursty.
    let mut buf = [0u8; 16 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => {
                // Defensive: use `try_send` so a full channel during
                // shutdown can't keep this thread alive past `read` EOF.
                // `Receiver` may already be dropped (Tab::drop reorders
                // fields so the events Receiver dies before PtySession);
                // in that case `try_send` returns `Err(Disconnected)`
                // immediately, which we don't care about — we're exiting.
                let _ = event_tx.try_send(PtyEvent::Exited {
                    reason: ExitReason::Eof,
                });
                break;
            }
            Ok(n) => {
                let slice = &buf[..n];
                if paused.load(Ordering::SeqCst) {
                    // Mux mode: route into the ring buffer instead of the
                    // event channel. We do not block the channel side at
                    // all — the app layer drains the ring on detach.
                    ring.lock().push(slice);
                    continue;
                }
                let payload = slice.to_vec();
                if event_tx.send(PtyEvent::Data(payload)).is_err() {
                    // Receiver dropped; nothing to do.
                    break;
                }
                // Pull the main event loop out of WaitUntil so the
                // bytes are drained on the next about_to_wait pass
                // instead of the 16 ms deadline. Critical for IME
                // commit echoes on Wayland — see crate::wakeup.
                crate::wakeup::wake();
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
        let paused = Arc::new(AtomicBool::new(false));
        let ring = Arc::new(Mutex::new(RingBuffer::default()));
        let join = std::thread::spawn(move || {
            reader_loop(&mut *reader, tx, paused, ring);
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

    /// Helper: a `Read` impl that returns a single canned chunk, then waits
    /// for permission to return EOF. Lets the reader-loop test inspect
    /// intermediate state without racing against `Ok(0)`.
    struct ScriptedReader {
        chunks: std::sync::Mutex<std::collections::VecDeque<Vec<u8>>>,
        eof_after: std::sync::Mutex<bool>,
    }

    impl ScriptedReader {
        fn new(chunks: Vec<Vec<u8>>) -> Self {
            Self {
                chunks: std::sync::Mutex::new(chunks.into_iter().collect()),
                eof_after: std::sync::Mutex::new(false),
            }
        }
        fn allow_eof(&self) {
            *self.eof_after.lock().unwrap() = true;
        }
    }

    impl Read for &ScriptedReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            loop {
                let mut chunks = self.chunks.lock().unwrap();
                if let Some(chunk) = chunks.pop_front() {
                    let n = chunk.len().min(buf.len());
                    buf[..n].copy_from_slice(&chunk[..n]);
                    return Ok(n);
                }
                drop(chunks);
                if *self.eof_after.lock().unwrap() {
                    return Ok(0);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    // ── TS-mux-int-4 helper: pause + ring buffer + resume ────────────────

    #[test]
    fn reader_routes_to_channel_when_unpaused() {
        let script = std::sync::Arc::new(ScriptedReader::new(vec![b"hello".to_vec()]));
        let script_for_thread = script.clone();
        let (tx, rx) = crossbeam_channel::bounded::<PtyEvent>(8);
        let paused = Arc::new(AtomicBool::new(false));
        let ring = Arc::new(Mutex::new(RingBuffer::default()));
        let ring_clone = ring.clone();
        let join = std::thread::spawn(move || {
            let mut reader = &*script_for_thread;
            reader_loop(&mut reader, tx, paused, ring_clone);
        });

        let evt = rx.recv_timeout(Duration::from_secs(1)).expect("data event");
        match evt {
            PtyEvent::Data(b) => assert_eq!(b, b"hello".to_vec()),
            other => panic!("expected Data, got {other:?}"),
        }
        // Ring buffer must be untouched in unpaused mode.
        assert!(ring.lock().is_empty());
        script.allow_eof();
        let _ = join.join();
    }

    #[test]
    fn reader_routes_to_ring_buffer_when_paused() {
        let script = std::sync::Arc::new(ScriptedReader::new(vec![b"PAUSED".to_vec()]));
        let script_for_thread = script.clone();
        let (tx, rx) = crossbeam_channel::bounded::<PtyEvent>(8);
        let paused = Arc::new(AtomicBool::new(true));
        let ring = Arc::new(Mutex::new(RingBuffer::default()));
        let ring_clone = ring.clone();
        let join = std::thread::spawn(move || {
            let mut reader = &*script_for_thread;
            reader_loop(&mut reader, tx, paused, ring_clone);
        });

        // Give the reader a moment to consume the chunk.
        std::thread::sleep(Duration::from_millis(50));
        // Channel must be empty (no Data events).
        assert!(rx.try_recv().is_err());
        // Ring buffer must hold the chunk.
        let drained = ring.lock().drain();
        assert_eq!(drained, b"PAUSED".to_vec());

        script.allow_eof();
        let _ = join.join();
    }

    #[test]
    fn pause_flag_change_takes_effect_per_read() {
        // Stronger sequencing than the round-trip test: we start in
        // paused mode, observe chunk1 in the ring, flip to unpaused, then
        // observe chunk2 on the channel. Each chunk is fed only after the
        // previous one was observed, removing any race window.
        let script = std::sync::Arc::new(ScriptedReader::new(vec![b"FIRST".to_vec()]));
        let script_for_thread = script.clone();
        let (tx, rx) = crossbeam_channel::bounded::<PtyEvent>(8);
        let paused = Arc::new(AtomicBool::new(true));
        let paused_clone = paused.clone();
        let ring = Arc::new(Mutex::new(RingBuffer::default()));
        let ring_clone = ring.clone();
        let join = std::thread::spawn(move || {
            let mut reader = &*script_for_thread;
            reader_loop(&mut reader, tx, paused_clone, ring_clone);
        });

        // Wait until the paused chunk is in the ring.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if !ring.lock().is_empty() {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "ring stayed empty");
            std::thread::sleep(Duration::from_millis(10));
        }
        let drained = ring.lock().drain();
        assert_eq!(drained, b"FIRST".to_vec());
        assert!(rx.try_recv().is_err(), "no data on channel while paused");

        // Flip and feed a second chunk via the script's queue.
        paused.store(false, Ordering::SeqCst);
        script.chunks.lock().unwrap().push_back(b"SECOND".to_vec());

        let evt = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("chunk on channel after resume");
        match evt {
            PtyEvent::Data(b) => assert_eq!(b, b"SECOND".to_vec()),
            other => panic!("expected Data, got {other:?}"),
        }

        script.allow_eof();
        let _ = join.join();
    }
}
