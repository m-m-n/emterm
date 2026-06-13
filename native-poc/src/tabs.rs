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
        spawn_overrides: Option<crate::profiles::SpawnOverrides>,
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
            folds: Self::new_fold_manager(fold_enabled),
            fold_enabled,
            pending_fold_begin: None,
            evicted_baseline: 0,
            pending_eviction_delta: 0,
            pending_frame_reset: false,
            ssh_connection_name,
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
                // Fold regions key off the same absolute frame as the prompt
                // marks, so a snapshot replay (which rebuilds that frame from
                // scratch) must discard them too — `backfill_prompt_marks`
                // re-registers any C→D pairs the replay carries, and
                // `backfill_fold_marks` re-registers any begin→end pairs.
                // Rebuild with the tab's fold-enable preference so a disabled
                // tab does not silently re-enable folding after a replay.
                self.folds = Self::new_fold_manager(self.fold_enabled);
                // Any in-flight custom-fold `begin` belonged to the old frame;
                // the replay rebuilds the frame from scratch, so drop it.
                self.pending_fold_begin = None;
                let (actions, evicted_total, pending_marks, pending_fold_marks) = {
                    let mut c = self.core.lock();
                    let actions = c.reset_and_replay(&msg.payload);
                    let (evicted_total, pending_marks, pending_fold_marks) = drain_marks(&mut c);
                    (actions, evicted_total, pending_marks, pending_fold_marks)
                };
                self.evicted_baseline = evicted_total;
                self.backfill_marks(evicted_total, pending_marks, pending_fold_marks);
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
                let (actions, evicted_total, pending_marks, pending_fold_marks) = {
                    let mut c = self.core.lock();
                    let actions = c.process_pty_data_fully(&msg.payload);
                    let (evicted_total, pending_marks, pending_fold_marks) = drain_marks(&mut c);
                    (actions, evicted_total, pending_marks, pending_fold_marks)
                };
                // Same drain/backfill as the native `pump` path so prompt
                // marks and custom-fold begin/end pairs arriving over the mux
                // stream are navigable / foldable too.
                self.backfill_marks(evicted_total, pending_marks, pending_fold_marks);
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
            // and read the current eviction total — all under the core lock
            // so they are consistent with the bytes just processed. The
            // actual backfill runs after `drop(c)` because it needs
            // `&mut self`.
            let (evicted_total, pending_marks, pending_fold_marks) = drain_marks(&mut c);
            drop(c);
            self.backfill_marks(evicted_total, pending_marks, pending_fold_marks);
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
        let command_text = match b_row {
            Some(row) => self.extract_line_text(row),
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

    /// Test-only helper that runs the prompt-mark backfill (the production
    /// `pump` step that records the scrollback-eviction delta / frame-reset
    /// latch). Lets cross-module tests in `app` drive the eviction
    /// bookkeeping without exposing the otherwise-private backfill method.
    #[cfg(test)]
    pub(crate) fn test_backfill_eviction(&mut self, evicted_total: u64) {
        self.backfill_prompt_marks(evicted_total, Vec::new());
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        if let Some(p) = &self.pty {
            p.resize(cols, rows);
        }
        self.core.lock().resize(cols, rows);
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
}
