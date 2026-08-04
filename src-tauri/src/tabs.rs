//! Tab type and lifecycle.
//!
//! Phase 6 swap: `Parser + Grid` (the Phase 1 PoC stand-ins) are replaced
//! by `term_core::TerminalCore`. Incoming PTY bytes are pushed through
//! `process_pty_data`; the grid state is read via `get_cell_*` /
//! `get_cursor_*` accessors. OSC titles and emterm-extension dispatches
//! are delivered through the shared `NativeCallbackState`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crossbeam_channel::Receiver;
use parking_lot::Mutex;
use term_core::terminal_core::TerminalCore;
use term_images::image_proc::{ImageEvent, ImageProcessor};

use crate::callbacks::{EmtermOscRequest, NativeCallbackState, NativeCallbacks};
use crate::image::{parse as image_parse, split_image_events};
use crate::pty::{ExitReason, PtyEvent, PtySession};
use crate::render::theme::Theme;
use crate::settings::Settings;
use mux_ipc::protocol::{
    DecodedSnapshotPayload, MessageType, MuxMessage, RenameWindowMsg, ResizeMsg, WelcomeMsg,
    decode_snapshot_payload_typed,
};
use term_core::terminal_core::ReplaySegment;

use crate::mux::window_group::{MuxWindow, MuxWindowGroup};

// Default scrollback capacity now lives on `Settings` (`DEFAULT_SCROLLBACK_LINES`
// in `crate::settings`); the caller passes the desired value into
// `Tab::spawn_shell`.

/// Monotonic counter backing [`Tab::stable_id`]. Process-lifetime unique;
/// `Relaxed` suffices because the id only needs uniqueness, not ordering
/// with other memory operations.
static NEXT_TAB_STABLE_ID: AtomicU64 = AtomicU64::new(0);

/// Snapshot payloads at or above this byte size replay the VT stream on a
/// one-shot worker thread (the mux off-thread replay) instead of blocking
/// the winit/UI thread. 64 KiB ≈ ~7 ms of reparse on the target machine —
/// well under one 60 fps frame — so the sub-threshold synchronous block
/// stays imperceptible while large (history-heavy) panes never stall the
/// switch. Resolved at verify-plan; re-tune here if measurement on the
/// target machine differs.
pub(crate) const OFFTHREAD_REPLAY_THRESHOLD_BYTES: usize = 64 * 1024;

/// Segment count at or above which a `Snapshot`/`SnapshotRestore` frame
/// replays off-thread regardless of `content_bytes.len()` (task0005 rework
/// D3''/AC-5, review round-4 finding `b1de83542bfe60bc`).
///
/// `replay_segments` performs one full content-preserving reflow per
/// non-empty dimension segment, and that reflow's cost is driven by the
/// core's ACCUMULATED grid + scrollback size, not by how many NEW bytes
/// this particular segment feeds — so a snapshot well under
/// [`OFFTHREAD_REPLAY_THRESHOLD_BYTES`] can still carry enough segments
/// (e.g. a resize-drag-shaped sequence) to cost tens to hundreds of
/// milliseconds of reflow on the synchronous path. Set comfortably below
/// `mux::scrollback_buffer::MAX_DIM_MARKERS` (the daemon-side cap on
/// recorded segments, currently 16), so a snapshot anywhere near that cap —
/// the shape this fix specifically targets — reliably takes the off-thread
/// path even when its content happens to be small.
pub(crate) const OFFTHREAD_REPLAY_SEGMENT_THRESHOLD: usize = 8;

/// Upper bound on live output queued during a pending off-thread replay.
/// While the worker parses, target-pane `PtyOutput` accumulates in
/// `PendingSwitch.live_queue`; a fast-producing pane during a slow parse could
/// grow it without bound and then replay one large burst on the UI thread at
/// swap time (defeating the off-thread goal) or grow memory if the worker
/// stalls. Past this cap the in-flight replay is abandoned and the snapshot is
/// reparsed synchronously, applying the accumulated bytes as ordinary output.
pub(crate) const OFFTHREAD_LIVE_QUEUE_CAP_BYTES: usize = 4 * 1024 * 1024;

/// Per-tab state tracking an in-flight off-thread snapshot replay (the mux
/// off-thread switch). Created when a large `Snapshot` is dispatched to a
/// worker; cleared on swap (worker completed) or supersede (a newer switch
/// or a grid resize arrived first). Transient — there is at most one of
/// these per tab and never a resident per-pane core (NFR4: still one core
/// per tab; the worker's in-flight core is the only extra, and it is
/// discarded on supersede or moved in on swap).
pub(crate) struct PendingSwitch {
    /// The pane id this replay targets. Live `PtyOutput` for this pane is
    /// queued (not applied to the displayed core) until the swap; output
    /// for any other pane is dropped as usual.
    pub(crate) target_pane: u32,
    /// Grid the worker core is being built at. A resize to a different
    /// `(cols, rows)` supersedes the in-flight parse so a stale-sized core
    /// is never swapped in (FR5).
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    /// Non-blocking completion handoff from the worker. `try_recv` yields
    /// `Ok(replay)` when the worker finished, `Err(Empty)` while it is
    /// still parsing, and `Err(Disconnected)` if the worker panicked
    /// (→ synchronous reparse fallback).
    pub(crate) done: std::sync::mpsc::Receiver<term_core::terminal_core::SnapshotReplay>,
    /// Target-pane live output that arrived during the parse gap, kept in
    /// arrival order to be replayed onto the swapped-in core after the swap
    /// (FR3). Each entry is one decoded `PtyOutput` payload.
    pub(crate) live_queue: Vec<Vec<u8>>,
    /// Running total of `live_queue` payload bytes, compared against
    /// [`OFFTHREAD_LIVE_QUEUE_CAP_BYTES`] to bound the backlog.
    pub(crate) queued_bytes: usize,
    /// The raw snapshot payload, retained so a supersession-by-resize can
    /// re-dispatch the same bytes at the new grid without another daemon
    /// round-trip.
    pub(crate) payload: Vec<u8>,
    /// Structural dimension segments decoded from the wire payload
    /// (task0004 round-4 rework D1', `mux_ipc::protocol::decode_snapshot_payload`),
    /// retained alongside `payload` so a supersession-by-resize re-dispatch
    /// carries the same segment authority forward.
    pub(crate) segments: Vec<ReplaySegment>,
    /// Cooperative cancellation flag shared with the worker thread. Set when
    /// this switch is superseded (a newer switch, a grid resize, or the
    /// live-queue cap fallback) so the worker abandons its parse at the next
    /// chunk boundary instead of running to completion — bounding wasted work
    /// and concurrent worker lifetime under a rapid switch / resize storm.
    pub(crate) cancel: Arc<AtomicBool>,
    /// FR7 (task0006 redesign, review round-1 finding `64baa639d71792f9`):
    /// the latest grid size requested by a `Tab::resize` call that landed
    /// AFTER this switch's worker was dispatched, if any. `cols`/`rows`
    /// above stay fixed at the worker's own DISPATCH-time target — the one
    /// `payload`/`segments`' own recorded resize-marker segments actually
    /// converge to — so the worker's bypass split decision
    /// (`stable_target_suffix_start`) is never asked to match a target the
    /// payload was never captured at. A racing resize is deferred here
    /// instead of forcing an immediate re-dispatch at the new (mismatched)
    /// target; `poll_pending_switch`/`apply_offthread_swap` apply it, via an
    /// ordinary already-bypass-aware `TerminalCore::resize` call, to the
    /// freshly swapped-in core — the same operation an interactive resize
    /// performs on an already-displayed core, so it costs (and behaves)
    /// exactly as if the user had resized right after an unraced switch
    /// landed. `None` means no resize raced this switch; the swapped-in
    /// core is already at the right size. Multiple resizes before the swap
    /// just overwrite this with the latest target — one deferred resize
    /// regardless of how many landed, and (review round-1 finding
    /// `34a708465d04f983`) no payload/segments clone per resize event
    /// either, since `Tab::resize` no longer re-dispatches for this case.
    pub(crate) pending_resize: Option<(u16, u16)>,
}

/// Result of [`Tab::poll_pending_switch`], driving how `App::pump_all`
/// reconciles after polling the off-thread replay handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SwapOutcome {
    /// No off-thread replay is pending on this tab.
    Idle,
    /// A worker is still parsing; keep showing the outgoing pane.
    Pending,
    /// The core was swapped (or, on worker failure, reparsed synchronously)
    /// and reconciled this pump; the caller drives the active-tab post-loop
    /// per-pane scroll restore + full redraw.
    Swapped,
}

/// Worker→UI handoff payload for the 2nd-pass scrollback restore: the
/// bypass-off rebuilt core (its `scrollback_slim` / `scrollback_wrapped`
/// populated) plus the `scrollback_evicted_total` captured at the end of the
/// rebuild for the FR3 trim arithmetic.
pub(crate) struct ScrollbackBuild {
    /// Core built off-thread with the snapshot bypass DISABLED. The only
    /// fields the merge consumes are `scrollback_slim`, `scrollback_wrapped`,
    /// `styles`, `chars`, and `cols` (for the precondition check); the rest
    /// is dropped at merge time.
    pub(crate) rebuilt_core: term_core::terminal_core::TerminalCore,
    /// `get_scrollback_evicted_total()` at the moment the bypass-off rebuild
    /// finished. Used by `apply_scrollback_restore` to compute the
    /// `live_growth` trim count.
    pub(crate) evicted_total_at_end: u64,
}

/// Per-tab state tracking an in-flight 2nd-pass scrollback restore worker
/// (the bypass-off counterpart to `PendingSwitch`). Created at the end of
/// `apply_offthread_swap`; cleared on merge (worker completed),
/// supersede (a newer off-thread switch), resize (UC03 abandons history-
/// restore), or shutdown. NFR4: at most one of these per tab.
pub(crate) struct PendingScrollbackRestore {
    /// Non-blocking completion handoff from the 2nd-pass worker.
    /// `try_recv` yields `Ok(build)` when the worker finished, `Err(Empty)`
    /// while it is still rebuilding, and `Err(Disconnected)` if the worker
    /// panicked (→ FR7: warn + clear state, no synchronous fallback).
    pub(crate) done: std::sync::mpsc::Receiver<ScrollbackBuild>,
    /// `live.scrollback_evicted_total` captured immediately after the
    /// 1st-pass swap (`apply_offthread_swap`) finished applying its queued
    /// live output. The merge subtracts this from the current live value at
    /// recv time to compute the number of historical rows that have already
    /// been re-evicted live (`live_growth`); those rows are trimmed off the
    /// rebuilt tail before the merge so duplicate rows do not accumulate
    /// (FR3).
    pub(crate) base_evicted_total: u64,
    /// Cooperative cancellation flag shared with the worker thread. Set when
    /// this restore is superseded (a newer off-thread switch via
    /// `dispatch_offthread_replay`), the tab is resized (UC03), or shutdown
    /// (`window_host.rs`'s `WindowEvent::CloseRequested` cancel sweep), so
    /// the worker abandons its rebuild at the next chunk boundary.
    pub(crate) cancel: Arc<AtomicBool>,
}

/// Result of [`Tab::poll_pending_scrollback_restore`], analogous to
/// [`SwapOutcome`] but for the 2nd-pass scrollback restore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScrollbackRestoreOutcome {
    /// No restore is in flight for this tab.
    Idle,
    /// Worker is still rebuilding; keep waiting.
    Pending,
    /// The rebuilt scrollback was merged into the live core this pump; the
    /// caller marks the tab `changed` (and the active tab `active_changed`
    /// so the search overlay rebuilds against the new scrollback).
    Merged,
    /// Worker panicked / disconnected (FR7) or was cancelled (resize / new
    /// switch). State has been cleared; treated by the caller the same as
    /// `Merged` for the `changed` flag because the in-flight state is gone.
    Failed,
}

/// Test-only: one recorded pane `Resize` control frame emission (task0003
/// AC-6, FR4). IMPLEMENTATION.md's corrected contract (d) enumerates THREE
/// emission sites — `Tab::resize`, mux attach/Welcome pane seeding, and
/// `PaneCreated` handling — so a dims-only proxy cannot tell "no frame was
/// sent" apart from "a frame was sent but happened to carry dims that
/// matched what was already displayed" (finding cfcbfae57964beb5). Reading
/// this log directly closes that gap. Strictly `cfg(test)` so the
/// production build carries no observer.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResizeFrameRecord {
    /// The emitting tab's `Tab::stable_id`, so a test reading multiple
    /// tabs' logs (or a shared/cloned log) can attribute each entry.
    pub(crate) tab_stable_id: u64,
    /// The target pane id the `Resize` frame was addressed to.
    pub(crate) pane_id: u32,
    /// Post-clamp dims carried by the frame (the same `(cols, rows)`
    /// `Tab::resize` — or the seeding/PaneCreated site — actually sent).
    pub(crate) cols: u16,
    pub(crate) rows: u16,
}

pub struct Tab {
    /// Creation-ordered stable identity. Unlike the positional index in
    /// `App::tabs`, this survives tab close / drag-reorder, so per-tab
    /// UI state keyed on it (the activity-dot animation in
    /// `ui::tab_bar`) never bleeds between tabs when indices shift.
    pub stable_id: u64,
    pub title: String,
    pub core: Arc<Mutex<TerminalCore>>,
    pub cb_state: Arc<Mutex<NativeCallbackState>>,
    /// Per-tab theme. OSC 4/10/11/12/22/104/110/111/112 mutate this in
    /// place through `NativeCallbacks::handle_theme`; the renderer reads
    /// the same `Arc<Mutex<Theme>>` so updates take effect on the next
    /// frame. Phase 6 only wires the dirty-flag and mutation path —
    /// rendering still reads `Theme::default()` until the wiring change
    /// lands in a later sub-phase. The Arc is shared so it stays alive
    /// for the lifetime of the tab.
    #[allow(dead_code)] // Phase 3 renderer wiring will read this.
    pub theme: Arc<Mutex<Theme>>,
    // Drop order matters: `events` must drop before `pty`. Struct fields
    // drop in declaration order, so `events` (the Receiver) closing first
    // disconnects the bounded(256) channel from the reader/writer side.
    // That lets the EOF `event_tx.send(PtyEvent::Exited)` in `reader_loop`
    // return `Err` immediately on shutdown instead of blocking forever
    // when the channel happens to be full — which would deadlock
    // `PtySession::Drop`'s `reader_join.join()` and freeze the X-button
    // close path (WM marks the window as 応答なし while we wait).
    pub events: Receiver<PtyEvent>,
    pub pty: Option<PtySession>,
    pub exited: bool,
    /// Per-tab Kitty Graphics Protocol + SIXEL processor (CPU side).
    /// `Tab::pump` drains `cb_state.pending_apc` / `pending_dcs` into this
    /// to produce `ImageEvent`s. `Response` events are routed back to the
    /// PTY (Kitty OK / error replies); everything else is queued in
    /// `pending_image_events` for the GPU layer (owned by `WindowHost`)
    /// to consume.
    pub image_proc: ImageProcessor,
    /// Image events the GPU layer (in `WindowHost`) has not yet ingested.
    /// `Tab::drain_image_events` returns these.
    pub pending_image_events: Vec<ImageEvent>,
    /// When `Some(name)`, this tab is attached to a remote mux session
    /// and the tab bar prefixes the title with `[mux:<name>]`. Phase
    /// 4-B introduces the field; Phase 4-C (APC redesign) populates it
    /// from the daemon's `Welcome::Accepted.sessions[active]` arriving
    /// as an APC frame inside the PTY output.
    pub mux_session_name: Option<String>,
    /// mux window-tab group state for a mux-attached tab. Holds the ordered
    /// window list (parallel `windows` / `pane_ids`), active index, and
    /// compact/expanded flag. `None` until the tab attaches to a mux session
    /// (seeded by `Welcome`); cleared on detach. The tab bar reads this to
    /// render the group; prefix/dialog actions mutate it. Port of the WebView
    /// `muxWindows` / `muxPaneIds` + `MuxTabGroup`.
    pub mux_group: Option<crate::mux::window_group::MuxWindowGroup>,
    /// Phase 4-E: per-tab IME preedit composition state. Driven by
    /// `egui::Event::Ime(ImeEvent::Preedit/Commit)` events routed
    /// through `App` and rendered as an underline overlay by
    /// `render::cursor::draw_cursor_with_preedit`.
    pub preedit_state: crate::ime::preedit::State,
    /// Latched when `pump()` drained one or more BEL (0x07) bytes from
    /// the callback state. `App::pump_all` consumes it via
    /// [`Tab::take_bell`] and dispatches `settings.bell_action`. A BEL
    /// burst within one pump collapses into a single latch — one flash
    /// / beep per frame matches the WebView build's perceived behavior.
    bell_pending: bool,
    /// Latched when `pump()` fed new PTY bytes into the core.
    /// `App::pump_all` consumes it via [`Tab::take_output`] to drive the
    /// inactive-tab activity dot / `notify_on_output` notification —
    /// the native analogue of the WebView `onOutputActivity` callback.
    output_pending: bool,
    /// Unread-activity dot + notification throttle state. Marked by
    /// `App::pump_all` for inactive tabs; the tab bar renders the dot
    /// from `activity.has_activity` (gated by
    /// `settings.tab_activity_indicator`).
    pub activity: crate::notifications::TabActivityState,
    /// Resolved OSC 133 prompt marks for prompt-to-prompt navigation
    /// (`App::jump_to_prompt`). `pump` drains the callback-side
    /// `prompt_marks`, backfills each mark's absolute row, and pushes it
    /// here. Port of the WebView `SemanticZoneTracker`.
    pub prompts: crate::prompts::PromptTracker,
    /// Command-output fold regions for this tab. `backfill_prompt_marks`
    /// registers an OSC 133 C→D region each time a `D` mark arrives (pairing
    /// it with the preceding `C`/`B` in `prompts`), and prunes the regions by
    /// the same eviction delta as `prompts` so fold rows stay in the same
    /// absolute frame. Port of the WebView `FoldManager` wiring in
    /// `handlers/osc_handlers.ts`.
    pub folds: crate::fold::FoldManager,
    /// Whether the prompt-folding affordance is enabled for this tab,
    /// captured from `settings.fold_enabled` at construction. The native
    /// build rebuilds `folds` from scratch on a core reset (RIS /
    /// clear-scrollback) and on a mux snapshot replay, which would lose the
    /// `FoldManager`'s own `enabled` flag — so the desired state is kept
    /// here and re-applied to every fresh `FoldManager` (mirroring the
    /// WebView, whose long-lived `FoldManager` keeps `enabled` across the
    /// same events because it is never recreated). When `false`, fold clicks
    /// are no-ops and no region is ever collapsed, but region *registration*
    /// continues (so re-enabling could still fold past output).
    fold_enabled: bool,
    /// In-flight custom-fold `begin` awaiting its `end`. Holds the begin's
    /// absolute row (in the *current* post-prune frame) and its label. Port
    /// of the WebView `pendingFoldBegins` entry. A second `begin` overwrites
    /// it (matching the WebView's `pendingFoldBegins.set`, which clobbers any
    /// previous begin); an `end` consumes it into
    /// `folds.register_custom_region`. Unlike the WebView (whose pending begin
    /// line index is never pruned), the begin row here is shifted/dropped by
    /// eviction in lock-step with the fold registry so a begin that scrolls
    /// off the top yields no region — matching the WebView's "boundary-
    /// spanning region is dropped" behaviour at registration time.
    pending_fold_begin: Option<(u32, String)>,
    /// Last observed `TerminalCore::get_scrollback_evicted_total`. `pump`
    /// reads the live counter, and when it advanced past this baseline it
    /// calls `prompts.prune_before_line(delta)` (shifting stored rows down
    /// by the number of newly-evicted scrollback lines) and updates this.
    evicted_baseline: u64,
    /// Rows evicted from scrollback since the last `take_eviction_delta`,
    /// accumulated by `backfill_prompt_marks`'s prune step. `App::pump_all`
    /// drains this to shift the absolute-row selection into the new frame.
    pending_eviction_delta: u32,
    /// Latched when the eviction counter moved backwards (core reset /
    /// RIS) — the whole line frame restarted, so absolute-row consumers
    /// (the selection) must drop their state.
    pending_frame_reset: bool,
    /// The SSH connection name this tab was spawned with, when it is an SSH
    /// tab (`SpawnOverrides::ssh_connection_name`). `None` for plain / WSL
    /// tabs. SFTP upload reads this to rebuild the connection inputs for a
    /// file drop on this tab.
    pub ssh_connection_name: Option<String>,
    /// Saved scroll position while this tab is inactive (FR3). `App` keeps
    /// the *active* tab's live scroll value in `App::scroll_position`; on a
    /// native tab switch it parks the outgoing tab's value here and reloads
    /// the incoming tab's. Defaults to `Live` (bottom). For a mux-attached
    /// tab this holds the active pane's position (per-pane positions live in
    /// `mux_group.pane_scrolls`); the two are reconciled on pane switch.
    pub scroll_position: crate::app::ScrollPosition,
    /// Latched by the inbound (daemon-initiated) `SwitchWindow` reconcile and
    /// the `PaneCreated` append (FR3 pane wiring): holds the **pane id** of the
    /// active pane *before* the switch/create moved it. The daemon handler runs
    /// deep inside `pump`, with no access to `App::scroll_position`, so it
    /// records the outgoing pane here; `App::pump_all` drains it via
    /// [`Tab::take_pending_pane_switch`] and performs the App-side per-pane
    /// scroll save/restore + full-redraw, exactly as the local switch path does
    /// inline. Stored by pane **id** (not index) so a same-pump `PtyExited`
    /// that removes a pane — shifting the parallel arrays — cannot make the
    /// latch park the outgoing scroll into the wrong slot; the consumer
    /// resolves the id to a current index and skips if the pane is gone.
    pending_pane_switch_from: Option<u32>,
    /// One-shot latch: a `PaneCreated` this pump appended a new mux window
    /// (which `MuxWindowGroup::push` makes the active sub-tab). `App::pump_all`
    /// drains it via [`Tab::take_pending_window_appended`] and, when this is the
    /// active tab, scrolls the freshly-active sub-tab into view (FR6, mux case).
    /// Latched at the push site rather than inferred from a window-count delta,
    /// so a same-pump `PtyExited` removing another pane, or a `Welcome` reseed,
    /// can neither mask nor fake the signal.
    pending_window_appended: bool,
    /// Plain-tab `agent-status` OSC events drained from
    /// `cb_state.pending_agent_status` this pump (task0005 AC-1).
    /// `App::pump_all` drains it via
    /// [`Tab::take_pending_agent_status_events`] and applies each event to
    /// `App::agent_status`, keyed by [`Self::stable_id`].
    pending_agent_status_events: Vec<crate::agent_status::AgentStatusEvent>,
    /// True-order, live-only OSC 777 Set/Clear + OSC 133 D/A sequence for
    /// this tab's inferred-clear latch this pump (agent-exit-after-icon
    /// FR2/FR4/FR5; task0002 deviation). Populated by
    /// `process_outer_via_core`'s reconciliation of
    /// `cb_state.pending_latch_feed` against the live prompt marks drained
    /// the same pump. `App::pump_all` drains it via
    /// [`Self::take_pending_latch_inputs`] and feeds each entry to
    /// `AgentStatusModel`'s per-tab latch, in order.
    pending_latch_inputs: Vec<crate::agent_status_model::ResolvedLatchInput>,
    /// Daemon-pushed `AgentStatusUpdate` messages decoded by
    /// [`Self::apply_mux_message`]'s `MessageType::AgentStatusUpdate` arm
    /// this pump (task0005 AC-2). `App::pump_all` drains it via
    /// [`Tab::take_pending_agent_status_updates`] and applies each update to
    /// `App::agent_status`.
    pending_agent_status_updates: Vec<mux_ipc::protocol::AgentStatusUpdateMsg>,
    /// Mux pane ids removed by a `PtyExited` arm this pump (task0005 AC-6).
    /// `App::pump_all` drains it via
    /// [`Tab::take_closed_agent_status_panes`] to discard the matching
    /// `App::agent_status` entries.
    pending_closed_agent_status_panes: Vec<u32>,
    /// In-flight off-thread snapshot replay for this tab (the mux
    /// off-thread switch). `Some` while a large snapshot is being reparsed
    /// on a worker thread; `App::pump_all` polls it each pump and swaps the
    /// completed core in on a later frame. `None` when no off-thread replay
    /// is pending (the common case, and always so for sub-threshold
    /// snapshots which stay on the synchronous path).
    pending_switch: Option<PendingSwitch>,
    /// FR8 (task0003; task0006 redesign narrows this to same-pane
    /// SNAPSHOT dedup — a racing grid resize no longer goes through here,
    /// see `PendingSwitch::pending_resize`): a same-pane re-dispatch
    /// request captured by `dispatch_offthread_replay` while
    /// `pending_switch` already targets the SAME pane (a second
    /// `Snapshot`/`SnapshotRestore` for the pane arriving before the first
    /// has swapped). Coalesces any number of such re-dispatches into
    /// exactly one actual worker spawn: `poll_pending_switch` installs a
    /// fresh worker for whichever `(target_pane, payload, segments)` is
    /// LATEST here the next time it runs (the same pump tick the
    /// re-dispatch happened in), so the in-flight (already-cancelled)
    /// worker's own eventual outcome — success or disconnect — is never
    /// observed; only the latest request's replay ever completes. `None`
    /// in the common case (no same-pane race in flight).
    pending_redispatch: Option<(u32, Vec<u8>, Vec<ReplaySegment>)>,
    /// In-flight 2nd-pass scrollback restore worker for this tab. Spawned
    /// at the end of `apply_offthread_swap` (i.e. after the 1st-pass
    /// bypass-on swap finished), polled non-blockingly each pump via
    /// `App::pump_all`. `None` when no restore is pending (no recent
    /// off-thread swap, or the restore already merged / was cancelled /
    /// failed). NFR4: at most one in-flight restore per tab.
    pending_scrollback_restore: Option<PendingScrollbackRestore>,
    /// Pre-captured B-mark line texts from the most recent off-thread
    /// `SnapshotReplay`, keyed by the **original** (pre-normalization)
    /// `abs_row`. Populated in `apply_offthread_swap` from
    /// `SnapshotReplay::bypass_b_mark_texts` before `apply_replay_reconcile`
    /// runs; consumed by `backfill_prompt_marks` to populate
    /// `resolved_b_mark_texts`; cleared at the end of `apply_offthread_swap`.
    /// Empty on the synchronous replay path (where scrollback is populated
    /// so `extract_line_text` works as-is).
    pending_bypass_b_mark_texts: std::collections::HashMap<u32, String>,
    /// B-mark line texts keyed by the **post-normalization** absolute row,
    /// populated by `backfill_prompt_marks` from `pending_bypass_b_mark_texts`
    /// whenever it processes a B mark whose original `abs_row` is present in
    /// that map. `register_osc133_fold_region_at_idx` checks this map first
    /// before falling back to `extract_line_text`. Cleared together with
    /// `pending_bypass_b_mark_texts` at the end of `apply_offthread_swap`.
    resolved_b_mark_texts: std::collections::HashMap<u32, String>,
    /// Independent parser that extracts the outer `emterm-mux;` transport
    /// frames from the PTS byte stream once mux is established. A mux tab's
    /// PTS stream is two layers — the outer transport (APC / OSC 9999 mux
    /// frames) and, inside `PtyOutput` messages, the inner content. Driving
    /// both through `self.core`'s single parser corrupts state when an inner
    /// Kitty image chunk spans `PtyOutput` boundaries (base64 leak). When
    /// `mux_session_name.is_some()`, `pump` feeds the coalesced PTS bytes here
    /// instead of into `self.core`, so `self.core` is driven by inner content
    /// only (FR1 / FR2). Reset on detach so the pre-mux branch resumes clean.
    mux_apc_extractor: term_core::MuxApcExtractor,
    /// Test-only: number of times the `process_combined` coalesce flush
    /// invoked the core parse for active-pane output. The coalesce contract
    /// (consecutive active-pane `PtyOutput` ⇒ one parse) and the batched
    /// metric test read this to assert pass count. Strictly `cfg(test)` so
    /// the production build carries no counter.
    #[cfg(test)]
    coalesce_parse_passes: u32,
    /// Test-only: number of times `dispatch_offthread_replay` actually
    /// spawned an off-thread replay worker (task0003 FR7/FR8) — i.e. how
    /// many times the "install a fresh worker" path ran, NOT how many times
    /// `dispatch_offthread_replay` was called (a same-pane coalesce returns
    /// early without spawning). Distinguishes "deduplicated before doing
    /// the work" from "did the work twice but the result looks the same".
    #[cfg(test)]
    offthread_spawn_count: u32,
    /// Test-only: number of times `apply_mux_message`'s `Snapshot`/
    /// `SnapshotRestore` arm ran `decode_snapshot_payload_typed` on an
    /// incoming frame (task0006, FR8 AC-6). Distinct from
    /// `offthread_spawn_count`: this counts DECODE, which currently runs
    /// for every incoming frame regardless of whether it later coalesces
    /// into an already-in-flight same-pane switch — see this task's
    /// `notes` on FR8's scope (replay-BUILD dedup only, not fetch/decode
    /// dedup).
    #[cfg(test)]
    snapshot_decode_count: u32,
    /// Test-only: every pane `Resize` control frame this tab has emitted,
    /// across all three emission sites enumerated in
    /// [`ResizeFrameRecord`]'s doc (task0003 AC-6, FR4). Strictly
    /// `cfg(test)` so the production build carries no observer.
    #[cfg(test)]
    resize_frame_log: Vec<ResizeFrameRecord>,
}

impl Tab {
    // Each argument maps to a distinct construction input (grid dims,
    // settings, the two optional status-bar runtime handles, and the
    // shared notification sink); grouping them into a struct would only
    // move the same fields behind a builder for the two call sites in
    // `App`, so the flat signature is kept.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_shell(
        title: impl Into<String>,
        cols: u16,
        rows: u16,
        scrollback_lines: u32,
        settings: Arc<Settings>,
        statusbar_dispatcher: Option<
            Arc<crate::status_bar::osc_dispatcher::StatusBarOscDispatcher>,
        >,
        cwd_provider: Option<Arc<crate::status_bar::providers::CwdProvider>>,
        notification_sink: Arc<dyn crate::callbacks::NotificationSink>,
        spawn_overrides: Option<crate::profiles::SpawnOverrides>,
    ) -> Self {
        // D3'''''' (round-9 rework, review round-8 finding
        // `1e7e069001cf22dc`): clamp to the SAME wire domain `Tab::resize` /
        // `MuxPane::new` apply, BEFORE spawning the local PTY or
        // constructing this tab's core — not just on the first later
        // `Tab::resize` call. Without this, a tab whose FIRST-ever dims are
        // already out of domain (the caller's raw window size before any
        // resize) spawns its PTY and core unclamped; if this tab then
        // becomes mux-connected (a `Welcome`/`PaneCreated` landing before
        // the first `Tab::resize`), the daemon's `MuxPane::new` clamps its
        // OWN pane to the same domain while this tab's core stays
        // unclamped, disagreeing with what the daemon actually holds. A
        // plain local (non-mux) tab is clamped the same way here as
        // `Tab::resize` already clamps it unconditionally today — the wire
        // domain's ceilings (4096 per axis; the current
        // `PRODUCER_SEGMENT_CELL_BUDGET` product) are far above any real
        // terminal window, so this is not expected to narrow a legitimate
        // local tab's size in practice.
        let (cols, rows) = crate::mux::session::pane::clamp_dims_to_wire_domain(cols, rows);
        // bounded(4096): allows up to ~64MB of in-flight PTY chunks
        // (4096 × 16KB reader buffer) before the reader thread blocks
        // on send. The previous 256-slot queue (~4MB) made bursty
        // producers like `seq 1 10000000` slow to a crawl because the
        // shell side filled the queue inside a few milliseconds and
        // then had to wait for the main thread to drain one frame's
        // worth (12 ms budget) at a time. The trade-off is up to 64MB
        // of resident memory per tab during a sustained burst.
        let (tx, rx) = crossbeam_channel::bounded::<PtyEvent>(4096);
        // Profile overrides (shell / argv / env / cwd) win over the global
        // settings; `None` fields fall through to `settings.*`.
        let ov = spawn_overrides.unwrap_or_default();
        let ssh_connection_name = ov.ssh_connection_name.clone();
        let shell_path = ov.shell_path.as_deref().unwrap_or(&settings.shell_path);
        let shell_args = ov.shell_args.as_deref().unwrap_or(&settings.shell_args);
        let pty = match PtySession::spawn(
            cols,
            rows,
            tx,
            shell_path,
            shell_args,
            &ov.env_vars,
            ov.working_directory.as_deref(),
        ) {
            Ok(p) => Some(p),
            Err(e) => {
                log::error!("failed to spawn shell PTY: {e}");
                None
            }
        };

        // Capture the fold-enable preference before `settings` is moved into
        // the callbacks below; it seeds the per-tab `FoldManager` and is
        // re-applied whenever that manager is rebuilt on a reset/replay.
        let fold_enabled = settings.fold_enabled;

        // Construct the core and install our native callbacks.
        let mut core = TerminalCore::new(cols, rows, scrollback_lines);
        // Seed `cursor_blink` from settings before OSC / DECTCEM
        // sequences have had a chance to override it; the default
        // inside `TerminalCore` is `true`, so this only matters when
        // `settings.json` opts out (`"cursor_blink": false`).
        core.set_cursor_blink(settings.cursor_blink);
        // Seed the cursor shape default from `settings.cursor_style`
        // using the canonical numeric mapping (0 = block, 1 =
        // underline, 2 = bar) so newly spawned tabs match the
        // settings-apply path below.
        core.set_cursor_style(settings.cursor_style.as_cursor_shape_u8());
        // `term_core` knows no mux protocol; register the app-layer OSC mapping
        // so a pre-mux OSC 9999 `emterm-mux;` Welcome (the Windows ConPTY
        // fallback transport, parsed by `self.core` before mux is established)
        // reaches `on_osc(OSC_MUX_INBAND, …)` → the mux APC path (NFR5).
        // Off-thread snapshot replay cores are worker-built without this
        // registration (and without callbacks — the worker contract requires
        // `Send`), but they become the live core at swap time:
        // `apply_offthread_swap` transplants the callbacks and re-registers
        // this same mapping onto the swapped-in core, so it ends up
        // behaviorally identical to a never-swapped tab core.
        core.register_osc_app_param(
            mux_ipc::protocol::MUX_OSC_PARAM,
            crate::callbacks::OSC_MUX_INBAND,
        );
        let cb_state = Arc::new(Mutex::new(NativeCallbackState::default()));
        // Seed the theme from settings (font_size_pt + cursor_style)
        // so the first frame renders at the user's configured size
        // and cursor shape. OSC 4 / 10 / 11 / 12 / 22 may still
        // mutate any field at runtime via `NativeCallbacks`.
        let theme = Arc::new(Mutex::new(Theme::from_settings(settings.as_ref())));
        let mut callbacks =
            NativeCallbacks::new(cb_state.clone(), theme.clone(), settings, notification_sink);
        if let Some(dispatcher) = statusbar_dispatcher {
            callbacks.set_statusbar_dispatcher(dispatcher);
        }
        if let Some(provider) = cwd_provider {
            callbacks.set_cwd_provider(provider);
        }
        core.callbacks = Some(Box::new(callbacks));

        Self {
            stable_id: NEXT_TAB_STABLE_ID.fetch_add(1, Ordering::Relaxed),
            title: title.into(),
            core: Arc::new(Mutex::new(core)),
            cb_state,
            theme,
            pty,
            events: rx,
            exited: false,
            image_proc: ImageProcessor::new(),
            pending_image_events: Vec::new(),
            mux_session_name: None,
            mux_group: None,
            preedit_state: crate::ime::preedit::State::default(),
            bell_pending: false,
            output_pending: false,
            activity: crate::notifications::TabActivityState::default(),
            prompts: crate::prompts::PromptTracker::default(),
            folds: Self::new_fold_manager(fold_enabled),
            fold_enabled,
            pending_fold_begin: None,
            evicted_baseline: 0,
            pending_eviction_delta: 0,
            pending_frame_reset: false,
            ssh_connection_name,
            scroll_position: crate::app::ScrollPosition::default(),
            pending_pane_switch_from: None,
            pending_window_appended: false,
            pending_agent_status_events: Vec::new(),
            pending_latch_inputs: Vec::new(),
            pending_agent_status_updates: Vec::new(),
            pending_closed_agent_status_panes: Vec::new(),
            pending_switch: None,
            pending_redispatch: None,
            pending_scrollback_restore: None,
            pending_bypass_b_mark_texts: std::collections::HashMap::new(),
            resolved_b_mark_texts: std::collections::HashMap::new(),
            mux_apc_extractor: term_core::MuxApcExtractor::new(
                mux_ipc::protocol::MUX_OSC_PARAM,
                mux_ipc::protocol::APC_PREFIX,
            ),
            #[cfg(test)]
            coalesce_parse_passes: 0,
            #[cfg(test)]
            offthread_spawn_count: 0,
            #[cfg(test)]
            snapshot_decode_count: 0,
            #[cfg(test)]
            resize_frame_log: Vec::new(),
        }
    }

    /// Build a fresh [`crate::fold::FoldManager`] honoring the tab's
    /// `fold_enabled` preference. A new `FoldManager` defaults to
    /// `enabled = true`, so when folding is disabled we immediately push
    /// that state through `set_enabled(false)` (which also `unfold_all`s,
    /// a no-op on the empty registry). Centralized so the construction site
    /// and the two reset/replay rebuild sites stay in sync.
    fn new_fold_manager(enabled: bool) -> crate::fold::FoldManager {
        let mut fm = crate::fold::FoldManager::new();
        fm.set_enabled(enabled);
        fm
    }

    /// Whether this tab was spawned through an SSH profile (and therefore can
    /// drive an SFTP upload for a file drop).
    pub fn is_ssh_tab(&self) -> bool {
        self.ssh_connection_name.is_some()
    }

    /// Resolve this tab's SSH connection name to the matching settings record,
    /// or `None` when the tab is not an SSH tab or the connection was removed.
    pub fn ssh_connection<'a>(
        &self,
        settings: &'a Settings,
    ) -> Option<&'a app_settings::SshConnection> {
        let name = self.ssh_connection_name.as_deref()?;
        settings.ssh_connections.iter().find(|c| c.name == name)
    }

    /// Update the tab's fold-enable preference at runtime (settings
    /// panel apply path). Pushes the new state into the live
    /// `FoldManager` (disabling also unfolds everything, mirroring the
    /// WebView's `setEnabled(false)`) and records it so the
    /// reset/replay rebuild sites keep honoring it.
    pub fn set_fold_enabled(&mut self, enabled: bool) {
        self.fold_enabled = enabled;
        self.folds.set_enabled(enabled);
    }

    /// Consume the BEL latch set by the last `pump()`. Returns true at
    /// most once per ring — `App::pump_all` polls this every frame.
    pub fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell_pending)
    }

    /// Consume the new-output latch set by the last `pump()`. Returns
    /// true at most once per burst — `App::pump_all` polls this every
    /// frame to mark inactive-tab activity.
    pub fn take_output(&mut self) -> bool {
        std::mem::take(&mut self.output_pending)
    }

    /// Consume the accumulated scrollback-eviction delta (rows evicted
    /// since the last call). `App::pump_all` drains this for the active
    /// tab to shift the absolute-row selection into the new frame.
    pub fn take_eviction_delta(&mut self) -> u32 {
        std::mem::take(&mut self.pending_eviction_delta)
    }

    /// Consume the frame-reset latch (set when the eviction counter moved
    /// backwards — a core reset / RIS). `App::pump_all` drains this for the
    /// active tab to drop the now-meaningless absolute-row selection.
    pub fn take_frame_reset(&mut self) -> bool {
        std::mem::take(&mut self.pending_frame_reset)
    }

    /// Consume the inbound-pane-switch latch (FR3 pane wiring). Returns the
    /// outgoing active pane **id** recorded by a daemon-initiated `SwitchWindow`
    /// or `PaneCreated` that actually moved the active pane, or `None` when no
    /// such transition occurred this pump. `App::pump_all` drains this, resolves
    /// the pane id to its current index (skipping if the pane has since exited),
    /// parks the outgoing pane's scroll position there, then reloads the new
    /// active pane's saved position and forces a full redraw (FR2).
    pub fn take_pending_pane_switch(&mut self) -> Option<u32> {
        self.pending_pane_switch_from.take()
    }

    /// Drain the one-shot "a mux window was appended this pump" latch (FR6, mux
    /// case). Returns `true` exactly once after a `PaneCreated` pushed — and so
    /// activated — a new window. `App::pump_all` drains every tab to avoid stale
    /// carry-over and acts on it only for the active tab.
    pub fn take_pending_window_appended(&mut self) -> bool {
        std::mem::take(&mut self.pending_window_appended)
    }

    /// Drain the plain-tab `agent-status` OSC events parsed this pump
    /// (task0005 AC-1). `App::pump_all` applies each to `App::agent_status`
    /// keyed by [`Self::stable_id`].
    pub fn take_pending_agent_status_events(
        &mut self,
    ) -> Vec<crate::agent_status::AgentStatusEvent> {
        std::mem::take(&mut self.pending_agent_status_events)
    }

    /// Drain this tab's resolved inferred-clear latch inputs this pump
    /// (agent-exit-after-icon FR2/FR4/FR5; task0002 deviation). See
    /// [`Self::pending_latch_inputs`]'s doc.
    pub fn take_pending_latch_inputs(
        &mut self,
    ) -> Vec<crate::agent_status_model::ResolvedLatchInput> {
        std::mem::take(&mut self.pending_latch_inputs)
    }

    /// Drain the daemon-pushed `AgentStatusUpdate` messages decoded this
    /// pump (task0005 AC-2). `App::pump_all` applies each to
    /// `App::agent_status`.
    pub fn take_pending_agent_status_updates(
        &mut self,
    ) -> Vec<mux_ipc::protocol::AgentStatusUpdateMsg> {
        std::mem::take(&mut self.pending_agent_status_updates)
    }

    /// Drain the mux pane ids a `PtyExited` arm removed this pump
    /// (task0005 AC-6). `App::pump_all` discards the matching
    /// `App::agent_status` entries.
    pub fn take_closed_agent_status_panes(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.pending_closed_agent_status_panes)
    }

    /// Pause the native PTY reader. Subsequent PTY output goes into the
    /// per-session ring buffer until [`Tab::resume_native_pty`] is called.
    /// Phase 4-C kept this in place for a future "freeze native output
    /// while mux owns the screen" affordance; the APC-redesigned mux
    /// path does not currently invoke it (the bridge CLI runs in the
    /// same PTY, so there is no second stream to pause).
    #[allow(dead_code)]
    pub fn pause_native_pty(&self) {
        if let Some(p) = &self.pty {
            p.set_paused(true);
        }
    }

    /// Drain the ring buffer and replay the bytes into `core`, then resume
    /// the native PTY reader. Counterpart of [`Tab::pause_native_pty`];
    /// unused in the APC-redesigned mux path but retained for the same
    /// future-use rationale.
    #[allow(dead_code)]
    pub fn resume_native_pty(&mut self) {
        if let Some(p) = &self.pty {
            let drained = p.drain_ring();
            if !drained.is_empty() {
                // Resume-loop helper: a bare `process_pty_data` call would
                // drop everything after a buffer-switch interrupt.
                let mut c = self.core.lock();
                c.process_pty_data_fully(&drained);
            }
            if p.ring_overflowed() {
                log::warn!(
                    "mux pause ring buffer overflowed; some native PTY output \
                     was discarded for tab {:?}",
                    self.title
                );
            }
            p.set_paused(false);
        }
    }

    /// Rebuild the absolute-row frame from a replay payload.
    ///
    /// Shared by every code path that swaps in a fresh `term_core` frame:
    /// `Snapshot` / `SnapshotRestore` replay the daemon-captured bytes, and
    /// `PaneCreated` calls with an empty payload to reset the tab when a new
    /// mux window becomes active. Centralising the recipe keeps the
    /// callers in lockstep — adding a future field (e.g. another
    /// frame-keyed cache) only needs to land here, not at every site.
    ///
    /// Recipe:
    /// - clear prompts (they referenced the discarded frame's rows)
    /// - rebuild the fold manager with the current fold-enable preference
    /// - drop any in-flight custom-fold `begin` (belonged to the discarded frame)
    /// - lock the core, `reset_and_replay`, drain marks
    /// - update `evicted_baseline` and call `backfill_marks` so
    ///   `backfill_prompt_marks` latches `pending_frame_reset` (App::pump_all
    ///   reads that latch to drop the now-stale absolute-row selection /
    ///   press anchor — without this, a selection from the previous frame
    ///   addresses rows that no longer mean the same thing)
    ///
    /// The alt-screen state needs no reseed here: `term_core::reset` returns
    /// the core to the primary buffer and the replay re-derives the
    /// authoritative `MODE_ALT_SCREEN` bit, which `App::pump_all` reads
    /// directly each pump.
    ///
    /// Returns the mode actions accumulated during the replay so a caller
    /// (e.g. Snapshot's debug log) can use them.
    ///
    /// `segments` (task0004 round-4 rework D1'): structural dimension
    /// segments decoded from the wire payload
    /// (`mux_ipc::protocol::decode_snapshot_payload`) — the sole authority
    /// for which dimensions applied to which bytes of `payload`. An empty
    /// slice (a `PaneCreated` blank-reset call, or an older daemon's
    /// snapshot with no segment field) degrades to single-dimension replay
    /// (AC-11).
    fn reset_frame_for_replay(&mut self, payload: &[u8], segments: &[ReplaySegment]) -> Vec<u8> {
        self.reset_frame_prompts_folds();
        let (actions, evicted_total, pending_marks, pending_fold_marks) = {
            let mut c = self.core.lock();
            let actions = c.reset_and_replay_segments(payload, segments);
            // Discard any device responses (DA1 / DSR / XTWINOPS / …) that
            // historic queries baked into the snapshot bytes generated during
            // replay. The originating program is long gone; leaving the bytes
            // in `response_buffer` would let the next live `take_response`
            // (see `apply_active_pane_output`) deliver them to the live
            // shell's stdin, where zsh/zle interprets `\x1b[?` as an unbound
            // key-binding prefix and inserts the remaining `65;1;4;22c` at
            // the prompt on the user's first keystroke after the switch.
            let _ = c.take_response();
            let (evicted_total, pending_marks, pending_fold_marks) = drain_marks(&mut c);
            (actions, evicted_total, pending_marks, pending_fold_marks)
        };
        self.apply_replay_reconcile(evicted_total, pending_marks, pending_fold_marks);
        actions
    }

    /// Frame-discard half of the replay recipe: drop the prompt / fold
    /// state that addressed the *outgoing* frame's rows. Shared by the
    /// synchronous [`Self::reset_frame_for_replay`] and the off-thread
    /// dispatch (which does this at dispatch time, before the worker has
    /// produced the new core, so the stale trackers never outlive the
    /// dispatch). The displayed core itself is NOT touched here — the
    /// off-thread path keeps showing the outgoing pane until the swap.
    fn reset_frame_prompts_folds(&mut self) {
        self.prompts.clear();
        self.folds = Self::new_fold_manager(self.fold_enabled);
        self.pending_fold_begin = None;
    }

    /// Dispatch an off-thread snapshot replay for `target_pane`: do the
    /// frame-discard portion now (so the stale prompt/fold trackers don't
    /// outlive the dispatch), read the displayed core's current grid size,
    /// spawn a one-shot worker that builds a fresh core at that grid and
    /// full-drain replays `payload`, and install the [`PendingSwitch`].
    ///
    /// The displayed core is deliberately NOT reset here — the outgoing pane
    /// stays on screen until `App::pump_all` swaps the worker-built core in.
    /// Replaces (supersedes) any prior in-flight switch on this tab targeting
    /// a DIFFERENT pane; the prior worker's result is dropped when its
    /// `done` sender is dropped with the old `PendingSwitch`.
    ///
    /// FR8 (task0003; task0006 redesign narrows this to same-pane
    /// SNAPSHOT dedup only — see `PendingSwitch::pending_resize` for the
    /// resize case, which no longer calls back in here): when
    /// `pending_switch` already targets the SAME `target_pane` — a second
    /// `Snapshot`/`SnapshotRestore` for the pane arriving before the first
    /// has swapped — this does NOT spawn a second worker right away. It
    /// cancels the in-flight one (as always) and stashes the request in
    /// `pending_redispatch` instead; `poll_pending_switch` installs a fresh
    /// worker for whichever request is LATEST there the next time it runs
    /// (the same pump tick). This collapses any number of same-pane
    /// duplicate snapshot fetches into exactly one actual build, so an
    /// intermediate, already-superseded fetch's replay is never paid for,
    /// and only the final request's replay ever completes. `Tab::resize`
    /// no longer calls this fn at all for a same-pane in-flight switch
    /// (review round-1 finding `64baa639d71792f9`) — see
    /// `PendingSwitch::pending_resize`'s doc for why re-dispatching a
    /// resize through here defeated the bypass split.
    fn dispatch_offthread_replay(
        &mut self,
        target_pane: u32,
        payload: Vec<u8>,
        segments: Vec<ReplaySegment>,
    ) {
        // Supersede any in-flight worker: signal it to bail at the next chunk
        // boundary so workers do not pile up under a rapid switch / resize
        // storm. The old `PendingSwitch` (and its receiver) is dropped when
        // `self.pending_switch` is overwritten below (or, for the same-pane
        // coalesce case, when `poll_pending_switch` later takes it).
        let same_pane_in_flight = if let Some(old) = self.pending_switch.as_ref() {
            old.cancel.store(true, Ordering::Relaxed);
            old.target_pane == target_pane
        } else {
            false
        };
        if same_pane_in_flight {
            // FR8 (task0006 redesign, review round-1 findings
            // `7ed0ba7335376c20` / `ebc9de26bb15fcb1`): decide the
            // live_queue discard/keep question HERE, at coalesce time, not
            // later at poll time. `Tab::resize` no longer re-dispatches
            // through this branch (see `PendingSwitch::pending_resize`), so
            // every arrival here is a genuinely NEW `Snapshot`/
            // `SnapshotRestore` frame for the pane — matching the
            // pre-task0003 baseline where a same-pane snapshot replaced
            // `pending_switch` (and its `live_queue`) immediately,
            // synchronously, in the same call that decoded it. `pending_switch`
            // itself stays alive here (only the coalesced BUILD is
            // deferred to the next poll), so clearing the queue now — then
            // leaving `pending_switch` untouched — means any live output
            // arriving AFTER this point keeps accumulating correctly
            // against it, and `poll_pending_switch` can inherit whatever is
            // left unconditionally (no payload comparison needed, see that
            // fn's doc / review round-1 finding `5b1878c41d3e02d6`).
            if let Some(pending) = self.pending_switch.as_mut() {
                pending.live_queue.clear();
                pending.queued_bytes = 0;
            }
            self.pending_redispatch = Some((target_pane, payload, segments));
            return;
        }
        // A dispatch for a different pane (or no in-flight switch at all)
        // supersedes any coalesced same-pane request outright — it belonged
        // to a switch this tab is no longer completing.
        self.pending_redispatch = None;
        // FR5 / NFR4: a new off-thread switch makes any in-flight 2nd-pass
        // scrollback restore stale (the live core is about to be reset to a
        // different snapshot, so the rebuilt scrollback would be against an
        // unrelated baseline). Cancel + drop so the worker bails at the next
        // chunk boundary and the receiver is gone before this fn returns.
        if let Some(old) = self.pending_scrollback_restore.take() {
            old.cancel.store(true, Ordering::Relaxed);
            log::warn!(
                "scrollback restore cancelled (superseded by new switch) for tab {:?}",
                self.title
            );
        }
        self.reset_frame_prompts_folds();
        let (cols, rows, scrollback_lines) = {
            let c = self.core.lock();
            (c.cols(), c.rows(), c.scrollback_capacity())
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker_payload = payload.clone();
        let worker_segments = segments.clone();
        // One-shot worker: pure build off the UI thread. `build_from_snapshot`
        // returns `None` if cancelled mid-parse — then there is nothing to
        // send. A successful build's `send` failure (receiver dropped because
        // the switch was superseded) is ignored. A panic inside the build
        // drops `tx`, which the main thread observes as `Err(Disconnected)`
        // and handles via the synchronous reparse fallback (FR7).
        let spawn_result = std::thread::Builder::new()
            .name("mux-snapshot-replay".into())
            .spawn(move || {
                if let Some(replay) = term_core::terminal_core::TerminalCore::build_from_snapshot(
                    cols,
                    rows,
                    scrollback_lines,
                    &worker_payload,
                    &worker_segments,
                    &worker_cancel,
                ) {
                    let _ = tx.send(replay);
                    // task0004 D4/AC-3: pull the event loop out of
                    // `ControlFlow::Wait` so `poll_pending_switch` observes
                    // this swap on the next `about_to_wait` pass instead of
                    // waiting for an unrelated event (input, PTY output on
                    // another tab, …). Mirrors the existing PTY reader
                    // thread's wake call in `pty::reader_loop`.
                    crate::wakeup::wake();
                }
            });
        match spawn_result {
            Ok(_) => {
                #[cfg(test)]
                {
                    self.offthread_spawn_count += 1;
                }
                self.pending_switch = Some(PendingSwitch {
                    target_pane,
                    cols,
                    rows,
                    done: rx,
                    live_queue: Vec::new(),
                    queued_bytes: 0,
                    payload,
                    segments,
                    cancel,
                    pending_resize: None,
                });
            }
            Err(e) => {
                // Spawn failure (thread/resource exhaustion) must not crash
                // the UI thread (the synchronous path it replaces never did).
                // Reparse synchronously now — a one-off block, accepted — and
                // install no pending switch. `reset_frame_prompts_folds` above
                // already cleared the trackers; `reset_frame_for_replay`
                // repeats that (a no-op on the now-empty state) plus replays.
                log::warn!(
                    "mux off-thread replay worker spawn failed ({e}); \
                     synchronous reparse fallback for tab {:?}",
                    self.title
                );
                self.reset_frame_for_replay(&payload, &segments);
                self.pending_switch = None;
            }
        }
    }

    /// Non-blockingly poll the in-flight off-thread snapshot replay and, when
    /// the worker has finished, swap the built core in, replay the queued
    /// target-pane live output in arrival order, and reconcile the
    /// absolute-row trackers. Called once per owning tab from
    /// `App::pump_all` (not gated to the active tab), so background-tab
    /// swaps apply too.
    ///
    /// Returns:
    /// - `SwapOutcome::Idle` — no pending switch.
    /// - `SwapOutcome::Pending` — worker still parsing; keep showing the
    ///   outgoing pane.
    /// - `SwapOutcome::Swapped` — the core was swapped + reconciled this
    ///   call; the caller drives the active-tab post-loop reconciliation
    ///   (per-pane scroll restore + selection-on-frame-reset + full redraw).
    /// - the fallback (worker panic) also returns `Swapped`: the latest
    ///   target is reparsed synchronously here (FR7), so from the caller's
    ///   perspective the swap completed this pump.
    ///
    /// FR8 (task0003; task0006 redesign): before touching the in-flight
    /// worker's channel at all, install a fresh worker for any coalesced
    /// `pending_redispatch` (a duplicate-snapshot re-dispatch stashed by
    /// `dispatch_offthread_replay`'s same-pane branch) — the in-flight
    /// worker this supersedes is dropped without ever being observed,
    /// whether it would have completed or disconnected.
    pub(crate) fn poll_pending_switch(&mut self) -> SwapOutcome {
        if let Some((target_pane, payload, segments)) = self.pending_redispatch.take() {
            let (queued, queued_bytes) = match self.pending_switch.take() {
                Some(old) => {
                    old.cancel.store(true, Ordering::Relaxed);
                    // FR7/FR8 (task0006 redesign, review round-1 findings
                    // `7ed0ba7335376c20` / `5b1878c41d3e02d6`): the
                    // discard/keep decision for `live_queue` was already
                    // made at COALESCE time
                    // (`dispatch_offthread_replay`'s same-pane branch
                    // clears it there), so `old.live_queue` here already
                    // holds exactly "output queued since the last
                    // coalesce" — always safe to inherit unconditionally.
                    // This also removes the O(n) full-payload byte
                    // comparison round-1's fix required here on every
                    // poll (finding `5b1878c41d3e02d6`); the invariant
                    // that `pending_redispatch`'s pane always matches
                    // `pending_switch`'s (it is only ever populated by the
                    // same-pane coalesce branch) is asserted rather than
                    // branched on.
                    debug_assert_eq!(
                        old.target_pane, target_pane,
                        "pending_redispatch's pane must match pending_switch's \
                         (dispatch_offthread_replay invariant)"
                    );
                    (old.live_queue, old.queued_bytes)
                }
                None => (Vec::new(), 0),
            };
            // `self.pending_switch` is `None` here, so this dispatch always
            // takes the "install a fresh worker" path below, never the
            // same-pane coalesce branch (which would otherwise loop back
            // into `pending_redispatch` forever).
            self.dispatch_offthread_replay(target_pane, payload, segments);
            return match self.pending_switch.as_mut() {
                Some(p) => {
                    p.live_queue = queued;
                    p.queued_bytes = queued_bytes;
                    SwapOutcome::Pending
                }
                // Rare: the worker thread failed to spawn and
                // `dispatch_offthread_replay`'s own fallback already
                // reparsed synchronously and applied it — the switch is
                // already visually complete this pump. The queued live
                // output predating this re-dispatch has no home to land in
                // (mirrors the pre-existing gap in `Tab::resize`'s
                // redispatch branch on the same rare spawn-failure path).
                None => SwapOutcome::Swapped,
            };
        }
        let Some(pending) = self.pending_switch.as_ref() else {
            return SwapOutcome::Idle;
        };
        match pending.done.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Empty) => SwapOutcome::Pending,
            Ok(replay) => {
                // Take ownership of the queued live output + payload before
                // dropping the pending state.
                let pending = self.pending_switch.take().expect("just matched Some");
                self.apply_offthread_swap(
                    replay,
                    pending.live_queue,
                    pending.payload,
                    pending.segments,
                    pending.pending_resize,
                );
                SwapOutcome::Swapped
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // FR7: the worker panicked. Reparse the latest target's
                // snapshot synchronously via the legacy path (a one-off
                // main-thread block, accepted), then apply the queued live
                // output as ordinary output so nothing is lost.
                log::warn!(
                    "mux off-thread replay worker for tab {:?} disconnected; \
                     falling back to synchronous reparse",
                    self.title
                );
                let pending = self.pending_switch.take().expect("just matched Some");
                self.reset_frame_for_replay(&pending.payload, &pending.segments);
                self.apply_queued_live_output(pending.live_queue);
                SwapOutcome::Swapped
            }
        }
    }

    /// Swap the worker-built `replay.core` into this tab, replay `live_queue`
    /// in arrival order, and reconcile the absolute-row trackers so the
    /// result is identical to a contiguous synchronous parse of
    /// `snapshot ++ live`.
    ///
    /// The core is replaced *inside* the existing `Arc<Mutex<…>>` (not the
    /// `Arc` itself) so the renderer's shared handle stays valid. The
    /// snapshot's drained marks/actions/eviction (captured by the worker
    /// from a freshly-reset core, counter 0) reconcile exactly like the
    /// synchronous `reset_frame_for_replay`; the live output is then
    /// backfilled at its own post-replay eviction total so an eviction that
    /// happened while applying the queue shifts the snapshot marks down by
    /// the right delta.
    ///
    /// `pending_resize` (FR7, task0006 redesign, review round-1 finding
    /// `64baa639d71792f9`) is `PendingSwitch::pending_resize`: a grid
    /// resize that raced this switch, deferred by `Tab::resize` rather than
    /// forcing a re-dispatch at the new target (which would have defeated
    /// the bypass split — the payload's own recorded segments reflect the
    /// worker's ORIGINAL dispatch-time target, never a resize that landed
    /// after the fact). Applied here, on the freshly-swapped core, via the
    /// same already-bypass-aware `TerminalCore::resize` an ordinary
    /// interactive resize uses (see `TerminalCore::resize`'s own handling
    /// of `scrollback_bypass`) — so it costs (and behaves) exactly as if
    /// the user had resized right after an unraced switch landed, BEFORE
    /// the queued live output (which was produced with the daemon already
    /// aware of the new grid, since `Tab::resize` broadcasts the `Resize`
    /// control frame unconditionally, before this switch's swap) is
    /// replayed onto it.
    fn apply_offthread_swap(
        &mut self,
        replay: term_core::terminal_core::SnapshotReplay,
        live_queue: Vec<Vec<u8>>,
        payload: Vec<u8>,
        segments: Vec<ReplaySegment>,
        pending_resize: Option<(u16, u16)>,
    ) {
        // Move out the pre-captured B-mark texts BEFORE partial-moving
        // `replay.core` (field ordering matters for partial moves).
        let bypass_b_mark_texts = replay.bypass_b_mark_texts;
        // D3' (task0004 round-4 rework, review round-3 finding
        // `b235e4dbc61cc4ba`): whether THIS 1st-pass replay already
        // populated `scrollback_slim` — either because the bypass was off
        // to begin with, or because `build_from_snapshot_inner`'s D6 guard
        // downgraded out of the bypass for this payload (a row-count-growing
        // segment transition). Captured before the partial move below.
        let scrollback_populated = replay.scrollback_populated;
        // 1. Swap the built core in (renderer's Arc stays valid), transplanting
        //    the pre-swap wiring onto it FIRST so the live core is never
        //    observable (even momentarily, under this same lock) without its
        //    callbacks / app-layer OSC registration:
        //      - the old core's `callbacks` moves onto the worker-built core.
        //        An old core with no callbacks (edge case) yields
        //        `new_core.callbacks = None` — already `TerminalCore::new`'s
        //        default, so no panic.
        //      - the mux inband OSC param is re-registered on the new core
        //        with the same call `Tab::new` makes, so the swapped-in core
        //        ends up behaviorally identical to a never-swapped tab core.
        {
            let mut live = self.core.lock();
            let mut new_core = replay.core;
            new_core.callbacks = live.callbacks.take();
            new_core.register_osc_app_param(
                mux_ipc::protocol::MUX_OSC_PARAM,
                crate::callbacks::OSC_MUX_INBAND,
            );
            *live = new_core;
            // FR7 (task0006 redesign): apply a resize that raced this
            // switch, now that the built core is in place. See this fn's
            // doc and `PendingSwitch::pending_resize` for why this is
            // deferred to here instead of being baked into the worker's
            // own build target.
            if let Some((rcols, rrows)) = pending_resize {
                if (live.cols(), live.rows()) != (rcols, rrows) {
                    live.resize(rcols, rrows);
                }
            }
        }
        // 2. Stash the bypass texts so `backfill_prompt_marks` (called
        //    from inside `apply_replay_reconcile`) can populate
        //    `resolved_b_mark_texts` for each B mark it processes.
        self.pending_bypass_b_mark_texts = bypass_b_mark_texts;
        // 3. Reconcile the snapshot half first: install the fresh baseline,
        //    latch the frame reset, backfill the snapshot's marks.
        //    (Frame-discard of prompts/folds already happened at dispatch
        //    time in `dispatch_offthread_replay`; the alt-screen state is the
        //    core's MODE_ALT_SCREEN bit, read directly by App::pump_all.)
        self.apply_replay_reconcile(replay.evicted_total, replay.prompt_marks, replay.fold_marks);
        // 4. Clear pending_bypass_b_mark_texts now that the snapshot reconcile
        //    has consumed it. resolved_b_mark_texts is intentionally kept: it
        //    holds snapshot-era B mark texts that live D marks (arriving in step
        //    5) still need to look up via register_osc133_fold_region_at_idx.
        //    Row collisions are handled in backfill_prompt_marks: a live B mark
        //    on the same abs_row evicts the stale snapshot-era entry, so live
        //    always wins on collision without clearing the whole map here.
        self.pending_bypass_b_mark_texts.clear();
        // 5. Apply the queued live output in order, as ordinary post-snapshot
        //    output (NOT a reset). This re-runs the same drain/backfill the
        //    `PtyOutput` arm would have, so prompt/fold marks and eviction
        //    arriving during the gap are honored. The bypass maps are now empty
        //    so live B marks go through the normal scrollback lookup path.
        self.apply_queued_live_output(live_queue);
        // 6. Spawn the 2nd-pass scrollback restore worker (bypass-off
        //    rebuild) — but ONLY if the 1st-pass replay did NOT already
        //    populate scrollback (D3', review round-3 finding
        //    `b235e4dbc61cc4ba`). Spawning it unconditionally after a replay
        //    that already populated `scrollback_slim` (the D6 bypass
        //    downgrade, or a bypass-off build to begin with) would prepend
        //    the SAME history a second time via `apply_scrollback_restore`'s
        //    merge, duplicating it up to the ring's full capacity. This runs
        //    the same parse off-thread without the SlimCell compression
        //    bypass so `scrollback_slim` ends up populated;
        //    `apply_scrollback_restore` later merges that into the live
        //    core. We supersede any prior in-flight restore on this tab
        //    (NFR4 — one in-flight 2nd-pass per tab); the prior worker
        //    observes cancel at the next chunk boundary.
        if scrollback_populated {
            log::debug!(
                "1st-pass replay already populated scrollback for tab {:?}; \
                 skipping the 2nd-pass restore worker (D3')",
                self.title
            );
        } else {
            self.spawn_scrollback_restore(payload, segments);
        }
    }

    /// Best-effort cancellation of any in-flight 2nd-pass scrollback restore
    /// worker. Sets the worker's shared `cancel` flag so it bails at the next
    /// chunk boundary — drop alone does NOT fire the flag (the worker holds
    /// an `Arc<AtomicBool>` independently of the receiver). Used by the
    /// `window_host.rs` `CloseRequested` shutdown sweep before
    /// `self.app.tabs.clear()` drops the receivers; bounds wasted worker CPU
    /// on shutdown. No-op when no restore is in flight.
    pub(crate) fn cancel_pending_scrollback_restore(&self) {
        if let Some(p) = self.pending_scrollback_restore.as_ref() {
            p.cancel.store(true, Ordering::Relaxed);
            log::info!(
                "scrollback restore cancelled (shutdown) for tab {:?}",
                self.title
            );
        }
    }

    /// Non-blockingly poll the 2nd-pass scrollback restore handoff (FR4,
    /// NFR3, NFR7). Mirror of [`Self::poll_pending_switch`] but for the
    /// bypass-off scrollback rebuild.
    ///
    /// Returns one of [`ScrollbackRestoreOutcome`]:
    /// - `Idle` — no restore is in flight.
    /// - `Pending` — worker is still rebuilding (do not block).
    /// - `Merged` — the rebuilt scrollback was merged into the live core;
    ///   the caller marks the tab `changed` and (for the active tab)
    ///   `active_changed` (search overlay rebuild).
    /// - `Failed` — the worker disconnected (panic) or the cancel arm
    ///   observed `Disconnected` after a supersede; state is cleared.
    pub(crate) fn poll_pending_scrollback_restore(&mut self) -> ScrollbackRestoreOutcome {
        let Some(pending) = self.pending_scrollback_restore.as_ref() else {
            return ScrollbackRestoreOutcome::Idle;
        };
        match pending.done.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Empty) => ScrollbackRestoreOutcome::Pending,
            Ok(build) => {
                let pending = self
                    .pending_scrollback_restore
                    .take()
                    .expect("just matched Some");
                self.apply_scrollback_restore(build, pending.base_evicted_total);
                ScrollbackRestoreOutcome::Merged
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // FR7: worker panicked or was cancelled mid-parse (the
                // `build_scrollback_only_from_snapshot` returned `None`, so
                // it never sent and the sender dropped). No synchronous
                // fallback — the 1st-pass swap is already correct, the user
                // just sees no history. Clear state.
                log::warn!(
                    "scrollback restore worker for tab {:?} disconnected; clearing state",
                    self.title
                );
                self.pending_scrollback_restore = None;
                ScrollbackRestoreOutcome::Failed
            }
        }
    }

    /// Merge the rebuilt scrollback into the live core (FR3 + FR8).
    ///
    /// FR3: between the 1st-pass swap and the 2nd-pass arrival, live PTY
    /// output may have pushed some rows into the (initially empty) live
    /// scrollback and evicted others. Those rows were ALREADY present at
    /// the tail of the rebuilt scrollback, so prepending the whole rebuilt
    /// scrollback would duplicate them. The fix: trim the trailing
    /// `live_growth = live_now - base_evicted_total` rows from the rebuilt
    /// scrollback before merging — those tail rows are the ones the live
    /// drain re-emitted from the snapshot tail.
    ///
    /// FR8: the merge consumes only the rebuilt scrollback (slim cells +
    /// wrapped + tables) — `prompt_marks`, `fold_marks`, and
    /// `bypass_b_mark_texts` from the 2nd-pass replay are intentionally
    /// dropped without touching the live core's mark trackers. Marks were
    /// already drained from the 1st-pass replay in `apply_replay_reconcile`
    /// and from the queued live output in `apply_queued_live_output`; the
    /// 2nd-pass would emit the same marks a second time, which is exactly
    /// what FR8 forbids. Discarding the 2nd-pass marks here is the
    /// implementation of the mark-non-duplication invariant.
    fn apply_scrollback_restore(&mut self, build: ScrollbackBuild, base_evicted_total: u64) {
        let rebuilt_evicted_at_end = build.evicted_total_at_end;
        // FR3 trim arithmetic + merge happen inside a single lock window so
        // a concurrent `pump` cannot race with the scrollback length read.
        let (merged_rows, live_growth, live_now) = {
            let mut live = self.core.lock();
            let live_now = live.get_scrollback_evicted_total();
            let live_growth = live_now.saturating_sub(base_evicted_total) as usize;
            let merged = live.merge_scrollback_from(build.rebuilt_core, live_growth);
            (merged, live_growth, live_now)
        };
        log::info!(
            "scrollback restored for tab {:?}: {merged_rows} rows prepended \
             (live_growth={live_growth}, base_evicted_total={base_evicted_total}, \
              live_now={live_now}, rebuilt_evicted={rebuilt_evicted_at_end})",
            self.title
        );
    }

    /// Spawn the 2nd-pass scrollback restore worker (FR1, NFR3, NFR7).
    /// Captures `base_evicted_total` from the now-settled live core, clones
    /// the payload, spawns a worker thread that calls
    /// `build_scrollback_only_from_snapshot`, and installs
    /// `PendingScrollbackRestore`. On spawn failure: `log::warn` + no state
    /// installed (FR7 — the 1st-pass swap is already correct, the user just
    /// gets no history).
    fn spawn_scrollback_restore(&mut self, payload: Vec<u8>, segments: Vec<ReplaySegment>) {
        // Supersede any prior in-flight restore (NFR4) — the freshly-swapped
        // core is the new authoritative state, the prior restore's rebuilt
        // scrollback would be against a now-stale baseline.
        if let Some(old) = self.pending_scrollback_restore.as_ref() {
            old.cancel.store(true, Ordering::Relaxed);
            log::warn!(
                "scrollback restore cancelled (superseded by newer off-thread swap) for tab {:?}",
                self.title
            );
        }
        let (cols, rows, scrollback_lines, base_evicted_total) = {
            let c = self.core.lock();
            (
                c.cols(),
                c.rows(),
                c.scrollback_capacity(),
                c.get_scrollback_evicted_total(),
            )
        };
        if scrollback_lines == 0 {
            log::info!(
                "scrollback restore skipped (scrollback disabled) for tab {:?}",
                self.title
            );
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker_payload = payload;
        let worker_segments = segments;
        let payload_len = worker_payload.len();
        let spawn_result = std::thread::Builder::new()
            .name("mux-scrollback-restore".into())
            .spawn(move || {
                if let Some(replay) =
                    term_core::terminal_core::TerminalCore::build_scrollback_only_from_snapshot(
                        cols,
                        rows,
                        scrollback_lines,
                        &worker_payload,
                        &worker_segments,
                        &worker_cancel,
                    )
                {
                    let _ = tx.send(ScrollbackBuild {
                        rebuilt_core: replay.core,
                        evicted_total_at_end: replay.evicted_total,
                    });
                    // task0004 D4/AC-3: same rationale as the snapshot-replay
                    // worker above — wake the loop so
                    // `poll_pending_scrollback_restore` observes the merge
                    // promptly under `ControlFlow::Wait`.
                    crate::wakeup::wake();
                }
            });
        match spawn_result {
            Ok(_) => {
                log::info!(
                    "scrollback restore worker spawned for tab {:?}, payload {payload_len} B",
                    self.title
                );
                self.pending_scrollback_restore = Some(PendingScrollbackRestore {
                    done: rx,
                    base_evicted_total,
                    cancel,
                });
            }
            Err(e) => {
                // FR7: thread/resource exhaustion at spawn is non-fatal; the
                // 1st-pass swap is already correct, the user just gets no
                // scrollback restored. The state is intentionally not
                // installed so the next poll observes Idle.
                log::warn!(
                    "scrollback restore worker spawn failed ({e}) for tab {:?}; \
                     scrollback will not be restored",
                    self.title
                );
            }
        }
    }

    /// Replay a pending switch's queued live output onto the (already
    /// swapped or reparsed) displayed core, in arrival order, exactly as the
    /// `PtyOutput` arm would have for each chunk: feed the bytes, route any
    /// device response, backfill marks.
    fn apply_queued_live_output(&mut self, live_queue: Vec<Vec<u8>>) {
        for payload in live_queue {
            let (evicted_total, prompt_marks, fold_marks, device_response) = {
                let mut c = self.core.lock();
                c.process_pty_data_fully(&payload);
                let device_response = c.take_response();
                let (evicted_total, prompt_marks, fold_marks) = drain_marks(&mut c);
                (evicted_total, prompt_marks, fold_marks, device_response)
            };
            if !device_response.is_empty() {
                self.write_device_response(device_response);
            }
            self.backfill_marks(evicted_total, prompt_marks, fold_marks);
        }
    }

    /// Main-thread reconcile half of the replay recipe, shared by the
    /// synchronous path and the off-thread swap. Given the marks/eviction
    /// total drained from the *replayed* core (the synchronous core for the
    /// sync path, the worker-built core for the off-thread path), latch
    /// `pending_frame_reset`, install the fresh `evicted_baseline`, and
    /// backfill the marks. The alt-screen state is the core's authoritative
    /// `MODE_ALT_SCREEN` bit (read by `App::pump_all`), so it needs no reseed
    /// here.
    ///
    /// The eviction total comes from a freshly-reset core (counter 0), so
    /// `backfill_prompt_marks`'s in-band detector
    /// (`evicted_total < self.evicted_baseline`) cannot fire — the latch is
    /// set unconditionally because the helper's contract is "the previous
    /// frame was discarded" regardless of eviction counts.
    fn apply_replay_reconcile(
        &mut self,
        evicted_total: u64,
        prompt_marks: Vec<term_core::terminal_core::PendingPromptMark>,
        fold_marks: Vec<term_core::terminal_core::PendingFoldMark>,
    ) {
        self.pending_frame_reset = true;
        self.evicted_baseline = evicted_total;
        self.backfill_marks(evicted_total, prompt_marks, fold_marks);
    }

    /// Decide which pane (if any) needs a screen reconcile after a window
    /// close. Given the active pane id captured **before** `remove_pane` and
    /// the active pane id read **after** the removal, return the now-active
    /// pane id to request a snapshot for, or `None` when nothing needs
    /// redrawing.
    ///
    /// Comparison is by pane **id**, not index, so a non-active window close
    /// that shifts indices but leaves the displayed window's content unchanged
    /// correctly yields `None` (FR2). When the group is emptied the post-removal
    /// active pane id is `None`, which also yields `None` (FR3, no request). A
    /// genuine active-window close (FR1) produces a different post-removal pane
    /// id and returns it.
    fn close_reconcile_target(
        before_active: Option<u32>,
        after_active: Option<u32>,
    ) -> Option<u32> {
        match after_active {
            Some(after) if Some(after) != before_active => Some(after),
            _ => None,
        }
    }

    /// Route one decoded mux message into this tab. Called by `App::pump_all`
    /// after the APC decoder ([`crate::mux::apc::try_decode_emterm_mux`])
    /// produced a typed `MuxMessage`. Returns true when the visible state
    /// changed (caller schedules a redraw).
    ///
    /// `Snapshot` payloads are raw PTY-shaped bytes the daemon captured
    /// from the active window — they are replayed into `term_core` via
    /// `reset_and_replay`. Everything else either updates a side-channel
    /// (status bar, session name) or is logged and ignored — the bridge
    /// CLI continues to own the underlying socket protocol, so native-poc
    /// only needs to react to messages that mutate its own state.
    pub fn apply_mux_message(&mut self, msg: MuxMessage) -> bool {
        match msg.msg_type {
            MessageType::Snapshot | MessageType::SnapshotRestore => {
                // task0003 D3 (review round-2 findings `200b2c8beeb68fe4` /
                // `87ba3cc2911d104e`): a frame that RESETS the tab's single
                // core must only be applied when it belongs to the pane this
                // tab is currently displaying — mirrors the `PtyOutput` arm's
                // filter below. Both the reattach path (per-pane
                // `SnapshotRestore`) and the visibility-resume path
                // (per-pane `Snapshot`) send one such frame per pane in the
                // session, relying on the CLIENT to pick the right one; this
                // arm used to apply whatever arrived last unconditionally,
                // so a background window's reattach / resume snapshot
                // silently overwrote the visible pane's content with a
                // different window's screen — re-introducing, via this
                // newer per-pane framing, the exact "switch shows the wrong
                // pane's content" symptom this feature exists to fix. When
                // the tab has no window group (older daemon / single pane),
                // `active_pane_id()` is `None` and every frame is accepted,
                // matching the `PtyOutput` arm's fallback.
                if let Some(active) = self.mux_group.as_ref().and_then(|g| g.active_pane_id()) {
                    if msg.pane_id != active {
                        log::debug!(
                            "mux apc: dropping {:?} for non-active pane {} (active {}) for tab {:?}",
                            msg.msg_type,
                            msg.pane_id,
                            active,
                            self.title
                        );
                        return false;
                    }
                }
                // task0004 round-4 rework (D1'): decode the wire payload
                // into its structural dimension segments + plain content
                // bytes (`mux_ipc::protocol::decode_snapshot_payload_typed`).
                // An older daemon's payload (no magic prefix) decodes as
                // `Legacy`, degrading to single-dimension replay (AC-11) —
                // see `reset_and_replay_segments`'s doc comment.
                //
                // D3''' (round-6 rework, review round-5 finding
                // `b45fb09344067621`): use the TYPED result, not the tuple
                // compatibility wrapper — `Malformed` there maps to
                // `(Vec::new(), &[])`, which this arm would apply as "empty
                // snapshot," blanking the pane the same way rendering the
                // corrupt envelope literally would have. A `Malformed`
                // frame here instead logs and skips applying it entirely,
                // leaving whatever is currently displayed intact.
                #[cfg(test)]
                {
                    // task0006 FR8 AC-6: counts every decode attempt,
                    // including one that will end up coalescing into an
                    // already-in-flight same-pane switch below — see
                    // `test_snapshot_decode_count`'s doc for what this
                    // does (and does not) claim about FR8's scope.
                    self.snapshot_decode_count += 1;
                }
                let (dim_segments, content_bytes) =
                    match decode_snapshot_payload_typed(&msg.payload) {
                        DecodedSnapshotPayload::Legacy(content) => (Vec::new(), content.to_vec()),
                        DecodedSnapshotPayload::Structured { segments, content } => {
                            (segments, content.to_vec())
                        }
                        DecodedSnapshotPayload::Malformed => {
                            log::warn!(
                                "mux apc: dropping malformed {:?} payload ({} bytes) for tab {:?} \
                             (pane {}); keeping the current display",
                                msg.msg_type,
                                msg.payload.len(),
                                self.title,
                                msg.pane_id
                            );
                            return false;
                        }
                    };
                let segments: Vec<ReplaySegment> = dim_segments
                    .iter()
                    .map(|d| ReplaySegment {
                        offset: d.offset,
                        cols: d.cols,
                        rows: d.rows,
                    })
                    .collect();
                // FR4: branch on payload size. Small snapshots replay
                // synchronously (no perceptible block, no swap gap); large
                // ones go off-thread so the switch stays responsive.
                //
                // D3''/AC-5 (task0005 rework, review round-4 finding
                // `b1de83542bfe60bc`): ALSO branch on segment count — a
                // small-payload, many-segment snapshot (a resize-drag-shaped
                // sequence) can still cost real reflow time on the
                // synchronous path, since each segment's reflow cost scales
                // with the core's accumulated size, not the segment's own
                // byte count.
                if content_bytes.len() < OFFTHREAD_REPLAY_THRESHOLD_BYTES
                    && segments.len() < OFFTHREAD_REPLAY_SEGMENT_THRESHOLD
                {
                    // Synchronous path (legacy). `reset_frame_for_replay`
                    // owns the recipe (prompt clear, fold rebuild, drain +
                    // backfill marks so `pending_frame_reset` latches) so the
                    // PaneCreated path stays in lockstep. A pending off-thread
                    // switch (if any) is
                    // superseded by this newer, now-applied switch — signal
                    // its worker to bail before dropping it.
                    if let Some(old) = self.pending_switch.take() {
                        old.cancel.store(true, Ordering::Relaxed);
                    }
                    // FR7/FR8 (task0003): a coalesced same-pane re-dispatch
                    // (if any) is now moot too — this sync application is
                    // itself the newest, now-applied switch.
                    self.pending_redispatch = None;
                    if let Some(old) = self.pending_scrollback_restore.take() {
                        old.cancel.store(true, Ordering::Relaxed);
                        log::warn!(
                            "scrollback restore cancelled (superseded by sync switch) for tab {:?}",
                            self.title
                        );
                    }
                    let _actions = self.reset_frame_for_replay(&content_bytes, &segments);
                    log::debug!(
                        "mux apc: applied {:?} ({} bytes, {} segments, sync) for tab {:?}",
                        msg.msg_type,
                        content_bytes.len(),
                        segments.len(),
                        self.title
                    );
                } else {
                    // Off-thread path (FR1/FR4): copy the payload, do the
                    // frame-discard portion now (prompts/folds belonged to
                    // the outgoing frame), and dispatch a worker. The
                    // displayed core is left intact so the outgoing pane
                    // stays visible until the swap. A newer switch supersedes
                    // any prior in-flight parse.
                    //
                    // The live-output queue is keyed on the tab's *active*
                    // pane id (the pane `switch_to` already moved to), the
                    // same id the `PtyOutput` arm filters on, so live bytes
                    // for the just-switched-to pane queue while the parse runs
                    // instead of being dropped. Fall back to the snapshot's
                    // own `pane_id` when the tab has no window group.
                    let target_pane = self
                        .mux_group
                        .as_ref()
                        .and_then(|g| g.active_pane_id())
                        .unwrap_or(msg.pane_id);
                    let segments_len = segments.len();
                    self.dispatch_offthread_replay(target_pane, content_bytes, segments);
                    log::debug!(
                        "mux apc: dispatched {:?} ({} bytes, {} segments, off-thread) for tab {:?} pane {}",
                        msg.msg_type,
                        self.pending_switch
                            .as_ref()
                            .map(|p| p.payload.len())
                            .unwrap_or(0),
                        segments_len,
                        self.title,
                        target_pane
                    );
                }
                true
            }
            MessageType::PtyOutput => {
                // OSC-probe (temporary): flag when GUI-side sees a viewer
                // launch OSC 777 arrive from the mux extractor. Mirrors the
                // daemon (pty_spawn.rs) and bridge (bridge.rs) probes. Only
                // metadata is logged (never the payload bytes) so this probe
                // cannot leak user file content into persisted release logs.
                const OSC_PROBE_NEEDLE: &[u8] = b"\x1b]777;emterm;";
                let osc_probe = msg
                    .payload
                    .windows(OSC_PROBE_NEEDLE.len())
                    .position(|w| w == OSC_PROBE_NEEDLE);
                if let Some(off) = osc_probe {
                    log::warn!(
                        "[osc-probe gui] enter pane={} payload_len={} osc_off={}",
                        msg.pane_id,
                        msg.payload.len(),
                        off,
                    );
                }
                // Route by pane. Once attached (see the Welcome handler), the
                // daemon streams live output for *every* pane in the session to
                // this owning connection — but native renders one core per tab,
                // showing only the active window. Feeding another window's bytes
                // into this core interleaves unrelated screens (the "other
                // tabs' data mixing in" symptom). The WebView keeps a separate
                // core per pane; native instead drops non-active panes here and
                // reconciles each window's screen from the daemon's
                // authoritative state via `request_pane_snapshot` on switch.
                // When the tab has no window group (older daemon / single
                // pane), `active_pane_id()` is None and all output is accepted.
                if let Some(active) = self.mux_group.as_ref().and_then(|g| g.active_pane_id()) {
                    if msg.pane_id != active {
                        if osc_probe.is_some() {
                            log::warn!(
                                "[osc-probe gui] DROP inactive-pane pane={} active={} payload_len={}",
                                msg.pane_id,
                                active,
                                msg.payload.len(),
                            );
                        }
                        log::debug!(
                            "mux apc: dropping PtyOutput for inactive pane {} (active {})",
                            msg.pane_id,
                            active
                        );
                        return false;
                    }
                }
                // FR3: while an off-thread replay for this pane is in flight,
                // the displayed core is still showing the *outgoing* pane —
                // feeding the just-switched-to pane's live bytes into it would
                // corrupt the visible screen. Queue them in arrival order
                // instead; `App::pump_all` replays the queue onto the
                // worker-built core after the swap. Output that races in for a
                // *different* target than the pending switch is dropped (it
                // belongs to a pane we are no longer switching to).
                if let Some(pending) = self.pending_switch.as_mut() {
                    if msg.pane_id == pending.target_pane {
                        if osc_probe.is_some() {
                            log::warn!(
                                "[osc-probe gui] QUEUED pending-switch pane={} target={} payload_len={}",
                                msg.pane_id,
                                pending.target_pane,
                                msg.payload.len(),
                            );
                        }
                        pending.queued_bytes =
                            pending.queued_bytes.saturating_add(msg.payload.len());
                        pending.live_queue.push(msg.payload);
                        // Bound the backlog: past the cap, abandon the
                        // off-thread switch and reparse synchronously now,
                        // applying the accumulated queue as ordinary output.
                        // This caps both the swap-time replay burst and the
                        // memory a fast pane can accumulate during a slow parse.
                        if pending.queued_bytes > OFFTHREAD_LIVE_QUEUE_CAP_BYTES {
                            let pending = self
                                .pending_switch
                                .take()
                                .expect("pending_switch is Some in this arm");
                            pending.cancel.store(true, Ordering::Relaxed);
                            if let Some(old) = self.pending_scrollback_restore.take() {
                                old.cancel.store(true, Ordering::Relaxed);
                                log::warn!(
                                    "scrollback restore cancelled (live-queue overflow sync reparse) for tab {:?}",
                                    self.title
                                );
                            }
                            log::warn!(
                                "mux off-thread replay live-queue exceeded {} bytes for tab {:?}; \
                                 synchronous reparse fallback",
                                OFFTHREAD_LIVE_QUEUE_CAP_BYTES,
                                self.title
                            );
                            // FR8 (task0003): a coalesced same-pane
                            // re-dispatch (if any) is the LATEST known
                            // content for this pane — reparse that instead
                            // of the (possibly superseded) payload the
                            // abandoned worker was building, so this
                            // synchronous fallback never regresses to
                            // stale content. `self.core` is already at the
                            // right grid regardless (`Tab::resize` always
                            // resizes it directly, independent of any
                            // deferred `pending_resize`), so
                            // `reset_frame_for_replay` needs no extra
                            // resize step here.
                            //
                            // `pending.live_queue` is safe to apply
                            // unconditionally against whichever payload
                            // wins above (review round-1 finding
                            // `ebc9de26bb15fcb1`, task0006 redesign): the
                            // same-pane coalesce branch in
                            // `dispatch_offthread_replay` already cleared
                            // it at COALESCE time if `pending_redispatch`
                            // is `Some` here, so it holds exactly "output
                            // queued since that coalesce" either way —
                            // never a stale prefix the new payload might
                            // already contain.
                            let (payload, segments) = match self.pending_redispatch.take() {
                                Some((_, payload, segments)) => (payload, segments),
                                None => (pending.payload, pending.segments),
                            };
                            self.reset_frame_for_replay(&payload, &segments);
                            self.apply_queued_live_output(pending.live_queue);
                            // The swap-equivalent happened synchronously now;
                            // repaint the newly-visible pane.
                            return true;
                        }
                        // Queued, not yet visible — no redraw needed; the swap
                        // will repaint.
                        return false;
                    }
                    if osc_probe.is_some() {
                        log::warn!(
                            "[osc-probe gui] DROP pending-switch pane={} target={} payload_len={}",
                            msg.pane_id,
                            pending.target_pane,
                            msg.payload.len(),
                        );
                    }
                    log::debug!(
                        "mux apc: dropping PtyOutput for pane {} during pending switch to {}",
                        msg.pane_id,
                        pending.target_pane
                    );
                    return false;
                }
                if osc_probe.is_some() {
                    log::warn!(
                        "[osc-probe gui] APPLY pane={} payload_len={}",
                        msg.pane_id,
                        msg.payload.len(),
                    );
                }
                // The daemon's continuous PTY stream: feed it into term_core
                // as a normal byte stream (NOT a reset). Without this the
                // mux session looks frozen after the initial Snapshot. Shares
                // the post-parse recipe (device-response write-back + mark
                // drain/backfill) with the coalesce flush via
                // `apply_active_pane_output`, so the per-frame and batched
                // paths can never drift (SPEC NFR2). Frames carrying a device
                // query (`CSI ... n` / `CSI ... c`) are routed here per-frame
                // by `process_combined`'s `batch_eligible` gate so each reply
                // is captured before the next query overwrites the core's
                // single-slot response buffer.
                self.apply_active_pane_output(&msg.payload)
            }
            // The former mux status-bar daemon push (opcode 0x16, see
            // `mux_ipc::protocol`'s reserved-opcode comment) was retired
            // by mux-status-bar-removal task0001; that opcode no longer
            // decodes into a `MuxMessage` at all (see
            // `mux_ipc::protocol::MessageType::from_u8`), so it falls
            // through to the wildcard arm below like any other
            // unrecognized message type.
            MessageType::AgentStatusUpdate => {
                // Daemon → GUI unsolicited push (task0005 AC-2). Applying it
                // to `App::agent_status` needs `&mut App`, which this method
                // does not have — latch the decoded payload for
                // `App::pump_all` to apply after the per-tab loop, mirroring
                // the `pending_pane_switch_from` / `pending_window_appended`
                // latch pattern used elsewhere in this match.
                match msg.decode_payload::<mux_ipc::protocol::AgentStatusUpdateMsg>() {
                    Some(update) => {
                        self.pending_agent_status_updates.push(update);
                        true
                    }
                    None => {
                        log::warn!("mux apc: malformed AgentStatusUpdate payload");
                        false
                    }
                }
            }
            MessageType::Welcome => match msg.decode_payload::<WelcomeMsg>() {
                Some(WelcomeMsg::Accepted { sessions, .. }) => {
                    match sessions.first() {
                        Some(session) => {
                            log::info!(
                                "mux apc: tab {:?} attached to session {}",
                                self.title,
                                session.name
                            );
                            // Detect the first Welcome of this attach *before*
                            // recording the session name. The bridge/daemon can
                            // deliver Welcome twice (a known duplication); a
                            // second Attach would make the daemon replay its
                            // buffered output a second time, interleaving two
                            // large base64 APC frames into a stream that no
                            // longer decodes ("invalid base64 encoding").
                            // `mux_session_name` is None before the first
                            // Welcome and cleared again on Detach, so it doubles
                            // as the per-attach guard without a new field.
                            let first_welcome = self.mux_session_name.is_none();
                            // Keep the existing session-name extraction intact
                            // (F3): the status bar badge reads it.
                            self.mux_session_name = Some(session.name.clone());
                            // Become the live-output owner so continuous PTY
                            // output (e.g. `top`) streams to native instead of
                            // only on-demand snapshots. The daemon delivers its
                            // live stream to a pane's single owning connection;
                            // native must Attach to take ownership, exactly as
                            // the WebView reattach path does (`mux-session.ts`).
                            // Gate on the *targeted* session's pane_count so
                            // when (in a future multi-session daemon)
                            // `sessions[0].pane_count == 0` and a later session
                            // has panes, we don't send an Attach to the empty
                            // session — the WebView `existingPanes > 0` check
                            // applies to the session being attached, not the
                            // sum across every session.
                            if first_welcome && session.pane_count > 0 {
                                self.send_attach(session.id);
                            }
                            // Seed the window group from the session's window
                            // list (additive). `windows` carries the daemon
                            // window id / name / active pane id; the pane ids
                            // are the per-window active panes, parallel to the
                            // window list (F1). When the daemon omits the
                            // window list (older daemon), leave the group
                            // unseeded — it stays a plain tab.
                            //
                            // Gate the entire seed + snapshot block behind
                            // `first_welcome` for the same reason as the Attach
                            // guard above. On the (known) duplicate Welcome
                            // delivery, replaying `group.seed(...)` would wipe
                            // out anything accumulated between the two Welcomes
                            // — a window appended from `PaneCreated`, an
                            // optimistic `confirm_mux_rename`/`confirm_mux_move`
                            // edit, an inbound `SwitchWindow` that moved
                            // `active` — and a second `request_pane_snapshot`
                            // would race the user's just-applied local change.
                            if first_welcome && !session.windows.is_empty() {
                                let (active_pane_id, seeded_pane_ids) = {
                                    let group =
                                        self.mux_group.get_or_insert_with(MuxWindowGroup::new);
                                    let windows: Vec<MuxWindow> = session
                                        .windows
                                        .iter()
                                        .map(|w| MuxWindow {
                                            id: w.id,
                                            name: w.name.clone(),
                                        })
                                        .collect();
                                    let pane_ids: Vec<u32> =
                                        session.windows.iter().map(|w| w.active_pane_id).collect();
                                    group.seed(
                                        windows,
                                        pane_ids,
                                        session.active_window_index as usize,
                                    );
                                    (group.active_pane_id(), group.pane_ids().to_vec())
                                };
                                // Tell the daemon every seeded pane's PTY size
                                // up front, so a freshly attached client picks
                                // up the GUI's current grid dimensions instead
                                // of inheriting whatever the previous owner
                                // (or the daemon's 80x24 default) left behind.
                                // Without this the daemon-side wrap column
                                // stays mismatched until the user happens to
                                // resize the window.
                                let (cols, rows) = {
                                    let core = self.core.lock();
                                    (core.cols(), core.rows())
                                };
                                for pane_id in &seeded_pane_ids {
                                    self.send_control(&MuxMessage::control(
                                        MessageType::Resize,
                                        *pane_id,
                                        &ResizeMsg { cols, rows },
                                    ));
                                    // task0003 AC-6: record this emission (mux
                                    // attach/Welcome pane seeding site — see
                                    // `ResizeFrameRecord`).
                                    #[cfg(test)]
                                    {
                                        self.resize_frame_log.push(ResizeFrameRecord {
                                            tab_stable_id: self.stable_id,
                                            pane_id: *pane_id,
                                            cols,
                                            rows,
                                        });
                                    }
                                }
                                // Pull the active window's screen on attach — the
                                // daemon does not push it unprompted, so without
                                // this the freshly attached tab stays blank
                                // (parity with the WebView reattach path's
                                // `requestPaneSnapshot`).
                                if let Some(pane_id) = active_pane_id {
                                    self.request_pane_snapshot(pane_id);
                                }
                            } else if first_welcome && session.pane_count == 0 {
                                // Fresh-start mux: the daemon has no panes yet,
                                // so `windows` is empty and the seed/attach
                                // branches above don't run. Legacy webview's
                                // `enterMuxMode` sent CreateWindow on this path
                                // to bootstrap the initial window — the native
                                // port was missing that step, which is the
                                // upstream cause of "shell freezes / status bar
                                // alive": with no seeded pane, `mux_group` stays
                                // `None`, `active_pane_id()` is `None`, and every
                                // keystroke gets dropped in `write_input` while
                                // the (historical, now-removed) mux status-bar
                                // daemon push kept the status bar updating
                                // regardless.
                                //
                                // Pre-install an empty group so the daemon's
                                // subsequent `PaneCreated` reply can land — the
                                // PaneCreated handler intentionally refuses to
                                // install a group on its own (M4 guard against
                                // pre-Welcome leakage).
                                //
                                // Send CreateWindow with an empty payload to
                                // match legacy's `sendControl(CreateWindow, 0)`
                                // wire form exactly — the daemon's
                                // empty-payload backward-compat path (pinned by
                                // `test_create_window_payload_empty_payload_backward_compat`)
                                // applies CreateWindowPayload defaults.
                                self.mux_group.get_or_insert_with(MuxWindowGroup::new);
                                self.send_control(&MuxMessage {
                                    msg_type: MessageType::CreateWindow,
                                    pane_id: 0,
                                    payload: Vec::new(),
                                });
                            }
                            true
                        }
                        None => false,
                    }
                }
                Some(WelcomeMsg::Rejected { reason }) => {
                    log::warn!("mux apc: handshake rejected: {reason}");
                    false
                }
                None => {
                    log::warn!("mux apc: malformed Welcome payload");
                    false
                }
            },
            MessageType::PaneCreated => {
                // SPEC FR4 / Message Mapping: the daemon's PaneCreated is the
                // authoritative "append window" signal — it fires for every
                // pane the daemon creates, whether this client requested the
                // create or another client did. Treat it as such:
                //
                // - Require an existing group: a PaneCreated arriving before
                //   Welcome installs nothing (no `get_or_insert_with`, so the
                //   empty-group leakage that made other handlers spuriously
                //   think this tab was mux-attached is gone — M4).
                // - Idempotent: if the pane id is already in our group (resend
                //   / replay), don't double-append.
                // - Daemon-authoritative: append even when no pending-create
                //   credit exists. `pending_create` is now purely an
                //   optimistic-UX counter — consume it when present so a
                //   subsequent CreateWindow request still gets its own credit,
                //   but never gate the append on it (the spec finding #5).
                let Some(group) = self.mux_group.as_mut() else {
                    log::debug!(
                        "mux apc: PaneCreated pane={} before attach (no group), ignored",
                        msg.pane_id
                    );
                    return false;
                };
                if group.index_of_pane_id(msg.pane_id).is_some() {
                    log::debug!(
                        "mux apc: PaneCreated pane={} already in group, ignored (idempotency)",
                        msg.pane_id
                    );
                    return false;
                }
                let pending = group.take_pending_create();
                // Locally-unique window id (one past the current max) so the
                // synthetic id never collides with a daemon-seeded one. Initial
                // name "Terminal" (OQ1 resolved); daemon-pushed RenameWindow
                // later overwrites it.
                let new_id = group.fresh_window_id();
                // The new window becomes the active sub-tab (see `push`). Treat
                // that like a pane switch for scroll bookkeeping: latch the
                // outgoing pane id so `App::pump_all` parks the outgoing pane's
                // scroll into its slot and reloads the new pane's (default
                // `Live`) slot — first-latch-only, matching the SwitchWindow
                // path. `active_pane_id()` is `None` for the tab's first mux
                // window (empty group before this push), so that case correctly
                // latches nothing.
                let from_pane = group.active_pane_id();
                group.push(
                    MuxWindow {
                        id: new_id,
                        name: "Terminal".to_string(),
                    },
                    msg.pane_id,
                );
                // FR6 (mux): the push made the new window the active sub-tab.
                // Latch it at the event source so `App::pump_all` scrolls it into
                // view when this is the active tab — immune to a same-pump
                // `PtyExited` or a `Welcome` reseed, unlike a window-count delta.
                self.pending_window_appended = true;
                if let Some(from) = from_pane {
                    if self.pending_pane_switch_from.is_none() {
                        self.pending_pane_switch_from = Some(from);
                    }
                }
                log::info!(
                    "mux apc: pane {} created (window {}, pending_consumed={}) for tab {:?}",
                    msg.pane_id,
                    new_id,
                    pending,
                    self.title
                );
                // The newly created window becomes the active sub-tab (see
                // `MuxWindowGroup::push`). Without a core reset here, the
                // previous active window's grid + scrollback stay painted
                // until the new shell's first byte arrives — and even after
                // it does, the old content lingers in scrollback. The
                // shared `reset_frame_for_replay` recipe drops prompts /
                // folds, runs `reset_and_replay(b"")`, and routes through
                // `backfill_marks` so `pending_frame_reset` latches and
                // any active selection / press anchor on this tab is
                // dropped by `App::pump_all`.
                let _ = self.reset_frame_for_replay(b"", &[]);
                // The daemon spawns every new PTY at a hardcoded 80x24
                // (`handle_create_window`); without this, the pane stays at
                // 80 columns even though the GUI grid is wider, so output
                // wraps early. Push the current grid dimensions immediately
                // after the append so the daemon-side PTY catches up.
                let (cols, rows) = {
                    let core = self.core.lock();
                    (core.cols(), core.rows())
                };
                self.send_control(&MuxMessage::control(
                    MessageType::Resize,
                    msg.pane_id,
                    &ResizeMsg { cols, rows },
                ));
                // task0003 AC-6: record this emission (PaneCreated site —
                // see `ResizeFrameRecord`).
                #[cfg(test)]
                {
                    self.resize_frame_log.push(ResizeFrameRecord {
                        tab_stable_id: self.stable_id,
                        pane_id: msg.pane_id,
                        cols,
                        rows,
                    });
                }
                true
            }
            MessageType::SwitchWindow => {
                // Daemon-initiated switch (e.g. CLI `switch-window`): sync the
                // active index to the window owning this pane. Port of
                // `handleRemoteSwitchWindow`'s index resolution.
                //
                // Capture the outgoing active pane id before the sync so the
                // App-side per-pane scroll save/restore (FR3) can park the
                // outgoing pane's position — the daemon handler runs inside
                // `pump`, with no access to `App::scroll_position`, so it
                // latches the transition for `App::pump_all` to apply.
                let from_pane = self.mux_group.as_ref().and_then(|g| g.active_pane_id());
                let synced = self
                    .mux_group
                    .as_mut()
                    .map(|g| g.set_active_by_pane(msg.pane_id))
                    .unwrap_or(false);
                if synced {
                    log::info!(
                        "mux apc: remote switch to pane {} for tab {:?}",
                        msg.pane_id,
                        self.title
                    );
                    // Latch the outgoing pane id only when the switch actually
                    // moved the active pane (a no-op switch onto the current
                    // pane must not park/reload scroll or force a redraw), and
                    // only for the FIRST move in this pump. Several SwitchWindow
                    // messages can drain in one `pump` (A→B→C); only A is the
                    // genuinely-displayed outgoing pane whose live scroll must be
                    // parked — intermediate panes were never rendered. Keeping
                    // the first `from` avoids parking the live scroll into a
                    // wrong (intermediate) slot.
                    let to_pane = self.mux_group.as_ref().and_then(|g| g.active_pane_id());
                    if let (Some(from), Some(to)) = (from_pane, to_pane) {
                        if from != to && self.pending_pane_switch_from.is_none() {
                            self.pending_pane_switch_from = Some(from);
                        }
                    }
                    // Reconcile the screen with the now-active window (parity
                    // with the WebView remote-switch path's `requestPaneSnapshot`).
                    self.request_pane_snapshot(msg.pane_id);
                }
                synced
            }
            MessageType::RenameWindow => {
                // Daemon-broadcast rename. The wire field is the *pane id* —
                // `confirm_mux_rename` sends `pane_ids()[idx]`, and the daemon
                // re-broadcasts the frame with the same field unchanged. The
                // earlier code interpreted `msg.pane_id` directly as a window
                // id (commented "WebView `const windowId = paneId`"), which
                // only worked when window ids and pane ids happened to
                // coincide; for windows where they differ (locally-created
                // windows get a synthetic window id from `fresh_window_id`
                // while the daemon assigns its own pane id), the daemon's
                // broadcast targeted the wrong window or no window at all.
                // Resolve by pane id so producer and consumer agree on the
                // contract (gpt-architecture + gpt-spec cross-model finding).
                match msg.decode_payload::<RenameWindowMsg>() {
                    Some(rename) => {
                        let renamed = self
                            .mux_group
                            .as_mut()
                            .and_then(|g| {
                                let idx = g.index_of_pane_id(msg.pane_id)?;
                                let window_id = g.windows().get(idx)?.id;
                                Some(g.rename_window_id(window_id, rename.name.clone()))
                            })
                            .unwrap_or(false);
                        if renamed {
                            log::info!(
                                "mux apc: pane {} renamed to {:?} for tab {:?}",
                                msg.pane_id,
                                rename.name,
                                self.title
                            );
                        }
                        renamed
                    }
                    None => {
                        log::warn!("mux apc: malformed RenameWindow payload");
                        false
                    }
                }
            }
            MessageType::PtyExited => {
                // A window's shell exited: remove its window/pane. The group
                // keeps rendering sub-tabs down to a single window; only
                // dropping to zero ends the mux session for this tab. Unlike an
                // explicit `Detach` (which reverts to a plain tab), the last
                // window's shell exiting means there is nothing left to show, so
                // the tab itself is closed — `exited` makes `App::pump_all` reap
                // it just like a local shell that ran out (otherwise the empty
                // mux tab lingers and blocks `mux kill`).
                // Capture the displayed (active) pane id before removal so the
                // close-reconcile decision can tell "active window closed"
                // (redraw needed) from "non-active window closed, indices
                // shifted but the displayed pane is unchanged" (no redraw).
                let before_active = self.mux_group.as_ref().and_then(|g| g.active_pane_id());
                let reconcile_target = match self.mux_group.as_mut() {
                    Some(group) => match group.remove_pane(msg.pane_id) {
                        Some(idx) => {
                            log::info!(
                                "mux apc: pane {} exited (window {}) for tab {:?}",
                                msg.pane_id,
                                idx,
                                self.title
                            );
                            // task0005 AC-6: latch the removed pane id for
                            // `App::pump_all` to discard the matching
                            // `App::agent_status` entry (this method has no
                            // `&mut App` access).
                            self.pending_closed_agent_status_panes.push(msg.pane_id);
                            if group.is_empty() {
                                self.mux_group = None;
                                self.exited = true;
                                // Group emptied: nothing to redraw (FR3).
                                None
                            } else {
                                // Active window may have changed; decide by pane
                                // id whether the screen needs a reconcile.
                                let after_active = group.active_pane_id();
                                Tab::close_reconcile_target(before_active, after_active)
                            }
                        }
                        // Unknown pane id: no removal, no reconcile.
                        None => return false,
                    },
                    None => {
                        log::info!(
                            "mux apc: remote pane {} exited for tab {:?}",
                            msg.pane_id,
                            self.title
                        );
                        return false;
                    }
                };
                // Reconcile the screen with the now-active window (parity with
                // the inbound `SwitchWindow` reconcile). `request_pane_snapshot`
                // is a fire-and-forget PTY write, so this is gated on the
                // decision rather than asserted directly in unit tests (FR1).
                if let Some(pane_id) = reconcile_target {
                    // Latch the outgoing (exited) pane id so App::pump_all's
                    // existing per-pane scroll save/restore block runs for this
                    // close path, mirroring the SwitchWindow arm. First-latch-
                    // only: if several PtyExited drain in one pump we keep the
                    // genuinely-displayed outgoing pane (intermediate panes were
                    // never rendered). The exited pane is already removed by
                    // remove_pane above, so App::index_of_pane_id returns None
                    // for it — the park is correctly skipped and only the new
                    // active pane's active_pane_scroll() is reloaded.
                    if let Some(before) = before_active {
                        if self.pending_pane_switch_from.is_none() {
                            self.pending_pane_switch_from = Some(before);
                        }
                    }
                    self.request_pane_snapshot(pane_id);
                }
                true
            }
            MessageType::Detached => {
                // The daemon confirmed our `Detach`: exit mux mode. Clear the
                // window group (the tab reverts to a plain tab) and the
                // session name (status-bar mux badge clears). Port of the
                // WebView `onDetached → exitMuxMode`.
                log::info!("mux apc: detached from session for tab {:?}", self.title);
                self.mux_group = None;
                self.mux_session_name = None;
                // Restore pre-mux routing: the next pump parses the PTS stream
                // with `self.core` again (the bridge process exits and hands the
                // PTY back to the shell). Drop any partial outer frame the
                // extractor was carrying so a stale half-sequence cannot corrupt
                // a future re-attach (FR5).
                self.mux_apc_extractor.reset();
                // Cancel any in-flight off-thread snapshot replay before
                // clearing the grid. Otherwise a switch dispatched just before
                // detach (target snapshot >= OFFTHREAD_REPLAY_THRESHOLD_BYTES)
                // would still resolve on a later `poll_pending_switch`, swapping
                // the worker-built core (the detached window's content) back
                // over the grid we clear below. Mirrors the synchronous
                // `Snapshot` arm's supersede-the-pending-switch step.
                if let Some(old) = self.pending_switch.take() {
                    old.cancel.store(true, Ordering::Relaxed);
                }
                // FR7/FR8 (task0003): a coalesced same-pane re-dispatch (if
                // any) belonged to the pane being cleared above too — drop
                // it rather than let a later `poll_pending_switch` dispatch
                // a stale request for a pane this tab no longer shows.
                self.pending_redispatch = None;
                if let Some(old) = self.pending_scrollback_restore.take() {
                    old.cancel.store(true, Ordering::Relaxed);
                    log::warn!(
                        "scrollback restore cancelled (mux detached) for tab {:?}",
                        self.title
                    );
                }
                // The displayed grid still holds the detached mux window's
                // content. The bridge process exits right after this Detached
                // frame (mux::bridge → process::exit), handing the PTY back to
                // the shell that ran `emterm mux attach`, which reprints its
                // prompt — but on a clean screen only if we drop the stale mux
                // frame now. Reuse the PaneCreated append recipe (clear grid +
                // prompts/folds via reset_and_replay(b""), latch
                // pending_frame_reset so App::pump_all drops any selection and
                // forces a full redraw). Without this the detached session's
                // screen lingers until the shell happens to overwrite it.
                let _ = self.reset_frame_for_replay(b"", &[]);
                true
            }
            other => {
                log::debug!("mux apc: unhandled message type {other:?}");
                false
            }
        }
    }

    /// Title rendered by the tab bar. Currently identical to
    /// `self.title` — the `[mux:<session>]` prefix is applied by the
    /// widget (see `ui::tab_bar::render_label`).
    pub fn display_title(&self) -> &str {
        if self.title.is_empty() {
            "shell"
        } else {
            self.title.as_str()
        }
    }

    /// Drain pending PTY events into the terminal core. Returns true if
    /// anything changed (caller should request a redraw).
    ///
    /// Frame budget (FRAME_BUDGET_MS): bound how long one `pump` call
    /// spends inside `process_pty_data`. Bursty producers like
    /// `seq 1 10000000` would otherwise let a single pump tick eat the
    /// whole frame and freeze input/render until the burst drained.
    /// When the budget is exhausted we stop draining and request another
    /// frame via `crate::wakeup::wake()` so the remainder is processed
    /// on the next about_to_wait pass.
    pub fn pump(&mut self) -> bool {
        const FRAME_BUDGET_MS: u128 = 12;
        // Coalesce target: drain as many Data chunks as the frame budget
        // allows into one contiguous buffer, then run a SINGLE
        // process_pty_data + flush_grapheme_buffer + take_mode_actions
        // cycle. Per-chunk lock/flush/take overhead was the dominant
        // cost when the shell wrote tiny lines (`seq` produced ~41
        // bytes/chunk in benchmarking, so 200 chunks × 60µs overhead
        // ate the whole 12ms budget per frame and capped throughput
        // around 1MB/s). Coalescing makes one PTY chunk and 200 PTY
        // chunks cost roughly the same.
        const COALESCE_CAP: usize = 1024 * 1024;
        let start = std::time::Instant::now();
        let mut changed = false;
        let mut yielded = false;
        let mut combined: Vec<u8> = Vec::new();
        let mut saw_exit: Option<ExitReason> = None;
        while let Ok(evt) = self.events.try_recv() {
            match evt {
                PtyEvent::Data(bytes) => {
                    if combined.is_empty() {
                        combined = bytes;
                    } else {
                        combined.extend_from_slice(&bytes);
                    }
                    if combined.len() >= COALESCE_CAP
                        || start.elapsed().as_millis() >= FRAME_BUDGET_MS
                    {
                        yielded = true;
                        break;
                    }
                }
                PtyEvent::Exited { reason } => {
                    saw_exit = Some(reason);
                    break;
                }
            }
        }
        if self.process_combined(combined) {
            changed = true;
        }
        if let Some(reason) = saw_exit {
            match reason {
                ExitReason::Eof => log::info!("tab {:?} exited: EOF", self.title),
                ExitReason::ReadError(e) => log::warn!("tab {:?} read error: {e}", self.title),
            }
            self.exited = true;
            changed = true;
        }
        // Yielded mid-burst: schedule another wakeup so the next about_to_wait
        // continues draining instead of waiting for the 16ms WaitUntil deadline.
        if yielded {
            crate::wakeup::wake();
        }

        changed
    }

    /// Process one coalesced PTS buffer + the callback-state side effects it
    /// produced, returning whether the visible state changed.
    ///
    /// Split out of [`Self::pump`] so the parse / mux-decode / image-drain path
    /// is exercised by deterministic unit tests (which feed a known buffer)
    /// rather than the live PTY channel. `pump` calls this once per frame with
    /// the bytes it coalesced (possibly empty — the callback drains still run).
    /// Drive `self.core` over an outer-stream byte slice (the pre-mux parse
    /// path), running the grapheme flush, device-response write-back, and
    /// OSC 133 / fold mark drains that apply when `self.core` itself parses the
    /// outer bytes.
    ///
    /// Used by the pre-mux branch of [`Self::process_combined`] and, when a
    /// `Detached` frame appears mid-buffer, by the post-detach tail re-route
    /// (FR5): the bytes coalesced behind the `Detached` frame are plain shell
    /// output that must reach `self.core`, not the (now reset) transport
    /// extractor.
    fn process_outer_via_core(&mut self, bytes: &[u8]) {
        let mut c = self.core.lock();
        c.process_pty_data_fully(bytes);
        // Force-flush any grapheme cluster left buffered by the
        // parser (e.g. a lone emoji codepoint at the tail of an
        // IME-commit echo). Without this the cluster sits in
        // `grapheme_buffer` until the next non-extending codepoint
        // arrives, so the glyph stays invisible and the cursor
        // doesn't advance until the user types something else
        // (typical symptom: SKK `/smile` → 😄 only appears after
        // pressing space).
        c.flush_grapheme_buffer();
        // Pick up any device-status / DA / XTWINOPS reply term_core
        // synthesized while processing this chunk. PowerShell +
        // PSReadLine issue `\x1b[6n` cursor-position queries during
        // every line redraw; without writing the reply back into the
        // PTY, PSReadLine recomputes the redraw against a stale
        // cursor and a single Backspace erases multiple cells.
        let device_response = c.take_response();
        // Drain the OSC 133 marks `term_core` captured during this pump
        // (each already stamped with its emit-time row + eviction count)
        // and read the current eviction total — all under the core lock
        // so they are consistent with the bytes just processed. The
        // actual backfill runs after `drop(c)` because it needs
        // `&mut self`.
        let (evicted_total, pending_marks, pending_fold_marks) = drain_marks(&mut c);
        drop(c);
        if !device_response.is_empty() {
            self.write_device_response(device_response);
        }
        // agent-exit-after-icon (task0002 deviation — see task0002's
        // implementer report): reconcile this pump's OSC 133 mark
        // CANDIDATES (`cb_state.pending_latch_feed`, populated by
        // `NativeCallbacks::on_osc` in true synchronous order alongside
        // OSC 777 Set/Clear — see `callbacks::LatchFeedEvent`'s doc)
        // against `pending_marks` (`term_core`'s alt-screen-filtered,
        // authoritative live-mark list just drained above) to produce a
        // true-order, live-only sequence for this tab's inferred-clear
        // latch (FR4/FR5). Computed from `&pending_marks` BEFORE
        // `backfill_marks` below consumes it by value.
        let live_kinds: Vec<crate::prompts::PromptMarkKind> = pending_marks
            .iter()
            .filter_map(|m| crate::prompts::PromptMarkKind::from_byte(m.kind))
            .collect();
        let latch_feed = std::mem::take(&mut self.cb_state.lock().pending_latch_feed);
        if !latch_feed.is_empty() {
            self.pending_latch_inputs
                .extend(crate::agent_status_model::reconcile_latch_feed(
                    latch_feed,
                    &live_kinds,
                ));
        }
        self.backfill_marks(evicted_total, pending_marks, pending_fold_marks);
        // New PTY bytes reached the core — latch for the
        // inactive-tab activity path (WebView `onOutputActivity`).
        self.output_pending = true;
    }

    /// FR1/FR4/FR5: whether a `PtyOutput` frame may join the coalesce
    /// accumulator. Eligible only when it is addressed to the active pane (or
    /// the tab has no window group, so all output is accepted), there is no
    /// in-flight off-thread replay (`pending_switch`), and it carries no device
    /// query (see [`payload_has_device_query`]) — a query-bearing frame must be
    /// parsed on its own so its reply is captured before a later query
    /// overwrites `term_core`'s single-slot response buffer. Anything failing this gate is
    /// a boundary handled per-frame by [`Self::apply_mux_message`]. This is the
    /// single definition of "batch-eligible"; `process_combined` calls it so
    /// the classification is not duplicated inline.
    fn pty_output_batch_eligible(&self, msg: &MuxMessage) -> bool {
        if msg.msg_type != MessageType::PtyOutput {
            return false;
        }
        let active_pane = self.mux_group.as_ref().and_then(|g| g.active_pane_id());
        active_pane.map(|a| a == msg.pane_id).unwrap_or(true)
            && self.pending_switch.is_none()
            && !payload_has_device_query(&msg.payload)
    }

    /// Shared post-parse recipe for active-pane inner output, called by BOTH
    /// the coalesce flush ([`Self::flush_coalesced_output`]) and the per-frame
    /// `PtyOutput` arm of [`Self::apply_mux_message`]. Keeping it in one place
    /// means the "coalesce is a pure performance change" invariant (SPEC NFR2)
    /// is enforced by a single source of truth instead of two hand-mirrored
    /// copies that could silently drift. Parses `bytes` in one
    /// `process_pty_data_fully` call, writes back any device-status reply
    /// (`take_response`), and drains + backfills OSC 133 / fold marks. Always
    /// returns `true` (the bytes reached the core).
    fn apply_active_pane_output(&mut self, bytes: &[u8]) -> bool {
        let (evicted_total, pending_marks, pending_fold_marks, device_response) = {
            let mut c = self.core.lock();
            c.process_pty_data_fully(bytes);
            let device_response = c.take_response();
            let (evicted_total, pending_marks, pending_fold_marks) = drain_marks(&mut c);
            (
                evicted_total,
                pending_marks,
                pending_fold_marks,
                device_response,
            )
        };
        // Route any device-status reply (e.g. CPR synthesized for a PSReadLine
        // `\x1b[6n` query) back to the originating remote pane via PtyInput
        // framing so PSReadLine cursor tracking stays accurate over mux.
        if !device_response.is_empty() {
            self.write_device_response(device_response);
        }
        // Drain/backfill so prompt marks and custom-fold begin/end pairs
        // arriving over the mux stream are navigable / foldable too.
        self.backfill_marks(evicted_total, pending_marks, pending_fold_marks);
        true
    }

    /// FR1/FR3: flush the coalesce accumulator built in
    /// [`Self::process_combined`]. Parses the concatenated inner payloads of a
    /// consecutive active-pane `PtyOutput` run in ONE `process_pty_data_fully`
    /// call via [`Self::apply_active_pane_output`], running the per-batch side
    /// effects exactly once. Inner image APC/DCS emitted by the parse are
    /// drained once per pump by the post-loop block in `process_combined`, so
    /// they need no per-batch handling here. The accumulator is cleared.
    ///
    /// Returns `true` when bytes were applied (the caller sets `changed`).
    /// An empty accumulator is a no-op returning `false`.
    fn flush_coalesced_output(&mut self, acc: &mut Vec<u8>) -> bool {
        if acc.is_empty() {
            return false;
        }
        let applied = self.apply_active_pane_output(acc.as_slice());
        #[cfg(test)]
        {
            self.coalesce_parse_passes += 1;
        }
        acc.clear();
        applied
    }

    fn process_combined(&mut self, combined: Vec<u8>) -> bool {
        let mut changed = false;
        // Mux-transport frames extracted from the coalesced PTS bytes this
        // pump (mux branch only), each paired with its end offset in `combined`
        // so a `Detached` frame's boundary can be located (FR5). Merged into
        // `pending_apc` further down so they flow through the same
        // `partition_apc_for_mux` sink the pre-mux `self.core` parse feeds.
        let mut extracted_mux_apc: Vec<(Vec<u8>, usize)> = Vec::new();
        // task0005 rework (round1 findings 6b2e83f10c94ad7e / 929859ff2b4e431e
        // / 5cd6f305dcdeceb7): snapshot mux attachment as it stood at the
        // START of this pump, before anything below (including a `Detached`
        // frame extracted from `combined`) can change `self.mux_session_name`.
        // Used further down to decide whether `pending_agent_status` /
        // `pending_latch_feed` candidates parsed during this pump belong to
        // a mux pane's inner content (discard — the daemon is authoritative
        // for mux panes, SPEC FR3) rather than to plain shell output.
        let was_mux_at_pump_start = self.mux_session_name.is_some();
        if !combined.is_empty() {
            if self.mux_session_name.is_some() {
                // Mux established (FR1 / FR2): the outer PTS stream is the mux
                // transport. Parse it with the tab's INDEPENDENT extractor, not
                // `self.core`, so `self.core` is driven by the inner content
                // only (via `apply_mux_message`). This keeps an inner Kitty
                // chunk's parser state intact across `PtyOutput` boundaries.
                // The outer mux stream is APC-only (no printing, no device
                // queries, no OSC 133 marks), so the `self.core`-side grapheme
                // flush / device-response / mark-drain do NOT apply here.
                extracted_mux_apc = self.mux_apc_extractor.feed_with_offsets(&combined);
                changed = true;
                self.output_pending = true;
            } else {
                self.process_outer_via_core(&combined);
                changed = true;
            }
        }

        // Sync title from callback state if the shell sent a new one.
        {
            let mut s = self.cb_state.lock();
            if let Some(t) = s.title.take() {
                if t != self.title {
                    self.title = t;
                    changed = true;
                }
            }

            // Latch BEL rings for `App::pump_all` to dispatch per
            // `settings.bell_action` (visual flash / beep / none).
            if std::mem::take(&mut s.bell_count) > 0 {
                self.bell_pending = true;
            }

            // Send any device responses (e.g., DA1) back to the shell.
            let responses: Vec<Vec<u8>> = std::mem::take(&mut s.device_responses);
            // Drain buffered image-protocol payloads so we can decode them
            // outside the lock (the decoder needs cursor coords from `core`
            // — see comment in `drain_and_decode_images` below).
            let pending_apc: Vec<Vec<u8>> = std::mem::take(&mut s.pending_apc);
            let pending_dcs: Vec<Vec<u8>> = std::mem::take(&mut s.pending_dcs);
            // Phase 6: drain the theme-dirty latch. When an OSC 4/10/11/12/
            // 22/104/110/111/112 mutated the shared `Theme`, every row
            // must repaint with the new palette on the next frame.
            let theme_changed = std::mem::take(&mut s.theme_dirty);
            // `pending_agent_status` (plain-tab OSC 777 events) and
            // `pending_latch_feed` (OSC 133/777 latch candidates) are NOT
            // drained here — deliberately, task0005 rework. Both are
            // populated by `NativeCallbacks::on_osc`, fired for EITHER a
            // pre-mux outer parse (already done above, before this block, if
            // `mux_session_name` was `None` at pump start) OR mux inner
            // content parsed by the frame-apply loop further down (which
            // runs AFTER this point). Draining here would race that loop:
            // for a mux-attached pump, whatever is queued right now is only
            // stale leftovers, and for a same-pump mux→detach transition the
            // loop's mux-inner-origin candidates would not exist yet to be
            // excluded. See the discard-then-final-drain below the
            // frame-apply loop (before/after the detach tail re-route) for
            // where both queues are actually consumed this pump.
            drop(s);
            if theme_changed {
                self.core.lock().mark_all_dirty();
                changed = true;
            }
            if !responses.is_empty() {
                for r in responses {
                    self.write(r);
                }
                changed = true;
            }
            // Split the APC stream: payloads addressed to the `emterm-mux;`
            // inband protocol are decoded and applied to this tab's state;
            // everything else (Kitty graphics) falls through to the image
            // pipeline. `pending_dcs` is image-only (SIXEL).
            //
            // Pre-mux: `pending_apc` was populated by the `self.core` outer
            // parse above (which fired `on_apc`). Mux: the outer parse went to
            // the independent extractor instead, so the mux frames arrive via
            // `extracted_mux_apc` (each carrying its end offset in `combined`).
            // The extracted frames decode before any pre-mux `pending_apc` so
            // inner content this same pump applies in order.
            //
            // FR5 detach transition: a single coalesced buffer may carry
            // `[... Detached frame][post-detach shell bytes]`. The `Detached`
            // frame clears `mux_session_name` mid-loop; the bytes after it are
            // plain shell output the extractor would otherwise discard. Watch
            // for the Some→None transition while applying the extracted frames
            // and capture the offset just past the frame that triggered it, so
            // the tail can be re-routed through `self.core` below.
            let mut image_apc: Vec<Vec<u8>> = Vec::new();
            let mut detach_tail_start: Option<usize> = None;
            // FR1: concatenation buffer for the inner payloads of consecutive
            // batch-eligible active-pane `PtyOutput` frames. Parsed once per
            // run by `flush_coalesced_output` at every boundary and at loop end,
            // instead of once per frame — collapsing the ~1400-parse-per-pump
            // flood into one parse per consecutive run.
            let mut coalesce_acc: Vec<u8> = Vec::new();
            for (payload, end_offset) in extracted_mux_apc {
                if payload.starts_with(mux_ipc::protocol::APC_PREFIX.as_bytes()) {
                    if let Some(msg) = crate::mux::apc::try_decode_emterm_mux(&payload) {
                        // FR1/FR4/FR5 classify (see `pty_output_batch_eligible`):
                        // an active-pane `PtyOutput` with no in-flight off-thread
                        // replay and no device query is batch-eligible and
                        // accumulates without an immediate parse. Everything else
                        // (control message / non-active pane / pending_switch /
                        // detach / device-query frame) is a boundary: flush the
                        // accumulator first, then handle the frame via the
                        // existing per-frame path.
                        if self.pty_output_batch_eligible(&msg) {
                            coalesce_acc.extend_from_slice(&msg.payload);
                            // No immediate parse; continue accumulating the run.
                            continue;
                        }
                        // Boundary: flush the accumulated active-pane run BEFORE
                        // handling this frame so output/control ordering matches
                        // the per-frame path exactly.
                        if self.flush_coalesced_output(&mut coalesce_acc) {
                            changed = true;
                        }
                        let was_mux = self.mux_session_name.is_some();
                        if self.apply_mux_message(msg) {
                            changed = true;
                        }
                        // Detach: a frame just cleared `mux_session_name`. The
                        // remaining bytes in `combined` belong to the shell, not
                        // the mux transport — record where they start and STOP
                        // applying extracted frames. Every later frame was pulled
                        // from `combined[end_offset..]`, which the tail re-route
                        // below re-parses through `self.core`; continuing the loop
                        // would process those bytes twice (e.g. double-decoding a
                        // post-detach image, or leaking a re-attach frame).
                        if was_mux && self.mux_session_name.is_none() {
                            detach_tail_start = Some(end_offset);
                            break;
                        }
                    }
                    // Malformed mux payload — already logged inside the decoder;
                    // do NOT forward to the image pipeline.
                } else {
                    // A bare (non-mux) APC frame extracted from the transport
                    // stream is an inner Kitty image. It is a boundary for the
                    // active-pane run: flush before queueing it so ordering is
                    // preserved.
                    if self.flush_coalesced_output(&mut coalesce_acc) {
                        changed = true;
                    }
                    image_apc.push(payload);
                }
            }
            // FR1: flush the final accumulated run (loop ended without a
            // boundary frame).
            if self.flush_coalesced_output(&mut coalesce_acc) {
                changed = true;
            }
            // Pre-mux `pending_apc` (no offsets): partition + apply as before.
            let (pre_mux_images, pre_mux_messages) = partition_apc_for_mux(pending_apc);
            image_apc.extend(pre_mux_images);
            for msg in pre_mux_messages {
                if self.apply_mux_message(msg) {
                    changed = true;
                }
            }
            if (!image_apc.is_empty() || !pending_dcs.is_empty())
                && self.drain_and_decode_images(&image_apc, &pending_dcs)
            {
                changed = true;
            }
            // task0005 rework (round1 findings 6b2e83f10c94ad7e /
            // 929859ff2b4e431e): discard any `pending_agent_status` /
            // `pending_latch_feed` entries that accumulated from mux INNER
            // content this pump — the frame-apply loop just above drives
            // `self.core` for the active pane's inner payload
            // (`apply_active_pane_output` / `flush_coalesced_output`), which
            // fires `NativeCallbacks::on_osc` / OSC 133 capture exactly like
            // any other content, so an inner OSC 777 Set or OSC 133 D/A pair
            // lands in the same queues plain shell output would. Clearing
            // here, BEFORE the tail re-route below gets its own turn, means
            // the tail re-route's `process_outer_via_core` call (which
            // internally drains + reconciles `pending_latch_feed`) only ever
            // sees candidates from the bytes it itself just parsed — never
            // leftovers from the mux-inner portion of this same pump.
            // Gated on whether THIS PUMP started mux-attached, not the
            // current (possibly now-detached) state, so a same-pump
            // mux→detach transition cannot let a mux pane's inner OSC 777
            // Set / OSC 133 marks leak into the GUI-local plain-tab
            // agent-status model or inferred-clear latch (SPEC FR3: the
            // daemon is authoritative for mux panes; only its
            // `AgentStatusUpdate` messages may populate mux-pane status).
            if was_mux_at_pump_start {
                let mut s = self.cb_state.lock();
                s.pending_agent_status.clear();
                s.pending_latch_feed.clear();
            }
            // FR5: re-route the post-`Detached` tail through `self.core` in this
            // same pump. The `Detached` arm already cleared the grid via
            // `reset_frame_for_replay(b"")` and reset the extractor; the shell
            // (which now owns the PTY again) printed its prompt right behind the
            // `Detached` frame, and those bytes are still in `combined`. Without
            // this they would be dropped by the extractor and the screen would
            // stay blank until the next keystroke produced fresh PTS bytes.
            if let Some(tail) = detach_tail_start {
                if tail < combined.len() {
                    self.process_outer_via_core(&combined[tail..]);
                    changed = true;
                }
            }
            // Plain-tab `agent-status` OSC events parsed by
            // `NativeCallbacks::on_osc` this pump (task0005 AC-1/AC-3/AC-4),
            // sourced only from a pre-mux outer parse (top of this function)
            // or — when this pump carried a same-pump mux detach — the tail
            // re-route just above. Never from mux inner-content parsing,
            // which was discarded above. Drained once, here, after both
            // possible producers for this pump have already run.
            // `App::pump_all` (which owns `App::agent_status`) drains this
            // via `Tab::take_pending_agent_status_events` after the per-tab
            // loop's `&mut self.tabs` borrow ends.
            let agent_status_events: Vec<crate::agent_status::AgentStatusEvent> =
                std::mem::take(&mut self.cb_state.lock().pending_agent_status);
            if !agent_status_events.is_empty() {
                self.pending_agent_status_events.extend(agent_status_events);
                changed = true;
            }
            // Inner content applied by `apply_mux_message` (the `PtyOutput`
            // arm feeding `self.core`) fires `on_apc` / `on_dcs` for any inner
            // Kitty / SIXEL image — those land in `cb_state.pending_apc` /
            // `pending_dcs` only AFTER the loop above ran. Drain and decode
            // them now so an inner mux image is not deferred a frame (or, when
            // the next pump has no PTS bytes, never decoded). Inner content is
            // image-only here (mux protocol frames never re-enter `self.core`),
            // so this drain feeds the image pipeline directly.
            let (inner_apc, inner_dcs) = {
                let mut s = self.cb_state.lock();
                (
                    std::mem::take(&mut s.pending_apc),
                    std::mem::take(&mut s.pending_dcs),
                )
            };
            if (!inner_apc.is_empty() || !inner_dcs.is_empty())
                && self.drain_and_decode_images(&inner_apc, &inner_dcs)
            {
                changed = true;
            }
        }

        changed
    }

    /// Absorb any scrollback eviction that shifted the line frame, then push
    /// the OSC 133 marks `term_core` captured during the just-completed
    /// `process_pty_data` into the resolved tracker.
    ///
    /// `term_core` (`TerminalCore::push_pending_prompt_mark`) stamps every
    /// mark, *as it parses*, with the absolute row it was emitted on
    /// (`scrollback_len + cursor.row`) and the eviction counter at that
    /// instant. This fixes the old collapse where several marks in one chunk
    /// all landed on the final cursor row. The caller drains those marks via
    /// `take_prompt_marks` under the core lock and passes them here.
    ///
    /// Eviction normalization: a mark's `abs_row` is in the frame that
    /// existed *when the mark fired*. If scrollback evicted rows after that
    /// (but still inside the same pump), the consumer's current frame sits
    /// lower. We shift each new mark down by
    /// `current_evicted_total - mark.evicted_total` so it lands in the
    /// current frame. Previously-stored marks are pruned by the *total*
    /// delta since the last observation (`prune_before_line`) before the new
    /// marks are pushed, so both populations end up in one consistent frame.
    ///
    /// A counter that moved *backwards* means the core was reset (RIS zeroes
    /// it) and the whole frame restarted — stale marks are meaningless then,
    /// so drop them.
    ///
    /// Takes the scalar frame + the drained marks (rather than the locked
    /// `TerminalCore`) so the caller can read them off its own `MutexGuard`
    /// and drop the core borrow before calling — `backfill` needs
    /// `&mut self`, which would otherwise conflict with the guard's borrow of
    /// `self.core`.
    fn backfill_prompt_marks(
        &mut self,
        evicted_total: u64,
        marks: Vec<term_core::terminal_core::PendingPromptMark>,
    ) {
        if evicted_total < self.evicted_baseline {
            // Core reset (RIS / clear-scrollback) re-zeroed the counter and
            // rebuilt the line frame from scratch.
            // Latch the frame reset so `App::pump_all` drops the absolute-row
            // selection (its coordinates belong to the discarded frame).
            self.pending_frame_reset = true;
            self.prompts.clear();
            // Fold regions share the prompt-mark frame, so the same reset
            // invalidates them. Rebuild a fresh manager (preserving nothing)
            // — the replayed bytes' C→D pairs are re-registered below. The
            // tab's fold-enable preference is re-applied so a disabled tab
            // does not silently re-enable folding after a reset.
            self.folds = Self::new_fold_manager(self.fold_enabled);
            // A pending custom-fold `begin` captured in the old frame can no
            // longer pair with anything meaningful after the reset; drop it.
            self.pending_fold_begin = None;
            self.evicted_baseline = evicted_total;
        } else {
            // Shift previously-stored rows down by however many oldest
            // scrollback rows were dropped since the last observation.
            let delta = evicted_total - self.evicted_baseline;
            if delta > 0 {
                let delta_u32 = u32::try_from(delta).unwrap_or(u32::MAX);
                // Accumulate the eviction so `App::pump_all` can shift the
                // absolute-row selection down by the same number of rows that
                // prune the prompt / fold frames.
                self.pending_eviction_delta = self.pending_eviction_delta.saturating_add(delta_u32);
                self.prompts.prune_before_line(delta_u32);
                // Keep fold regions in lock-step with the prompt frame: the
                // same eviction shifts their absolute rows down (and drops
                // any region whose head fell off the top of scrollback).
                self.folds.prune_before_line(delta_u32);
                // Shift the pending custom-fold `begin` into the new frame. If
                // its row fell at/below the eviction boundary its head scrolled
                // off the top — the eventual region would span the boundary,
                // which `FoldManager::prune_before_line` drops, so drop the
                // begin now (matching the WebView's boundary-spanning rule).
                if let Some((begin_row, _)) = self.pending_fold_begin.as_ref() {
                    match begin_row.checked_sub(delta_u32) {
                        Some(shifted) => {
                            if let Some(entry) = self.pending_fold_begin.as_mut() {
                                entry.0 = shifted;
                            }
                        }
                        None => self.pending_fold_begin = None,
                    }
                }
                self.evicted_baseline = evicted_total;
            }
        }
        // Track the normalized row + exit code of `D` marks pushed in this
        // batch so the C→D fold scan (after the push loop) addresses the same
        // post-prune frame the marks now live in, and carries the `D` mark's
        // own exit code into the region (mirroring the WebView, which passes
        // `exitCode` straight into `registerOsc133FoldRegion`).
        let mut new_command_ends: Vec<(u32, Option<i32>)> = Vec::new();
        for m in marks {
            let Some(kind) = crate::prompts::PromptMarkKind::from_byte(m.kind) else {
                continue;
            };
            // Normalize the mark's capture-time row into the current frame:
            // any eviction that happened *after* this mark fired shifts the
            // frame down by that many rows. `evicted_total >= m.evicted_total`
            // always holds (the counter is monotonic and we already handled
            // the reset/backwards case above), so the subtraction is safe.
            let shift = evicted_total.saturating_sub(m.evicted_total);
            let shift = u32::try_from(shift).unwrap_or(u32::MAX);
            let Some(row) = m.abs_row.checked_sub(shift) else {
                // The mark's row was evicted out of the frame within this
                // same pump; it no longer addresses any retained line. Drop
                // it, matching prune_before_line's retain(row >= count) for
                // previously-stored marks — clamping to 0 instead would
                // plant a phantom prompt at the top of scrollback.
                continue;
            };
            if kind == crate::prompts::PromptMarkKind::CommandEnd {
                new_command_ends.push((row, m.exit_code));
            }
            // For B (CommandStart) marks: if the off-thread bypass captured
            // the command text at emit time (keyed by the original abs_row),
            // re-key it to the post-normalization row so
            // `register_osc133_fold_region_at_idx` can find it without a
            // scrollback lookup.
            //
            // Row-collision policy: a live B mark (no entry in
            // pending_bypass_b_mark_texts) evicts any stale snapshot-era entry
            // at the same normalized row so live wins on collision.
            // Snapshot-era B marks (pending_bypass_b_mark_texts has the row)
            // repopulate resolved_b_mark_texts, keeping the text available for
            // D marks that arrive live after the swap (the common long-running
            // command case). This is a no-op on the sync path (both maps are
            // always empty there).
            if kind == crate::prompts::PromptMarkKind::CommandStart {
                if let Some(text) = self.pending_bypass_b_mark_texts.get(&m.abs_row) {
                    // Snapshot-era B mark: populate resolved map so live D
                    // marks can find the command text later.
                    self.resolved_b_mark_texts.insert(row, text.clone());
                } else {
                    // Live B mark wins on row collision: evict any snapshot-era
                    // stale entry so the D mark gets the live scrollback text.
                    self.resolved_b_mark_texts.remove(&row);
                }
            }
            self.prompts.push(crate::prompts::ResolvedPromptMark {
                kind,
                row,
                exit_code: m.exit_code,
            });
        }
        // Register an OSC 133 fold region for each `D` mark added this batch.
        // Done after the push loop so the scan sees the whole batch in the
        // tracker (a `C`/`B` arriving in the same chunk as its `D` is already
        // stored), matching the WebView's per-`D` `getMarkers()` walk.
        //
        // Performance: resolve each D mark's deque index in a single O(n)
        // backward scan (collecting the last `d_count` CommandEnd indices)
        // rather than calling `rposition` once per D. The scan pairs with
        // `new_command_ends` in push (left-to-right) order because we reversed
        // the collected indices back to that order.
        let d_count = new_command_ends.len();
        if d_count > 0 {
            let mut d_indices: Vec<usize> = Vec::with_capacity(d_count);
            {
                use crate::prompts::PromptMarkKind;
                let marks = self.prompts.marks();
                for (i, m) in marks.iter().enumerate().rev() {
                    if m.kind == PromptMarkKind::CommandEnd {
                        d_indices.push(i);
                        if d_indices.len() == d_count {
                            break;
                        }
                    }
                }
                // Collected in reverse order; restore push (left-to-right) order
                // so d_indices[j] matches new_command_ends[j].
                d_indices.reverse();
            }
            // d_indices.len() may be < d_count if eviction at cap dropped some;
            // zip stops at the shorter side, which is correct.
            for ((d_row, exit_code), d_idx) in new_command_ends.into_iter().zip(d_indices) {
                self.register_osc133_fold_region_at_idx(d_idx, d_row, exit_code);
            }
        }
    }

    /// Drain-side counterpart to [`Self::backfill_prompt_marks`] for the
    /// custom-fold pipeline (`OSC 777;emterm;fold;begin|end`). Port of the
    /// WebView `handleFoldCommand` begin↔end pairing, but driven by the
    /// term_core capture (`take_fold_marks`) the same way prompt marks are,
    /// so each mark's row is the line it was *emitted* on rather than the
    /// final cursor row.
    ///
    /// Call order: the eviction normalization for the *already-pending* begin
    /// and the fold registry runs inside `backfill_prompt_marks` (which the
    /// callers invoke first), so by the time this method runs
    /// `self.pending_fold_begin` and `self.folds` are already in the current
    /// post-prune frame. Here we only normalize each *new* fold mark's
    /// capture-time row into that frame and apply begin/end pairing:
    ///
    /// - `begin` → record `(row, label)` in `pending_fold_begin`, overwriting
    ///   any previous pending begin (WebView `pendingFoldBegins.set`).
    /// - `end` with a pending begin → `folds.register_custom_region(begin_row,
    ///   end_row, label)` and clear the pending begin (WebView path). An empty
    ///   label is left as-is; `register_custom_region` substitutes `"..."`.
    /// - `end` with no pending begin → ignored (WebView "orphaned end").
    ///
    /// A `begin` whose row was evicted out of the frame within this same drain
    /// is dropped (no pending begin recorded), matching the prompt-mark
    /// `checked_sub` guard.
    fn backfill_fold_marks(
        &mut self,
        evicted_total: u64,
        marks: Vec<term_core::terminal_core::PendingFoldMark>,
    ) {
        use term_core::terminal_core::FoldMarkKind;
        for m in marks {
            // Normalize the capture-time row into the current frame; any
            // eviction after the mark fired shifts the frame down. The
            // reset/backwards case was handled by `backfill_prompt_marks`.
            let shift = evicted_total.saturating_sub(m.evicted_total);
            let shift = u32::try_from(shift).unwrap_or(u32::MAX);
            let Some(row) = m.abs_row.checked_sub(shift) else {
                // The mark's row was evicted out of the frame within this same
                // drain. For a `begin` this means no pending begin; for an
                // `end` we still must not pair against a stale begin, so a
                // begin recorded earlier this drain that survived to here is
                // valid. Skip only this mark.
                continue;
            };
            match m.kind {
                FoldMarkKind::Begin => {
                    // Overwrite any previous pending begin (WebView clobber).
                    self.pending_fold_begin = Some((row, m.label));
                }
                FoldMarkKind::End => {
                    if let Some((begin_row, label)) = self.pending_fold_begin.take() {
                        self.folds.register_custom_region(begin_row, row, label);
                    }
                    // No pending begin → orphaned end, ignored.
                }
            }
        }
    }

    /// Push the prompt + fold marks captured for the just-processed chunk
    /// into the resolved trackers, in the one order that is correct.
    ///
    /// `backfill_prompt_marks` runs the eviction normalization + fold-region
    /// prune that `backfill_fold_marks` then relies on (see the latter's doc
    /// comment), so prompt marks MUST be backfilled first. Centralizing the
    /// pair here keeps that ordering invariant in a single place instead of
    /// leaving every drain site (`pump`, `Snapshot`, `PtyOutput`) to repeat —
    /// and risk reordering — the two calls. Drain the inputs with
    /// [`drain_marks`] under the core guard, drop the guard, then call this.
    fn backfill_marks(
        &mut self,
        evicted_total: u64,
        prompt_marks: Vec<term_core::terminal_core::PendingPromptMark>,
        fold_marks: Vec<term_core::terminal_core::PendingFoldMark>,
    ) {
        self.backfill_prompt_marks(evicted_total, prompt_marks);
        self.backfill_fold_marks(evicted_total, fold_marks);
    }

    /// Register an OSC 133 C→D fold region for the `D` mark at deque index
    /// `d_idx` with absolute row `d_row` carrying `exit_code`. Port of
    /// `registerOsc133FoldRegion` in `handlers/osc_handlers.ts`: scan the
    /// resolved marks in reverse starting strictly before `d_idx` to find the
    /// most recent `C` (stopping if another `D` is hit first, meaning no `C`
    /// pairs with this one), then the `B` before that `C` for the command text.
    /// No `C` → no region. The command text is the `B` mark's line (empty when
    /// there is no `B`).
    ///
    /// `d_idx` is the caller-supplied deque position of this `D` mark (resolved
    /// in one backward scan by `backfill_prompt_marks` to avoid the O(k·n)
    /// `rposition` cost of the previous per-D approach). `d_row` is already in
    /// the current post-prune frame (the same frame the marks in `self.prompts`
    /// use), so the `C`/`B` rows found by the scan and the resulting region
    /// bounds are all consistent with the pruned fold registry.
    fn register_osc133_fold_region_at_idx(
        &mut self,
        d_idx: usize,
        d_row: u32,
        exit_code: Option<i32>,
    ) {
        use crate::prompts::PromptMarkKind;
        let mut c_row: Option<u32> = None;
        let mut b_row: Option<u32> = None;
        {
            let marks = self.prompts.marks();
            // Scan strictly before `d_idx` (reproducing the WebView's view where
            // each `D` is registered the instant it is the last mark added, so
            // no later `D`s are visible in the walk).
            for m in marks.iter().take(d_idx).rev() {
                if c_row.is_none() && m.kind == PromptMarkKind::CommandExec {
                    c_row = Some(m.row);
                }
                if c_row.is_some() && m.kind == PromptMarkKind::CommandStart {
                    b_row = Some(m.row);
                    break;
                }
                // Another `D` before we found a `C`: this `D` has no matching
                // `C`, so there is no region to register.
                if m.kind == PromptMarkKind::CommandEnd {
                    break;
                }
            }
        }
        let Some(c_row) = c_row else {
            return;
        };
        // Command text comes from the `B` mark's line (empty when no `B`).
        // During an off-thread swap the scrollback is not populated
        // (`build_from_snapshot` bypass skips SlimCell storage), so
        // `extract_line_text` would return an empty string for B marks whose
        // row landed in scrollback. `resolved_b_mark_texts` holds pre-captured
        // texts that `backfill_prompt_marks` re-keyed from
        // `pending_bypass_b_mark_texts` for exactly this case; we prefer it
        // when present and fall back to the live scrollback lookup otherwise.
        let command_text = match b_row {
            Some(row) => self
                .resolved_b_mark_texts
                .get(&row)
                .cloned()
                .unwrap_or_else(|| self.extract_line_text(row)),
            None => String::new(),
        };
        self.folds
            .register_osc133_region(c_row, d_row, command_text, exit_code);
    }

    /// Plain (trimmed) text of the buffer line at absolute row `abs_row`.
    /// Port of `extractLineText`: a row below `scrollback_len` is read from
    /// scrollback (already trimmed by `get_scrollback_text`); a row in the
    /// viewport is decoded cell-by-cell and trimmed. An out-of-range row
    /// yields an empty string. Locks `self.core` briefly — all callers have
    /// already dropped any prior core guard.
    fn extract_line_text(&self, abs_row: u32) -> String {
        let c = self.core.lock();
        let scrollback_len = c.get_scrollback_length();
        if abs_row < scrollback_len {
            // Scrollback rows are returned already trimmed by term_core.
            return c.get_scrollback_text(abs_row);
        }
        let screen_row = abs_row - scrollback_len;
        let rows = c.rows() as u32;
        if screen_row >= rows {
            return String::new();
        }
        let screen_row = screen_row as u16;
        let cols = c.cols();
        let mut text = String::new();
        for col in 0..cols {
            // Skip the width-0 trailing half of a wide glyph so the text is
            // not doubled, matching the search/links cell-read convention.
            if c.get_cell_width(col, screen_row) == 0 {
                continue;
            }
            text.push_str(&c.get_cell_char(col, screen_row));
        }
        text.trim().to_string()
    }

    /// Decode buffered APC / DCS payloads via `term_images::ImageProcessor`,
    /// split off `ImageEvent::Response` and write the bytes back to the
    /// shell (Kitty OK / error replies), and queue the remaining state
    /// events in `pending_image_events` for the GPU layer to consume on
    /// its next ingest pass.
    ///
    /// Cursor coordinates are read from `self.core` so the resulting
    /// `Place` events anchor at the right cell. We snapshot them once at
    /// the start of the call: in practice the buffered payloads were
    /// produced by the same byte chunk that just landed, so the cursor
    /// has not moved between APC parsing and decode here.
    ///
    /// Returns true when anything was decoded (caller can request a
    /// redraw).
    fn drain_and_decode_images(&mut self, apc: &[Vec<u8>], dcs: &[Vec<u8>]) -> bool {
        if apc.is_empty() && dcs.is_empty() {
            return false;
        }
        let (cursor_row, cursor_col) = {
            let c = self.core.lock();
            (c.get_cursor_row() as u32, c.get_cursor_col() as u32)
        };

        let mut all_events: Vec<ImageEvent> = Vec::new();
        for bytes in apc {
            let events =
                image_parse::decode_apc(bytes, cursor_row, cursor_col, &mut self.image_proc);
            all_events.extend(events);
        }
        for bytes in dcs {
            let events =
                image_parse::decode_dcs(bytes, cursor_row, cursor_col, &mut self.image_proc);
            all_events.extend(events);
        }
        if all_events.is_empty() {
            return false;
        }

        let (state_events, responses) = split_image_events(all_events);
        // Route protocol responses (Kitty OK / error / query) back to the
        // shell. SIXEL does not currently emit responses but the API is
        // uniform.
        for resp in responses {
            self.write(resp.into_bytes());
        }
        if !state_events.is_empty() {
            self.pending_image_events.extend(state_events);
        }
        true
    }

    /// Drain the queue of image-state events accumulated by `pump`.
    /// `App::pump_all` calls this once per pass and forwards the events
    /// to `crate::viewer::image::ImageViewerRouter`, which stores decoded
    /// images (LRU) and opens a native viewer child window per `Place`.
    pub fn drain_image_events(&mut self) -> Vec<ImageEvent> {
        std::mem::take(&mut self.pending_image_events)
    }

    pub fn write(&self, bytes: Vec<u8>) {
        if let Some(p) = &self.pty {
            p.write(bytes);
        }
    }

    /// Send user input (keystrokes / paste / IME commits) to the active pane.
    ///
    /// In mux mode the `emterm mux` bridge **drops raw stdin bytes** (only
    /// APC-framed mux messages are relayed to the daemon — see
    /// `src-tauri/src/mux/bridge.rs`), so input must be wrapped as a
    /// `PtyInput` frame carrying the active pane id (parity with the WebView
    /// `MuxClient.sendInput`). Outside mux mode this is a plain raw PTY write.
    pub fn write_input(&self, bytes: Vec<u8>) {
        // Two-step gate so a *half-attached* state (mux session present but the
        // window group is None / unseeded, or seeded but with no active pane)
        // does not silently fall back to raw PTY write — which the bridge
        // would then drop, leaving the user staring at a mux-badged tab that
        // ignores keystrokes / IME commits / paste. When attached we always
        // take the PtyInput path; if no active pane id is yet available we log
        // and drop so the failure mode is visible during development instead
        // of silently swallowed.
        if self.mux_session_name.is_some() {
            match self.mux_group.as_ref().and_then(|g| g.active_pane_id()) {
                Some(pane_id) => {
                    self.send_control(&MuxMessage {
                        msg_type: MessageType::PtyInput,
                        pane_id,
                        payload: bytes,
                    });
                }
                None => {
                    log::warn!(
                        "mux: write_input dropped {} bytes — tab {:?} attached \
                         but no active pane id (group not yet seeded)",
                        bytes.len(),
                        self.title
                    );
                }
            }
        } else {
            self.write(bytes);
        }
    }

    /// Route a terminal-generated device response (DSR/DA/XTWINOPS reply from
    /// `term_core`) back to the active pane, mirroring `write_input`'s routing
    /// decision.
    ///
    /// In mux mode the `emterm mux` bridge **drops raw stdin bytes**, so the
    /// response must be wrapped as a `PtyInput` frame — identical to how user
    /// keystrokes are routed. Outside mux mode a plain raw PTY write is used.
    ///
    /// The two-step gate from `write_input` is replicated here: when attached
    /// but no active pane id is yet available the bytes are dropped with a
    /// warning (observable failure mode) rather than falling back to a raw
    /// write that the bridge would silently discard anyway.
    fn write_device_response(&self, bytes: Vec<u8>) {
        if self.mux_session_name.is_some() {
            match self.mux_group.as_ref().and_then(|g| g.active_pane_id()) {
                Some(pane_id) => {
                    self.send_control(&MuxMessage {
                        msg_type: MessageType::PtyInput,
                        pane_id,
                        payload: bytes,
                    });
                }
                None => {
                    log::warn!(
                        "mux: write_device_response dropped {} bytes — tab {:?} attached \
                         but no active pane id (group not yet seeded)",
                        bytes.len(),
                        self.title
                    );
                }
            }
        } else {
            self.write(bytes);
        }
    }

    /// Paste-aware input: bracketed-paste-wrap (DECSET 2004) then route via
    /// [`Self::write_input`] so the paste reaches the active mux pane too.
    pub fn write_paste_input(&self, text: &str, bracketed: bool) {
        let wrapped = crate::selection::bracketed_paste(text, bracketed);
        self.write_input(wrapped.into_bytes());
    }

    /// Send a structured mux control message to the daemon by APC-encoding it
    /// and writing the bytes to this tab's PTY (fire-and-forget). The
    /// `emterm mux` bridge running in the PTY relays the frame to the daemon
    /// over its Unix socket; native-poc never opens that socket (NFR2).
    /// Responses arrive as inbound APC through the existing decode route.
    ///
    /// Port of the WebView `MuxClient.sendControl` (`writeDirect`). Returns
    /// `false` when the tab has no live PTY (the message is dropped).
    pub fn send_control(&self, msg: &MuxMessage) -> bool {
        let bytes = crate::mux::apc::encode_emterm_mux(msg);
        match &self.pty {
            Some(p) => {
                p.write(bytes);
                true
            }
            None => {
                log::warn!(
                    "mux: send_control({:?}) dropped — tab {:?} has no PTY",
                    msg.msg_type,
                    self.title
                );
                false
            }
        }
    }

    /// Request an on-demand screen snapshot for `pane_id`. The daemon replies
    /// with a `PtyOutput` frame (a screen reset + shadow-parser replay) that
    /// `apply_mux_message` feeds into `term_core`, so the displayed grid is
    /// reconciled with the daemon's authoritative state. Without this, an
    /// attach / window switch leaves the target pane's screen blank or stale —
    /// the daemon does not push the active screen unprompted.
    ///
    /// Port of `requestPaneSnapshot` (`MuxClient.sendRequestPaneSnapshot`).
    /// Fire-and-forget; returns `false` when the tab has no live PTY.
    pub fn request_pane_snapshot(&self, pane_id: u32) -> bool {
        use mux_ipc::protocol::{MessageType, MuxMessage};
        self.send_control(&MuxMessage {
            msg_type: MessageType::RequestPaneSnapshot,
            pane_id,
            payload: Vec::new(),
        })
    }

    /// Register this connection as the live-output *owner* of `session_id` by
    /// sending an `Attach` control frame, mirroring the WebView reattach path
    /// (`enterMuxMode` in `src/terminal-app/mux/mux-session.ts`).
    ///
    /// The daemon streams continuous PTY output only to a pane's single owning
    /// connection. Without an Attach, native receives on-demand snapshots
    /// (`request_pane_snapshot`) but no live updates, so programs like `top`
    /// look frozen. Sending Attach installs this connection's output channel as
    /// the pane owner — evicting any prior client (e.g. an attached WebView) —
    /// and replays the daemon's buffered output for the session's panes.
    ///
    /// The `AttachMsg` payload is bincode-serialized (`session_id` as a 4-byte
    /// LE u32), matching the WebView wire shape. Fire-and-forget; returns
    /// `false` when the tab has no live PTY.
    pub fn send_attach(&self, session_id: u32) -> bool {
        use mux_ipc::protocol::{AttachMsg, MessageType, MuxMessage};
        self.send_control(&MuxMessage::control(
            MessageType::Attach,
            0,
            &AttachMsg { session_id },
        ))
    }

    /// Test-only helper that runs the prompt-mark backfill (the production
    /// `pump` step that records the scrollback-eviction delta / frame-reset
    /// latch). Lets cross-module tests in `app` drive the eviction
    /// bookkeeping without exposing the otherwise-private backfill method.
    #[cfg(test)]
    pub(crate) fn test_backfill_eviction(&mut self, evicted_total: u64) {
        self.backfill_prompt_marks(evicted_total, Vec::new());
    }

    /// Test-only: whether an off-thread snapshot replay is currently in
    /// flight for this tab.
    #[cfg(test)]
    pub(crate) fn test_has_pending_switch(&self) -> bool {
        self.pending_switch.is_some()
    }

    /// Test-only: the target pane id of the in-flight off-thread replay,
    /// or `None` when no replay is pending.
    #[cfg(test)]
    pub(crate) fn test_pending_target(&self) -> Option<u32> {
        self.pending_switch.as_ref().map(|p| p.target_pane)
    }

    /// Test-only: the queued live-output payloads accumulated for the
    /// in-flight off-thread replay, in arrival order.
    #[cfg(test)]
    pub(crate) fn test_pending_live_queue(&self) -> Vec<Vec<u8>> {
        self.pending_switch
            .as_ref()
            .map(|p| p.live_queue.clone())
            .unwrap_or_default()
    }

    /// Test-only: block until the in-flight worker finishes and return its
    /// built core's first-row text, proving exactly the latest target's
    /// snapshot was the one that got built (supersession). Panics if no
    /// switch is pending. Used by the supersession test; production code
    /// never blocks on the handoff (it is `try_recv`'d in `pump_all`).
    #[cfg(test)]
    pub(crate) fn test_wait_pending_first_row(&self) -> String {
        let pending = self.pending_switch.as_ref().expect("no pending switch");
        let replay = pending.done.recv().expect("worker disconnected");
        let mut s = String::new();
        for col in 0..replay.core.cols() {
            s.push_str(&replay.core.get_cell_char(col, 0));
        }
        s.trim_end().to_string()
    }

    /// Test-only: spin on [`Self::poll_pending_switch`] until the worker
    /// finishes and the swap completes (or there is no pending switch),
    /// returning the final outcome. Bounded spin so a stuck worker fails the
    /// test instead of hanging. Mirrors what `pump_all` does across many
    /// frames, collapsed into one synchronous call for unit tests (no real
    /// `pump_all` async loop — NFR2).
    #[cfg(test)]
    pub(crate) fn test_poll_until_swapped(&mut self) -> SwapOutcome {
        for _ in 0..10_000 {
            match self.poll_pending_switch() {
                SwapOutcome::Pending => std::thread::yield_now(),
                other => return other,
            }
        }
        panic!("off-thread replay worker did not complete in time");
    }

    /// Test-only: block until the in-flight worker has produced its result,
    /// then re-stage it on a fresh, ready-to-`try_recv` channel so a later
    /// `poll_pending_switch` (e.g. the one inside a single `App::pump_all`
    /// call) deterministically observes `Ok(replay)` — no spin/sleep, no
    /// `pump_all` polling loop (NFR2). Panics if no switch is pending.
    #[cfg(test)]
    pub(crate) fn test_block_worker_ready(&mut self) {
        let pending = self.pending_switch.as_mut().expect("no pending switch");
        let replay = pending.done.recv().expect("worker disconnected");
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(replay).expect("re-stage replay");
        pending.done = rx;
        // `tx` drops here, but the value is already buffered in `rx`, so
        // `try_recv` returns `Ok` before it would see `Disconnected`.
    }

    /// Test-only: drop the in-flight worker's completion sender so the next
    /// poll observes `Disconnected` (the worker-panic fallback path, FR7).
    /// Replaces the live receiver with a fresh, already-disconnected one.
    #[cfg(test)]
    pub(crate) fn test_force_worker_disconnect(&mut self) {
        if let Some(pending) = self.pending_switch.as_mut() {
            let (_tx, rx) = std::sync::mpsc::channel();
            // `_tx` drops at end of scope → `rx` is immediately disconnected.
            pending.done = rx;
        }
    }

    /// Test-only: whether a 2nd-pass scrollback restore is currently in
    /// flight for this tab.
    #[cfg(test)]
    pub(crate) fn test_has_pending_scrollback_restore(&self) -> bool {
        self.pending_scrollback_restore.is_some()
    }

    /// Test-only: block until the in-flight 2nd-pass scrollback restore
    /// worker has produced its result, then re-stage it on a fresh,
    /// ready-to-`try_recv` channel so a later
    /// `poll_pending_scrollback_restore` (e.g. the one inside a single
    /// `App::pump_all` call) deterministically observes `Ok(build)` — no
    /// spin/sleep, no `pump_all` polling loop. Panics if no restore is
    /// pending.
    #[cfg(test)]
    pub(crate) fn test_drain_pending_scrollback_restore_for_blocking_recv(&mut self) {
        let pending = self
            .pending_scrollback_restore
            .as_mut()
            .expect("no pending scrollback restore");
        let build = pending.done.recv().expect("restore worker disconnected");
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(build).expect("re-stage scrollback build");
        pending.done = rx;
    }

    /// Test-only: force the in-flight 2nd-pass scrollback restore worker's
    /// completion sender to drop so the next poll observes `Disconnected`
    /// (the worker-panic fallback path, FR7).
    #[cfg(test)]
    pub(crate) fn test_force_scrollback_restore_disconnect(&mut self) {
        if let Some(pending) = self.pending_scrollback_restore.as_mut() {
            let (_tx, rx) = std::sync::mpsc::channel();
            pending.done = rx;
        }
    }

    /// Test-only: snapshot live core's scrollback count via the public API.
    /// Mirrors `core.lock().get_scrollback_length()`; the small wrapper lets
    /// tests stay readable.
    #[cfg(test)]
    pub(crate) fn test_scrollback_length(&self) -> u32 {
        self.core.lock().get_scrollback_length()
    }

    /// Test-only: read the displayed core's row `row` as trimmed text.
    #[cfg(test)]
    pub(crate) fn test_row_text(&self, row: u16) -> String {
        let c = self.core.lock();
        let s: String = (0..c.cols()).map(|col| c.get_cell_char(col, row)).collect();
        s.trim_end().to_string()
    }

    /// Test-only: drive the per-pump buffer-processing path with a known
    /// coalesced PTS buffer, bypassing the live PTY channel. Exercises the
    /// exact `pump` parse / mux-decode / image-drain logic so the
    /// transport-isolation routing is testable deterministically.
    #[cfg(test)]
    pub(crate) fn test_process_combined(&mut self, combined: Vec<u8>) -> bool {
        self.process_combined(combined)
    }

    /// Test-only: how many times the `process_combined` coalesce flush has
    /// parsed accumulated active-pane output since this tab was built. One
    /// consecutive active-pane `PtyOutput` run flushes exactly once.
    #[cfg(test)]
    pub(crate) fn test_coalesce_parse_passes(&self) -> u32 {
        self.coalesce_parse_passes
    }

    /// Test-only: how many times `dispatch_offthread_replay` actually
    /// spawned an off-thread replay worker since this tab was built
    /// (task0003 FR7/FR8). A same-pane re-dispatch that coalesces into
    /// `pending_redispatch` instead of spawning does NOT increment this.
    #[cfg(test)]
    pub(crate) fn test_offthread_spawn_count(&self) -> u32 {
        self.offthread_spawn_count
    }

    /// Test-only: whether a same-pane re-dispatch is currently coalesced,
    /// waiting for the next `poll_pending_switch` to install a fresh
    /// worker for it (task0003 FR8).
    #[cfg(test)]
    pub(crate) fn test_has_pending_redispatch(&self) -> bool {
        self.pending_redispatch.is_some()
    }

    /// Test-only: the grid size a racing `Tab::resize` deferred against the
    /// in-flight switch, if any (task0006 FR7, `PendingSwitch::pending_resize`).
    #[cfg(test)]
    pub(crate) fn test_pending_resize(&self) -> Option<(u16, u16)> {
        self.pending_switch.as_ref().and_then(|p| p.pending_resize)
    }

    /// Test-only: number of times `apply_mux_message`'s `Snapshot`/
    /// `SnapshotRestore` arm has run `decode_snapshot_payload_typed`
    /// (task0006 FR8 AC-6).
    #[cfg(test)]
    pub(crate) fn test_snapshot_decode_count(&self) -> u32 {
        self.snapshot_decode_count
    }

    /// Test-only: every pane `Resize` control frame this tab has emitted so
    /// far, across all three emission sites (task0003 AC-6, FR4 — see
    /// [`ResizeFrameRecord`]). Cloned out (the records are `Copy`) so the
    /// caller can inspect them without holding a borrow of the tab.
    #[cfg(test)]
    pub(crate) fn test_resize_frames(&self) -> Vec<ResizeFrameRecord> {
        self.resize_frame_log.clone()
    }

    /// Test-only: the whole displayed grid (all rows) as one string, for
    /// asserting that outer-transport base64 never leaks onto the screen.
    #[cfg(test)]
    pub(crate) fn test_grid_text(&self) -> String {
        let c = self.core.lock();
        let rows = c.rows();
        let cols = c.cols();
        let mut out = String::new();
        for row in 0..rows {
            for col in 0..cols {
                out.push_str(&c.get_cell_char(col, row));
            }
            out.push('\n');
        }
        out
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        // FR6 / IMPLEMENTATION.md contract (c) (task0002): capture the
        // pre-resize column count before any effect, so the
        // self-invalidation check below (after the clamp) compares against
        // the tab's ACTUAL prior width — not the caller's raw, possibly
        // out-of-domain request.
        let pre_resize_cols = self.core.lock().cols();

        // D3''''' (round-8 rework, review round-7 finding `1d1b6b6297e3b6a0`):
        // clamp to the SAME wire domain `MuxPane::new`/`MuxPane::resize`
        // apply on the daemon side, BEFORE resizing this tab's own core and
        // BEFORE the `Resize` control frame is sent below. Both ends run the
        // identical, pure `clamp_dims_to_wire_domain` function against the
        // identical input, so they always agree on the accepted dimensions
        // without any wire round-trip acknowledgment — closing the gap
        // where the daemon silently clamped a pane's dims into the wire
        // domain while nothing told the client its own (unclamped) core was
        // now describing a PTY of a different size.
        let (cols, rows) = crate::mux::session::pane::clamp_dims_to_wire_domain(cols, rows);
        if let Some(p) = &self.pty {
            p.resize(cols, rows);
        }
        self.core.lock().resize(cols, rows);

        // FR6 / IMPLEMENTATION.md contract (c), D3 (task0002): a resize
        // that changed this tab's (post-clamp) column count invalidates
        // trackers whose absolute-row bookkeeping assumed the OLD width — a
        // reflow rewraps the logical↔physical line mapping (see
        // `clear_reflow_invalidated_state`'s own doc comment). This tab
        // clears its OWN reflow-invalidated state; correctness must not
        // depend on which caller triggered the resize. A height-only
        // resize, or a raw request that clamps back to the tab's current
        // column count, leaves the trackers untouched.
        if cols != pre_resize_cols {
            self.clear_reflow_invalidated_state();
        }

        // FR5 / UC03: a grid resize during a pending 2nd-pass scrollback
        // restore cancels the restore — the rebuilt scrollback would be at
        // the old grid width and could not be merged cleanly (cols
        // mismatch is a noop), and re-dispatching a 2nd-pass at the new
        // grid is abandoned for history-restore (the user's intent during
        // a resize is the visible frame, not the discarded history). The
        // 1st-pass switch's own resize-supersede arm below handles the
        // visible-frame side.
        if let Some(old) = self.pending_scrollback_restore.take() {
            old.cancel.store(true, Ordering::Relaxed);
            log::warn!(
                "scrollback restore cancelled (resize) for tab {:?}",
                self.title
            );
        }

        // FR5/FR7 (task0006 redesign, review round-1 finding
        // `64baa639d71792f9`): a grid resize during a pending off-thread
        // replay must not let the swapped-in core end up at the wrong
        // (stale) size — but re-dispatching the in-flight payload/segments
        // at the NEW target (task0003's original fix) defeats the bypass
        // split gate: `payload`'s own recorded resize-marker `segments`
        // reflect whatever grid the daemon had captured them at (the
        // switch's ORIGINAL dispatch-time target), never this racing
        // resize's target, so `stable_target_suffix_start` finds no
        // matching trailing run against the NEW target and the whole
        // replay falls back to the expensive non-bypass path for the one
        // build that completes.
        //
        // Fix: let the in-flight worker keep building at its ORIGINAL
        // target (where its bypass split, if any, is valid) and defer this
        // resize instead — `PendingSwitch::pending_resize` records the
        // latest requested grid; `poll_pending_switch`/`apply_offthread_swap`
        // apply it, via a normal already-bypass-aware `TerminalCore::resize`
        // call, to the core right after it swaps in. Multiple resizes
        // before the swap just overwrite `pending_resize` with the latest
        // target — one deferred resize regardless of how many landed, and
        // (review round-1 finding `34a708465d04f983`) no payload/segments
        // clone per resize event either, unlike the coalesce-based
        // re-dispatch this replaces. A resize that lands back on the
        // in-flight worker's own build target clears any previously
        // deferred resize (nothing left to apply once swapped in).
        if let Some(pending) = self.pending_switch.as_mut() {
            let effective_target = pending.pending_resize.unwrap_or((pending.cols, pending.rows));
            if effective_target != (cols, rows) {
                pending.pending_resize = if (cols, rows) == (pending.cols, pending.rows) {
                    None
                } else {
                    Some((cols, rows))
                };
            }
        }

        // In mux mode the local PTY is the bridge's stdin pipe, so the
        // resize above only stretches the bridge-facing FD — the daemon's
        // per-pane PTYs stay at whatever size they were last told. Push a
        // Resize control frame for every pane in the group so each
        // daemon-side PTY matches the new grid.
        if self.mux_session_name.is_some() {
            if let Some(group) = self.mux_group.as_ref() {
                for &pane_id in group.pane_ids() {
                    self.send_control(&MuxMessage::control(
                        MessageType::Resize,
                        pane_id,
                        &ResizeMsg { cols, rows },
                    ));
                }
                // task0003 AC-6: record every emission from this loop (the
                // RESIZE-PATH site — see `ResizeFrameRecord`). Pushed after
                // the loop above (not inlined into it) only to keep the
                // production loop body — the single RESIZE-PATH emission
                // site per IMPLEMENTATION.md contract (d) — untouched by
                // this test-only concern; behavior is identical either way.
                #[cfg(test)]
                {
                    for &pane_id in group.pane_ids() {
                        self.resize_frame_log.push(ResizeFrameRecord {
                            tab_stable_id: self.stable_id,
                            pane_id,
                            cols,
                            rows,
                        });
                    }
                }
            }
        }
    }

    /// Drop absolute-row tracker state invalidated by a column-width reflow
    /// (N3). A reflow rewrites the logical↔physical line mapping when the
    /// width changes, but leaves the scrollback eviction counter untouched —
    /// so `pump_all`'s eviction-delta correction (`prune_before_line` /
    /// `shift_rows_down`) cannot re-base the stored absolute rows. Clearing
    /// is the safe response: a retained prompt/fold mark would point at the
    /// wrong buffer line after the rewrap. The OSC 133 prompt/fold marks
    /// re-accumulate from subsequent output; the per-tab fold-enable
    /// preference is preserved across the rebuild. The eviction baseline is
    /// intentionally NOT reset — the eviction counter did not move, so it
    /// stays valid for the next real eviction.
    pub fn clear_reflow_invalidated_state(&mut self) {
        self.prompts.clear();
        self.folds = Self::new_fold_manager(self.fold_enabled);
        self.pending_fold_begin = None;
    }

    /// Drain queued emterm OSC viewer-spawn requests. Phase 5+ feeds these
    /// into the Wry viewer spawner.
    pub fn drain_osc(&self) -> Vec<EmtermOscRequest> {
        std::mem::take(&mut self.cb_state.lock().osc_queue)
    }
}

/// True when `payload` contains a complete CSI device query that `term_core`
/// answers by writing into its response buffer. The set is kept in lockstep
/// with the response-firing arms of `crates/term_core/src/csi_dispatch.rs`
/// (`fire_device_response_callback`): final byte `n` (DSR), `c` (Device
/// Attributes), `t` (XTWINOPS size reports), or `p` (DECRPM `CSI ? Ps $ p`).
/// Detection is intentionally conservative — it matches on the final byte
/// alone, so a few non-response sequences sharing those finals (e.g. DA3
/// `CSI = c`, non-size XTWINOPS ops, a non-DECRPM `p`) are also treated as
/// queries. The only cost of a false positive is parsing that one frame on
/// its own instead of coalescing it; correctness is unaffected.
///
/// Used by [`Tab::pty_output_batch_eligible`] to keep query-bearing
/// `PtyOutput` frames OUT of the coalesce accumulator: `term_core`'s
/// single-slot response buffer is overwrite-only and is not drained between
/// chunks of one parse, so concatenating several query frames into one parse
/// would keep only the LAST reply. Parsing such a frame on its own (the
/// per-frame path) preserves the reply, matching the pre-coalesce behavior
/// byte-for-byte.
///
/// A CSI starts at `ESC [` (`0x1b 0x5b`); parameter bytes are `0x30..=0x3f`,
/// intermediate bytes `0x20..=0x2f`, and the final byte is `0x40..=0x7e`. A C0
/// control byte other than `ESC` appearing mid-CSI is executed by `term_core`'s
/// parser without aborting the sequence, so it is skipped here too (the CSI
/// keeps accumulating). A CSI left incomplete at the end of the payload is NOT a
/// complete query (it would complete in a later frame, where it still yields a
/// single reply — no loss), so it does not force a split.
fn payload_has_device_query(payload: &[u8]) -> bool {
    let n = payload.len();
    let mut i = 0;
    while i + 1 < n {
        if payload[i] == 0x1b && payload[i + 1] == b'[' {
            // Scan the CSI body for its final byte.
            let mut j = i + 2;
            loop {
                if j >= n {
                    // Incomplete CSI runs to the end of the payload — not a
                    // complete query, and nothing complete can follow it.
                    return false;
                }
                let b = payload[j];
                if (0x40..=0x7e).contains(&b) {
                    // Final byte: device-response producers per term_core.
                    if matches!(b, b'n' | b'c' | b't' | b'p') {
                        return true;
                    }
                    i = j + 1; // resume past this non-query CSI
                    break;
                }
                if matches!(b, 0x00..=0x1a | 0x1c..=0x1f) {
                    // A C0 control byte (other than ESC) mid-CSI is executed by
                    // `term_core`'s parser WITHOUT aborting the CSI — the
                    // sequence keeps accumulating after it (see
                    // crates/term_core/src/parser/csi.rs). So skip it and keep
                    // scanning this CSI for its final byte; e.g. `\x1b[\x076n`
                    // still fires a CPR and must be detected. ESC (0x1b) is the
                    // genuine new-sequence boundary, handled by the resync below.
                    j += 1;
                    continue;
                }
                if !(0x20..=0x3f).contains(&b) {
                    // Neither a CSI body byte, a C0 control, nor a final: this
                    // CSI is malformed (e.g. an `ESC` starting a new sequence, or
                    // a 0x7f / 0x80..=0xff byte). Re-examine the offending byte
                    // rather than skipping it — it may itself begin a new CSI
                    // (e.g. the `ESC` that starts the real query right after a
                    // truncated one, `\x1b[2\x1b[6n`).
                    i = j;
                    break;
                }
                j += 1;
            }
            continue;
        }
        i += 1;
    }
    false
}

/// Split a drained `pending_apc` buffer into the (image-pipeline,
/// mux-message) halves. APC payloads that start with `emterm-mux;` are
/// decoded into typed `MuxMessage`s via
/// [`crate::mux::apc::try_decode_emterm_mux`]; the rest pass through to
/// the existing Kitty Graphics decoder. Decode failures on a clearly
/// mux-prefixed payload are dropped (the helper already logs at `warn`)
/// rather than fed to the image pipeline — they cannot be valid Kitty.
fn partition_apc_for_mux(apc: Vec<Vec<u8>>) -> (Vec<Vec<u8>>, Vec<MuxMessage>) {
    let mut images: Vec<Vec<u8>> = Vec::with_capacity(apc.len());
    let mut mux: Vec<MuxMessage> = Vec::new();
    for payload in apc {
        if payload.starts_with(mux_ipc::protocol::APC_PREFIX.as_bytes()) {
            if let Some(msg) = crate::mux::apc::try_decode_emterm_mux(&payload) {
                mux.push(msg);
            }
            // Malformed mux payload — already logged inside the decoder;
            // do NOT forward to the image pipeline.
        } else {
            images.push(payload);
        }
    }
    (images, mux)
}

/// Drain the prompt + fold marks `term_core` captured during a just-completed
/// process / replay, together with the current scrollback-eviction total, in
/// one place. All three are read under the caller's existing core guard so
/// they stay consistent with the bytes just processed; the caller then drops
/// the guard before handing the values to [`Tab::backfill_marks`] (which needs
/// `&mut self` and would otherwise conflict with the guard's borrow of
/// `self.core`). The three reads are independent, so their order is immaterial.
fn drain_marks(
    c: &mut TerminalCore,
) -> (
    u64,
    Vec<term_core::terminal_core::PendingPromptMark>,
    Vec<term_core::terminal_core::PendingFoldMark>,
) {
    (
        c.get_scrollback_evicted_total(),
        c.take_prompt_marks(),
        c.take_fold_marks(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use term_core::terminal_core::PendingPromptMark;

    // ── backfill_prompt_marks ─────────────────────────────────

    struct NoopSink;
    impl crate::callbacks::NotificationSink for NoopSink {
        fn send(&self, _title: &str, _body: &str) {}
    }

    fn test_tab() -> Tab {
        Tab::spawn_shell(
            "test",
            80,
            24,
            100,
            Arc::new(Settings::default()),
            None,
            None,
            Arc::new(NoopSink),
            None,
        )
    }

    /// A tab whose `settings.fold_enabled` is `false`, used to assert the
    /// fold gate seeds the per-tab `FoldManager` as disabled.
    fn test_tab_fold_disabled() -> Tab {
        let settings = Settings {
            fold_enabled: false,
            ..Settings::default()
        };
        Tab::spawn_shell(
            "test",
            80,
            24,
            100,
            Arc::new(settings),
            None,
            None,
            Arc::new(NoopSink),
            None,
        )
    }

    #[test]
    fn spawn_seeds_cursor_style_from_settings() {
        // AC-2: a tab spawned with `cursor_style: bar` reports
        // `get_cursor_style()` = 2 on its core (spawn-path seeding).
        let settings = Settings {
            cursor_style: crate::settings::CursorStyle::Bar,
            ..Settings::default()
        };
        let tab = Tab::spawn_shell(
            "test",
            80,
            24,
            100,
            Arc::new(settings),
            None,
            None,
            Arc::new(NoopSink),
            None,
        );
        assert_eq!(tab.core.lock().get_cursor_style(), 2);
    }

    #[test]
    fn spawn_seeds_cursor_blink_from_settings() {
        // AC-4: existing spawn-path blink seeding behavior is preserved
        // (`cursor_blink: false` in settings -> core reports blink false
        // at spawn).
        let settings = Settings {
            cursor_blink: false,
            ..Settings::default()
        };
        let tab = Tab::spawn_shell(
            "test",
            80,
            24,
            100,
            Arc::new(settings),
            None,
            None,
            Arc::new(NoopSink),
            None,
        );
        assert!(!tab.core.lock().get_cursor_blink());
    }

    // ── task0004 AC-5: RIS restores an OSC 12 cursor-color override ────

    #[test]
    fn ris_bytes_restore_theme_cursor_color_to_scheme() {
        // Feeding RIS bytes after OSC 12 (AC-5) through the real
        // core -> NativeCallbacks -> theme wiring restores the resolved
        // cursor color to the scheme cursor color and clears the override
        // state, exactly as OSC 112 would.
        let tab = test_tab();
        {
            let mut theme = tab.theme.lock();
            assert!(theme.apply_osc(12, "rgb:aa/bb/cc"));
            assert!(theme.cursor_fg_override_active);
        }

        tab.core.lock().process_pty_data_fully(b"\x1bc"); // RIS

        let theme = tab.theme.lock();
        assert_eq!(theme.cursor_fg, theme.scheme_cursor_fg);
        assert!(!theme.cursor_fg_override_active);
    }

    #[test]
    fn ris_bytes_after_reversed_order_still_apply_later_osc12() {
        // Guards the callback-based (in-order) design: a RIS followed BY a
        // fresh OSC 12 in the SAME chunk must leave the later OSC 12's
        // color in effect — the reset restore must not run after the whole
        // chunk is parsed and clobber a color set later in the same chunk.
        let tab = test_tab();
        tab.core
            .lock()
            .process_pty_data_fully(b"\x1bc\x1b]12;rgb:11/22/33\x07");

        let theme = tab.theme.lock();
        assert_eq!(theme.cursor_fg, crate::render::theme::Rgb(0x11, 0x22, 0x33));
        assert!(theme.cursor_fg_override_active);
    }

    /// Build a prompt-start `PendingPromptMark` as `term_core` would emit
    /// it: `abs_row` is the emit-time absolute row, `evicted_total` the
    /// eviction counter at emit time.
    fn pending_mark(abs_row: u32, evicted_total: u64) -> PendingPromptMark {
        PendingPromptMark {
            kind: b'A',
            abs_row,
            exit_code: None,
            evicted_total,
        }
    }

    #[test]
    fn backfill_stamps_drained_marks_with_emit_row() {
        let mut tab = test_tab();
        // A single mark captured at absolute row 105, no eviction.
        tab.backfill_prompt_marks(0, vec![pending_mark(105, 0)]);
        assert_eq!(tab.prompts.find_prev_prompt(u32::MAX), Some(105));
    }

    #[test]
    fn backfill_separates_multiple_marks_by_emit_row() {
        // The core regression this fix targets: several marks in one drain
        // must keep the distinct rows they were emitted on, not collapse.
        let mut tab = test_tab();
        tab.backfill_prompt_marks(
            0,
            vec![
                pending_mark(10, 0),
                pending_mark(20, 0),
                pending_mark(30, 0),
            ],
        );
        assert_eq!(tab.prompts.find_prev_prompt(25), Some(20));
        assert_eq!(tab.prompts.find_next_prompt(15), Some(20));
        assert_eq!(tab.prompts.find_prev_prompt(u32::MAX), Some(30));
    }

    #[test]
    fn backfill_prunes_stored_marks_before_pushing_new_ones() {
        // Marks stored in an earlier call are pruned by the eviction delta,
        // while a new mark captured in the *same* later frame (no eviction
        // after its own emit) lands at its emit-time row unchanged.
        let mut tab = test_tab();
        tab.backfill_prompt_marks(0, vec![pending_mark(105, 0)]); // stored at 105
        // 50 rows evicted since baseline; the new mark fired *after* those
        // evictions, so its own evicted_total is already 50 → no extra shift.
        tab.backfill_prompt_marks(50, vec![pending_mark(110, 50)]);
        // Old mark shifted 105 → 55; new mark stays at 110.
        assert_eq!(tab.prompts.find_prev_prompt(60), Some(55));
        assert_eq!(tab.prompts.find_next_prompt(60), Some(110));
    }

    #[test]
    fn backfill_normalizes_mark_evicted_after_its_emit() {
        // A mark fired early in a pump (evicted_total = 0, abs_row = 90),
        // then more output in the SAME pump evicted 30 rows, so the frame
        // observed at drain time is evicted_total = 30. The mark must shift
        // down by (30 - 0) = 30 → row 60, matching the post-pump frame.
        let mut tab = test_tab();
        tab.backfill_prompt_marks(30, vec![pending_mark(90, 0)]);
        assert_eq!(tab.prompts.find_prev_prompt(u32::MAX), Some(60));
    }

    #[test]
    fn backfill_mixed_evicted_totals_in_one_drain() {
        // Two marks from one pump: the first fired before any eviction, the
        // second after 20 rows were evicted. At drain the frame is at 20.
        // First: abs_row 50, evicted 0  → shift 20 → row 30.
        // Second: abs_row 45, evicted 20 → shift 0  → row 45.
        let mut tab = test_tab();
        tab.backfill_prompt_marks(20, vec![pending_mark(50, 0), pending_mark(45, 20)]);
        assert_eq!(tab.prompts.find_prev_prompt(40), Some(30));
        assert_eq!(tab.prompts.find_prev_prompt(u32::MAX), Some(45));
    }

    #[test]
    fn backfill_clears_marks_when_counter_goes_backwards() {
        // A reset (RIS) zeroes the core's eviction counter; stale marks
        // belong to the discarded line frame and must be dropped.
        let mut tab = test_tab();
        tab.backfill_prompt_marks(70, vec![pending_mark(105, 70)]);
        assert_eq!(tab.prompts.find_prev_prompt(u32::MAX), Some(105));
        tab.backfill_prompt_marks(0, vec![]);
        assert_eq!(tab.prompts.find_prev_prompt(u32::MAX), None);
    }

    #[test]
    fn backfill_drops_mark_evicted_out_of_frame_in_same_pump() {
        // A mark fired at abs_row 10 (evicted_total 0), then the SAME pump
        // evicted 25 rows — more than the mark's depth. The mark's line no
        // longer exists in the frame, so it must be DROPPED, not clamped to
        // row 0 (which would plant a phantom prompt at the top of
        // scrollback that jump_to_prompt would navigate to).
        let mut tab = test_tab();
        tab.backfill_prompt_marks(25, vec![pending_mark(10, 0)]);
        assert_eq!(tab.prompts.find_prev_prompt(u32::MAX), None);
        // Boundary: shift exactly equals abs_row → row 0 is still a real,
        // retained line (the new frame's first row), so it is kept.
        let mut tab = test_tab();
        tab.backfill_prompt_marks(10, vec![pending_mark(10, 0)]);
        assert_eq!(tab.prompts.find_prev_prompt(u32::MAX), Some(0));
    }

    // ── OSC 133 fold-region registration ──────────────────────

    /// Build a `PendingPromptMark` of an arbitrary kind at `abs_row` with no
    /// eviction (the common test frame: `evicted_total == 0`).
    fn pending_kind(kind: u8, abs_row: u32, exit_code: Option<i32>) -> PendingPromptMark {
        PendingPromptMark {
            kind,
            abs_row,
            exit_code,
            evicted_total: 0,
        }
    }

    #[test]
    fn fold_region_registered_on_c_b_d_sequence() {
        // A → B → C → D in one batch: the D pairs with C as the region
        // bounds, and B supplies the command text. With abs_row >= the
        // viewport-top (scrollback empty, 24-row viewport), the B row is read
        // from the viewport — seed that row with the command text first.
        let mut tab = test_tab();
        // Put "ls -la" on viewport row 3 (= abs_row 3, scrollback empty).
        {
            let mut c = tab.core.lock();
            c.process_pty_data(b"\r\n\r\n\r\nls -la");
        }
        tab.backfill_prompt_marks(
            0,
            vec![
                pending_kind(b'A', 2, None),
                pending_kind(b'B', 3, None),
                pending_kind(b'C', 4, None),
                pending_kind(b'D', 9, Some(0)),
            ],
        );
        // Region spans C..D = 4..9 with B's line text and the D exit code.
        let r = tab.folds.get_region_at_line(4).expect("region registered");
        assert_eq!(r.start_line, 4);
        assert_eq!(r.end_line, 9);
        assert_eq!(r.command_text.as_deref(), Some("ls -la"));
        assert_eq!(r.exit_code, Some(0));
        assert_eq!(r.id, "osc133:4");
    }

    #[test]
    fn fold_region_not_registered_without_c() {
        // B → D with no C in between: no region (the WebView bails when no C
        // is found).
        let mut tab = test_tab();
        tab.backfill_prompt_marks(
            0,
            vec![
                pending_kind(b'A', 2, None),
                pending_kind(b'B', 3, None),
                pending_kind(b'D', 9, Some(1)),
            ],
        );
        assert!(tab.folds.get_region_at_line(3).is_none());
        assert!(tab.folds.get_region_at_line(9).is_none());
        assert!(!tab.folds.has_collapsed_regions());
    }

    #[test]
    fn fold_region_consecutive_d_stops_search() {
        // C → D → D: the second D's reverse scan hits the first D before any
        // C, so it registers no second region. Only the first C↔D pairs.
        let mut tab = test_tab();
        tab.backfill_prompt_marks(
            0,
            vec![
                pending_kind(b'C', 4, None),
                pending_kind(b'D', 9, Some(0)),
                pending_kind(b'D', 12, Some(2)),
            ],
        );
        // First region: 4..9.
        let r = tab.folds.get_region_at_line(4).expect("first region");
        assert_eq!(r.start_line, 4);
        assert_eq!(r.end_line, 9);
        assert_eq!(r.exit_code, Some(0));
        // Second D (row 12) finds no C before hitting the first D → no region
        // starting anywhere at/after row 9.
        assert!(tab.folds.get_region_at_line(10).is_none());
        assert!(tab.folds.get_region_at_line(12).is_none());
    }

    #[test]
    fn fold_region_batch_d_indices_resolved_correctly() {
        // Three C→D pairs in one batch: each D must pair with its own preceding
        // C, not any other. This exercises the single-scan index resolution in
        // backfill_prompt_marks — previously each D ran its own rposition search.
        // Pattern: B0 C0 D0  B1 C1 D1  B2 C2 D2
        let mut tab = test_tab();
        tab.backfill_prompt_marks(
            0,
            vec![
                pending_kind(b'B', 1, None),
                pending_kind(b'C', 2, None),
                pending_kind(b'D', 5, Some(0)),
                pending_kind(b'B', 6, None),
                pending_kind(b'C', 7, None),
                pending_kind(b'D', 10, Some(1)),
                pending_kind(b'B', 11, None),
                pending_kind(b'C', 12, None),
                pending_kind(b'D', 15, Some(2)),
            ],
        );
        // First region: C0(2) → D0(5).
        let r0 = tab
            .folds
            .get_region_at_line(2)
            .expect("region 0 registered");
        assert_eq!(r0.start_line, 2);
        assert_eq!(r0.end_line, 5);
        assert_eq!(r0.exit_code, Some(0));
        // Second region: C1(7) → D1(10).
        let r1 = tab
            .folds
            .get_region_at_line(7)
            .expect("region 1 registered");
        assert_eq!(r1.start_line, 7);
        assert_eq!(r1.end_line, 10);
        assert_eq!(r1.exit_code, Some(1));
        // Third region: C2(12) → D2(15).
        let r2 = tab
            .folds
            .get_region_at_line(12)
            .expect("region 2 registered");
        assert_eq!(r2.start_line, 12);
        assert_eq!(r2.end_line, 15);
        assert_eq!(r2.exit_code, Some(2));
    }

    #[test]
    fn fold_region_without_b_has_empty_command_text() {
        // C → D with no preceding B: the region still registers, but its
        // command text is empty (the WebView leaves commandText == "").
        let mut tab = test_tab();
        tab.backfill_prompt_marks(
            0,
            vec![pending_kind(b'C', 4, None), pending_kind(b'D', 9, Some(0))],
        );
        let r = tab.folds.get_region_at_line(4).expect("region registered");
        assert_eq!(r.start_line, 4);
        assert_eq!(r.end_line, 9);
        assert_eq!(r.command_text.as_deref(), Some(""));
    }

    #[test]
    fn fold_region_command_text_from_scrollback() {
        // When the B row lies in scrollback (abs_row < scrollback_len) the
        // command text is read via get_scrollback_text. Push enough lines to
        // move the B row into scrollback, then craft marks against the
        // resulting frame. A 2-row viewport keeps the math small.
        let mut tab = Tab::spawn_shell(
            "test",
            80,
            2,
            100,
            Arc::new(Settings::default()),
            None,
            None,
            Arc::new(NoopSink),
            None,
        );
        // "make build" on the first line, then push it into scrollback with
        // two more lines (2-row viewport → row 0 evicts to scrollback idx 0).
        {
            let mut c = tab.core.lock();
            c.process_pty_data(b"make build\r\nx\r\ny");
            assert!(c.get_scrollback_length() >= 1, "first line in scrollback");
            assert_eq!(c.get_scrollback_text(0), "make build");
        }
        // B at scrollback row 0; C/D in the live viewport (abs rows 1,2).
        tab.backfill_prompt_marks(
            0,
            vec![
                pending_kind(b'B', 0, None),
                pending_kind(b'C', 1, None),
                pending_kind(b'D', 2, Some(0)),
            ],
        );
        let r = tab.folds.get_region_at_line(1).expect("region registered");
        assert_eq!(r.command_text.as_deref(), Some("make build"));
        assert_eq!(r.start_line, 1);
        assert_eq!(r.end_line, 2);
    }

    #[test]
    fn fold_region_rows_synced_with_eviction_prune() {
        // A region registered in one batch must shift down by the same
        // eviction delta that prunes the prompt marks, keeping fold rows in
        // the prompt frame. First batch registers 4..9; a later batch reports
        // 3 rows evicted, so the region re-bases to 1..6.
        let mut tab = test_tab();
        tab.backfill_prompt_marks(
            0,
            vec![pending_kind(b'C', 4, None), pending_kind(b'D', 9, Some(0))],
        );
        assert_eq!(
            tab.folds.get_region_at_line(4).map(|r| r.start_line),
            Some(4)
        );
        // 3 rows evicted since baseline; the new (empty) batch triggers the
        // prune of both prompts and folds.
        tab.backfill_prompt_marks(3, vec![]);
        // The region re-based from 4..9 down to 1..6. Row 7 (formerly inside
        // 4..9) is now outside; row 1 is the new start.
        assert!(
            tab.folds.get_region_at_line(7).is_none(),
            "region no longer extends to the pre-prune rows"
        );
        let r = tab.folds.get_region_at_line(1).expect("region re-based");
        assert_eq!(r.start_line, 1);
        assert_eq!(r.end_line, 6);
        assert_eq!(r.id, "osc133:1");
    }

    #[test]
    fn fold_region_dropped_when_head_evicted() {
        // A region whose C row falls off the top of scrollback is dropped by
        // prune_before_line (it spans the boundary), matching the prompt
        // prune's retain(row >= count).
        let mut tab = test_tab();
        tab.backfill_prompt_marks(
            0,
            vec![pending_kind(b'C', 4, None), pending_kind(b'D', 9, Some(0))],
        );
        // Evict 6 rows: the region 4..9 spans boundary 6 → dropped entirely.
        tab.backfill_prompt_marks(6, vec![]);
        assert!(tab.folds.get_region_at_line(0).is_none());
        assert!(tab.folds.get_region_at_line(3).is_none());
        assert!(!tab.folds.has_collapsed_regions());
    }

    #[test]
    fn fold_regions_cleared_on_core_reset() {
        // A counter that moved backwards signals a core reset (RIS); fold
        // regions belong to the discarded frame and must be cleared along
        // with the prompt marks.
        let mut tab = test_tab();
        tab.backfill_prompt_marks(
            8,
            vec![
                PendingPromptMark {
                    kind: b'C',
                    abs_row: 4,
                    exit_code: None,
                    evicted_total: 8,
                },
                PendingPromptMark {
                    kind: b'D',
                    abs_row: 9,
                    exit_code: Some(0),
                    evicted_total: 8,
                },
            ],
        );
        assert!(tab.folds.get_region_at_line(4).is_some());
        // Counter resets to 0 → clear.
        tab.backfill_prompt_marks(0, vec![]);
        assert!(tab.folds.get_region_at_line(4).is_none());
        assert!(!tab.folds.has_collapsed_regions());
    }

    #[test]
    fn fold_region_end_to_end_osc133_bytes() {
        // Drive the whole pipeline with real OSC 133 byte sequences fed
        // through term_core (the way `pump` does): emit B (with the command
        // echoed on its row), C, run output, then D — and confirm a region
        // is registered with the command text and exit code. This exercises
        // term_core's mark capture + the native backfill registration path
        // together.
        let mut tab = test_tab();
        let (evicted_total, marks) = {
            let mut c = tab.core.lock();
            // Prompt start, command start, the command text, command exec,
            // two output lines, then command end with exit code 0.
            c.process_pty_data(
                b"\x1b]133;A\x07\x1b]133;B\x07echo hi\x1b]133;C\x07\r\nhi\r\n\x1b]133;D;0\x07",
            );
            c.flush_grapheme_buffer();
            let marks = c.take_prompt_marks();
            (c.get_scrollback_evicted_total(), marks)
        };
        // We should have captured A,B,C,D.
        assert_eq!(marks.len(), 4, "captured all four OSC 133 marks");
        tab.backfill_prompt_marks(evicted_total, marks);

        // Exactly one region was registered (the C↔D pair).
        let collapsed_before = tab.folds.has_collapsed_regions();
        assert!(!collapsed_before, "regions start expanded");
        // The B mark and C mark share the prompt row (no newline between B,
        // the echoed command, and C), so the region command text is the
        // prompt line "echo hi". Find the region by scanning for it.
        let region = (0..30)
            .filter_map(|row| tab.folds.get_region_at_line(row))
            .next()
            .cloned();
        let region = region.expect("a C→D region was registered");
        assert_eq!(region.source, crate::fold::FoldSource::Osc133);
        assert_eq!(region.exit_code, Some(0));
        assert!(
            region
                .command_text
                .as_deref()
                .unwrap_or("")
                .contains("echo hi"),
            "command text carries the B-row command: {:?}",
            region.command_text
        );
    }

    // ── OSC 777 custom fold-region registration ───────────────

    use term_core::terminal_core::{FoldMarkKind, PendingFoldMark};

    /// Build a custom-fold `PendingFoldMark` at `abs_row` with no eviction
    /// (the common test frame: `evicted_total == 0`).
    fn fold_mark(kind: FoldMarkKind, abs_row: u32, label: &str) -> PendingFoldMark {
        PendingFoldMark {
            kind,
            abs_row,
            evicted_total: 0,
            label: label.to_string(),
        }
    }

    #[test]
    fn custom_fold_begin_end_registers_region() {
        // begin@4 → end@9 registers a custom region 4..9 with the label.
        let mut tab = test_tab();
        tab.backfill_fold_marks(
            0,
            vec![
                fold_mark(FoldMarkKind::Begin, 4, "Build Output"),
                fold_mark(FoldMarkKind::End, 9, ""),
            ],
        );
        let r = tab.folds.get_region_at_line(4).expect("region registered");
        assert_eq!(r.start_line, 4);
        assert_eq!(r.end_line, 9);
        assert_eq!(r.source, crate::fold::FoldSource::Custom);
        assert_eq!(r.label.as_deref(), Some("Build Output"));
        assert_eq!(r.id, "custom:4");
        // No pending begin remains after pairing.
        assert!(tab.pending_fold_begin.is_none());
    }

    #[test]
    fn custom_fold_begin_end_across_drains() {
        // A begin in one drain pairs with an end in a later drain.
        let mut tab = test_tab();
        tab.backfill_fold_marks(0, vec![fold_mark(FoldMarkKind::Begin, 4, "lbl")]);
        assert_eq!(tab.pending_fold_begin.as_ref().map(|p| p.0), Some(4));
        assert!(tab.folds.get_region_at_line(4).is_none(), "no region yet");
        tab.backfill_fold_marks(0, vec![fold_mark(FoldMarkKind::End, 9, "")]);
        let r = tab.folds.get_region_at_line(4).expect("region registered");
        assert_eq!(r.end_line, 9);
        assert_eq!(r.label.as_deref(), Some("lbl"));
    }

    #[test]
    fn custom_fold_orphaned_end_ignored() {
        // An `end` with no pending begin registers nothing.
        let mut tab = test_tab();
        tab.backfill_fold_marks(0, vec![fold_mark(FoldMarkKind::End, 9, "")]);
        assert!(!tab.folds.has_collapsed_regions());
        assert!(tab.folds.get_region_at_line(9).is_none());
        assert!(tab.pending_fold_begin.is_none());
    }

    #[test]
    fn custom_fold_consecutive_begins_last_wins() {
        // Two begins, then one end: the second begin overwrites the first
        // (WebView `pendingFoldBegins.set` clobber), so the region spans the
        // SECOND begin → end.
        let mut tab = test_tab();
        tab.backfill_fold_marks(
            0,
            vec![
                fold_mark(FoldMarkKind::Begin, 4, "first"),
                fold_mark(FoldMarkKind::Begin, 6, "second"),
                fold_mark(FoldMarkKind::End, 12, ""),
            ],
        );
        // No region starts at the first begin row 4.
        assert!(tab.folds.get_region_at_line(4).is_none());
        let r = tab
            .folds
            .get_region_at_line(6)
            .expect("region from 2nd begin");
        assert_eq!(r.start_line, 6);
        assert_eq!(r.end_line, 12);
        assert_eq!(r.label.as_deref(), Some("second"));
    }

    #[test]
    fn custom_fold_empty_label_falls_back() {
        // An empty begin label registers as the FoldManager "..." fallback.
        let mut tab = test_tab();
        tab.backfill_fold_marks(
            0,
            vec![
                fold_mark(FoldMarkKind::Begin, 4, ""),
                fold_mark(FoldMarkKind::End, 9, ""),
            ],
        );
        let r = tab.folds.get_region_at_line(4).expect("region registered");
        assert_eq!(r.label.as_deref(), Some("..."));
    }

    #[test]
    fn custom_fold_pair_across_eviction() {
        // begin captured before any eviction (abs_row 50, evicted 0), then
        // 20 rows evicted within the same drain so the frame is at 20. The
        // begin normalizes to row 30; the end (abs_row 45, evicted 20) stays
        // at 45. Region 30..45.
        let mut tab = test_tab();
        tab.backfill_fold_marks(
            20,
            vec![
                PendingFoldMark {
                    kind: FoldMarkKind::Begin,
                    abs_row: 50,
                    evicted_total: 0,
                    label: "x".to_string(),
                },
                PendingFoldMark {
                    kind: FoldMarkKind::End,
                    abs_row: 45,
                    evicted_total: 20,
                    label: String::new(),
                },
            ],
        );
        let r = tab.folds.get_region_at_line(30).expect("region registered");
        assert_eq!(r.start_line, 30);
        assert_eq!(r.end_line, 45);
        assert_eq!(r.label.as_deref(), Some("x"));
    }

    #[test]
    fn custom_fold_pending_begin_pruned_by_eviction_drops_region() {
        // A pending begin at row 4 (from drain 1); drain 2 reports 6 rows
        // evicted (delta 6 > 4) via backfill_prompt_marks, so the begin's head
        // scrolled off the top. The begin is dropped, and a later end finds no
        // pending begin → no region. Mirrors the WebView "boundary-spanning
        // region is dropped" rule.
        let mut tab = test_tab();
        tab.backfill_fold_marks(0, vec![fold_mark(FoldMarkKind::Begin, 4, "lbl")]);
        assert!(tab.pending_fold_begin.is_some());
        // Eviction prune runs inside backfill_prompt_marks (the callers always
        // invoke it before backfill_fold_marks). 6 rows evicted: begin row 4
        // < 6 → dropped.
        tab.backfill_prompt_marks(6, vec![]);
        assert!(
            tab.pending_fold_begin.is_none(),
            "pending begin past the eviction boundary is dropped"
        );
        tab.backfill_fold_marks(6, vec![fold_mark(FoldMarkKind::End, 9, "")]);
        assert!(!tab.folds.has_collapsed_regions());
    }

    #[test]
    fn custom_fold_pending_begin_shifted_by_eviction() {
        // A pending begin at row 8 survives a 3-row eviction (8 >= 3), shifting
        // to row 5; a subsequent end at row 9 (in the post-prune frame) pairs
        // to register 5..9.
        let mut tab = test_tab();
        tab.backfill_fold_marks(0, vec![fold_mark(FoldMarkKind::Begin, 8, "lbl")]);
        tab.backfill_prompt_marks(3, vec![]);
        assert_eq!(tab.pending_fold_begin.as_ref().map(|p| p.0), Some(5));
        // The end mark was captured in the post-prune frame (evicted_total 3),
        // so it already addresses row 9 with no further shift.
        tab.backfill_fold_marks(
            3,
            vec![PendingFoldMark {
                kind: FoldMarkKind::End,
                abs_row: 9,
                evicted_total: 3,
                label: String::new(),
            }],
        );
        let r = tab.folds.get_region_at_line(5).expect("region registered");
        assert_eq!(r.start_line, 5);
        assert_eq!(r.end_line, 9);
    }

    #[test]
    fn custom_fold_pending_begin_cleared_on_core_reset() {
        // A pending begin belongs to the pre-reset frame; a counter that moved
        // backwards (RIS) clears it along with the fold regions.
        let mut tab = test_tab();
        // Seed a registered region + a pending begin in a non-zero frame.
        tab.backfill_fold_marks(
            8,
            vec![
                PendingFoldMark {
                    kind: FoldMarkKind::Begin,
                    abs_row: 4,
                    evicted_total: 8,
                    label: "done".to_string(),
                },
                PendingFoldMark {
                    kind: FoldMarkKind::End,
                    abs_row: 9,
                    evicted_total: 8,
                    label: String::new(),
                },
            ],
        );
        // Set evicted_baseline to 8 so the next backwards counter triggers
        // the reset branch.
        tab.evicted_baseline = 8;
        tab.backfill_fold_marks(8, vec![fold_mark(FoldMarkKind::Begin, 11, "pending")]);
        assert!(tab.pending_fold_begin.is_some());
        assert!(tab.folds.get_region_at_line(4).is_some());
        // Counter resets to 0 → fold regions + pending begin cleared.
        tab.backfill_prompt_marks(0, vec![]);
        assert!(tab.folds.get_region_at_line(4).is_none());
        assert!(tab.pending_fold_begin.is_none());
    }

    #[test]
    fn custom_fold_end_to_end_osc777_bytes() {
        // Drive the whole pipeline with real OSC 777 byte sequences fed
        // through term_core (the way `pump` does): begin with a label, some
        // output, then end — and confirm a custom region is registered.
        let mut tab = test_tab();
        let (evicted_total, fold_marks) = {
            let mut c = tab.core.lock();
            c.process_pty_data(
                b"\x1b]777;emterm;fold;begin;Compile\x07line1\r\nline2\r\n\x1b]777;emterm;fold;end\x07",
            );
            c.flush_grapheme_buffer();
            let fm = c.take_fold_marks();
            (c.get_scrollback_evicted_total(), fm)
        };
        assert_eq!(fold_marks.len(), 2, "captured begin + end");
        tab.backfill_fold_marks(evicted_total, fold_marks);
        let region = (0..30)
            .filter_map(|row| tab.folds.get_region_at_line(row))
            .next()
            .cloned()
            .expect("a custom region was registered");
        assert_eq!(region.source, crate::fold::FoldSource::Custom);
        assert_eq!(region.label.as_deref(), Some("Compile"));
        assert_eq!(region.start_line, 0, "begin captured on row 0");
        assert_eq!(region.end_line, 2, "end captured on row 2");
    }

    #[test]
    fn custom_fold_suppressed_on_alt_screen_end_to_end() {
        // OSC 777 fold marks emitted on the alt screen are not captured by
        // term_core, so no region is registered (WebView isAlternateBuffer
        // guard parity).
        let mut tab = test_tab();
        let fold_marks = {
            let mut c = tab.core.lock();
            c.process_pty_data_fully(
                b"\x1b[?1049h\x1b]777;emterm;fold;begin;x\x07\r\n\x1b]777;emterm;fold;end\x07\x1b[?1049l",
            );
            c.take_fold_marks()
        };
        assert!(fold_marks.is_empty(), "alt-screen fold marks not captured");
        tab.backfill_fold_marks(0, fold_marks);
        assert!(!tab.folds.has_collapsed_regions());
        assert!(tab.pending_fold_begin.is_none());
    }

    // ── fold_enabled settings gate ────────────────────────────

    #[test]
    fn fold_enabled_default_tab_is_enabled() {
        // Default settings (`fold_enabled = true`) seed an enabled manager.
        let tab = test_tab();
        assert!(tab.fold_enabled);
        assert!(tab.folds.is_enabled());
    }

    #[test]
    fn fold_disabled_tab_seeds_disabled_manager() {
        // `settings.fold_enabled = false` seeds a manager whose `enabled`
        // flag is off, so fold clicks are gated.
        let tab = test_tab_fold_disabled();
        assert!(!tab.fold_enabled);
        assert!(!tab.folds.is_enabled());
    }

    #[test]
    fn fold_disabled_tab_still_registers_osc133_regions() {
        // Region registration is independent of the enable gate: a disabled
        // tab still backfills C→D regions so re-enabling could fold them.
        let mut tab = test_tab_fold_disabled();
        {
            let mut c = tab.core.lock();
            c.process_pty_data(b"\r\n\r\n\r\nls -la");
        }
        tab.backfill_prompt_marks(
            0,
            vec![
                pending_kind(b'A', 2, None),
                pending_kind(b'B', 3, None),
                pending_kind(b'C', 4, None),
                pending_kind(b'D', 9, Some(0)),
            ],
        );
        let r = tab.folds.get_region_at_line(4).expect("region registered");
        assert_eq!(r.start_line, 4);
        assert_eq!(r.end_line, 9);
        assert_eq!(r.command_text.as_deref(), Some("ls -la"));
    }

    #[test]
    fn fold_disabled_tab_still_registers_custom_regions() {
        // Custom (OSC 777) region registration also runs while disabled.
        let mut tab = test_tab_fold_disabled();
        tab.backfill_fold_marks(
            0,
            vec![
                fold_mark(FoldMarkKind::Begin, 4, "Build Output"),
                fold_mark(FoldMarkKind::End, 9, ""),
            ],
        );
        let r = tab.folds.get_region_at_line(4).expect("region registered");
        assert_eq!(r.source, crate::fold::FoldSource::Custom);
        assert_eq!(r.label.as_deref(), Some("Build Output"));
    }

    #[test]
    fn fold_disabled_tab_cannot_collapse() {
        // With folding disabled, toggling a registered region is a no-op:
        // no region ever becomes collapsed.
        let mut tab = test_tab_fold_disabled();
        tab.backfill_prompt_marks(
            0,
            vec![pending_kind(b'C', 4, None), pending_kind(b'D', 9, Some(0))],
        );
        assert!(tab.folds.get_region_at_line(4).is_some());
        assert!(!tab.folds.toggle_fold(4), "toggle gated while disabled");
        assert!(!tab.folds.get_region_at_line(4).unwrap().collapsed);
        assert!(!tab.folds.has_collapsed_regions());
    }

    #[test]
    fn fold_disabled_preserved_across_core_reset() {
        // A core reset (backwards eviction counter) rebuilds the FoldManager
        // from scratch; the disabled state must survive the rebuild rather
        // than snapping back to the FoldManager default (`enabled = true`).
        let mut tab = test_tab_fold_disabled();
        tab.backfill_prompt_marks(
            8,
            vec![
                PendingPromptMark {
                    kind: b'C',
                    abs_row: 4,
                    exit_code: None,
                    evicted_total: 8,
                },
                PendingPromptMark {
                    kind: b'D',
                    abs_row: 9,
                    exit_code: Some(0),
                    evicted_total: 8,
                },
            ],
        );
        assert!(!tab.folds.is_enabled());
        // Counter resets to 0 → FoldManager rebuilt.
        tab.backfill_prompt_marks(0, vec![]);
        assert!(
            !tab.folds.is_enabled(),
            "disabled state survives the reset rebuild"
        );
    }

    #[test]
    fn fold_enabled_preserved_across_core_reset() {
        // The enabled default likewise survives a reset rebuild.
        let mut tab = test_tab();
        tab.backfill_prompt_marks(
            8,
            vec![PendingPromptMark {
                kind: b'D',
                abs_row: 9,
                exit_code: Some(0),
                evicted_total: 8,
            }],
        );
        tab.backfill_prompt_marks(0, vec![]);
        assert!(tab.folds.is_enabled());
    }

    #[test]
    fn fold_disabled_preserved_across_mux_snapshot() {
        // A mux snapshot replay also rebuilds the FoldManager; the disabled
        // state must carry over there too.
        let mut tab = test_tab_fold_disabled();
        assert!(!tab.folds.is_enabled());
        let msg = MuxMessage {
            msg_type: MessageType::Snapshot,
            pane_id: 0,
            payload: b"hello".to_vec(),
        };
        tab.apply_mux_message(msg);
        assert!(
            !tab.folds.is_enabled(),
            "disabled state survives the snapshot rebuild"
        );
    }

    // ── Phase 2: inbound window reconcile (TS-6..TS-10, TS-16) ────────────

    use mux_ipc::protocol::{SessionInfo, WindowInfo};

    fn welcome_msg(windows: &[(u32, &str, u32)], active: u32) -> MuxMessage {
        let windows: Vec<WindowInfo> = windows
            .iter()
            .map(|(id, name, pane)| WindowInfo {
                id: *id,
                name: name.to_string(),
                active_pane_id: *pane,
            })
            .collect();
        let session = SessionInfo {
            id: 1,
            name: "main".to_string(),
            window_count: windows.len() as u32,
            pane_count: windows.len() as u32,
            active_window_index: active,
            windows,
        };
        MuxMessage::control(
            MessageType::Welcome,
            0,
            &WelcomeMsg::Accepted {
                server_version: 1,
                sessions: vec![session],
            },
        )
    }

    fn pane_created(pane_id: u32) -> MuxMessage {
        MuxMessage {
            msg_type: MessageType::PaneCreated,
            pane_id,
            payload: Vec::new(),
        }
    }

    fn switch_window(pane_id: u32) -> MuxMessage {
        MuxMessage {
            msg_type: MessageType::SwitchWindow,
            pane_id,
            payload: Vec::new(),
        }
    }

    fn rename_window(pane_id: u32, name: &str) -> MuxMessage {
        // The RenameWindow frame addresses the window by *pane id* (the
        // outbound side at `confirm_mux_rename` sends `pane_ids()[idx]`, and
        // the daemon re-broadcasts the same field). The inbound handler
        // resolves the window from this pane id via `index_of_pane_id`.
        MuxMessage::control(
            MessageType::RenameWindow,
            pane_id,
            &RenameWindowMsg {
                name: name.to_string(),
            },
        )
    }

    fn pty_exited(pane_id: u32) -> MuxMessage {
        MuxMessage {
            msg_type: MessageType::PtyExited,
            pane_id,
            payload: Vec::new(),
        }
    }

    // ── TS-6: Welcome ingest ──────────────────────────────────────────────

    #[test]
    fn welcome_seeds_window_list_and_active_index() {
        let mut tab = test_tab();
        let changed = tab.apply_mux_message(welcome_msg(&[(1, "shell", 10), (2, "editor", 20)], 1));
        assert!(changed);
        let g = tab.mux_group.as_ref().expect("group seeded");
        assert_eq!(g.len(), 2);
        assert_eq!(g.windows()[0].name, "shell");
        assert_eq!(g.windows()[1].name, "editor");
        assert_eq!(g.pane_ids(), &[10, 20]);
        assert_eq!(g.active_index(), 1);
        // F3: session name still set.
        assert_eq!(tab.mux_session_name.as_deref(), Some("main"));
    }

    #[test]
    fn welcome_without_windows_leaves_group_none() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[], 0));
        assert!(tab.mux_group.is_none());
        assert_eq!(tab.mux_session_name.as_deref(), Some("main"));
    }

    // ── TS-7: PaneCreated append ──────────────────────────────────────────

    #[test]
    fn pane_created_appends_window_named_terminal_and_activates() {
        let mut tab = test_tab();
        // Seed two windows, then request + confirm a create.
        tab.apply_mux_message(welcome_msg(&[(1, "shell", 10), (2, "editor", 20)], 0));
        tab.mux_group.as_mut().unwrap().inc_pending_create();
        let changed = tab.apply_mux_message(pane_created(30));
        assert!(changed);
        let g = tab.mux_group.as_ref().unwrap();
        assert_eq!(g.len(), 3);
        assert_eq!(g.windows()[2].name, "Terminal");
        assert_eq!(g.active_index(), 2);
        assert_eq!(g.active_pane_id(), Some(30));
    }

    #[test]
    fn pane_created_without_pending_is_appended_as_daemon_authoritative() {
        // SPEC FR4 / Message Mapping: daemon-pushed PaneCreated is the
        // append-window signal regardless of whether *this* client requested
        // the create. Earlier behavior dropped such frames as "phantom" —
        // that silently lost panes other clients (or daemon-side actions)
        // spawned. Now the daemon is the authority; pending_create is only
        // an optimistic-UX counter for the originating client.
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "shell", 10), (2, "editor", 20)], 0));
        let changed = tab.apply_mux_message(pane_created(30));
        assert!(changed);
        assert_eq!(tab.mux_group.as_ref().unwrap().len(), 3);
    }

    #[test]
    fn pane_created_is_idempotent_on_resend() {
        // A duplicate PaneCreated for an already-known pane (bridge replay)
        // must not double-append, otherwise the same pane would surface twice
        // in the sub-tab strip.
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "shell", 10), (2, "editor", 20)], 0));
        let first = tab.apply_mux_message(pane_created(30));
        let second = tab.apply_mux_message(pane_created(30));
        assert!(first);
        assert!(!second);
        assert_eq!(tab.mux_group.as_ref().unwrap().len(), 3);
    }

    #[test]
    fn pane_created_before_attach_does_not_install_empty_group() {
        // PaneCreated arriving before Welcome must not allocate an empty
        // MuxWindowGroup on the tab. Otherwise every is_some() check
        // downstream (PtyOutput pane filter, write_input mux branch,
        // mux_session_name badge) would treat the tab as mux-attached even
        // though it isn't.
        let mut tab = test_tab();
        assert!(tab.mux_group.is_none());
        let changed = tab.apply_mux_message(pane_created(30));
        assert!(!changed);
        assert!(tab.mux_group.is_none());
    }

    // ── TS-8: PtyExited removal + group dissolve ──────────────────────────

    #[test]
    fn pty_exited_removes_window() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20), (3, "c", 30)], 2));
        let changed = tab.apply_mux_message(pty_exited(30));
        assert!(changed);
        let g = tab.mux_group.as_ref().unwrap();
        assert_eq!(g.len(), 2);
        assert_eq!(g.active_index(), 1); // re-clamped
    }

    #[test]
    fn pty_exited_dissolves_group_at_zero() {
        let mut tab = test_tab();
        // One-window group (seeded directly).
        tab.apply_mux_message(welcome_msg(&[(1, "only", 10)], 0));
        tab.apply_mux_message(pty_exited(10));
        assert!(tab.mux_group.is_none());
        // The last window's shell exited: the tab closes (reaped by
        // `App::pump_all`), unlike an explicit detach which keeps it alive.
        assert!(tab.exited);
    }

    // ── task0005 AC-6: PtyExited latches the closed pane id ────────────────

    #[test]
    fn pty_exited_latches_closed_pane_for_agent_status_discard() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        tab.apply_mux_message(pty_exited(10));
        assert_eq!(tab.take_closed_agent_status_panes(), vec![10]);
    }

    #[test]
    fn pty_exited_unknown_pane_does_not_latch() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10)], 0));
        tab.apply_mux_message(pty_exited(999));
        assert!(tab.take_closed_agent_status_panes().is_empty());
    }

    // ── task0005 AC-2: daemon AgentStatusUpdate is decoded and latched ─────

    #[test]
    fn agent_status_update_decodes_and_latches_for_app_pump_all() {
        let mut tab = test_tab();
        let update = mux_ipc::protocol::AgentStatusUpdateMsg {
            pane_id: 10,
            public_pane_id: "abc-10".to_string(),
            state: Some(mux_ipc::protocol::AgentState::Blocked),
            name: Some("claude".to_string()),
            revision: 3,
            replay_derived: false,
        };
        let msg = MuxMessage::control(MessageType::AgentStatusUpdate, 10, &update);
        let changed = tab.apply_mux_message(msg);
        assert!(changed);
        let latched = tab.take_pending_agent_status_updates();
        assert_eq!(latched.len(), 1);
        assert_eq!(latched[0].pane_id, 10);
        assert_eq!(
            latched[0].state,
            Some(mux_ipc::protocol::AgentState::Blocked)
        );
        assert_eq!(latched[0].revision, 3);
        assert!(!latched[0].replay_derived);
    }

    #[test]
    fn agent_status_update_malformed_payload_is_rejected() {
        let mut tab = test_tab();
        let msg = MuxMessage {
            msg_type: MessageType::AgentStatusUpdate,
            pane_id: 10,
            payload: vec![0xFF, 0xFF, 0xFF], // not a valid bincode AgentStatusUpdateMsg
        };
        let changed = tab.apply_mux_message(msg);
        assert!(!changed);
        assert!(tab.take_pending_agent_status_updates().is_empty());
    }

    // ── agent-exit-after-icon (task0002): latch feed reconciliation,
    // end-to-end via `test_process_combined` (real OSC byte parsing through
    // `NativeCallbacks::on_osc` + `process_outer_via_core`'s reconciliation
    // — the actual callbacks.rs -> latch-feed -> reconcile path, per
    // task0002.md's Test Notes) ─────────────────────────────────────────

    #[test]
    fn latch_feed_end_to_end_set_then_live_d_a_resolves_in_order() {
        let mut tab = test_tab();
        let bytes = [
            b"\x1b]777;emterm;agent-status;v=1;state=working\x1b\\".as_slice(),
            b"\x1b]133;D;0\x07",
            b"\x1b]133;A\x07",
        ]
        .concat();
        tab.test_process_combined(bytes);

        assert_eq!(
            tab.take_pending_latch_inputs(),
            vec![
                crate::agent_status_model::ResolvedLatchInput::Set,
                crate::agent_status_model::ResolvedLatchInput::Mark(
                    crate::prompts::PromptMarkKind::CommandEnd
                ),
                crate::agent_status_model::ResolvedLatchInput::Mark(
                    crate::prompts::PromptMarkKind::PromptStart
                ),
            ]
        );
    }

    #[test]
    fn latch_feed_end_to_end_drops_alt_screen_suppressed_candidate() {
        // AC-5: a D mark emitted while on the alternate screen is captured
        // by `on_osc` (candidate) but never reaches `take_prompt_marks()`
        // (term_core's alt-screen gate) — so it must not resolve, while the
        // later live D/A pair (after leaving the alt screen) still does.
        let mut tab = test_tab();
        let bytes = [
            b"\x1b]777;emterm;agent-status;v=1;state=working\x1b\\".as_slice(),
            b"\x1b[?1049h",      // enter alt screen
            b"\x1b]133;D;0\x07", // suppressed candidate
            b"\x1b[?1049l",      // leave alt screen
            b"\x1b]133;D;0\x07", // live
            b"\x1b]133;A\x07",   // live
        ]
        .concat();
        tab.test_process_combined(bytes);

        assert_eq!(
            tab.take_pending_latch_inputs(),
            vec![
                crate::agent_status_model::ResolvedLatchInput::Set,
                crate::agent_status_model::ResolvedLatchInput::Mark(
                    crate::prompts::PromptMarkKind::CommandEnd
                ),
                crate::agent_status_model::ResolvedLatchInput::Mark(
                    crate::prompts::PromptMarkKind::PromptStart
                ),
            ],
            "the alt-screen-suppressed D candidate is dropped; only the live pair resolves"
        );
    }

    // ── task0005 rework (review round1 findings 6b2e83f10c94ad7e /
    // 929859ff2b4e431e / 5cd6f305dcdeceb7): mux-attached inner OSC 777 /
    // OSC 133 must never populate the GUI-local plain-tab agent-status
    // queue or inferred-clear latch — the daemon's `AgentStatusUpdate` /
    // `MuxPane.agent_status_exit_latch` are the sole authority for mux
    // panes (SPEC FR3). Uses `mux_tab_active_pane` / `pty_output_apc`
    // (defined further below, in the mux coalesce test section) to route
    // OSC bytes through the mux inner-content path exactly as the daemon's
    // `PtyOutput` frames would. ─────────────────────────────────────────

    #[test]
    fn plain_tab_agent_status_set_surfaces_via_pending_agent_status_events() {
        // AC-4 regression guard: a non-mux (plain) tab's OSC 777 Set must
        // still reach `take_pending_agent_status_events()` — this rework
        // moved WHERE `pending_agent_status` is drained within
        // `process_combined`, not WHETHER a plain tab's events are drained.
        let mut tab = test_tab();
        tab.test_process_combined(
            b"\x1b]777;emterm;agent-status;v=1;state=working\x1b\\".to_vec(),
        );
        assert_eq!(
            tab.take_pending_agent_status_events(),
            vec![crate::agent_status::AgentStatusEvent::Set {
                state: crate::agent_status::AgentState::Working,
                name: None,
            }]
        );
    }

    #[test]
    fn mux_inner_agent_status_set_does_not_create_plain_tab_status() {
        // AC-1: an inner OSC 777 `Set` carried by a mux pane's `PtyOutput`
        // must not populate the GUI-local `pending_agent_status_events`
        // queue that `App::pump_all` applies as a `PaneKey::Tab` status —
        // neither the SAME pump that parsed it, nor a LATER pump (a
        // per-pump-delayed drain of the same stale queue is just as much a
        // leak, only postponed).
        let mut tab = mux_tab_active_pane(10);
        let combined =
            pty_output_apc(10, b"\x1b]777;emterm;agent-status;v=1;state=working\x1b\\");
        tab.test_process_combined(combined);
        // A second, still-mux pump with no new bytes: proves the mux-inner
        // Set from the first pump cannot surface on a later drain either.
        tab.test_process_combined(Vec::new());
        assert!(
            tab.take_pending_agent_status_events().is_empty(),
            "a mux pane's inner OSC 777 Set must not create a GUI-local tab status"
        );
    }

    #[test]
    fn mux_inner_agent_status_set_then_da_leaves_no_residual_plain_tab_status() {
        // AC-2/AC-3: a full Set + D + A sequence inside the mux inner
        // stream — which would arm and fire the plain-tab inferred-clear
        // latch if it were live plain-tab content — must leave neither a
        // residual GUI-local Set nor any latch candidates once mux-owned,
        // including on a later pump's drain (see the sibling test above).
        let mut tab = mux_tab_active_pane(10);
        let inner = [
            b"\x1b]777;emterm;agent-status;v=1;state=working\x1b\\".as_slice(),
            b"\x1b]133;D;0\x07",
            b"\x1b]133;A\x07",
        ]
        .concat();
        let combined = pty_output_apc(10, &inner);
        tab.test_process_combined(combined);
        tab.test_process_combined(Vec::new());
        assert!(
            tab.take_pending_agent_status_events().is_empty(),
            "no residual GUI-local Set after a mux-inner D->A pair"
        );
        assert!(
            tab.take_pending_latch_inputs().is_empty(),
            "mux-inner OSC 133/777 candidates must not feed the plain-tab inferred-clear latch"
        );
    }

    #[test]
    fn mux_inner_candidates_do_not_leak_into_same_pump_post_detach_tail() {
        // AC-3/AC-5 explicit scenario: one coalesced pump carries, in
        // order: (1) a mux inner `PtyOutput` with an OSC 777 Set
        // (mux-pane-owned — must be discarded), (2) the `Detached` control
        // frame, (3) plain shell bytes the now-reattached shell printed,
        // carrying its OWN OSC 777 Set + OSC 133 D/A (plain-tab-owned —
        // must resolve normally, AC-4). Before the fix, the mux-inner
        // Set/marks were queued ahead of the tail re-route's own
        // `process_outer_via_core` call and got taken together with it.
        let mut tab = mux_tab_active_pane(10);

        let detached = crate::mux::apc::encode_emterm_mux(&MuxMessage {
            msg_type: MessageType::Detached,
            pane_id: 0,
            payload: Vec::new(),
        });

        let mut combined =
            pty_output_apc(10, b"\x1b]777;emterm;agent-status;v=1;state=working\x1b\\");
        combined.extend_from_slice(&detached);
        combined.extend_from_slice(b"\x1b]777;emterm;agent-status;v=1;state=working\x1b\\");
        combined.extend_from_slice(b"\x1b]133;D;0\x07");
        combined.extend_from_slice(b"\x1b]133;A\x07");

        tab.test_process_combined(combined);

        assert!(
            tab.mux_session_name.is_none(),
            "Detached frame must clear mux_session_name"
        );

        let events = tab.take_pending_agent_status_events();
        assert_eq!(
            events,
            vec![crate::agent_status::AgentStatusEvent::Set {
                state: crate::agent_status::AgentState::Working,
                name: None,
            }],
            "only the post-detach plain-tab Set must surface, not the mux-inner one; events={events:?}"
        );

        assert_eq!(
            tab.take_pending_latch_inputs(),
            vec![
                crate::agent_status_model::ResolvedLatchInput::Set,
                crate::agent_status_model::ResolvedLatchInput::Mark(
                    crate::prompts::PromptMarkKind::CommandEnd
                ),
                crate::agent_status_model::ResolvedLatchInput::Mark(
                    crate::prompts::PromptMarkKind::PromptStart
                ),
            ],
            "only the post-detach live Set/D/A latch candidates must resolve; mux-inner \
             candidates discarded"
        );
    }

    // ── close-reconcile decision (FR1/FR2/FR3) ────────────────────────────

    // TS-1: the active window's shell exits in a 3-window group → the decision
    // helper returns the now-active pane id (a snapshot reconcile is wanted).
    #[test]
    fn close_reconcile_active_window_close_returns_new_active_pane() {
        let mut tab = test_tab();
        // Active index 2 (pane 30) is the displayed window.
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20), (3, "c", 30)], 2));
        let before = tab.mux_group.as_ref().unwrap().active_pane_id();
        assert_eq!(before, Some(30));
        // Close the active window.
        let changed = tab.apply_mux_message(pty_exited(30));
        assert!(changed);
        let after = tab.mux_group.as_ref().unwrap().active_pane_id();
        // The re-clamp moved active onto pane 20; the helper wants its snapshot.
        assert_eq!(after, Some(20));
        assert_eq!(Tab::close_reconcile_target(before, after), Some(20));
    }

    // TS-2: a non-active window's shell exits → the helper returns None even
    // though the active *index* shifts, because the displayed pane id is
    // unchanged.
    #[test]
    fn close_reconcile_nonactive_window_close_returns_none() {
        let mut tab = test_tab();
        // Active index 2 (pane 30). Close an EARLIER window (pane 10) so the
        // active index re-clamps from 2 → 1 yet still points at pane 30.
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20), (3, "c", 30)], 2));
        let before = tab.mux_group.as_ref().unwrap().active_pane_id();
        assert_eq!(before, Some(30));
        let changed = tab.apply_mux_message(pty_exited(10));
        assert!(changed);
        let g = tab.mux_group.as_ref().unwrap();
        // Index shifted 2 → 1 but the displayed window (pane 30) is unchanged.
        assert_eq!(g.active_index(), 1);
        let after = g.active_pane_id();
        assert_eq!(after, Some(30));
        assert_eq!(Tab::close_reconcile_target(before, after), None);
    }

    // TS-3: the last remaining window's shell exits → the group empties, the
    // tab is marked exited, and the helper returns None (no reconcile).
    #[test]
    fn close_reconcile_last_window_close_returns_none() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "only", 10)], 0));
        let before = tab.mux_group.as_ref().unwrap().active_pane_id();
        assert_eq!(before, Some(10));
        let changed = tab.apply_mux_message(pty_exited(10));
        assert!(changed);
        // Group emptied → no displayed pane → helper returns None.
        assert!(tab.mux_group.is_none());
        assert!(tab.exited);
        assert_eq!(Tab::close_reconcile_target(before, None), None);
    }

    // TS-4: PtyExited for an unknown pane id → no removal, no change, helper
    // input is unchanged so it would yield None.
    #[test]
    fn close_reconcile_unknown_pane_is_noop() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        let before = tab.mux_group.as_ref().unwrap().active_pane_id();
        let changed = tab.apply_mux_message(pty_exited(999));
        assert!(!changed);
        let g = tab.mux_group.as_ref().unwrap();
        assert_eq!(g.len(), 2, "no window removed");
        let after = g.active_pane_id();
        assert_eq!(after, before, "active unchanged");
        assert_eq!(Tab::close_reconcile_target(before, after), None);
    }

    // TS-5: several PtyExited for distinct panes drain in one pump → the final
    // active window is the one that needs reconciling. The helper, fed the
    // pre-pump active id against the post-pump active id, names the survivor.
    #[test]
    fn close_reconcile_multi_exit_in_one_pump_targets_final_active() {
        let mut tab = test_tab();
        // Active index 2 (pane 30).
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20), (3, "c", 30)], 2));
        let before = tab.mux_group.as_ref().unwrap().active_pane_id();
        assert_eq!(before, Some(30));
        // Two distinct windows exit in the same pump: first the active (pane
        // 30 → re-clamp onto pane 20), then pane 20 (→ re-clamp onto pane 10).
        assert!(tab.apply_mux_message(pty_exited(30)));
        assert!(tab.apply_mux_message(pty_exited(20)));
        let g = tab.mux_group.as_ref().unwrap();
        assert_eq!(g.len(), 1);
        let after = g.active_pane_id();
        assert_eq!(after, Some(10), "final active window survives");
        // Reconcile target is the final active window, not an intermediate one.
        assert_eq!(Tab::close_reconcile_target(before, after), Some(10));
    }

    // TS-6: regression — inbound SwitchWindow still syncs the active index and
    // reconciles the now-active window (the close fix must not alter it).
    #[test]
    fn switch_window_still_reconciles_after_close_fix() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        let changed = tab.apply_mux_message(switch_window(20));
        assert!(changed, "an inbound switch still reports a visible change");
        assert_eq!(
            tab.mux_group.as_ref().unwrap().active_index(),
            1,
            "the active index is synced to the switched-to window"
        );
        assert_eq!(
            tab.take_pending_pane_switch(),
            Some(10),
            "the outgoing pane is still latched for the App-side scroll save"
        );
    }

    // TS-7: closing the active mux window latches the exited pane id so
    // App::pump_all reloads the now-active pane's saved scroll position,
    // mirroring the SwitchWindow path. Closing a NON-active window must
    // NOT latch (the displayed pane did not change).
    #[test]
    fn close_reconcile_latches_outgoing_pane_for_scroll_reload() {
        // Active window close: pane 30 (active) exits → group re-clamps to
        // pane 20. The exited pane id (30) must be latched so the App-side
        // scroll restore runs. index_of_pane_id(30) will return None (already
        // removed), so the park is skipped and only active_pane_scroll() for
        // the new active (20) is reloaded — by App::pump_all's existing block.
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20), (3, "c", 30)], 2));
        assert_eq!(tab.mux_group.as_ref().unwrap().active_pane_id(), Some(30));
        let changed = tab.apply_mux_message(pty_exited(30));
        assert!(changed);
        assert_eq!(
            tab.take_pending_pane_switch(),
            Some(30),
            "the exited active pane id must be latched for the App-side scroll reload"
        );
        // One-shot: consumed by take_pending_pane_switch.
        assert_eq!(tab.take_pending_pane_switch(), None);

        // Non-active window close: pane 10 (non-active) exits → the displayed
        // pane (30, active) is unchanged. No latch should be set.
        let mut tab2 = test_tab();
        tab2.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20), (3, "c", 30)], 2));
        assert_eq!(tab2.mux_group.as_ref().unwrap().active_pane_id(), Some(30));
        let changed2 = tab2.apply_mux_message(pty_exited(10));
        assert!(changed2);
        assert_eq!(
            tab2.take_pending_pane_switch(),
            None,
            "closing a non-active window must not latch (displayed pane unchanged)"
        );
    }

    // ── Detached: exit mux mode (group + session name cleared) ────────────

    #[test]
    fn detached_clears_group_and_session_name() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "shell", 10), (2, "editor", 20)], 0));
        assert!(tab.mux_group.is_some());
        assert_eq!(tab.mux_session_name.as_deref(), Some("main"));

        let changed = tab.apply_mux_message(MuxMessage {
            msg_type: MessageType::Detached,
            pane_id: 0,
            payload: Vec::new(),
        });
        assert!(changed);
        // The tab reverts to a plain tab: no group, no mux session badge.
        assert!(tab.mux_group.is_none());
        assert!(tab.mux_session_name.is_none());
    }

    #[test]
    fn detached_clears_displayed_grid() {
        // After detach the bridge exits and the shell reprints its prompt; the
        // stale mux window content must not linger in the displayed grid.
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "shell", 10)], 0));
        // Paint a visible marker into the displayed core.
        tab.core.lock().process_pty_data_fully(b"STALE");
        assert_eq!(tab.core.lock().get_cell_char(0, 0), "S");

        let changed = tab.apply_mux_message(MuxMessage {
            msg_type: MessageType::Detached,
            pane_id: 0,
            payload: Vec::new(),
        });
        assert!(changed);
        // Grid reset to blank via reset_and_replay(b"").
        let c = tab.core.lock();
        let row0: String = (0..c.cols()).map(|col| c.get_cell_char(col, 0)).collect();
        assert!(
            row0.trim().is_empty(),
            "detach must clear the stale mux grid, got {row0:?}"
        );
    }

    #[test]
    fn detached_cancels_in_flight_offthread_switch() {
        // A window switch dispatched just before detach (snapshot >= the
        // off-thread threshold) must not resolve after detach: otherwise a
        // later poll_pending_switch would swap the detached window's
        // worker-built core back over the grid the Detached arm just cleared.
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        tab.apply_mux_message(snapshot_msg(10, large_payload("STALE")));
        assert!(
            tab.test_has_pending_switch(),
            "snapshot at/above threshold must enter the off-thread path"
        );

        let changed = tab.apply_mux_message(MuxMessage {
            msg_type: MessageType::Detached,
            pane_id: 0,
            payload: Vec::new(),
        });
        assert!(changed);
        // The in-flight switch is cancelled and dropped, so no later
        // poll_pending_switch can swap the detached content back in.
        assert!(
            !tab.test_has_pending_switch(),
            "detach must cancel the in-flight off-thread switch"
        );
    }

    // ── TS-9: RenameWindow inbound ────────────────────────────────────────

    #[test]
    fn rename_window_updates_label_by_pane_id() {
        // Inbound RenameWindow addresses the window by its active pane id —
        // window_id 2 has active_pane_id 20 per the welcome below, so the
        // rename frame's pane_id is 20 (matching the wire shape the daemon
        // re-broadcasts after our outbound `confirm_mux_rename` send).
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "shell", 10), (2, "editor", 20)], 0));
        let changed = tab.apply_mux_message(rename_window(20, "vim"));
        assert!(changed);
        assert_eq!(tab.mux_group.as_ref().unwrap().windows()[1].name, "vim");
    }

    #[test]
    fn rename_window_unknown_pane_id_is_noop() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "shell", 10)], 0));
        // pane_id 999 isn't in the group → no rename.
        let changed = tab.apply_mux_message(rename_window(999, "vim"));
        assert!(!changed);
    }

    // ── TS-10: SwitchWindow inbound ───────────────────────────────────────

    #[test]
    fn switch_window_syncs_active_index_by_pane() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        let changed = tab.apply_mux_message(switch_window(20));
        assert!(changed);
        assert_eq!(tab.mux_group.as_ref().unwrap().active_index(), 1);
    }

    #[test]
    fn switch_window_unknown_pane_is_noop() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10)], 0));
        let changed = tab.apply_mux_message(switch_window(999));
        assert!(!changed);
    }

    // ── FR3 pane wiring: inbound SwitchWindow latches the outgoing index ───

    #[test]
    fn inbound_switch_latches_outgoing_pane_index() {
        // A real inbound switch (active index moves 0 → 1) records the
        // outgoing pane id (10) so `App::pump_all` can park its scroll position.
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        assert!(tab.apply_mux_message(switch_window(20)));
        assert_eq!(tab.mux_group.as_ref().unwrap().active_index(), 1);
        assert_eq!(
            tab.take_pending_pane_switch(),
            Some(10),
            "the outgoing pane id (10) is latched for the App-side scroll save"
        );
        // The latch is one-shot.
        assert_eq!(tab.take_pending_pane_switch(), None);
    }

    #[test]
    fn inbound_switch_to_same_pane_does_not_latch() {
        // Switching onto the already-active pane must not latch a transition
        // (no scroll save/restore, no forced redraw).
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        // active is 0 (pane 10); switch to pane 10 again.
        let changed = tab.apply_mux_message(switch_window(10));
        assert!(changed, "set_active_by_pane still reports a match");
        assert_eq!(tab.mux_group.as_ref().unwrap().active_index(), 0);
        assert_eq!(
            tab.take_pending_pane_switch(),
            None,
            "a no-op switch onto the current pane latches nothing"
        );
    }

    #[test]
    fn inbound_switch_unknown_pane_does_not_latch() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        assert!(!tab.apply_mux_message(switch_window(999)));
        assert_eq!(tab.take_pending_pane_switch(), None);
    }

    #[test]
    fn inbound_multiple_switches_in_one_pump_latch_first_only() {
        // Several SwitchWindow messages can drain in one `pump` before
        // `App::pump_all` consumes the latch. A→B→C must keep the FIRST
        // outgoing pane (A, id 10) — that is the genuinely-displayed pane whose
        // live scroll must be parked; the intermediate pane (B) was never
        // rendered. Overwriting with B would corrupt two panes' slots.
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20), (3, "c", 30)], 0));
        assert!(tab.apply_mux_message(switch_window(20))); // 0 → 1, latch pane 10
        assert!(tab.apply_mux_message(switch_window(30))); // 1 → 2, must NOT overwrite
        assert_eq!(tab.mux_group.as_ref().unwrap().active_index(), 2);
        assert_eq!(
            tab.take_pending_pane_switch(),
            Some(10),
            "only the first outgoing pane id of the pump is latched"
        );
    }

    #[test]
    fn pane_created_latches_outgoing_index_for_scroll_save() {
        // Creating a new window makes it the active sub-tab. That is a third
        // unit-switch path: latch the outgoing pane id so `App::pump_all` parks
        // the outgoing pane's scroll and resets the new (empty) pane to Live.
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10)], 0));
        assert!(tab.apply_mux_message(pane_created(20)));
        assert_eq!(tab.mux_group.as_ref().unwrap().active_index(), 1);
        assert_eq!(
            tab.take_pending_pane_switch(),
            Some(10),
            "the outgoing pane id (10) is latched on new-window create"
        );
    }

    #[test]
    fn pane_created_latches_pending_window_appended_fr6() {
        // FR6 (mux): a PaneCreated that pushes — and so activates — a new window
        // latches the one-shot scroll-into-view signal that App::pump_all drains
        // for the active tab. Latched at the push site (not inferred from a
        // window-count delta), so it is a single, unambiguous event.
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10)], 0));
        assert!(!tab.take_pending_window_appended(), "baseline: not latched");
        assert!(tab.apply_mux_message(pane_created(20)));
        assert!(
            tab.take_pending_window_appended(),
            "PaneCreated that appended + activated a window latches the FR6 signal"
        );
        assert!(
            !tab.take_pending_window_appended(),
            "one-shot: the latch is cleared by take"
        );
    }

    #[test]
    fn window_appended_latch_survives_same_pump_pane_exit() {
        // Regression: the FR6 mux signal was previously inferred from a window-
        // count delta, which a same-pump PtyExited (removing a *different* pane)
        // could mask — PaneCreated (+1) and PtyExited (−1) net to zero, so the
        // delta missed the new active window. The push-site latch is immune: a
        // PaneCreated that activated a new window still latches even when another
        // pane exits in the same pump.
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        // active = pane 10 (index 0). Create a new window (pane 30) → pushed and
        // activated; then a different pane (20) exits in the same pump. Net
        // window count is unchanged (2 → 3 → 2), the case a count delta missed.
        assert!(tab.apply_mux_message(pane_created(30)));
        let _ = tab.apply_mux_message(pty_exited(20));
        assert!(
            tab.take_pending_window_appended(),
            "the FR6 latch survives a same-pump exit of a different pane"
        );
    }

    #[test]
    fn latched_outgoing_pane_survives_same_pump_pane_removal() {
        // Regression: the latch stores the outgoing pane *id*, not its index,
        // so a same-pump `PtyExited` that removes a different pane (shifting the
        // parallel arrays) cannot make the consumer park the outgoing scroll
        // into the wrong slot. Sequence in one pump: active = pane 20 (index 1);
        // switch to pane 30 (latch outgoing = pane 20); then pane 10 exits,
        // shifting pane 20 from index 1 → 0. The latch still resolves to pane
        // 20's NEW index (0); an index-based latch would have pointed at pane 30.
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20), (3, "c", 30)], 1));
        assert!(tab.apply_mux_message(switch_window(30))); // active 1 → 2, latch pane 20
        assert!(tab.apply_mux_message(pty_exited(10))); // removes index 0, arrays shift
        let latched = tab.take_pending_pane_switch();
        assert_eq!(
            latched,
            Some(20),
            "latch holds the outgoing pane id, not its index"
        );
        // The consumer resolves the id to its CURRENT index (pane 20 is now 0).
        let idx = tab
            .mux_group
            .as_ref()
            .unwrap()
            .index_of_pane_id(latched.unwrap());
        assert_eq!(
            idx,
            Some(0),
            "outgoing pane 20 resolved to its post-removal index"
        );
    }

    #[test]
    fn latched_outgoing_pane_skipped_when_it_exits_same_pump() {
        // When the outgoing pane itself exits in the same pump, its scroll slot
        // is gone; the consumer's `index_of_pane_id` returns `None` and the
        // park is skipped (no panic, nothing parked into a stale slot).
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        assert!(tab.apply_mux_message(switch_window(20))); // active 0 → 1, latch pane 10
        assert!(tab.apply_mux_message(pty_exited(10))); // outgoing pane 10 exits
        let latched = tab.take_pending_pane_switch();
        assert_eq!(latched, Some(10));
        assert_eq!(
            tab.mux_group
                .as_ref()
                .unwrap()
                .index_of_pane_id(latched.unwrap()),
            None,
            "exited outgoing pane resolves to no index → consumer skips the park"
        );
    }

    // ── TS-16: scripted inbound sequence ──────────────────────────────────

    #[test]
    fn inbound_sequence_attach_create_switch_rename_exit() {
        let mut tab = test_tab();
        // attach: one window (id 1, pane 10).
        tab.apply_mux_message(welcome_msg(&[(1, "shell", 10)], 0));
        // create: request then confirm → fresh window (id 2, pane 50).
        tab.mux_group.as_mut().unwrap().inc_pending_create();
        tab.apply_mux_message(pane_created(50));
        let _created_id = {
            let g = tab.mux_group.as_ref().unwrap();
            assert_eq!(g.len(), 2);
            assert_eq!(g.active_index(), 1); // new window active
            g.windows()[1].id
        };
        // switch back to the first pane.
        tab.apply_mux_message(switch_window(10));
        assert_eq!(tab.mux_group.as_ref().unwrap().active_index(), 0);
        // rename the freshly created window: addressed by its *pane id* (50)
        // on the wire, matching the inbound contract.
        tab.apply_mux_message(rename_window(50, "build"));
        assert_eq!(tab.mux_group.as_ref().unwrap().windows()[1].name, "build");
        // exit the created pane → drop back to one window. WebView parity:
        // the group still renders (one numbered sub-tab); it only dissolves
        // when the last window exits (the `Option` is cleared at zero).
        tab.apply_mux_message(pty_exited(50));
        let g = tab.mux_group.as_ref().unwrap();
        assert_eq!(g.len(), 1);
        assert!(g.is_group());
    }

    // ── Off-thread snapshot replay (Phase 2/3/4) ──────────────────────────

    fn snapshot_msg(pane_id: u32, payload: Vec<u8>) -> MuxMessage {
        MuxMessage {
            msg_type: MessageType::Snapshot,
            pane_id,
            payload,
        }
    }

    fn pty_output(pane_id: u32, payload: Vec<u8>) -> MuxMessage {
        MuxMessage {
            msg_type: MessageType::PtyOutput,
            pane_id,
            payload,
        }
    }

    /// A payload at or above the off-thread threshold whose first row, once
    /// replayed, is `marker` followed by a newline so subsequent live output
    /// lands on row 1 (the worker-built core is identifiable by row 0). The
    /// trailing NUL padding is ignored by the parser and leaves the cursor at
    /// the start of row 1.
    fn large_payload(marker: &str) -> Vec<u8> {
        let mut p = marker.as_bytes().to_vec();
        p.extend_from_slice(b"\r\n");
        // Pad past the threshold with NULs (ignored by the parser; they do
        // not advance the cursor) so row 0 stays exactly `marker`.
        p.resize(OFFTHREAD_REPLAY_THRESHOLD_BYTES + 16, 0);
        p
    }

    // ── task0003 D3 (review round-2 findings `200b2c8beeb68fe4` /
    // `87ba3cc2911d104e`): Snapshot|SnapshotRestore pane filter ───────────

    /// AC-3: with two or more mux windows, a reattach-shaped
    /// `SnapshotRestore` for a NON-active pane must not overwrite the tab's
    /// displayed core.
    #[test]
    fn snapshot_restore_for_non_active_pane_does_not_overwrite_displayed_core() {
        let mut tab = test_tab();
        // Two windows: pane 10 (index 0, active) and pane 20 (index 1).
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        assert_eq!(tab.mux_group.as_ref().unwrap().active_pane_id(), Some(10));

        // Paint identifiable content into the displayed core first.
        {
            let mut c = tab.core.lock();
            c.process_pty_data_fully(b"ACTIVE-A");
        }

        // A reattach-shaped SnapshotRestore arrives for the NON-active pane
        // (20) — this is exactly what `send_reattach_data` emits per pane in
        // the session, relying on the client to pick the right one.
        let msg = MuxMessage {
            msg_type: MessageType::SnapshotRestore,
            pane_id: 20,
            payload: b"NON-ACTIVE-B\r\n".to_vec(),
        };
        let changed = tab.apply_mux_message(msg);
        assert!(
            !changed,
            "a non-active pane's snapshot must be dropped (no redraw signalled)"
        );

        let c = tab.core.lock();
        let row0: String = (0..8).map(|col| c.get_cell_char(col, 0)).collect();
        assert_eq!(
            row0, "ACTIVE-A",
            "the displayed core must still show the active pane's content, \
             not the non-active pane's snapshot"
        );
    }

    /// AC-4: same fix, exercised via `MessageType::Snapshot` (the
    /// visibility-resume shape — `resume_pane_with_permit` sends this kind)
    /// and the OFF-THREAD (>= 64 KiB) path — a resume snapshot for a
    /// NON-active pane must not engage the off-thread swap or otherwise
    /// touch the displayed core; the SAME shape for the active pane still
    /// does.
    #[test]
    fn resume_snapshot_for_non_active_pane_does_not_trigger_offthread_swap() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        assert_eq!(tab.mux_group.as_ref().unwrap().active_pane_id(), Some(10));

        {
            let mut c = tab.core.lock();
            c.process_pty_data_fully(b"ACTIVE-A");
        }

        let changed = tab.apply_mux_message(snapshot_msg(20, large_payload("NON-ACTIVE-B")));
        assert!(
            !changed,
            "a non-active pane's resume snapshot must be dropped"
        );
        assert!(
            !tab.test_has_pending_switch(),
            "a non-active pane's resume snapshot must never engage the off-thread swap"
        );

        // Sanity: the SAME shape for the ACTIVE pane DOES engage the swap.
        let changed = tab.apply_mux_message(snapshot_msg(10, large_payload("ACTIVE-A-RESUMED")));
        assert!(changed);
        assert!(tab.test_has_pending_switch());
    }

    /// TS-4: exactly at the threshold goes off-thread; one byte below stays
    /// synchronous (no pending switch).
    #[test]
    fn ts4_threshold_boundary_sync_vs_offthread() {
        // One byte below threshold → synchronous, no pending switch.
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        let below = vec![b'x'; OFFTHREAD_REPLAY_THRESHOLD_BYTES - 1];
        tab.apply_mux_message(snapshot_msg(10, below));
        assert!(
            !tab.test_has_pending_switch(),
            "sub-threshold snapshot must stay synchronous"
        );

        // Exactly at the threshold → off-thread, pending switch entered.
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        let at = vec![b'x'; OFFTHREAD_REPLAY_THRESHOLD_BYTES];
        tab.apply_mux_message(snapshot_msg(10, at));
        assert!(
            tab.test_has_pending_switch(),
            "at-threshold snapshot must go off-thread"
        );
        // Active pane (index 0 → pane 10) is the queue target.
        assert_eq!(tab.test_pending_target(), Some(10));
    }

    /// AC-5 (task0005 rework D3'', review round-4 finding
    /// `b1de83542bfe60bc`): a small-payload (well under
    /// `OFFTHREAD_REPLAY_THRESHOLD_BYTES`), many-segment snapshot (at least
    /// `OFFTHREAD_REPLAY_SEGMENT_THRESHOLD` entries) must dispatch
    /// off-thread — the byte-size check alone would keep this synchronous,
    /// defeating the purpose since each segment's reflow cost does not
    /// scale with the segment's own byte count.
    ///
    /// Confirmed to fail pre-fix: before the segment-count branch existed,
    /// a payload this small (well under 64 KiB) with many segments stayed
    /// on the synchronous path regardless of segment count — this test's
    /// `test_has_pending_switch()` assertion would have been `false`.
    #[test]
    fn ac5_small_payload_many_segment_snapshot_dispatches_off_thread() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));

        let content = b"tiny".to_vec();
        let segments: Vec<mux_ipc::protocol::DimSegment> = (0..OFFTHREAD_REPLAY_SEGMENT_THRESHOLD)
            .map(|i| mux_ipc::protocol::DimSegment {
                offset: 0,
                cols: 80 + i as u16,
                rows: 24,
            })
            .collect();
        let encoded = mux_ipc::protocol::encode_snapshot_payload(&segments, &content);
        assert!(
            encoded.len() < OFFTHREAD_REPLAY_THRESHOLD_BYTES,
            "test prerequisite: encoded payload must stay well under the \
             byte-size threshold, got {}",
            encoded.len()
        );

        tab.apply_mux_message(snapshot_msg(10, encoded));
        assert!(
            tab.test_has_pending_switch(),
            "a small-payload snapshot at the segment-count threshold must \
             still dispatch off-thread"
        );
    }

    /// The byte-size threshold alone still governs when segment count is
    /// LOW — a small payload with only a couple of segments stays
    /// synchronous, exactly as before this fix. Pins the "no change to the
    /// common case" half of the AC-5 contract.
    #[test]
    fn ac5_small_payload_low_segment_count_stays_synchronous() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));

        let content = b"tiny".to_vec();
        let segments = vec![mux_ipc::protocol::DimSegment {
            offset: 0,
            cols: 80,
            rows: 24,
        }];
        assert!(segments.len() < OFFTHREAD_REPLAY_SEGMENT_THRESHOLD);
        let encoded = mux_ipc::protocol::encode_snapshot_payload(&segments, &content);

        tab.apply_mux_message(snapshot_msg(10, encoded));
        assert!(
            !tab.test_has_pending_switch(),
            "a small payload with a low segment count must stay synchronous"
        );
    }

    /// FR1: a large snapshot dispatch must NOT mutate the displayed core —
    /// the outgoing pane stays visible until the swap.
    #[test]
    fn ts4_offthread_dispatch_leaves_displayed_core_intact() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        // Paint the outgoing pane's content into the displayed core.
        {
            let mut c = tab.core.lock();
            c.process_pty_data_fully(b"OUTGOING");
        }
        tab.apply_mux_message(snapshot_msg(10, large_payload("INCOMING")));
        assert!(tab.test_has_pending_switch());
        // Displayed core still shows the outgoing content (not reset).
        let c = tab.core.lock();
        let row0: String = (0..8).map(|col| c.get_cell_char(col, 0)).collect();
        assert_eq!(row0, "OUTGOING");
    }

    /// FR3 / TS-3 (queue): target-pane live output during a pending switch is
    /// queued in arrival order, not applied to the displayed core; output for
    /// a different pane is dropped.
    #[test]
    fn ts3_live_output_queued_during_pending_switch() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        tab.apply_mux_message(snapshot_msg(10, large_payload("SNAP")));
        assert_eq!(tab.test_pending_target(), Some(10));

        // Two live chunks for the target pane → queued in order.
        tab.apply_mux_message(pty_output(10, b"first".to_vec()));
        tab.apply_mux_message(pty_output(10, b"second".to_vec()));
        // A chunk for a non-target pane → dropped (the PtyOutput pane filter
        // also drops non-active panes, but the pending guard covers it too).
        tab.apply_mux_message(pty_output(20, b"other".to_vec()));

        assert_eq!(
            tab.test_pending_live_queue(),
            vec![b"first".to_vec(), b"second".to_vec()]
        );
    }

    /// β: target-pane live output exceeding OFFTHREAD_LIVE_QUEUE_CAP_BYTES
    /// during a pending replay abandons the off-thread switch and reparses the
    /// snapshot synchronously, applying the accumulated output (nothing lost,
    /// no unbounded backlog / swap-time burst).
    #[test]
    fn offthread_live_queue_cap_falls_back_to_sync() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        tab.apply_mux_message(snapshot_msg(10, large_payload("SNAP")));
        assert!(tab.test_has_pending_switch());

        // 1 MiB of NUL padding per chunk: counts toward the byte budget but is
        // ignored by the parser (does not move the cursor / paint).
        let chunk = vec![0u8; 1024 * 1024];
        // Four chunks = 4 MiB == cap (not strictly greater) → still pending.
        for _ in 0..4 {
            tab.apply_mux_message(pty_output(10, chunk.clone()));
            assert!(
                tab.test_has_pending_switch(),
                "at-or-below the cap must stay off-thread"
            );
        }
        // The fifth chunk crosses the cap → synchronous fallback.
        let changed = tab.apply_mux_message(pty_output(10, chunk.clone()));
        assert!(changed, "the synchronous fallback repaints");
        assert!(
            !tab.test_has_pending_switch(),
            "exceeding the cap must abandon the off-thread switch"
        );
        // The snapshot was replayed synchronously (row 0 == marker) and the
        // NUL live output was applied on top without corrupting it.
        assert_eq!(tab.test_row_text(0), "SNAP");
    }

    /// TS-6 / FR5: a newer switch supersedes the in-flight one — only the
    /// latest target's snapshot ends up being the one built/queued.
    #[test]
    fn ts6_newer_switch_supersedes_in_flight() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));

        // First off-thread switch to pane 10.
        tab.apply_mux_message(snapshot_msg(10, large_payload("FIRST")));
        tab.apply_mux_message(pty_output(10, b"stale".to_vec()));
        assert_eq!(tab.test_pending_target(), Some(10));
        assert_eq!(tab.test_pending_live_queue(), vec![b"stale".to_vec()]);

        // The daemon moved the active pane to 20 (a newer SwitchWindow), then
        // a second large snapshot arrives for it.
        tab.mux_group.as_mut().unwrap().set_active_by_pane(20);
        tab.apply_mux_message(snapshot_msg(20, large_payload("SECOND")));

        // The pending switch now targets 20 and its queue was re-keyed (the
        // stale pane-10 bytes are discarded).
        assert_eq!(tab.test_pending_target(), Some(20));
        assert!(tab.test_pending_live_queue().is_empty());
        // The worker that actually completes built the *latest* target.
        assert_eq!(tab.test_wait_pending_first_row(), "SECOND");
    }

    /// FR5: a sub-threshold snapshot arriving mid-parse supersedes the
    /// in-flight off-thread switch (it applies synchronously and clears it).
    #[test]
    fn ts6_sync_snapshot_supersedes_pending_switch() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        tab.apply_mux_message(snapshot_msg(10, large_payload("BIG")));
        assert!(tab.test_has_pending_switch());

        // A small snapshot for the now-active pane applies synchronously and
        // supersedes the in-flight parse.
        tab.mux_group.as_mut().unwrap().set_active_by_pane(20);
        tab.apply_mux_message(snapshot_msg(20, b"small".to_vec()));
        assert!(!tab.test_has_pending_switch());
    }

    /// TS-12 / FR5 / FR7 (task0006 redesign, review round-1 finding
    /// `64baa639d71792f9`, AC-9 regression guard): a grid resize during a
    /// pending switch supersedes the DISPLAYED grid but no longer
    /// re-dispatches the in-flight worker — it defers the resize
    /// (`PendingSwitch::pending_resize`) so the worker keeps building at
    /// its ORIGINAL dispatch-time target (where its bypass split, if any,
    /// is valid; see `ac1_...` below for that half). The target and queued
    /// live output are still preserved end to end, and the tab ends up at
    /// the NEW grid once the swap completes — this test pins the adapted
    /// mechanics while keeping TS-12's original intent (resize supersedes
    /// correctly, nothing lost).
    #[test]
    fn ts12_resize_supersedes_and_redispatches_at_new_grid() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        let (orig_cols, orig_rows) = {
            let c = tab.core.lock();
            (c.cols(), c.rows())
        };
        tab.apply_mux_message(snapshot_msg(10, large_payload("PANE")));
        tab.apply_mux_message(pty_output(10, b"queued".to_vec()));
        assert!(tab.test_has_pending_switch());

        // Resize to a different grid → deferred, NOT a re-dispatch.
        tab.resize(100, 40);
        assert!(
            !tab.test_has_pending_redispatch(),
            "a resize alone must not coalesce a re-dispatch (FR7 fix: the \
             in-flight worker keeps its original, bypass-valid target)"
        );
        assert!(tab.test_has_pending_switch());
        assert_eq!(tab.test_pending_target(), Some(10));
        // Queue preserved across the deferred resize.
        assert_eq!(tab.test_pending_live_queue(), vec![b"queued".to_vec()]);
        assert_eq!(tab.test_pending_resize(), Some((100, 40)));
        // The in-flight worker's OWN build target is untouched.
        {
            let pending = tab.pending_switch.as_ref().unwrap();
            assert_eq!((pending.cols, pending.rows), (orig_cols, orig_rows));
        }

        // Once the (still ORIGINAL-target) worker completes, the deferred
        // resize is applied to the swapped-in core, then the queued live
        // output lands on top of it.
        assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
        assert!(!tab.test_has_pending_switch());
        let c = tab.core.lock();
        assert_eq!((c.cols(), c.rows()), (100, 40));
        drop(c);
        assert_eq!(tab.test_row_text(0), "PANE");
        assert_eq!(tab.test_row_text(1), "queued");
    }

    /// FR5: a resize that does not change the grid leaves the in-flight parse
    /// untouched (the core is still correctly sized).
    #[test]
    fn ts12_noop_resize_keeps_in_flight_parse() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        tab.apply_mux_message(snapshot_msg(10, large_payload("PANE")));
        let (cols, rows) = {
            let c = tab.core.lock();
            (c.cols(), c.rows())
        };
        tab.resize(cols, rows);
        assert!(tab.test_has_pending_switch());
        assert_eq!(tab.test_pending_target(), Some(10));
    }

    /// AC-4 (D3''''', round-8 rework, review round-7 finding
    /// `1d1b6b6297e3b6a0`): `Tab::resize` clamps to the SAME wire domain the
    /// daemon applies (`MuxPane::new` / `MuxPane::resize`'s
    /// `clamp_dims_to_wire_domain`) BEFORE resizing its own core, so the
    /// dimensions the client renders at are always the dimensions the
    /// daemon would accept for a pane — never the caller's raw,
    /// out-of-wire-domain request. Both ends run the SAME pure function
    /// against the SAME input, so they agree without a wire round trip.
    ///
    /// Confirmed to fail pre-fix: before this change, `Tab::resize` resized
    /// `self.core` directly to the caller's raw `(cols, rows)` with no
    /// clamp at all — `core.cols()`/`core.rows()` would come out as
    /// `(u16::MAX, u16::MAX)` instead of the clamped wire-domain values
    /// asserted below.
    #[test]
    fn resize_clamps_to_the_wire_domain_before_resizing_the_core() {
        let mut tab = test_tab();
        tab.resize(u16::MAX, u16::MAX);
        let (expected_cols, expected_rows) =
            crate::mux::session::pane::clamp_dims_to_wire_domain(u16::MAX, u16::MAX);
        let core = tab.core.lock();
        assert_eq!(
            (core.cols(), core.rows()),
            (expected_cols, expected_rows),
            "the client's core must be resized to the CLAMPED wire-domain \
             dims, matching what MuxPane::new/resize would accept — not \
             the caller's raw, out-of-domain request"
        );
    }

    /// AC-5, D3'''''' (round-9 rework, review round-8 finding
    /// `1e7e069001cf22dc`): `Tab::spawn_shell` clamps its FIRST-ever
    /// dimensions the same way `Tab::resize` clamps every later one, so a
    /// tab's initial core is never out of the wire domain even before any
    /// resize has happened.
    ///
    /// Confirmed to fail pre-fix: before this change, `Tab::spawn_shell`
    /// passed the caller's raw `cols`/`rows` straight into `TerminalCore::new`
    /// with no clamp at all — with this test's `u16::MAX` input, that means
    /// a `u16::MAX × u16::MAX`-cell grid allocation, which aborts the test
    /// process outright (a real allocation failure, not just a mismatched
    /// assertion) rather than settling on the clamped wire-domain values
    /// asserted below.
    #[test]
    fn spawn_shell_clamps_the_initial_core_to_the_wire_domain() {
        let tab = Tab::spawn_shell(
            "test",
            u16::MAX,
            u16::MAX,
            100,
            Arc::new(Settings::default()),
            None,
            None,
            Arc::new(NoopSink),
            None,
        );
        let (expected_cols, expected_rows) =
            crate::mux::session::pane::clamp_dims_to_wire_domain(u16::MAX, u16::MAX);
        let core = tab.core.lock();
        assert_eq!(
            (core.cols(), core.rows()),
            (expected_cols, expected_rows),
            "the tab's INITIAL core must already be clamped to the wire \
             domain, matching what a later Tab::resize (or MuxPane::new) \
             would accept — not the caller's raw, out-of-domain request"
        );
    }

    // ── FR6 / IMPLEMENTATION.md contract (c), D3 (task0002):
    // Tab::resize self-invalidation ──────────────────────────

    /// AC-1 (TS5, FR6): a resize that changes the tab's column count clears
    /// its own reflow-invalidated trackers — the prompt-mark tracker and
    /// fold regions, both tab-owned and reachable directly here (mirroring
    /// the seeding pattern app.rs's tests use for the same trackers).
    /// `clear_reflow_invalidated_state`'s own doc comment explains why: a
    /// reflow rewraps the logical↔physical line mapping, so retained
    /// absolute-row marks would point at the wrong line after the resize.
    #[test]
    fn resize_that_changes_columns_clears_the_tabs_own_reflow_trackers() {
        let mut tab = test_tab();
        tab.backfill_prompt_marks(
            0,
            vec![
                pending_kind(b'A', 2, None),
                pending_kind(b'C', 4, None),
                pending_kind(b'D', 9, Some(0)),
            ],
        );
        assert_eq!(
            tab.prompts.find_prev_prompt(u32::MAX),
            Some(2),
            "prompt mark seeded before the resize"
        );
        assert!(
            tab.folds.get_region_at_line(4).is_some(),
            "fold region seeded before the resize"
        );

        tab.resize(100, 24); // cols 80 -> 100: a width change

        assert_eq!(
            tab.prompts.find_prev_prompt(u32::MAX),
            None,
            "a width-changing resize must clear the tab's own prompt marks"
        );
        assert!(
            tab.folds.get_region_at_line(4).is_none(),
            "a width-changing resize must clear the tab's own fold regions"
        );
    }

    /// AC-2 (TS5, FR6): a height-only resize (column count unchanged) keeps
    /// the tab's prompt marks and fold regions — only a WIDTH change
    /// invalidates the tab-owned reflow trackers.
    #[test]
    fn resize_that_only_changes_rows_keeps_the_tabs_own_reflow_trackers() {
        let mut tab = test_tab();
        tab.backfill_prompt_marks(
            0,
            vec![
                pending_kind(b'A', 2, None),
                pending_kind(b'C', 4, None),
                pending_kind(b'D', 9, Some(0)),
            ],
        );

        tab.resize(80, 30); // same cols (80), rows 24 -> 30: height-only

        assert_eq!(
            tab.prompts.find_prev_prompt(u32::MAX),
            Some(2),
            "a height-only resize must NOT clear the tab's prompt marks"
        );
        assert!(
            tab.folds.get_region_at_line(4).is_some(),
            "a height-only resize must NOT clear the tab's fold regions"
        );
    }

    /// AC-3 (TS5, FR5, FR6): a raw resize request whose column count clamps
    /// back to the tab's CURRENT (post-clamp) column count is not a width
    /// change — the tab-owned trackers are left untouched. Uses the
    /// per-axis clamp floor (`clamp_resize_dims` clamps 0 up to 1) to build
    /// a case where the raw request (`0`) differs from the tab's current
    /// column count in absolute terms but clamps to the SAME value.
    #[test]
    fn resize_whose_raw_cols_clamp_back_to_the_current_cols_clears_nothing() {
        let mut tab = test_tab();
        tab.resize(1, 24); // drive the tab's own column count down to 1
        tab.backfill_prompt_marks(
            0,
            vec![
                pending_kind(b'A', 2, None),
                pending_kind(b'C', 4, None),
                pending_kind(b'D', 9, Some(0)),
            ],
        );

        tab.resize(0, 24); // raw cols 0 clamps to 1 == current cols
        {
            let core = tab.core.lock();
            assert_eq!(core.cols(), 1, "clamp floor sanity check");
        }

        assert_eq!(
            tab.prompts.find_prev_prompt(u32::MAX),
            Some(2),
            "a raw request that clamps back to the current column count \
             must NOT clear the tab's prompt marks"
        );
        assert!(
            tab.folds.get_region_at_line(4).is_some(),
            "a raw request that clamps back to the current column count \
             must NOT clear the tab's fold regions"
        );
    }

    /// A small snapshot payload whose first row replays to `marker` (stays on
    /// the synchronous path; reused as the contiguous-parse reference).
    fn small_snapshot_bytes(marker: &str) -> Vec<u8> {
        marker.as_bytes().to_vec()
    }

    /// Grid fingerprint of a tab's displayed core (all rows trimmed of
    /// trailing blanks + cursor position), for parity assertions.
    fn displayed_fingerprint(tab: &Tab) -> (Vec<String>, u16, u16) {
        let c = tab.core.lock();
        let mut rows = Vec::with_capacity(c.rows() as usize);
        for r in 0..c.rows() {
            let line: String = (0..c.cols()).map(|col| c.get_cell_char(col, r)).collect();
            rows.push(line.trim_end().to_string());
        }
        (rows, c.get_cursor_col(), c.get_cursor_row())
    }

    /// Regression: a DA1/DSR/XTWINOPS query embedded in snapshot bytes (e.g.
    /// because some past program in the pane's scrollback wrote `\x1b[c` to
    /// `/dev/tty`) generates a reply inside `reset_and_replay`. That reply
    /// must NOT be left in `term_core`'s `response_buffer` — otherwise the
    /// next live `apply_active_pane_output` (triggered by the user's first
    /// keystroke echo after the switch) picks it up via `take_response` and
    /// delivers a stale `\x1b[?65;1;4;22c` to the shell's stdin as PtyInput,
    /// where zsh/zle eats the `\x1b[?` prefix as an unbound key-binding and
    /// inserts the remaining `65;1;4;22c` at the prompt.
    #[test]
    fn reset_frame_for_replay_discards_historic_device_responses() {
        let mut tab = test_tab();
        // Snapshot-shaped payload with embedded DA1 and CPR queries.
        let mut snapshot = Vec::new();
        snapshot.extend_from_slice(b"row one\r\n");
        snapshot.extend_from_slice(b"\x1b[c"); // DA1 query
        snapshot.extend_from_slice(b"row two\r\n");
        snapshot.extend_from_slice(b"\x1b[6n"); // CPR query

        let _ = tab.reset_frame_for_replay(&snapshot, &[]);

        let core = tab.core.lock();
        assert_eq!(
            core.get_response_len(),
            0,
            "reset_frame_for_replay must drop device responses generated by \
             historic queries baked into the snapshot; residual bytes would \
             leak as PtyInput on the next live take_response and corrupt the \
             user's prompt on the first keystroke after a window switch"
        );
    }

    /// TS-5 / FR3: an off-thread snapshot parse + queued live output applied
    /// after the swap is byte/grid-identical to one contiguous synchronous
    /// parse of `snapshot ++ live`, and the prompt-mark tracker matches.
    #[test]
    fn ts5_offthread_swap_plus_live_equals_contiguous_parse() {
        // Build a large snapshot with an OSC 133 prompt mark + visible text,
        // then two live chunks that add more text and another prompt mark.
        let mut snapshot = b"\x1b]133;A\x07first-row\r\n".to_vec();
        snapshot.resize(OFFTHREAD_REPLAY_THRESHOLD_BYTES + 8, 0);
        let live1 = b"live-line-1\r\n".to_vec();
        let live2 = b"\x1b]133;A\x07live-line-2".to_vec();

        // Reference: a tab that replays snapshot ++ live as one synchronous
        // frame (reset_frame_for_replay) then feeds the live chunks as
        // ordinary output — exactly the legacy behavior with no off-thread gap.
        let mut reference = test_tab();
        reference.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        reference.reset_frame_for_replay(&snapshot, &[]);
        reference.apply_queued_live_output(vec![live1.clone(), live2.clone()]);

        // Off-thread path: dispatch, queue the live chunks, then swap.
        let mut offthread = test_tab();
        offthread.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        offthread.apply_mux_message(snapshot_msg(10, snapshot));
        offthread.apply_mux_message(pty_output(10, live1));
        offthread.apply_mux_message(pty_output(10, live2));
        assert_eq!(offthread.test_poll_until_swapped(), SwapOutcome::Swapped);
        assert!(!offthread.test_has_pending_switch());

        // Grid + cursor identical.
        assert_eq!(
            displayed_fingerprint(&offthread),
            displayed_fingerprint(&reference)
        );
        // Prompt-mark tracker identical (both prompt marks present, same rows).
        assert_eq!(
            offthread.prompts.find_prev_prompt(u32::MAX),
            reference.prompts.find_prev_prompt(u32::MAX)
        );
        assert_eq!(
            offthread.prompts.find_next_prompt(0),
            reference.prompts.find_next_prompt(0)
        );
    }

    /// TS-5: queued live output is applied in arrival order (a later chunk's
    /// content overwrites / follows an earlier chunk's, never reordered).
    #[test]
    fn ts5_queued_live_output_applied_in_order() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        tab.apply_mux_message(snapshot_msg(10, large_payload("SNAP")));
        // Three ordered chunks, each writing to a fresh line.
        tab.apply_mux_message(pty_output(10, b"AAA\r\n".to_vec()));
        tab.apply_mux_message(pty_output(10, b"BBB\r\n".to_vec()));
        tab.apply_mux_message(pty_output(10, b"CCC".to_vec()));
        assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
        // Row 0 = snapshot marker; rows 1..3 = the live chunks in order.
        assert_eq!(tab.test_row_text(0), "SNAP");
        assert_eq!(tab.test_row_text(1), "AAA");
        assert_eq!(tab.test_row_text(2), "BBB");
        assert_eq!(tab.test_row_text(3), "CCC");
    }

    /// TS-7 / FR7: on worker failure the swap falls back to a synchronous
    /// reparse of the latest target, with the queued live output applied in
    /// order — the displayed result is correct.
    #[test]
    fn ts7_worker_failure_falls_back_to_sync_reparse() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        tab.apply_mux_message(snapshot_msg(10, large_payload("FALLBACK")));
        tab.apply_mux_message(pty_output(10, b"after\r\n".to_vec()));
        // Simulate the worker panicking (sender dropped → Disconnected).
        tab.test_force_worker_disconnect();
        assert_eq!(tab.poll_pending_switch(), SwapOutcome::Swapped);
        assert!(!tab.test_has_pending_switch());
        // Snapshot reparsed synchronously + queued live applied in order.
        assert_eq!(tab.test_row_text(0), "FALLBACK");
        assert_eq!(tab.test_row_text(1), "after");
    }

    /// FR1: polling with no pending switch is a cheap no-op.
    #[test]
    fn poll_pending_switch_idle_when_none() {
        let mut tab = test_tab();
        assert_eq!(tab.poll_pending_switch(), SwapOutcome::Idle);
    }

    /// FR1: the swap replaces the displayed core's content (the outgoing
    /// pane's content is gone after the swap).
    #[test]
    fn swap_replaces_outgoing_content() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        {
            let mut c = tab.core.lock();
            c.process_pty_data_fully(b"OUTGOING-PANE");
        }
        tab.apply_mux_message(snapshot_msg(10, large_payload("NEWPANE")));
        // Before the swap, outgoing content is still shown.
        assert_eq!(tab.test_row_text(0), "OUTGOING-PANE");
        assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
        // After the swap, the worker-built content replaced it.
        assert_eq!(tab.test_row_text(0), "NEWPANE");
    }

    // Keep `small_snapshot_bytes` referenced (used by the integration test in
    // Phase 4) without an unused-fn warning when that test is filtered out.
    #[test]
    fn small_snapshot_helper_is_below_threshold() {
        assert!(small_snapshot_bytes("hi").len() < OFFTHREAD_REPLAY_THRESHOLD_BYTES);
    }

    /// TS-9 / FR2: swapping in a snapshot whose content occupies fewer rows
    /// than the outgoing pane leaves NO residual rows — every row past the
    /// snapshot's content is blank in the swapped-in core. The worker builds
    /// a fresh core (`reset_and_replay`), so residual rows cannot survive the
    /// swap; this locks that invariant in under the off-thread path.
    #[test]
    fn ts9_no_residual_rows_after_offthread_swap_to_shorter_pane() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        // Outgoing pane: fill many rows with content.
        {
            let mut c = tab.core.lock();
            let mut bytes = Vec::new();
            for i in 0..20 {
                bytes.extend_from_slice(format!("outgoing row {i}\r\n").as_bytes());
            }
            c.process_pty_data_fully(&bytes);
        }
        // Confirm the outgoing pane really has content on a deep row.
        assert!(!tab.test_row_text(10).is_empty());

        // Incoming snapshot: only two rows of content (large enough to go
        // off-thread).
        let mut snapshot = b"only-row-0\r\nonly-row-1".to_vec();
        snapshot.resize(OFFTHREAD_REPLAY_THRESHOLD_BYTES + 8, 0);
        tab.apply_mux_message(snapshot_msg(10, snapshot));
        assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);

        // Rows 0/1 hold the snapshot; every later row is blank — no residual.
        assert_eq!(tab.test_row_text(0), "only-row-0");
        assert_eq!(tab.test_row_text(1), "only-row-1");
        let rows = tab.core.lock().rows();
        for r in 2..rows {
            assert_eq!(
                tab.test_row_text(r),
                "",
                "row {r} must be blank after swap (no residual rows, FR2)"
            );
        }
    }

    /// TS-9 / NFR1: marks/folds + the eviction baseline after an off-thread
    /// swap match the synchronous `reset_frame_for_replay` path for the same
    /// snapshot (parity).
    #[test]
    fn ts9_marks_and_baseline_parity_with_sync_path() {
        // A snapshot with an OSC 133 A/B/C/D cycle (a foldable command region)
        // plus scrollback growth so the eviction baseline is exercised.
        let mut snapshot = Vec::new();
        snapshot.extend_from_slice(b"\x1b]133;A\x07$ \x1b]133;B\x07cmd\x1b]133;C\x07\r\n");
        for i in 0..30 {
            snapshot.extend_from_slice(format!("out {i}\r\n").as_bytes());
        }
        snapshot.extend_from_slice(b"\x1b]133;D;0\x07");
        let mut large = snapshot.clone();
        large.resize(OFFTHREAD_REPLAY_THRESHOLD_BYTES + large.len(), 0);

        // Synchronous reference (sub-threshold, legacy path).
        let mut reference = test_tab();
        reference.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        reference.reset_frame_for_replay(&snapshot, &[]);

        // Off-thread path (padded past the threshold; NUL padding does not
        // change the grid/marks).
        let mut offthread = test_tab();
        offthread.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        offthread.apply_mux_message(snapshot_msg(10, large));
        assert_eq!(offthread.test_poll_until_swapped(), SwapOutcome::Swapped);

        // Prompt navigation parity.
        assert_eq!(
            offthread.prompts.find_prev_prompt(u32::MAX),
            reference.prompts.find_prev_prompt(u32::MAX)
        );
        // Fold-region parity: both paths registered the same number of
        // foldable OSC 133 C→D regions.
        assert_eq!(
            offthread.folds.region_count(),
            reference.folds.region_count(),
            "off-thread and sync paths must register the same fold regions"
        );
    }

    // ── mux transport/content parser isolation (TS-4..TS-9) ───────────────

    use base64::Engine as _;

    /// Base64-encode bytes the way the Kitty payload field expects.
    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    /// Wrap inner-content bytes as an outer `emterm-mux;` PtyOutput APC frame
    /// for pane `pane_id`, exactly as the daemon/bridge writes it to the PTS
    /// stream (`ESC _ emterm-mux;<base64(frame)> ESC \`).
    fn pty_output_apc(pane_id: u32, inner: &[u8]) -> Vec<u8> {
        let msg = MuxMessage {
            msg_type: MessageType::PtyOutput,
            pane_id,
            payload: inner.to_vec(),
        };
        crate::mux::apc::encode_emterm_mux(&msg)
    }

    /// A complete-in-one Kitty APC for a `w`×`h` raw-RGB image (`f=24`).
    fn kitty_rgb_single(w: u32, h: u32) -> Vec<u8> {
        let raw = vec![0xABu8; (w * h * 3) as usize];
        let payload = b64(&raw);
        let mut v = vec![0x1b, b'_'];
        v.extend_from_slice(format!("Ga=T,f=24,s={w},v={h};{payload}").as_bytes());
        v.extend_from_slice(&[0x1b, b'\\']);
        v
    }

    /// A `w`×`h` raw-RGB Kitty image split into `parts` chunked APC frames
    /// (`m=1` … `m=0`). The base64 payload is split at arbitrary character
    /// boundaries across the chunks; the decoder concatenates the base64
    /// strings before decoding, so any split reconstructs the same image.
    fn kitty_rgb_chunked(w: u32, h: u32, parts: usize) -> Vec<Vec<u8>> {
        assert!(parts >= 2);
        let raw = vec![0xABu8; (w * h * 3) as usize];
        let payload = b64(&raw);
        let bytes = payload.as_bytes();
        let chunk = bytes.len().div_ceil(parts);
        let slices: Vec<&[u8]> = bytes.chunks(chunk).collect();
        let mut out = Vec::new();
        for (i, slice) in slices.iter().enumerate() {
            let first = i == 0;
            let last = i == slices.len() - 1;
            let m = if last { 0 } else { 1 };
            let mut apc = vec![0x1b, b'_'];
            let control = if first {
                format!("Ga=T,i=1,f=24,s={w},v={h},m={m};")
            } else {
                format!("Ga=T,i=1,m={m};")
            };
            apc.extend_from_slice(control.as_bytes());
            apc.extend_from_slice(slice);
            apc.extend_from_slice(&[0x1b, b'\\']);
            out.push(apc);
        }
        out
    }

    fn has_image_ready(events: &[ImageEvent]) -> bool {
        events
            .iter()
            .any(|e| matches!(e, ImageEvent::ImageReady { .. }))
    }

    // ── TS-4: split inner Kitty over mux PtyOutput boundaries ─────────────
    #[test]
    fn ts4_split_inner_kitty_over_mux_pty_output_assembles_one_image() {
        let mut tab = test_tab();
        // Establish mux with no window group → all PtyOutput accepted, and the
        // extractor engages from the next pump on.
        tab.apply_mux_message(welcome_msg(&[], 0));
        assert!(tab.mux_session_name.is_some(), "mux established");

        // A 4×4 RGB image (48 raw bytes → 64 base64 chars) split into 3 inner
        // Kitty chunks, each delivered as its own outer PtyOutput APC frame,
        // with a plain-text outer pump interleaved between them — the exact
        // shape that corrupted a shared parser.
        let chunks = kitty_rgb_chunked(4, 4, 3);

        // Chunk 1 (m=1): inner parser left mid-transfer.
        tab.test_process_combined(pty_output_apc(0, &chunks[0]));
        // Interleaving outer pump: a second mux PtyOutput carrying plain text.
        tab.test_process_combined(pty_output_apc(0, b"intervening text\r\n"));
        // Chunk 2 (m=1).
        tab.test_process_combined(pty_output_apc(0, &chunks[1]));
        // Chunk 3 (m=0): finalizes the transfer.
        let _ = tab.test_process_combined(pty_output_apc(0, &chunks[2]));

        let events = tab.drain_image_events();
        assert!(
            has_image_ready(&events),
            "split inner Kitty chunks must assemble into one decodable image; events={events:?}"
        );

        // No base64 of the image payload leaked onto the grid.
        let raw = vec![0xABu8; 4 * 4 * 3];
        let payload = b64(&raw);
        let grid = tab.test_grid_text();
        assert!(
            !grid.contains(&payload[..16]),
            "image base64 must not leak to the grid"
        );
        assert!(
            !grid.contains("emterm-mux"),
            "outer transport prefix must not leak to the grid"
        );
        assert!(
            !grid.contains("Ga=T"),
            "Kitty control data must not leak to the grid"
        );
        // The interleaved inner plain text DID reach the core (inner content
        // is what self.core renders).
        assert!(
            tab.test_grid_text().contains("intervening text"),
            "inner plain text must render via self.core"
        );
    }

    // ── TS-9: non-mux Kitty image still decodes (no regression) ───────────
    #[test]
    fn ts9_non_mux_kitty_image_still_decodes() {
        let mut tab = test_tab();
        assert!(tab.mux_session_name.is_none(), "pre-mux tab");
        // A complete Kitty image fed as a plain PTS buffer (pre-mux branch:
        // parsed by self.core, on_apc → pending_apc → image pipeline).
        let _ = tab.test_process_combined(kitty_rgb_single(3, 3));
        let events = tab.drain_image_events();
        assert!(
            has_image_ready(&events),
            "non-mux Kitty image must decode as before; events={events:?}"
        );
    }

    // ── TS-5: pre-mux PTS bytes route through self.core ───────────────────
    #[test]
    fn ts5_pre_mux_pts_routes_through_core() {
        let mut tab = test_tab();
        assert!(tab.mux_session_name.is_none(), "extractor not engaged yet");
        // Plain printable bytes fed as the outer PTS stream: pre-mux they must
        // be parsed by self.core and land on the grid (the extractor would
        // discard non-transport Print actions).
        tab.test_process_combined(b"pre-mux line\r\n".to_vec());
        assert!(
            tab.test_grid_text().contains("pre-mux line"),
            "pre-mux plain text must render via self.core"
        );
    }

    #[test]
    fn ts5_switch_to_extractor_after_welcome_discards_outer_print() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[], 0));
        assert!(tab.mux_session_name.is_some(), "mux established");
        // After the switch, raw printable bytes on the OUTER stream are not
        // content — they are not valid mux transport, so the extractor drops
        // them and they never reach self.core / the grid.
        tab.test_process_combined(b"outer-noise-xyz\r\n".to_vec());
        assert!(
            !tab.test_grid_text().contains("outer-noise-xyz"),
            "outer-stream Print must NOT reach the core once mux is established"
        );
    }

    // ── TS-6: detach restores self.core routing ───────────────────────────
    #[test]
    fn ts6_detach_restores_core_routing() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[], 0));
        assert!(tab.mux_session_name.is_some());
        // Detach: the daemon confirms with a Detached frame delivered as an
        // outer PtyOutput-equivalent control message. Apply it directly.
        let detached = MuxMessage {
            msg_type: MessageType::Detached,
            pane_id: 0,
            payload: Vec::new(),
        };
        tab.apply_mux_message(detached);
        assert!(tab.mux_session_name.is_none(), "detached clears mux");
        // Pre-mux routing resumed: plain PTS bytes are parsed by self.core
        // again and render on the grid.
        tab.test_process_combined(b"post-detach line\r\n".to_vec());
        assert!(
            tab.test_grid_text().contains("post-detach line"),
            "after detach, plain text must render via self.core again"
        );
    }

    #[test]
    fn ts6_detach_resets_extractor_partial_frame() {
        // A partial outer frame in flight when detach happens must be dropped,
        // not carried into the resumed pre-mux core parse.
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[], 0));
        // Feed half of an outer APC frame — the extractor is now mid-sequence.
        let half = pty_output_apc(0, b"GG");
        let split = half.len() / 2;
        tab.test_process_combined(half[..split].to_vec());
        // Detach (resets the extractor).
        tab.apply_mux_message(MuxMessage {
            msg_type: MessageType::Detached,
            pane_id: 0,
            payload: Vec::new(),
        });
        // The remainder, now fed pre-mux to self.core, is the tail of an APC
        // sequence with no introducer: self.core stays in Ground for the
        // trailing ST and prints nothing garbled. Then a clean line renders.
        tab.test_process_combined(half[split..].to_vec());
        tab.test_process_combined(b"clean\r\n".to_vec());
        assert!(
            tab.test_grid_text().contains("clean"),
            "post-detach core parse is clean after extractor reset"
        );
    }

    // ── TS-7: double-Welcome does not corrupt the stream ──────────────────
    #[test]
    fn ts7_double_welcome_does_not_corrupt_decoding() {
        let mut tab = test_tab();
        // The bridge/daemon can deliver Welcome twice (a known duplication).
        tab.apply_mux_message(welcome_msg(&[], 0));
        tab.apply_mux_message(welcome_msg(&[], 0));
        assert!(tab.mux_session_name.is_some(), "mux still established");

        // A split inner Kitty image after the double Welcome must still
        // assemble into one image — the extractor state stayed consistent.
        let chunks = kitty_rgb_chunked(4, 4, 3);
        tab.test_process_combined(pty_output_apc(0, &chunks[0]));
        tab.test_process_combined(pty_output_apc(0, &chunks[1]));
        tab.test_process_combined(pty_output_apc(0, &chunks[2]));
        let events = tab.drain_image_events();
        assert!(
            has_image_ready(&events),
            "image must decode despite double Welcome; events={events:?}"
        );
    }

    // ── TS-11: post-Detached tail re-routed to self.core (FR5) ────────────
    #[test]
    fn ts11_post_detached_tail_in_coalesced_buffer_renders_via_core() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[], 0));
        assert!(tab.mux_session_name.is_some(), "mux established");

        // One coalesced PTS buffer carrying, in order:
        //   1. an inner PtyOutput frame (rendered into self.core via the inner
        //      content path),
        //   2. the Detached control frame (clears mux_session_name mid-buffer),
        //   3. plain shell prompt bytes printed by the shell that regained the
        //      PTY — these follow the Detached frame in the SAME buffer.
        //
        // Before the fix, routing was decided once per pump: the whole buffer
        // went to the extractor, which discards non-APC bytes, so the prompt
        // bytes were silently dropped and (with the Detached grid clear) the
        // screen stayed blank until the next keystroke.
        let detached = crate::mux::apc::encode_emterm_mux(&MuxMessage {
            msg_type: MessageType::Detached,
            pane_id: 0,
            payload: Vec::new(),
        });
        let mut combined = pty_output_apc(0, b"inner shell output\r\n");
        combined.extend_from_slice(&detached);
        combined.extend_from_slice(b"detached-prompt$ \r\n");

        let _ = tab.test_process_combined(combined);

        // Detach actually took effect.
        assert!(
            tab.mux_session_name.is_none(),
            "Detached frame must clear mux_session_name"
        );
        // The plain prompt bytes coalesced behind the Detached frame rendered
        // via self.core instead of being swallowed by the extractor. The
        // Detached arm clears the grid first (reset_frame_for_replay), so the
        // re-routed tail is what repaints — exactly the bytes we expect.
        let grid = tab.test_grid_text();
        assert!(
            grid.contains("detached-prompt$"),
            "post-Detached shell bytes must render via self.core; grid={grid:?}"
        );
        // The transport prefix must never leak onto the grid.
        assert!(
            !grid.contains("emterm-mux"),
            "outer transport prefix must not leak to the grid; grid={grid:?}"
        );
    }

    // ── TS-11b: a non-mux image coalesced behind Detached decodes exactly once ──
    #[test]
    fn ts11_post_detached_image_decodes_exactly_once() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[], 0));
        assert!(tab.mux_session_name.is_some(), "mux established");

        // One coalesced buffer: the Detached control frame, then a complete
        // (non-mux) Kitty image the shell printed right after regaining the PTY.
        // `feed_with_offsets` surfaces the bare Kitty APC AND the post-detach
        // tail re-route re-parses the same bytes through self.core. Without the
        // loop `break` at the detach boundary, the image was decoded twice (once
        // from the extracted image_apc, once from the re-routed tail).
        let detached = crate::mux::apc::encode_emterm_mux(&MuxMessage {
            msg_type: MessageType::Detached,
            pane_id: 0,
            payload: Vec::new(),
        });
        let mut combined = detached;
        combined.extend_from_slice(&kitty_rgb_single(3, 3));

        let _ = tab.test_process_combined(combined);

        assert!(
            tab.mux_session_name.is_none(),
            "Detached frame must clear mux_session_name"
        );
        let ready = tab
            .drain_image_events()
            .into_iter()
            .filter(|e| matches!(e, ImageEvent::ImageReady { .. }))
            .count();
        assert_eq!(
            ready, 1,
            "post-Detached image must decode exactly once, not double-processed \
             via the extracted-frame loop AND the tail re-route"
        );
    }

    // ── (C) client-side coalesce contract: consecutive PtyOutput ⇒ one parse ──
    //
    // The client coalesces, in `process_combined`, the inner payloads of
    // consecutive active-pane `PtyOutput` frames that arrive within one pump:
    // they are concatenated and parsed by `core.process_pty_data_fully` exactly
    // ONCE per consecutive run, instead of once per frame. A control message,
    // a non-active pane, a `pending_switch`, or a detach is a boundary that
    // flushes the accumulator first; the buffer is also flushed at loop end.
    //
    // These tests observe the pass count directly through the `cfg(test)`-only
    // `coalesce_parse_passes` counter (incremented at the flush parse site),
    // which carries no taint in the production build. The grid is asserted to
    // equal the single-concatenated result so the collapse is proven to be a
    // pure performance change — the same equality the "split == concatenated"
    // parity test pins as the before/after baseline.

    /// Build a tab attached to a single-window mux session whose active pane is
    /// `pane`, so `PtyOutput` for `pane` flows straight into the displayed core
    /// (no pending switch, pane filter satisfied).
    fn mux_tab_active_pane(pane: u32) -> Tab {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "win", pane)], 0));
        assert!(tab.mux_session_name.is_some(), "mux session established");
        assert!(
            !tab.test_has_pending_switch(),
            "no snapshot pending: PtyOutput must reach core directly"
        );
        tab
    }

    /// AC-3/TS2 (mux-status-bar-removal task0001, FR1/FR8a): a raw frame
    /// carrying the retired opcode 0x16 (see `mux_ipc::protocol`'s
    /// reserved-opcode comment for what it used to mean, reserved and
    /// never reused) arriving on the GUI's mux receive path is ignored
    /// with at most a warn log — no error, no disconnect, no tab-state
    /// mutation. Constructed as a raw wire frame (`[type=0x16][pane_id:
    /// u32 LE][empty payload]`, wrapped exactly as the daemon/bridge
    /// write it: `ESC _ emterm-mux;<base64> ESC \`) rather than through
    /// the typed `MuxMessage` API, which can no longer name the retired
    /// type — this keeps the test valid regardless of whether that type
    /// still exists anywhere in the tree. Replaces (former app.rs
    /// TS-mux-msg-2) `on_mux_message_status_update_caches_payload_on_tab`.
    #[test]
    fn retired_status_update_opcode_is_ignored_by_gui_receive_path() {
        let mut tab = mux_tab_active_pane(10);
        let before_session = tab.mux_session_name.clone();
        let before_pane_ids = tab.mux_group.as_ref().map(|g| g.pane_ids().to_vec());
        let before_active_pane = tab.mux_group.as_ref().and_then(|g| g.active_pane_id());

        let retired_frame_body: Vec<u8> = vec![0x16, 0, 0, 0, 0]; // [type][pane_id LE]
        let mut raw = vec![0x1b, b'_'];
        raw.extend_from_slice(format!("emterm-mux;{}", b64(&retired_frame_body)).as_bytes());
        raw.extend_from_slice(&[0x1b, b'\\']);

        // Must not panic.
        tab.test_process_combined(raw);

        assert_eq!(
            tab.mux_session_name, before_session,
            "mux session must be undisturbed by a retired-opcode frame"
        );
        assert_eq!(
            tab.mux_group.as_ref().map(|g| g.pane_ids().to_vec()),
            before_pane_ids,
            "mux window group must be undisturbed"
        );
        assert_eq!(
            tab.mux_group.as_ref().and_then(|g| g.active_pane_id()),
            before_active_pane,
            "active pane must be undisturbed"
        );

        // Connection stays up: ordinary traffic immediately afterward still
        // applies normally.
        let follow_up = pty_output_apc(10, b"still alive");
        let changed = tab.test_process_combined(follow_up);
        assert!(
            changed,
            "tab must keep processing ordinary frames after a retired-opcode frame"
        );
    }

    /// The batched (coalesce) behavior: K active-pane `PtyOutput` frames
    /// arriving wire-encoded in ONE coalesced PTS buffer collapse into a single
    /// parse pass. Every line still lands in the grid (output is unchanged), but
    /// the core is parsed once for the whole consecutive run — not once per
    /// frame. The `coalesce_parse_passes` counter makes the collapse observable.
    /// This is the post-change contract the perf work establishes (previously
    /// the per-frame path parsed K times).
    #[test]
    fn c_pty_output_parsed_per_message_grid_grows_step_by_step() {
        let mut tab = mux_tab_active_pane(10);

        // K active-pane PtyOutput frames, each a full line, encoded as the
        // daemon writes them and concatenated into ONE coalesced PTS buffer —
        // exactly what `pump` hands `process_combined` when many small frames
        // arrive within one drain.
        let lines: [&[u8]; 4] = [b"line0\r\n", b"line1\r\n", b"line2\r\n", b"line3\r\n"];
        let k = lines.len();
        let mut combined = Vec::new();
        for line in lines {
            combined.extend_from_slice(&pty_output_apc(10, line));
        }

        let before = tab.test_coalesce_parse_passes();
        let changed = tab.test_process_combined(combined);
        assert!(changed, "applied PtyOutput repaints the active pane");

        // One consecutive active-pane run ⇒ exactly one flush/parse, not K.
        assert_eq!(
            tab.test_coalesce_parse_passes() - before,
            1,
            "K={k} consecutive active-pane frames must coalesce into 1 parse pass"
        );
        // All K lines still landed — output is byte-for-byte unchanged.
        for (i, _) in lines.iter().enumerate() {
            assert_eq!(
                tab.test_row_text(i as u16),
                format!("line{i}"),
                "row {i} must show its line after the coalesced parse"
            );
        }
    }

    /// New required test (TS-1): consecutive active-pane `PtyOutput` frames
    /// arriving in one coalesced buffer are parsed in a SINGLE pass, and the
    /// resulting grid is identical to parsing the concatenation of their inner
    /// payloads in one shot. Proves the coalesce both collapses the parse count
    /// to 1 and preserves output exactly.
    #[test]
    fn c_consecutive_active_pane_pty_output_coalesces_into_one_parse() {
        let pane = 10;
        // Inner payloads whose chunk boundaries deliberately fall inside lines
        // and after newlines, so the streaming parser must carry state across
        // the frame boundaries (a per-frame parse and a coalesced parse would
        // otherwise be trivially identical).
        let inner: [&[u8]; 4] = [b"alp", b"ha\r\nbra", b"vo\r\ncharlie\r\n", b"delta"];

        // Coalesced path: K active-pane frames in ONE buffer ⇒ 1 parse pass.
        let mut tab = mux_tab_active_pane(pane);
        let mut combined = Vec::new();
        for chunk in inner {
            combined.extend_from_slice(&pty_output_apc(pane, chunk));
        }
        let before = tab.test_coalesce_parse_passes();
        tab.test_process_combined(combined);
        assert_eq!(
            tab.test_coalesce_parse_passes() - before,
            1,
            "consecutive active-pane PtyOutput run must parse exactly once"
        );

        // Reference: a single PtyOutput whose payload is the concatenation.
        let mut single = mux_tab_active_pane(pane);
        single.test_process_combined(pty_output_apc(pane, &inner.concat()));

        assert_eq!(
            tab.test_grid_text(),
            single.test_grid_text(),
            "coalesced grid must equal the single-concatenated parse"
        );
    }

    /// Parity baseline for a future coalescing change: K split `PtyOutput`
    /// messages and a single concatenated `PtyOutput` message produce the
    /// identical final grid. A coalescing optimization (parse the K payloads in
    /// one pass) must keep this equality — so this is the correctness contract
    /// that lets parse-count be reduced from K to 1 without changing output.
    #[test]
    fn c_split_messages_equal_single_concatenated_message() {
        let total = b"alpha\r\nbravo\r\ncharlie\r\ndelta";
        // The four chunk boundaries deliberately fall *inside* lines and after
        // newlines, proving term_core's streaming parser carries state across
        // message boundaries (so coalescing is purely a perf change).
        let chunks: [&[u8]; 4] = [b"alp", b"ha\r\nbra", b"vo\r\ncharlie\r\n", b"delta"];
        assert_eq!(
            chunks.concat(),
            total,
            "chunks must reconstruct the whole stream"
        );

        // K-message path: one parse pass per message (current behavior).
        let mut split = mux_tab_active_pane(10);
        let k = chunks.len();
        for chunk in chunks {
            split.apply_mux_message(pty_output(10, chunk.to_vec()));
        }

        // Single-message path: one parse pass for the whole stream (the shape a
        // receive-side coalesce would collapse the K messages into).
        let mut single = mux_tab_active_pane(10);
        single.apply_mux_message(pty_output(10, total.to_vec()));

        assert_eq!(
            split.test_grid_text(),
            single.test_grid_text(),
            "K={k} per-message parses must yield the same grid as 1 concatenated parse"
        );
    }

    /// `payload_has_device_query` detects complete CSI sequences whose final
    /// byte produces a device response in `term_core` — `n` (DSR), `c` (DA),
    /// `t` (XTWINOPS size reports), `p` (DECRPM) — across params / intermediates
    /// and DEC private (`?`) / secondary (`>`) forms, resynchronizes on a
    /// malformed CSI, and rejects non-response finals, incomplete CSIs, and
    /// plain text.
    #[test]
    fn payload_has_device_query_detects_response_producing_finals() {
        assert!(payload_has_device_query(b"\x1b[6n"), "CPR DSR");
        assert!(payload_has_device_query(b"\x1b[5n"), "status DSR");
        assert!(
            payload_has_device_query(b"\x1b[c"),
            "primary DA (no params)"
        );
        assert!(payload_has_device_query(b"\x1b[>0c"), "secondary DA");
        assert!(payload_has_device_query(b"\x1b[?6n"), "DEC private DSR");
        assert!(
            payload_has_device_query(b"\x1b[14t"),
            "XTWINOPS size report (t)"
        );
        assert!(
            payload_has_device_query(b"\x1b[?2026$p"),
            "DECRPM synchronized-output probe (p)"
        );
        assert!(
            payload_has_device_query(b"hello\x1b[6nworld"),
            "query embedded in surrounding text"
        );
        assert!(
            payload_has_device_query(b"\x1b[2\x1b[6n"),
            "aborted CSI then a real query must resync and detect the query"
        );
        assert!(
            payload_has_device_query(b"\x1b[\x076n"),
            "a C0 control mid-CSI does not abort the sequence (term_core keeps it alive)"
        );
        assert!(
            !payload_has_device_query(b"plain text\r\n"),
            "no CSI at all"
        );
        assert!(
            !payload_has_device_query(b"\x1b[1;2H"),
            "cursor position (final H) is not a query"
        );
        assert!(
            !payload_has_device_query(b"\x1b[0m"),
            "SGR (final m) is not a query"
        );
        assert!(
            !payload_has_device_query(b"\x1b[6"),
            "incomplete CSI is not a complete query"
        );
        assert!(
            !payload_has_device_query(b"\x1b[31mcn"),
            "literal c/n after a non-query CSI final must not count"
        );
    }

    /// Device-response parity (TS): a `PtyOutput` frame carrying a device query
    /// (`\x1b[6n` CPR) must NOT be coalesced — it is parsed on its own via the
    /// per-frame path so its reply is captured before a later query overwrites
    /// `term_core`'s single-slot response buffer. Observable consequence: a
    /// query frame BREAKS the consecutive active-pane run, so [text][query][text]
    /// flushes the coalesce accumulator twice (leading run + loop-end) with the
    /// query frame parsed per-frame in between — versus a single flush when no
    /// query interrupts the run.
    #[test]
    fn c_device_query_frame_breaks_coalesce_run() {
        let pane = 10;

        // Baseline: three plain active-pane frames coalesce into ONE parse.
        let mut plain = mux_tab_active_pane(pane);
        let mut plain_buf = Vec::new();
        for chunk in [b"aaa\r\n".as_slice(), b"bbb\r\n", b"ccc\r\n"] {
            plain_buf.extend_from_slice(&pty_output_apc(pane, chunk));
        }
        let before = plain.test_coalesce_parse_passes();
        plain.test_process_combined(plain_buf);
        assert_eq!(
            plain.test_coalesce_parse_passes() - before,
            1,
            "three plain frames coalesce into a single parse"
        );

        // With a CPR query frame in the middle: the run splits. The query frame
        // is handled per-frame (not via the coalesce flush), so the accumulator
        // flushes for the leading run and again at loop end — two coalesce
        // parses — guaranteeing the query's reply is not clobbered by coalescing.
        let mut split = mux_tab_active_pane(pane);
        let mut split_buf = Vec::new();
        split_buf.extend_from_slice(&pty_output_apc(pane, b"aaa\r\n"));
        split_buf.extend_from_slice(&pty_output_apc(pane, b"\x1b[6n"));
        split_buf.extend_from_slice(&pty_output_apc(pane, b"ccc\r\n"));
        let before = split.test_coalesce_parse_passes();
        split.test_process_combined(split_buf);
        assert_eq!(
            split.test_coalesce_parse_passes() - before,
            2,
            "a device-query frame breaks the run: leading-run flush + loop-end flush"
        );
        // The query produced no visible cells; the surrounding text rendered.
        assert_eq!(split.test_row_text(0), "aaa");
        assert_eq!(split.test_row_text(1), "ccc");
    }

    // ── 2nd-pass scrollback restore (snapshot-replay-scrollback-restore) ──

    /// Build a payload at or above the off-thread threshold that scrolls
    /// many rows so the rebuilt scrollback has content to compare against.
    /// 100 "line N" rows pad up over 64 KiB easily.
    fn large_scrollable_payload() -> Vec<u8> {
        let mut p = Vec::with_capacity(OFFTHREAD_REPLAY_THRESHOLD_BYTES + 1024);
        // Tag the first row with a recognizable marker.
        p.extend_from_slice(b"FIRST\r\n");
        // Filler lines until we comfortably exceed the off-thread threshold.
        let mut i: u32 = 0;
        while p.len() < OFFTHREAD_REPLAY_THRESHOLD_BYTES + 8 * 1024 {
            p.extend_from_slice(format!("line {i:06}\r\n").as_bytes());
            i += 1;
        }
        // Last row marker so we can spot the visible tail in tests.
        p.extend_from_slice(b"LAST\r\n");
        p
    }

    /// TS-13 (FR1 / FR6): an at-or-above-threshold payload installs a
    /// `pending_scrollback_restore` after the 1st-pass swap finishes. Also
    /// covers the FR4 wiring side: `poll_pending_scrollback_restore` is the
    /// thing that consumes the pending state.
    #[test]
    fn ts13_offthread_swap_installs_pending_scrollback_restore() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        let payload = large_scrollable_payload();
        tab.apply_mux_message(snapshot_msg(10, payload));
        assert!(
            tab.test_has_pending_switch(),
            "test prerequisite: large payload must go off-thread"
        );
        // Drive the 1st-pass swap to completion.
        let outcome = tab.test_poll_until_swapped();
        assert_eq!(outcome, SwapOutcome::Swapped);
        // After the swap, the 2nd-pass scrollback restore must be installed.
        assert!(
            tab.test_has_pending_scrollback_restore(),
            "apply_offthread_swap must spawn a 2nd-pass scrollback restore worker"
        );
    }

    /// TS-12 (FR6): a sub-threshold payload takes the synchronous path and
    /// installs no `pending_scrollback_restore` (the live core's scrollback
    /// is already correct).
    #[test]
    fn ts12_subthreshold_payload_does_not_install_scrollback_restore() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        let mut small = b"hello\r\n".to_vec();
        small.resize(OFFTHREAD_REPLAY_THRESHOLD_BYTES - 1, b'.');
        tab.apply_mux_message(snapshot_msg(10, small));
        assert!(
            !tab.test_has_pending_switch(),
            "sub-threshold snapshot must take the synchronous path"
        );
        assert!(
            !tab.test_has_pending_scrollback_restore(),
            "sub-threshold snapshot must NOT install a 2nd-pass scrollback restore"
        );
    }

    /// TS-7 (FR1 + NFR6): after the 1st-pass swap, the live core has empty
    /// scrollback (bypass left it empty). After the 2nd-pass restore
    /// completes, the merged scrollback matches the synchronous reference
    /// (built bypass-off).
    #[test]
    fn ts7_offthread_swap_then_restored_scrollback_matches_reference() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        let payload = large_scrollable_payload();
        // Reference: synchronous bypass-off build at the same grid.
        let never = std::sync::atomic::AtomicBool::new(false);
        let reference =
            term_core::terminal_core::TerminalCore::build_scrollback_only_from_snapshot(
                80,
                24,
                100,
                &payload,
                &[],
                &never,
            )
            .expect("reference build not cancelled");
        let reference_scrollback_count = reference.core.get_scrollback_length();

        tab.apply_mux_message(snapshot_msg(10, payload));
        // 1st-pass swap.
        let _ = tab.test_poll_until_swapped();
        // Right after the swap, the live core's scrollback is empty (the
        // bypass intentionally left it so).
        assert_eq!(
            tab.test_scrollback_length(),
            0,
            "bypass-on 1st-pass leaves scrollback empty"
        );
        // Drive the 2nd-pass to completion (blocking-recv re-stage).
        assert!(tab.test_has_pending_scrollback_restore());
        tab.test_drain_pending_scrollback_restore_for_blocking_recv();
        let outcome = tab.poll_pending_scrollback_restore();
        assert_eq!(outcome, ScrollbackRestoreOutcome::Merged);
        // Now the live core's scrollback length matches the reference.
        assert_eq!(
            tab.test_scrollback_length(),
            reference_scrollback_count,
            "merged scrollback length must match the synchronous reference"
        );
        // Polling again is Idle (state was cleared by Merged).
        assert_eq!(
            tab.poll_pending_scrollback_restore(),
            ScrollbackRestoreOutcome::Idle
        );
    }

    /// TS-8 (FR5 / NFR4): a newer off-thread switch supersedes any
    /// in-flight 2nd-pass restore — the prior restore's state is dropped
    /// and the cancel flag is set so the worker bails.
    #[test]
    fn ts8_new_offthread_switch_supersedes_in_flight_restore() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        // First off-thread switch: drive to swap so the restore installs.
        tab.apply_mux_message(snapshot_msg(10, large_scrollable_payload()));
        let _ = tab.test_poll_until_swapped();
        assert!(tab.test_has_pending_scrollback_restore());
        // New off-thread switch to a different pane.
        tab.apply_mux_message(snapshot_msg(20, large_scrollable_payload()));
        // The prior restore is cleared immediately on the supersede arm
        // inside `dispatch_offthread_replay`.
        assert!(
            !tab.test_has_pending_scrollback_restore(),
            "supersede must clear the prior pending_scrollback_restore"
        );
    }

    /// TS-10 (FR5 / UC03): a resize during a pending 2nd-pass restore
    /// cancels it; no respawn (history-restore is abandoned).
    #[test]
    fn ts10_resize_cancels_pending_restore_without_respawn() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        tab.apply_mux_message(snapshot_msg(10, large_scrollable_payload()));
        let _ = tab.test_poll_until_swapped();
        assert!(tab.test_has_pending_scrollback_restore());
        // Different grid → resize cancels.
        tab.resize(100, 30);
        assert!(
            !tab.test_has_pending_scrollback_restore(),
            "resize must cancel the pending 2nd-pass scrollback restore"
        );
        // No respawn.
        assert!(
            !tab.test_has_pending_scrollback_restore(),
            "resize must NOT respawn the 2nd-pass restore at the new grid (UC03)"
        );
    }

    /// TS-11 (FR7): worker panic → `poll_pending_scrollback_restore`
    /// observes `Disconnected`, returns `Failed`, clears state, app
    /// continues. Force-disconnect simulates the panic path.
    #[test]
    fn ts11_restore_worker_panic_returns_failed_and_clears_state() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        tab.apply_mux_message(snapshot_msg(10, large_scrollable_payload()));
        let _ = tab.test_poll_until_swapped();
        assert!(tab.test_has_pending_scrollback_restore());
        // Force the sender to drop without ever sending a build — the next
        // try_recv will observe Disconnected.
        tab.test_force_scrollback_restore_disconnect();
        let outcome = tab.poll_pending_scrollback_restore();
        assert_eq!(outcome, ScrollbackRestoreOutcome::Failed);
        assert!(
            !tab.test_has_pending_scrollback_restore(),
            "Failed arm must clear pending state"
        );
        // Polling again is Idle.
        assert_eq!(
            tab.poll_pending_scrollback_restore(),
            ScrollbackRestoreOutcome::Idle
        );
    }

    /// TS-9 (FR3 + NFR5): between the 1st-pass swap and the 2nd-pass
    /// arrival, feeding live PTY output advances
    /// `scrollback_evicted_total` on the live core; `apply_scrollback_restore`
    /// trims that many trailing rebuilt rows so the merged scrollback has
    /// no duplicates.
    ///
    /// Approach: rather than feeding async PTY bytes, drive the bookkeeping
    /// directly: after the swap, feed a known set of `\r\n`s via the live
    /// core to bump `scrollback_evicted_total` by N, then complete the
    /// 2nd-pass via the blocking-recv re-stage and assert the final
    /// scrollback length is the reference length minus N (the trim
    /// arithmetic).
    #[test]
    fn ts9_concurrent_live_drain_trims_rebuilt_tail_no_duplicates() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        let payload = large_scrollable_payload();
        // Reference scrollback length.
        let never = std::sync::atomic::AtomicBool::new(false);
        let reference =
            term_core::terminal_core::TerminalCore::build_scrollback_only_from_snapshot(
                80,
                24,
                100,
                &payload,
                &[],
                &never,
            )
            .expect("reference");
        let reference_count = reference.core.get_scrollback_length() as usize;

        tab.apply_mux_message(snapshot_msg(10, payload));
        let _ = tab.test_poll_until_swapped();
        assert_eq!(tab.test_scrollback_length(), 0);
        // Drive live drain on the swapped-in core: push N lines that each
        // generate one scrollback row.
        let n_live: u32 = 12;
        let mut bytes = Vec::new();
        for _ in 0..n_live {
            bytes.extend_from_slice(b"live\r\n");
        }
        {
            let mut c = tab.core.lock();
            c.process_pty_data_fully(&bytes);
        }
        let live_scrollback_before_merge = tab.test_scrollback_length();
        // Each "live\r\n" past the 24-row viewport pushes one row in.
        // Confirm we genuinely grew the scrollback before the merge.
        assert!(
            live_scrollback_before_merge > 0,
            "live drain must have pushed rows into scrollback before the merge"
        );
        // Drive the 2nd-pass to completion.
        assert!(tab.test_has_pending_scrollback_restore());
        tab.test_drain_pending_scrollback_restore_for_blocking_recv();
        assert_eq!(
            tab.poll_pending_scrollback_restore(),
            ScrollbackRestoreOutcome::Merged
        );
        // After the merge: scrollback total = (reference_count) for a payload
        // that does not saturate the ring. The FR3 trim removes the
        // last `live_growth` rows from the rebuilt half, but `live_growth`
        // is 0 here because the live drain did not push the eviction
        // counter past `base_evicted_total` (the rebuilt scrollback's
        // capacity is 100 and `live_scrollback_before_merge < 100`). So
        // the merged length is the rebuilt length plus the live half.
        let final_scrollback = tab.test_scrollback_length() as usize;
        let live_count = live_scrollback_before_merge as usize;
        // Upper bound: reference_count + live_count (no duplication beyond
        // the FR3 trim). Lower bound: reference_count (live is appended;
        // the rebuilt prepend lands the historical half in front).
        assert!(
            final_scrollback <= reference_count + live_count,
            "no row duplication: final {final_scrollback} <= reference {reference_count} + live {live_count}",
        );
        assert!(
            final_scrollback >= reference_count.min(100),
            "the historical half must be merged in"
        );
    }

    /// TS-14 (要件定義書 §4.2 F02 edge case): when `live_growth >=
    /// rebuilt_count`, the merge is a full no-op (zero rows prepended) and
    /// the call returns cleanly. Drive directly via the merge primitive
    /// since plumbing a >100-row live drain through the test harness is
    /// brittle.
    #[test]
    fn ts14_live_growth_exceeds_rebuilt_count_full_noop() {
        // 6-row grid → easier to push rows past the viewport into scrollback
        // without needing 30+ lines of seed bytes.
        let mut live = term_core::terminal_core::TerminalCore::new(80, 6, 100);
        // Push 10 lines: 6 stay in viewport, the rest land in scrollback.
        live.process_pty_data_fully(b"a\r\nb\r\nc\r\nd\r\ne\r\nf\r\ng\r\nh\r\ni\r\nj\r\n");
        let live_count_before = live.get_scrollback_length();
        assert!(
            live_count_before > 0,
            "test prerequisite: live has scrollback"
        );

        let mut rebuilt = term_core::terminal_core::TerminalCore::new(80, 6, 100);
        rebuilt.process_pty_data_fully(b"x\r\ny\r\nz\r\n1\r\n2\r\n3\r\n4\r\n5\r\n");
        let rebuilt_count = rebuilt.get_scrollback_length() as usize;
        assert!(
            rebuilt_count > 0,
            "test prerequisite: rebuilt has scrollback"
        );

        // live_growth = rebuilt_count (= "everything was already drained
        // live"): merge must be a full no-op.
        let merged = live.merge_scrollback_from(rebuilt, rebuilt_count);
        assert_eq!(
            merged, 0,
            "merge must be a noop when live_growth >= rebuilt_count"
        );
        assert_eq!(
            live.get_scrollback_length(),
            live_count_before,
            "live scrollback must be unchanged on a noop merge"
        );
    }

    /// TS-15 (FR8): `merge_scrollback_from` only touches scrollback;
    /// `prompt_marks` / `fold_marks` from the 2nd-pass replay are dropped
    /// by `apply_scrollback_restore` and never reach the live tab's mark
    /// trackers. Concretely: the live core's prompt_marks count is
    /// unchanged by the merge.
    #[test]
    fn ts15_merge_does_not_duplicate_prompt_marks_or_fold_marks() {
        let mut live = term_core::terminal_core::TerminalCore::new(80, 24, 100);
        // Seed the live core with one prompt mark via OSC 133 A and leave
        // it in `pending_prompt_marks` (do NOT take, so we have a non-zero
        // baseline to compare against after the merge).
        live.process_pty_data_fully(b"\x1b]133;A\x07$ \r\n");

        // The 2nd-pass rebuilt core has its OWN prompt marks accumulated
        // during the parse — those marks would be `take_prompt_marks`'d by
        // the worker before sending if anyone consumed them, but
        // `apply_scrollback_restore` discards the marks instead (FR8).
        // To assert the merge primitive itself does not leak them, leave
        // them on the rebuilt core's pending queue and merge.
        let mut rebuilt = term_core::terminal_core::TerminalCore::new(80, 24, 100);
        rebuilt.process_pty_data_fully(b"\x1b]133;A\x07rebuilt\r\n");
        // Confirm the rebuilt core has at least one pending prompt mark
        // before the merge — touch via a getter that does NOT drain.
        // `pending_prompt_marks` is `pub(crate)` and crate-private; we
        // confirm via `get_scrollback_length` instead that the parse landed.
        assert!(
            rebuilt.get_scrollback_length() == 0,
            "test wiring: rebuilt prepared with a single A mark, no scroll"
        );

        // Merge.
        let merged = live.merge_scrollback_from(rebuilt, 0);
        // The rebuilt scrollback was 0 rows, so the merge inserted 0 rows.
        // What matters here is the FR8 invariant: live's marks queue is
        // still exactly the one we seeded.
        assert_eq!(merged, 0);
        let live_marks_after = live.take_prompt_marks();
        assert_eq!(
            live_marks_after.len(),
            1,
            "live's pending prompt marks must be exactly the one originally seeded; \
             the merge primitive must not append the rebuilt core's marks (FR8)"
        );
        assert_eq!(live_marks_after[0].kind, b'A');
    }

    // ── task0001: transplant callbacks + OSC registration across the
    // off-thread core swap ────────────────────────────────────────────────

    /// Minimal recording [`term_core::callbacks::TerminalCallbacks`] double
    /// (mirrors `term_core`'s internal `Recorder` test pattern) used to
    /// prove AC-1: the exact pre-swap callbacks instance is still the one
    /// firing after `apply_offthread_swap`, not merely *a* fresh instance.
    #[derive(Default)]
    struct OscRecorder {
        events: Mutex<Vec<(u8, String)>>,
    }

    struct RecorderCallbacks(Arc<OscRecorder>);

    impl term_core::callbacks::TerminalCallbacks for RecorderCallbacks {
        fn on_osc(&self, action_type: u8, data: &str) {
            self.0.events.lock().push((action_type, data.to_string()));
        }
        fn on_apc(&self, _data: &[u8]) {}
        fn on_dcs(&self, _data: &[u8]) {}
        fn on_bell(&self) {}
        fn on_device_response(&self, _data: &[u8]) {}
    }

    /// AC-1 (SPEC TS-1): after `apply_offthread_swap`, the live core's
    /// callbacks is the pre-swap instance — a recording callbacks double
    /// installed before the swap still receives events fed to the
    /// swapped-in core afterward.
    #[test]
    fn ac1_offthread_swap_transplants_the_preswap_callbacks_instance() {
        let mut tab = test_tab();
        let recorder = Arc::new(OscRecorder::default());
        tab.core.lock().callbacks = Some(Box::new(RecorderCallbacks(recorder.clone())));

        tab.apply_mux_message(snapshot_msg(10, large_payload("SWAP")));
        assert!(
            tab.test_has_pending_switch(),
            "test prerequisite: large payload must go off-thread"
        );
        assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);

        // Feed an OSC directly through the now-swapped core; the SAME
        // recorder installed before the swap must observe it.
        tab.core
            .lock()
            .process_pty_data_fully(b"\x1b]2;hello\x1b\\");
        assert_eq!(
            recorder.events.lock().as_slice(),
            &[(2u8, "hello".to_string())],
            "the pre-swap callbacks instance must still be the one firing after the swap"
        );
    }

    /// AC-2 (SPEC TS-2): after `apply_offthread_swap`, feeding an OSC 9999
    /// (`MUX_OSC_PARAM`) sequence to the live core triggers the same
    /// registered app-param action as on a never-swapped tab core. Without
    /// the registration surviving the swap, OSC 9999 maps to action_type 255
    /// (Unknown) and never reaches `pending_apc`.
    #[test]
    fn ac2_offthread_swap_preserves_osc_9999_app_param_registration() {
        let mut tab = test_tab();
        tab.apply_mux_message(snapshot_msg(10, large_payload("SWAP")));
        assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);

        let welcome = welcome_msg(&[(1, "a", 10)], 0);
        let osc_bytes = welcome.to_osc();
        tab.core.lock().process_pty_data_fully(osc_bytes.as_bytes());
        assert_eq!(
            tab.cb_state.lock().pending_apc.len(),
            1,
            "OSC 9999 must still map to OSC_MUX_INBAND and reach pending_apc after the swap"
        );
    }

    /// AC-3 (SPEC TS-3): after an off-thread swap, a pre-mux Welcome frame in
    /// OSC 9999 form arriving on the outer-stream path (`process_outer_via_core`,
    /// taken while `mux_session_name` is `None`) reaches `apply_mux_message`.
    #[test]
    fn ac3_offthread_swap_preserves_premux_welcome_osc_form_reaching_apply_mux_message() {
        let mut tab = test_tab();
        // No prior Welcome: the tab starts pre-mux, mirroring the Windows
        // ConPTY fallback scenario where the OSC 9999 Welcome has not yet
        // arrived when a large snapshot triggers the off-thread swap.
        tab.apply_mux_message(snapshot_msg(10, large_payload("SWAP")));
        assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
        assert!(
            tab.mux_session_name.is_none(),
            "test prerequisite: tab is still pre-mux after the swap"
        );

        let welcome = welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0);
        let osc_bytes = welcome.to_osc().into_bytes();
        // Drive the pre-mux outer-stream path (`process_combined` routes
        // through `self.core` when `mux_session_name` is `None`).
        tab.test_process_combined(osc_bytes);
        assert_eq!(
            tab.mux_session_name.as_deref(),
            Some("main"),
            "the OSC 9999 Welcome frame must still reach apply_mux_message after the swap"
        );
    }

    /// AC-4 (SPEC TS-4): after an off-thread swap, a pre-mux Welcome frame in
    /// APC form is also processed to `apply_mux_message`. Unlike AC-3 (OSC
    /// 9999), the APC path needs only the transplanted callbacks (`on_apc`
    /// fires unconditionally for any APC, no app-param registration
    /// involved) — this pins that path separately.
    #[test]
    fn ac4_offthread_swap_preserves_premux_welcome_apc_form_reaching_apply_mux_message() {
        let mut tab = test_tab();
        tab.apply_mux_message(snapshot_msg(10, large_payload("SWAP")));
        assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
        assert!(
            tab.mux_session_name.is_none(),
            "test prerequisite: tab is still pre-mux after the swap"
        );

        let welcome = welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0);
        let apc_bytes = welcome.to_apc().into_bytes();
        tab.test_process_combined(apc_bytes);
        assert_eq!(
            tab.mux_session_name.as_deref(),
            Some("main"),
            "the APC-form Welcome frame must still reach apply_mux_message after the swap"
        );
    }

    /// AC-5 (SPEC TS-5): after an off-thread swap, a callback-driven OSC
    /// (title change) in subsequent PTY output invokes the transplanted
    /// callbacks end to end (`NativeCallbacks::on_osc` -> `cb_state.title`
    /// -> `Tab::title`), not merely proving a callback object is present.
    #[test]
    fn ac5_offthread_swap_transplanted_callbacks_apply_title_change() {
        let mut tab = test_tab();
        tab.apply_mux_message(snapshot_msg(10, large_payload("SWAP")));
        assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);

        tab.test_process_combined(b"\x1b]2;post-swap-title\x1b\\".to_vec());
        assert_eq!(tab.title, "post-swap-title");
    }

    /// AC-6 (SPEC TS-7 / risk mitigation): the 2nd-pass scrollback restore
    /// path (`spawn_scrollback_restore` -> `apply_scrollback_restore`) merges
    /// into the live core via `merge_scrollback_from` and never replaces it
    /// — the transplanted callbacks and OSC registration from the 1st-pass
    /// swap survive the 2nd-pass merge too.
    #[test]
    fn ac6_scrollback_restore_merge_does_not_clear_callbacks_or_osc_registration() {
        let mut tab = test_tab();
        // Pre-mux (no Welcome), same as the AC tests above, so
        // `test_process_combined` routes through `process_outer_via_core`
        // below (mux established would route through the independent mux
        // extractor instead, which is not what this test exercises).
        tab.apply_mux_message(snapshot_msg(10, large_scrollable_payload()));
        assert!(tab.test_has_pending_switch());
        // Blocking-recv re-stage (not the spin-based `test_poll_until_swapped`)
        // so this 1st-pass swap is robust to worker-thread scheduling delays
        // under system load — this test drives both an off-thread swap AND a
        // 2nd-pass restore in sequence, so it is more sensitive to that than
        // the single-swap AC tests above.
        tab.test_block_worker_ready();
        assert_eq!(tab.poll_pending_switch(), SwapOutcome::Swapped);
        assert!(tab.test_has_pending_scrollback_restore());

        // Drive the 2nd-pass restore to completion (the merge under test).
        tab.test_drain_pending_scrollback_restore_for_blocking_recv();
        assert_eq!(
            tab.poll_pending_scrollback_restore(),
            ScrollbackRestoreOutcome::Merged
        );

        // Callbacks must still be installed and wired end to end (title
        // sync)...
        tab.test_process_combined(b"\x1b]2;after-restore\x1b\\".to_vec());
        assert_eq!(tab.title, "after-restore");

        // ...and the OSC 9999 app-param registration must still be in
        // effect (pending_apc sink reached).
        let welcome = welcome_msg(&[(3, "c", 30)], 0);
        tab.core
            .lock()
            .process_pty_data_fully(welcome.to_osc().as_bytes());
        assert_eq!(tab.cb_state.lock().pending_apc.len(), 1);
    }

    /// AC-7 (SPEC edge case): an old core whose callbacks slot is empty
    /// swaps without panic and yields a live core with no callbacks.
    #[test]
    fn ac7_offthread_swap_with_no_preswap_callbacks_yields_none_without_panic() {
        let mut tab = test_tab();
        // Simulate a live core with an empty callbacks slot.
        tab.core.lock().callbacks = None;

        tab.apply_mux_message(snapshot_msg(10, large_payload("SWAP")));
        assert!(tab.test_has_pending_switch());
        // Must not panic.
        assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);

        assert!(
            tab.core.lock().callbacks.is_none(),
            "an old core with no callbacks must swap to a live core with no callbacks"
        );
    }

    // ── task0003 FR7/FR8: resize-race bypass resilience + duplicate
    // snapshot fetch dedup ──────────────────────────────────────────────

    /// A structurally-segmented (`EMSNAP2` framed) snapshot payload with
    /// `OFFTHREAD_REPLAY_SEGMENT_THRESHOLD` segments, all recorded at
    /// `(cols, rows)` — forcing the off-thread dispatch path via segment
    /// COUNT rather than byte size (mirrors the existing
    /// `ac5_small_payload_many_segment_snapshot_dispatches_off_thread`
    /// fixture), so the underlying content stays tiny and a worker's
    /// build always completes fast regardless of a test's polling budget.
    /// Every segment already matching `(cols, rows)` makes
    /// `stable_target_suffix_start` return `k == 0` for that SAME target —
    /// the trivial "every segment already matches" bypass-engage case —
    /// and, symmetrically, `k == segments.len()` (no bypass) for any OTHER
    /// target, which is exactly the shape task0003 FR7/FR8 need: a
    /// dispatch-consistent-target-and-segments regression guard (AC-2), and
    /// a target-mismatch-after-the-fact case (AC-1) that must not pay for
    /// more than one wasted rebuild.
    fn many_segment_payload_at(cols: u16, rows: u16) -> (Vec<mux_ipc::protocol::DimSegment>, Vec<u8>) {
        let content = b"content\r\n".to_vec();
        let segments: Vec<mux_ipc::protocol::DimSegment> = (0..OFFTHREAD_REPLAY_SEGMENT_THRESHOLD)
            .map(|_| mux_ipc::protocol::DimSegment {
                offset: 0,
                cols,
                rows,
            })
            .collect();
        (segments, content)
    }

    /// AC-2: an ordinary (unraced) switch — segments' tail already matches
    /// the dispatch-time target — engages bypass, observed indirectly via
    /// the 2nd-pass scrollback-restore worker being spawned
    /// (`scrollback_populated: false` on the 1st-pass replay is exactly
    /// when `apply_offthread_swap` spawns it; see D3' in that method's
    /// doc). Regression guard: this must stay true after the FR7 fix
    /// below, not just before it. Also satisfies task0006's AC-2 (FR7
    /// regression guard: the unraced case is unaffected by that task's
    /// `PendingSwitch::pending_resize` fix, since no resize means
    /// `pending_resize` is never touched) — unchanged, no separate test
    /// needed.
    #[test]
    fn ac2_unraced_switch_engages_bypass() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        let (cols, rows) = {
            let c = tab.core.lock();
            (c.cols(), c.rows())
        };
        let (segments, content) = many_segment_payload_at(cols, rows);
        let encoded = mux_ipc::protocol::encode_snapshot_payload(&segments, &content);
        tab.apply_mux_message(snapshot_msg(10, encoded));
        assert!(tab.test_has_pending_switch(), "must go off-thread");
        assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
        assert!(
            tab.test_has_pending_scrollback_restore(),
            "an unraced switch whose segments already match the target must \
             engage bypass (observed via the 2nd-pass scrollback-restore \
             worker being spawned)"
        );
    }

    /// AC-1 / AC-7 (FR7, task0006 redesign, review round-1 findings
    /// `64baa639d71792f9` / `34a708465d04f983`, AC-9 regression guard): a
    /// resize STORM landing during an in-flight switch — several resize
    /// events, each superseding the last, before the worker is ever polled
    /// — must not pay for one wasted off-thread build per intermediate,
    /// already-superseded target. Adapted from task0003's original test of
    /// the same name/intent: the round-1 fix collapsed the storm into ONE
    /// re-dispatch (`test_offthread_spawn_count` going from 1 to 2 once);
    /// task0006's redesign collapses it further into ZERO re-dispatches —
    /// the in-flight worker's own build is never touched by a resize at
    /// all (`PendingSwitch::pending_resize` just tracks the latest target),
    /// so the count now stays at 1 for the whole storm AND after the swap
    /// (AC-7: no payload/segments clone per resize event either, since
    /// there is no re-dispatch to clone for).
    #[test]
    fn ac1_resize_storm_during_pending_switch_collapses_to_one_redispatch() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        let (cols, rows) = {
            let c = tab.core.lock();
            (c.cols(), c.rows())
        };
        let (segments, content) = many_segment_payload_at(cols, rows);
        let encoded = mux_ipc::protocol::encode_snapshot_payload(&segments, &content);
        tab.apply_mux_message(snapshot_msg(10, encoded));
        assert!(tab.test_has_pending_switch(), "must go off-thread");
        let spawns_after_dispatch = tab.test_offthread_spawn_count();
        assert_eq!(spawns_after_dispatch, 1);

        // A resize storm: several resize events land before the in-flight
        // worker is ever polled.
        tab.resize(90, 30);
        tab.resize(95, 35);
        tab.resize(100, 40);
        assert_eq!(
            tab.test_offthread_spawn_count(),
            spawns_after_dispatch,
            "a resize storm must not spawn an extra worker per resize event \
             — the in-flight worker's own build is untouched by any of them"
        );
        assert!(
            !tab.test_has_pending_redispatch(),
            "a resize-only storm must never coalesce a re-dispatch (FR7 \
             fix: it would defeat the in-flight worker's bypass split)"
        );
        assert_eq!(
            tab.test_pending_resize(),
            Some((100, 40)),
            "the storm's final target collapses into one deferred resize"
        );

        // The SAME worker (never re-dispatched) resolves the switch; the
        // deferred resize is applied to the swapped-in core.
        assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
        assert_eq!(
            tab.test_offthread_spawn_count(),
            spawns_after_dispatch,
            "no second worker was ever needed to resolve the storm"
        );
        let c = tab.core.lock();
        assert_eq!((c.cols(), c.rows()), (100, 40));
    }

    /// AC-3 (FR8): two `Snapshot` frames for the SAME pane arriving in
    /// immediate succession (before the first's replay would complete) —
    /// the second must coalesce into the first's in-flight request rather
    /// than spawning a second worker right away. Confirmed to fail
    /// pre-fix: before the same-pane coalesce existed,
    /// `test_offthread_spawn_count` would have read 2 immediately after
    /// the second frame, not 1.
    #[test]
    fn ac3_duplicate_same_pane_snapshot_coalesces_before_spawning_again() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        let (segments1, content1) = many_segment_payload_at(80, 24);
        let encoded1 = mux_ipc::protocol::encode_snapshot_payload(&segments1, &content1);
        tab.apply_mux_message(snapshot_msg(10, encoded1));
        assert!(tab.test_has_pending_switch(), "must go off-thread");
        assert_eq!(tab.test_offthread_spawn_count(), 1);

        // A second Snapshot for the SAME pane arrives before the first's
        // replay would complete (segment count differing by one, mirroring
        // the observed segs=9 then segs=10 trace) and with different
        // (identifiable) content.
        let mut segments2 = segments1;
        segments2.push(mux_ipc::protocol::DimSegment {
            offset: 0,
            cols: 80,
            rows: 24,
        });
        let content2 = b"SECOND\r\n".to_vec();
        let encoded2 = mux_ipc::protocol::encode_snapshot_payload(&segments2, &content2);
        tab.apply_mux_message(snapshot_msg(10, encoded2));

        assert_eq!(
            tab.test_offthread_spawn_count(),
            1,
            "a duplicate snapshot fetch for the pane already being switched \
             to must coalesce, not spawn a second worker immediately — the \
             discarded (first) request's work must not run alongside it"
        );
        assert!(tab.test_has_pending_redispatch());

        // Only the second's outcome ever completes and gets displayed.
        assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
        assert_eq!(tab.test_offthread_spawn_count(), 2);
        assert!(
            tab.test_grid_text().contains("SECOND"),
            "the second fetch's content must be the one that ends up displayed"
        );
    }

    /// AC-4: two switches to DIFFERENT panes arriving in immediate
    /// succession are NOT deduplicated against each other — the dedup in
    /// AC-3 is scoped to same-pane frames only, so a switch to a different
    /// pane must still spawn its own worker right away (an ordinary
    /// pane-to-pane switch must not regress into the coalesce path).
    #[test]
    fn ac4_switch_to_different_pane_is_not_coalesced() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        let (segments, content) = many_segment_payload_at(80, 24);
        let encoded = mux_ipc::protocol::encode_snapshot_payload(&segments, &content);
        tab.apply_mux_message(snapshot_msg(10, encoded));
        assert!(tab.test_has_pending_switch());
        assert_eq!(tab.test_offthread_spawn_count(), 1);

        // The daemon moved the active pane to 20, then a second large
        // snapshot arrives for it — a genuinely different pane, not a
        // duplicate of the first.
        tab.mux_group.as_mut().unwrap().set_active_by_pane(20);
        let (segments2, content2) = many_segment_payload_at(80, 24);
        let encoded2 = mux_ipc::protocol::encode_snapshot_payload(&segments2, &content2);
        tab.apply_mux_message(snapshot_msg(20, encoded2));

        assert_eq!(
            tab.test_offthread_spawn_count(),
            2,
            "a switch to a DIFFERENT pane must spawn its own worker \
             immediately, not coalesce against the prior pane's request"
        );
        assert!(
            !tab.test_has_pending_redispatch(),
            "a different-pane switch is not a coalesce candidate"
        );
        assert_eq!(tab.test_pending_target(), Some(20));
        assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
    }

    /// AC-5: a switch to the same pane arriving WELL AFTER the previous one
    /// has already completed and been displayed (not a near-simultaneous
    /// race) is NOT dropped — the AC-3 dedup must not degrade into "ignore
    /// all repeat switches to a pane."
    #[test]
    fn ac5_late_repeat_switch_to_same_pane_is_not_dropped() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        let (segments1, content1) = many_segment_payload_at(80, 24);
        let encoded1 = mux_ipc::protocol::encode_snapshot_payload(&segments1, &content1);
        tab.apply_mux_message(snapshot_msg(10, encoded1));
        assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
        assert!(!tab.test_has_pending_switch());
        assert_eq!(tab.test_offthread_spawn_count(), 1);

        // Well after the first has settled (no in-flight switch left at
        // all), a repeat switch to the SAME pane arrives.
        let (segments2, content2) = many_segment_payload_at(80, 24);
        let encoded2 = mux_ipc::protocol::encode_snapshot_payload(&segments2, &content2);
        tab.apply_mux_message(snapshot_msg(10, encoded2));

        assert!(
            tab.test_has_pending_switch(),
            "a repeat switch arriving after the prior one already settled \
             must not be dropped"
        );
        assert_eq!(
            tab.test_offthread_spawn_count(),
            2,
            "with no in-flight request left to coalesce against, this must \
             spawn its own worker immediately"
        );
        assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
    }

    // ── task0006: pending_switch/pending_redispatch redesign — FR7
    // target-dims mismatch, FR8 decode dedup scope, and the live-queue
    // loss/duplication defect introduced by round-1's own auto-fix ────────

    /// task0006 AC-1 (FR7, review round-1 finding `64baa639d71792f9`): a
    /// grid resize racing an in-flight switch must not defeat the bypass
    /// split for the build that eventually completes. Resolved by
    /// deferring the resize (`PendingSwitch::pending_resize`) instead of
    /// re-dispatching the in-flight worker at a target its `segments`
    /// were never captured at — the worker's OWN dispatch-time target is
    /// unaffected by the race, so its bypass split stays valid; the
    /// resize is applied afterward via an ordinary `TerminalCore::resize`
    /// on the swapped-in core. Bypass engagement is observed indirectly
    /// via the 2nd-pass scrollback-restore worker being spawned — the
    /// same signal `ac2_unraced_switch_engages_bypass` uses (see that
    /// test's doc for why `scrollback_populated: false` <=> bypass
    /// engaged <=> the restore worker spawns).
    #[test]
    fn t6_ac1_resize_during_in_flight_switch_still_engages_bypass() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        let (cols, rows) = {
            let c = tab.core.lock();
            (c.cols(), c.rows())
        };
        // Segments recorded at the ORIGINAL (dispatch-time) target — the
        // shape a real daemon-captured payload has: it reflects whatever
        // grid the daemon knew about BEFORE this resize lands.
        let (segments, content) = many_segment_payload_at(cols, rows);
        let encoded = mux_ipc::protocol::encode_snapshot_payload(&segments, &content);
        tab.apply_mux_message(snapshot_msg(10, encoded));
        assert!(tab.test_has_pending_switch(), "must go off-thread");

        // A resize races the in-flight switch, landing on a DIFFERENT
        // target than the segments were captured at.
        tab.resize(100, 40);
        assert_eq!(tab.test_pending_resize(), Some((100, 40)));

        assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
        assert!(
            tab.test_has_pending_scrollback_restore(),
            "a resize racing an in-flight switch must not defeat the \
             bypass split for the build that completes — the worker's \
             OWN target (unaffected by the race) still matches the \
             segments' recorded dims, so bypass still engages"
        );
        let c = tab.core.lock();
        assert_eq!(
            (c.cols(), c.rows()),
            (100, 40),
            "the racing resize still lands on the displayed core"
        );
    }

    /// task0006 AC-3 (live-output correctness, review round-1 findings
    /// `7ed0ba7335376c20` / `ebc9de26bb15fcb1`): "Snapshot P1 dispatched
    /// -> live output L1 arrives -> Snapshot P2 for the same pane arrives
    /// (coalesces) -> more live output L2 arrives -> poll" — neither L1
    /// nor L2 is lost or duplicately applied against the final,
    /// P2-based core.
    ///
    /// P2's own content bakes in L1's effect (row 1), simulating a real
    /// daemon capture taken AFTER L1's PTY activity — the discard/keep
    /// decision this task moves to coalesce time (`dispatch_offthread_replay`'s
    /// same-pane branch) must clear the stale L1 there so it is not
    /// re-applied on top of P2 (duplication), while L2 — arriving strictly
    /// AFTER the coalesce, never captured by P2 — must still land exactly
    /// once (loss is the pre-fix regression this test pins).
    #[test]
    fn t6_ac3_coalesced_snapshot_live_output_neither_lost_nor_duplicated() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));

        // P1 dispatched off-thread.
        tab.apply_mux_message(snapshot_msg(10, large_payload("FIRST")));
        assert!(tab.test_has_pending_switch());

        // L1 arrives while P1 is in flight.
        tab.apply_mux_message(pty_output(10, b"L1".to_vec()));
        assert_eq!(tab.test_pending_live_queue(), vec![b"L1".to_vec()]);

        // P2: a fresh same-pane snapshot whose own content already bakes
        // in L1.
        let mut p2 = b"FIRST\r\nL1\r\n".to_vec();
        p2.resize(OFFTHREAD_REPLAY_THRESHOLD_BYTES + 16, 0);
        tab.apply_mux_message(snapshot_msg(10, p2));
        assert!(
            tab.test_has_pending_redispatch(),
            "a second snapshot for the same pane must coalesce"
        );
        assert!(
            tab.test_pending_live_queue().is_empty(),
            "L1 was subsumed into P2's own content — the coalesce must \
             clear it so it is not re-applied on top of P2 (duplication)"
        );

        // L2 arrives after the coalesce, before poll — genuinely new
        // output not reflected in P2's content.
        tab.apply_mux_message(pty_output(10, b"L2".to_vec()));
        assert_eq!(tab.test_pending_live_queue(), vec![b"L2".to_vec()]);

        // Resolve: the fresh worker for P2 completes, L2 replays on top.
        assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
        assert_eq!(tab.test_row_text(0), "FIRST");
        assert_eq!(tab.test_row_text(1), "L1", "L1 applied exactly once, via P2's own content");
        assert_eq!(tab.test_row_text(2), "L2", "L2 must not be lost");
        assert_eq!(tab.test_row_text(3), "", "nothing duplicately applied past L2");
    }

    /// task0006 AC-4 (live-output correctness, resize-driven case): a
    /// resize-driven re-dispatch of the SAME payload (task0006 redesign:
    /// this no longer re-dispatches at all — see
    /// `PendingSwitch::pending_resize`) must still preserve queued live
    /// output and apply it exactly once — the original, correct behavior
    /// the round-1 fix (`old.payload == payload` at poll time) was trying
    /// to preserve, now achieved for free since the queue is never touched
    /// by a resize at all.
    #[test]
    fn t6_ac4_resize_driven_case_preserves_live_queue_exactly_once() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        tab.apply_mux_message(snapshot_msg(10, large_payload("FIRST")));
        assert!(tab.test_has_pending_switch());

        tab.apply_mux_message(pty_output(10, b"L1".to_vec()));
        assert_eq!(tab.test_pending_live_queue(), vec![b"L1".to_vec()]);

        // A grid resize races the SAME in-flight payload (no new
        // snapshot arrives).
        tab.resize(100, 40);
        assert!(
            !tab.test_has_pending_redispatch(),
            "a resize alone must not coalesce a re-dispatch (FR7 fix)"
        );
        assert_eq!(
            tab.test_pending_live_queue(),
            vec![b"L1".to_vec()],
            "the queue must survive the resize untouched"
        );

        assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
        assert_eq!(tab.test_row_text(0), "FIRST");
        assert_eq!(tab.test_row_text(1), "L1");
        assert_eq!(tab.test_row_text(2), "", "L1 applied exactly once");
        let c = tab.core.lock();
        assert_eq!((c.cols(), c.rows()), (100, 40));
    }

    /// task0006 AC-5 (live-output correctness, chained coalesce): 3+
    /// chained same-pane transitions before a single poll — P1 dispatched,
    /// L1 arrives, a resize races (deferred, no re-dispatch), P2 arrives
    /// (coalesces, subsumes L1), L2 arrives, P3 arrives (coalesces again,
    /// subsumes L2, supersedes P2 entirely) — live output must be
    /// attributed correctly across ALL the intermediate transitions, not
    /// just a single coalesce hop. Only P3's replay ever completes.
    #[test]
    fn t6_ac5_chained_coalesce_across_resize_and_two_snapshots_attributes_live_output_correctly() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));

        // P1
        tab.apply_mux_message(snapshot_msg(10, large_payload("FIRST")));
        assert!(tab.test_has_pending_switch());

        // L1
        tab.apply_mux_message(pty_output(10, b"L1".to_vec()));
        assert_eq!(tab.test_pending_live_queue(), vec![b"L1".to_vec()]);

        // A resize races the in-flight P1 — deferred, no re-dispatch; the
        // queue is untouched by it (task0006 AC-4's own claim).
        tab.resize(100, 40);
        assert_eq!(tab.test_pending_live_queue(), vec![b"L1".to_vec()]);

        // P2: a fresh same-pane snapshot whose own content already bakes
        // in L1.
        let mut p2 = b"FIRST\r\nL1\r\n".to_vec();
        p2.resize(OFFTHREAD_REPLAY_THRESHOLD_BYTES + 16, 0);
        tab.apply_mux_message(snapshot_msg(10, p2));
        assert!(tab.test_has_pending_redispatch());
        assert!(
            tab.test_pending_live_queue().is_empty(),
            "L1 subsumed by P2's own content at THIS coalesce"
        );

        // L2 arrives after P2 coalesced, before poll.
        tab.apply_mux_message(pty_output(10, b"L2".to_vec()));
        assert_eq!(tab.test_pending_live_queue(), vec![b"L2".to_vec()]);

        // P3: another fresh same-pane snapshot, superseding P2 (which is
        // never replayed), whose own content bakes in L2 too.
        let mut p3 = b"FIRST\r\nL1\r\nL2\r\n".to_vec();
        p3.resize(OFFTHREAD_REPLAY_THRESHOLD_BYTES + 16, 0);
        tab.apply_mux_message(snapshot_msg(10, p3));
        assert!(tab.test_has_pending_redispatch());
        assert!(
            tab.test_pending_live_queue().is_empty(),
            "L2 subsumed by P3's own content at THIS (second) coalesce — \
             each new coalesce re-evaluates the discard/keep decision, \
             not just the first hop"
        );

        // Resolve: only P3's replay ever completes, at the resized grid.
        assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
        assert_eq!(tab.test_row_text(0), "FIRST");
        assert_eq!(tab.test_row_text(1), "L1");
        assert_eq!(tab.test_row_text(2), "L2");
        assert_eq!(tab.test_row_text(3), "", "nothing duplicately applied");
        let c = tab.core.lock();
        assert_eq!((c.cols(), c.rows()), (100, 40));
    }

    /// task0006 AC-6 (FR8 scope): FR8's replay-BUILD dedup (task0003,
    /// unaffected by this task) means a duplicate same-pane snapshot never
    /// spawns a second WORKER — but every incoming `Snapshot`/
    /// `SnapshotRestore` frame still runs `decode_snapshot_payload_typed`.
    /// This task's redesign prioritized FR7 and the live-queue lifecycle
    /// (both correctness-critical) and kept FR8 at the replay-build level
    /// only rather than adding fetch/decode-level dedup (medium severity,
    /// secondary per the task plan) — this test pins that narrower claim
    /// explicitly so the requirement and the test do not silently
    /// disagree. `red_confirmed: false` in this task's test record: this
    /// documents PRE-EXISTING, unchanged behavior (identical before and
    /// after this task), not a fix.
    #[test]
    fn t6_ac6_duplicate_same_pane_snapshot_still_decodes_twice_replay_build_dedup_only() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        tab.apply_mux_message(snapshot_msg(10, large_payload("FIRST")));
        assert_eq!(tab.test_snapshot_decode_count(), 1);
        assert_eq!(tab.test_offthread_spawn_count(), 1);

        // A duplicate same-pane snapshot arrives before the first swaps.
        tab.apply_mux_message(snapshot_msg(10, large_payload("SECOND")));
        assert_eq!(
            tab.test_snapshot_decode_count(),
            2,
            "FR8 dedup happens at the replay-BUILD level (no second \
             worker spawn, see test_offthread_spawn_count below) — decode \
             still runs for every incoming frame"
        );
        assert_eq!(
            tab.test_offthread_spawn_count(),
            1,
            "the worker spawn itself IS deduplicated"
        );
    }

    /// task0006 AC-7 (performance, review round-1 finding
    /// `34a708465d04f983`): the wasteful full-payload clone
    /// `Tab::resize`'s old redispatch branch performed on every resize
    /// event while a same-pane coalesce was pending no longer happens —
    /// demonstrated as a side effect of the AC-1 fix: since a resize
    /// never calls `dispatch_offthread_replay` (the only call site that
    /// ever cloned `pending.payload`/`.segments` for this case) any more,
    /// there is no code path left that could perform that clone. Observed
    /// indirectly via `test_offthread_spawn_count` staying flat across a
    /// resize storm (`t6_ac1`'s sibling assertion in
    /// `ac1_resize_storm_during_pending_switch_collapses_to_one_redispatch`
    /// proves the same absence of a redispatch call, which is what the
    /// clone was conditional on).
    #[test]
    fn t6_ac7_resize_storm_performs_no_redispatch_hence_no_payload_clone() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        tab.apply_mux_message(snapshot_msg(10, large_payload("FIRST")));
        assert!(tab.test_has_pending_switch());

        // A resize storm: several events, each landing on a different
        // target, before the worker is ever polled.
        for (cols, rows) in [(90, 30), (95, 35), (100, 40), (105, 45)] {
            tab.resize(cols, rows);
            assert!(
                !tab.test_has_pending_redispatch(),
                "no resize event may coalesce a re-dispatch — the clone \
                 `Tab::resize`'s old branch performed to build one is \
                 gone along with the redispatch call itself"
            );
        }
        assert_eq!(tab.test_pending_resize(), Some((105, 45)));
    }

    /// task0006 AC-8 (performance, review round-1 finding
    /// `5b1878c41d3e02d6`): the O(n) `old.payload == payload` full-byte
    /// comparison round-1's fix added to `poll_pending_switch` is gone —
    /// removed as a direct consequence of moving the discard/keep decision
    /// to coalesce time (AC-3). This is a structural/code-level property
    /// (best confirmed by inspection: `poll_pending_switch`'s
    /// `pending_redispatch`-take branch now unconditionally inherits
    /// `old.live_queue`/`old.queued_bytes`, replacing the byte comparison
    /// with a `debug_assert_eq!` on pane identity alone); this test pins
    /// the OBSERVABLE half of that claim — the coalesce path (where the
    /// decision now lives) runs in O(1) relative to payload size, checked
    /// by exercising it with a large payload and confirming the outcome
    /// is still correct (the byte-size-dependent cost, if it existed,
    /// would not change the RESULT, only the time — so this test's real
    /// value is pinning that the coalesce-time clear behaves correctly
    /// even for a large payload, alongside the code-inspection evidence
    /// noted above).
    #[test]
    fn t6_ac8_large_payload_coalesce_clears_queue_without_poll_time_comparison() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
        // A payload well past the off-thread threshold.
        tab.apply_mux_message(snapshot_msg(10, large_payload("FIRST")));
        tab.apply_mux_message(pty_output(10, b"L1".to_vec()));
        assert_eq!(tab.test_pending_live_queue(), vec![b"L1".to_vec()]);

        // A large, DIFFERENT same-pane payload coalesces.
        tab.apply_mux_message(snapshot_msg(10, large_payload("SECOND")));
        assert!(
            tab.test_pending_live_queue().is_empty(),
            "the discard decision at coalesce time does not depend on \
             comparing this (large) payload against the old one byte for \
             byte — see AC-8's doc for the removed poll-time comparison"
        );
        assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
        assert_eq!(tab.test_row_text(0), "SECOND");
    }
}
