//! Top-level `App` state container.
//!
//! Phase 1 was empty. Phase 2 holds a PTY-backed tab and offers an
//! `on_resize` hook that propagates the new grid dimensions to all PTYs.
//! Sub-phase 2 adds dirty-row tracking state (cursor + selection history,
//! `needs_full_redraw` flag, `EMTERM_FULL_REDRAW` toggle) used by the
//! renderer to skip frames that wouldn't change a pixel.
//! Later phases extend this with a viewer registry and settings.

use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use term_core::terminal_core::TerminalCore;

use crate::callbacks::{NotificationSink, NotifyRustSink};
use crate::ime::backend::ImeBackend;
use crate::ime::null::NullBackend;
use crate::render::font::cache::GlyphCache;
use crate::render::font::fallback::FallbackChain;
use crate::render::font::resolver::Resolver;
use crate::render::font::traits::{FontId, GlyphRasterizer};
use crate::selection::Selection;
use crate::settings::Settings;
use crate::status_bar::StatusBarRuntime;
use crate::tabs::Tab;
use crate::ui::emoji_cache::EmojiTextureCache;

mod agent_status;
mod chooser;
mod font_settings;
mod ime;
mod mux_ui;
mod scroll_search_fold;
mod sftp;
mod tab_lifecycle;
mod timing;

/// Where the viewport currently sits relative to the live tail.
///
/// Scrollback position. Defined in [`crate::scroll`] as a layer-free value
/// type and re-exported here for backward compatibility; `mux::window_group`
/// imports it from `crate::scroll` so the pure mux model does not depend on
/// the `app` layer (no `app ↔ mux` cycle).
pub use crate::scroll::ScrollPosition;
use agent_status::{
    agent_notification_rate_limit_key, agent_status_keys_for_tab, agent_status_pane_tab_title,
    agent_status_pane_visible,
};
pub use font_settings::{
    FONT_SIZE_PT_MAX, FONT_SIZE_PT_MIN, FONT_SIZE_PT_STEP, clamp_font_size_pt,
};
use mux_ui::build_mux_latch;
pub use mux_ui::{
    MuxActionOutcome, MuxSidebarVisibility, OVERLAY_BRIGHT_HOLD, OVERLAY_DIM_FADE,
    OVERLAY_IDLE_OPACITY, mux_sidebar_grid_inset, resolve_mux_sidebar_dim_deadline,
    resolve_mux_sidebar_dim_opacity,
};
pub use scroll_search_fold::{AUTO_RESEARCH_THROTTLE, JumpDirection, auto_research_allowed};
#[cfg(test)]
use timing::RESTART_TOAST_LINGER_SECS;
pub use timing::{BELL_FLASH_MS, BLINK_HALF_MS, RestartToast};

/// Bounded poll interval the event loop keeps waking on while a restart or
/// SFTP toast is active (task0004 D4, [`App::next_toast_deadline`]). Toast
/// auto-dismiss (`pump_sftp` / `pump_restart_toast`) runs on egui's own
/// frame-time clock, which only advances when a frame actually paints;
/// nothing else schedules those intermediate frames, so this bounds the cost
/// of keeping them flowing. Matches the interval `about_to_wait`
/// unconditionally rearmed before task0004 — now scoped to only apply while
/// a toast is actually pending.
pub const TOAST_POLL_MS: u64 = 16;

pub struct App {
    pub tabs: Vec<Tab>,
    pub active: usize,
    /// Settings-window launcher: the settings UI runs in a child
    /// `--settings` process (Wry WebView), spawned on the settings icon /
    /// `Ctrl+,`. Boxed so tests can swap in a counting double instead of
    /// spawning real processes.
    pub settings_launcher: Box<dyn crate::settings_launcher::SettingsWindowLauncher>,
    /// Last known grid size in cells. Updated by `window_host` whenever the
    /// window resizes; PTYs are resized to match.
    pub cell_size: GridDims,
    /// Active mouse selection on the active tab, if any. Phase 4 owns this;
    /// later phases may move it per-tab when tabs preserve selection.
    pub selection: Option<Selection>,
    /// Absolute-row anchor of a single-click press that has not yet been
    /// upgraded to a drag-selection. Lives here (not in `WindowHost`) so the
    /// `pump_all` eviction shift treats it as absolute-row state alongside
    /// `selection` — otherwise a scrollback eviction between the press and
    /// the first drag motion would leave the anchor pointing at a stale
    /// buffer row. `None` when no press is pending.
    pub pending_selection_anchor: Option<crate::selection::Pos>,
    /// In-terminal text-search state (query, options, matches, current
    /// match cursor). App-global like [`Self::selection`]: one search
    /// overlay shared by the window. Matches are resolved against the
    /// active tab's scrollback + viewport; switching tabs closes the
    /// overlay and clears the state (see `window_host` tab-switch path)
    /// because match rows are indexed into the previous tab's buffer.
    pub search: crate::search::SearchState,
    /// Set on the frame the search overlay is (re)opened so the UI layer
    /// grabs keyboard focus + selects the existing query once, mirroring
    /// the WebView `SearchBar.show()` `focus()` + `select()`. Cleared by
    /// the renderer after it consumes the request.
    pub search_focus_request: bool,
    /// Modal profile-selector state (visibility + highlighted row).
    /// Opened by the `profile_selector` keybind; while visible the
    /// keyboard is captured in `window_host` (navigation / confirm /
    /// cancel) and never reaches the PTY.
    pub profile_selector: crate::ui::profile_selector::ProfileSelectorState,
    /// Timestamp of the last *automatic* re-search (the per-frame
    /// [`Self::auto_research_if_dirty`] path). The auto re-search is
    /// time-throttled to at most one run per [`AUTO_RESEARCH_THROTTLE`] so a
    /// burst of PTY output does not rebuild the logical-line document and
    /// recompile the match on every frame. `None` until the first auto
    /// re-search runs. User-driven [`Self::run_search`] / navigation bypass
    /// this gate entirely.
    last_auto_research: Option<Instant>,
    /// Runtime settings (ambiguous-width policy, OSC 52 policy, and
    /// future fields). Loaded from `settings.json` in Phase 7; today
    /// initialized to default. Wrapped in `Arc` so the per-tab
    /// `NativeCallbacks` can share an immutable view without copying.
    pub settings: Arc<Settings>,
    /// Markdown viewer subsystem: drains each tab's emterm OSC queue,
    /// reassembles `markdown` sessions, and spawns child viewer processes
    /// on completion. Lives on `App` so session state survives across
    /// `pump_all` passes.
    viewer_spawner: crate::viewer::ViewerSpawner,
    /// Sink the spawner emits completed render requests to — spawns a
    /// `--viewer` child process per request and reaps closed children.
    viewer_sink: crate::viewer::ProcessViewerSink,
    /// Image-viewer router: stores decoded Kitty/SIXEL images (LRU,
    /// `image_memory_quota_mb` cap) and spawns a native `--image-viewer`
    /// child window per `Place` event.
    image_viewer: crate::viewer::image::ImageViewerRouter,
    /// Resolved keyboard-chord table, parsed once from
    /// `settings.keybinds` at construction. The keyboard handler in
    /// `window_host` matches incoming events against this for tab-roster
    /// actions (`new_tab` / `close_tab` / `next_tab` / `prev_tab`) and
    /// clipboard chords (`copy` / `paste`).
    pub keybinds: crate::ui::keybinds::KeybindTable,
    /// mux prefix-key latch. Armed on the prefix chord; the follow-up key is
    /// mapped to a [`crate::mux::prefix::PrefixAction`] via the configured
    /// `mux.keybinds`. Only consulted for the active tab when it is
    /// mux-attached (has a `mux_group`). Rebuilt on `apply_settings`.
    pub mux_latch: crate::mux::prefix::Latch,
    /// Currently-open mux rename / move dialog (if any). Plain-data state
    /// so the domain layer stays free of egui widget types (the rendering
    /// for these dialogs lives in `ui::mux_dialogs`, the only place that
    /// imports `egui::Context` for them). Single field replaces the two
    /// `Option<Widget>` fields; reentry is guarded by the
    /// `MuxDialogState::Closed` variant.
    pub mux_dialog: crate::mux::dialog::MuxDialogState,
    /// Runtime open/closed flag for the mux window-list sidebar overlay
    /// (`mux.window_sidebar_overlay` placement mode). Toggled by
    /// `PrefixAction::ToggleWindowSidebar` in `dispatch_mux_action` — a
    /// strict no-op while overlay mode is off (FR4). Reset to `false` in
    /// `pump_all` when the focused tab's mux group tears down, and set to
    /// `true` in `pump_all` on the not-attached -> attached transition
    /// (task0001 FR1/FR2/FR3 — covers startup attach and any later
    /// reattach, including after an explicit close). Rendering input only;
    /// this field does not drive any drawing itself (task0005).
    mux_sidebar_overlay_open: bool,
    /// Bookkeeping for the reset/reopen above: `Some(tab_index)` when
    /// `tab_index` was the active tab AND was mux-attached as of the end of
    /// the previous `pump_all` call; `None` otherwise. A
    /// same-tab `Some -> not-attached` transition is a genuine teardown
    /// (Detach confirmed / last window's PtyExited emptied the group) and
    /// resets the overlay-open flag; a `None -> attached` transition is a
    /// fresh attach or reattach and opens it (task0001). A changed
    /// `self.active` between calls is just a tab switch and must NOT reset
    /// the flag on its own (the overlay's visibility already gates on
    /// "active tab is mux-attached" separately — see the Shared Components
    /// contract in IMPLEMENTATION.md) — though switching onto an
    /// already-attached tab does satisfy the `None -> attached` reopen rule
    /// above when this field held no value, per IMPLEMENTATION.md D2.
    active_mux_attached_prev_pump: Option<usize>,
    /// task0002 FR3: whether the pointer is currently inside the overlay
    /// card's rect. Fed by `window_host`'s `PointerMoved` handler, using
    /// the SAME hit test (`ui::mux_sidebar::point_in_sidebar`) the press
    /// and wheel routing already query, evaluated against
    /// [`crate::ui::mux_sidebar::Placement::Overlay`] — always `false`
    /// outside overlay mode (the persistent panel and hidden state never
    /// dim). Read by [`Self::resolve_mux_sidebar_opacity`].
    mux_sidebar_overlay_hovered: bool,
    /// Snapshot of [`Self::mux_sidebar_overlay_hovered`] as of the last
    /// actually-rendered frame (`record_render_state` /
    /// `record_render_state_no_tab`). [`Self::mux_sidebar_hover_changed`]
    /// compares against this so a hover-predicate transition can feed
    /// `window_host`'s `overlay_work` — mirrors
    /// `previous_status_bar_view_model`'s read-only-comparison-against-a-
    /// snapshot shape.
    mux_sidebar_hover_prev_render: bool,
    /// task0002 FR4: instant of the most recent mux window switch (next /
    /// prev / select-by-digit via [`Self::dispatch_mux_action`], or a
    /// sidebar row click via [`Self::apply_tab_event`]'s `MuxSwitch` arm).
    /// `None` before any switch has ever happened — [`resolve_mux_sidebar_dim_opacity`]
    /// treats that as "not recently switched" (FR5).
    mux_sidebar_last_switch: Option<Instant>,
    /// task0002 fade bookkeeping: the instant the bright condition (hover
    /// OR pending hold) most recently stopped holding. Seeded on
    /// construction to an instant already [`OVERLAY_DIM_FADE`] in the past
    /// so a freshly constructed app resolves directly to
    /// [`OVERLAY_IDLE_OPACITY`] (AC-1) rather than opening with a phantom
    /// fade-in; reset to `None` by [`Self::resolve_mux_sidebar_opacity`]
    /// whenever bright, and re-armed to `Some(now)` the next time it
    /// observes "not bright" (the `Option::get_or_insert` idiom — a `None`
    /// reached from there is therefore always a genuine, just-happened
    /// bright-to-dim transition, never the fresh-construction case).
    mux_sidebar_fade_started: Option<Instant>,
    /// Merged agent-status store (mux-agent-status-api task0005, SPEC FR5 /
    /// FR6 / NFR3): one [`crate::agent_status_model::AgentStatusModel`]
    /// covering both plain tabs (this tab's own OSC 777 `agent-status`
    /// events) and mux panes (daemon-pushed `AgentStatusUpdate`). Owned here
    /// so the UI (task0006) and notification (task0007) layers can read it;
    /// `pump_all` is the only writer.
    pub agent_status: crate::agent_status_model::AgentStatusModel,
    /// pane_id -> daemon-minted public pane ID (task0006 AC-5, `mux_ipc`'s
    /// "Public pane ID format" shared component). The GUI only LEARNS a
    /// pane's public ID when the daemon pushes an `AgentStatusUpdate` for
    /// it (a real report or a replay-on-attach restatement) — there is no
    /// separate wire message that hands it over. Populated alongside
    /// `Self::agent_status` in `pump_all` from the same
    /// `AgentStatusUpdateMsg` batch and cleared on the same closed-pane
    /// list, so a pane with no entry here (never reported) simply hides
    /// the sidebar's copy-to-clipboard row (`ui::mux_sidebar` AC-5).
    mux_public_pane_ids: std::collections::HashMap<u32, String>,
    /// Display locale resolved once from `settings.language` at
    /// construction (`Auto` consults the OS locale). Consumed by the
    /// desktop-notification body formatting in `pump_all`.
    pub locale: crate::i18n::Locale,
    /// Scrollback position. `Live` = auto-follow; `OffsetFromLive(n)` = the
    /// viewport is pinned `n` rows above the live tail.
    pub scroll_position: ScrollPosition,
    /// Whether the alternate screen buffer (DECSET 1049/47/1047) is active.
    /// Tracked by draining `core.take_mode_actions()` after every chunk in
    /// `Tab::pump`. While true, scrollback inputs are suppressed and the
    /// position is pinned to `Live`.
    pub alt_screen: bool,
    /// OS focus state. Updated from `WindowEvent::Focused`. WezTerm-style
    /// rendering: focused → filled block cursor (fg/bg swap in grid pass),
    /// unfocused → outline-only via the egui overlay.
    pub window_focused: bool,
    /// Reference point for cursor-blink phase computation.
    blink_started: Instant,
    /// Cursor-blink "visible" phase observed during the previous render.
    /// When the phase flips, the cursor row joins the dirty union so the
    /// renderer can paint/erase the cursor overlay.
    previous_blink_visible: bool,
    /// Start of the in-flight visual-bell flash (`bell_action =
    /// "visual"`). Set by `pump_all` when a tab drained a BEL, cleared
    /// by [`App::needs_bell_repaint`] once [`BELL_FLASH_MS`] elapsed.
    /// `render::draw_terminal` reads the decay via
    /// [`App::visual_bell_progress`].
    visual_bell_started: Option<Instant>,
    /// One-shot latch (task0005 AC-4, finding 063320466ae233fe): set by
    /// [`App::needs_bell_repaint`] in the same turn it observes the flash
    /// crossing [`BELL_FLASH_MS`] and clears `visual_bell_started`. By the
    /// time `window_host::render` runs, `visual_bell_progress()` already
    /// reads `None` (the overlay is gone), so without this signal the
    /// frame that erases the overlay would look identical to a fully idle
    /// frame and get skipped — freezing the flash at its last painted
    /// alpha instead of fading it out. Consumed (read + cleared) via
    /// [`App::take_bell_erase_pending`].
    bell_erase_pending: bool,
    /// Cursor row/col from the previous rendered frame. The renderer dirties
    /// this row so a moved cursor doesn't ghost the old position.
    previous_cursor: Option<(u16, u16)>,
    /// Selection from the previous rendered frame. Vacated selection rows
    /// must be repainted to clear highlight.
    previous_selection: Option<Selection>,
    /// `visible_start` (`scrollback_len - scroll_offset`, saturating) from
    /// the previous rendered frame. Used to translate the absolute-row
    /// [`Self::previous_selection`] back into screen rows when computing the
    /// dirty set, so the highlight cleared on the rows it occupied *last*
    /// frame even after the viewport scrolled.
    previous_visible_start: u32,
    /// Set on construction, resize, and surface recovery. Forces the next
    /// frame to repaint every row regardless of `term_core` dirty bits.
    needs_full_redraw: bool,
    /// FR4 (tab-bar scroll-into-view): one-shot signal raised when the active
    /// cell changes via the keyboard (a committed plain-tab switch through
    /// `switch_to_tab`, or a committed mux window switch in
    /// `dispatch_mux_action`). The tab strip reads it for one frame and
    /// scrolls the active visual cell into view; `window_host` clears it
    /// post-frame so it never re-fires on an unrelated repaint (e.g. after a
    /// mouse-driven scroll). Default off.
    scroll_active_tab_into_view: bool,
    /// Debug toggle (env `EMTERM_FULL_REDRAW=1`) that permanently disables
    /// the dirty-row optimization for triage.
    force_full_redraw: bool,
    /// D2 (FR3, task0003 rework): a one-shot request left by an activation
    /// path (`switch_to_tab`, the close-tab active-index fix-up, the
    /// exited-tab reap fix-up) that the now-active tab may need reconciling
    /// against `cell_size`. NOT executed synchronously where it is set — at
    /// that moment `cell_size` may still hold dims computed for the
    /// OUTGOING tab (the persistent mux sidebar's width inset depends on
    /// whether the ACTIVE tab is mux-attached). `window_host::render`
    /// consumes it via [`Self::execute_pending_reconcile`] once insets /
    /// any pending display-area resize have settled `cell_size` for the
    /// INCOMING tab. Consecutive requests before that point collapse into
    /// one (idempotent `bool`, not a queue).
    pending_active_tab_reconcile: bool,
    /// Phase 4-G: native IME backend. The App holds a `Box<dyn ImeBackend>`
    /// so the OS-specific clients (X11 / Wayland / Windows) and the
    /// passthrough `NullBackend` share the same seam. Default-constructed
    /// to `NullBackend` so unit tests do not require window / display
    /// handles; `window_host::run` replaces it with the factory-resolved
    /// backend at startup via `App::set_ime_backend`.
    ime_backend: Box<dyn ImeBackend>,
    /// Last `(row, col)` reported to `ImeBackend::notify_cursor_rect`.
    /// `None` until the first cell-position notification. Updated in
    /// `notify_cursor_rect_if_changed` so we never spam the IM server
    /// when the cursor stays put (SPEC.md FR7).
    ime_last_cursor_cell: Option<(u16, u16)>,
    /// Whether the active backend is the passthrough `NullBackend`. The
    /// event loop uses this to decide whether `WindowEvent::ReceivedImeText`
    /// should drive the commit path (NullBackend only) or be ignored
    /// (real backend, which routes commits via `ImeEvent::Commit`). This
    /// avoids double-committing the same composition (SPEC.md FR9).
    ime_is_null: bool,
    /// Whether the IME pump has already reported a budget-exceeded
    /// drop. Latched so the warn log fires at most once per process
    /// (`IME_E901`).
    ime_overflow_warned: bool,
    /// Phase 4-H (font-swash-migration FR12): font stack shared with
    /// the `TerminalGridPass`. The rasterizer is selected once at
    /// startup based on `Settings::font_engine` (Swash default /
    /// AbGlyph escape hatch). `GlyphCache` lives behind a mutex so the
    /// same handle can be reused across frames; `FallbackChain` is
    /// immutable after `new` returns and is wrapped in an `Arc` so the
    /// renderer pass can hold its own clone.
    #[allow(dead_code)]
    pub font_resolver: Arc<Resolver>,
    pub font_fallback: Arc<FallbackChain>,
    pub font_cache: Arc<Mutex<GlyphCache>>,
    pub font_rasterizer: Arc<dyn GlyphRasterizer>,
    /// `FontId` returned by the resolver for the bundled CJK base font.
    /// `TerminalGridPass` uses this as the chain root when no user font
    /// override is registered.
    #[allow(dead_code)]
    pub font_base_id: FontId,
    /// Per-cell width in egui logical pixels (1.0× scale). Computed
    /// once at startup from the base font's advance for "M" at
    /// `settings.font_size`. The renderer multiplies by
    /// `pixels_per_point` for the physical-pixel `CellMetrics` handed
    /// to wgpu. Mirrors the legacy WebView build's
    /// `ctx.measureText("M").width` so settings.json's `font_size`
    /// produces visually-matching cells across both binaries.
    pub cell_w_logical: f32,
    /// Per-cell height in egui logical pixels (1.0× scale). Computed
    /// once at startup from the base font's ascent + descent at
    /// `settings.font_size`. See [`Self::cell_w_logical`].
    pub cell_h_logical: f32,
    /// Runtime terminal font size in logical points. Seeded from
    /// `settings.font_size` at startup and mutated by the zoom keybinds
    /// (`ZoomIn` / `ZoomOut` / `ZoomReset`). Kept distinct from
    /// `settings.font_size` (the persisted baseline) so `ZoomReset` can
    /// restore the configured value and new tabs can inherit the live
    /// zoom level. `cell_w_logical` / `cell_h_logical` and every tab's
    /// `Theme::font_size_pt` are re-derived from this whenever it
    /// changes (see [`Self::set_font_size_pt`]).
    pub runtime_font_size_pt: f32,
    /// Runtime tab-bar visibility, toggled by the `ToggleTabBar`
    /// keybind. Seeded from `settings.show_tab_bar`; the renderer and
    /// the grid-size / hit-test paths read this instead of the
    /// persisted setting so the toggle takes effect without rewriting
    /// `settings.json`.
    pub show_tab_bar: bool,
    /// Color-emoji texture cache for the status bar. egui's text path
    /// (ab_glyph) cannot raster CBDT/COLR glyphs, so the widget walks
    /// each run via [`crate::ui::emoji_cache::split_segments`] and
    /// substitutes a swash-rasterized `egui::Image` for emoji spans.
    /// The cache lives behind a `Mutex` so the renderer can take it
    /// from `&App` (mirroring `font_cache`).
    pub emoji_texture_cache: Arc<Mutex<EmojiTextureCache>>,
    /// Status-bar runtime (template engine + providers + OSC
    /// dispatcher). Constructed once at startup; per-frame snapshots
    /// flow through [`App::status_bar_view_model`].
    pub status_bar_runtime: StatusBarRuntime,
    /// Snapshot of the active tab's cwd, updated once per frame in
    /// [`App::sync_active_cwd`]. The status-bar `{cwd}` /
    /// `{git_branch}` providers read this through a `CwdSource` closure
    /// so worker threads stay isolated from `App`.
    pub active_cwd: Arc<Mutex<Option<String>>>,
    /// View model rendered in the previous frame. Compared against the
    /// freshly-built model in [`App::status_bar_view_model_changed`]
    /// so the dirty-row skip path in `WindowHost::render` can bypass
    /// the early return when only the status bar (e.g. the wall clock,
    /// the git branch worker, or an OSC 777 push) changed since the
    /// last frame. Without this, the provider-owned wake chain reaches
    /// `user_event` -> `request_redraw` but the next `render()` call
    /// observes `dirty_rows_this_frame() == 0` on an idle shell and
    /// returns early, freezing the clock display.
    previous_status_bar_view_model: Option<crate::status_bar::StatusBarViewModel>,
    /// Desktop-notification surface shared with every tab's
    /// `NativeCallbacks` (OSC 9 notifications) and used directly by
    /// link handling (`WindowHost::open_file_in_editor`) to surface
    /// file-not-found / editor-launch failures to the user. Constructed
    /// once in [`App::with_settings`] as the production [`NotifyRustSink`]
    /// and cloned into each tab so a single sink instance is shared.
    ///
    /// アプリケーションドメイン外からの直接アクセスは禁止。通知送信は
    /// [`App::notify`] を経由すること。
    pub(crate) notification_sink: Arc<dyn NotificationSink>,
    /// Per-frame fold layout for the active tab, recomputed at the top of
    /// each `WindowHost::render` via [`App::refresh_fold_layout`]. `Some`
    /// only when the active tab has at least one *collapsed* region
    /// (mirroring the WebView's `getCollapsedRegions().length > 0` gate);
    /// `None` otherwise so the renderer takes the unchanged linear path.
    /// Read by `collect_cell_inputs` (cell row selection),
    /// `draw_fold_summaries` (summary overlays), and
    /// `draw_search_highlights` (fold-aware match → screen-row mapping).
    /// Building it once with `&mut self` lets the renderer query it
    /// immutably for the rest of the frame (the fold line-mapping otherwise
    /// needs `&mut FoldManager` for its collapsed cache).
    fold_layout: Option<crate::fold::FoldLayout>,
    /// In-process SFTP upload service (process manager + concurrency pool +
    /// progress/result senders). Constructed from
    /// `settings.sftp_max_concurrent_uploads`. Shared via `Arc` so worker
    /// threads spawned by `start_upload` outlive a single `pump`.
    pub sftp_service: Arc<crate::sftp::service::SftpService>,
    /// SFTP UI state (drop aggregation + overlay + dialogs + toasts).
    pub sftp_ui: crate::sftp::ui::SftpUiState,
    /// Progress receiver drained each frame by [`App::pump_sftp`].
    sftp_progress_rx: crate::sftp::service::ProgressReceiver,
    /// Duplicate-check result receiver drained each frame by [`App::pump_sftp`].
    sftp_result_rx: crate::sftp::service::ResultReceiver,
    /// Binary-mismatch restart toast (armed by a failed self-spawn, drawn by
    /// the render path, auto-dismissed by [`App::pump_sftp`]).
    pub restart_toast: RestartToast,
    /// Per-pane rate limiter for agent-status (blocked/done) desktop
    /// notifications (task0007 AC-4). Keyed by whatever stable pane
    /// identity the caller of [`App::maybe_notify_agent_transition`]
    /// supplies (a mux pane's `public_pane_id`, or a caller-chosen key for
    /// plain tabs).
    agent_notification_rate_limiter: crate::notifications::AgentNotificationRateLimiter<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridDims {
    pub cols: u16,
    pub rows: u16,
}

impl Default for GridDims {
    fn default() -> Self {
        Self { cols: 80, rows: 24 }
    }
}

impl App {
    /// Construct an `App` with built-in default settings. Retained for
    /// tests; binary callers should prefer [`App::with_settings`] so
    /// `settings.json` overrides are honored.
    pub fn new() -> Self {
        Self::with_settings(Settings::new())
    }

    /// Construct an `App` from a pre-built [`Settings`]. `main.rs` calls
    /// this with [`Settings::load_or_default`] so on-disk overrides
    /// (`~/.config/net.laser5.app.emterm/settings.json`) take effect.
    pub fn with_settings(settings: Settings) -> Self {
        let force_full_redraw = std::env::var("EMTERM_FULL_REDRAW")
            .ok()
            .map(|v| v == "1")
            .unwrap_or(false);
        if force_full_redraw {
            log::warn!("EMTERM_FULL_REDRAW=1: dirty-row optimization disabled");
        }
        // Surface the user-configured terminal settings so a
        // typo / unexpected default is visible in the log instead of
        // silently rendering at the wrong size or with the wrong
        // cursor shape. Mirrors the existing `font.base / font.jp /
        // font.emoji` lines emitted by `build_font_stack` below.
        log::info!(
            "settings: font_size={}pt padding={}px cursor_style={:?} cursor_blink={}",
            settings.font_size,
            settings.padding,
            settings.cursor_style,
            settings.cursor_blink
        );
        log::info!(
            "settings: ui_theme={:?} ui_theme_preset={:?} show_tab_bar={} terminal_color_scheme={:?}",
            settings.ui_theme,
            settings.ui_theme_preset,
            settings.show_tab_bar,
            settings.terminal_color_scheme
        );
        // Seed the MD3 palette slot (accent preset × brightness) before
        // any widget runs. `OnceLock` means subsequent calls (e.g. a
        // reload path) are no-ops, which matches the WebView build's
        // startup-only behavior for the UI theme preset. `System`
        // resolves to dark inside `set_preset` — no desktop-portal
        // brightness lookup in the native build yet.
        crate::ui::md3::set_preset(settings.ui_theme_preset, settings.ui_theme);
        let settings = Arc::new(settings);
        // Construct the in-process SFTP service from the configured
        // concurrency cap; the receivers are drained each frame by
        // `App::pump_sftp`.
        let (sftp_service, sftp_progress_rx, sftp_result_rx) =
            crate::sftp::service::SftpService::new(settings.sftp_max_concurrent_uploads);
        let sftp_service = Arc::new(sftp_service);
        // Resolve the user-configured chord table once. Unparseable
        // specs fall back to their built-in defaults with a warn log
        // (see `KeybindTable::from_settings`).
        let keybinds = crate::ui::keybinds::KeybindTable::from_settings(&settings.keybinds);
        let mux_latch = build_mux_latch(&settings);
        // Resolve `language` to a concrete locale once; `Auto` consults
        // the OS locale (sys-locale), unsupported tags fall back to En.
        let locale = crate::i18n::resolve(settings.language);
        log::info!(
            "settings: language={:?} -> locale={:?}",
            settings.language,
            locale
        );
        let (font_resolver, font_fallback, font_cache, font_rasterizer, font_base_id) =
            Self::build_font_stack(&settings);

        // Compute per-cell logical-pixel dimensions from the freshly
        // built font stack so the grid matches the legacy WebView
        // build's `ctx.measureText("M")` path. Done after the font
        // stack so the rasterizer + fallback chain are available.
        // `font_size_px()` applies the CSS-compatible `pt → px`
        // conversion (96/72) that the legacy WebView build does in
        // `renderer-settings.ts` — without it, native-poc rasterizes
        // at ~75% of the WebView size for the same settings value.
        let font_size_px = settings.font_size_px();
        let (cell_w_logical, cell_h_logical) = crate::render::compute_cell_dims(
            font_rasterizer.as_ref(),
            font_fallback.as_ref(),
            font_size_px,
        );
        log::info!(
            "cell metrics: {}x{} logical px (font_size={}pt = {}px)",
            cell_w_logical,
            cell_h_logical,
            settings.font_size,
            font_size_px
        );

        // Per-frame cwd snapshot shared with status-bar providers. The
        // app updates it in `sync_active_cwd` before the runtime is
        // queried; the runtime hands a closure reading this `Arc` to
        // the Cwd / GitBranch providers.
        let active_cwd: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let cwd_for_source = active_cwd.clone();
        let cwd_source: crate::status_bar::providers::CwdSource =
            Arc::new(move || cwd_for_source.lock().clone());
        // Hand the providers a clone of the global `wake` so each
        // owns its own refresh-redraw seam (SPEC.md Notes section).
        let status_bar_runtime = StatusBarRuntime::new(
            &settings.statusbar,
            cwd_source,
            crate::wakeup::shared_wake_fn(),
        );

        // Seed the runtime view state from settings before the `Arc`
        // moves into the struct literal's `settings` field (the literal
        // initializes fields in source order, so a later
        // `settings.font_size` read would be a use-after-move).
        let runtime_font_size_pt = settings.font_size;
        let show_tab_bar = settings.show_tab_bar;

        // Single production notification sink, shared with every tab's
        // callbacks (OSC 9) and used directly by link handling for
        // file-not-found / editor-launch failures.
        let notification_sink: Arc<dyn NotificationSink> = Arc::new(NotifyRustSink);

        // task0002: seed the fade-bookkeeping instant already
        // `OVERLAY_DIM_FADE` in the past so a freshly constructed app
        // resolves directly to `OVERLAY_IDLE_OPACITY` (AC-1) instead of
        // opening with a phantom fade-in. `checked_sub` only fails within
        // `OVERLAY_DIM_FADE` of process start (an `Instant` reference
        // point that near boot); falling back to `now` there costs at
        // most one imperceptible extra fade-in frame.
        let mux_sidebar_fade_started = Instant::now().checked_sub(OVERLAY_DIM_FADE);

        Self {
            tabs: Vec::new(),
            active: 0,
            mux_latch,
            mux_dialog: crate::mux::dialog::MuxDialogState::Closed,
            // FR2/AC-7: the overlay's runtime open flag starts open so a
            // default-configured session shows the window list without a
            // toggle press.
            mux_sidebar_overlay_open: true,
            active_mux_attached_prev_pump: None,
            mux_sidebar_overlay_hovered: false,
            mux_sidebar_hover_prev_render: false,
            mux_sidebar_last_switch: None,
            mux_sidebar_fade_started,
            agent_status: crate::agent_status_model::AgentStatusModel::new(),
            mux_public_pane_ids: std::collections::HashMap::new(),
            settings_launcher: Box::new(crate::settings_launcher::ProcessSettingsLauncher::new()),
            cell_size: GridDims::default(),
            selection: None,
            pending_selection_anchor: None,
            search: crate::search::SearchState::default(),
            search_focus_request: false,
            profile_selector: crate::ui::profile_selector::ProfileSelectorState::default(),
            last_auto_research: None,
            viewer_spawner: crate::viewer::ViewerSpawner::new(),
            viewer_sink: crate::viewer::ProcessViewerSink::new(settings.clone()),
            image_viewer: crate::viewer::image::ImageViewerRouter::new(&settings),
            settings,
            keybinds,
            locale,
            scroll_position: ScrollPosition::Live,
            alt_screen: false,
            window_focused: true,
            blink_started: Instant::now(),
            previous_blink_visible: true,
            visual_bell_started: None,
            bell_erase_pending: false,
            previous_cursor: None,
            previous_selection: None,
            previous_visible_start: 0,
            needs_full_redraw: true,
            scroll_active_tab_into_view: false,
            force_full_redraw,
            pending_active_tab_reconcile: false,
            ime_backend: Box::new(NullBackend::new()),
            ime_last_cursor_cell: None,
            ime_is_null: true,
            ime_overflow_warned: false,
            font_resolver,
            font_fallback,
            font_cache,
            font_rasterizer,
            font_base_id,
            cell_w_logical,
            cell_h_logical,
            runtime_font_size_pt,
            show_tab_bar,
            emoji_texture_cache: Arc::new(Mutex::new(EmojiTextureCache::new())),
            status_bar_runtime,
            active_cwd,
            previous_status_bar_view_model: None,
            notification_sink,
            fold_layout: None,
            sftp_service,
            sftp_ui: crate::sftp::ui::SftpUiState::default(),
            sftp_progress_rx,
            sftp_result_rx,
            restart_toast: RestartToast::default(),
            agent_notification_rate_limiter:
                crate::notifications::AgentNotificationRateLimiter::default(),
        }
    }

    /// Apply a global keybind [`crate::ui::AppAction`]. Returns `true`
    /// when the resulting state should exit the window.
    pub fn apply_action(&mut self, action: crate::ui::AppAction) -> bool {
        match action {
            crate::ui::AppAction::NewTab => {
                // Profile-aware: applies the `is_default` profile when one
                // exists (WebView `handleNewTab` parity).
                self.spawn_new_tab_profile_aware();
                false
            }
            crate::ui::AppAction::NewTabGlobal => {
                self.spawn_new_tab();
                false
            }
            crate::ui::AppAction::OpenProfileSelector => {
                self.open_profile_selector();
                false
            }
            crate::ui::AppAction::CloseTab => {
                let idx = self.active;
                self.close_tab(idx)
            }
            crate::ui::AppAction::NextTab => {
                let total = self.tabs.len();
                if total == 0 {
                    return false;
                }
                let next = (self.active + 1) % total;
                let before = self.active;
                self.switch_to_tab(next);
                // FR4: keyboard activation → scroll the new active cell into
                // view, but only when the active index actually moved.
                if self.active != before {
                    self.scroll_active_tab_into_view = true;
                }
                false
            }
            crate::ui::AppAction::PrevTab => {
                let total = self.tabs.len();
                if total == 0 {
                    return false;
                }
                let prev = if self.active == 0 {
                    total - 1
                } else {
                    self.active - 1
                };
                let before = self.active;
                self.switch_to_tab(prev);
                // FR4: keyboard activation → scroll the new active cell into
                // view, but only when the active index actually moved.
                if self.active != before {
                    self.scroll_active_tab_into_view = true;
                }
                false
            }
            crate::ui::AppAction::JumpTab(n) => {
                let total = self.tabs.len();
                if total == 0 {
                    return false;
                }
                // n is 1-based and clamped to the existing tab range.
                let idx = (n.saturating_sub(1) as usize).min(total - 1);
                let before = self.active;
                self.switch_to_tab(idx);
                // FR4: keyboard activation → scroll the new active cell into
                // view, but only when the active index actually moved.
                if self.active != before {
                    self.scroll_active_tab_into_view = true;
                }
                false
            }
            crate::ui::AppAction::SelectAll => {
                self.select_all();
                false
            }
            crate::ui::AppAction::JumpToPrevPrompt => {
                self.jump_to_prompt(JumpDirection::Prev);
                false
            }
            crate::ui::AppAction::JumpToNextPrompt => {
                self.jump_to_prompt(JumpDirection::Next);
                false
            }
            crate::ui::AppAction::OpenSettings => {
                self.open_settings_window();
                false
            }
            // The remaining view-level actions need the host's
            // `winit::window::Window` handle and/or the deferred-resize
            // machinery, so the keyboard handler in `window_host`
            // dispatches them directly against `WindowHost` instead of
            // routing them here. They are listed explicitly (rather than
            // a catch-all) so a future variant cannot silently fall into
            // a no-op arm.
            crate::ui::AppAction::ZoomIn
            | crate::ui::AppAction::ZoomOut
            | crate::ui::AppAction::ZoomReset
            | crate::ui::AppAction::ToggleFullscreen
            | crate::ui::AppAction::ToggleTabBar
            | crate::ui::AppAction::OpenSearch => false,
        }
    }

    /// Select the entire visible viewport of the active tab. No-op when
    /// there is no active tab or the grid is empty.
    ///
    /// Coordinate-system note: [`crate::selection::Pos`] addresses
    /// **absolute** buffer rows (the same frame `fold` / `prompts` /
    /// `search` use). "Select all" covers the on-screen rows, expressed in
    /// absolute coordinates: the top visible row is
    /// `visible_start = scrollback_len - scroll_offset` (saturating), and
    /// the selection spans `(visible_start, 0)` to
    /// `(visible_start + rows - 1, cols - 1)`. Because the endpoints are
    /// absolute, the highlight stays anchored to the same buffer lines if
    /// the viewport scrolls afterward (and a later scrollback-aware
    /// "select all including history" only needs a wider row range).
    ///
    /// When a fold layout is active the screen rows are non-contiguous in
    /// absolute space (collapsed bodies are hidden), so the top/bottom
    /// endpoints are read from the layout's first/last visible rows rather
    /// than the linear `visible_start + (rows - 1)` model — mirroring the
    /// mouse path's `screen_row_to_abs` mapping.
    pub fn select_all(&mut self) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        let (cols, rows, scrollback_len) = {
            let core = tab.core.lock();
            (core.cols(), core.rows(), core.get_scrollback_length())
        };
        if cols == 0 || rows == 0 {
            return;
        }
        // Derive the visible row span from the fold layout when one is active
        // (collapsed bodies make the screen rows non-contiguous in absolute
        // space), mirroring the mouse path's screen_row_to_abs mapping. Falls
        // back to the linear scrollback model when no layout is active.
        let row_to_abs = |kind: &crate::fold::FoldRowKind| -> u32 {
            match kind {
                crate::fold::FoldRowKind::Cells { actual_line } => *actual_line,
                crate::fold::FoldRowKind::Summary { region } => region.start_line,
            }
        };
        let (anchor_row, extent_row) = match self.fold_layout() {
            Some(layout) if !layout.rows.is_empty() => (
                row_to_abs(layout.rows.first().unwrap()),
                row_to_abs(layout.rows.last().unwrap()),
            ),
            _ => {
                let visible_start = scrollback_len.saturating_sub(self.scroll_offset());
                (visible_start, visible_start + (rows - 1) as u32)
            }
        };
        self.selection = Some(Selection {
            anchor: crate::selection::Pos {
                row: anchor_row,
                col: 0,
            },
            extent: crate::selection::Pos {
                row: extent_row,
                col: cols - 1,
            },
            mode: crate::selection::SelectionMode::Character,
            origin: crate::selection::Pos {
                row: anchor_row,
                col: 0,
            },
        });
        self.needs_full_redraw = true;
    }

    /// ユーザーにデスクトップ通知を送る。
    ///
    /// `notification_sink` フィールドの直接アクセスを避け、通知送信を
    /// アプリケーションドメインにカプセル化するためのメソッド。
    /// ウィンドウ層など外部からの通知送信はこのメソッドを経由すること。
    pub fn notify(&self, title: &str, body: &str) {
        self.notification_sink.send(title, body);
    }

    #[allow(dead_code)] // retained for window_host / tests
    pub fn has_tabs(&self) -> bool {
        !self.tabs.is_empty()
    }

    /// Build a per-frame view model for the status-bar widget. The
    /// runtime owns the template engine, providers, and OSC
    /// dispatcher; this method refreshes the shared cwd cell the
    /// providers read through.
    ///
    /// The render pipeline calls this once per frame and hands the
    /// result to [`crate::ui::status_bar::draw`]. Mux attach state is
    /// not an input (mux-status-bar-removal task0001, FR1/FR5): the
    /// view model is a pure function of settings and the OSC
    /// `777;statusbar` dispatcher's own state.
    pub fn status_bar_view_model(&self) -> crate::status_bar::StatusBarViewModel {
        // Refresh the cwd snapshot the providers read through their
        // `CwdSource` closure. The lock is held only for the duration
        // of the swap.
        let active_cwd_value = self
            .active_tab()
            .and_then(|t| t.cb_state.lock().cwd.clone());
        *self.active_cwd.lock() = active_cwd_value;

        self.status_bar_runtime
            .build_view_model(&self.settings.statusbar)
    }

    /// Build the current status-bar view model and compare it against
    /// the snapshot stored in [`Self::previous_status_bar_view_model`].
    ///
    /// Returns `true` when the model differs from the previous frame
    /// (or no previous frame was recorded yet). Called by
    /// `WindowHost::render` so the dirty-row skip path bypasses its
    /// early-return when the status bar's content has changed even
    /// though no terminal cell needs to repaint — the canonical case
    /// being the wall-clock `{time}` provider's per-second tick on an
    /// otherwise-idle PTY.
    ///
    /// Constructing the view model here re-runs the per-frame resolve
    /// cache; the second call inside `egui::run` hits that cache on
    /// the no-change path, so the extra work is bounded to one
    /// `VariableProvider::version` poll per registered variable.
    pub fn status_bar_view_model_changed(&self) -> bool {
        let current = self.status_bar_view_model();
        match &self.previous_status_bar_view_model {
            Some(prev) => *prev != current,
            None => true,
        }
    }
    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    /// Next `Instant` the event loop must wake while a restart or SFTP toast
    /// is active (task0004 D4). See [`TOAST_POLL_MS`] for why this is a
    /// bounded poll rather than an exact deadline. `None` once no toast is
    /// active — the loop then only wakes on other timed work (blink/bell)
    /// or an event.
    ///
    /// Lives here (not `timing.rs`) because it spans both toast owners:
    /// timing owns the restart toast, sftp owns the SFTP toasts.
    pub fn next_toast_deadline(&self) -> Option<Instant> {
        let toast_pending = self.restart_toast.active() || !self.sftp_ui.toasts.toasts.is_empty();
        toast_pending.then(|| Instant::now() + std::time::Duration::from_millis(TOAST_POLL_MS))
    }

    #[allow(dead_code)] // retained for future mutation paths / tests
    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active)
    }

    /// Drain PTY events on every tab. Returns true if any tab produced
    /// new bytes (caller schedules a redraw).
    pub fn pump_all(&mut self) -> bool {
        let mut changed = false;
        // Whether the *active* tab specifically produced new bytes this
        // pump. Only the active tab's output mutates the buffer the search
        // overlay is resolved against, so background-tab output must not
        // invalidate the active tab's cached logical-line document (H3).
        let mut active_changed = false;
        let mut bell_rang = false;
        let now = Instant::now();
        let active = self.active;
        // Scroll-stick: sample the active tab's scrollback length now and
        // again after the per-tab loop. The saturating difference is fed
        // to `on_pty_output` so an `OffsetFromLive(n)` view advances by Δ
        // and the visible row composition stays anchored while below the
        // configured `scrollback_lines` capacity.
        let before_scrollback_len = self
            .tabs
            .get(active)
            .map(|t| t.core.lock().get_scrollback_length())
            .unwrap_or(0);
        // Desktop notifications collected during the tab loop and
        // dispatched after it — `tab` holds `&mut self.tabs`, so
        // `self.notify()` (a `&self` call) can't run inside the loop.
        let mut pending_notifications: Vec<(String, crate::notifications::ActivityKind)> =
            Vec::new();
        // Absolute-row selection bookkeeping, captured from the active tab in
        // the loop and applied after it (the loop holds `&mut self.tabs`, so
        // mutating `self.selection` must wait until the borrow ends).
        let mut active_eviction_delta: u32 = 0;
        let mut active_frame_reset = false;
        // Outgoing pane index latched by a daemon-initiated SwitchWindow on
        // the active tab this pump (FR3 pane wiring); applied after the
        // `&mut self.tabs` borrow ends.
        let mut active_pane_switch_from: Option<u32> = None;
        // Whether the *active* tab's off-thread snapshot replay completed and
        // swapped in this pump. Drives the active-tab post-loop full redraw so
        // a shorter incoming pane leaves no residual rows (FR2) once the
        // worker-built core is shown. Selection drop is handled by the
        // `pending_frame_reset` latch (drained into `active_frame_reset`),
        // which `apply_offthread_swap` set during the swap.
        let mut active_offthread_swapped = false;
        // Whether the *active* tab's 2nd-pass scrollback-restore merged this
        // pump. `merge_scrollback_from` prepends historical rows to the front
        // of `scrollback_slim`, so `get_scrollback_length()` grows by
        // `merged_rows` — but those rows are older than the user's parked
        // `OffsetFromLive(n)` view of live, so the visible content must NOT
        // shift. Gates the scroll-stick Δ to 0 (the merge is unrelated to
        // live PTY output).
        let mut active_scrollback_restore_merged = false;
        // FR6 (mux): a new mux window appended to the ACTIVE tab this pump means
        // a fresh sub-tab became active (off-screen when the strip overflows),
        // so it should scroll into view next frame — the async mux analogue of
        // the `+`-button new-tab path. Sourced from each tab's one-shot
        // `take_pending_window_appended` latch (set at the `PaneCreated` push
        // site); applied for the active tab after the `&mut self.tabs` borrow ends.
        let mut active_mux_window_added = false;
        // emterm viewer OSC payloads collected across all tabs this pass,
        // routed to the spawner after the `&mut self.tabs` borrow ends.
        let mut viewer_osc: Vec<crate::callbacks::EmtermOscRequest> = Vec::new();
        // Decoded Kitty/SIXEL image events (ImageReady / Place / Delete),
        // likewise buffered during the loop and routed to the image-viewer
        // router after it. Every tab is drained — like the WebView build,
        // any tab's `emterm image` opens a viewer window.
        let mut image_events: Vec<term_images::image_proc::ImageEvent> = Vec::new();
        // Agent-status inputs collected across every tab this pass (task0005),
        // applied to `self.agent_status` after the `&mut self.tabs` borrow
        // ends: plain-tab OSC events (tagged with the originating tab's
        // `stable_id`), daemon-pushed `AgentStatusUpdate` messages, and mux
        // pane ids a `PtyExited` arm removed this pump.
        let mut agent_status_plain_events: Vec<(u64, crate::agent_status::AgentStatusEvent)> =
            Vec::new();
        let mut agent_status_updates: Vec<mux_ipc::protocol::AgentStatusUpdateMsg> = Vec::new();
        let mut agent_status_closed_panes: Vec<u32> = Vec::new();
        // agent-exit-after-icon (task0002 deviation): each tab's resolved,
        // true-order, live-only inferred-clear latch inputs this pump
        // (tagged with the originating tab's `stable_id`, mirroring
        // `agent_status_plain_events`). See `Tab::pending_latch_inputs`'s
        // doc.
        let mut agent_status_latch_inputs: Vec<(
            u64,
            crate::agent_status_model::ResolvedLatchInput,
        )> = Vec::new();
        for (idx, tab) in self.tabs.iter_mut().enumerate() {
            // Phase 4-C (APC redesign): `Tab::pump` already routes
            // APC-encoded mux messages into the tab's own state via
            // `apply_mux_message` (see `crate::mux::apc`). There is no
            // separate `pump_mux` pass — the bridge CLI runs inside the
            // same PTY, so a single drain is sufficient.
            let was_exited = tab.exited;
            if tab.pump() {
                changed = true;
                if idx == active {
                    active_changed = true;
                }
            }
            // task0005: drain this tab's agent-status latches (plain-tab OSC
            // events, daemon `AgentStatusUpdate` pushes, closed mux pane
            // ids) every pump, not just the active tab — a background tab's
            // agent can report status too.
            let tab_stable_id = tab.stable_id;
            for event in tab.take_pending_agent_status_events() {
                agent_status_plain_events.push((tab_stable_id, event));
            }
            for input in tab.take_pending_latch_inputs() {
                agent_status_latch_inputs.push((tab_stable_id, input));
            }
            agent_status_updates.extend(tab.take_pending_agent_status_updates());
            agent_status_closed_panes.extend(tab.take_closed_agent_status_panes());
            // FR6 (mux): drain every tab's one-shot "window appended this pump"
            // latch (set at the `PaneCreated` push site) to avoid stale
            // carry-over; act on it only for the active tab, where the freshly
            // pushed window is now the active sub-tab and should scroll into view.
            if tab.take_pending_window_appended() && idx == active {
                active_mux_window_added = true;
            }
            // Non-blockingly poll this tab's in-flight off-thread snapshot
            // replay (the mux off-thread switch). Run per owning tab, not just
            // the active one, so a background tab's swap applies as well (its
            // selection/scroll are reconciled when it later becomes active,
            // parity with the existing background-tab bookkeeping). On a swap,
            // the displayed content changed, so mark `changed`; the active tab
            // additionally drives the post-loop full redraw + scroll restore.
            match tab.poll_pending_switch() {
                crate::tabs::SwapOutcome::Swapped => {
                    changed = true;
                    if idx == active {
                        active_changed = true;
                        active_offthread_swapped = true;
                    }
                }
                crate::tabs::SwapOutcome::Pending | crate::tabs::SwapOutcome::Idle => {}
            }
            // Non-blockingly poll this tab's in-flight 2nd-pass scrollback
            // restore (the bypass-off rebuild merged into the live core).
            // Run per owning tab so a background tab's restore lands as
            // well — the merge does not touch the viewport, so no
            // `active_offthread_swapped`-style full redraw is required;
            // only `active_changed` matters so the search overlay rebuilds
            // against the new scrollback.
            match tab.poll_pending_scrollback_restore() {
                crate::tabs::ScrollbackRestoreOutcome::Merged => {
                    changed = true;
                    if idx == active {
                        active_changed = true;
                        active_scrollback_restore_merged = true;
                    }
                }
                crate::tabs::ScrollbackRestoreOutcome::Failed => {
                    changed = true;
                    if idx == active {
                        active_changed = true;
                    }
                }
                crate::tabs::ScrollbackRestoreOutcome::Pending
                | crate::tabs::ScrollbackRestoreOutcome::Idle => {}
            }
            // Markdown viewer (FR1/FR2/FR3): collect this tab's emterm OSC
            // queue for the spawner. We cannot touch `self.viewer_spawner`
            // here because the loop holds `&mut self.tabs`, so the drained
            // payloads are buffered and routed after the loop. Draining
            // every tab (not just the active one) matches the WebView
            // build, where any tab's `emterm markdown` opens a viewer.
            let mut osc = tab.drain_osc();
            if !osc.is_empty() {
                // Release the parked `emterm markdown` CLI as soon as its
                // session end marker passes by. The release is gated on the
                // CLI's `interactive=1` flag (set only when its stdin was a
                // TTY, i.e. it is actually parked in the navigate/image/quit
                // stdin loop), which suppresses the common *accidental* case:
                // a non-interactive (piped/redirected) `emterm markdown`
                // omits the flag, so we don't inject `quit` after it has
                // already returned the prompt. The native viewer child
                // resolves images and links itself, so releasing here returns
                // the shell prompt immediately instead of holding it until the
                // viewer window closes. `.any()` caps this at one quit per tab
                // per drain — only one interactive markdown CLI can be parked
                // per PTY.
                //
                // SECURITY (accepted residual): the flag is plaintext in the
                // terminal output stream, which is attacker-controllable, so
                // untrusted output (a `cat`'d file, an SSH peer, a log line)
                // CAN forge `markdown;end;…;interactive=1` and make us write
                // `quit\n` into this tab's foreground program. The blast
                // radius is bounded to that single line (not arbitrary input);
                // accepted as documented in SPEC.md ("Interactive CLI release").
                if osc
                    .iter()
                    .any(|r| crate::viewer::markdown_end_wants_release(&r.payload))
                {
                    tab.write(crate::viewer::MARKDOWN_RELEASE_INPUT.to_vec());
                }
                viewer_osc.append(&mut osc);
            }
            // Image viewer: collect this tab's decoded image events. Must
            // run before the `idx == active` early-continue below so the
            // active tab's images open a viewer too.
            image_events.extend(tab.drain_image_events());
            // Any tab's BEL triggers the bell action — same as the
            // WebView build, where a background tab's BEL still flashes
            // the shared terminal container / beeps.
            let bell = tab.take_bell();
            if bell {
                bell_rang = true;
            }
            let output = tab.take_output();
            // Drain the eviction bookkeeping for every tab so stale deltas do
            // not accumulate on a background tab and then mis-shift the
            // selection the moment it becomes active. Only the active tab's
            // values feed the selection (it owns the buffer the selection
            // addresses).
            let eviction_delta = tab.take_eviction_delta();
            let frame_reset = tab.take_frame_reset();
            // Drain the inbound-pane-switch latch for every tab so a stale
            // transition recorded on a background tab cannot mis-apply when it
            // later becomes active; only the active tab's value is consumed.
            let pane_switch_from = tab.take_pending_pane_switch();
            if idx == active {
                active_eviction_delta = eviction_delta;
                active_frame_reset = frame_reset;
                active_pane_switch_from = pane_switch_from;
            } else if let Some(from_pane) = pane_switch_from {
                // Background tab: a daemon-initiated pane switch (or new-window
                // create) moved this tab's active pane while it was off screen.
                // Its live scroll lives in `Tab::scroll_position` (only the
                // active tab's lives in `App::scroll_position`), so park that
                // into the outgoing pane's slot and reload the now-active pane's
                // saved slot. Without this, a later `switch_to_tab` would
                // restore the OLD pane's scroll against the NEW active pane
                // (FR3 / NFR3 divergence). No full redraw — the tab is hidden.
                // The latch holds the outgoing pane *id*; resolve it to a
                // current index (a same-pump `PtyExited` may have shifted the
                // arrays) and skip the park if the pane has since exited.
                let saved = tab.scroll_position;
                let reloaded = tab.mux_group.as_mut().map(|group| {
                    if let Some(from_idx) = group.index_of_pane_id(from_pane) {
                        group.set_pane_scroll_at(from_idx, saved);
                    }
                    group.active_pane_scroll()
                });
                if let Some(scroll) = reloaded {
                    tab.scroll_position = scroll;
                }
            }
            let exited_now = !was_exited && tab.exited;

            // ── Inactive-tab activity (dot + desktop notification) ──
            // WebView parity: `TabActivityTracker.markActivity` ignores
            // the active tab entirely; clearing the active tab's dot
            // every frame covers all switch paths (click, keybind,
            // reorder, exited-tab reap) without per-path hooks.
            if idx == active {
                if tab.activity.has_activity {
                    tab.activity.clear();
                    changed = true;
                }
                continue;
            }
            for (fired, kind) in [
                (exited_now, crate::notifications::ActivityKind::ProcessExit),
                (output, crate::notifications::ActivityKind::Output),
                (bell, crate::notifications::ActivityKind::Bell),
            ] {
                if !fired || !crate::notifications::kind_enabled(&self.settings, kind) {
                    continue;
                }
                // `mark` owns the 1 s output throttle; a swallowed mark
                // produces neither a dot nor a notification.
                if !tab.activity.mark(kind, now) {
                    continue;
                }
                // Desktop notification gates, in WebView order:
                // master switch → window focus → per-tab 5 s throttle
                // (ProcessExit bypasses the check but re-arms it).
                if self.settings.notification_enabled
                    && !self.window_focused
                    && tab.activity.should_notify(kind, now)
                {
                    // Sanitize at capture time: carries ≤ 100 chars
                    // instead of cloning a potentially huge OSC title.
                    pending_notifications
                        .push((crate::notifications::sanitize_title(&tab.title), kind));
                }
            }
        }
        // Route this pass' collected emterm viewer OSC payloads now that the
        // `&mut self.tabs` borrow has ended. The spawner reassembles
        // markdown sessions and emits completed documents to `viewer_sink`,
        // which spawns a separate `--viewer` child process per document.
        // Always call so the spawner's idle-session timeout sweep runs even
        // on empty passes; cheap when there is nothing queued.
        self.viewer_spawner
            .drain(viewer_osc, now, &mut self.viewer_sink);
        // Route decoded image events: ImageReady fills the LRU store,
        // each Place spawns a native `--image-viewer` child window.
        if !image_events.is_empty() {
            self.image_viewer.handle_events(image_events);
        }
        // Apply this pass' collected agent-status inputs now that the
        // `&mut self.tabs` borrow has ended (task0005). FR4 requires a tab's
        // real-status events and its latch inputs to be applied as ONE
        // ordered stream: `reconcile_latch_feed` derives both lists from the
        // same `pending_latch_feed` in order, so within a single tab each
        // `Set`/`Clear` latch input corresponds 1:1 and same-order with one
        // `AgentStatusEvent`. Walking the latch inputs and pulling that tab's
        // next unconsumed plain event per `Set`/`Clear` reconstructs the true
        // order; a bare `Mark` consumes no plain event (it only ever applies
        // a real change indirectly, via `record_live_prompt_mark`'s own
        // inferred `apply_plain_tab_event(Clear)` when the latch fires).
        //
        // The pairing MUST be per tab, never positional across the flattened
        // lists: the two lists are filled by the same tab loop but a tab can
        // contribute to one and not the other. A mux-connected tab still
        // pushes real-status events while `process_combined` discards its
        // latch feed (that tab's pane status is daemon-authoritative), so it
        // contributes plain events and zero latch inputs. Consuming
        // positionally would let that tab's events satisfy ANOTHER tab's
        // `Set`/`Clear` and push the real events after the latch bookkeeping
        // they belong with — reintroducing the very reordering this loop
        // exists to prevent. Events with no matching latch input are applied
        // afterwards, in their original order.
        let mut agent_status_plain_events: Vec<
            Option<(u64, crate::agent_status::AgentStatusEvent)>,
        > = agent_status_plain_events.into_iter().map(Some).collect();
        for (tab_stable_id, input) in agent_status_latch_inputs {
            match input {
                crate::agent_status_model::ResolvedLatchInput::Set
                | crate::agent_status_model::ResolvedLatchInput::Clear => {
                    let paired = agent_status_plain_events
                        .iter_mut()
                        .find(|slot| matches!(slot, Some((id, _)) if *id == tab_stable_id))
                        .and_then(Option::take);
                    if let Some((_, event)) = paired {
                        self.agent_status
                            .apply_plain_tab_event(tab_stable_id, event);
                    }
                    if matches!(input, crate::agent_status_model::ResolvedLatchInput::Set) {
                        self.agent_status.record_latch_set(tab_stable_id);
                    } else {
                        self.agent_status.record_latch_clear(tab_stable_id);
                    }
                }
                crate::agent_status_model::ResolvedLatchInput::Mark(kind) => {
                    self.agent_status
                        .record_live_prompt_mark(tab_stable_id, kind);
                }
            }
        }
        for (tab_stable_id, event) in agent_status_plain_events.into_iter().flatten() {
            self.agent_status
                .apply_plain_tab_event(tab_stable_id, event);
        }
        for update in agent_status_updates {
            // task0006 AC-5: learn/refresh this pane's public ID from the
            // same message before applying it to the model — the daemon is
            // the only source for it (see `Self::mux_public_pane_ids`).
            self.mux_public_pane_ids
                .insert(update.pane_id, update.public_pane_id.clone());
            self.agent_status.apply_daemon_update(
                update.pane_id,
                update.state.map(crate::agent_status_model::state_from_wire),
                update.name,
                update.revision,
                update.replay_derived,
            );
        }
        for pane_id in agent_status_closed_panes {
            // task0009 AC-4: resolve the rate-limit key from the still-
            // present public-id mapping BEFORE removing it below.
            let key = crate::agent_status_model::PaneKey::MuxPane(pane_id);
            let rate_limit_key = agent_notification_rate_limit_key(&self.mux_public_pane_ids, &key);
            self.mux_public_pane_ids.remove(&pane_id);
            self.discard_agent_notification_state(&rate_limit_key);
            self.agent_status.discard(&key);
        }
        // task0009: drain queued real-transition events (task0005's
        // `AgentStatusModel::drain_transitions`) and dispatch qualifying
        // ones to the notification layer. Runs unconditionally — even
        // while `settings.agent_status_notifications` is off — so the
        // transition queue never grows unbounded while the setting is
        // toggled off (NFR3); the settings gate lives inside
        // `maybe_notify_agent_transition`. Must run BEFORE mark_seen below:
        // mark_seen would otherwise flip a freshly-arrived transition's
        // pane to "seen" before its own visibility is evaluated here (the
        // two operate on independent flags today, but ordering keeps the
        // gating and mark_seen concerns from becoming coupled).
        for transition in self.agent_status.drain_transitions() {
            let crate::agent_status_model::Transition {
                pane,
                old_state,
                new_state,
                name,
            } = transition;
            // AC-2: Clear transitions (new_state: None) are never
            // notification-eligible — only Set into blocked/done qualifies.
            let Some(new_state) = new_state else {
                continue;
            };
            let pane_visible =
                agent_status_pane_visible(self.window_focused, self.tabs.get(self.active), &pane);
            let rate_limit_key =
                agent_notification_rate_limit_key(&self.mux_public_pane_ids, &pane);
            let tab_title = agent_status_pane_tab_title(&self.tabs, &pane)
                .unwrap_or_default()
                .to_string();
            let agent_transition = crate::notifications::AgentTransition {
                old_state: old_state.map(crate::agent_status_model::state_to_wire),
                new_state: crate::agent_status_model::state_to_wire(new_state),
                name,
            };
            self.maybe_notify_agent_transition(
                rate_limit_key,
                pane_visible,
                &agent_transition,
                &tab_title,
            );
        }
        // mark_seen (task0005 AC-5): the active tab's panes are "displayed"
        // whenever the OS window is focused, regardless of whether this
        // pump produced any other change — the user could simply be looking
        // at an already-idle screen. Re-running every pump is intentionally
        // idempotent (`mark_seen` on an already-seen entry is a no-op).
        if self.window_focused {
            if let Some(active_tab) = self.tabs.get(self.active) {
                let panes = agent_status_keys_for_tab(active_tab);
                self.agent_status.mark_seen(panes.iter());
            }
        }
        // Apply the active tab's absolute-row selection bookkeeping now that
        // the `&mut self.tabs` borrow has ended. A frame reset drops the
        // selection outright (its rows belong to the discarded frame); an
        // eviction shifts it down by the number of evicted rows and drops it
        // when the whole range scrolled off the top. The `previous_selection`
        // is intentionally *not* shifted — it is interpreted against the
        // matching `previous_visible_start` captured in the same prior frame.
        // Inbound (daemon-initiated) pane switch on the active tab: park the
        // outgoing pane's scroll position, reload the now-active pane's saved
        // position into the single active value, and force a full redraw so a
        // shorter incoming pane leaves no residual rows (FR2 + FR3 pane wiring).
        // Mirrors the local switch path, but the active index has already moved
        // inside `apply_mux_message`. The latch holds the outgoing pane *id*;
        // resolve it to a current index (a same-pump `PtyExited` may have
        // shifted the arrays) and skip the park if the pane has since exited.
        if let Some(from_pane) = active_pane_switch_from {
            let restored = self.tabs.get_mut(self.active).and_then(|tab| {
                let group = tab.mux_group.as_mut()?;
                if let Some(from_idx) = group.index_of_pane_id(from_pane) {
                    group.set_pane_scroll_at(from_idx, self.scroll_position);
                }
                Some(group.active_pane_scroll())
            });
            if let Some(scroll) = restored {
                self.scroll_position = scroll;
                self.needs_full_redraw = true;
                changed = true;
            }
        }
        // The active tab's off-thread snapshot replay completed and swapped in
        // this pump: the displayed core changed from the outgoing pane to the
        // (possibly shorter) target pane, so force a full redraw to clear any
        // residual rows (FR2). The per-pane scroll position was already
        // restored at `switch_to` time (the active index moved synchronously
        // there); the swap only changes the rendered content, so no scroll
        // reload is needed here. Selection drop is handled by the
        // `active_frame_reset` latch below.
        if active_offthread_swapped {
            self.needs_full_redraw = true;
            changed = true;
        }
        if active_frame_reset {
            self.selection = None;
            self.pending_selection_anchor = None;
        } else if active_eviction_delta > 0 {
            if let Some(sel) = self.selection.as_mut() {
                if !sel.shift_rows_down(active_eviction_delta) {
                    self.selection = None;
                }
            }
            // Shift the pending press anchor in lockstep with the selection.
            // An anchor whose row scrolled off the top is dropped, since the
            // first drag motion would otherwise anchor against a stale row.
            if let Some(anchor) = self.pending_selection_anchor {
                match anchor.row.checked_sub(active_eviction_delta) {
                    Some(r) => {
                        self.pending_selection_anchor =
                            Some(crate::selection::Pos { row: r, ..anchor })
                    }
                    None => self.pending_selection_anchor = None,
                }
            }
        }
        // FR6 (mux): a new mux window was appended to the active tab this pump,
        // so its freshly-active sub-tab should scroll into view next frame (the
        // async mux analogue of the `+`-button new-tab path). Applied here,
        // after the `&mut self.tabs` borrow has ended.
        if active_mux_window_added {
            self.scroll_active_tab_into_view = true;
        }
        // Reset the mux sidebar overlay flag when the FOCUSED tab's mux
        // group tore down this pump (a `Detached` reply, or the last
        // window's `PtyExited` emptying the group — both routed into
        // `tab.mux_group` by `Tab::pump` above, before this read). Compared
        // against `self.active` before the exited-tab reap below (which can
        // renumber it, mirroring the before/after-scrollback sampling
        // rationale above the tab loop): the reap only runs when a tab is
        // fully removed, which already implies its mux group (if any) went
        // to `None` earlier in this same pass, so reading here still
        // targets the right tab. A changed `self.active` between pumps
        // (the user just switched tabs) intentionally does NOT reset the
        // flag — see the field doc on `active_mux_attached_prev_pump`.
        let active_mux_attached_now = self
            .tabs
            .get(self.active)
            .is_some_and(|t| t.mux_group.is_some());
        if self.active_mux_attached_prev_pump == Some(self.active) && !active_mux_attached_now {
            self.mux_sidebar_overlay_open = false;
        }
        // task0001 FR1/FR2/FR3: open the overlay flag on the
        // not-attached -> attached transition (bookkeeping field held no
        // value at the end of the previous pump, AND the active tab is
        // mux-attached now). Restores the AC-7 "default open" guarantee at
        // startup and on reattach, since the runtime flag only starts open
        // at construction — a later attach (the common case: the mux
        // daemon connection completes asynchronously after `App::new`)
        // would otherwise leave a stale `false` from an earlier detach.
        // Unconditional, no gate on `window_sidebar_overlay` (mirrors the
        // detach guard above): in persistent mode the flag drives no
        // rendering, so the assignment is inert there. Read BEFORE the
        // bookkeeping reassignment below, mirroring the detach guard.
        // Reattach after an explicit close re-opens the sidebar (FR3,
        // accepted) and switching the active tab onto an already-attached
        // mux tab also satisfies this transition and reopens the sidebar —
        // both accepted per IMPLEMENTATION.md D2 (the single-slot
        // bookkeeping cannot distinguish the two cases).
        if self.active_mux_attached_prev_pump.is_none() && active_mux_attached_now {
            self.mux_sidebar_overlay_open = true;
        }
        self.active_mux_attached_prev_pump = active_mux_attached_now.then_some(self.active);
        for (sanitized_title, kind) in pending_notifications {
            let body = crate::notifications::notification_body(&sanitized_title, kind, self.locale);
            self.notify(crate::notifications::NOTIFICATION_TITLE, &body);
        }
        if bell_rang {
            match self.settings.bell_action {
                crate::settings::BellAction::Visual => {
                    self.visual_bell_started = Some(Instant::now());
                    changed = true;
                }
                crate::settings::BellAction::Sound => crate::bell::play_beep(),
                crate::settings::BellAction::None => {}
            }
        }
        // Mirror the active tab's alt-screen state onto the App so the
        // scroll input routes can suppress wheel / Shift+Page during
        // alt-screen sessions. Read the core's authoritative
        // `MODE_ALT_SCREEN` bit (set at parse time on every 1049/47/1047
        // toggle) rather than the toggle-tracked `Tab::alt_screen`: a single
        // buffer-switch action lost across a reattach / off-thread replay /
        // snapshot gap would otherwise strand `Tab::alt_screen` true and kill
        // scrolling permanently even after the app returned to the main
        // buffer. The core's mode bit cannot desync from the displayed buffer.
        let active_alt = self.tabs.get(self.active).map(|t| {
            t.core
                .lock()
                .get_mode(term_core::terminal_core::MODE_ALT_SCREEN)
        });
        if let Some(active_alt) = active_alt {
            self.set_alt_screen(active_alt);
        }
        // Notify scroll-position state machine that new bytes arrived so
        // the auto-follow rule can preserve the off-tail offset. Pass
        // whether the *active* tab changed so only its output invalidates
        // the search overlay's cached document (H3). Sample the active
        // tab's scrollback length now (post-pump) so the saturating
        // difference from `before_scrollback_len` is the per-pump Δ that
        // anchors the visible content while parked at an `OffsetFromLive`
        // view. Reading the same `self.active` index as the before sample
        // is intentional; a same-pump tab reap shifts `self.active` later
        // (below), so this read still targets the tab that produced the
        // bytes.
        if changed {
            let after_scrollback_len = self
                .tabs
                .get(self.active)
                .map(|t| t.core.lock().get_scrollback_length())
                .unwrap_or(before_scrollback_len);
            // Pass Δ=0 whenever the active tab's scrollback length grew for a
            // reason other than live PTY output, so on_pty_output does not
            // corrupt the parked OffsetFromLive(n):
            //
            // * `active_pane_switch_from`: the active pane changed identity
            //   between the before_scrollback_len sample and now, so the
            //   saturating difference compares two distinct panes' scrollback
            //   lengths. The incoming pane's restored scroll_position is the
            //   authoritative state.
            // * `active_offthread_swapped`: `apply_offthread_swap` replaced
            //   `*self.core.lock()` wholesale; after_scrollback_len now reads
            //   a freshly-built snapshot core whose length is unrelated to
            //   before_scrollback_len. The per-pane scroll position was
            //   restored synchronously at switch_to time; the swap only
            //   changes the rendered content.
            // * `active_scrollback_restore_merged`: the 2nd-pass restore
            //   prepended historical rows to scrollback. get_scrollback_length
            //   grew by merged_rows, but those rows are older than the parked
            //   view — the visible content must not shift.
            let scrollback_delta = if active_pane_switch_from.is_some()
                || active_offthread_swapped
                || active_scrollback_restore_merged
            {
                0
            } else {
                after_scrollback_len.saturating_sub(before_scrollback_len)
            };
            self.on_pty_output(active_changed, scrollback_delta);
        }
        // Reap exited tabs (Phase 5 will refine the policy).
        let before = self.tabs.len();
        // task0005 AC-6 / task0009 AC-4: discard every reaped tab's
        // agent-status entries AND notification rate-limit bookkeeping
        // before removal (a mux tab whose last pane exited reaches this
        // path via `exited = true` rather than `close_tab`). Collected
        // into an owned `Vec` first so the immutable `self.tabs` borrow
        // (held by the iterator/filter) ends before the mutable
        // `self.discard_agent_notification_state` calls below.
        let reaped_agent_status_keys: Vec<crate::agent_status_model::PaneKey> = self
            .tabs
            .iter()
            .filter(|t| t.exited)
            .flat_map(agent_status_keys_for_tab)
            .collect();
        for key in reaped_agent_status_keys {
            let rate_limit_key = agent_notification_rate_limit_key(&self.mux_public_pane_ids, &key);
            self.discard_agent_notification_state(&rate_limit_key);
            self.agent_status.discard(&key);
        }
        self.tabs.retain(|t| !t.exited);
        if self.tabs.len() != before {
            if self.active >= self.tabs.len() && !self.tabs.is_empty() {
                self.active = self.tabs.len() - 1;
            }
            // The reap shifted `self.active` onto a different tab's buffer,
            // so an open search overlay is now indexed into a buffer that is
            // no longer active. Close it to match the `close_tab` /
            // `switch_to_tab` behavior (H4).
            if self.search.visible {
                self.search.close();
            }
            changed = true;
            // Tab roster changed; redraw everything to repaint the title bar
            // and the new active grid.
            self.needs_full_redraw = true;
            // D2: the reap may have moved the active index onto a different
            // tab (the exited tab was the active one, or the last one);
            // request a reconcile of its size against `cell_size` (deferred
            // — see `request_active_tab_reconcile`). No-op when empty
            // (nothing to reconcile) or already at size.
            self.request_active_tab_reconcile();
        }
        changed
    }

    /// Propagate a new grid size to the ACTIVE tab only (FR1, FR2). Inactive
    /// tabs are left untouched here — their PTYs must never receive a
    /// TIOCSWINSZ behind the user's back. A tab's size is reconciled to
    /// `self.cell_size` lazily, at the moment it becomes active (see
    /// `execute_pending_reconcile`, invoked by `window_host::render` after
    /// every activation path's deferred request).
    pub fn set_grid_size(&mut self, cols: u16, rows: u16) {
        // D3'''''' (round-9 rework, review round-8 finding
        // `1e7e069001cf22dc`, AC-5): clamp HERE, before comparing against or
        // assigning `self.cell_size` — `Tab::resize` (invoked below, via
        // `apply_tab_resize`) applies the IDENTICAL pure
        // `clamp_dims_to_wire_domain` clamp to the active tab's own core, so
        // computing it once up front and using the SAME clamped value for
        // both `self.cell_size` and the active-tab resize keeps the app's
        // own grid record from ever disagreeing with what the core it
        // drives actually holds. Before this fix, `self.cell_size` recorded
        // the caller's RAW request while `Tab::resize` silently narrowed
        // the core to a smaller wire-domain size — renderer / hit-testing /
        // `window_host::grid_size` all read `self.cell_size`, so they
        // described a size the core itself was never actually at.
        let (cols, rows) = crate::mux::session::pane::clamp_dims_to_wire_domain(cols, rows);
        if self.cell_size.cols == cols && self.cell_size.rows == rows {
            return;
        }
        self.cell_size = GridDims { cols, rows };
        // Design 2 (FR6, task0003 rework): issue the resize through the
        // SAME App-side application path the reconcile executor uses —
        // reads the active tab's pre-resize column count, applies
        // `Tab::resize`, and clears the App-owned reflow-invalidated
        // trackers (selection, pending anchor) if the resize changed the
        // column count (N3). The active tab's OWN reflow-invalidated
        // trackers (prompt / fold marks) are cleared by the tab itself
        // inside its own `resize` (IMPLEMENTATION.md D3); an inactive tab
        // is never resized here, so its trackers are unaffected.
        self.apply_tab_resize(self.active, cols, rows);
        // A reshape rewraps scrollback / viewport, so an open search
        // overlay's cached logical-line document no longer matches the
        // buffer; flag it for rebuild on the next `execute`.
        if self.search.visible {
            self.search.mark_buffer_dirty();
        }
        // Resize invalidates the previously rendered framebuffer; the
        // simplest correct response is a full redraw.
        self.needs_full_redraw = true;
    }

    /// Mark the next frame as needing a full repaint regardless of the
    /// `term_core` dirty bits. Use after resize / surface recovery /
    /// other structural changes.
    pub fn mark_full_redraw(&mut self) {
        self.needs_full_redraw = true;
    }

    /// Whether a full repaint is currently pending (`mark_full_redraw` /
    /// `force_full_redraw`). Read by `window_host::render` at the grid-build
    /// point: events applied from the frame's own egui pass (a tab switch,
    /// a scrollbar jump) raise the flag *mid-frame*, after the frame-top
    /// dirty snapshot was taken — the build must then widen to every row
    /// instead of trusting the stale snapshot, or the per-row cache keeps
    /// serving the previous tab's content.
    pub fn full_redraw_pending(&self) -> bool {
        self.needs_full_redraw || self.force_full_redraw
    }

    /// React to new PTY output on the active tab. `scrollback_delta` is the
    /// number of rows that spilled into scrollback during this `pump_all`
    /// pass (`after_len - before_len` of the active tab's
    /// `get_scrollback_length`). It is used to keep the visible content
    /// anchored when the user is parked at an `OffsetFromLive` view:
    ///
    /// * Below scrollback capacity, every pushed row grows `scrollback_len`
    ///   by 1, so the visible-row formula `scrollback_len - scroll_offset`
    ///   would advance unless we increment `scroll_offset` by the same Δ.
    /// * Once capacity is reached, `scrollback_len` is pinned and Δ == 0
    ///   — the visible row composition shifts (as intended; the user has
    ///   accepted that "you can't keep your place forever").
    ///
    /// `active_changed` is `true` only when the **active** tab produced
    /// the new bytes. The search overlay is resolved against the active
    /// tab's buffer, so background-tab output must not invalidate its
    /// cached logical-line document (H3) — a switch to that tab closes
    /// the overlay anyway, so a background change never needs to be
    /// reflected.
    pub fn on_pty_output(&mut self, active_changed: bool, scrollback_delta: u32) {
        // New PTY output on the active tab mutates its scrollback / viewport,
        // so an open search overlay's cached logical-line document is stale,
        // and the matches' absolute rows shift as rows spill into scrollback.
        // Flag it dirty here; the frame loop calls `auto_research_if_dirty`
        // once per frame to rebuild and re-resolve (the per-frame cadence
        // throttles bursts of PTY chunks — we do not re-search per chunk).
        // Only meaningful while the overlay is visible; harmless otherwise
        // since `clear`/`close` resets the flag anyway.
        if active_changed && self.search.visible {
            self.search.mark_buffer_dirty();
        }
        // No-op for `Live`; explicit branch documents intent.
        if let ScrollPosition::OffsetFromLive(n) = self.scroll_position {
            // Advance the offset by Δ so the visible row composition stays
            // anchored: `visible_start = scrollback_len - scroll_offset`
            // grows in lockstep with `scrollback_len` while below the
            // configured capacity. Clamp to `scrollback_lines` because the
            // offset cannot exceed the ring's depth — once we hit the cap,
            // further pushes evict rows and the parked view drifts (Δ == 0
            // at capacity, accepted).
            let max = self.settings.scrollback_lines;
            let new_n = n.saturating_add(scrollback_delta).min(max);
            self.scroll_position = ScrollPosition::OffsetFromLive(new_n);
            self.needs_full_redraw = true;
        }
    }

    /// Update the alt-screen flag. Called from `Tab::pump` after draining
    /// `core.take_mode_actions()`. Entering alt-screen forces the scroll
    /// position back to `Live` (alt buffers have no scrollback).
    pub fn set_alt_screen(&mut self, active: bool) {
        if self.alt_screen == active {
            return;
        }
        self.alt_screen = active;
        if active {
            self.scroll_position = ScrollPosition::Live;
        }
        // The selection holds absolute-row coordinates of the screen it was
        // made on; toggling between the main and alt screen changes the
        // buffer the selection addresses, so drop it (and any pending press
        // anchor) — matches current eMterm behavior and the SPEC's
        // "alt-screen toggle during selection → selection cleared".
        self.selection = None;
        self.pending_selection_anchor = None;
        self.needs_full_redraw = true;
    }

    /// Compute the rows that must be repainted on the next frame. Union of:
    /// 1. `term_core::get_dirty_rows()` for cell-level edits
    /// 2. previous + current cursor row, but only when the cursor cell
    ///    actually moved since the previous frame (clears the cursor ghost
    ///    on move; a stationary cursor with a stable blink phase costs no
    ///    repaint of its row now that the block cursor lives in the egui
    ///    overlay rather than the grid instances — see task0001)
    /// 3. the current cursor row again, but only on a blink phase flip
    ///    while blink is enabled (so the overlay can paint/erase the glyph)
    /// 4. previous + current selection rows (to clear highlight on shrink)
    ///
    /// Returns a sorted, deduplicated `Vec`. Returns `0..rows` when
    /// `needs_full_redraw` or `force_full_redraw` is set. Honest emptiness
    /// is the point: a frame with no content, cursor, blink, selection, or
    /// scroll change returns an empty `Vec` so the render-skip decision in
    /// `window_host.rs` can actually fire (task0002 AC-1).
    pub fn dirty_rows_this_frame(&self, core: &TerminalCore) -> Vec<u16> {
        let rows = core.rows();
        if rows == 0 {
            return Vec::new();
        }

        if self.force_full_redraw || self.needs_full_redraw {
            return (0..rows).collect();
        }

        let mut set: Vec<u16> = core.get_dirty_rows();

        let push_unique = |set: &mut Vec<u16>, r: u16| {
            if r < rows && !set.contains(&r) {
                set.push(r);
            }
        };

        // Cursor history: previous + current row, but only when the
        // cursor cell (row, col) actually moved since the previous frame —
        // a stationary cursor pushes no row here, letting an idle frame's
        // dirty set go empty.
        let cursor_row = core.get_cursor_row();
        let cursor_col = core.get_cursor_col();
        let cursor_moved = self.previous_cursor != Some((cursor_row, cursor_col));
        if cursor_moved {
            push_unique(&mut set, cursor_row);
            if let Some((prev_row, _)) = self.previous_cursor {
                push_unique(&mut set, prev_row);
            }
        }
        // Independently of cursor movement, a blink phase flip still needs
        // the cursor row repainted (or erased) — but only while blink is
        // enabled; a disabled blink never pushes a row here.
        let blink_enabled = core.get_cursor_blink();
        if blink_enabled && self.blink_visible_now(blink_enabled) != self.previous_blink_visible {
            push_unique(&mut set, cursor_row);
        }

        // Selection history: union of previous + current. The selection now
        // holds *absolute* buffer rows, so each range must be translated back
        // into the screen rows it occupies before being added to the dirty
        // set.
        //
        // With a fold layout the core's row coordinates (viewport rows, used
        // by core.get_dirty_rows()/get_cursor_row() above) and the absolute
        // → screen mapping are both non-linear with respect to screen rows
        // (collapsed bodies are hidden, summary rows draw no cells), so a
        // simple `abs - visible_start` does not hold and core-space dirty
        // rows cannot be reinterpreted as screen rows. Rather than reproduce
        // the fold walk here just for the dirty set, repaint every row
        // whenever a fold layout is active — folds are rare and the cost is
        // bounded to the viewport.
        if self.fold_layout.is_some() {
            return (0..rows).collect();
        }

        // Linear case: map the absolute selection range onto screen rows by
        // intersecting it with the visible window `[visible_start,
        // visible_start + rows)` and offsetting by `visible_start`.
        let push_abs_range = |set: &mut Vec<u16>, visible_start: u32, sel: &Selection| {
            let (start, end) = sel.ordered();
            let win_end = visible_start + rows as u32; // exclusive
            let lo = start.row.max(visible_start);
            let hi = (end.row + 1).min(win_end); // exclusive
            if lo >= hi {
                return;
            }
            for abs in lo..hi {
                push_unique(set, (abs - visible_start) as u16);
            }
        };
        let visible_start_now = core
            .get_scrollback_length()
            .saturating_sub(self.scroll_offset());
        if let Some(sel) = &self.selection {
            push_abs_range(&mut set, visible_start_now, sel);
        }
        if let Some(sel) = &self.previous_selection {
            // The previous selection's rows are interpreted against the
            // visible-start captured when that frame rendered.
            push_abs_range(&mut set, self.previous_visible_start, sel);
        }

        set.sort_unstable();
        set
    }

    /// Called after the renderer consumed the dirty set. Stores current
    /// cursor/selection/blink-phase for next-frame ghost prevention,
    /// clears the `needs_full_redraw` flag, and clears the core's dirty
    /// bits.
    pub fn record_render_state(&mut self, core: &mut TerminalCore) {
        self.previous_cursor = Some((core.get_cursor_row(), core.get_cursor_col()));
        self.previous_selection = self.selection;
        // Snapshot the visible-start used this frame so next frame's dirty
        // computation can translate `previous_selection`'s absolute rows back
        // into the screen rows it actually occupied.
        self.previous_visible_start = core
            .get_scrollback_length()
            .saturating_sub(self.scroll_offset());
        self.previous_blink_visible = self.blink_visible_now(core.get_cursor_blink());
        self.previous_status_bar_view_model = Some(self.status_bar_view_model());
        // task0002 AC-9: snapshot the hover flag so the NEXT frame's
        // `mux_sidebar_hover_changed` compares against what this frame
        // actually painted, not a stale earlier value.
        self.mux_sidebar_hover_prev_render = self.mux_sidebar_overlay_hovered;
        self.needs_full_redraw = false;
        core.clear_dirty();
    }

    /// Variant for the no-active-tab case (e.g. just after the last tab
    /// exited). Drops the `needs_full_redraw` flag so the next frame can
    /// short-circuit, but leaves cursor/selection history untouched
    /// because no core is available.
    pub fn record_render_state_no_tab(&mut self) {
        self.previous_cursor = None;
        self.previous_selection = None;
        self.previous_visible_start = 0;
        self.previous_blink_visible = true;
        self.previous_status_bar_view_model = Some(self.status_bar_view_model());
        self.mux_sidebar_hover_prev_render = self.mux_sidebar_overlay_hovered;
        self.needs_full_redraw = false;
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
