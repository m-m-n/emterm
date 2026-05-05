//! Per-session visibility state.
//!
//! Holds the shadow VT100 parser, the raw passthrough buffer, and the
//! atomic `visible` flag for one PTY session. The reader thread consults
//! `is_visible()` to decide whether to forward bytes to the frontend or
//! to feed them into the shadow parser only.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use tauri::ipc::{Channel, InvokeResponseBody};

use super::SessionId;
use super::passthrough_scanner::PassthroughScanner;

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
    buf: Vec<u8>,
    drop_warned: bool,
}

impl RawPassthroughBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            buf: Vec::new(),
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
            self.buf.extend_from_slice(&data[start..]);
            let first = !self.drop_warned;
            self.drop_warned = true;
            return first;
        }
        let new_len = self.buf.len() + data.len();
        if new_len > self.capacity {
            let drop_n = new_len - self.capacity;
            self.buf.drain(..drop_n);
            self.buf.extend_from_slice(data);
            let first = !self.drop_warned;
            self.drop_warned = true;
            return first;
        }
        self.buf.extend_from_slice(data);
        false
    }

    pub fn read_all(&self) -> Vec<u8> {
        self.buf.clone()
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
pub struct SessionVisibilityState {
    visible: AtomicBool,
    inner: Mutex<InnerState>,
}

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
        inner.shadow.set_size(rows, cols);
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
    ///
    /// Bytes are NOT forwarded to the frontend channel.
    ///
    /// `vt100::Parser::process` is wrapped in `catch_unwind` because the
    /// upstream crate has known panic paths (e.g. `grid::col_wrap` unwrap
    /// on certain wide-character + scroll boundaries). On panic the shadow
    /// parser is rebuilt at its previous dimensions; the snapshot for the
    /// next visible-resume will reflect a cleared screen rather than
    /// crashing the process.
    pub fn process_hidden(&self, data: &[u8]) {
        if data.is_empty() {
            return;
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
    }

    /// Atomically swap to visible. If a transition actually happened,
    /// build a resume snapshot (`ESC[H ESC[2J` + shadow contents +
    /// raw_passthrough bytes), clear the passthrough buffer, and return
    /// `Some(bytes)`. If already visible, return `None`.
    ///
    /// `vt100::Screen::contents_formatted` is wrapped in `catch_unwind`
    /// for the same reason as `process_hidden`. On panic the shadow is
    /// rebuilt and an empty contents block is used; the snapshot still
    /// includes the clear-screen prefix and any buffered passthrough so
    /// the frontend recovers without a backend crash.
    pub fn set_visible_and_take_snapshot(&self) -> Option<Vec<u8>> {
        let prev = self.visible.swap(true, Ordering::AcqRel);
        if prev {
            return None;
        }
        log::debug!("[DEBUG][BACKEND] visibility: hidden -> visible (building snapshot)");
        let mut inner = self.inner.lock().expect("visibility state poisoned");
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
        Some(out)
    }

    /// Send the resume snapshot through the registered reader channel.
    /// Returns `true` if a snapshot was produced and sent.
    pub fn dispatch_resume_snapshot(&self) -> bool {
        let snapshot = self.set_visible_and_take_snapshot();
        let Some(bytes) = snapshot else {
            return false;
        };
        let inner = self.inner.lock().expect("visibility state poisoned");
        let Some(channel) = inner.channel.as_ref() else {
            log::warn!(
                "[WARN][BACKEND] visibility: no channel registered, snapshot ({}B) dropped",
                bytes.len()
            );
            return false;
        };
        if let Err(e) = channel.send(InvokeResponseBody::Raw(bytes)) {
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
#[derive(Clone, Default)]
pub struct VisibilityRegistry {
    inner: Arc<RwLock<HashMap<SessionId, Arc<SessionVisibilityState>>>>,
}

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
    fn raw_passthrough_clear_resets_warn_flag() {
        let mut buf = RawPassthroughBuffer::new(4);
        buf.append(b"abcd");
        buf.append(b"e");
        buf.clear();
        assert_eq!(buf.len(), 0);
        let first = buf.append(b"abcde");
        assert!(first, "after clear, the warn flag must rearm");
    }

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

    /// TS-2: snapshot prefix is the standard reset-and-home pair.
    #[test]
    fn visibility_state_snapshot_starts_with_reset_prefix() {
        let s = SessionVisibilityState::new(80, 24, 1024);
        s.set_hidden();
        s.process_hidden(b"x");
        let snap = s.set_visible_and_take_snapshot().expect("snapshot");
        assert!(snap.starts_with(b"\x1b[H\x1b[2J"));
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
