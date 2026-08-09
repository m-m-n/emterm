//! Frame pacing and redraw-decision helpers: frame skip, wait deadlines,
//! control-flow selection, dirty-row resolution, the resize settler, and
//! the `EMTERM_RENDER_PERF` frame / rebuilt-row counters.

use std::time::{Duration, Instant};

use winit::event_loop::ControlFlow;

use crate::app::App;

/// Sub-phase 2 dirty-row skip decision (task0002 AC-5): extracted from
/// `WindowHost::render` as a pure function — plain values in, plain bool
/// out, no window/app/egui types — so it is directly unit-testable.
///
/// `dirty_count` is `None` when there is no active tab (the hint-message
/// frame always proceeds so it can draw); `Some(n)` is
/// `App::dirty_rows_this_frame(..).len()`. `status_bar_changed` is
/// `App::status_bar_view_model_changed()` — the carve-out that keeps the
/// status bar's own wake chain (clock tick, git branch, OSC 777 push)
/// live even when the terminal grid itself is quiescent.
///
/// `overlay_work` is `true` when a restart/SFTP toast is counting down to
/// auto-dismiss, a visual-bell flash is still decaying, the search UI is
/// visible, or the one-shot bell-erase-frame signal is pending — any of
/// these needs the egui pass (`pump_sftp` / toast prune / bell paint /
/// search overlay) to run every frame, the same carve-out
/// `status_bar_changed` gets for the status bar's own wake chain.
///
/// `egui_input_pending` is `true` when `pending_egui_events` holds input
/// (a click, wheel, or key destined for the egui chrome) that no egui pass
/// has consumed yet. Those events are drained only by `build_raw_input`,
/// which runs *after* this decision — skipping such a frame would leave
/// the click queued until the next unrelated wakeup (worst case the next
/// blink flip, ~530 ms), which is exactly the sluggish tab-switch the
/// post-merge report described. Any pending egui input therefore vetoes
/// the skip.
///
/// Returns `true` (skip the frame) only when the dirty count is known to
/// be exactly zero AND the status bar did not change AND there is no
/// pending overlay work AND no egui input is waiting; every other
/// combination proceeds to a full frame.
pub(super) fn should_skip_frame(
    dirty_count: Option<usize>,
    status_bar_changed: bool,
    overlay_work: bool,
    egui_input_pending: bool,
) -> bool {
    matches!(dirty_count, Some(0)) && !status_bar_changed && !overlay_work && !egui_input_pending
}

/// Whether `about_to_wait` should request a redraw on behalf of an active
/// toast this turn: a toast is up AND at least [`crate::app::TOAST_POLL_MS`]
/// has elapsed since the last toast-driven request (`None` = no request was
/// made yet, so the first one fires immediately). This is the rate limit
/// that keeps the toast-driven `request_redraw` → `RedrawRequested` →
/// `about_to_wait` cycle at the poll cadence instead of spinning at full
/// speed under a non-blocking present mode. Extracted as a pure function —
/// plain values in, plain bool out — so it is directly unit-testable
/// (mirrors [`should_skip_frame`] above).
pub(super) fn toast_redraw_due(
    toast_pending: bool,
    last_redraw: Option<Instant>,
    now: Instant,
) -> bool {
    toast_pending
        && last_redraw.is_none_or(|last| {
            now.duration_since(last) >= Duration::from_millis(crate::app::TOAST_POLL_MS)
        })
}

/// Minimum real wall-clock interval between resize-settle self-wake-driven
/// redraw requests (mux-tab-switch-bypass-refix task0002 Change 1, finding
/// `81507f39e384b34e`). The unconditional per-frame wake this replaces
/// re-enters `RedrawRequested` immediately (never reaching the `WaitUntil`
/// timer — see `last_toast_redraw`'s doc comment for the same mechanism),
/// spinning the render loop at full speed for up to
/// `RESIZE_SETTLE_MAX_DURATION` on every startup/reattach/tab-switch.
/// Set equal to [`RESIZE_SETTLE_QUIET_DURATION`] (64 ms): any interval at
/// or below the quiescence window bounds the wake rate far below a
/// display's full frame rate (NFR2) while still letting the settler
/// observe a stable candidate long enough to detect quiescence well
/// within `RESIZE_SETTLE_MAX_DURATION`.
pub(super) const RESIZE_SETTLE_SELF_WAKE_INTERVAL: Duration = RESIZE_SETTLE_QUIET_DURATION;

/// Whether the render loop should request another redraw on behalf of an
/// open [`ResizeSettler`] settling window this turn: the window is still
/// awaiting a decision AND at least [`RESIZE_SETTLE_SELF_WAKE_INTERVAL`]
/// has elapsed since the last self-wake-driven request (`None` = no
/// self-wake has fired yet in this window, so the first one fires
/// immediately). Mirrors [`toast_redraw_due`]'s form exactly — plain
/// values in, plain bool out — so it is directly unit-testable. Consulted
/// from both [`WindowHost::refresh_status_bar_insets`] (the fast path,
/// when render() is already running) and `PocApp::about_to_wait` (the
/// fallback path, when [`next_resize_settle_wake_deadline`]'s `WaitUntil`
/// is what re-enters the loop with no other activity at all) — both read
/// the same `last_resize_settle_wake` state, so whichever runs first for a
/// given tick naturally gates the other out.
pub(super) fn resize_settle_self_wake_due(
    awaiting_decision: bool,
    last_self_wake: Option<Instant>,
    now: Instant,
) -> bool {
    awaiting_decision
        && last_self_wake
            .is_none_or(|last| now.duration_since(last) >= RESIZE_SETTLE_SELF_WAKE_INTERVAL)
}

/// When `about_to_wait` should next re-check
/// [`resize_settle_self_wake_due`], for `control_flow_for`'s `WaitUntil`
/// scheduling — mirrors `App::next_toast_deadline`'s role for the toast
/// poll cadence. Folding this into the event loop's `WaitUntil` deadline
/// is what guarantees the resize-settle self-wake keeps arriving even on a
/// fully idle window (no ime/pty/search/blink/bell/toast activity ever
/// feeding the loop): an unconditional `request_redraw()` call bypasses
/// `WaitUntil` entirely (see `RESIZE_SETTLE_SELF_WAKE_INTERVAL`'s doc
/// comment), so without this deadline a rate-limited gate alone would
/// strand a pending settle exactly like round-1's findings
/// `02546e5e10deb500` / `5b1878c41d3e02d6-perf-P2` — which this must not
/// reintroduce. `None` when the settling window is already closed (nothing
/// to wake for).
pub(super) fn next_resize_settle_wake_deadline(
    awaiting_decision: bool,
    now: Instant,
) -> Option<Instant> {
    awaiting_decision.then(|| now + RESIZE_SETTLE_SELF_WAKE_INTERVAL)
}

/// Whether the cached status-bar drawing insets need to be (re)applied
/// this frame: the candidate top/bot values differ from what's currently
/// cached at all (exact comparison). Change 2 (mux-tab-switch-bypass-refix
/// task0002, findings `a82206113b8160fd` / `aba5ebbdf9a9addb`): extracted
/// as a pure function — plain values in, plain bool out — so the fix
/// (apply insets on value change, independent of `ResizeSettler`'s
/// grid-size decision) is directly unit-testable. D-D (IMPLEMENTATION.md):
/// applying the insets here is deliberately independent of
/// `pending_resize`, which stays gated on the settler's forwarded decision
/// only.
///
/// task0006 (finding `869ddd643c123a44`): an earlier version used an
/// absolute `f32::EPSILON` tolerance intended to avoid float-noise churn,
/// mirroring [`WindowHost::refresh_mux_sidebar_inset`]'s own epsilon-style
/// check for its inset. That tolerance was dead weight: at real inset
/// magnitudes (tens of logical px) one ULP already exceeds
/// `f32::EPSILON`, so no representable perturbation ever fell inside the
/// band and the comparison was already exact in practice. This makes the
/// exactness explicit instead of carrying a threshold that never
/// triggers.
pub(super) fn status_bar_insets_changed(
    current_top: f32,
    current_bot: f32,
    candidate_top: f32,
    candidate_bot: f32,
) -> bool {
    current_top != candidate_top || current_bot != candidate_bot
}

/// Whether `events` contains at least one egui event that must veto the
/// idle-skip decision above. `egui::Event::PointerMoved` is deliberately
/// excluded: `PointerMoved` pushes one unconditionally on every mouse
/// motion, so treating it as actionable would force a full egui+GPU frame
/// on every hover pixel over an otherwise idle terminal. A click still
/// vetoes because it arrives as `[PointerMoved, PointerButton]` — the
/// trailing `PointerButton` is not excluded — so click responsiveness is
/// unaffected; only chrome hover feedback for motion-only frames is
/// deferred until the next discrete event or a content change.
///
/// Exception: while a pointer button is held (`pointer_button_held`),
/// motion IS actionable — egui chrome drags (scrollbar thumb, tab
/// reorder) live entirely in the press→release motion stream, and
/// skipping those frames would freeze the drag's live tracking on an
/// idle terminal until the release finally vetoes.
pub(super) fn has_actionable_egui_input(events: &[egui::Event], pointer_button_held: bool) -> bool {
    if pointer_button_held {
        !events.is_empty()
    } else {
        events
            .iter()
            .any(|e| !matches!(e, egui::Event::PointerMoved(_)))
    }
}

/// The dirty-row snapshot the grid build may trust, given whether a full
/// redraw was raised since (or survived past) the frame-top snapshot —
/// extracted as a pure function (plain values in, plain values out)
/// mirroring `should_skip_frame` above.
///
/// The snapshot in `frame_dirty_rows` is taken at the top of `render` for
/// the skip decision, but the egui pass in the middle of the frame can
/// apply events that invalidate it: a tab-bar click switches the active
/// tab (the snapshot then indexes a *different* tab's core), a scrollbar
/// jump moves the viewport. Those paths call `App::mark_full_redraw`, so
/// "the flag is set at build time" is exactly the signal that the
/// snapshot is stale. Returning `None` routes both build branches to
/// their existing every-row path (`None` already means "forced full
/// redraw" there), rebuilding the whole cache against the current state.
///
/// Without this, the frame paints the new tab's dirty rows over the
/// previous tab's cached rows, and `record_render_state` then consumes
/// the mid-frame flag at end of frame — leaving the mixed content on
/// screen indefinitely (the post-merge "switching tabs keeps the old
/// tab's output, only the prompt row updates" report).
pub(super) fn resolve_build_dirty_rows(
    snapshot: Option<Vec<u16>>,
    full_redraw_pending: bool,
) -> Option<Vec<u16>> {
    if full_redraw_pending { None } else { snapshot }
}

/// task0006: whether this frame's pending core scroll event should
/// rotate the per-row instance cache — extracted as a pure function
/// (plain values in, plain bool out) mirroring `should_skip_frame`
/// above, so the decision is directly unit-testable without a window.
///
/// `scroll_count` is `TerminalCore::get_scroll_event_count()`.
/// `partial_dirty_rows` is `true` only on the ordinary cached path,
/// where `frame_dirty_rows` names FEWER rows than the viewport's total —
/// `false` on any turn where the effective dirty set is already every
/// row (a forced full redraw: `was_surface_dirty`, `needs_full_redraw`
/// / `force_full_redraw`, a fold layout, or a scrolled-back viewport
/// reacting to new output — see `App::on_pty_output`). In every `false`
/// case the rebuild below overwrites the whole cache regardless of any
/// rotation, so rotating first would just be wasted work.
///
/// Callers still clear the core-side event whenever `scroll_count > 0`
/// regardless of this function's answer (task0006 Design:
/// "needs_full_redraw frames: full rebuild already; just clear the
/// event") — this function only gates the rotation itself.
pub(super) fn should_rotate_row_cache_for_scroll_event(
    scroll_count: u16,
    partial_dirty_rows: bool,
) -> bool {
    scroll_count > 0 && partial_dirty_rows
}

/// The dirty-row set fed to [`crate::render::terminal_grid_pass::
/// TerminalGridPass::rebuild_and_collect`] during an IME-preedit-active
/// frame (task0003 High finding fix): extracted as a pure function —
/// plain values in, plain `Vec<u16>` out, no window/app/core types — so
/// every combination is directly unit-testable, mirroring
/// `should_skip_frame` above.
///
/// Starts from `frame_dirty_rows` (the same set `App::dirty_rows_this_frame`
/// already computed this turn) or every row `0..row_count` when `None` (a
/// forced full redraw). The preedit anchor row and the row immediately
/// below it (composition may wrap) are then force-included even if
/// `term_core` itself considers them clean — otherwise `row_cache` would
/// keep whatever content those rows had *before* preedit started, one
/// frame stale, the moment preedit ends. The result is sorted ascending
/// and deduplicated, matching the invariant `rebuild_dirty_rows` requires.
pub(super) fn preedit_effective_dirty_rows(
    frame_dirty_rows: Option<Vec<u16>>,
    row_count: u16,
    anchor_row: u16,
) -> Vec<u16> {
    let mut rows: Vec<u16> = frame_dirty_rows.unwrap_or_else(|| (0..row_count).collect());
    for r in [anchor_row, anchor_row.saturating_add(1)] {
        if r < row_count && !rows.contains(&r) {
            rows.push(r);
        }
    }
    rows.sort_unstable();
    rows.dedup();
    rows
}

/// task0004 AC-1/AC-2: pure decision for the next winit control flow, given
/// the pending timed-work deadlines this turn observed. Extracted as a free
/// function — plain `Option<Instant>` in, plain `Option<Instant>` out, no
/// winit/App types — so every combination is directly unit-testable
/// (mirrors `should_skip_frame` above).
///
/// Each argument is `None` when that concern has no pending timed work:
/// `blink_deadline` is `None` when blink is disabled, the window is
/// unfocused, the cursor is hidden, or no tab is active
/// ([`App::next_blink_deadline`]); `bell_deadline` is `None` when no
/// visual-bell flash is decaying ([`App::next_bell_deadline`]);
/// `toast_deadline` is `None` when no restart/SFTP toast is up
/// ([`App::next_toast_deadline`]); `mux_sidebar_dim_deadline` is `None`
/// when the overlay card's dim/fade feature is settled or not shown
/// ([`App::next_mux_sidebar_dim_deadline`] — task0002 AC-5).
///
/// Returns `None` when every concern is quiescent — the caller maps this to
/// `ControlFlow::Wait` (AC-2: an idle terminal, e.g. blink disabled, never
/// reschedules a periodic wakeup). Returns the earliest deadline otherwise —
/// the caller maps this to `ControlFlow::WaitUntil`.
pub(super) fn next_wait_deadline(
    blink_deadline: Option<Instant>,
    bell_deadline: Option<Instant>,
    toast_deadline: Option<Instant>,
    mux_sidebar_dim_deadline: Option<Instant>,
) -> Option<Instant> {
    [
        blink_deadline,
        bell_deadline,
        toast_deadline,
        mux_sidebar_dim_deadline,
    ]
    .into_iter()
    .flatten()
    .min()
}

/// Compute the winit `ControlFlow` for this turn from the App's pending
/// timed-work deadlines (task0004 D4). Thin wiring around
/// [`next_wait_deadline`] (the unit-tested pure decision) — used by both
/// `PocApp::can_create_surfaces`'s initial control flow and
/// `PocApp::about_to_wait`'s end-of-turn rearm so the two follow the same
/// rule.
///
/// `resize_settle_deadline` (mux-tab-switch-bypass-refix task0002 Change
/// 1) is folded in alongside `next_wait_deadline`'s four App-owned
/// concerns rather than added as a fifth parameter to that function: the
/// resize-settle self-wake state (`ResizeSettler::awaiting_decision`)
/// lives on `WindowHost`, not `App`, so the caller computes it via
/// [`next_resize_settle_wake_deadline`] and passes the result straight
/// through. This is what lets `about_to_wait` re-enter at the self-wake
/// cadence even when every other concern (blink/bell/toast/mux-sidebar-dim)
/// is quiescent — see [`resize_settle_self_wake_due`]'s doc comment for why
/// this deadline, not just a rate-limited gate, is required.
pub(super) fn control_flow_for(app: &App, resize_settle_deadline: Option<Instant>) -> ControlFlow {
    let now = Instant::now();
    let app_deadline = next_wait_deadline(
        app.next_blink_deadline(),
        app.next_bell_deadline(),
        app.next_toast_deadline(),
        app.next_mux_sidebar_dim_deadline(now),
    );
    match [app_deadline, resize_settle_deadline]
        .into_iter()
        .flatten()
        .min()
    {
        Some(deadline) => ControlFlow::WaitUntil(deadline),
        None => ControlFlow::Wait,
    }
}

/// Frames-drawn counter for `EMTERM_RENDER_PERF=1` (task0002 AC-6).
/// Counts every frame `record_draw` is called for and reports the
/// running total at most once per second of activity, so an idle host
/// logging at 60 Hz doesn't flood `emterm.log`.
#[derive(Debug, Default)]
pub(super) struct FrameCounter {
    pub(super) drawn: u64,
    last_log_at: Option<Instant>,
}

impl FrameCounter {
    /// Record one drawn (non-skipped) frame. Returns `Some(total)` when
    /// at least a second has passed since the last reported log point
    /// (or this is the first call ever), `None` otherwise. The count
    /// itself always advances regardless of the return value.
    pub(super) fn record_draw(&mut self, now: Instant) -> Option<u64> {
        self.drawn += 1;
        let should_log = match self.last_log_at {
            None => true,
            Some(t) => now.duration_since(t) >= Duration::from_secs(1),
        };
        if should_log {
            self.last_log_at = Some(now);
            Some(self.drawn)
        } else {
            None
        }
    }
}

/// Wires the `EMTERM_RENDER_PERF` gate to [`FrameCounter`]: a no-op that
/// never touches `counter` when `enabled` is `false` (AC-6's "no
/// counting side effects" half), otherwise delegates to
/// `FrameCounter::record_draw`. Kept separate from `WindowHost::render`
/// so both halves of AC-6 are unit-testable without a window.
pub(super) fn record_drawn_frame(
    enabled: bool,
    counter: &mut FrameCounter,
    now: Instant,
) -> Option<u64> {
    if !enabled {
        return None;
    }
    counter.record_draw(now)
}

/// Rows-rebuilt counter for `EMTERM_RENDER_PERF=1` (task0003 FR6-half /
/// AC-5). Same idiom as [`FrameCounter`]: accumulates every rebuilt row
/// and reports the running total at most once per second of activity, so
/// an idle host doesn't flood `emterm.log`.
#[derive(Debug, Default)]
pub(super) struct RowsRebuiltCounter {
    pub(super) rebuilt: u64,
    last_log_at: Option<Instant>,
}

impl RowsRebuiltCounter {
    /// Record `rows` freshly rebuilt rows. Returns `Some(total)` when at
    /// least a second has passed since the last reported log point (or
    /// this is the first call ever), `None` otherwise. The running total
    /// always advances regardless of the return value.
    pub(super) fn record_rebuilt(&mut self, rows: u64, now: Instant) -> Option<u64> {
        self.rebuilt += rows;
        let should_log = match self.last_log_at {
            None => true,
            Some(t) => now.duration_since(t) >= Duration::from_secs(1),
        };
        if should_log {
            self.last_log_at = Some(now);
            Some(self.rebuilt)
        } else {
            None
        }
    }
}

/// Wires the `EMTERM_RENDER_PERF` gate to [`RowsRebuiltCounter`]: a no-op
/// that never touches `counter` when `enabled` is `false` (AC-5's "no
/// side effects when unset" half) or when `rows == 0` (a fully
/// cache-served frame has nothing to report), otherwise delegates to
/// `RowsRebuiltCounter::record_rebuilt`. Kept separate from
/// `WindowHost::render` so both halves of AC-5 are unit-testable without a
/// window, mirroring `record_drawn_frame`.
pub(super) fn record_rebuilt_rows(
    enabled: bool,
    counter: &mut RowsRebuiltCounter,
    rows: u64,
    now: Instant,
) -> Option<u64> {
    if !enabled || rows == 0 {
        return None;
    }
    counter.record_rebuilt(rows, now)
}

/// Wall-clock quiescence window [`ResizeSettler`] requires a candidate to
/// hold before considering it settled (mux-tab-switch-replay-latency
/// task0005, findings 12cac263b7dab24b / 02546e5e10deb500-c). task0002
/// originally debounced on a fixed count of consecutive `observe` calls
/// (`RESIZE_SETTLE_QUIET_OBSERVATIONS`), but `observe` is only invoked
/// from [`WindowHost::refresh_status_bar_insets`] at the head of
/// `render()` — render cadence and actual `visible_row_count` transition
/// cadence are independent, so a transient candidate held for as few as 2
/// renders (an eyeblink of wall-clock time during an active storm) was
/// indistinguishable from a genuinely settled one and got forwarded
/// mid-storm. Measuring wall-clock stability instead makes the decision
/// independent of how many renders occur while a candidate holds: `64 ms`
/// is roughly four frames at a typical 60 Hz cadence, comfortably above
/// the ~2-frame window the round-1 review measured the old bug forwarding
/// within, while still being imperceptible as a one-off startup/reattach
/// delay.
pub(super) const RESIZE_SETTLE_QUIET_DURATION: Duration = Duration::from_millis(64);

/// Hard backstop on how long [`ResizeSettler`]'s settling window may stay
/// open before forwarding the latest candidate regardless of whether it
/// has stabilized (task0005, same findings as
/// [`RESIZE_SETTLE_QUIET_DURATION`] — supersedes task0002's
/// `RESIZE_SETTLE_MAX_OBSERVATIONS`, a call-count backstop that coupled
/// the backstop to render frequency: a never-settling stream fed at a
/// high render rate could exhaust a fixed observation budget in far less
/// real time than intended). `1` second is a generous ceiling above
/// SPEC.md's measured 24-transition storm so quiescence detection alone
/// absorbs any realistic storm, while still guaranteeing (AC-2) that a
/// pathological, never-quite-settling stream cannot withhold a resize
/// forever — every daemon-side pane must eventually learn the (possibly
/// still-transient) latest size rather than be stuck at a stale one.
pub(super) const RESIZE_SETTLE_MAX_DURATION: Duration = Duration::from_secs(1);

/// Bounds how often a status-bar-height-driven grid-size candidate reaches
/// [`Tab::resize`](crate::tabs::Tab::resize)'s group-wide `Resize`
/// broadcast while the status bar's `visible_row_count` is still settling
/// right after mux attach/reattach (FR6, mux-tab-switch-replay-latency
/// task0002 — unrelated to this file's other, pre-existing "task0002"
/// references, which belong to an earlier, different feature).
///
/// task0005 rework (findings 12cac263b7dab24b / 02546e5e10deb500-c /
/// 02546e5e10deb500 / 5b1878c41d3e02d6-perf-P2): two defects in the
/// original design compounded to defeat FR6 entirely for the SPEC's
/// measured 2-state storm, plus a third that stranded a pending decision:
///
/// 1. **Render-count debounce.** `observe` used to settle after
///    `RESIZE_SETTLE_QUIET_OBSERVATIONS` consecutive CALLS agreeing — but
///    calls happen once per `render()`, not once per actual
///    `visible_row_count` change, so a transient candidate held for only
///    2 renders looked identical to a genuinely settled one. Fixed by
///    measuring wall-clock stability ([`RESIZE_SETTLE_QUIET_DURATION`])
///    instead — independent of how many renders occur while a candidate
///    holds.
/// 2. **Caller-side filtering bias.** `refresh_status_bar_insets` used to
///    call `observe` only when the computed inset differed from the
///    currently-applied one, so whichever side of a 2-state oscillation
///    matched the still-applied value was never observed at all —
///    biasing the settler toward the other state and defeating
///    quiescence detection for exactly the storm shape SPEC.md measures.
///    Fixed by feeding every computed candidate on every render
///    unconditionally; `observe` now tracks the most recently forwarded
///    (or already-applied) size itself ([`Self::last_forwarded`]) and
///    no-ops when a candidate merely repeats it, so the caller no longer
///    needs to filter anything upstream.
/// 3. **No self-wake.** When `observe` returned `None` (still
///    debouncing), nothing scheduled the next render — a fully idle
///    window (no ime/pty/search/blink/bell/toast activity) right after a
///    status-bar height change could strand the pending candidate
///    indefinitely. Fixed by [`Self::awaiting_decision`]:
///    `refresh_status_bar_insets` requests a redraw whenever it is
///    `true`, so the settler drives its own next observation until it
///    decides, bounded by [`RESIZE_SETTLE_QUIET_DURATION`] /
///    [`RESIZE_SETTLE_MAX_DURATION`].
///
/// Once the window closes (forwards), every subsequent distinct candidate
/// is forwarded immediately — identical to the pre-fix, undebounced
/// behavior — so an ordinary, isolated resize well after startup is not
/// perceptibly delayed (AC-3).
///
/// [`Self::reset`] reopens a closed window; `WindowHost` calls it when the
/// active tab's `mux_session_name` transitions from absent to present (a
/// fresh mux attach/reattach). `last_forwarded` is intentionally NOT
/// cleared by `reset` — it still reflects the last size actually applied,
/// and a fresh storm is judged against that, not against nothing.
#[derive(Debug)]
pub(super) struct ResizeSettler {
    /// `Some(instant)` while the settling window is open, holding the
    /// wall-clock instant it opened (for [`RESIZE_SETTLE_MAX_DURATION`]'s
    /// backstop); `None` once closed (normal, immediate-forwarding mode).
    pub(super) window_opened_at: Option<Instant>,
    /// The candidate currently accumulating wall-clock stability while the
    /// window is open.
    pub(super) candidate: Option<(u16, u16)>,
    /// The wall-clock instant `candidate` was last seen to CHANGE — i.e.
    /// the instant since which it has held. Reset whenever a new,
    /// distinct candidate arrives.
    pub(super) stable_since: Option<Instant>,
    /// The most recently forwarded (or otherwise already-applied) grid
    /// size — compared against every observed candidate, even while the
    /// window is open, so a render that merely repeats the value already
    /// applied is never re-forwarded (02546e5e10deb500-c).
    pub(super) last_forwarded: Option<(u16, u16)>,
}

impl ResizeSettler {
    pub(super) fn new() -> Self {
        Self {
            window_opened_at: Some(Instant::now()),
            candidate: None,
            stable_since: None,
            last_forwarded: None,
        }
    }

    /// Reopen the settling window (a fresh mux attach/reattach).
    pub(super) fn reset(&mut self) {
        self.window_opened_at = Some(Instant::now());
        self.candidate = None;
        self.stable_since = None;
    }

    /// Feed one candidate grid size, computed unconditionally on every
    /// render regardless of whether it differs from the currently-applied
    /// size (task0005 finding 02546e5e10deb500-c — the caller must NOT
    /// filter upstream; `observe` decides everything itself). Returns
    /// `Some(size)` exactly when `size` should now reach `Tab::resize`;
    /// `None` while still absorbing transient churn, or when `size` is a
    /// no-op repeat of what is already applied.
    pub(super) fn observe(&mut self, candidate: (u16, u16), now: Instant) -> Option<(u16, u16)> {
        let Some(opened_at) = self.window_opened_at else {
            // Closed: forward only genuine changes, so a steady-state
            // repeat of the already-applied size never re-triggers
            // Tab::resize's broadcast every render.
            if self.last_forwarded == Some(candidate) {
                return None;
            }
            self.last_forwarded = Some(candidate);
            return Some(candidate);
        };

        match self.candidate {
            Some(c) if c == candidate => {}
            _ => {
                self.candidate = Some(candidate);
                self.stable_since = Some(now);
            }
        }
        let stable_since = self.stable_since.unwrap_or(now);
        let settled = now.duration_since(stable_since) >= RESIZE_SETTLE_QUIET_DURATION;
        let capped = now.duration_since(opened_at) >= RESIZE_SETTLE_MAX_DURATION;
        if !(settled || capped) {
            return None;
        }
        self.window_opened_at = None; // close the window
        if self.last_forwarded == Some(candidate) {
            return None;
        }
        self.last_forwarded = Some(candidate);
        Some(candidate)
    }

    /// Whether the settling window is currently open — i.e. whether
    /// [`WindowHost::refresh_status_bar_insets`] must request another
    /// redraw so this settler keeps getting observations even if nothing
    /// else would wake the render loop (task0005 findings
    /// 02546e5e10deb500 / 5b1878c41d3e02d6-perf-P2).
    pub(super) fn awaiting_decision(&self) -> bool {
        self.window_opened_at.is_some()
    }
}
