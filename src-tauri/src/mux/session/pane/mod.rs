//! Pane state: owns a PTY, reader thread, and bounded channel.

use std::io::Write;
use std::sync::{Arc, Mutex as StdMutex};

use portable_pty::MasterPty;
use tokio::sync::mpsc;

use crate::agent_status::{AgentState, AgentStatusEvent};
use crate::agent_status_exit_latch::AgentStatusExitLatch;
use crate::mux::scrollback_buffer::{
    DEFAULT_SCROLLBACK_CAPACITY, MAX_DIM_MARKERS, ScrollbackRingBuffer,
};
use crate::mux::session::child_reaper;
use crate::mux::snapshot_bytes::build_resume_snapshot_bytes;
use crate::prompts::PromptMarkKind;
use crate::pty::passthrough_scanner::PassthroughScanner;
use crate::pty::visibility::{HIDDEN_PASSTHROUGH_CAPACITY_MUX, RawPassthroughBuffer};

mod handles;
mod output_queue;
mod output_target;

pub use handles::*;
pub use output_queue::*;
pub use output_target::*;

/// Encode `(bytes, segments)` into the D1' wire format
/// (`mux_ipc::protocol::encode_snapshot_payload`) for a `Snapshot`-kind
/// `PtyOutputChunk` (task0004 round-4 rework). Thin wrapper converting the
/// mux-layer's plain `(usize, u16, u16)` segment tuples into
/// `mux_ipc::protocol::DimSegment` at the wire boundary.
pub(in crate::mux) fn encode_snapshot_segments(
    bytes: &[u8],
    segments: &[(usize, u16, u16)],
) -> Vec<u8> {
    let dim_segments: Vec<mux_ipc::protocol::DimSegment> = segments
        .iter()
        .map(|&(offset, cols, rows)| mux_ipc::protocol::DimSegment {
            offset: offset as u32,
            cols,
            rows,
        })
        .collect();
    mux_ipc::protocol::encode_snapshot_payload(&dim_segments, bytes)
}

/// A pane's child-process reference (task plan task0007, IMPLEMENTATION.md
/// D6): either an owned PTY-library child handle (a freshly spawned pane,
/// task0001 SPEC FR2) or a bare process id (a pane restored from a handoff,
/// whose owned handle could not be rebuilt after the process image was
/// replaced). [`MuxPane::mark_exited`] routes to the matching
/// [`child_reaper`] entry point for whichever variant is present, so a
/// restored pane and a freshly spawned pane are indistinguishable to the
/// rest of the daemon.
pub enum PaneChild {
    /// A freshly spawned pane's owned child handle.
    Owned(Box<dyn portable_pty::Child + Send + Sync>),
    /// A process id belonging to a pane restored from a handoff. Unix only:
    /// the reaping path this variant routes to
    /// ([`child_reaper::spawn_reaper_pid`]) is Unix-only (task plan Design —
    /// "Guard the process-id path to Unix").
    #[cfg(unix)]
    ProcessId(u32),
}

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
    /// A pane's child-process reference (task0001 SPEC FR2; task plan
    /// task0007, IMPLEMENTATION.md D6), retained for the pane's lifetime and
    /// taken + handed to the matching [`child_reaper`] entry point on
    /// [`Self::mark_exited`]. `None` for panes constructed without a
    /// spawned/restored child (`new_test` / `new_test_with_writer`, and this
    /// module's own tests that open a real PTY but never call
    /// `spawn_command`).
    child: Option<PaneChild>,
    /// Whether this pane's PTY has exited.
    pub exited: bool,
    /// Shadow VT100 parser for screen state tracking (used for reattach restoration).
    pub shadow_parser: SharedShadowParser,
    /// Cached working directory from OSC 7 detection.
    pub cwd: Arc<StdMutex<Option<String>>>,
    /// Cached title from OSC 0/2 detection (set by pty_reader_loop).
    pub title: Arc<StdMutex<Option<String>>>,
    /// Swappable title change sender (reader thread reads from this on each title change).
    pub title_sender: SharedTitleSender,
    /// Daemon-lifetime notification sender. The reader thread forwards OSC 9
    /// desktop notifications detected on Detached output through this channel
    /// so the daemon can relay them to the GUI client.
    pub notification_sender: SharedNotificationSender,
    /// This pane's agent-status state (SPEC FR3): current report + revision.
    pub agent_status: SharedAgentStatus,
    /// Daemon-lifetime agent-status report sender. The reader thread forwards
    /// raw agent-status OSC payload strings through this channel (regardless
    /// of attach state) so the daemon can validate, apply, and broadcast them.
    pub agent_status_report_sender: SharedAgentStatusReportSender,
    /// Registered `WaitAgentState` waiters for this pane (task0004). See
    /// [`AgentWaiter`].
    pub agent_waiters: SharedAgentWaiters,
    /// This pane's inferred-clear latch (task0001 `AgentStatusExitLatch`,
    /// SPEC FR1/FR2). Fed by [`Self::apply_agent_status_event`] (explicit
    /// Set/Clear) and [`Self::record_live_osc133_mark`] (live OSC 133
    /// marks); a fired inferred clear is applied through the SAME path as
    /// an explicit `Clear` — no parallel clear logic.
    pub agent_status_exit_latch: SharedAgentStatusExitLatch,
    /// Per-pane raw passthrough buffer for image / Markdown OSC sequences
    /// captured while the pane is detached (network detach OR client hidden).
    /// Drained into the reattach / resume snapshot.
    pub raw_passthrough: Arc<StdMutex<RawPassthroughBuffer>>,
    /// Stateful scanner that pulls passthrough sequences out of the raw PTY
    /// byte stream. Lives alongside `raw_passthrough` because partial
    /// sequences span multiple reader chunks.
    pub passthrough_scanner: Arc<StdMutex<PassthroughScanner>>,
    /// Per-pane scrollback ring buffer. Holds recent PTY bytes for replay
    /// on reattach. Phase B: the reader only writes here while the pane is
    /// detached; Phase C will switch to always-on writes so pre-detach
    /// scrollback is also retained.
    pub scrollback: SharedScrollback,
    /// Lock-free snapshot of this pane's current dimensions (task0003 D5).
    /// See [`PaneDims`] for the ordering guarantee this closes.
    pub dims: SharedPaneDims,
}

/// Clamp `(cols, rows)` to the domain the wire decoder ACTUALLY accepts for
/// a snapshot segment (D6'''', round-7 rework, review round-6 finding
/// `6cefb1dd16c126b6`).
///
/// `term_core::terminal_core::clamp_resize_dims` bounds each axis
/// independently to `1..=4096`, but `mux_ipc::protocol`'s segment decoder
/// additionally rejects any segment whose `cols * rows` product exceeds
/// `MAX_SEGMENT_CELLS` (1_000_000) — a legitimate-looking size like
/// 2000x600 (product 1,200,000) or the per-axis ceiling itself
/// (4096x4096 = 16,777,216) clears the per-axis clamp untouched but is
/// `Malformed` on the wire. Before this fix, `MuxPane::new` / `resize`
/// validated ONLY the per-axis domain, so a pane could be created/resized
/// to dimensions whose own initial segment (or a later resize segment) its
/// own peer would reject when decoding the resulting snapshot — the
/// producer's accepted domain and the decoder's accepted domain had
/// drifted apart even though both are meant to share one contract.
///
/// When the per-axis-clamped product still exceeds the ceiling, `rows` is
/// reduced to fit under it (preserving `cols`, since column count is
/// usually the more consequential axis for terminal rendering) rather than
/// rejecting the resize outright — mirroring `clamp_resize_dims`'s own
/// "clamp, don't reject" policy so callers never need special-case error
/// handling for an oversized resize request.
///
/// D3''''' (round-8 rework, review round-7 finding `1d1b6b6297e3b6a0`):
/// `pub(crate)` so `tabs.rs` (the GUI client side, same crate) can apply the
/// IDENTICAL clamp before resizing its own `TerminalCore` and before sending
/// the `Resize` control message — a deterministic, PURE function of
/// `(cols, rows)` shared by both ends means the client's core and the
/// daemon's PTY always agree on the accepted dimensions without any wire
/// round-trip acknowledgment: same function, same input, same output.
pub(crate) fn clamp_dims_to_wire_domain(cols: u16, rows: u16) -> (u16, u16) {
    let (cols, rows) = term_core::terminal_core::clamp_resize_dims(cols, rows);
    if (cols as u32) * (rows as u32) <= PRODUCER_SEGMENT_CELL_BUDGET {
        return (cols, rows);
    }
    let max_rows = (PRODUCER_SEGMENT_CELL_BUDGET / cols as u32).max(1) as u16;
    (cols, rows.min(max_rows))
}

/// The largest number of `DimSegment`s a single daemon-assembled snapshot
/// can ever contain: every surviving `dim_markers` entry
/// ([`MAX_DIM_MARKERS`]), plus at most one synthesized head segment for a
/// single cap eviction (`ScrollbackRingBuffer::read_segments`, D1'''''),
/// plus at most one trailing screen-dump segment for an alt-screen pane
/// (`build_snapshot_bytes_with_layout`'s D7'' segment).
///
/// D1'''''' (round-9 rework, review round-8 finding `6082de4e619d7f51`):
/// with `MAX_DIM_MARKERS` raised to 62, this now evaluates to EXACTLY the
/// wire decoder's own `mux_ipc::protocol::MAX_SEGMENTS` (64) ceiling, not
/// comfortably under it — 62 was chosen as `MAX_SEGMENTS - 2` specifically
/// so the largest daemon-producible segment list exactly saturates, never
/// exceeds, what the decoder accepts. `decode_snapshot_payload_typed`
/// rejects only `count > MAX_SEGMENTS` (strictly greater), so a `count ==
/// MAX_SEGMENTS` payload still decodes as `Structured` — see
/// `largest_daemon_producible_segment_list_round_trips_cleanly` and
/// `largest_real_producer_segment_list_round_trips_cleanly` (the latter
/// drives the REAL ring → snapshot → encode → decode path, not just this
/// constant, closing review round-8 finding `45033eaafbdf8e25`).
const MAX_DAEMON_SNAPSHOT_SEGMENTS: u64 = MAX_DIM_MARKERS as u64 + 2;

/// Producer-side per-segment cell budget (D4''''', round-8 rework, review
/// round-7 finding `4bc6ab813edd6d22`, independently confirmed by
/// `codex:architecture`).
///
/// `clamp_dims_to_wire_domain` previously bounded every segment's own
/// product to the decoder's PER-SEGMENT ceiling
/// (`mux_ipc::protocol::MAX_SEGMENT_CELLS`, 1,000,000) alone — but the
/// decoder ALSO enforces a CUMULATIVE ceiling across every segment in one
/// payload (`MAX_CUMULATIVE_SEGMENT_CELLS`). A daemon snapshot can
/// legitimately carry up to [`MAX_DAEMON_SNAPSHOT_SEGMENTS`] segments, each
/// individually within `MAX_SEGMENT_CELLS` but at `1,000,000` cells apiece
/// that would sum to far past the cumulative ceiling, so the daemon's own
/// peer would reject the ENTIRE snapshot as `Malformed` and the pane would
/// keep showing stale content after every switch (the exact symptom this
/// feature exists to fix). Deriving the producer's per-segment budget from
/// the SAME cumulative ceiling divided by the largest segment count the
/// daemon can ever emit guarantees a daemon-recorded snapshot's segments
/// can never sum past it, regardless of how many distinct dimensions were
/// recorded. `.min(...)` with the decoder's own per-segment ceiling is a
/// belt-and-braces bound (the derived value is already well under it).
///
/// D1'''''' (round-9 rework, review round-8 findings `6082de4e619d7f51` /
/// `45033eaafbdf8e25`): `MAX_DAEMON_SNAPSHOT_SEGMENTS` rose from 26 to 64
/// alongside the marker cap raise, which — left against the OLD
/// `MAX_CUMULATIVE_SEGMENT_CELLS` (8,000,000) — would have shrunk this
/// derived budget from 307,692 to 125,000, a real regression for a large
/// display at a small font (`MAX_SEGMENT_CELLS`'s own doc estimates "a few
/// hundred thousand cells" for that case — already above 125,000).
/// `mux_ipc::protocol::MAX_CUMULATIVE_SEGMENT_CELLS` was raised in lockstep
/// (32,000,000, its own doc) so this derived budget comes out to 500,000 —
/// ABOVE the pre-round-9 value, not below it. See
/// `producer_segment_cell_budget_fits_a_real_large_terminal` for an
/// executable check that a realistically large terminal size still fits
/// unclamped (AC-2).
const PRODUCER_SEGMENT_CELL_BUDGET: u32 = {
    let derived = mux_ipc::protocol::MAX_CUMULATIVE_SEGMENT_CELLS / MAX_DAEMON_SNAPSHOT_SEGMENTS;
    let capped = if derived > mux_ipc::protocol::MAX_SEGMENT_CELLS as u64 {
        mux_ipc::protocol::MAX_SEGMENT_CELLS as u64
    } else {
        derived
    };
    capped as u32
};

/// Control sequence that switches a `vt100` shadow parser onto the
/// alternate screen (DECSET 1049), fed ahead of a restored alt-screen
/// dump by [`MuxPane::from_restored`] (mux-hot-upgrade-alt-screen
/// task0002, SPEC FR6).
const ALT_SCREEN_ENTER: &[u8] = b"\x1b[?1049h";

/// Enforce the D1 alt-screen dump size cap (IMPLEMENTATION.md,
/// mux-hot-upgrade-alt-screen): a `dump` whose length exceeds
/// `mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD` (16 MiB minus the frame
/// header — the largest a dump could ever be delivered to a client through
/// the reattach snapshot anyway) is replaced with an empty one; a dump at
/// or under the cap is returned untouched. `pane_id` is only used for the
/// warn-level log line the D1 policy requires (AC-7) — this function does
/// not touch the pane itself, which is what keeps the cap DECISION (as
/// opposed to the shadow-parser read that produces `dump`) unit-testable
/// against a synthetic byte vector, without allocating a real 16 MiB
/// screen (task plan Test Notes AC-7).
fn cap_alt_screen_dump(pane_id: PaneId, dump: Vec<u8>) -> Vec<u8> {
    if dump.len() <= mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD {
        return dump;
    }
    log::warn!(
        "upgrade snapshot: pane {} alt-screen dump ({} bytes) exceeds the D1 cap \
         ({} bytes); storing an empty dump with the alt-screen flag preserved true",
        pane_id,
        dump.len(),
        mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD,
    );
    Vec::new()
}

impl MuxPane {
    /// Create a new pane (PTY spawn handled by caller).
    ///
    /// `child` is the shell child-process handle `spawn_command` returned
    /// (task0001 SPEC FR2), retained for the pane's lifetime and reaped on
    /// [`Self::mark_exited`]. `None` for panes constructed without spawning
    /// a command (`new_test` / `new_test_with_writer`, and direct-PTY test
    /// call sites in this module's own tests).
    pub fn new(
        id: PaneId,
        cols: u16,
        rows: u16,
        output_target: SharedOutputTarget,
        writer: Box<dyn Write + Send>,
        master: Box<dyn MasterPty + Send>,
        child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    ) -> Self {
        // task0004 round-4 rework (review round-3 finding `b546481e9c2fcc85`):
        // validate the pane's INITIAL dimensions against the SAME domain
        // `resize()` enforces before recording them anywhere. Without this,
        // a caller passing an out-of-domain size would have its initial
        // segment silently clamped/rejected only at REPLAY time,
        // misaligning the earliest retained content's attributed
        // dimensions from what was actually recorded here.
        //
        // D6'''' (round-7 rework, review round-6 finding `6cefb1dd16c126b6`):
        // `clamp_dims_to_wire_domain` also bounds the PRODUCT to what the
        // wire decoder accepts (`MAX_SEGMENT_CELLS`), not just each axis —
        // see its doc for why the per-axis clamp alone let this drift.
        let (clamped_cols, clamped_rows) = clamp_dims_to_wire_domain(cols, rows);
        // D3''''' (round-8 rework, review round-7 finding `1d1b6b6297e3b6a0`,
        // AC-5): `master` was already opened by the caller (`spawn_pty`) at
        // the ORIGINAL, possibly out-of-domain `cols`/`rows` — BEFORE the
        // clamp above runs. Without this, the recorded fields / shadow
        // parser / initial segment would describe dimensions the PTY itself
        // does not actually have (pane creation recording a value the PTY
        // never had). Resize the PTY to the accepted dims whenever the
        // clamp actually changed something, so it can never disagree with
        // what gets recorded below.
        if (clamped_cols, clamped_rows) != (cols, rows) {
            if let Err(e) = master.resize(portable_pty::PtySize {
                rows: clamped_rows,
                cols: clamped_cols,
                pixel_width: 0,
                pixel_height: 0,
            }) {
                log::warn!(
                    "MuxPane::new: failed to resize PTY {} to clamped dims \
                     {}x{} (requested {}x{}): {}",
                    id,
                    clamped_cols,
                    clamped_rows,
                    cols,
                    rows,
                    e
                );
            }
        }
        // D4'''''' (round-9 rework, review round-8 finding
        // `7be271b2ead1bf07`, independently confirmed by
        // `codex:architecture`): read the PTY's ACTUAL size back rather than
        // trusting `clamped_cols`/`clamped_rows` unconditionally. Before this
        // fix, a `master.resize()` failure above was only logged — the PANE
        // still recorded the clamped values as if the PTY had reached them,
        // so every byte the child subsequently produces would be attributed
        // to a size the PTY never actually had, reproducing the exact
        // resize-interleaved coordinate drift this whole feature exists to
        // fix. The sibling `Self::resize` rolls `self.dims` back to the
        // PTY's last-known-good size on the same failure (D7'', task0005);
        // this constructor has no "last known good" to roll back to, so it
        // asks the PTY directly instead. `get_size()` failing too is
        // vanishingly unlikely immediately after a successful `openpty` —
        // fall back to the clamped values in that exceptional case (still
        // in-domain, unlike the caller's raw request).
        let (cols, rows) = match master.get_size() {
            Ok(actual) => (actual.cols, actual.rows),
            Err(e) => {
                log::error!(
                    "MuxPane::new: failed to query PTY {}'s actual size after \
                     a resize attempt; recording the clamped dims {}x{} \
                     on a best-effort basis: {}",
                    id,
                    clamped_cols,
                    clamped_rows,
                    e
                );
                (clamped_cols, clamped_rows)
            }
        };
        let mut scrollback = ScrollbackRingBuffer::new(DEFAULT_SCROLLBACK_CAPACITY);
        // IMPLEMENTATION.md D1/D2 (task0001): record the pane's INITIAL
        // dimensions as the very first scrollback bytes, mirroring what
        // `resize()` writes at every later transition. Without this, a
        // replay has no marker to resize into before the EARLIEST retained
        // segment (produced under THESE dims), reproducing the
        // resize-interleaved coordinate drift for that leading segment even
        // when every later resize is correctly marked.
        //
        // `write_resize_marker` (not a plain `write`) also records this
        // marker's offset so `ScrollbackRingBuffer::read_all` can
        // reconstruct it after ring wraparound evicts it — review round-1
        // rework, findings `81947e02402b5ace` / `ee93d8be8823e5d7`.
        scrollback.write_resize_marker(cols, rows);
        Self {
            id,
            cols,
            rows,
            output_target,
            writer: Some(Arc::new(StdMutex::new(writer))),
            master: Some(master),
            child: child.map(PaneChild::Owned),
            exited: false,
            shadow_parser: Arc::new(StdMutex::new(new_shadow_parser(rows, cols))),
            cwd: Arc::new(StdMutex::new(None)),
            title: Arc::new(StdMutex::new(None)),
            title_sender: Arc::new(StdMutex::new(None)),
            notification_sender: Arc::new(StdMutex::new(None)),
            agent_status: Arc::new(StdMutex::new(AgentStatus::default())),
            agent_status_report_sender: Arc::new(StdMutex::new(None)),
            agent_waiters: Arc::new(StdMutex::new(Vec::new())),
            agent_status_exit_latch: Arc::new(StdMutex::new(AgentStatusExitLatch::new())),
            raw_passthrough: Arc::new(StdMutex::new(RawPassthroughBuffer::new(
                HIDDEN_PASSTHROUGH_CAPACITY_MUX,
            ))),
            passthrough_scanner: Arc::new(StdMutex::new(PassthroughScanner::new())),
            scrollback: Arc::new(StdMutex::new(scrollback)),
            dims: Arc::new(PaneDims::new(cols, rows)),
        }
    }

    /// Create a new pane whose child is referenced by a raw process id
    /// rather than an owned handle (task plan task0007, IMPLEMENTATION.md
    /// D6): a pane restored from a handoff, whose PTY-library child handle
    /// did not survive the process replacement. Unix only, mirroring
    /// [`PaneChild::ProcessId`]'s own gating.
    ///
    /// Identical to [`Self::new`] in every other respect (dimension
    /// clamping, initial scrollback marker, and every other field) — only
    /// the child reference differs, so a restored pane and a freshly
    /// spawned pane are indistinguishable to the rest of the daemon.
    #[cfg(unix)]
    pub fn new_with_process_id(
        id: PaneId,
        cols: u16,
        rows: u16,
        output_target: SharedOutputTarget,
        writer: Box<dyn Write + Send>,
        master: Box<dyn MasterPty + Send>,
        pid: u32,
    ) -> Self {
        let mut pane = Self::new(id, cols, rows, output_target, writer, master, None);
        pane.child = Some(PaneChild::ProcessId(pid));
        pane
    }

    /// Write input data to the PTY.
    pub fn write_input(&self, data: &[u8]) -> std::io::Result<()> {
        let writer = self
            .writer
            .as_ref()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "writer closed"))?;
        write_via_writer_handle(writer, data)
    }

    /// Clone this pane's PTY-writer handle, if the pane still has one
    /// (`None` once [`MuxPane::mark_exited`] has run).
    ///
    /// task0011 lock hygiene: callers that need to perform a synchronous
    /// PTY write WITHOUT holding the `SessionManager` lock for the
    /// duration (e.g. `handle_send_text`) resolve the pane under the
    /// manager lock, clone this handle, release the manager lock, and
    /// write through the cloned handle via [`write_via_writer_handle`].
    /// Because the clone is an `Arc` over the SAME `std::sync::Mutex` as
    /// `self.writer`, concurrent writes — whether via `write_input` or via
    /// a cloned handle — still serialize on that mutex for the full
    /// write+flush, preserving atomic-per-request write semantics.
    pub fn writer_handle(&self) -> Option<Arc<StdMutex<Box<dyn Write + Send>>>> {
        self.writer.clone()
    }

    /// The underlying PTY's ACTUAL current size, as the kernel reports it —
    /// not `self.cols`/`self.rows` (the daemon's own recorded bookkeeping).
    /// AC-5 regression test observer (D3''''', round-8 rework, review
    /// round-7 finding `1d1b6b6297e3b6a0`): lets a test confirm `MuxPane::new`
    /// actually resized the PTY to match the clamped dims it records, rather
    /// than recording a value the PTY never had. `None` once the pane's
    /// master has been dropped ([`Self::mark_exited`]).
    #[cfg(test)]
    fn master_size(&self) -> Option<portable_pty::PtySize> {
        self.master.as_ref().and_then(|m| m.get_size().ok())
    }

    /// Whether this pane currently holds a child handle (task0001 TS-2
    /// observer): `true` from construction with `Some(child)` until
    /// [`Self::mark_exited`] takes it.
    #[cfg(test)]
    fn has_child(&self) -> bool {
        self.child.is_some()
    }

    /// Resize the PTY to the given dimensions.
    ///
    /// task0003 D2 (review round-2 finding `602e685494248cbb`): `cols` /
    /// `rows` are clamped to the domain the marker decoder accepts BEFORE
    /// anything else — so the PTY's actual size, the shadow parser, and the
    /// recorded marker can never disagree with what a replay is willing to
    /// honor. D6'''' (round-7 rework, review round-6 finding
    /// `6cefb1dd16c126b6`): that domain is `clamp_dims_to_wire_domain`'s,
    /// not `clamp_resize_dims`'s alone — see its doc.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), String> {
        let (cols, rows) = clamp_dims_to_wire_domain(cols, rows);
        let master = self
            .master
            .as_ref()
            .ok_or_else(|| "PTY master closed".to_string())?;
        // IMPLEMENTATION.md D1/D2 (task0001), structural since task0004
        // round-4 rework D1': record a resize segment into the scrollback
        // ring's `dim_markers` side channel (`write_resize_marker`) so a
        // later replay resizes its core to match the dimensions the
        // FOLLOWING bytes were produced for. This is the fix for the
        // resize-interleaved scrollback replay coordinate drift (PROBE D,
        // `tmp/apt-progress-bar-regression-2026-07-09.md`): without it, a
        // scrollback recording spanning a resize replays into a core fixed
        // at one row count and misinterprets DECSTBM / CUP coordinates
        // recorded for the other, mixing content from two logical output
        // lines onto one row. Recorded directly into `self.scrollback`'s
        // `dim_markers` (not fed through the PTY reader thread) since it is
        // synthesized here, not real PTY output — no bytes are written, so
        // there is nothing for PTY-sourced content to collide with (see
        // `ScrollbackRingBuffer::read_segments`). Only recorded when the
        // dimensions actually change, so a redundant Resize message does
        // not pollute the stream.
        //
        // Ordering (review round-1 rework, finding `83bed291fb779f52`,
        // high): `master.resize()` triggers SIGWINCH, and the PTY reader
        // thread can observe the child's response and try to append it to
        // `self.scrollback` concurrently. Pre-fix, `master.resize()` ran
        // OUTSIDE any lock, leaving a real window for the reader thread to
        // win the scrollback lock and record post-resize bytes BEFORE the
        // marker — replay would then interpret them under the OLD
        // dimensions, reproducing the exact coordinate drift this marker
        // exists to prevent. Holding `self.scrollback`'s lock across BOTH
        // the PTY-visible resize and the marker write establishes a single
        // ordering owner: the reader thread needs this SAME lock to append
        // anything, so it cannot record a single byte until AFTER the
        // marker is in place.
        if self.cols != cols || self.rows != rows {
            let mut scrollback = self.scrollback.lock().unwrap();
            // task0004 round-4 rework (review round-3 finding
            // `5ac1a5171a1e6a58`): publish the new dims via `self.dims`
            // BEFORE `master.resize()` sends SIGWINCH — still inside this
            // same scrollback-locked section. `master.resize()` is what
            // makes the child observe the new size and start producing
            // output at it; if the reader thread's `read()` returns that
            // output and calls `PaneDims::get()` in the gap BETWEEN
            // `master.resize()` and the OLD placement of this `set()` call
            // (after resize + marker write), it would see the OLD dims and
            // misattribute genuinely-new-size content via
            // `ScrollbackRingBuffer::attribute_write`'s correction path —
            // the opposite of what that path exists to prevent. Publishing
            // first closes that window: by the time the child could
            // possibly react to SIGWINCH, `PaneDims` already reports the
            // size the reaction was produced under.
            let (old_cols, old_rows) = (self.cols, self.rows);
            self.dims.set(cols, rows);
            if let Err(e) = master.resize(portable_pty::PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            }) {
                // D7'' (task0005 rework, review round-4 finding
                // `ef9ab1689853785c`, medium): `master.resize()` failing
                // means the PTY never actually changed size — but `self.dims`
                // was already published above (needed to close the OTHER
                // race this ordering fixes). Left as-is, `PaneDims` would
                // keep reporting a size the PTY was never at; the reader
                // thread reads it on every chunk and hands it to
                // `ScrollbackRingBuffer::attribute_write`, which — seeing a
                // mismatch against the ring's last-recorded dims — would
                // record a CORRECTIVE marker for those bogus dims, and every
                // later chunk in this pane's scrollback would be attributed
                // to a size that never existed. Roll `self.dims` back to the
                // size the PTY still actually has (still inside this same
                // scrollback-locked section, so no reader-thread read can
                // observe the bogus dims and misattribute against them
                // between the `set` above and this rollback) before
                // returning the error. `self.cols`/`self.rows` were never
                // updated on this path (still assigned after this whole
                // `if`/`else`), so they already agree with the rollback.
                self.dims.set(old_cols, old_rows);
                return Err(format!("PTY resize failed: {}", e));
            }
            scrollback.write_resize_marker(cols, rows);
        } else {
            master
                .resize(portable_pty::PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| format!("PTY resize failed: {}", e))?;
        }
        self.cols = cols;
        self.rows = rows;
        let mut parser = lock_shadow_parser(&self.shadow_parser);
        let resized = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            parser.screen_mut().set_size(rows, cols);
        }));
        if resized.is_err() {
            // vt100 panicked mid-resize; its internal state may be torn.
            // Rebuild so subsequent output re-populates the shadow screen.
            *parser = new_shadow_parser(rows, cols);
            log::error!(
                "pane {}: shadow parser panicked during resize; parser reset",
                self.id
            );
        }
        Ok(())
    }

    /// Apply an ACCEPTED agent-status report (SPEC FR3): update
    /// state/name and increment revision. Returns the resulting revision.
    ///
    /// Callers must only invoke this with an `AgentStatusEvent` that
    /// `agent_status::parse` already returned `Some` for — a rejected
    /// (`None`) parse must never reach here, which is what leaves state and
    /// revision untouched on rejection (AC-2).
    ///
    /// task0003 (SPEC FR1): every accepted event also feeds this pane's
    /// inferred-clear latch — `Set` arms it (new generation), `Clear`
    /// disarms it. This is the SINGLE place an explicit Set/Clear reaches
    /// the latch, so [`Self::record_live_osc133_mark`] (the live OSC 133
    /// feed) and this method can never drift out of sync with each other.
    pub fn apply_agent_status_event(&self, event: AgentStatusEvent) -> u64 {
        let is_clear = matches!(event, AgentStatusEvent::Clear);
        let revision = {
            let mut status = self.agent_status.lock().unwrap();
            match event {
                AgentStatusEvent::Set { state, name } => {
                    status.state = Some(state);
                    status.name = name;
                }
                AgentStatusEvent::Clear => {
                    status.state = None;
                    status.name = None;
                }
            }
            status.revision += 1;
            status.revision
        };
        let mut latch = self.agent_status_exit_latch.lock().unwrap();
        if is_clear {
            latch.record_clear();
        } else {
            latch.record_set();
        }
        revision
    }

    /// Record a live, main-screen-observed OSC 133 mark for this pane
    /// (task0001 `AgentStatusExitLatch`, SPEC FR1/FR2) and, if it completes
    /// an armed `D`→`A` transition, apply the resulting inferred clear
    /// through the EXACT SAME path an explicit `Clear` report takes
    /// ([`Self::apply_agent_status_event`]) — same revision increment,
    /// same downstream broadcast the caller drives from the returned
    /// revision, no parallel clear logic (FR2).
    ///
    /// Callers (the daemon's live PTY reader path for this pane) must only
    /// ever pass marks that are LIVE and main-screen-observed, in true
    /// arrival order relative to this pane's Set/Clear reports (FR4/FR5) —
    /// this method has no way to tell a live mark from a replayed one, so
    /// that guarantee is entirely the caller's responsibility.
    ///
    /// Returns `Some(revision)` when an inferred clear fired; `None` when
    /// the mark produced no state change (AC-2/AC-3: e.g. an `A` with no
    /// preceding `D`, or any mark while disarmed).
    pub fn record_live_osc133_mark(&self, kind: PromptMarkKind) -> Option<u64> {
        let fired = {
            let mut latch = self.agent_status_exit_latch.lock().unwrap();
            latch.record_mark(kind)
        };
        if fired {
            Some(self.apply_agent_status_event(AgentStatusEvent::Clear))
        } else {
            None
        }
    }

    /// Mark PTY as exited (task0001 SPEC FR3/FR4, task plan D2; task plan
    /// task0007, IMPLEMENTATION.md D6).
    ///
    /// Clears the writer/master (dropping the master delivers the hangup to
    /// the shell) and — when a child reference is present — takes it out of
    /// the pane and hands it off to the matching [`child_reaper`] entry
    /// point for a bounded, off-thread reap: an owned handle goes through
    /// [`child_reaper::spawn_reaper`] (unchanged, task0001), a process id
    /// (a pane restored from a handoff) goes through
    /// [`child_reaper::spawn_reaper_pid`] (task0007). Taking the reference
    /// here is the multi-call gate: a second `mark_exited` (concurrent
    /// teardown paths racing, e.g. destroy-pane racing graceful-shutdown, or
    /// the PTY-EOF reap racing an explicit destroy) finds no reference and
    /// starts no second reap.
    ///
    /// Returns immediately: performs no waiting of any kind, so callers
    /// holding the `SessionManager` lock are never blocked on the child's
    /// exit (NFR1).
    pub fn mark_exited(&mut self) {
        self.exited = true;
        self.writer = None;
        self.master = None;
        match self.child.take() {
            Some(PaneChild::Owned(child)) => {
                child_reaper::spawn_reaper(self.id, child);
            }
            #[cfg(unix)]
            Some(PaneChild::ProcessId(pid)) => {
                child_reaper::spawn_reaper_pid(self.id, pid);
            }
            None => {}
        }
    }

    /// Raw fd number of this pane's PTY master (task0003 AC-2 snapshot
    /// accessor): the daemon clears its `FD_CLOEXEC` flag and records this
    /// number so the descriptor survives `execve`. `None` once the master
    /// has been dropped ([`Self::mark_exited`]) or for a pane with no
    /// master at all (test-only construction).
    #[cfg(unix)]
    pub fn master_raw_fd(&self) -> Option<std::os::unix::io::RawFd> {
        self.master.as_ref().and_then(|m| m.as_raw_fd())
    }

    /// This pane's shell child process id (task0003 AC-2 / IMPLEMENTATION.md
    /// D6 snapshot accessor), regardless of whether the child is an owned
    /// handle ([`PaneChild::Owned`], a freshly spawned pane) or a bare
    /// process id ([`PaneChild::ProcessId`], a pane itself previously
    /// restored from an earlier handoff). `None` once the child has been
    /// reaped ([`Self::mark_exited`]) or for a pane with no child reference
    /// at all (test-only construction).
    pub fn child_pid(&self) -> Option<u32> {
        match &self.child {
            Some(PaneChild::Owned(child)) => child.process_id(),
            #[cfg(unix)]
            Some(PaneChild::ProcessId(pid)) => Some(*pid),
            None => None,
        }
    }

    /// Capture this pane's alternate-screen state for the hot-upgrade
    /// handoff document (mux-hot-upgrade-alt-screen task0002, SPEC FR5/FR7/
    /// FR8): `(alt_screen, dump)`, where `dump` is the shadow parser's
    /// formatted alternate-screen contents when `alt_screen` is true, empty
    /// otherwise.
    ///
    /// Implements the SAME main/alt split `build_snapshot_bytes` already
    /// uses (`crate::mux::snapshot_bytes`): under the shadow-parser lock,
    /// read whether the parser is on the alternate screen, and only take
    /// the (expensive) `contents_formatted()` dump when it is. This is the
    /// ONE helper both `snapshot_pane` (initial capture) and
    /// `refresh_live_agent_state` (task0006 re-capture) call, so the split
    /// logic never forks between the two call sites.
    ///
    /// The D1 size cap (IMPLEMENTATION.md) is enforced here: a dump whose
    /// length exceeds `mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD` is
    /// replaced with an empty one — the flag stays true (restore still
    /// enters the alternate screen, just blank — IMPLEMENTATION.md D1) — and
    /// a warn-level log line names the pane id and the oversize length
    /// (AC-7).
    ///
    /// Callers are responsible for the "exited pane contributes flag false"
    /// rule (task plan Design "Capture"): this method reports the shadow
    /// parser's CURRENT state regardless of `self.exited`, since an exited
    /// pane's parser reflects whatever it last observed, which is not the
    /// contract callers want for an exited pane's document entry.
    pub fn capture_alt_state(&self) -> (bool, Vec<u8>) {
        let (alt_screen, dump) = {
            let parser = lock_shadow_parser(&self.shadow_parser);
            let alt = parser.screen().alternate_screen();
            let dump = if alt {
                parser.screen().contents_formatted()
            } else {
                Vec::new()
            };
            (alt, dump)
        };
        if !alt_screen {
            return (false, Vec::new());
        }
        (true, cap_alt_screen_dump(self.id, dump))
    }

    /// Construct a live pane around an adopted master, for restore from a
    /// handoff document (task0003 AC-1/AC-3/AC-4/AC-5). Builds on
    /// [`Self::new_with_process_id`] / [`Self::new`] for every field their
    /// construction already gets right (dimension clamping, child
    /// reference, the wire-up new panes get) and then overwrites `cwd` /
    /// `title` / agent-status / scrollback with the restored values instead
    /// of the empty ones those constructors start with. The shadow parser's
    /// view is rebuilt by REPLAYING the restored scrollback
    /// (IMPLEMENTATION.md D8) rather than expecting serialised parser
    /// state — mirroring the panic-recovery convention `pty_reader_loop`
    /// already uses for live output.
    ///
    /// `alt_screen` / `alt_screen_dump` (mux-hot-upgrade-alt-screen
    /// task0002, SPEC FR6): after the scrollback replay above, when
    /// `alt_screen` is true, the alternate-screen-enter control sequence
    /// (`ESC [?1049h`) followed by `alt_screen_dump` is fed into the SAME
    /// shadow parser under the SAME panic guard as the scrollback replay —
    /// so a formerly-alt-screen pane's parser reports the alternate screen
    /// as active again, with the dump's content on it and the replayed
    /// scrollback beneath. `alt_screen == false` feeds nothing extra:
    /// byte-identical to the pre-task0002 behavior. `alt_screen == true`
    /// with an empty `alt_screen_dump` (the D1 overflow shape) still enters
    /// the alternate screen, just blank.
    #[cfg(unix)]
    #[allow(clippy::too_many_arguments)]
    pub fn from_restored(
        id: PaneId,
        cols: u16,
        rows: u16,
        output_target: SharedOutputTarget,
        writer: Box<dyn Write + Send>,
        master: Box<dyn MasterPty + Send>,
        scrollback: ScrollbackRingBuffer,
        cwd: Option<String>,
        title: Option<String>,
        agent_status: AgentStatus,
        restored_child_pid: Option<u32>,
        alt_screen: bool,
        alt_screen_dump: Vec<u8>,
    ) -> Self {
        let pane = match restored_child_pid {
            Some(pid) => {
                Self::new_with_process_id(id, cols, rows, output_target, writer, master, pid)
            }
            None => Self::new(id, cols, rows, output_target, writer, master, None),
        };

        *pane.cwd.lock().unwrap() = cwd;
        *pane.title.lock().unwrap() = title;
        *pane.agent_status.lock().unwrap() = agent_status;

        let replay_bytes = scrollback.read_all();
        *pane.scrollback.lock().unwrap() = scrollback;
        if !replay_bytes.is_empty() {
            let mut parser = lock_shadow_parser(&pane.shadow_parser);
            let (parser_rows, parser_cols) = parser.screen().size();
            let processed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                parser.process(&replay_bytes);
            }));
            if processed.is_err() {
                *parser = new_shadow_parser(parser_rows, parser_cols);
                log::error!(
                    "pane {}: shadow parser panicked while replaying {} bytes of \
                     restored scrollback; parser reset",
                    id,
                    replay_bytes.len()
                );
            }
        }

        if alt_screen {
            let mut parser = lock_shadow_parser(&pane.shadow_parser);
            let (parser_rows, parser_cols) = parser.screen().size();
            let mut alt_bytes = Vec::with_capacity(ALT_SCREEN_ENTER.len() + alt_screen_dump.len());
            alt_bytes.extend_from_slice(ALT_SCREEN_ENTER);
            alt_bytes.extend_from_slice(&alt_screen_dump);
            let processed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                parser.process(&alt_bytes);
            }));
            if processed.is_err() {
                *parser = new_shadow_parser(parser_rows, parser_cols);
                log::error!(
                    "pane {}: shadow parser panicked while replaying {} bytes of \
                     restored alt-screen state; parser reset",
                    id,
                    alt_bytes.len()
                );
            }
        }

        pane
    }

    /// Construct an already-exited pane directly from restored parts
    /// (task0003 AC-6/AC-7): no descriptor is adopted — either the
    /// document recorded no live descriptor (AC-6), or restore could not
    /// adopt the one it recorded (AC-7) — and no child reference is
    /// retained, since an exited pane has nothing left to reap.
    pub fn from_restored_exited(
        id: PaneId,
        cols: u16,
        rows: u16,
        output_target: SharedOutputTarget,
        scrollback: ScrollbackRingBuffer,
        cwd: Option<String>,
        title: Option<String>,
        agent_status: AgentStatus,
    ) -> Self {
        Self {
            id,
            cols,
            rows,
            output_target,
            writer: None,
            master: None,
            child: None,
            exited: true,
            shadow_parser: Arc::new(StdMutex::new(new_shadow_parser(rows, cols))),
            cwd: Arc::new(StdMutex::new(cwd)),
            title: Arc::new(StdMutex::new(title)),
            title_sender: Arc::new(StdMutex::new(None)),
            notification_sender: Arc::new(StdMutex::new(None)),
            agent_status: Arc::new(StdMutex::new(agent_status)),
            agent_status_report_sender: Arc::new(StdMutex::new(None)),
            agent_waiters: Arc::new(StdMutex::new(Vec::new())),
            agent_status_exit_latch: Arc::new(StdMutex::new(AgentStatusExitLatch::new())),
            raw_passthrough: Arc::new(StdMutex::new(RawPassthroughBuffer::new(
                HIDDEN_PASSTHROUGH_CAPACITY_MUX,
            ))),
            passthrough_scanner: Arc::new(StdMutex::new(PassthroughScanner::new())),
            scrollback: Arc::new(StdMutex::new(scrollback)),
            dims: Arc::new(PaneDims::new(cols, rows)),
        }
    }

    /// Create a pane without a PTY master, for testing only.
    #[cfg(test)]
    pub fn new_test(id: PaneId, cols: u16, rows: u16, output_target: SharedOutputTarget) -> Self {
        Self::new_test_with_writer(id, cols, rows, output_target, Box::new(std::io::sink()))
    }

    /// Like [`Self::new_test`], but with a caller-supplied writer so tests
    /// can capture exactly what `write_input` sends (e.g. `SendText`'s "no
    /// trailing newline added" contract, task0004 AC-2).
    #[cfg(test)]
    pub fn new_test_with_writer(
        id: PaneId,
        cols: u16,
        rows: u16,
        output_target: SharedOutputTarget,
        writer: Box<dyn Write + Send>,
    ) -> Self {
        Self {
            id,
            cols,
            rows,
            output_target,
            writer: Some(Arc::new(StdMutex::new(writer))),
            master: None,
            child: None,
            exited: false,
            shadow_parser: Arc::new(StdMutex::new(new_shadow_parser(rows, cols))),
            cwd: Arc::new(StdMutex::new(None)),
            title: Arc::new(StdMutex::new(None)),
            title_sender: Arc::new(StdMutex::new(None)),
            notification_sender: Arc::new(StdMutex::new(None)),
            agent_status: Arc::new(StdMutex::new(AgentStatus::default())),
            agent_status_report_sender: Arc::new(StdMutex::new(None)),
            agent_waiters: Arc::new(StdMutex::new(Vec::new())),
            agent_status_exit_latch: Arc::new(StdMutex::new(AgentStatusExitLatch::new())),
            raw_passthrough: Arc::new(StdMutex::new(RawPassthroughBuffer::new(
                HIDDEN_PASSTHROUGH_CAPACITY_MUX,
            ))),
            passthrough_scanner: Arc::new(StdMutex::new(PassthroughScanner::new())),
            scrollback: Arc::new(StdMutex::new(ScrollbackRingBuffer::new(
                DEFAULT_SCROLLBACK_CAPACITY,
            ))),
            dims: Arc::new(PaneDims::new(cols, rows)),
        }
    }
}

#[cfg(test)]
mod tests;
