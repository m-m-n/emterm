//! Per-session visibility state.
//!
//! Holds the shadow VT100 parser, the raw passthrough buffer, and the
//! atomic `visible` flag for one PTY session. The reader thread consults
//! `is_visible()` to decide whether to forward bytes to the frontend or
//! to feed them into the shadow parser only.

#[cfg(feature = "gui")]
use std::collections::HashMap;
use std::collections::VecDeque;
#[cfg(feature = "gui")]
use std::panic::{catch_unwind, AssertUnwindSafe};
#[cfg(feature = "gui")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "gui")]
use std::sync::{Arc, Mutex, RwLock};

#[cfg(feature = "gui")]
use tauri::ipc::{Channel, InvokeResponseBody};

#[cfg(feature = "gui")]
use super::passthrough_scanner::PassthroughScanner;
#[cfg(feature = "gui")]
use super::SessionId;

/// Default raw passthrough capacity for non-mux sessions (4 MiB).
pub const HIDDEN_PASSTHROUGH_CAPACITY_NONMUX: usize = 4 * 1024 * 1024;

/// Default raw passthrough capacity for mux panes (1 MiB per pane).
pub const HIDDEN_PASSTHROUGH_CAPACITY_MUX: usize = 1024 * 1024;

/// Maximum partial-buffer size while a passthrough scanner is mid-sequence.
/// Beyond this, the in-flight sequence is dropped (with a warn).
pub const PARTIAL_SEQUENCE_MAX: usize = 16 * 1024 * 1024;

/// Frontend-side debounce for hide transitions. Visible -> hidden is delayed
/// by this duration; hidden -> visible is immediate.
pub const HIDDEN_DEBOUNCE_MS: u64 = 1000;

/// Bounded byte-buffer that retains the *most recent* `capacity` bytes
/// of raw passthrough sequences (Kitty / SIXEL / OSC 9999) seen while
/// hidden. Old bytes are evicted from the front when capacity is exceeded.
///
/// `append` returns `true` whenever the call caused bytes to be dropped,
/// so the caller can emit a single warn per drop episode.
pub struct RawPassthroughBuffer {
    capacity: usize,
    buf: VecDeque<u8>,
    drop_warned: bool,
}

impl RawPassthroughBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            buf: VecDeque::new(),
            drop_warned: false,
        }
    }

    /// Append `data` to the buffer, evicting from the front if capacity is
    /// exceeded. Returns `true` if a drop happened *and* this is the first
    /// drop since the last `clear()` (so the caller knows to warn once).
    pub fn append(&mut self, data: &[u8]) -> bool {
        if data.is_empty() {
            return false;
        }
        if data.len() > self.capacity {
            // Single chunk is larger than the entire buffer. Keep the
            // last `capacity` bytes only.
            self.buf.clear();
            let start = data.len() - self.capacity;
            self.buf.extend(data[start..].iter().copied());
            let first = !self.drop_warned;
            self.drop_warned = true;
            return first;
        }
        let new_len = self.buf.len() + data.len();
        if new_len > self.capacity {
            let drop_n = new_len - self.capacity;
            self.buf.drain(..drop_n);
            self.buf.extend(data.iter().copied());
            let first = !self.drop_warned;
            self.drop_warned = true;
            return first;
        }
        self.buf.extend(data.iter().copied());
        false
    }

    pub fn read_all(&self) -> Vec<u8> {
        Vec::from_iter(self.buf.iter().copied())
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.drop_warned = false;
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

/// Internal state guarded by a mutex. Held inside `SessionVisibilityState`.
#[cfg(feature = "gui")]
struct InnerState {
    shadow: vt100::Parser,
    /// Last known shadow parser dimensions, kept so that we can rebuild
    /// the parser at the same size after a `vt100` panic.
    shadow_cols: u16,
    shadow_rows: u16,
    passthrough: RawPassthroughBuffer,
    scanner: PassthroughScanner,
    /// Channel for sending bytes to the frontend WASM. Set by the reader
    /// thread on first registration; used by `set_visible_and_take_snapshot`
    /// to deliver the resume snapshot through the same path the reader uses.
    channel: Option<Channel<InvokeResponseBody>>,
}

/// Visibility + shadow state for a single PTY session.
#[cfg(feature = "gui")]
pub struct SessionVisibilityState {
    visible: AtomicBool,
    inner: Mutex<InnerState>,
}

#[cfg(feature = "gui")]
impl SessionVisibilityState {
    pub fn new(cols: u16, rows: u16, passthrough_capacity: usize) -> Self {
        Self {
            visible: AtomicBool::new(true),
            inner: Mutex::new(InnerState {
                shadow: vt100::Parser::new(rows, cols, 0),
                shadow_cols: cols,
                shadow_rows: rows,
                passthrough: RawPassthroughBuffer::new(passthrough_capacity),
                scanner: PassthroughScanner::new(),
                channel: None,
            }),
        }
    }

    /// Lock-free read of the current visibility flag. Consulted by the
    /// reader thread on each batch.
    pub fn is_visible(&self) -> bool {
        self.visible.load(Ordering::Acquire)
    }

    /// Register the reader-thread `Channel` so resume snapshots can be
    /// delivered via the same route as normal PTY data. Idempotent: a
    /// later registration overwrites the prior one (e.g., on session
    /// reuse).
    pub fn register_channel(&self, channel: Channel<InvokeResponseBody>) {
        let mut inner = self.inner.lock().expect("visibility state poisoned");
        inner.channel = Some(channel);
    }

    /// Resize the shadow parser to match a frontend resize.
    pub fn resize(&self, cols: u16, rows: u16) {
        let mut inner = self.inner.lock().expect("visibility state poisoned");
        inner.shadow.screen_mut().set_size(rows, cols);
        inner.shadow_cols = cols;
        inner.shadow_rows = rows;
    }

    /// Mark the session hidden. Returns the previous flag.
    pub fn set_hidden(&self) -> bool {
        let prev = self.visible.swap(false, Ordering::AcqRel);
        if prev {
            log::debug!("[DEBUG][BACKEND] visibility: visible -> hidden");
        }
        prev
    }

    /// Process one batch of PTY bytes while hidden:
    ///   1. feed the shadow parser
    ///   2. extract passthrough sequences (Kitty / SIXEL / OSC 9999) and
    ///      append to the raw passthrough buffer
    ///   3. surface any recognized OSC 9 desktop-notification messages
    ///
    /// Returns the recognized OSC 9 notification messages (FR1). These are
    /// side-effect events and are deliberately kept OUT of the raw passthrough
    /// buffer so they fire once and are never replayed on resume (FR5). The
    /// caller (reader thread) forwards them to the frontend notification sink.
    ///
    /// Bytes are NOT forwarded to the frontend channel.
    ///
    /// `vt100::Parser::process` is wrapped in `catch_unwind` because the
    /// upstream crate has known panic paths (e.g. `grid::col_wrap` unwrap
    /// on certain wide-character + scroll boundaries). On panic the shadow
    /// parser is rebuilt at its previous dimensions; the snapshot for the
    /// next visible-resume will reflect a cleared screen rather than
    /// crashing the process.
    pub fn process_hidden(&self, data: &[u8]) -> Vec<String> {
        if data.is_empty() {
            return Vec::new();
        }
        let mut inner = self.inner.lock().expect("visibility state poisoned");
        let shadow_ref = &mut inner.shadow;
        let result = catch_unwind(AssertUnwindSafe(|| shadow_ref.process(data)));
        if result.is_err() {
            log::warn!(
                "[WARN][BACKEND] visibility: shadow vt100 parser panicked; rebuilding at {}x{}",
                inner.shadow_cols,
                inner.shadow_rows,
            );
            inner.shadow = vt100::Parser::new(inner.shadow_rows, inner.shadow_cols, 0);
        }
        let extracted = inner.scanner.process(data);
        if !extracted.is_empty() {
            let dropped = inner.passthrough.append(&extracted);
            if dropped {
                log::warn!(
                    "[WARN][BACKEND] visibility: raw_passthrough overflow (capacity {}B); oldest bytes dropped",
                    inner.passthrough.capacity()
                );
            }
        }
        inner.scanner.take_notifications()
    }

    /// Feed the shadow parser while visible.
    ///
    /// Mirrors `process_hidden` but skips the passthrough scanner because
    /// passthrough sequences (Kitty / SIXEL / OSC 9999) are already being
    /// forwarded live to the frontend. Keeping the shadow in sync during
    /// visible periods means a subsequent visible→hidden→visible cycle
    /// produces a faithful resume snapshot even when the shell is idle
    /// during the hidden window — without this the shadow only contains
    /// bytes received while hidden, so an idle hidden period yields a
    /// blank snapshot that wipes the visible screen on resume.
    pub fn process_visible(&self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let mut inner = self.inner.lock().expect("visibility state poisoned");
        let shadow_ref = &mut inner.shadow;
        let result = catch_unwind(AssertUnwindSafe(|| shadow_ref.process(data)));
        if result.is_err() {
            log::warn!(
                "[WARN][BACKEND] visibility: shadow vt100 parser panicked (visible path); rebuilding at {}x{}",
                inner.shadow_cols,
                inner.shadow_rows,
            );
            inner.shadow = vt100::Parser::new(inner.shadow_rows, inner.shadow_cols, 0);
        }
    }

    /// Build the resume snapshot (`ESC[H ESC[2J` + shadow contents +
    /// raw_passthrough bytes) and clear the passthrough buffer. Caller must
    /// hold the inner lock throughout to keep the snapshot consistent with
    /// the shadow state observed at lock-acquire time.
    ///
    /// `vt100::Screen::contents_formatted` is wrapped in `catch_unwind`
    /// for the same reason as `process_hidden`. On panic the shadow is
    /// rebuilt and an empty contents block is used; the snapshot still
    /// includes the clear-screen prefix and any buffered passthrough so
    /// the frontend recovers without a backend crash.
    fn build_snapshot_locked(inner: &mut InnerState) -> Vec<u8> {
        let screen = match catch_unwind(AssertUnwindSafe(|| {
            inner.shadow.screen().contents_formatted()
        })) {
            Ok(bytes) => bytes,
            Err(_) => {
                log::warn!(
                    "[WARN][BACKEND] visibility: shadow vt100 parser panicked while building snapshot; rebuilding at {}x{}",
                    inner.shadow_cols,
                    inner.shadow_rows,
                );
                inner.shadow = vt100::Parser::new(inner.shadow_rows, inner.shadow_cols, 0);
                Vec::new()
            }
        };
        let passthrough = inner.passthrough.read_all();
        let mut out = Vec::with_capacity(4 + screen.len() + passthrough.len());
        out.extend_from_slice(b"\x1b[H\x1b[2J");
        out.extend_from_slice(&screen);
        out.extend_from_slice(&passthrough);
        inner.passthrough.clear();
        out
    }

    /// Build a resume snapshot and flip the visible flag, returning the
    /// snapshot bytes. If already visible, returns `None` and does not
    /// touch the flag or buffers.
    ///
    /// Caller-side ordering: the caller must have already enqueued the
    /// returned bytes onto the reader channel before any subsequent live
    /// reader batch can be sent. `dispatch_resume_snapshot` does this in
    /// one call by holding the inner lock across `channel.send` and the
    /// flag flip; external callers that bypass `dispatch_resume_snapshot`
    /// inherit the responsibility for that ordering.
    pub fn set_visible_and_take_snapshot(&self) -> Option<Vec<u8>> {
        let mut inner = self.inner.lock().expect("visibility state poisoned");
        if self.visible.load(Ordering::Acquire) {
            return None;
        }
        log::debug!("[DEBUG][BACKEND] visibility: hidden -> visible (building snapshot)");
        let bytes = Self::build_snapshot_locked(&mut inner);
        self.visible.store(true, Ordering::Release);
        Some(bytes)
    }

    /// Send the resume snapshot through the registered reader channel.
    /// Returns `true` if a snapshot was produced and sent.
    ///
    /// FR9 ordering guarantee: the snapshot is enqueued onto the reader
    /// `Channel` *before* the `visible` atomic flag flips to `true`. The
    /// reader thread checks `is_visible()` lock-free and only forwards live
    /// PTY bytes once that flag is true, so the channel's FIFO order
    /// guarantees the snapshot lands ahead of any subsequent live batch.
    pub fn dispatch_resume_snapshot(&self) -> bool {
        let mut inner = self.inner.lock().expect("visibility state poisoned");
        if self.visible.load(Ordering::Acquire) {
            return false;
        }
        log::debug!("[DEBUG][BACKEND] visibility: hidden -> visible (building snapshot)");
        let bytes = Self::build_snapshot_locked(&mut inner);
        let Some(channel) = inner.channel.as_ref() else {
            log::warn!(
                "[WARN][BACKEND] visibility: no channel registered, snapshot ({}B) dropped",
                bytes.len()
            );
            self.visible.store(true, Ordering::Release);
            return false;
        };
        let send_result = channel.send(InvokeResponseBody::Raw(bytes));
        self.visible.store(true, Ordering::Release);
        if let Err(e) = send_result {
            log::warn!(
                "[WARN][BACKEND] visibility: snapshot channel.send failed: {}",
                e
            );
            return false;
        }
        true
    }
}

/// Per-`PtyManager` registry of `SessionVisibilityState` arcs keyed by session id.
#[cfg(feature = "gui")]
#[derive(Clone, Default)]
pub struct VisibilityRegistry {
    inner: Arc<RwLock<HashMap<SessionId, Arc<SessionVisibilityState>>>>,
}

#[cfg(feature = "gui")]
impl VisibilityRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a new state for `id`, replacing any existing entry.
    pub fn register(
        &self,
        id: SessionId,
        cols: u16,
        rows: u16,
        passthrough_capacity: usize,
    ) -> Arc<SessionVisibilityState> {
        let state = Arc::new(SessionVisibilityState::new(
            cols,
            rows,
            passthrough_capacity,
        ));
        let mut guard = self.inner.write().expect("visibility registry poisoned");
        guard.insert(id, state.clone());
        state
    }

    pub fn get(&self, id: &str) -> Option<Arc<SessionVisibilityState>> {
        let guard = self.inner.read().expect("visibility registry poisoned");
        guard.get(id).cloned()
    }

    pub fn remove(&self, id: &str) -> Option<Arc<SessionVisibilityState>> {
        let mut guard = self.inner.write().expect("visibility registry poisoned");
        guard.remove(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_passthrough_keeps_tail_when_over_capacity() {
        let mut buf = RawPassthroughBuffer::new(8);
        assert!(!buf.append(b"abcd"));
        assert!(!buf.append(b"efgh"));
        // Now at capacity (8 bytes). Append 4 more -> drop oldest 4.
        assert!(buf.append(b"ijkl"), "first overflow must report drop");
        assert_eq!(buf.read_all(), b"efghijkl");
    }

    #[test]
    fn raw_passthrough_warn_only_once() {
        let mut buf = RawPassthroughBuffer::new(4);
        buf.append(b"abcd");
        let first = buf.append(b"e"); // drop
        let second = buf.append(b"f"); // drop again, but flagged
        assert!(first);
        assert!(!second, "second drop must NOT report (warn-once)");
        // After clear(), the next drop must warn again.
        buf.clear();
        buf.append(b"wxyz");
        let third = buf.append(b"!");
        assert!(third);
    }

    #[test]
    fn raw_passthrough_single_chunk_larger_than_capacity() {
        let mut buf = RawPassthroughBuffer::new(4);
        let dropped = buf.append(b"123456789");
        assert!(dropped);
        assert_eq!(buf.read_all(), b"6789");
    }

    #[test]
    fn raw_passthrough_fifo_eviction_byte_by_byte() {
        // Capacity 16, append 1 byte at a time for 1000 iterations.
        // After each append past capacity, the buffer must hold the
        // trailing 16 bytes in insertion order (FIFO). The VecDeque
        // backing makes per-append eviction O(1); the test asserts the
        // observable contract (FIFO content + length cap) rather than
        // timing.
        let mut buf = RawPassthroughBuffer::new(16);
        for i in 0u32..1000 {
            let byte = [(i & 0xFF) as u8];
            buf.append(&byte);
            assert!(buf.len() <= 16);
        }
        assert_eq!(buf.len(), 16);
        let tail = buf.read_all();
        let expected: Vec<u8> = (1000u32 - 16..1000u32).map(|i| (i & 0xFF) as u8).collect();
        assert_eq!(tail, expected);
    }

    #[test]
    fn raw_passthrough_fifo_eviction_chunked() {
        // Capacity 8. Multiple chunks that together overflow must keep
        // the trailing capacity bytes in arrival order.
        let mut buf = RawPassthroughBuffer::new(8);
        buf.append(b"AAAA");
        buf.append(b"BBBB");
        buf.append(b"CCCC");
        buf.append(b"DD");
        assert_eq!(buf.len(), 8);
        assert_eq!(buf.read_all(), b"BBCCCCDD");
    }

    #[test]
    fn raw_passthrough_read_all_does_not_consume() {
        let mut buf = RawPassthroughBuffer::new(8);
        buf.append(b"abcd");
        let first = buf.read_all();
        let second = buf.read_all();
        assert_eq!(first, b"abcd");
        assert_eq!(second, b"abcd");
        assert_eq!(buf.len(), 4);
    }

    #[test]
    fn raw_passthrough_clear_resets_warn_flag() {
        let mut buf = RawPassthroughBuffer::new(4);
        buf.append(b"abcd");
        buf.append(b"e");
        buf.clear();
        assert_eq!(buf.len(), 0);
        let first = buf.append(b"abcde");
        assert!(first, "after clear, the warn flag must rearm");
    }
}

#[cfg(all(test, feature = "gui"))]
mod visibility_state_tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    #[test]
    fn visibility_state_starts_visible() {
        let s = SessionVisibilityState::new(80, 24, 1024);
        assert!(s.is_visible());
    }

    #[test]
    fn visibility_state_set_hidden_returns_prev() {
        let s = SessionVisibilityState::new(80, 24, 1024);
        let prev = s.set_hidden();
        assert!(prev);
        assert!(!s.is_visible());
        let prev = s.set_hidden();
        assert!(!prev);
    }

    #[test]
    fn visibility_state_set_visible_returns_none_when_already_visible() {
        let s = SessionVisibilityState::new(80, 24, 1024);
        assert!(s.set_visible_and_take_snapshot().is_none());
    }

    #[test]
    fn visibility_state_set_visible_returns_snapshot_after_hidden() {
        let s = SessionVisibilityState::new(80, 24, 1024);
        s.set_hidden();
        s.process_hidden(b"hello world");
        let snap = s.set_visible_and_take_snapshot().expect("snapshot");
        assert!(snap.starts_with(b"\x1b[H\x1b[2J"));
        // Snapshot must contain the visible chars somewhere (vt100 may
        // emit additional control sequences).
        let s = String::from_utf8_lossy(&snap);
        assert!(s.contains("hello world"), "snapshot should contain text");
    }

    #[test]
    fn visibility_state_process_visible_seeds_shadow_so_idle_hidden_resume_preserves_screen() {
        // Regression: when the session was visible and producing output, then
        // went hidden during an idle period (no PTY bytes), the resume
        // snapshot used to be `clear-screen + blank` because the shadow had
        // never been fed. With process_visible() in place the shadow
        // reflects the pre-hidden screen state, so the snapshot redraws it.
        let s = SessionVisibilityState::new(80, 24, 1024);
        // Simulate visible-period output.
        s.process_visible(b"prompt> running tail -f\r\nline 1\r\nline 2\r\n");
        s.set_hidden();
        // Idle hidden: no process_hidden calls.
        let snap = s.set_visible_and_take_snapshot().expect("snapshot");
        assert!(snap.starts_with(b"\x1b[H\x1b[2J"));
        let snap_str = String::from_utf8_lossy(&snap);
        assert!(
            snap_str.contains("prompt>"),
            "snapshot must redraw pre-hidden visible content, got: {:?}",
            snap_str,
        );
        assert!(
            snap_str.contains("line 2"),
            "snapshot must include later visible lines, got: {:?}",
            snap_str,
        );
    }

    #[test]
    fn visibility_state_process_visible_then_hidden_combines_both() {
        // Visible-fed bytes plus hidden-fed bytes both land in the snapshot.
        let s = SessionVisibilityState::new(80, 24, 1024);
        s.process_visible(b"before-hidden\r\n");
        s.set_hidden();
        s.process_hidden(b"during-hidden\r\n");
        let snap = s.set_visible_and_take_snapshot().expect("snapshot");
        let snap_str = String::from_utf8_lossy(&snap);
        assert!(
            snap_str.contains("before-hidden"),
            "snapshot must include visible-period content, got: {:?}",
            snap_str,
        );
        assert!(
            snap_str.contains("during-hidden"),
            "snapshot must include hidden-period content, got: {:?}",
            snap_str,
        );
    }

    #[test]
    fn visibility_state_resize_updates_shadow_size() {
        let s = SessionVisibilityState::new(80, 24, 1024);
        s.resize(120, 40);
        let inner = s.inner.lock().unwrap();
        assert_eq!(inner.shadow.screen().size(), (40, 120));
    }

    #[test]
    fn visibility_state_process_hidden_extracts_kitty_image_into_passthrough() {
        let s = SessionVisibilityState::new(80, 24, 4096);
        s.set_hidden();
        // Wrap a tiny APC payload (Gi=1,a=T;) in ESC _ ... ESC \
        let mut data = Vec::new();
        data.extend_from_slice(b"hello\x1b_Gi=1,a=T;ABCDE\x1b\\world");
        s.process_hidden(&data);
        let snap = s.set_visible_and_take_snapshot().expect("snapshot");
        // raw_passthrough must contain the APC payload
        let s = String::from_utf8_lossy(&snap);
        assert!(s.contains("\u{1b}_Gi=1"), "snapshot must include kitty APC");
        assert!(s.contains("ABCDE"), "snapshot must include APC body");
    }

    /// TS-15: hidden session can absorb 10 MiB of bytes without registering
    /// any "sent" volume on the diagnostic counters (the reader path that
    /// would call `add_sent` is not exercised in this unit test, but
    /// `process_hidden` itself never increments anything observable).
    #[test]
    fn visibility_state_hidden_absorbs_large_payload() {
        let s = SessionVisibilityState::new(80, 24, 4 * 1024 * 1024);
        s.set_hidden();
        let big = vec![b'A'; 10 * 1024 * 1024];
        s.process_hidden(&big);
        // Resume snapshot: shadow contents at most cols*rows ≈ tens of KB
        // plus the (empty) raw_passthrough — must not be 10 MiB.
        let snap = s.set_visible_and_take_snapshot().expect("snapshot");
        assert!(
            snap.len() < 1024 * 1024,
            "snapshot must be a screen replay, not a tape replay (got {}B)",
            snap.len()
        );
    }

    /// F3 regression: `dispatch_resume_snapshot` must enqueue the snapshot
    /// onto the registered `Channel` *before* flipping the `visible` flag to
    /// true. Without that ordering, a reader thread that observes
    /// `is_visible() == true` lock-free could send a live PTY batch ahead
    /// of the snapshot, and the snapshot's `\x1b[H\x1b[2J` prefix would
    /// then wipe that live batch from the frontend grid.
    #[test]
    fn visibility_dispatch_enqueues_snapshot_before_flag_flip() {
        let s = Arc::new(SessionVisibilityState::new(80, 24, 1024));
        s.set_hidden();
        s.process_hidden(b"resume-payload");

        let visible_when_received: Arc<StdMutex<Option<bool>>> = Arc::new(StdMutex::new(None));
        let captured_bytes: Arc<StdMutex<Vec<u8>>> = Arc::new(StdMutex::new(Vec::new()));
        let captured_clone = captured_bytes.clone();
        let visible_clone = visible_when_received.clone();
        let s_for_cb = s.clone();
        let channel = Channel::new(move |body| {
            *visible_clone.lock().unwrap() = Some(s_for_cb.is_visible());
            if let InvokeResponseBody::Raw(bytes) = body {
                captured_clone.lock().unwrap().extend_from_slice(&bytes);
            }
            Ok(())
        });
        s.register_channel(channel);

        assert!(s.dispatch_resume_snapshot());
        assert!(s.is_visible(), "flag must be true after dispatch");
        let observed = visible_when_received
            .lock()
            .unwrap()
            .expect("send callback must have fired");
        assert!(
            !observed,
            "callback observed visible=true at send time; snapshot would race the next reader batch"
        );
        let body = captured_bytes.lock().unwrap();
        assert!(body.starts_with(b"\x1b[H\x1b[2J"));
        let s = String::from_utf8_lossy(&body);
        assert!(s.contains("resume-payload"));
    }

    /// Re-using the same dispatch path while already visible must be a
    /// no-op — no snapshot, no flag change.
    #[test]
    fn visibility_dispatch_when_already_visible_is_noop() {
        let s = SessionVisibilityState::new(80, 24, 1024);
        let count: Arc<StdMutex<u32>> = Arc::new(StdMutex::new(0));
        let count_clone = count.clone();
        let channel = Channel::new(move |_body| {
            *count_clone.lock().unwrap() += 1;
            Ok(())
        });
        s.register_channel(channel);

        assert!(!s.dispatch_resume_snapshot());
        assert_eq!(*count.lock().unwrap(), 0);
    }

    /// TS-2: snapshot prefix is the standard reset-and-home pair.
    #[test]
    fn visibility_state_snapshot_starts_with_reset_prefix() {
        let s = SessionVisibilityState::new(80, 24, 1024);
        s.set_hidden();
        s.process_hidden(b"x");
        let snap = s.set_visible_and_take_snapshot().expect("snapshot");
        assert!(snap.starts_with(b"\x1b[H\x1b[2J"));
    }

    /// TS-8: process_hidden surfaces an OSC 9 notification message and does
    /// NOT add it to the passthrough buffer (so it is never replayed).
    #[test]
    fn visibility_process_hidden_surfaces_osc9_notification() {
        let s = SessionVisibilityState::new(80, 24, 4096);
        s.set_hidden();
        let notifications = s.process_hidden(b"work\x1b]9;build done\x07more");
        assert_eq!(notifications, vec!["build done".to_string()]);
        // The OSC 9 bytes must NOT be in the resume snapshot passthrough.
        let snap = s.set_visible_and_take_snapshot().expect("snapshot");
        let snap_str = String::from_utf8_lossy(&snap);
        assert!(
            !snap_str.contains("\x1b]9;build done"),
            "OSC 9 notification must not be replayed in the snapshot"
        );
    }

    /// TS-3 / FR4: process_hidden does NOT surface a progress sequence.
    #[test]
    fn visibility_process_hidden_ignores_osc9_progress() {
        let s = SessionVisibilityState::new(80, 24, 4096);
        s.set_hidden();
        let notifications = s.process_hidden(b"\x1b]9;4;1;50\x07");
        assert!(
            notifications.is_empty(),
            "progress sequence must not surface a notification"
        );
    }

    /// FR5: a notification surfaced while hidden is not re-surfaced when the
    /// next hidden batch contains no new OSC 9, and is absent from the resume
    /// snapshot — so window restore cannot replay it.
    #[test]
    fn visibility_process_hidden_notification_not_resurfaced() {
        let s = SessionVisibilityState::new(80, 24, 4096);
        s.set_hidden();
        let first = s.process_hidden(b"\x1b]9;done\x07");
        assert_eq!(first, vec!["done".to_string()]);
        // Subsequent hidden batch without OSC 9: no notification re-emitted.
        let second = s.process_hidden(b"plain output\r\n");
        assert!(second.is_empty());
    }

    #[test]
    fn visibility_registry_register_get_remove() {
        let reg = VisibilityRegistry::new();
        let _state = reg.register("s1".to_string(), 80, 24, 1024);
        assert!(reg.get("s1").is_some());
        assert!(reg.remove("s1").is_some());
        assert!(reg.get("s1").is_none());
    }
}
