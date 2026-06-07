//! Tab type and lifecycle.
//!
//! Phase 6 swap: `Parser + Grid` (the Phase 1 PoC stand-ins) are replaced
//! by `term_core::TerminalCore`. Incoming PTY bytes are pushed through
//! `process_pty_data`; the grid state is read via `get_cell_*` /
//! `get_cursor_*` accessors. OSC titles and emterm-extension dispatches
//! are delivered through the shared `NativeCallbackState`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crossbeam_channel::Receiver;
use parking_lot::Mutex;
use term_core::terminal_core::TerminalCore;
use term_images::image_proc::{ImageEvent, ImageProcessor};

use crate::callbacks::{EmtermOscRequest, NativeCallbackState, NativeCallbacks};
use crate::image::{parse as image_parse, split_image_events};
use crate::pty::{ExitReason, PtyEvent, PtySession};
use crate::render::theme::Theme;
use crate::settings::Settings;
use mux_ipc::protocol::{MessageType, MuxMessage, StatusUpdateMsg, WelcomeMsg};

// Default scrollback capacity now lives on `Settings` (`DEFAULT_SCROLLBACK_LINES`
// in `crate::settings`); the caller passes the desired value into
// `Tab::spawn_shell`.

/// Monotonic counter backing [`Tab::stable_id`]. Process-lifetime unique;
/// `Relaxed` suffices because the id only needs uniqueness, not ordering
/// with other memory operations.
static NEXT_TAB_STABLE_ID: AtomicU64 = AtomicU64::new(0);

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
    /// Whether the alternate screen buffer is currently active for this tab.
    /// Updated by `pump()` after draining `core.take_mode_actions()`.
    pub alt_screen: bool,
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
    /// Phase 4-C: most recent status-update payload received from the
    /// daemon. Phase 4-D's status-bar widget (`ui::status_bar`) reads
    /// this through `App::status_bar_state()`.
    pub mux_status_state: Option<StatusUpdateMsg>,
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
    /// Last observed `TerminalCore::get_scrollback_evicted_total`. `pump`
    /// reads the live counter, and when it advanced past this baseline it
    /// calls `prompts.prune_before_line(delta)` (shifting stored rows down
    /// by the number of newly-evicted scrollback lines) and updates this.
    evicted_baseline: u64,
}

/// Mode action codes emitted by `TerminalCore` after CSI ?47h / ?47l /
/// ?1047h / ?1047l / ?1049h / ?1049l. Kept in sync with
/// `crates/term_core/src/csi_modes.rs`.
const MODE_ACTION_SWITCH_TO_ALT: u8 = 1;
const MODE_ACTION_SAVE_AND_SWITCH_TO_ALT: u8 = 2;
const MODE_ACTION_SWITCH_TO_MAIN: u8 = 3;
/// TS_FALLBACK escape codes — followed by 2 payload bytes. Skipped when
/// scanning for buffer-switch markers.
const MODE_ACTION_TS_FALLBACK_A: u8 = 0xFF;
const MODE_ACTION_TS_FALLBACK_B: u8 = 0xFE;

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
    ) -> Self {
        // bounded(4096): allows up to ~64MB of in-flight PTY chunks
        // (4096 × 16KB reader buffer) before the reader thread blocks
        // on send. The previous 256-slot queue (~4MB) made bursty
        // producers like `seq 1 10000000` slow to a crawl because the
        // shell side filled the queue inside a few milliseconds and
        // then had to wait for the main thread to drain one frame's
        // worth (12 ms budget) at a time. The trade-off is up to 64MB
        // of resident memory per tab during a sustained burst.
        let (tx, rx) = crossbeam_channel::bounded::<PtyEvent>(4096);
        let pty =
            match PtySession::spawn(cols, rows, tx, &settings.shell_path, &settings.shell_args) {
                Ok(p) => Some(p),
                Err(e) => {
                    log::error!("failed to spawn shell PTY: {e}");
                    None
                }
            };

        // Construct the core and install our native callbacks.
        let mut core = TerminalCore::new(cols, rows, scrollback_lines);
        // Seed `cursor_blink` from settings before OSC / DECTCEM
        // sequences have had a chance to override it; the default
        // inside `TerminalCore` is `true`, so this only matters when
        // `settings.json` opts out (`"cursor_blink": false`).
        core.set_cursor_blink(settings.cursor_blink);
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
            alt_screen: false,
            image_proc: ImageProcessor::new(),
            pending_image_events: Vec::new(),
            mux_session_name: None,
            mux_status_state: None,
            preedit_state: crate::ime::preedit::State::default(),
            bell_pending: false,
            output_pending: false,
            activity: crate::notifications::TabActivityState::default(),
            prompts: crate::prompts::PromptTracker::default(),
            evicted_baseline: 0,
        }
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
                // `reset_and_replay` discards all scrollback and zeroes the
                // eviction counter, so the absolute line frame is rebuilt
                // from scratch. Drop the resolved tracker, take a fresh
                // eviction baseline, then backfill the replayed bytes' marks
                // — each captured by `term_core` during the replay with its
                // own emit-time row, so the whole snapshot's history no
                // longer collapses onto one line.
                self.prompts.clear();
                let (actions, evicted_total, pending_marks) = {
                    let mut c = self.core.lock();
                    let actions = c.reset_and_replay(&msg.payload);
                    (
                        actions,
                        c.get_scrollback_evicted_total(),
                        c.take_prompt_marks(),
                    )
                };
                self.evicted_baseline = evicted_total;
                self.backfill_prompt_marks(evicted_total, pending_marks);
                // A snapshot captured while a full-screen app was running
                // carries its buffer-switch sequences; mirror the replayed
                // end state onto the tab flag.
                if let Some(new_alt) = parse_alt_screen_action(&actions) {
                    self.alt_screen = new_alt;
                }
                log::debug!(
                    "mux apc: applied {:?} ({} bytes) for tab {:?}",
                    msg.msg_type,
                    msg.payload.len(),
                    self.title
                );
                true
            }
            MessageType::PtyOutput => {
                // The daemon's continuous PTY stream: feed it into term_core
                // as a normal byte stream (NOT a reset). Without this the
                // mux session looks frozen after the initial Snapshot.
                let (actions, evicted_total, pending_marks) = {
                    let mut c = self.core.lock();
                    let actions = c.process_pty_data_fully(&msg.payload);
                    (
                        actions,
                        c.get_scrollback_evicted_total(),
                        c.take_prompt_marks(),
                    )
                };
                // Same drain/backfill as the native `pump` path so prompt
                // marks arriving over the mux stream are navigable too.
                self.backfill_prompt_marks(evicted_total, pending_marks);
                if let Some(new_alt) = parse_alt_screen_action(&actions) {
                    self.alt_screen = new_alt;
                }
                true
            }
            MessageType::StatusUpdate => match msg.decode_payload::<StatusUpdateMsg>() {
                Some(payload) => {
                    self.mux_status_state = Some(payload);
                    true
                }
                None => {
                    log::warn!("mux apc: malformed StatusUpdate payload");
                    false
                }
            },
            MessageType::Welcome => match msg.decode_payload::<WelcomeMsg>() {
                Some(WelcomeMsg::Accepted { sessions, .. }) => {
                    let active = sessions.first().map(|s| s.name.clone());
                    if let Some(name) = active {
                        log::info!("mux apc: tab {:?} attached to session {name}", self.title);
                        self.mux_session_name = Some(name);
                        true
                    } else {
                        false
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
            MessageType::PtyExited => {
                log::info!("mux apc: remote pane exited for tab {:?}", self.title);
                false
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
        if !combined.is_empty() {
            let mut c = self.core.lock();
            let actions = c.process_pty_data_fully(&combined);
            // Force-flush any grapheme cluster left buffered by the
            // parser (e.g. a lone emoji codepoint at the tail of an
            // IME-commit echo). Without this the cluster sits in
            // `grapheme_buffer` until the next non-extending codepoint
            // arrives, so the glyph stays invisible and the cursor
            // doesn't advance until the user types something else
            // (typical symptom: SKK `/smile` → 😄 only appears after
            // pressing space).
            c.flush_grapheme_buffer();
            // Drain the OSC 133 marks `term_core` captured during this pump
            // (each already stamped with its emit-time row + eviction count)
            // and read the current eviction total — both under the core lock
            // so they are consistent with the bytes just processed. The
            // actual backfill runs after `drop(c)` because it needs
            // `&mut self`.
            let pending_marks = c.take_prompt_marks();
            let evicted_total = c.get_scrollback_evicted_total();
            drop(c);
            self.backfill_prompt_marks(evicted_total, pending_marks);
            if let Some(new_alt) = parse_alt_screen_action(&actions) {
                self.alt_screen = new_alt;
            }
            changed = true;
            // New PTY bytes reached the core — latch for the
            // inactive-tab activity path (WebView `onOutputActivity`).
            self.output_pending = true;
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
            let (image_apc, mux_messages) = partition_apc_for_mux(pending_apc);
            for msg in mux_messages {
                if self.apply_mux_message(msg) {
                    changed = true;
                }
            }
            if (!image_apc.is_empty() || !pending_dcs.is_empty())
                && self.drain_and_decode_images(&image_apc, &pending_dcs)
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
            self.prompts.clear();
            self.evicted_baseline = evicted_total;
        } else {
            // Shift previously-stored rows down by however many oldest
            // scrollback rows were dropped since the last observation.
            let delta = evicted_total - self.evicted_baseline;
            if delta > 0 {
                self.prompts
                    .prune_before_line(u32::try_from(delta).unwrap_or(u32::MAX));
                self.evicted_baseline = evicted_total;
            }
        }
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
            self.prompts.push(crate::prompts::ResolvedPromptMark {
                kind,
                row,
                exit_code: m.exit_code,
            });
        }
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
    /// `WindowHost` calls this once per frame and forwards the events to
    /// `ImageLayer::ingest`, which handles GPU texture upload + LRU
    /// eviction.
    pub fn drain_image_events(&mut self) -> Vec<ImageEvent> {
        std::mem::take(&mut self.pending_image_events)
    }

    pub fn write(&self, bytes: Vec<u8>) {
        if let Some(p) = &self.pty {
            p.write(bytes);
        }
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        if let Some(p) = &self.pty {
            p.resize(cols, rows);
        }
        self.core.lock().resize(cols, rows);
    }

    /// Drain queued emterm OSC viewer-spawn requests. Phase 5+ feeds these
    /// into the Wry viewer spawner.
    pub fn drain_osc(&self) -> Vec<EmtermOscRequest> {
        std::mem::take(&mut self.cb_state.lock().osc_queue)
    }
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
        if payload.starts_with(b"emterm-mux;") {
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

/// Scan a `take_mode_actions()` payload for buffer-switch markers and
/// return the resulting alt-screen state, if any. Returns `None` when no
/// buffer-switch action is present (the caller keeps the previous flag).
///
/// The payload is a byte slice of action codes; TS_FALLBACK markers
/// (`0xFF` / `0xFE`) introduce a 3-byte entry (marker + mode_lo + mode_hi)
/// and must be skipped.
fn parse_alt_screen_action(actions: &[u8]) -> Option<bool> {
    let mut last: Option<bool> = None;
    let mut i = 0;
    while i < actions.len() {
        let code = actions[i];
        if code == MODE_ACTION_TS_FALLBACK_A || code == MODE_ACTION_TS_FALLBACK_B {
            i += 3;
            continue;
        }
        match code {
            MODE_ACTION_SWITCH_TO_ALT | MODE_ACTION_SAVE_AND_SWITCH_TO_ALT => {
                last = Some(true);
            }
            MODE_ACTION_SWITCH_TO_MAIN => {
                last = Some(false);
            }
            _ => {}
        }
        i += 1;
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;
    use term_core::terminal_core::PendingPromptMark;

    #[test]
    fn parse_alt_screen_switch_to_alt() {
        assert_eq!(parse_alt_screen_action(&[1]), Some(true));
        assert_eq!(parse_alt_screen_action(&[2]), Some(true));
    }

    #[test]
    fn parse_alt_screen_switch_to_main() {
        assert_eq!(parse_alt_screen_action(&[3]), Some(false));
    }

    #[test]
    fn parse_alt_screen_empty_returns_none() {
        assert_eq!(parse_alt_screen_action(&[]), None);
    }

    #[test]
    fn parse_alt_screen_skips_ts_fallback() {
        // 0xFF / 0xFE are followed by two mode bytes.
        assert_eq!(parse_alt_screen_action(&[0xFF, 0x01, 0x00]), None);
        assert_eq!(parse_alt_screen_action(&[0xFE, 0x07, 0x00, 1]), Some(true));
    }

    #[test]
    fn parse_alt_screen_takes_last_seen() {
        // Within one chunk: enter alt then leave → last is `false`.
        assert_eq!(parse_alt_screen_action(&[2, 3]), Some(false));
        // Enter alt twice → last is `true`.
        assert_eq!(parse_alt_screen_action(&[3, 2]), Some(true));
    }

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
        )
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
}
