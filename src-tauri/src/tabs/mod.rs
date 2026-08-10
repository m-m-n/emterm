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
use crate::pty::{PtyEvent, PtySession};
use crate::render::theme::Theme;
use crate::settings::Settings;
use mux_ipc::protocol::{MessageType, MuxMessage, ResizeMsg};
#[cfg(test)]
use mux_ipc::protocol::{RenameWindowMsg, WelcomeMsg};
use term_core::terminal_core::ReplaySegment;

mod images;
mod input;
mod marks_fold;
mod mux_link;
mod output_pipeline;
mod replay;

#[cfg(test)]
use input::payload_has_device_query;

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
    /// Test-only (tmux-startup-query-response-leak task0001): every payload
    /// actually handed to this tab's outbound channel by [`Self::write`]
    /// (plain-tab raw PTY write) or [`Self::send_control`] (mux `PtyInput`
    /// frame — the pre-encode payload is recorded, not the APC-wrapped
    /// wire bytes), regardless of whether `self.pty` is populated. This is
    /// the byte-level sink a synthesized device response ultimately reaches
    /// on its way to the querying application, whichever of the two
    /// delivery routes carried it — the seam this task's I1 (exactly-once)
    /// regression tests read from. `Mutex` because both methods take
    /// `&self`. Strictly `cfg(test)` so the production build carries no
    /// observer.
    #[cfg(test)]
    outbound_write_log: Mutex<Vec<Vec<u8>>>,
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
        Self::build(
            title,
            cols,
            rows,
            scrollback_lines,
            settings,
            statusbar_dispatcher,
            cwd_provider,
            notification_sink,
            spawn_overrides,
            true,
        )
    }

    /// Test-only: a tab built exactly like [`Self::spawn_shell`] except
    /// that no real shell process is spawned — `pty` stays `None` and
    /// the event channel starts disconnected, so `pump` drains nothing
    /// but the state the test itself injects. Tests that drive
    /// `App::pump_all` over injected eviction deltas need this: a real
    /// shell's startup output arriving mid-pump under host load shifts
    /// the counters those tests assert on (the historical flakiness of
    /// the `pump_all_*` selection/anchor tests).
    #[cfg(test)]
    pub(crate) fn test_shell_less(
        title: impl Into<String>,
        cols: u16,
        rows: u16,
        scrollback_lines: u32,
        settings: Arc<Settings>,
        notification_sink: Arc<dyn crate::callbacks::NotificationSink>,
    ) -> Self {
        Self::build(
            title,
            cols,
            rows,
            scrollback_lines,
            settings,
            None,
            None,
            notification_sink,
            None,
            false,
        )
    }

    /// Shared constructor behind [`Self::spawn_shell`] (production,
    /// `spawn_pty: true`) and [`Self::test_shell_less`] (tests,
    /// `spawn_pty: false`). Identical in every respect except whether a
    /// real PTY child is spawned.
    #[allow(clippy::too_many_arguments)]
    fn build(
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
        spawn_pty: bool,
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
        let pty = if spawn_pty {
            match PtySession::spawn(
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
            }
        } else {
            // Test-only route (`test_shell_less`): dropping `tx` here
            // leaves the event channel disconnected, so `pump`'s
            // `try_recv` loop drains nothing — the same shape as a
            // spawn failure, minus the error log.
            drop(tx);
            None
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
            #[cfg(test)]
            outbound_write_log: Mutex::new(Vec::new()),
        }
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
    /// returning the final outcome. Bounded by a wall-clock deadline so a
    /// stuck worker fails the test instead of hanging — a fixed iteration
    /// count is machine-speed dependent (10k yields burn ~1ms on an idle
    /// multicore host, far less than a debug-build replay of a 64KB+
    /// snapshot needs). Mirrors what `pump_all` does across many frames,
    /// collapsed into one synchronous call for unit tests (no real
    /// `pump_all` async loop — NFR2).
    #[cfg(test)]
    pub(crate) fn test_poll_until_swapped(&mut self) -> SwapOutcome {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            match self.poll_pending_switch() {
                SwapOutcome::Pending => {
                    if std::time::Instant::now() >= deadline {
                        panic!("off-thread replay worker did not complete in time");
                    }
                    std::thread::yield_now();
                }
                other => return other,
            }
        }
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

    /// Test-only (tmux-startup-query-response-leak task0001): every payload
    /// recorded by [`Self::write`] / [`Self::send_control`] so far — see
    /// [`Self::outbound_write_log`]'s doc for what "payload" means in each
    /// case.
    #[cfg(test)]
    pub(crate) fn test_outbound_writes(&self) -> Vec<Vec<u8>> {
        self.outbound_write_log.lock().clone()
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
            let effective_target = pending
                .pending_resize
                .unwrap_or((pending.cols, pending.rows));
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

#[cfg(test)]
mod tests;
