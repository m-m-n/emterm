//! PTY session + reader/writer threads.
//!
//! Phase 2 establishes the PTY pair, runs separate reader and writer threads,
//! and surfaces the lifecycle to higher layers via an event channel.
//!
//! Phase 3 replaces the raw byte sink with the parser-driven grid by
//! retargeting the `on_bytes` callback at the parser entry point.

pub mod input;
pub mod passthrough_scanner;
pub mod ring;
pub mod visibility;

use std::io::{Read, Write};
use std::sync::Arc;
#[cfg(windows)]
use std::sync::Weak;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender, bounded};
use parking_lot::Mutex;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

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
///
/// # Platform field split (FR9 / FR10)
///
/// On non-Windows the layout is unchanged from the pre-watcher
/// implementation: a single `Arc<Mutex<Child>>` and an
/// `Option<Arc<Mutex<MasterPty>>>` are held directly by the struct, and
/// the kernel-side EOF on the PTY (driven by `child.kill()` in
/// [`Drop::drop`]) is what wakes the reader. No watcher thread runs.
///
/// On Windows the `Child` itself is moved into a dedicated child-exit
/// watcher thread together with the **strong** master `Arc`. The struct
/// only keeps a `ChildKiller` (cloned at spawn time via
/// `Child::clone_killer()`) plus a `Weak` of the master, so the watcher
/// can drop the master `Arc` from inside `wait()` without contending with
/// the struct for the `Mutex<Child>`. When the watcher's `wait()` returns
/// — either because the shell exited naturally, or because Drop step 1
/// killed it — the master is the sole remaining strong reference and
/// dropping it fires `ClosePseudoConsole`, which is what unblocks the
/// reader's `ReadFile` and triggers the single-shot `PtyEvent::Exited`
/// (FR6 / FR7).
pub struct PtySession {
    /// Outbound queue consumed by the writer thread.
    input_tx: Sender<Vec<u8>>,

    /// Non-Windows: resize handle for the PTY master. Wrapped so the App
    /// can call `resize` without owning the session mutably. `Option` so
    /// `Drop` can `take()` it and run cleanup BEFORE joining the reader
    /// thread. Always `Some` outside `Drop`.
    #[cfg(not(windows))]
    master: Option<Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>>,
    /// Windows: `Weak` to the master `Arc`. The matching strong `Arc` lives
    /// in the watcher thread (see [`Self::spawn`]); when the watcher exits
    /// it drops that strong ref, which fires `ClosePseudoConsole`. The
    /// struct upgrades the `Weak` on each `resize()`; once the watcher has
    /// dropped the master, `upgrade()` returns `None` and we warn-log.
    #[cfg(windows)]
    master: Weak<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,

    /// Non-Windows: child process handle. `Drop` calls `kill()` on it,
    /// which closes the slave PTY and produces the kernel EOF that wakes
    /// the reader (no watcher thread is needed).
    #[cfg(not(windows))]
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    /// Windows: cloned `ChildKiller` retained for the Drop step-1 kill
    /// path. The owning `Child` is moved into the watcher thread at spawn
    /// time so no `Mutex<Child>` is shared between the struct and the
    /// watcher — that is what guarantees Drop can call `kill` even while
    /// the watcher is mid-`wait()` (FR9, "no deadlock between Drop and
    /// watcher").
    #[cfg(windows)]
    child_killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,

    reader_join: Option<JoinHandle<()>>,
    writer_join: Option<JoinHandle<()>>,

    /// Windows: join handle for the child-exit watcher thread. Joined in
    /// `Drop` step 4 so the master `Arc` it holds is guaranteed to have
    /// been dropped (and thus `ClosePseudoConsole` to have fired) before
    /// the reader is joined.
    #[cfg(windows)]
    watcher_join: Option<JoinHandle<()>>,

    /// Phase 4-C: when `true`, the reader thread routes incoming PTY bytes
    /// into [`ring`] instead of emitting them as `PtyEvent::Data`. Toggled
    /// by [`set_paused`]; observed by the reader on every chunk.
    paused: Arc<AtomicBool>,
    /// Phase 4-C: drop-oldest ring buffer that absorbs PTY output while
    /// `paused` is true. Drained by the app layer on detach so the data
    /// can be replayed into `term_core`.
    ring: Arc<Mutex<RingBuffer>>,
}

/// Resolve the Windows PowerShell path from the `SystemRoot` environment
/// variable. Returns an absolute path under `%SystemRoot%\System32\...`
/// when `SystemRoot` is set and non-empty. Returns `Err` when `SystemRoot`
/// is unset or empty so the caller does not silently fall back to a
/// PATH-searched executable.
#[cfg(target_os = "windows")]
fn windows_default_shell_path() -> std::io::Result<String> {
    match std::env::var("SystemRoot") {
        Ok(root) if !root.is_empty() => {
            let mut path = std::path::PathBuf::from(root);
            path.push("System32");
            path.push("WindowsPowerShell");
            path.push("v1.0");
            path.push("powershell.exe");
            Ok(path.to_string_lossy().into_owned())
        }
        _ => Err(std::io::Error::other(
            "SystemRoot is unset; cannot resolve default PowerShell path",
        )),
    }
}

/// Platform-default shell when neither `shell_path` nor `$SHELL` is set.
/// Linux returns `/bin/sh`. Windows resolves an absolute path to
/// `%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe`.
/// Returns `Err` on Windows when `SystemRoot` is unset/empty so the caller
/// does not silently fall back to a PATH-searched executable.
fn default_shell() -> std::io::Result<String> {
    #[cfg(target_os = "windows")]
    {
        windows_default_shell_path()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok("/bin/sh".to_string())
    }
}

impl PtySession {
    /// Spawn a shell. Selection order:
    ///   1. `shell_path` argument when non-empty (from `settings.shell_path`
    ///      or a profile override),
    ///   2. `$SHELL` environment variable,
    ///   3. platform default ([`default_shell`]): `/bin/sh` on Linux,
    ///      `powershell.exe` on Windows.
    /// `shell_args` is appended verbatim to the resulting argv. `env_vars`
    /// entries (profile overrides) are applied in order; `cwd` sets the
    /// child's working directory when it exists (a missing directory logs
    /// a warning and is skipped, mirroring `src-tauri/src/pty/session.rs`).
    /// `event_tx` receives `PtyEvent` items.
    pub fn spawn(
        cols: u16,
        rows: u16,
        event_tx: Sender<PtyEvent>,
        shell_path: &str,
        shell_args: &[String],
        env_vars: &[(String, String)],
        cwd: Option<&str>,
    ) -> std::io::Result<Self> {
        let shell = if !shell_path.trim().is_empty() {
            shell_path.to_string()
        } else if let Ok(s) = std::env::var("SHELL") {
            s
        } else {
            default_shell()?
        };

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
        for arg in shell_args {
            cmd.arg(arg);
        }
        // Advertise this terminal's capabilities to the child shell.
        // portable-pty does not set `TERM` itself, so without this the
        // shell inherits whatever the OUTER terminal that launched
        // emterm had (e.g. `xterm-kitty`, `wezterm`). Readline / line
        // editors then read that foreign terminfo and emit cursor /
        // erase sequences keyed to the wrong terminal, which manifests
        // as backspace not visually erasing characters even though the
        // shell's buffer is correct. The mux daemon spawn applies the
        // same baseline (`mux/ipc/pty_spawn.rs`); keep them in sync.
        // Set BEFORE the profile env loop so a profile can still
        // override `TERM` (e.g. for SSH targets that need a specific
        // value).
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "emterm");
        // Profile-provided environment variables, in declaration order.
        for (key, value) in env_vars {
            cmd.env(key, value);
        }
        // Strip multiplexer-injected env vars: the shell we spawn here is a
        // fresh child of native-poc, not of any outer mux/tmux session. Leaving
        // these set would (a) make `emterm mux` refuse to start with
        // "Cannot nest mux sessions (EMTERM_MUX is set)", and (b) make tmux-
        // aware CLI commands wrap responses in DCS passthrough when they
        // shouldn't. Applied after the profile env so a profile cannot
        // re-inject them.
        cmd.env_remove("EMTERM_MUX");
        cmd.env_remove("EMTERM_MUX_SOCKET");
        cmd.env_remove("TMUX");
        cmd.env_remove("TMUX_PANE");
        // Profile-provided working directory. Validated here (not at
        // resolve time) so the check runs against the directory state at
        // spawn, same as the legacy build.
        if let Some(dir) = cwd.filter(|d| !d.is_empty()) {
            let path = std::path::Path::new(dir);
            if path.is_dir() {
                cmd.cwd(path);
            } else {
                log::warn!("profile working directory does not exist, ignoring: {dir:?}");
            }
        }
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

        // ── Platform split for child + master ownership ──────────────────
        //
        // Non-Windows keeps the long-standing layout: the struct holds the
        // `Child` (wrapped) and the master `Arc` directly, the reader is
        // woken by the kernel-side EOF that `child.kill()` produces in
        // `Drop`, and no extra thread is needed.
        //
        // Windows must work around ConPTY's "no automatic EOF on child
        // exit" behavior. We clone a `ChildKiller` for the Drop fast path,
        // move the `Child` itself plus the strong master `Arc` into a
        // watcher thread, and keep only a `Weak` of the master in the
        // struct. The watcher blocks in `Child::wait()`; whenever that
        // returns (natural exit or Drop's `ChildKiller::kill`), the
        // watcher drops the master `Arc` it owns — being the sole strong
        // reference at that point, this fires `ClosePseudoConsole`, which
        // unblocks the reader's `ReadFile` so the existing `reader_loop`
        // sends exactly one `PtyEvent::Exited { Eof }` (FR6 / FR7).

        #[cfg(not(windows))]
        {
            Ok(Self {
                input_tx,
                master: Some(Arc::new(Mutex::new(master))),
                child: Arc::new(Mutex::new(child)),
                reader_join: Some(reader_join),
                writer_join: Some(writer_join),
                paused,
                ring,
            })
        }

        #[cfg(windows)]
        {
            // Clone the killer BEFORE moving the child into the watcher.
            // `clone_killer()` returns an independent `Box<dyn ChildKiller
            // + Send + Sync>`, so the struct keeps a handle into the
            // child while the watcher owns the `Child` outright.
            let child_killer = child.clone_killer();

            let master_arc = Arc::new(Mutex::new(master));
            // `Weak` for the struct; the watcher gets the strong `Arc`.
            let master_weak = Arc::downgrade(&master_arc);

            let watcher_join = std::thread::Builder::new()
                .name("native-poc-pty-watcher".into())
                .spawn(move || {
                    watcher_loop(child, master_arc);
                })
                .expect("spawn watcher thread");

            Ok(Self {
                input_tx,
                master: master_weak,
                child_killer,
                reader_join: Some(reader_join),
                writer_join: Some(writer_join),
                watcher_join: Some(watcher_join),
                paused,
                ring,
            })
        }
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

    /// Update the PTY size. Called on window resize.
    ///
    /// Non-Windows: the master `Arc` lives in the struct, so we just take
    /// the lock and call `resize`. The `Option` is only ever `None` inside
    /// `Drop` (where this method is never called), so the early-return
    /// path is defensive only.
    ///
    /// Windows: the strong `Arc` lives in the watcher thread; the struct
    /// only holds a `Weak`. We upgrade on use — if the watcher has
    /// already dropped the master after observing child exit, the upgrade
    /// returns `None` and we `warn`-log instead of touching freed memory
    /// (FR9 lifecycle invariant).
    #[cfg(not(windows))]
    pub fn resize(&self, cols: u16, rows: u16) {
        let Some(master) = self.master.as_ref() else {
            return;
        };
        let master = master.lock();
        if let Err(e) = master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }) {
            log::warn!("pty resize failed: {e}");
        }
    }

    #[cfg(windows)]
    pub fn resize(&self, cols: u16, rows: u16) {
        let Some(master_arc) = self.master.upgrade() else {
            log::warn!("pty resize: master already dropped (child exited); ignoring");
            return;
        };
        let master = master_arc.lock();
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
    // ── Non-Windows: existing 4-step shutdown, kept bit-identical (FR10) ─
    //
    // 1. Kill the child → kernel-side EOF on the master fd wakes the
    //    reader.
    // 2. Force-disconnect the input channel → writer exits.
    // 3. Drop the master `Arc` → defensive; harmless on Linux because the
    //    reader holds its own duped fd from `try_clone_reader`.
    // 4. Join reader, then writer.
    #[cfg(not(windows))]
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

        // 3. Drop the master PTY handle. On Linux this is largely
        //    defensive: the reader holds its own duped fd from
        //    `try_clone_reader`, so closing the master side does not
        //    affect the reader's read, and the kernel-side EOF that
        //    `child.kill()` already produced is what drives the reader
        //    to exit.
        //
        //    `master` is `Option` exactly so we can move it out here.
        //    `take()` decrements the `Arc` to its last owner and drops
        //    the `MasterPty`, invoking the platform-specific cleanup.
        drop(self.master.take());

        // 4. Join threads. Order matters: reader is unblocked by step 1
        //    (kernel-side EOF on master read); writer is unblocked by
        //    step 2 (channel disconnect).
        if let Some(h) = self.reader_join.take() {
            let _ = h.join();
        }
        if let Some(h) = self.writer_join.take() {
            let _ = h.join();
        }
    }

    // ── Windows: 6-step shutdown (FR8 / FR11) ────────────────────────────
    //
    // ConPTY does not produce a pipe EOF when the child exits on its own,
    // so we layered a watcher thread on top in `PtySession::spawn`. The
    // shutdown sequence has to coordinate with that watcher while
    // preserving the existing single-shot `PtyEvent::Exited` semantics
    // (FR7) and the natural-exit-vs-X-button convergence (FR7 / FR8).
    //
    // 1. `ChildKiller::kill` — wakes any in-flight `Child::wait()` in the
    //    watcher (no-op if the child is already dead).
    // 2. Force-disconnect the input channel → writer exits.
    // 3. No master to drop in the struct: the watcher owns the only
    //    remaining strong `Arc`. The struct only had a `Weak`, which is
    //    dropped implicitly when the struct is freed after this method
    //    returns.
    // 4. Join the watcher — this is the critical ordering step. Joining
    //    the watcher *before* the reader guarantees the watcher has
    //    dropped its master `Arc`, which is what fires
    //    `ClosePseudoConsole` and unblocks the reader's `ReadFile`. Step
    //    1 alone is not sufficient (TerminateProcess does not release
    //    conhost's pipe end).
    // 5. Join the reader (it observed EOF in step 4 and sent
    //    `PtyEvent::Exited`).
    // 6. Join the writer (it exited in step 2).
    #[cfg(windows)]
    fn drop(&mut self) {
        // 1. Tell the child to die. If it already exited naturally the
        //    `kill` is a no-op; either way the watcher's `wait()` will
        //    return next.
        let _ = self.child_killer.kill();

        // 2. Force-disconnect the writer's input channel. Mirror of the
        //    non-Windows path's step 2.
        let (dummy_tx, _dummy_rx) = bounded::<Vec<u8>>(1);
        let old_tx = std::mem::replace(&mut self.input_tx, dummy_tx);
        drop(old_tx);

        // 3. No-op: the watcher owns the sole strong master `Arc`. The
        //    struct's `Weak` is freed with the struct itself.

        // 4. Join the watcher. When this returns, the watcher has
        //    dropped its master `Arc` and the reader's `ReadFile` has
        //    been unblocked.
        if let Some(h) = self.watcher_join.take() {
            let _ = h.join();
        }

        // 5. Join the reader. It has either already sent
        //    `PtyEvent::Exited { Eof }` (natural exit or post-kill path)
        //    or sent it into an already-closed channel — `reader_loop`
        //    tolerates that defensively.
        if let Some(h) = self.reader_join.take() {
            let _ = h.join();
        }

        // 6. Join the writer.
        if let Some(h) = self.writer_join.take() {
            let _ = h.join();
        }
    }
}

/// Windows-only child-exit watcher. Owns the `Child` and the strong
/// master `Arc`; the [`PtySession`] struct keeps only a `ChildKiller` and
/// a `Weak` of the master.
///
/// The watcher's job is simple: block in `Child::wait()`, then drop the
/// master `Arc` (Ok or Err path alike) so `ClosePseudoConsole` fires
/// exactly once. That EOF is what the existing `reader_loop` consumes to
/// emit the single `PtyEvent::Exited` event (FR6 / FR7). The watcher
/// itself never sends events, so there is no way to double-fire `Exited`.
///
/// On `wait()` `Err`, we still drop the master so the X-button close path
/// (Drop step 1 → `ChildKiller::kill` → conhost teardown) remains
/// functional (FR12).
#[cfg(windows)]
fn watcher_loop(
    mut child: Box<dyn portable_pty::Child + Send + Sync>,
    master: Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
) {
    match child.wait() {
        Ok(_) => {}
        Err(e) => {
            log::warn!("pty watcher: Child::wait failed: {e}");
        }
    }
    // Dropping `master` decrements the strong refcount to zero (the
    // struct only held a `Weak`), which drops the underlying `MasterPty`
    // and triggers `ClosePseudoConsole`. That EOF is what the reader's
    // blocking `ReadFile` is waiting for.
    drop(master);
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
        use portable_pty::{CommandBuilder, PtySize, native_pty_system};

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

    // ── FR10 regression: non-Windows Drop sequence still constructs and
    //    tears down cleanly. We spawn a long-running shell command
    //    (`/bin/sh -c 'sleep 10'`), drop the session immediately, and
    //    assert that Drop returns within a generous timeout. If the
    //    legacy 4-step ordering regressed (e.g. the writer thread was
    //    left waiting on `recv` because we forgot to disconnect the
    //    input channel), the join would hang. Linux-only because Windows
    //    runs the 6-step variant.
    #[test]
    #[cfg(all(unix, not(target_os = "macos")))]
    fn drop_returns_quickly_on_linux() {
        use std::time::{Duration, Instant};

        let (tx, _rx) = crossbeam_channel::bounded::<PtyEvent>(64);
        let session = PtySession::spawn(
            80,
            24,
            tx,
            "/bin/sh",
            &["-c".into(), "sleep 10".into()],
            &[],
            None,
        )
        .expect("spawn pty session");

        let start = Instant::now();
        drop(session);
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_secs(3),
            "PtySession::drop took {elapsed:?}; the 4-step Drop sequence regressed"
        );
    }
}
