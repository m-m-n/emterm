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
use crate::ime::backend::{ImeBackend, ImeEvent, KeyDispatchResult, PUMP_BUDGET, RawKeyEvent};
use crate::ime::null::NullBackend;
use crate::render::font::cache::GlyphCache;
use crate::render::font::fallback::FallbackChain;
use crate::render::font::resolver::Resolver;
use crate::render::font::traits::{FontId, GlyphRasterizer};
use crate::selection::Selection;
use crate::settings::{FontEngine, Settings};
use crate::status_bar::StatusBarRuntime;
use crate::tabs::Tab;
use crate::ui::emoji_cache::EmojiTextureCache;

/// Cursor blink half-period in milliseconds. 530 ms matches xterm's
/// `cursorBlinkXOR` interval; one full on/off cycle is `2 * BLINK_HALF_MS`.
pub const BLINK_HALF_MS: u128 = 530;

/// Visual-bell flash duration. Mirrors the WebView build's
/// `.terminal-bell-flash` animation (150 ms ease-out, `src/styles.css`).
pub const BELL_FLASH_MS: u64 = 150;

/// Minimum wall-clock gap between two automatic (per-frame) search
/// re-resolves. A burst of PTY output flags the search document dirty on
/// every chunk; without this throttle [`App::auto_research_if_dirty`] would
/// rebuild the logical-line document and re-run the match on every painted
/// frame. The dirty flag is *preserved* when a re-search is skipped here, so
/// the pending change is reflected on the next frame past the gap.
pub const AUTO_RESEARCH_THROTTLE: std::time::Duration = std::time::Duration::from_millis(150);

/// Whether an automatic re-search may run now, given the instant of the
/// previous one. `None` (no prior auto re-search) always allows. Split out
/// as a pure function so the throttle policy is unit-testable without an
/// `App` or real wall-clock sleeps.
pub fn auto_research_allowed(last: Option<Instant>, now: Instant) -> bool {
    match last {
        None => true,
        Some(prev) => now.duration_since(prev) >= AUTO_RESEARCH_THROTTLE,
    }
}

/// Per-step change applied by the `ZoomIn` / `ZoomOut` keybinds, in
/// logical points.
pub const FONT_SIZE_PT_STEP: f32 = 1.0;
/// Lower clamp for the runtime terminal font size (logical points).
pub const FONT_SIZE_PT_MIN: f32 = 6.0;
/// Upper clamp for the runtime terminal font size (logical points).
pub const FONT_SIZE_PT_MAX: f32 = 72.0;

/// Clamp a candidate terminal font size (logical points) into
/// [`FONT_SIZE_PT_MIN`]..=[`FONT_SIZE_PT_MAX`]. Split out as a pure
/// function so the zoom clamp can be unit-tested without constructing a
/// full `App` (which builds the font stack and a status-bar runtime).
pub fn clamp_font_size_pt(pt: f32) -> f32 {
    pt.clamp(FONT_SIZE_PT_MIN, FONT_SIZE_PT_MAX)
}

/// Where the viewport currently sits relative to the live tail.
///
/// Scrollback position. Defined in [`crate::scroll`] as a layer-free value
/// type and re-exported here for backward compatibility; `mux::window_group`
/// imports it from `crate::scroll` so the pure mux model does not depend on
/// the `app` layer (no `app ↔ mux` cycle).
pub use crate::scroll::ScrollPosition;

/// Direction for [`App::jump_to_prompt`]: `Prev` scrolls toward older
/// prompts (up), `Next` toward newer prompts (down).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpDirection {
    Prev,
    Next,
}

/// Result of dispatching a mux prefix action ([`App::dispatch_mux_action`]).
/// The caller (the key path in `window_host`) reacts to it: redraw on
/// `Changed`, open the corresponding egui dialog on `OpenRename` /
/// `OpenMove`, tear down the group on `Detach`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MuxActionOutcome {
    /// The action did not apply (no mux tab, single-window switch, unknown
    /// follow-up, …). Caller does nothing.
    None,
    /// Local mux state changed and a control message was sent; redraw.
    Changed,
    /// A `Detach` was sent; the daemon's `Detached` reply dissolves the
    /// group through the inbound path.
    Detach,
    /// Open the rename dialog for the window with this stable id, seeded with
    /// `current_name`. Confirmed via [`App::confirm_mux_rename`].
    OpenRename {
        window_id: u32,
        current_name: String,
    },
    /// Open the move dialog for the window with this stable id. Confirmed via
    /// [`App::confirm_mux_move`].
    OpenMove {
        window_id: u32,
        current_position: usize,
        window_count: usize,
    },
}

/// Build the mux prefix latch from settings: the prefix chord
/// (`mux_prefix_key`, falling back to `Ctrl+Z` on a parse error) and the
/// action bindings (`mux.keybinds`).
fn build_mux_latch(settings: &Settings) -> crate::mux::prefix::Latch {
    use crate::mux::prefix::{ActionBindings, Latch, PrefixChord, parse_prefix_key};
    let chord = parse_prefix_key(&settings.mux_prefix_key).unwrap_or_else(|| {
        log::warn!(
            "settings.mux.prefix: invalid chord {:?}, falling back to Ctrl+Z",
            settings.mux_prefix_key
        );
        PrefixChord::default()
    });
    let bindings = ActionBindings::from_settings_map(&settings.mux.keybinds);
    Latch::with_bindings(chord, crate::mux::prefix::DEFAULT_ARMED_TIMEOUT, bindings)
}

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
}

/// How long (in egui frame-time seconds) the binary-mismatch restart toast
/// lingers before it auto-dismisses. Owned by this feature (deliberately NOT
/// the SFTP `TOAST_LINGER_SECS`) so the two toasts can diverge.
const RESTART_TOAST_LINGER_SECS: f64 = 4.0;

/// Single auto-dismissing toast prompting a restart after the running binary
/// no longer matches the on-disk binary. Mirrors the SFTP toast's monotonic
/// frame-time dismiss model (no wall-clock).
#[derive(Debug, Default)]
pub struct RestartToast {
    /// Frame-time at which the toast auto-dismisses. `None` while inactive.
    dismiss_at: Option<f64>,
}

impl RestartToast {
    /// (Re)arm the single toast: schedule dismissal at `now + linger`. A
    /// subsequent arm overwrites the prior instant (one toast, refreshed).
    fn arm(&mut self, now: f64) {
        self.dismiss_at = Some(now + RESTART_TOAST_LINGER_SECS);
    }

    /// Clear the toast once the frame time reaches its dismissal instant.
    fn prune(&mut self, now: f64) {
        if matches!(self.dismiss_at, Some(at) if now >= at) {
            self.dismiss_at = None;
        }
    }

    /// Whether the toast should currently be drawn.
    pub fn active(&self) -> bool {
        self.dismiss_at.is_some()
    }
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
        //
        // `mux.statusbar.commands` registers `{cmd:<name>}` consumers
        // for the mux templates. The WebView build runs those in the
        // mux daemon; native-poc is in-process and shares the same
        // CommandProvider with `statusbar.custom_commands`, so we merge
        // the mux entries here before spinning up workers. On a name
        // collision the app side wins (the mux entry is dropped with a
        // warn-log) so a `settings.json` typo can never silently
        // replace an app-defined command.
        //
        // SPEC mux-statusbar §FR1: "When `enabled` is `false`, daemon
        // skips all command execution, timer setup, and StatusUpdate
        // sending." We honour the same contract here by gating the merge
        // on `mux.statusbar.enabled` — when disabled, mux-only commands
        // never enter the shared CommandProvider, so a user keeping the
        // default `enabled=false` while pre-staging entries for a future
        // toggle does not see them silently start running via the app
        // statusbar's `{cmd:<name>}` templates.
        let mut runtime_statusbar = settings.statusbar.clone();
        if settings.mux.statusbar.enabled {
            for (name, cmd) in &settings.mux.statusbar.commands {
                if runtime_statusbar.custom_commands.contains_key(name) {
                    log::warn!(
                        "settings.mux.statusbar.commands.{name}: name collides with statusbar.custom_commands; mux entry dropped"
                    );
                    continue;
                }
                runtime_statusbar
                    .custom_commands
                    .insert(name.clone(), cmd.clone());
            }
        }
        let status_bar_runtime = StatusBarRuntime::new(
            &runtime_statusbar,
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

        Self {
            tabs: Vec::new(),
            active: 0,
            mux_latch,
            mux_dialog: crate::mux::dialog::MuxDialogState::Closed,
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
            previous_cursor: None,
            previous_selection: None,
            previous_visible_start: 0,
            needs_full_redraw: true,
            scroll_active_tab_into_view: false,
            force_full_redraw,
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
        }
    }

    /// Phase 4-H startup wiring (FR12 + FR6 + FR7 + FR11). Build the
    /// resolver, register bundled fonts, branch on
    /// `Settings::font_engine` to construct either the Swash or AbGlyph
    /// rasterizer, build the fallback chain, and seed the glyph cache.
    /// The returned tuple is owned by `App`; the renderer's
    /// `TerminalGridPass` borrows clones of each `Arc`.
    fn build_font_stack(
        settings: &Settings,
    ) -> (
        Arc<Resolver>,
        Arc<FallbackChain>,
        Arc<Mutex<GlyphCache>>,
        Arc<dyn GlyphRasterizer>,
        FontId,
    ) {
        #[cfg(not(test))]
        use crate::render::font::resolver::FontRole;

        let mut resolver = Resolver::new();
        let (bundled_cjk_id, emoji_id) = resolver.register_bundled();

        // Host-font preferences sourced from `settings.font_family_fallback`:
        //   fallback[0] -> base (Latin / monospace)
        //   fallback[1] -> CJK fallback
        // Built-in defaults ("Inconsolata" / "Noto Sans JP") apply
        // when `settings.json` does not override them, matching the
        // resolved family list the legacy WebView build picks for the
        // same configuration. The bundled CJK font's Latin sub-set is
        // not monospaced, so falling back to it for ASCII produces
        // visibly jagged grid alignment — the resolver returns `None`
        // when the requested family is absent on the host, and the
        // chain root degrades to the bundled CJK font in that case
        // (see `base_id` below).
        #[cfg(not(test))]
        let base_family = settings
            .font_family_fallback
            .first()
            .map(String::as_str)
            .unwrap_or("Inconsolata")
            .to_string();
        #[cfg(not(test))]
        let cjk_family = settings
            .font_family_fallback
            .get(1)
            .map(String::as_str)
            .unwrap_or("Noto Sans JP")
            .to_string();
        #[cfg(not(test))]
        let inconsolata_id = resolver.register_system_family(&base_family, FontRole::Base);
        #[cfg(test)]
        let inconsolata_id: Option<FontId> = None;

        // SGR-bold faces: register the real Bold cut of the base / CJK
        // families when the host has one. `None` (family ships no face
        // of weight ≥ 600) simply leaves bold cells on the regular face.
        #[cfg(not(test))]
        let base_bold_id = inconsolata_id
            .and_then(|_| resolver.register_system_family_bold(&base_family, FontRole::Base));
        #[cfg(test)]
        let base_bold_id: Option<FontId> = None;

        #[cfg(not(test))]
        let noto_sans_jp_id = resolver.register_system_family(&cjk_family, FontRole::Cjk);
        #[cfg(test)]
        let noto_sans_jp_id: Option<FontId> = None;

        #[cfg(not(test))]
        let cjk_bold_id = noto_sans_jp_id
            .and_then(|_| resolver.register_system_family_bold(&cjk_family, FontRole::Cjk));
        #[cfg(test)]
        let cjk_bold_id: Option<FontId> = None;

        // Optional host-installed emoji font preference: when
        // `settings.emoji_font` is set, try the named family and use it
        // as the preferred color emoji source so users on platforms
        // that ship a newer Noto Color Emoji (or a different family
        // entirely, e.g. Apple Color Emoji on macOS) get the host
        // glyphs instead of the bundled `NotoColorEmoji.ttf`. The
        // bundled font remains in the chain as a last-resort fallback.
        #[cfg(not(test))]
        let host_emoji_id = settings
            .emoji_font
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .and_then(|family| resolver.register_system_family(family, FontRole::Emoji));
        #[cfg(test)]
        let host_emoji_id: Option<FontId> = None;

        // Symbol fallback families: cover prompt arrows / math symbols
        // / geometric shapes the base monospace + CJK + emoji fonts
        // miss (e.g. `❯` U+276F shown by starship, ✱ U+2731, ▰ U+25B0
        // bars used by some TUIs). On Linux `fc-match :charset=276f`
        // typically resolves to `Noto Sans Symbols2`; on systems
        // without it `DejaVu Sans` catches a wide BMP range. Registered
        // as `Secondary` so they sit at the chain tail and only get
        // consulted when every earlier role misses. Each call is
        // silent on absence — missing families just leave the slot
        // unfilled.
        #[cfg(not(test))]
        for family in [
            "Noto Sans Symbols2",
            "Symbola",
            "DejaVu Sans Mono",
            "DejaVu Sans",
        ] {
            let _ = resolver.register_system_family(family, FontRole::Secondary);
        }

        // Best-effort wider system scan (logged at WARN on failure;
        // family-name only, byte loading is deferred). Tests skip the
        // scan to keep cargo test deterministic.
        #[cfg(not(test))]
        resolver.scan_system_fonts();

        let rasterizer: Arc<dyn GlyphRasterizer> = match settings.font_engine {
            FontEngine::Swash => {
                let swash = Arc::new(
                    crate::render::font::swash_adapter::SwashRasterizer::with_axes(
                        &settings.variable_font_axes,
                    ),
                );
                swash.ingest_resolver(&resolver);
                log::info!(
                    "font.antialias = {}",
                    if swash.subpixel() {
                        "subpixel-rgb"
                    } else {
                        "grayscale (EMTERM_SUBPIXEL=0)"
                    }
                );
                swash
            }
            FontEngine::AbGlyph => {
                // ab_glyph escape hatch: we wrap the bundled CJK font
                // (which carries a Latin sub-set) so ASCII still
                // renders. CJK / emoji return None and the fallback
                // chain stops — that is the documented degradation
                // path (FR5).
                if !settings.variable_font_axes.is_empty() {
                    log::warn!(
                        "font.variable_axes: ignored under font_engine = ab_glyph (swash only)"
                    );
                }
                match crate::render::font::ab_glyph_adapter::AbGlyphRasterizer::from_static_bytes(
                    crate::render::font::resolver::BUNDLED_CJK_FONT,
                    bundled_cjk_id,
                ) {
                    Some(r) => {
                        log::info!("font_engine = ab_glyph (escape hatch); CJK / emoji may tofu");
                        Arc::new(r)
                    }
                    None => {
                        // Honor the "axes ignored under ab_glyph" contract
                        // even on this fallback: the user explicitly chose
                        // the no-variable-font escape hatch, so a parse
                        // failure that lands us back on swash must not
                        // silently re-enable the axes we just warned were
                        // off (`new()`, not `with_axes`).
                        log::warn!(
                            "font.unknown_engine: ab_glyph failed to parse bundled CJK; falling back to swash (variable_font_axes stay ignored)"
                        );
                        let swash =
                            Arc::new(crate::render::font::swash_adapter::SwashRasterizer::new());
                        swash.ingest_resolver(&resolver);
                        swash
                    }
                }
            }
        };

        // Pick the chain root: prefer Inconsolata (monospace Latin) when
        // it loaded successfully; otherwise the bundled CJK font remains
        // the base so ASCII still renders (just less prettily).
        let base_id = inconsolata_id.unwrap_or(bundled_cjk_id);
        let mut extras: Vec<FontId> = Vec::new();
        if let Some(jp) = noto_sans_jp_id {
            extras.push(jp);
        }
        if base_id != bundled_cjk_id {
            // Keep the bundled CJK font as a last-resort CJK fallback
            // (covers KR / TC / SC / extended CJK that NSJP omits).
            extras.push(bundled_cjk_id);
        }
        // Symbol fallback families registered above as
        // FontRole::Secondary (`Noto Sans Symbols2`, `Symbola`, etc.)
        // occupy the `font_family_fallback...` slot in SPEC FR8's
        // `[base, font_family_fallback..., emoji_font]` chain order.
        // They cover codepoints the base + CJK fonts miss — most
        // visibly `❯` U+276F shown by starship.
        //
        // Note: for codepoints in `is_pictographic`'s range
        // (0x2600..=0x27BF + emoji blocks), `FallbackChain::resolve`
        // checks `self.emoji` FIRST regardless of chain order, so
        // Secondary only catches a pictographic codepoint when the
        // emoji font does NOT cover it. Today Noto Color Emoji omits
        // dingbat ornaments like U+276F / U+2731, so the Secondary
        // chain catches them as intended. If a future emoji font
        // gains dingbat coverage, the `resolve_pictographic_falls_to_
        // secondary_when_emoji_misses` regression test in
        // `render/font/fallback.rs` pins the "miss → Secondary"
        // contract so the regression surfaces immediately.
        #[cfg(not(test))]
        for f in resolver.by_role(FontRole::Secondary) {
            extras.push(f.id);
            log::info!("font.symbol = {} (id={:?})", f.family, f.id);
        }
        // Host-installed emoji font (if any) takes precedence over the
        // bundled one — both go in the chain so a codepoint absent
        // from the host font still falls through to bundled.
        if let Some(id) = host_emoji_id {
            extras.push(id);
        }
        extras.push(emoji_id);
        // Pick the preferred color-emoji source. Host wins only when
        // the rasterizer reports it actually carries color glyphs it
        // can paint — Windows' system "Noto Color Emoji" ships as
        // COLRv1 + SVG (no CBDT strikes), and swash advertises the
        // run as `Content::Color` but rasterizes nothing. Without
        // this guard the chain marker would point at an empty face
        // and every 🟢 / 😀 cluster would render as blank. Falling
        // back to the bundled CBDT/CBLC font keeps color emoji
        // visible on hosts whose system emoji font swash cannot
        // raster. The unusable host font stays in the chain so a
        // codepoint the bundled font misses still has a last-resort
        // monochrome fallback before tofu.
        let preferred_emoji_id = match host_emoji_id {
            Some(host) if rasterizer.has_color(host) => host,
            _ => emoji_id,
        };
        #[cfg(not(test))]
        if let Some(id) = inconsolata_id {
            log::info!("font.base = {} (id={:?})", base_family, id);
        } else {
            log::warn!(
                "font.base = bundled Noto Sans CJK JP ({:?} not found on host; ASCII will not be monospaced)",
                base_family
            );
        }
        #[cfg(not(test))]
        if let Some(id) = noto_sans_jp_id {
            log::info!("font.jp = {} (id={:?})", cjk_family, id);
        } else {
            log::warn!(
                "font.jp = bundled Noto Sans CJK JP ({:?} not found on host)",
                cjk_family
            );
        }
        match (host_emoji_id, settings.emoji_font.as_deref()) {
            (Some(id), Some(family)) if preferred_emoji_id == id => {
                log::info!("font.emoji = {} (id={:?})", family, id)
            }
            (Some(id), Some(family)) => log::warn!(
                "font.emoji = bundled Noto Color Emoji (requested {:?} id={:?} has no rasterizable color glyphs)",
                family,
                id,
            ),
            (None, Some(family)) => log::warn!(
                "font.emoji = bundled Noto Color Emoji (requested {:?} not found on host)",
                family
            ),
            _ => log::info!("font.emoji = bundled Noto Color Emoji (id={:?})", emoji_id),
        }
        let mut chain = FallbackChain::new(base_id, extras);
        // Mark the preferred emoji font as the color-emoji source so
        // VS-16-bearing clusters (e.g. ⚠️ = U+26A0 + U+FE0F) and bare
        // pictographs (✅ U+2705, 🟢 U+1F7E2) resolve to it instead of
        // the BW base / CJK fonts that may also cover those codepoints.
        chain.set_emoji(preferred_emoji_id);
        // Wire the real bold faces so SGR-bold cells render with them.
        #[cfg(not(test))]
        if let (Some(regular), Some(bold)) = (inconsolata_id, base_bold_id) {
            chain.set_bold_variant(regular, bold);
            log::info!("font.base.bold = {} (id={:?})", base_family, bold);
        }
        #[cfg(not(test))]
        if let (Some(regular), Some(bold)) = (noto_sans_jp_id, cjk_bold_id) {
            chain.set_bold_variant(regular, bold);
            log::info!("font.jp.bold = {} (id={:?})", cjk_family, bold);
        }
        #[cfg(test)]
        {
            // Silence unused-variable lints in the test cfg where the
            // bold ids are compile-time `None`.
            let _ = (base_bold_id, cjk_bold_id);
        }

        // Font smoke diagnostic — release/non-test only. Forces the
        // result into `emterm.log` even when the host has not enabled
        // `log_recording_enabled`, so a Windows user can hand us the
        // log file without editing settings.json first. Runs once at
        // startup so the cost is bounded.
        #[cfg(not(test))]
        {
            let probe_cp: u32 = 0x1F600; // 😀
            let covers = rasterizer.has_codepoint(preferred_emoji_id, probe_cp);
            let summary_chain: Vec<String> = chain
                .chain()
                .iter()
                .map(|id| {
                    resolver
                        .font(*id)
                        .map(|f| format!("{:?}={}", id, f.family))
                        .unwrap_or_else(|| format!("{:?}=?", id))
                })
                .collect();
            crate::logging::force_log_line(
                log::Level::Warn,
                &format!(
                    "font.diag.chain = [{}] base={:?} emoji={:?} covers_U+1F600={}",
                    summary_chain.join(", "),
                    base_id,
                    preferred_emoji_id,
                    covers,
                ),
            );
            log::warn!(
                "font.diag.chain = [{}] base={:?} emoji={:?} covers_U+1F600={}",
                summary_chain.join(", "),
                base_id,
                preferred_emoji_id,
                covers,
            );
            // Shape + raster smoke test for U+1F600 at a representative
            // terminal cell pixel size. Empty shape / None raster is the
            // first visible symptom on Windows; the log line tells us
            // which stage broke (charmap miss vs raster failure vs
            // bitmap-strike unavailable).
            let shaped = rasterizer.shape("\u{1F600}", preferred_emoji_id, 17.0);
            let first = shaped.into_iter().next();
            match first {
                None => {
                    let msg = format!(
                        "font.diag.smoke: shape returned no glyphs for U+1F600 (font={:?})",
                        preferred_emoji_id,
                    );
                    crate::logging::force_log_line(log::Level::Warn, &msg);
                    log::warn!("{}", msg);
                }
                Some(g) => match rasterizer.raster(g.font, g.glyph_id, g.size_px) {
                    None => {
                        let msg = format!(
                            "font.diag.smoke: raster returned None glyph_id={} size_px={} font={:?}",
                            g.glyph_id, g.size_px, g.font,
                        );
                        crate::logging::force_log_line(log::Level::Warn, &msg);
                        log::warn!("{}", msg);
                    }
                    Some(b) => {
                        let nonzero = match b.format {
                            crate::render::font::traits::AtlasFormat::Rgba => {
                                b.pixels.chunks_exact(4).filter(|px| px[3] != 0).count()
                            }
                            _ => b.pixels.iter().filter(|&&v| v != 0).count(),
                        };
                        let msg = format!(
                            "font.diag.smoke: raster ok format={:?} w={} h={} advance={:.1} bytes={} nonzero={}",
                            b.format,
                            b.width,
                            b.height,
                            b.advance,
                            b.pixels.len(),
                            nonzero,
                        );
                        crate::logging::force_log_line(log::Level::Warn, &msg);
                        log::warn!("{}", msg);
                    }
                },
            }
        }

        (
            Arc::new(resolver),
            Arc::new(chain),
            Arc::new(Mutex::new(GlyphCache::new())),
            rasterizer,
            base_id,
        )
    }

    /// Replace the IME backend installed by `App::new`. Called once by
    /// `window_host::run` after the tao window exists and
    /// `ImeBackendFactory::build` has chosen the right OS backend.
    ///
    /// Phase 4-G-A: any non-`NullBackend` backend reports its `name()`
    /// via the trait. The App tracks whether the current backend is
    /// the passthrough so the event-loop hook can gate
    /// `WindowEvent::ReceivedImeText` (Phase 4 commit path) on
    /// "NullBackend only" — real backends emit `ImeEvent::Commit`
    /// instead.
    pub fn set_ime_backend(&mut self, backend: Box<dyn ImeBackend>) {
        self.ime_is_null = backend.name() == "null";
        self.ime_backend = backend;
    }

    /// `true` when the currently installed backend is the passthrough
    /// `NullBackend`. Used by `window_host` to decide whether
    /// `WindowEvent::ReceivedImeText` should drive the commit path.
    #[allow(dead_code)] // exercised via tests; production caller is window_host
    pub fn ime_is_null(&self) -> bool {
        self.ime_is_null
    }

    /// Offer a raw key event to the active backend before the existing
    /// `tao_key_to_bytes` path runs. Returns the backend's
    /// `KeyDispatchResult`: `Consumed` → skip `tao_key_to_bytes`,
    /// `Passthrough` → continue with the Phase 4 path. SPEC.md FR6.
    pub fn dispatch_key_event_via_ime(&mut self, raw: &RawKeyEvent) -> KeyDispatchResult {
        self.ime_backend.dispatch_key_event(raw)
    }

    /// Forward focus state to the active backend. Wired from
    /// `WindowEvent::Focused(b)` in `window_host`. SPEC.md FR8.
    pub fn notify_ime_focus(&mut self, focused: bool) {
        self.ime_backend.notify_focus(focused);
    }

    /// Forward a winit `WindowEvent::Ime` payload to the active
    /// backend. The default `ImeBackend::on_winit_ime` impl is a
    /// no-op; only `WinitImeBridge` overrides it. SPEC.md FR11.
    pub fn pass_winit_ime(&mut self, ime: &winit::event::Ime) {
        self.ime_backend.on_winit_ime(ime);
    }

    /// Drain queued `ImeEvent`s from the active backend and route them
    /// through the existing Phase 4-E layer
    /// (`on_ime_preedit` / `on_ime_commit` / `on_ime_focus_lost`).
    /// Bounded to `PUMP_BUDGET` events per tick; overflow is dropped
    /// with a single warn log (latched). SPEC.md FR5 + IME_E901.
    pub fn pump_ime(&mut self) -> bool {
        let mut events: Vec<ImeEvent> = Vec::new();
        self.ime_backend.pump(&mut events);
        if events.is_empty() {
            return false;
        }
        if events.len() >= PUMP_BUDGET && !self.ime_overflow_warned {
            log::warn!(
                "ime pump reached PUMP_BUDGET ({PUMP_BUDGET}); overflow events dropped (IME_E901)"
            );
            self.ime_overflow_warned = true;
        }
        let n = events.len();
        for ev in events {
            match ev {
                ImeEvent::Preedit(text) => self.on_ime_preedit(&text),
                ImeEvent::Commit(text) => self.on_ime_commit(&text),
                ImeEvent::FocusOut => self.on_ime_focus_lost(),
            }
        }
        log::debug!("ime pump routed {n} event(s)");
        true
    }

    /// Push the active cursor cell (in pixels) to the IME backend
    /// **only** when the (row, col) actually changed. Rate-limits the
    /// `XICAttribute::XNSpotLocation` / `set_cursor_rectangle` /
    /// `ImmSetCompositionWindow` calls so frequent redraws on a static
    /// cursor don't flood the IM server. SPEC.md FR7.
    ///
    /// `cell_w_px` / `cell_h_px` and `origin_x_px` / `origin_y_px` must
    /// match what `window_host` actually uses to lay out the grid; the
    /// computed cursor rect is in physical pixels.
    pub fn notify_cursor_rect_if_changed(
        &mut self,
        cell_w_px: u32,
        cell_h_px: u32,
        origin_x_px: i32,
        origin_y_px: i32,
    ) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let (row, col) = {
            let core = tab.core.lock();
            (core.get_cursor_row(), core.get_cursor_col())
        };
        if self.ime_last_cursor_cell == Some((row, col)) {
            return;
        }
        self.ime_last_cursor_cell = Some((row, col));
        let x = (col as i32) * (cell_w_px as i32) + origin_x_px;
        let y = (row as i32) * (cell_h_px as i32) + origin_y_px;
        self.ime_backend
            .notify_cursor_rect(x, y, cell_w_px as i32, cell_h_px as i32);
    }

    /// True when the cursor should currently render its glyph. When the
    /// terminal disables blink the cursor is always considered visible
    /// here (terminal-visibility is gated separately in `draw_cursor`).
    pub fn blink_visible_now(&self, blink_enabled: bool) -> bool {
        if !blink_enabled {
            return true;
        }
        let phase = self.blink_started.elapsed().as_millis() / BLINK_HALF_MS;
        phase.is_multiple_of(2)
    }

    /// Reset the blink reference to "now" so the cursor enters its visible
    /// half-cycle. Use this when the user does something that should
    /// re-pin attention to the cursor (typing, paste, tab switch, focus
    /// regain).
    pub fn reset_blink_phase(&mut self) {
        self.blink_started = Instant::now();
        self.previous_blink_visible = true;
    }

    /// Progress (0.0–1.0) of the in-flight visual-bell flash, `None`
    /// when idle. `render::draw_terminal` maps this to the overlay's
    /// decaying alpha.
    pub fn visual_bell_progress(&self) -> Option<f32> {
        let started = self.visual_bell_started?;
        let t = started.elapsed().as_secs_f32() / (BELL_FLASH_MS as f32 / 1000.0);
        (t < 1.0).then_some(t)
    }

    /// True while a visual-bell flash needs frames. Polled in
    /// `about_to_wait` alongside [`App::needs_blink_repaint`] so the
    /// 150 ms decay animates even when no PTY / input event would
    /// otherwise request a redraw. Clears the latch once the flash
    /// expired — returning true one last time so the final frame erases
    /// the overlay.
    pub fn needs_bell_repaint(&mut self) -> bool {
        match self.visual_bell_started {
            None => false,
            Some(started) if started.elapsed().as_millis() as u64 >= BELL_FLASH_MS => {
                self.visual_bell_started = None;
                true
            }
            Some(_) => true,
        }
    }

    /// True when the cursor's blink half-cycle has crossed a boundary
    /// since the last paint and the cell needs to repaint to flip the
    /// on/off state. The event loop polls this in `about_to_wait` so a
    /// blinking cursor advances even when no PTY / IME / input event
    /// would otherwise dirty a row. Without this, `egui_ctx`'s
    /// `request_repaint_after` is silent (no callback bridges it back
    /// to `window.request_redraw()`), so the cursor would freeze at
    /// whatever phase the last paint landed on.
    pub fn needs_blink_repaint(&self) -> bool {
        // Blink is suppressed while the window is unfocused (the
        // outline cursor stays steady). Skip waking up for blink
        // transitions in that case — saves a redraw every 530 ms when
        // the user is working in another window.
        if !self.window_focused {
            return false;
        }
        let Some(tab) = self.tabs.get(self.active) else {
            return false;
        };
        let core = tab.core.lock();
        if !core.get_cursor_visible() {
            return false;
        }
        let blink_enabled = core.get_cursor_blink();
        if !blink_enabled {
            return false;
        }
        self.blink_visible_now(blink_enabled) != self.previous_blink_visible
    }

    /// Spawn the initial shell tab. Called once at startup.
    pub fn spawn_initial_tab(&mut self) {
        let dims = self.cell_size;
        let tab = Tab::spawn_shell(
            "shell",
            dims.cols,
            dims.rows,
            self.settings.scrollback_lines,
            self.settings.clone(),
            Some(self.status_bar_runtime.dispatcher()),
            Some(self.status_bar_runtime.cwd_provider()),
            self.notification_sink.clone(),
            None,
        );
        self.tabs.push(tab);
        self.active = 0;
        // A brand-new tab populated rows; ensure the first frame draws them.
        self.needs_full_redraw = true;
    }

    /// Spawn an additional shell tab using the global settings, switch to
    /// it, and request a repaint. Used by `AppAction::NewTabGlobal` and as
    /// the no-default-profile fallback of [`App::spawn_new_tab_profile_aware`].
    pub fn spawn_new_tab(&mut self) {
        self.spawn_new_tab_with_overrides(None);
    }

    /// Spawn an additional shell tab with optional profile spawn
    /// overrides, switch to it, and request a repaint.
    pub fn spawn_new_tab_with_overrides(
        &mut self,
        overrides: Option<crate::profiles::SpawnOverrides>,
    ) {
        let dims = self.cell_size;
        let tab = Tab::spawn_shell(
            "shell",
            dims.cols,
            dims.rows,
            self.settings.scrollback_lines,
            self.settings.clone(),
            Some(self.status_bar_runtime.dispatcher()),
            Some(self.status_bar_runtime.cwd_provider()),
            self.notification_sink.clone(),
            overrides,
        );
        // A fresh tab seeds its theme from `settings.font_size`; carry
        // the live zoom level over so a tab opened after the user zoomed
        // matches the existing tabs instead of snapping back to the
        // configured baseline.
        if (self.runtime_font_size_pt - self.settings.font_size).abs() >= f32::EPSILON {
            tab.theme.lock().font_size_pt = self.runtime_font_size_pt;
        }
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        // FR4-adjacent: a freshly created tab lands at the end of the strip,
        // which is off-screen when the tabs overflow. Raise the same one-shot
        // scroll-into-view flag the keyboard switch path uses so the new tab
        // surfaces next frame. Unlike an existing-tab mouse click (already
        // visible, so it must NOT scroll — see `switch_to_tab`), a tab the user
        // has not seen yet should always surface, whether opened via the `+`
        // button or a keybind. All new-tab paths funnel through here.
        self.scroll_active_tab_into_view = true;
        self.needs_full_redraw = true;
    }

    /// `new_tab` keybind: apply the `is_default` profile when one exists,
    /// otherwise spawn with the global settings. Port of the WebView's
    /// `keyboard-handler.ts::handleNewTab`. A profile that fails to
    /// resolve (unknown SSH connection / unconfigured ssh path) logs an
    /// error and spawns nothing, matching the WebView's alert-and-abort.
    pub fn spawn_new_tab_profile_aware(&mut self) {
        let Some(profile) = crate::profiles::default_profile(&self.settings.profiles) else {
            self.spawn_new_tab();
            return;
        };
        match crate::profiles::resolve_spawn(profile, &self.settings) {
            Ok(overrides) => self.spawn_new_tab_with_overrides(Some(overrides)),
            Err(e) => log::error!("profile {:?}: {e}", profile.name),
        }
    }

    /// `profile_selector` keybind: open the modal selector. No-op when no
    /// profiles are configured (WebView parity:
    /// `keyboard-handler.ts::handleProfileSelector`).
    pub fn open_profile_selector(&mut self) {
        if self.settings.profiles.is_empty() {
            return;
        }
        self.profile_selector.open();
        self.needs_full_redraw = true;
    }

    /// Tab-bar `+` button (`TabEvent::New`): spawn directly with the
    /// global settings when no profiles exist, otherwise open the
    /// new-tab chooser (a "Global Settings" row + each profile, with the
    /// default profile preselected). Port of the WebView's
    /// `tab-bar-ui.ts::handleNewTabClick` dialog.
    pub fn open_new_tab_chooser(&mut self) {
        if self.settings.profiles.is_empty() {
            self.spawn_new_tab();
            return;
        }
        // Preselect the default profile's row (chooser mode prepends a
        // "Global Settings" row, so the row<->profile offset lives in
        // `ProfileSelectorState::profile_row`). No default → row 0
        // (Global Settings).
        self.profile_selector.open_with_global(0);
        if let Some(i) = self.settings.profiles.iter().position(|p| p.is_default) {
            self.profile_selector.selected = self.profile_selector.profile_row(i);
        }
        self.needs_full_redraw = true;
    }

    /// Number of rows the open selector shows (profiles, plus the
    /// leading "Global Settings" row in new-tab chooser mode). Drives
    /// the keyboard wrap-around in `window_host`.
    pub fn profile_selector_row_count(&self) -> usize {
        self.settings.profiles.len() + usize::from(self.profile_selector.include_global)
    }

    /// Selector confirmed: resolve the chosen row and spawn a tab. The
    /// row→choice decode (including the chooser-mode "Global Settings"
    /// offset) lives in `ProfileSelectorState::row_to_choice`, the single
    /// authority shared with the renderer. Resolution failures log an
    /// error and spawn nothing (WebView parity with `launchSshProfile`'s
    /// alert path).
    pub fn confirm_profile_selection(&mut self, index: usize) {
        let choice = self.profile_selector.row_to_choice(index);
        self.profile_selector.close();
        let profile_index = match choice {
            crate::ui::profile_selector::Choice::Global => {
                self.spawn_new_tab();
                return;
            }
            crate::ui::profile_selector::Choice::Profile(i) => i,
        };
        let Some(profile) = self.settings.profiles.get(profile_index) else {
            return;
        };
        match crate::profiles::resolve_spawn(profile, &self.settings) {
            Ok(overrides) => self.spawn_new_tab_with_overrides(Some(overrides)),
            Err(e) => log::error!("profile {:?}: {e}", profile.name),
        }
    }

    /// Close the tab at `idx`. Returns `true` when the close emptied
    /// the tabs vector, signaling the app loop that the window should
    /// exit. `tabs.is_empty()` is the same signal; this is a
    /// convenience for code that needs to branch immediately after
    /// the close.
    /// Request closing the tab at `idx`. When that tab has active SFTP
    /// uploads, the close is deferred behind a confirmation dialog (the
    /// `close_guard` is armed and the tab stays open) and `false` is returned.
    /// Otherwise it closes immediately via [`Self::close_tab`].
    pub fn request_close_tab(&mut self, idx: usize) -> bool {
        if let Some(tab) = self.tabs.get(idx) {
            if self.sftp_service.has_active_for_tab(tab.stable_id) {
                // Store the tab's stable_id, not its index: the roster can
                // change (tabs added/removed/reordered) while the guard dialog
                // is open, which would invalidate a stored index (#7).
                self.sftp_ui.close_guard = Some(tab.stable_id);
                return false;
            }
        }
        self.close_tab(idx)
    }

    /// Confirm a guarded tab close: cancel the guarded tab's active uploads,
    /// then close it. The guard holds a stable_id, resolved to a current index
    /// here; if the tab is already gone the guard is cleared cleanly.
    /// Returns true when the close emptied the tabs vector.
    pub fn confirm_close_guard(&mut self) -> bool {
        let Some(id) = self.sftp_ui.close_guard.take() else {
            return false;
        };
        let Some(idx) = self.tabs.iter().position(|t| t.stable_id == id) else {
            // The guarded tab no longer exists; nothing to close.
            return self.tabs.is_empty();
        };
        // Cancel only the guarded tab's sessions.
        self.sftp_service.cancel_for_tab(id);
        self.close_tab(idx)
    }

    /// Dismiss the close-guard dialog without closing the tab.
    pub fn cancel_close_guard(&mut self) {
        self.sftp_ui.close_guard = None;
    }

    pub fn close_tab(&mut self, idx: usize) -> bool {
        if idx >= self.tabs.len() {
            return self.tabs.is_empty();
        }
        // Drop the tab — its `PtySession::Drop` impl kills the child
        // and joins reader/writer threads.
        self.tabs.remove(idx);
        // Closing a tab shifts the active buffer; any open search overlay
        // is now indexed into a buffer that may no longer be active, so
        // clear it (mirrors the switch_to_tab rationale).
        if self.search.visible {
            self.search.close();
        }
        if self.tabs.is_empty() {
            self.active = 0;
            self.needs_full_redraw = true;
            return true;
        }
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        } else if idx < self.active {
            // Removed tab was to the left of the active one; shift left.
            self.active -= 1;
        }
        self.needs_full_redraw = true;
        false
    }

    /// Switch to the tab at `idx` (no-op for out-of-range / same idx).
    pub fn switch_to_tab(&mut self, idx: usize) {
        if idx >= self.tabs.len() || idx == self.active {
            return;
        }
        // FR3: park the outgoing tab's live scroll position into its own slot,
        // then commit the new active index, then reload the incoming tab's
        // saved position into the single active value (`scroll_position`) the
        // renderer and the scroll mutators read. `Live` restores to the
        // bottom; `OffsetFromLive(n)` restores to that offset. The scroll-pin
        // hot path (wheel / PageUp) is untouched — it still reads/writes the
        // single `scroll_position`; only the switch boundary swaps it.
        if let Some(outgoing) = self.tabs.get_mut(self.active) {
            outgoing.scroll_position = self.scroll_position;
        }
        self.active = idx;
        // FR4: the scroll-into-view flag is intentionally NOT raised here.
        // `switch_to_tab` is also reached by mouse paths (`TabEvent::Switch`
        // and `MuxSwitch`), and FR4 fires only on keyboard activation. The
        // keyboard handlers (`NextTab` / `PrevTab` / `JumpTab`) raise the flag
        // themselves after confirming the active index actually moved.
        if let Some(incoming) = self.tabs.get(self.active) {
            self.scroll_position = incoming.scroll_position;
        }
        // The search overlay's matches are indexed into the previous
        // tab's scrollback + viewport, so they are meaningless against
        // the new tab. Close the overlay + clear state on every active-
        // tab change (the WebView keeps a single SearchHandler with no
        // per-tab restore hook in the basic build, so closing is the
        // faithful, non-stale behavior).
        if self.search.visible {
            self.search.close();
        }
        // The selection holds absolute-row buffer coordinates of the
        // *previous* tab, which are meaningless against the new tab's
        // buffer (same rationale as closing the search overlay above), so
        // drop it on every active-tab change.
        self.selection = None;
        // The pending press anchor is likewise scoped to the previous tab's
        // buffer, so drop it as well.
        self.pending_selection_anchor = None;
        self.needs_full_redraw = true;
    }

    /// Move the tab at `from` to land at position `to` (drag-and-drop
    /// reorder). `to` is the **insertion index** into the post-removal
    /// vector, in 0..=tabs.len(). No-op when:
    ///
    /// - `from` is out of range
    /// - `to` falls on either side of `from` (would land in the same
    ///   slot — `from` itself or immediately after it).
    ///
    /// The active index is fixed up so the same logical tab remains
    /// active after the move.
    pub fn reorder_tab(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() {
            return;
        }
        // Skip moves that would land the tab in its current slot.
        if to == from || to == from + 1 {
            return;
        }
        let to = to.min(self.tabs.len());
        let tab = self.tabs.remove(from);
        // After `remove`, indices >= `from` shifted down by 1; adjust
        // the insertion point to match the user's intent.
        let insert_at = if to > from { to - 1 } else { to };
        self.tabs.insert(insert_at, tab);

        // Fix up `self.active` so the logically-active tab follows the
        // move. Four cases:
        //   - The moved tab was active → it now lives at `insert_at`.
        //   - Active was to the left of `from` and to the left of
        //     `insert_at` → unchanged.
        //   - Active was to the right of `from` and to the right of
        //     `insert_at` → unchanged.
        //   - Otherwise the active tab shifts by ±1.
        if self.active == from {
            self.active = insert_at;
        } else if from < self.active && insert_at >= self.active {
            self.active -= 1;
        } else if from > self.active && insert_at <= self.active {
            self.active += 1;
        }

        self.needs_full_redraw = true;
    }

    /// Open the settings window (child `--settings` process). Replaces
    /// the former in-app egui settings tab: the WebView settings panel
    /// runs in its own window, and saves are applied live through the
    /// launcher's save-event watcher.
    pub fn open_settings_window(&mut self) {
        self.settings_launcher.open();
    }

    /// Apply a [`crate::ui::TabEvent`] emitted by the tab bar widget.
    /// Returns `true` when the resulting state should exit the window
    /// (i.e. the last tab was closed).
    pub fn apply_tab_event(&mut self, evt: crate::ui::TabEvent) -> bool {
        // Mux rename / move dialogs commit against `self.tabs.get_mut(self.active)`
        // (they only carry a `window_id`, not an owning-tab anchor). Any tab
        // event that mutates `self.tabs` / `self.active` (creation, close,
        // reorder, switch, mux-switch — and any future variant we add) makes
        // the captured anchor stale or out-of-range, so the conservative
        // choice is to drop *every* tab event while a dialog is open. Fail
        // closed: a new `TabEvent` variant added later defaults to "blocked
        // while dialog open", not "silently leaks past the guard".
        if self.mux_dialog_open() {
            return false;
        }
        match evt {
            crate::ui::TabEvent::New => {
                // `+` button: plain spawn without profiles, otherwise the
                // new-tab chooser modal (WebView `handleNewTabClick`).
                self.open_new_tab_chooser();
                false
            }
            crate::ui::TabEvent::OpenSettings => {
                self.open_settings_window();
                false
            }
            crate::ui::TabEvent::Close(idx) => self.request_close_tab(idx),
            crate::ui::TabEvent::Switch(idx) => {
                if idx < self.tabs.len() {
                    self.switch_to_tab(idx);
                }
                false
            }
            crate::ui::TabEvent::Reorder { from, to } => {
                if from >= self.tabs.len() {
                    return false;
                }
                self.reorder_tab(from, to.min(self.tabs.len()));
                false
            }
            crate::ui::TabEvent::MuxSwitch { tab, window } => {
                // Sub-tab click: focus the tab if needed, then switch the
                // mux window (local active index + `SwitchWindow`), mirroring
                // the keyboard `prefix 0..9` path (FR5).
                if tab < self.tabs.len() {
                    if self.active != tab {
                        self.switch_to_tab(tab);
                    }
                    // Swap the active scroll value through the per-pane slots
                    // (FR3) and force a full redraw on a committed switch
                    // (FR2), mirroring the keyboard prefix path.
                    let mut scroll = self.scroll_position;
                    let outcome = match self.tabs.get_mut(self.active) {
                        Some(t) => Self::switch_to(t, Some(window), &mut scroll),
                        None => MuxActionOutcome::None,
                    };
                    if outcome == MuxActionOutcome::Changed {
                        self.scroll_position = scroll;
                        self.needs_full_redraw = true;
                    }
                }
                false
            }
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
        });
        self.needs_full_redraw = true;
    }

    /// Increase the runtime terminal font size by one point (clamped to
    /// [`FONT_SIZE_PT_MIN`]..=[`FONT_SIZE_PT_MAX`]). Returns `true` when
    /// the size actually changed so the caller can reshape the grid.
    pub fn zoom_in(&mut self) -> bool {
        self.set_font_size_pt(self.runtime_font_size_pt + FONT_SIZE_PT_STEP)
    }

    /// Decrease the runtime terminal font size by one point (clamped).
    /// Returns `true` when the size actually changed.
    pub fn zoom_out(&mut self) -> bool {
        self.set_font_size_pt(self.runtime_font_size_pt - FONT_SIZE_PT_STEP)
    }

    /// Reset the runtime terminal font size back to the configured
    /// `settings.font_size`. Returns `true` when the size actually
    /// changed.
    pub fn zoom_reset(&mut self) -> bool {
        self.set_font_size_pt(self.settings.font_size)
    }

    /// Set the runtime terminal font size to `new_pt` (clamped to
    /// [`FONT_SIZE_PT_MIN`]..=[`FONT_SIZE_PT_MAX`]). On a real change:
    /// re-derive `cell_w_logical` / `cell_h_logical` from the font stack
    /// at the new pixel size, push the new point size into every tab's
    /// `Theme` (leaving the rest of each theme's OSC-mutated state
    /// intact), force a full redraw, and return `true`. Returns `false`
    /// (no mutation) when the clamped target equals the current size.
    ///
    /// The PTY grid is *not* reshaped here — the caller (`window_host`)
    /// owns the window pixel size and triggers a deferred resize so the
    /// new cell metrics produce the right `(cols, rows)` on the next
    /// frame.
    pub fn set_font_size_pt(&mut self, new_pt: f32) -> bool {
        let clamped = clamp_font_size_pt(new_pt);
        if (clamped - self.runtime_font_size_pt).abs() < f32::EPSILON {
            return false;
        }
        self.runtime_font_size_pt = clamped;
        // Re-derive cell metrics at the new size. `font_size_px` applies
        // the same 96/72 pt→px conversion `settings.font_size_px()` uses
        // at startup so the grid stays consistent with the WebView build.
        let new_px = clamped * crate::settings::PT_TO_PX;
        let (cell_w, cell_h) = crate::render::compute_cell_dims(
            self.font_rasterizer.as_ref(),
            self.font_fallback.as_ref(),
            new_px,
        );
        self.cell_w_logical = cell_w;
        self.cell_h_logical = cell_h;
        // Push the new point size into every tab's theme. Only
        // `font_size_pt` is touched so OSC-driven palette / cursor
        // mutations a tab accumulated are preserved.
        for tab in &self.tabs {
            tab.theme.lock().font_size_pt = clamped;
        }
        self.needs_full_redraw = true;
        true
    }

    /// Apply a settings draft committed by the in-app settings panel to
    /// the running app. Re-derives every startup-resolved state that
    /// the panel's categories can affect; settings that only bind at
    /// tab spawn time (`shell_path` / `shell_args` /
    /// `scrollback_lines`) intentionally reach new tabs only, matching
    /// the WebView build.
    ///
    /// Returns `true` when the caller (`window_host`) must reshape the
    /// window grid (cell metrics or padding changed).
    pub fn apply_settings(&mut self, mut new: Settings) -> bool {
        crate::settings_store::clamp_for_save(&mut new);
        let old = Arc::clone(&self.settings);

        // The profile selector / new-tab chooser renders its rows live
        // from `self.settings.profiles` and its highlight index is bound
        // to that list. A settings save (from the external WebView
        // settings window) can add / remove / reorder profiles while the
        // modal is open, leaving `selected` pointing past the new list or
        // at a different profile than the highlighted row. Close it so the
        // user never confirms against a stale list (the WebView rebuilt
        // the list on every open; closing is the equivalent invariant).
        if self.profile_selector.visible {
            self.profile_selector.close();
            self.needs_full_redraw = true;
        }

        // UI chrome palette: preset × brightness swaps live (the md3
        // slot is process-wide, so the next frame re-skins every
        // widget).
        crate::ui::md3::set_preset(new.ui_theme_preset, new.ui_theme);
        // Keybinds / locale resolve the same way as startup.
        self.keybinds = crate::ui::keybinds::KeybindTable::from_settings(&new.keybinds);
        self.locale = crate::i18n::resolve(new.language);

        // mux: rebuild the prefix latch (chord + action bindings) from the
        // new settings (FR11 dynamic apply). The tab group always renders its
        // windows as sub-tabs (WebView parity), so there is no expand
        // preference to push onto tabs.
        self.mux_latch = build_mux_latch(&new);

        let font_families_changed = new.font_family_fallback != old.font_family_fallback
            || new.emoji_font != old.emoji_font;
        let font_size_changed = (new.font_size - old.font_size).abs() >= f32::EPSILON;
        let padding_changed = new.padding != old.padding;

        self.settings = Arc::new(new);

        // Reflect the (possibly changed) SFTP concurrency cap onto the live
        // pool so reload takes effect without restarting in-flight uploads.
        self.sftp_service
            .set_max_concurrent(self.settings.sftp_max_concurrent_uploads);

        if font_families_changed {
            let (resolver, fallback, cache, rasterizer, base_id) =
                Self::build_font_stack(&self.settings);
            self.font_resolver = resolver;
            self.font_fallback = fallback;
            self.font_cache = cache;
            self.font_rasterizer = rasterizer;
            self.font_base_id = base_id;
        }

        // Re-derive cell metrics. A font_size change routes through
        // `set_font_size_pt` (which also pushes the size into every tab
        // theme); a pure family swap keeps the size but the new chain's
        // advance may differ, so recompute the dims in place.
        if font_size_changed {
            self.set_font_size_pt(self.settings.font_size);
        } else if font_families_changed {
            let px = self.runtime_font_size_pt * crate::settings::PT_TO_PX;
            let (w, h) = crate::render::compute_cell_dims(
                self.font_rasterizer.as_ref(),
                self.font_fallback.as_ref(),
                px,
            );
            self.cell_w_logical = w;
            self.cell_h_logical = h;
        }

        // Rebuild every tab's theme from the new settings (color scheme
        // / cursor style / bold-brighten), preserving the live zoom
        // level. OSC-driven palette mutations a tab accumulated are
        // reset — same outcome as the WebView's
        // `applyTerminalColorScheme` full remap.
        for tab in &mut self.tabs {
            let mut theme = crate::render::theme::Theme::from_settings(self.settings.as_ref());
            theme.font_size_pt = self.runtime_font_size_pt;
            *tab.theme.lock() = theme;
            {
                let mut core = tab.core.lock();
                core.set_cursor_blink(self.settings.cursor_blink);
                core.mark_all_dirty();
            }
            tab.set_fold_enabled(self.settings.fold_enabled);
        }

        self.needs_full_redraw = true;
        font_size_changed || font_families_changed || padding_changed
    }

    #[allow(dead_code)] // retained for window_host / tests
    pub fn has_tabs(&self) -> bool {
        !self.tabs.is_empty()
    }

    /// Build a per-frame view model for the status-bar widget. The
    /// runtime owns the template engine, providers, and OSC
    /// dispatcher; this method just snapshots the active tab's mux
    /// state into the runtime and refreshes the shared cwd cell.
    ///
    /// The render pipeline calls this once per frame and hands the
    /// result to [`crate::ui::status_bar::draw`].
    pub fn status_bar_view_model(&self) -> crate::status_bar::StatusBarViewModel {
        // Refresh the cwd snapshot the providers read through their
        // `CwdSource` closure. The lock is held only for the duration
        // of the swap.
        let active_cwd_value = self
            .active_tab()
            .and_then(|t| t.cb_state.lock().cwd.clone());
        *self.active_cwd.lock() = active_cwd_value;

        let (mux_session_name, mux_status) = match self.active_tab() {
            Some(t) => (t.mux_session_name.as_deref(), t.mux_status_state.as_ref()),
            None => (None, None),
        };

        // SPEC US5: App Line 1/2 keep rendering the app's own templates
        // while the mux daemon's StatusUpdate populates the OSC row.
        // `mux.statusbar.*` is consumed by the daemon (and the GUI's
        // `CommandProvider` merge at startup); the runtime does not need
        // it on the per-frame path.
        self.status_bar_runtime.build_view_model(
            &self.settings.statusbar,
            mux_session_name,
            mux_status,
        )
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

    // ── SFTP upload (drag & drop) ────────────────────────────────

    /// Binary-mismatch restart toast: a failed self-spawn (possibly off the
    /// App thread) sets a process-global flag. Consume it once per frame to
    /// arm/refresh the single toast, then auto-dismiss via frame time. Returns
    /// true when the toast state changed (so the caller can request a redraw).
    /// `now` is the egui frame time (monotonic, wall-clock-free).
    pub fn pump_restart_toast(&mut self, now: f64) -> bool {
        let mut changed = false;
        if crate::self_exec::restart_required() {
            self.restart_toast.arm(now);
            changed = true;
        }
        let was_active = self.restart_toast.active();
        self.restart_toast.prune(now);
        if was_active != self.restart_toast.active() {
            changed = true;
        }
        changed
    }

    /// Drain the SFTP progress + duplicate-check channels and update the UI.
    /// `now` is the current egui frame time (monotonic, wall-clock-free).
    /// Returns true when any toast/dialog state changed (so the caller can
    /// request a redraw).
    pub fn pump_sftp(&mut self, now: f64) -> bool {
        // The binary-mismatch restart toast shares this per-frame pump but is
        // an independent concern (see `pump_restart_toast`).
        let mut changed = self.pump_restart_toast(now);

        // Progress events → toasts.
        while let Ok(progress) = self.sftp_progress_rx.try_recv() {
            self.sftp_ui.toasts.apply(progress, now);
            changed = true;
        }
        // Auto-dismiss elapsed terminal toasts.
        let before = self.sftp_ui.toasts.toasts.len();
        self.sftp_ui.toasts.prune_expired(now);
        if self.sftp_ui.toasts.toasts.len() != before {
            changed = true;
        }

        // Duplicate-check results → overwrite dialog or direct upload.
        // pending_check is a map keyed by request_id so concurrent checks are
        // not clobbered (#13); a result with no matching entry (already
        // superseded/consumed) is simply ignored.
        while let Ok(result) = self.sftp_result_rx.try_recv() {
            changed = true;
            let Some(dialog) = self.sftp_ui.pending_check.remove(&result.request_id) else {
                continue;
            };
            match result.outcome {
                Ok(duplicates) => match crate::sftp::ui::confirm_branch(duplicates) {
                    crate::sftp::ui::ConfirmOutcome::StartUploads => {
                        self.start_uploads_with(
                            now,
                            dialog.tab_id,
                            dialog.connection,
                            dialog.paths,
                            dialog.remote_dir,
                        );
                    }
                    crate::sftp::ui::ConfirmOutcome::OpenOverwrite(dups) => {
                        self.sftp_ui.overwrite_dialog = Some(crate::sftp::ui::OverwriteDialog {
                            paths: dialog.paths,
                            remote_dir: dialog.remote_dir,
                            duplicates: dups,
                            tab_id: dialog.tab_id,
                            connection: dialog.connection,
                        });
                    }
                },
                Err(_e) => {
                    // The remote listing failed. Do NOT silently fall through to
                    // an upload (#12): surface the failure as a toast and abort,
                    // so an unverified destination never receives an implicit
                    // overwrite.
                    self.push_sftp_error_toast(
                        now,
                        "重複チェックに失敗したためアップロードを中止しました",
                        "Upload aborted: duplicate check failed",
                    );
                }
            }
        }

        changed
    }

    /// Route an aggregated drop batch by the active tab kind. Returns the
    /// drop target chosen (so the caller / tests can assert the branch).
    pub fn dispatch_drop(&mut self, paths: Vec<std::path::PathBuf>) -> crate::sftp::ui::DropTarget {
        // Capture the originating tab's identity (stable_id + resolved
        // connection) at drop time so the later confirm/overwrite paths do not
        // re-read the active tab, which may have changed.
        let identity = self.active_tab().filter(|t| t.is_ssh_tab()).and_then(|t| {
            let id = t.stable_id;
            t.ssh_connection(&self.settings)
                .map(crate::sftp::service::SftpConnection::from_ssh_connection)
                .map(|conn| (id, conn))
        });
        if let Some((tab_id, connection)) = identity {
            let remote_dir = self.active_tab_remote_dir();
            self.sftp_ui.upload_dialog = Some(crate::sftp::ui::UploadDialog {
                paths,
                remote_dir: remote_dir.clone(),
                tab_id,
                connection,
            });
            crate::sftp::ui::DropTarget::SshUpload { remote_dir }
        } else {
            let strs: Vec<String> = paths
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            let line = crate::sftp::remote_path::format_paths_for_paste(&strs);
            if let Some(tab) = self.active_tab() {
                tab.write(line.into_bytes());
            }
            crate::sftp::ui::DropTarget::Paste
        }
    }

    /// The remote upload directory derived from the active tab's OSC 7 CWD.
    fn active_tab_remote_dir(&self) -> String {
        let cwd = self
            .active_tab()
            .and_then(|tab| tab.cb_state.lock().cwd.clone())
            .unwrap_or_default();
        crate::sftp::remote_path::extract_remote_path(&cwd)
    }

    /// Confirm the upload dialog: request an off-thread duplicate check for the
    /// pending paths. The result channel pump branches to overwrite or upload.
    ///
    /// Uses the connection/tab identity captured in the dialog at drop time —
    /// it never re-reads the active tab — so switching tabs between drop and
    /// confirm cannot redirect the upload (#4).
    pub fn confirm_upload_dialog(&mut self, now: f64) {
        let Some(dialog) = self.sftp_ui.upload_dialog.take() else {
            return;
        };
        // Guard: the originating tab must still exist and still be an SSH tab.
        if !self.tab_is_still_ssh(dialog.tab_id) {
            self.push_sftp_error_toast(
                now,
                "アップロード対象のタブが見つかりません",
                "Upload target tab is no longer available",
            );
            return;
        }
        let file_names: Vec<String> = dialog
            .paths
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .collect();
        let req_id = self.sftp_ui.next_request_id();
        let connection = dialog.connection.clone();
        let remote_dir = dialog.remote_dir.clone();
        self.sftp_ui.pending_check.insert(req_id, dialog);
        self.sftp_service
            .check_duplicates(req_id, connection, remote_dir, file_names);
    }

    /// Confirm the overwrite dialog: start the uploads despite the duplicates,
    /// using the dialog's captured identity (not the active tab).
    pub fn confirm_overwrite_dialog(&mut self, now: f64) {
        let Some(dialog) = self.sftp_ui.overwrite_dialog.take() else {
            return;
        };
        self.start_uploads_with(
            now,
            dialog.tab_id,
            dialog.connection,
            dialog.paths,
            dialog.remote_dir,
        );
    }

    /// Cancel a running upload (toast cancel control).
    pub fn cancel_sftp_upload(&mut self, session_id: &str) {
        self.sftp_service.cancel(session_id);
        self.sftp_ui.toasts.remove(session_id);
    }

    /// Whether the tab with `tab_id` still exists and is still an SSH tab.
    fn tab_is_still_ssh(&self, tab_id: u64) -> bool {
        self.tabs
            .iter()
            .find(|t| t.stable_id == tab_id)
            .map(|t| t.is_ssh_tab())
            .unwrap_or(false)
    }

    /// Surface an SFTP error to the user as a transient failure toast. Uses a
    /// synthetic session id so it slots into the same toast stack and
    /// auto-dismisses like a real terminal-state toast.
    fn push_sftp_error_toast(&mut self, now: f64, ja: &'static str, en: &'static str) {
        let msg = match self.locale {
            crate::i18n::Locale::Ja => ja,
            crate::i18n::Locale::En => en,
        };
        let session_id = format!("sftp-error-{}", self.sftp_ui.next_request_id());
        self.sftp_ui.toasts.apply(
            crate::sftp::SftpUploadProgress {
                session_id,
                file_name: msg.to_string(),
                bytes_transferred: 0,
                total_bytes: 0,
                status: crate::sftp::SftpUploadStatus::Failed,
                error_message: Some(msg.to_string()),
            },
            now,
        );
    }

    /// Start one upload per path in the batch against the captured identity.
    /// The originating tab is re-validated (existence + still SSH); if it is
    /// gone the batch is dropped with an error toast instead of being
    /// redirected to whatever tab happens to be active.
    fn start_uploads_with(
        &mut self,
        now: f64,
        tab_id: u64,
        connection: crate::sftp::service::SftpConnection,
        paths: Vec<std::path::PathBuf>,
        remote_dir: String,
    ) {
        if !self.tab_is_still_ssh(tab_id) {
            self.push_sftp_error_toast(
                now,
                "アップロード対象のタブが見つかりません",
                "Upload target tab is no longer available",
            );
            return;
        }
        for path in paths {
            let is_directory = crate::sftp::remote_path::is_directory(&path);
            let local_path = path.to_string_lossy().to_string();
            let _ = self.sftp_service.start_upload(
                tab_id,
                connection.clone(),
                local_path,
                remote_dir.clone(),
                is_directory,
            );
        }
    }

    #[allow(dead_code)] // retained for future mutation paths / tests
    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active)
    }

    /// Dispatch a mux prefix action against the active tab. Switch / new /
    /// detach are handled here; rename / move open dialogs (Phase 4) and are
    /// surfaced via [`MuxActionOutcome`] for the caller to drive. Returns the
    /// outcome so the caller (window_host) can react (open a dialog, redraw).
    ///
    /// Port of `handleMuxAction` in `mux-action-handler.ts`, fused with the
    /// `switchMuxWindow` index math (the native build sends `SwitchWindow`
    /// and lets the daemon snapshot swap the screen, so there is no local
    /// grid save/restore here).
    pub fn dispatch_mux_action(
        &mut self,
        action: crate::mux::prefix::PrefixAction,
    ) -> MuxActionOutcome {
        use crate::mux::prefix::PrefixAction;
        use mux_ipc::protocol::{CreateWindowPayload, MessageType, MuxMessage};

        let Some(tab) = self.tabs.get_mut(self.active) else {
            return MuxActionOutcome::None;
        };
        if tab.mux_group.is_none() {
            return MuxActionOutcome::None;
        }

        // FR3: the switch arms swap the App's single active scroll value
        // through `switch_to`. Copy it into a local so the `tab` borrow above
        // does not conflict with `&mut self.scroll_position`; write the
        // (possibly updated) value back after the match and force a full
        // redraw on a committed switch (FR2) below.
        let mut scroll = self.scroll_position;
        // FR4: capture the active window index before any switch so the
        // scroll-into-view flag can be raised strictly on a real window change
        // (TS-9 option b: a same-window digit jump reports `Changed` but does
        // not move `active`, so it must not raise the flag).
        let active_before = tab.mux_group.as_ref().map(|g| g.active_index());
        let outcome = match action {
            PrefixAction::None | PrefixAction::Literal => MuxActionOutcome::None,
            PrefixAction::Detach => {
                tab.send_control(&MuxMessage {
                    msg_type: MessageType::Detach,
                    pane_id: 0,
                    payload: Vec::new(),
                });
                MuxActionOutcome::Detach
            }
            PrefixAction::NewWindow => {
                tab.mux_group.as_mut().unwrap().inc_pending_create();
                let sent = tab.send_control(&MuxMessage::control(
                    MessageType::CreateWindow,
                    0,
                    &CreateWindowPayload::default(),
                ));
                if !sent {
                    // No PTY — undo the optimistic pending credit so a later
                    // stray PaneCreated cannot append a phantom window.
                    tab.mux_group.as_mut().unwrap().take_pending_create();
                    return MuxActionOutcome::None;
                }
                MuxActionOutcome::Changed
            }
            PrefixAction::NextWindow => {
                let target = tab.mux_group.as_ref().unwrap().next_index();
                Self::switch_to(tab, target, &mut scroll)
            }
            PrefixAction::PrevWindow => {
                let target = tab.mux_group.as_ref().unwrap().prev_index();
                Self::switch_to(tab, target, &mut scroll)
            }
            PrefixAction::SelectWindow(d) => {
                let target = tab.mux_group.as_ref().unwrap().digit_index(d);
                Self::switch_to(tab, target, &mut scroll)
            }
            PrefixAction::RenameWindow => {
                // Re-resolved by the dialog handler; surface the active
                // window's stable id so the dialog can re-find it on confirm.
                match tab.mux_group.as_ref().unwrap().active_window() {
                    Some(w) => MuxActionOutcome::OpenRename {
                        window_id: w.id,
                        current_name: w.name.clone(),
                    },
                    None => MuxActionOutcome::None,
                }
            }
            PrefixAction::MoveWindow => {
                let group = tab.mux_group.as_ref().unwrap();
                if group.len() <= 1 {
                    return MuxActionOutcome::None;
                }
                match group.active_window() {
                    Some(w) => MuxActionOutcome::OpenMove {
                        window_id: w.id,
                        current_position: group.active_index() + 1,
                        window_count: group.len(),
                    },
                    None => MuxActionOutcome::None,
                }
            }
        };
        // The `tab` borrow has ended; commit the swapped scroll value and, on
        // a committed pane switch, force a full redraw so a shorter incoming
        // pane leaves no residual rows from the longer outgoing one (FR2).
        if outcome == MuxActionOutcome::Changed {
            self.scroll_position = scroll;
            self.needs_full_redraw = true;
            // FR4: raise the scroll-into-view flag only when the active window
            // index actually moved (TS-9 option b). `NewWindow` reports
            // `Changed` but appends asynchronously (awaiting the daemon's
            // `PaneCreated`), so `active` is unchanged this frame and the flag
            // stays down; the new window's sub-tab is scrolled in when the
            // daemon confirms it and the user is on it. A same-window digit
            // jump (`set_active_clamped` lands on the same index) also leaves
            // `active` unchanged, so the flag is not raised.
            let active_after = self
                .tabs
                .get(self.active)
                .and_then(|t| t.mux_group.as_ref())
                .map(|g| g.active_index());
            if active_before != active_after {
                self.scroll_active_tab_into_view = true;
            }
        }
        outcome
    }

    /// React to a [`MuxActionOutcome`] from [`Self::observe_mux_key`]. Switch
    /// / new / detach already applied their effects in `dispatch_mux_action`;
    /// here we open the rename / move dialogs (Phase 4). `None` / `Changed` /
    /// `Detach` need no further action at this layer.
    pub fn handle_mux_outcome(&mut self, outcome: MuxActionOutcome) {
        match outcome {
            MuxActionOutcome::None | MuxActionOutcome::Changed | MuxActionOutcome::Detach => {}
            MuxActionOutcome::OpenRename {
                window_id,
                current_name,
            } => {
                self.open_mux_rename_dialog(window_id, current_name);
            }
            MuxActionOutcome::OpenMove {
                window_id,
                current_position,
                window_count,
            } => {
                self.open_mux_move_dialog(window_id, current_position, window_count);
            }
        }
    }

    /// Open the rename dialog for `window_id`, seeded with `current_name`.
    /// Reentry guard: a no-op if a rename dialog is already open (port of
    /// `renameDialogOpen`).
    pub fn open_mux_rename_dialog(&mut self, window_id: u32, current_name: String) {
        if self.mux_dialog.is_open() {
            return;
        }
        self.mux_dialog = crate::mux::dialog::MuxDialogState::Rename {
            window_id,
            name: current_name,
            focused_once: false,
        };
    }

    /// Open the move dialog for `window_id`. Reentry guard via the
    /// `MuxDialogState::Closed` discriminant (port of `moveDialogOpen`).
    pub fn open_mux_move_dialog(
        &mut self,
        window_id: u32,
        current_position: usize,
        window_count: usize,
    ) {
        if self.mux_dialog.is_open() {
            return;
        }
        self.mux_dialog = crate::mux::dialog::MuxDialogState::Move {
            window_id,
            current_position,
            window_count,
            target: current_position,
        };
    }

    /// Whether a mux rename / move dialog is currently open (the caller
    /// routes input to the dialog and swallows the PTY while it is).
    pub fn mux_dialog_open(&self) -> bool {
        self.mux_dialog.is_open()
    }

    /// Reconcile the open mux dialog against the active tab's current window
    /// group. Called by the render pipeline (`window_host::drive_mux_dialogs`)
    /// every frame before draw so a daemon-driven change that arrived while
    /// the dialog was open (a `PaneCreated` widened the window list, a
    /// `PtyExited` removed the captured window, a `SwitchWindow` shifted the
    /// current position) is reflected — instead of the dialog still showing
    /// a stale `current_position` / `window_count` captured at open time, or
    /// the user confirming a target the group can no longer accept.
    ///
    /// Outcomes:
    /// - The captured `window_id` no longer exists in the group → close the
    ///   dialog (`Closed`); the user's pending edit is silently discarded
    ///   since the target window is gone (parity with WebView, which closes
    ///   the dialog on a server-broadcast detach of the targeted window).
    /// - For the move dialog: refresh `current_position` to the captured
    ///   window's new 1-based index and `window_count` to `group.len()`, and
    ///   clamp `target` into the new `1..=window_count` range so an
    ///   in-flight edit can never confirm a value the group cannot accept.
    /// - For the rename dialog: nothing to refresh (the only display field is
    ///   the user-edited buffer).
    pub fn refresh_mux_dialog(&mut self) {
        use crate::mux::dialog::MuxDialogState;
        if matches!(self.mux_dialog, MuxDialogState::Closed) {
            return;
        }
        let Some(tab) = self.tabs.get(self.active) else {
            self.mux_dialog = MuxDialogState::Closed;
            return;
        };
        let Some(group) = tab.mux_group.as_ref() else {
            self.mux_dialog = MuxDialogState::Closed;
            return;
        };
        let captured_id = match &self.mux_dialog {
            MuxDialogState::Closed => return,
            MuxDialogState::Rename { window_id, .. } | MuxDialogState::Move { window_id, .. } => {
                *window_id
            }
        };
        let Some(idx) = group.index_of_window_id(captured_id) else {
            self.mux_dialog = MuxDialogState::Closed;
            return;
        };
        if let MuxDialogState::Move {
            current_position,
            window_count,
            target,
            ..
        } = &mut self.mux_dialog
        {
            let new_count = group.len();
            let new_current = idx + 1;
            *current_position = new_current;
            *window_count = new_count;
            *target = (*target).clamp(1, new_count.max(1));
        }
    }

    /// Feed one key event into the mux prefix latch and act on the result.
    /// Only intercepts when the active tab is mux-attached (has a
    /// `mux_group`); otherwise returns `(false, None)` so the caller falls
    /// through to the normal keybind / PTY path.
    ///
    /// Returns `(consumed, outcome)`:
    /// - `consumed = true` means the key was absorbed by the mux layer and
    ///   must NOT reach the keybind dispatch or PTY passthrough (covers
    ///   arming, the action follow-up, the double-prefix literal, and the
    ///   unknown-after-prefix ignore — FR2).
    /// - `outcome` is the action result (for the caller to open a dialog /
    ///   redraw); `None` when nothing actionable happened.
    ///
    /// `now` is the wall-clock instant (production passes `Instant::now()`;
    /// tests pass a synthetic clock). Port of `PrefixKeyHandler.handleKeyEvent`
    /// fused with `handleMuxAction` dispatch.
    pub fn observe_mux_key(
        &mut self,
        input: &crate::mux::prefix::KeyInput,
        now: Instant,
    ) -> (bool, MuxActionOutcome) {
        use crate::mux::prefix::PrefixAction;
        // Only a mux-attached active tab gets the prefix intercept.
        let is_mux = self
            .tabs
            .get(self.active)
            .map(|t| t.mux_group.is_some())
            .unwrap_or(false);
        if !is_mux {
            return (false, MuxActionOutcome::None);
        }
        let was_armed = self.mux_latch.is_armed();
        let action = self.mux_latch.observe(input, now);
        match action {
            PrefixAction::None => {
                // Two cases: (1) this key just armed the latch → consume it;
                // (2) it was an unknown follow-up after the prefix → consume
                // it (unknown-after-prefix is ignored, FR2). Otherwise the
                // key is unrelated and must fall through.
                if self.mux_latch.is_armed() || was_armed {
                    (true, MuxActionOutcome::None)
                } else {
                    (false, MuxActionOutcome::None)
                }
            }
            PrefixAction::Literal => {
                // Double-prefix: send the *configured* prefix's literal byte to
                // the active pane so programs that themselves use the chord
                // still receive it. Pre-fix this was hardcoded to
                // `DEFAULT_LITERAL_BYTE` (0x1A = Ctrl+Z), so a user with
                // `Ctrl+A` set would double-tap Ctrl+A and the pane would see
                // Ctrl+Z instead — silently breaking a nested tmux. Derive the
                // byte from the live chord via `Latch::literal_byte`.
                let byte = self.mux_latch.literal_byte();
                if let Some(tab) = self.tabs.get(self.active) {
                    tab.write_input(vec![byte]);
                }
                (true, MuxActionOutcome::None)
            }
            other => {
                let outcome = self.dispatch_mux_action(other);
                (true, outcome)
            }
        }
    }

    /// Apply a local switch to `target` (if any) and notify the daemon. The
    /// daemon-pushed snapshot swaps the on-screen content; here we only move
    /// the active index and send `SwitchWindow` with the new pane id. A
    /// `None` target (single window / out-of-range digit) is a no-op.
    ///
    /// Send-first / commit-after: when the PTY write fails (no PTY, broken
    /// pipe) the local active index is left untouched, mirroring the
    /// `NewWindow` rollback path. Pre-fix this committed the local switch
    /// unconditionally, leaving the UI and the pane-filter (`PtyOutput`
    /// drops bytes for non-active panes) pointing at a window the daemon
    /// never moved off of.
    ///
    /// `scroll` is the App's single active scroll value (`App::scroll_position`).
    /// On a committed switch this saves the outgoing pane's position into its
    /// per-pane slot, then reloads the incoming pane's saved position into
    /// `scroll` after the snapshot request (FR3 pane wiring). A failed send or
    /// empty group leaves `scroll` untouched.
    fn switch_to(
        tab: &mut Tab,
        target: Option<usize>,
        scroll: &mut crate::app::ScrollPosition,
    ) -> MuxActionOutcome {
        use mux_ipc::protocol::{MessageType, MuxMessage};
        let Some(idx) = target else {
            return MuxActionOutcome::None;
        };
        // Peek the target pane via shared borrow so a failed send does not
        // leave `group.active` already mutated.
        let pane_id = {
            let Some(group) = tab.mux_group.as_ref() else {
                return MuxActionOutcome::None;
            };
            let panes = group.pane_ids();
            if panes.is_empty() {
                return MuxActionOutcome::None;
            }
            let clamped = idx.min(panes.len() - 1);
            panes[clamped]
        };
        let sent = tab.send_control(&MuxMessage {
            msg_type: MessageType::SwitchWindow,
            pane_id,
            payload: Vec::new(),
        });
        if !sent {
            return MuxActionOutcome::None;
        }
        // Daemon accepted; now commit local state and pull the new window's
        // screen on demand (the daemon does not push the active grid
        // unprompted — parity with `switchMuxWindow`'s `requestPaneSnapshot`).
        if let Some(group) = tab.mux_group.as_mut() {
            // FR3: park the outgoing pane's live scroll position before the
            // active index moves, then reload the incoming pane's saved
            // position after committing the switch + requesting its snapshot,
            // so the restore lands together with the replayed content.
            group.set_active_pane_scroll(*scroll);
            group.set_active_clamped(idx);
        }
        tab.request_pane_snapshot(pane_id);
        if let Some(group) = tab.mux_group.as_ref() {
            *scroll = group.active_pane_scroll();
        }
        MuxActionOutcome::Changed
    }

    /// Confirm an optimistic rename: relabel the window with `window_id` (if
    /// it still exists) and notify the daemon with the active pane id. An
    /// empty name is a no-op (matching the WebView). Returns true when a
    /// rename was applied.
    pub fn confirm_mux_rename(&mut self, window_id: u32, name: String) -> bool {
        use mux_ipc::protocol::{MessageType, MuxMessage, RenameWindowMsg};
        // Trim then reject empty. The originating dialog already returns
        // `trimmed.to_string()`, but this is a `pub` entry point — a future
        // caller (CLI rebind, test, remote-confirm) could pass `"   "` and
        // the literal-empty guard alone would let a whitespace-only name
        // reach the daemon and the local `MuxWindow.name`. Make the contract
        // explicit.
        let name = name.trim().to_string();
        if name.is_empty() {
            return false;
        }
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return false;
        };
        let Some(group) = tab.mux_group.as_mut() else {
            return false;
        };
        let Some(idx) = group.index_of_window_id(window_id) else {
            return false; // window closed during the dialog
        };
        let pane_id = group.pane_ids()[idx];
        group.rename_window_id(window_id, name.clone());
        tab.send_control(&MuxMessage::control(
            MessageType::RenameWindow,
            pane_id,
            &RenameWindowMsg { name },
        ));
        true
    }

    /// Confirm an optimistic move: reorder the window with stable `window_id`
    /// to 1-based `target_position`, notify the daemon, and roll back the
    /// reorder if the send fails (the daemon does not broadcast order, so
    /// local state is authoritative). Returns true when a move was applied
    /// and survived (no rollback). Port of the `move-window` handler.
    pub fn confirm_mux_move(&mut self, window_id: u32, target_position: usize) -> bool {
        use mux_ipc::protocol::{MessageType, MoveWindowMsg, MuxMessage};
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return false;
        };
        let Some(group) = tab.mux_group.as_mut() else {
            return false;
        };
        let Some(from) = group.index_of_window_id(window_id) else {
            return false; // window closed during the dialog
        };
        let count = group.len();
        if target_position < 1 || target_position > count {
            return false; // out of range
        }
        let to = target_position - 1;
        if to == from {
            return false; // same position — no-op
        }
        let pane_id = group.pane_ids()[from];
        // Optimistic reorder so the UI updates immediately.
        group.reorder(from, to);
        let sent = tab.send_control(&MuxMessage::control(
            MessageType::MoveWindow,
            pane_id,
            &MoveWindowMsg {
                target_index: to as u32,
            },
        ));
        if !sent {
            // Roll back: re-insert from `to` back to `from`.
            if let Some(g) = tab.mux_group.as_mut() {
                g.reorder(to, from);
            }
            log::warn!("mux: MoveWindow send failed, reverted optimistic reorder");
            return false;
        }
        true
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
                crate::tabs::ScrollbackRestoreOutcome::Merged
                | crate::tabs::ScrollbackRestoreOutcome::Failed => {
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
        // the search overlay's cached document (H3).
        if changed {
            self.on_pty_output(active_changed);
        }
        // Reap exited tabs (Phase 5 will refine the policy).
        let before = self.tabs.len();
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
        }
        changed
    }

    /// Propagate a new grid size to all PTYs.
    pub fn set_grid_size(&mut self, cols: u16, rows: u16) {
        if self.cell_size.cols == cols && self.cell_size.rows == rows {
            return;
        }
        self.cell_size = GridDims { cols, rows };
        // A column-width change triggers a `term_core` reflow that rewrites
        // the logical↔physical line mapping (a height-only change does not —
        // `resize_same_width` keeps the wrap boundaries). Detect it before
        // the resize so the now-stale absolute-row trackers can be dropped
        // afterward (N3). All tabs share `cell_size`, so any one differing is
        // enough; checking all is harmless.
        let mut width_changed = false;
        for tab in &mut self.tabs {
            let old_cols = tab.core.lock().cols();
            if old_cols != cols {
                width_changed = true;
            }
            tab.resize(cols, rows);
        }
        if width_changed {
            // The reflow re-wrapped scrollback/viewport without moving the
            // eviction counter, so `pump_all`'s eviction-delta correction
            // cannot re-base the stored absolute rows. Drop every
            // absolute-row tracker: the App-global selection / pending anchor
            // here, and each tab's prompt / fold marks (which re-accumulate
            // from subsequent OSC 133 output).
            self.selection = None;
            self.pending_selection_anchor = None;
            for tab in &mut self.tabs {
                tab.clear_reflow_invalidated_state();
            }
        }
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

    /// FR4: whether the active tab/window cell should be scrolled into view
    /// this frame. Read by `render::draw_terminal` (immutable `&App`) and
    /// threaded into the tab strip; cleared post-frame by `window_host`.
    pub fn scroll_active_tab_into_view(&self) -> bool {
        self.scroll_active_tab_into_view
    }

    /// FR4: clear the one-shot scroll-into-view signal after the egui pass so
    /// it fires for exactly one frame and never re-fires on an unrelated
    /// repaint. Called from `window_host::render` where `&mut App` is held.
    pub fn clear_scroll_active_tab_into_view(&mut self) {
        self.scroll_active_tab_into_view = false;
    }

    // ── Scrollback (Phase 4 sub-phase 4) ─────────────────────

    /// Current scrollback offset in rows. `0` means `Live`. Saturates at the
    /// configured `scrollback_lines` ceiling.
    pub fn scroll_offset(&self) -> u32 {
        match self.scroll_position {
            ScrollPosition::Live => 0,
            ScrollPosition::OffsetFromLive(n) => n,
        }
    }

    /// Recompute the per-frame [`crate::fold::FoldLayout`] for the active
    /// tab. Called once at the top of `WindowHost::render` (before the
    /// immutable-borrow paint passes). Stores `Some(layout)` only when the
    /// active tab has at least one collapsed region — the renderer's
    /// fold-aware paths gate on `fold_layout().is_some()`, mirroring the
    /// WebView's `getCollapsedRegions().length > 0`. Otherwise stores `None`
    /// so the linear (non-folded) draw path runs unchanged.
    ///
    /// The viewport row count + scrollback length are read from the active
    /// tab's `TerminalCore`; the offset from [`Self::scroll_offset`].
    pub fn refresh_fold_layout(&mut self) {
        let scroll_offset = self.scroll_offset();
        // Snapshot the dimensions under the core lock, then build the layout
        // off the tab's `FoldManager` (which needs `&mut` for its collapsed
        // cache) without holding the core lock across the build.
        let dims = self.active_tab().map(|tab| {
            let core = tab.core.lock();
            (core.get_scrollback_length(), core.rows())
        });
        self.fold_layout = match (dims, self.active_tab_mut()) {
            (Some((scrollback_len, rows)), Some(tab)) => {
                if tab.folds.has_collapsed_regions() {
                    Some(tab.folds.build_layout(scrollback_len, rows, scroll_offset))
                } else {
                    None
                }
            }
            _ => None,
        };
    }

    /// The fold layout built for the current frame, if the active tab has
    /// collapsed regions. `None` selects the linear (non-folded) render
    /// path. See [`Self::refresh_fold_layout`].
    pub fn fold_layout(&self) -> Option<&crate::fold::FoldLayout> {
        self.fold_layout.as_ref()
    }

    /// Toggle (or expand) the fold region under a plain left-click at screen
    /// row `display_row` (0-based, counted from the top grid row). Port of
    /// the WebView `FoldHandler.handleFoldClick` (`handlers/fold.ts`); the
    /// caller (`window_host`) is responsible for the WebView's
    /// outer guards that have no equivalent in pure `App` state: a plain
    /// left-click (no Ctrl/Alt/Meta), with no active text selection, that
    /// did not become a drag-select.
    ///
    /// Behavior, mirroring the WebView:
    /// - Clicking a collapsed region's **summary row** expands that region.
    /// - Clicking inside an **expanded** foldable region collapses it. When
    ///   the region's summary sits *above* the current view top
    ///   (`regionDisplayLine < display_start`) the scroll offset is reduced
    ///   by `line_count - 1` (the rows the collapse hides) so the content
    ///   under the pointer stays visually anchored.
    /// - Clicking outside any region (or in scrollback below a region) is a
    ///   no-op.
    ///
    /// Returns `true` when fold state changed (the caller should request a
    /// redraw). `display_row >= rows` (a click below the grid) is rejected,
    /// matching the WebView's `displayRow < 0 || >= rows` guard.
    pub fn handle_fold_click(&mut self, display_row: u16) -> bool {
        // Snapshot the geometry under the core lock, then operate on the
        // fold manager (which needs `&mut` for its collapsed cache) without
        // holding the lock across the mutation — same pattern as
        // `refresh_fold_layout`.
        let scroll_offset = self.scroll_offset();
        let Some((scrollback_len, rows)) = self.active_tab().map(|tab| {
            let core = tab.core.lock();
            (core.get_scrollback_length(), core.rows())
        }) else {
            return false;
        };
        if display_row >= rows {
            return false;
        }
        let Some(tab) = self.active_tab_mut() else {
            return false;
        };
        if !tab.folds.is_enabled() {
            return false;
        }

        let total_actual = scrollback_len + rows as u32;
        let total_display = tab.folds.get_total_display_lines(total_actual);
        // display_start = max(0, total_display - rows - scroll_offset),
        // saturating to stay in u32 (matches `build_layout`).
        let display_start = total_display
            .saturating_sub(rows as u32)
            .saturating_sub(scroll_offset);
        let display_line = display_start + display_row as u32;

        // A click on a collapsed region's summary row expands it.
        if let Some(region) = tab.folds.get_summary_region(display_line) {
            tab.folds.expand_region_containing(region.start_line);
            self.needs_full_redraw = true;
            return true;
        }

        // Otherwise: a click inside an expanded foldable region collapses it.
        let actual_line = tab.folds.display_line_to_actual(display_line);
        let region = match tab.folds.get_region_at_line(actual_line) {
            Some(r) if !r.collapsed => (r.start_line, r.line_count),
            _ => return false,
        };
        let (region_start, line_count) = region;
        // Capture the region's display row BEFORE collapsing (matches the
        // WebView's `regionDisplayLine` computed before `toggleFold`).
        let region_display_line = tab.folds.actual_line_to_display(region_start);
        tab.folds.toggle_fold(actual_line);
        // Collapsing a region whose summary is above the view top hides
        // `line_count - 1` rows from the scrollback above us; pull the
        // offset down by the same amount so the click target stays put.
        if region_display_line < display_start {
            let delta = line_count - 1;
            let new_offset = scroll_offset.saturating_sub(delta);
            // Re-borrow `self` (the `tab` borrow ends here): `scroll_set_offset`
            // takes `&mut self` and clamps / snaps to Live at 0.
            self.scroll_set_offset(new_offset);
        }
        self.needs_full_redraw = true;
        true
    }

    /// Scroll back by `delta` rows (clamps to `scrollback_lines`). No-op
    /// when alt-screen is active.
    pub fn scroll_up_by(&mut self, delta: u32) {
        if self.alt_screen {
            return;
        }
        let new = self.scroll_offset().saturating_add(delta);
        let max = self.settings.scrollback_lines;
        self.scroll_position = if new == 0 {
            ScrollPosition::Live
        } else {
            ScrollPosition::OffsetFromLive(new.min(max))
        };
        self.needs_full_redraw = true;
    }

    /// Scroll forward (toward live tail) by `delta` rows. Snaps to `Live`
    /// when the resulting offset would be zero. No-op when alt-screen is
    /// active.
    pub fn scroll_down_by(&mut self, delta: u32) {
        if self.alt_screen {
            return;
        }
        let cur = self.scroll_offset();
        let new = cur.saturating_sub(delta);
        self.scroll_position = if new == 0 {
            ScrollPosition::Live
        } else {
            ScrollPosition::OffsetFromLive(new)
        };
        self.needs_full_redraw = true;
    }

    /// Jump to an absolute scrollback offset in rows back from live
    /// (`0` snaps to `Live`). Used by the scrollbar thumb, which
    /// computes its target against the *actual* scrollback length;
    /// the `scrollback_lines` clamp here is a safety net only. No-op
    /// when alt-screen is active.
    pub fn scroll_set_offset(&mut self, offset: u32) {
        if self.alt_screen {
            return;
        }
        self.scroll_position = if offset == 0 {
            ScrollPosition::Live
        } else {
            ScrollPosition::OffsetFromLive(offset.min(self.settings.scrollback_lines))
        };
        self.needs_full_redraw = true;
    }

    /// Jump to the top of the scrollback buffer (max offset).
    pub fn scroll_to_top(&mut self) {
        if self.alt_screen {
            return;
        }
        let max = self.settings.scrollback_lines;
        self.scroll_position = if max == 0 {
            ScrollPosition::Live
        } else {
            ScrollPosition::OffsetFromLive(max)
        };
        self.needs_full_redraw = true;
    }

    /// Jump back to the live tail.
    pub fn scroll_to_live(&mut self) {
        self.scroll_position = ScrollPosition::Live;
        self.needs_full_redraw = true;
    }

    // ── Prompt-to-prompt navigation (OSC 133) ────────────────

    /// Scroll to the nearest OSC 133 prompt mark in `direction` relative to
    /// the current view top. Port of the WebView
    /// `KeyboardHandler.handlePromptJump` (`handlers/keyboard.ts`).
    ///
    /// Coordinate model: marks carry an absolute scrollback-frame row
    /// (`0..scrollback_len` is scrollback, `scrollback_len..+rows` is the
    /// viewport). The current view top is `scrollback_len - scroll_offset`
    /// (offset is rows back from live). Scrolling to a mark sets the offset
    /// to `scrollback_len - mark_row`, putting the mark line at the top of
    /// the view — a viewport-row mark therefore resolves to a `Live`
    /// (`offset == 0`) position. When no mark exists in `direction`, fall
    /// to the top (`Prev`) or the live tail (`Next`), matching WebView.
    ///
    /// No-op on the alternate screen (same guard as the `scroll_*` family;
    /// alt-screen apps own the whole viewport and have no scrollback).
    ///
    /// Note: when the target prompt mark sits inside a *collapsed* fold
    /// region the region is auto-expanded first (mirroring the WebView
    /// `handlePromptJump`'s `foldManager.expandRegionContaining(marker.lineIndex)`),
    /// so the prompt is visible after the jump. The scroll offset is still
    /// computed against the mark's absolute row — expansion only changes the
    /// display↔actual mapping, not the buffer row the mark lives on, so the
    /// `scrollback_len - row` offset places the prompt at the view top either
    /// way.
    pub fn jump_to_prompt(&mut self, direction: JumpDirection) {
        if self.alt_screen {
            return;
        }
        let scrollback_len = match self.tabs.get(self.active) {
            Some(tab) => tab.core.lock().get_scrollback_length(),
            None => return,
        };
        let current_top_line = scrollback_len.saturating_sub(self.scroll_offset());
        let target = match self.tabs.get(self.active) {
            Some(tab) => match direction {
                JumpDirection::Prev => tab.prompts.find_prev_prompt(current_top_line),
                JumpDirection::Next => tab.prompts.find_next_prompt(current_top_line),
            },
            None => return,
        };
        match target {
            Some(row) => {
                // Auto-expand a collapsed fold region containing the mark so
                // jumping into folded output reveals the prompt. No-op when
                // the mark is in no region (or an already-expanded one).
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.folds.expand_region_containing(row);
                }
                let offset = scrollback_len.saturating_sub(row);
                self.scroll_set_offset(offset);
            }
            None => match direction {
                // No previous prompt: jump to the top of scrollback
                // (`scroll_set_offset` clamps to the configured ceiling).
                JumpDirection::Prev => self.scroll_set_offset(scrollback_len),
                // No next prompt: snap back to the live tail.
                JumpDirection::Next => self.scroll_set_offset(0),
            },
        }
    }

    // ── In-terminal search ───────────────────────────────────

    /// Open the search overlay (or re-focus it if already open). Sets the
    /// one-shot `search_focus_request` so the renderer grabs keyboard
    /// focus + selects the existing query on the next frame, mirroring
    /// `SearchHandler.toggleSearch` → `SearchBar.show()`.
    pub fn open_search(&mut self) {
        self.search.open();
        self.search_focus_request = true;
        self.needs_full_redraw = true;
    }

    /// Close the search overlay and clear its state + highlights.
    pub fn close_search(&mut self) {
        self.search.close();
        self.search_focus_request = false;
        self.needs_full_redraw = true;
    }

    /// Whether the search overlay is currently visible. Used by the key
    /// router in `window_host` to decide between the search-input path
    /// and the normal PTY/keybind path.
    pub fn search_visible(&self) -> bool {
        self.search.visible
    }

    /// Re-run the search against the active tab and update highlights.
    /// Called after the query / options change (incremental search). The
    /// current match is scrolled into view via [`Self::scroll_to_current_match`].
    pub fn run_search(&mut self) {
        if let Some(tab) = self.tabs.get(self.active) {
            let core = tab.core.lock();
            self.search.execute(&core);
        }
        self.scroll_to_current_match();
        self.needs_full_redraw = true;
    }

    /// Re-resolve the matches against the active tab's *current* buffer
    /// without scrolling. Called once per frame (after the pumps in the
    /// event loop) when [`SearchState::needs_research`] is set, so an open
    /// overlay's highlights keep tracking their text as new PTY output (or a
    /// resize) shifts rows into scrollback and changes their absolute row
    /// numbers.
    ///
    /// Time-throttled to one re-resolve per [`AUTO_RESEARCH_THROTTLE`]: a
    /// burst of PTY output flags the document dirty every frame, but
    /// rebuilding the logical-line document + recompiling the match that
    /// often is wasteful. When the gate blocks a run, the dirty flag is
    /// **kept** (we do not call `execute`), so the pending change is picked
    /// up on the next frame past the gap. User-driven [`Self::run_search`]
    /// bypasses the gate.
    ///
    /// Unlike [`Self::run_search`], this deliberately does **not** call
    /// [`Self::scroll_to_current_match`]: an automatic re-resolve must not
    /// yank the viewport, and it preserves the navigation cursor (clamped to
    /// the new match count) rather than snapping back to the first hit, so
    /// the N/M indicator does not jitter as the buffer scrolls (H5). Returns
    /// `true` when a re-search ran so the caller can request a repaint.
    pub fn auto_research_if_dirty(&mut self) -> bool {
        if !self.search.needs_research() {
            return false;
        }
        let now = Instant::now();
        if !auto_research_allowed(self.last_auto_research, now) {
            // Throttled: leave the dirty flag set so the next frame past the
            // gap re-resolves. No `execute`, no repaint request.
            return false;
        }
        self.last_auto_research = Some(now);
        if let Some(tab) = self.tabs.get(self.active) {
            let core = tab.core.lock();
            self.search.execute_preserving_current(&core);
        }
        self.needs_full_redraw = true;
        true
    }

    /// Advance to the next match (wrap-around) and scroll it into view.
    pub fn search_next(&mut self) {
        self.search.next_match();
        self.scroll_to_current_match();
        self.needs_full_redraw = true;
    }

    /// Step to the previous match (wrap-around) and scroll it into view.
    pub fn search_prev(&mut self) {
        self.search.prev_match();
        self.scroll_to_current_match();
        self.needs_full_redraw = true;
    }

    /// Scroll the viewport so the current match is roughly centered, if it
    /// is not already visible. No-op on the alt screen (no scrollback) or
    /// when there is no current match.
    ///
    /// Also auto-expands any collapsed fold region that contains the match,
    /// mirroring the WebView's `search.ts:154`
    /// `foldManager.expandRegionContaining(match.lineIndex)`. The expand
    /// happens before the scroll calculation so the geometry is already
    /// updated when the offset is computed.
    fn scroll_to_current_match(&mut self) {
        if self.alt_screen {
            return;
        }
        // Collect the match row first, then drop the borrow on `self.search`
        // so `self.tabs` can be mutably borrowed for the fold expand below.
        let abs_row = {
            let Some(m) = self.search.current_match() else {
                return;
            };
            // The match's first segment's absolute row anchors the scroll.
            let Some(seg) = m.segments.first() else {
                return;
            };
            seg.abs_row
        };
        // Auto-expand a collapsed fold region containing the match so the
        // hit is visible — mirroring the WebView's search.ts:154 call to
        // `foldManager.expandRegionContaining(match.lineIndex)`.
        // No-op when the match is in no region or an already-expanded one.
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.folds.expand_region_containing(abs_row);
        }
        let (scrollback_len, rows) = match self.tabs.get(self.active) {
            Some(tab) => {
                let core = tab.core.lock();
                (core.get_scrollback_length(), core.rows())
            }
            None => return,
        };
        if let Some(offset) = crate::search::scroll_offset_for_match(
            abs_row,
            scrollback_len,
            rows,
            self.scroll_offset(),
        ) {
            self.scroll_set_offset(offset);
        }
    }

    /// React to new PTY output. When already at the live tail, no-op (the
    /// renderer will pick up the new rows automatically). When sitting at an
    /// offset, preserve the offset so the user is not yanked to the bottom.
    /// Visually, the offset stays anchored because `term_core`'s ring buffer
    /// shifts the old content into scrollback under us.
    ///
    /// `active_changed` is `true` only when the **active** tab produced the
    /// new bytes. The search overlay is resolved against the active tab's
    /// buffer, so background-tab output must not invalidate its cached
    /// logical-line document (H3) — a switch to that tab closes the overlay
    /// anyway, so a background change never needs to be reflected.
    pub fn on_pty_output(&mut self, active_changed: bool) {
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
        if matches!(self.scroll_position, ScrollPosition::OffsetFromLive(_)) {
            // The offset is preserved; nothing to mutate, but we mark the
            // viewport as needing repaint because the visible row content
            // shifted by one (live tail advanced underneath the offset).
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
    /// 2. previous + current cursor row (to clear cursor ghost on move,
    ///    or to flip the cursor in/out of view on blink phase change)
    /// 3. previous + current selection rows (to clear highlight on shrink)
    ///
    /// Returns a sorted, deduplicated `Vec`. Returns `0..rows` when
    /// `needs_full_redraw` or `force_full_redraw` is set.
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

        // Cursor history: previous + current. Also include the cursor row
        // when the blink phase flips so the cursor overlay can repaint or
        // erase without leaving a stale glyph.
        let cursor_row = core.get_cursor_row();
        push_unique(&mut set, cursor_row);
        if let Some((prev_row, _)) = self.previous_cursor {
            push_unique(&mut set, prev_row);
        }
        let blink_enabled = core.get_cursor_blink();
        if blink_enabled && self.blink_visible_now(blink_enabled) != self.previous_blink_visible {
            push_unique(&mut set, cursor_row);
        }

        // Selection history: union of previous + current. The selection now
        // holds *absolute* buffer rows, so each range must be translated back
        // into the screen rows it occupies before being added to the dirty
        // set.
        //
        // With a fold layout the absolute → screen mapping is non-linear
        // (collapsed bodies are hidden, summary rows draw no cells), so a
        // simple `abs - visible_start` does not hold. Rather than reproduce
        // the fold walk here just for the dirty set, repaint every row when a
        // selection is (or was) present and a fold layout is active — folds
        // are rare and the selection touches at most the viewport, so the
        // cost is bounded.
        if self.fold_layout.is_some()
            && (self.selection.is_some() || self.previous_selection.is_some())
        {
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
        self.needs_full_redraw = false;
    }

    /// Phase 4-E: route an `egui::Event::Ime(ImeEvent::Preedit(_))`
    /// payload to the active tab's preedit state. The anchor is the
    /// current cursor cell of the active tab's `TerminalCore`. No-op
    /// when there is no active tab.
    ///
    /// tao 0.34 only surfaces `WindowEvent::ReceivedImeText` (commit);
    /// this method is the routing point for future preedit plumbing
    /// (richer IME via egui's `ImeEvent::Preedit` once available) and
    /// is exercised directly by the unit tests.
    ///
    /// Phase 4-G-E: when `EMTERM_IME_PERF=1` is set, the entry time
    /// is captured via `Instant::now()` and the delta to
    /// `needs_full_redraw = true` is logged at warn level so TS-perf-3
    /// can be measured on a release host (release builds drop debug
    /// + log levels below warn).
    #[allow(dead_code)]
    pub fn on_ime_preedit(&mut self, text: &str) {
        let perf = ime_perf_enabled();
        let t0 = perf.then(Instant::now);
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let anchor = {
            let core = tab.core.lock();
            crate::ime::preedit::Anchor {
                row: core.get_cursor_row(),
                col: core.get_cursor_col(),
            }
        };
        tab.preedit_state.set(text, anchor);
        // The renderer skips frames when no row is dirty; force the
        // cursor row into the dirty set so the underline overlay
        // repaints immediately.
        self.needs_full_redraw = true;
        if let Some(start) = t0 {
            log::warn!(
                "ime perf [TS-perf-3] on_ime_preedit → needs_full_redraw: {} µs",
                start.elapsed().as_micros()
            );
        }
    }

    /// Phase 4-E: route an `egui::Event::Ime(ImeEvent::Commit(_))`
    /// payload to the active tab. Sanitizes the bytes via
    /// `ime::commit::write_commit` (same sanitizer the preedit state
    /// uses) and writes them to the active PTY exactly once. Then
    /// clears the preedit state so the overlay disappears. No-op when
    /// there is no active tab.
    ///
    /// Phase 4-G-E: when `EMTERM_IME_PERF=1` is set, the entry time
    /// is captured via `Instant::now()` and the delta to the
    /// `PtySession::write` return is logged at warn level so
    /// TS-perf-4 can be measured on a release host.
    pub fn on_ime_commit(&mut self, text: &str) {
        let perf = ime_perf_enabled();
        let t0 = perf.then(Instant::now);
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        // mux mode: route the commit as a PtyInput frame (the bridge drops raw
        // stdin). Otherwise write directly to the PTY.
        if tab
            .mux_group
            .as_ref()
            .and_then(|g| g.active_pane_id())
            .is_some()
        {
            let bytes = crate::ime::commit::commit_bytes(text);
            if !bytes.is_empty() {
                tab.write_input(bytes);
            }
        } else if let Some(pty) = tab.pty.as_ref() {
            if let Err(e) = crate::ime::commit::write_commit(pty, text) {
                log::warn!("ime commit write failed: {e}");
            }
        }
        tab.preedit_state.clear();
        self.needs_full_redraw = true;
        if let Some(start) = t0 {
            log::warn!(
                "ime perf [TS-perf-4] on_ime_commit → PtySession::write: {} µs",
                start.elapsed().as_micros()
            );
        }
    }

    /// Phase 4-E: clear the active tab's preedit state. Called on
    /// focus loss and on active tab close.
    pub fn on_ime_focus_lost(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.preedit_state.clear();
            self.needs_full_redraw = true;
        }
    }

    /// Phase 4-C (APC redesign): route one decoded `MuxMessage` to the
    /// tab at `tab_idx`. The actual routing logic lives on `Tab` so the
    /// tab can mutate its own grid / status state directly — this
    /// wrapper exists primarily as the test seam and as a stable name
    /// for future cross-tab mux behavior.
    ///
    /// Returns `true` when the tab's visible state changed (the caller
    /// should request a redraw).
    #[allow(dead_code)] // retained for future mux pumping / tests
    pub fn on_mux_message(&mut self, tab_idx: usize, msg: mux_ipc::protocol::MuxMessage) -> bool {
        let Some(tab) = self.tabs.get_mut(tab_idx) else {
            log::warn!("on_mux_message: tab_idx {tab_idx} out of range");
            return false;
        };
        tab.apply_mux_message(msg)
    }

    /// ユーザーにデスクトップ通知を送る。
    ///
    /// `notification_sink` フィールドの直接アクセスを避け、通知送信を
    /// アプリケーションドメインにカプセル化するためのメソッド。
    /// ウィンドウ層など外部からの通知送信はこのメソッドを経由すること。
    pub fn notify(&self, title: &str, body: &str) {
        self.notification_sink.send(title, body);
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// Phase 4-G-E performance instrumentation gate. Returns `true` when
/// the env `EMTERM_IME_PERF=1` is set. Cached on first call so the
/// hot path (called once per preedit / commit event) is a single
/// atomic load.
fn ime_perf_enabled() -> bool {
    use std::sync::atomic::{AtomicU8, Ordering};
    static CACHED: AtomicU8 = AtomicU8::new(0); // 0 = unset, 1 = false, 2 = true
    match CACHED.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => {
            let enabled = std::env::var("EMTERM_IME_PERF")
                .ok()
                .map(|v| v == "1")
                .unwrap_or(false);
            CACHED.store(if enabled { 2 } else { 1 }, Ordering::Relaxed);
            enabled
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::{Pos, SelectionMode};
    use std::time::Duration;
    use term_core::terminal_core::TerminalCore;

    fn fresh_core(cols: u16, rows: u16) -> TerminalCore {
        TerminalCore::new(cols, rows, 100)
    }

    fn app_with_cleared_state(core: &mut TerminalCore) -> App {
        let mut app = App::new();
        // Initial frame uses a full redraw; clear it so subsequent calls
        // exercise the union logic rather than the bypass.
        app.record_render_state(core);
        app
    }

    // TS-5: arm(now) sets the dismissal instant to now + linger window.
    #[test]
    fn restart_toast_arm_sets_dismiss_at() {
        let mut toast = RestartToast::default();
        assert!(!toast.active());
        toast.arm(10.0);
        assert!(toast.active());
        assert_eq!(toast.dismiss_at, Some(10.0 + RESTART_TOAST_LINGER_SECS));
    }

    // TS-6: prune keeps the toast while now < instant, clears it at/after.
    #[test]
    fn restart_toast_prune_keeps_then_clears() {
        let mut toast = RestartToast::default();
        toast.arm(0.0);
        // Before the linger window elapses → still active.
        toast.prune(RESTART_TOAST_LINGER_SECS - 0.1);
        assert!(toast.active());
        // At/after the dismissal instant → cleared.
        toast.prune(RESTART_TOAST_LINGER_SECS);
        assert!(!toast.active());
        assert_eq!(toast.dismiss_at, None);
    }

    // TS-7: re-arm after a prior arm keeps a single toast and refreshes the
    // dismissal instant (no second toast, instant moves forward).
    #[test]
    fn restart_toast_rearm_refreshes_single_toast() {
        let mut toast = RestartToast::default();
        toast.arm(0.0);
        assert_eq!(toast.dismiss_at, Some(RESTART_TOAST_LINGER_SECS));
        // A later failed spawn re-arms: same single toast, refreshed instant.
        toast.arm(5.0);
        assert_eq!(toast.dismiss_at, Some(5.0 + RESTART_TOAST_LINGER_SECS));
        // The earlier instant would have dismissed by now, but the refresh
        // keeps the toast active.
        toast.prune(RESTART_TOAST_LINGER_SECS + 0.1);
        assert!(toast.active());
    }

    #[test]
    fn visual_bell_idle_by_default() {
        let mut app = App::new();
        assert_eq!(app.visual_bell_progress(), None);
        assert!(!app.needs_bell_repaint());
    }

    #[test]
    fn open_search_sets_visible_and_focus_request() {
        let mut app = App::new();
        assert!(!app.search_visible());
        app.open_search();
        assert!(app.search_visible());
        assert!(app.search_focus_request);
    }

    #[test]
    fn close_search_clears_state() {
        let mut app = App::new();
        app.open_search();
        app.search.query = "x".to_string();
        app.close_search();
        assert!(!app.search_visible());
        assert!(app.search.query.is_empty());
        assert!(!app.search_focus_request);
    }

    // ── profile selector / new-tab chooser ───────────────────────────

    fn profile(name: &str, is_default: bool) -> app_settings::Profile {
        app_settings::Profile {
            name: name.to_string(),
            shell_path: String::new(),
            shell_args: Vec::new(),
            env_vars: String::new(),
            working_directory: String::new(),
            is_default,
            ssh_connection_name: String::new(),
            wsl_distro_name: String::new(),
        }
    }

    fn app_with_profiles(profiles: Vec<app_settings::Profile>) -> App {
        let settings = crate::settings::Settings {
            profiles,
            ..Default::default()
        };
        App::with_settings(settings)
    }

    #[test]
    fn open_profile_selector_noop_without_profiles() {
        let mut app = App::new();
        app.open_profile_selector();
        assert!(!app.profile_selector.visible);
    }

    #[test]
    fn open_profile_selector_lists_profiles_only() {
        let mut app = app_with_profiles(vec![profile("a", false), profile("b", true)]);
        app.open_profile_selector();
        assert!(app.profile_selector.visible);
        assert!(!app.profile_selector.include_global);
        assert_eq!(app.profile_selector_row_count(), 2);
        assert_eq!(app.profile_selector.selected, 0);
    }

    #[test]
    fn new_tab_chooser_prepends_global_and_preselects_default() {
        let mut app = app_with_profiles(vec![profile("a", false), profile("b", true)]);
        app.open_new_tab_chooser();
        assert!(app.profile_selector.visible);
        assert!(app.profile_selector.include_global);
        // Global row + 2 profiles.
        assert_eq!(app.profile_selector_row_count(), 3);
        // Default profile "b" (profiles[1]) → row 2.
        assert_eq!(app.profile_selector.selected, 2);
    }

    #[test]
    fn new_tab_chooser_without_default_preselects_global() {
        let mut app = app_with_profiles(vec![profile("a", false)]);
        app.open_new_tab_chooser();
        assert!(app.profile_selector.include_global);
        assert_eq!(app.profile_selector.selected, 0);
    }

    #[test]
    fn confirm_out_of_range_closes_without_spawn() {
        let mut app = app_with_profiles(vec![profile("a", false)]);
        app.open_profile_selector();
        app.confirm_profile_selection(5);
        assert!(!app.profile_selector.visible);
        assert!(app.tabs.is_empty());
    }

    #[test]
    fn apply_settings_closes_open_profile_selector() {
        let mut app = app_with_profiles(vec![profile("a", false), profile("b", false)]);
        app.open_new_tab_chooser();
        assert!(app.profile_selector.visible);
        // A settings save reloads profiles (here: a shorter list) while
        // the modal is open. The selector must close rather than confirm
        // against the stale list.
        let reloaded = crate::settings::Settings {
            profiles: vec![profile("a", false)],
            ..Default::default()
        };
        app.apply_settings(reloaded);
        assert!(!app.profile_selector.visible);
    }

    #[test]
    fn auto_research_reresolves_matches_without_scrolling() {
        // Spawn a tab so there is an active core to search against.
        let mut app = App::new();
        app.spawn_initial_tab();
        {
            let mut core = app.tabs[0].core.lock();
            core.process_pty_data(b"needle\r\n");
        }
        app.open_search();
        app.search.query = "needle".to_string();
        app.run_search();
        assert_eq!(app.search.matches.len(), 1);

        // User scrolls back; the auto re-search must preserve this offset.
        app.scroll_set_offset(5);
        assert_eq!(app.scroll_offset(), 5);

        // New PTY output brings a second "needle"; on_pty_output flags the
        // cache dirty (mirrors the pump_all path).
        {
            let mut core = app.tabs[0].core.lock();
            core.process_pty_data(b"another needle line\r\n");
        }
        app.on_pty_output(true);
        assert!(app.search.needs_research());

        // The frame-loop hook re-resolves against the current buffer without
        // scrolling.
        let researched = app.auto_research_if_dirty();
        assert!(
            researched,
            "dirty + visible + non-empty query → re-search ran"
        );
        assert_eq!(
            app.search.matches.len(),
            2,
            "re-search reflects the new occurrence in the current buffer"
        );
        assert_eq!(
            app.scroll_offset(),
            5,
            "auto re-search must NOT move the viewport"
        );
    }

    #[test]
    fn auto_research_noop_when_overlay_hidden_or_query_empty() {
        let mut app = App::new();
        app.spawn_initial_tab();
        {
            let mut core = app.tabs[0].core.lock();
            core.process_pty_data(b"needle");
        }
        // Hidden overlay: even after a buffer change, no re-search.
        app.search.query = "needle".to_string();
        app.on_pty_output(true);
        assert!(
            !app.auto_research_if_dirty(),
            "hidden overlay does not research"
        );

        // Visible but empty query: nothing to re-resolve.
        app.open_search();
        app.search.query.clear();
        app.on_pty_output(true);
        assert!(
            !app.auto_research_if_dirty(),
            "empty query does not research"
        );
    }

    #[test]
    fn switch_to_tab_closes_open_search() {
        // Two synthetic tabs so `switch_to_tab` actually changes `active`.
        // We avoid spawning PTYs by leaving the search overlay open and
        // asserting it closes on the active-tab change path. Construct a
        // bare app and drive `switch_to_tab` against an out-of-range and
        // an in-range index to confirm only a real switch closes search.
        let mut app = App::new();
        app.open_search();
        // No tabs → out-of-range switch is a no-op; search stays open.
        app.switch_to_tab(1);
        assert!(app.search_visible(), "no-op switch must not close search");
    }

    // ── TS-1 / TS-2 / TS-3 / TS-7: per-tab scroll save/restore (FR3) ─────

    /// Two synthetic tabs to exercise the native tab-switch scroll handoff.
    fn app_with_two_tabs() -> App {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab();
        // `spawn_new_tab` makes the new tab active AND raises the one-shot
        // scroll-into-view flag; reset both so the test starts from a known
        // clean state (active tab 0, flag down). Use the direct field rather
        // than `switch_to_tab` so we do not exercise the path under test.
        app.active = 0;
        app.scroll_active_tab_into_view = false;
        app.scroll_position = ScrollPosition::Live;
        app
    }

    #[test]
    fn switch_to_tab_saves_outgoing_and_restores_incoming_scroll() {
        // TS-1: switching tabs saves the outgoing tab's scroll position and
        // restores the incoming tab's.
        let mut app = app_with_two_tabs();
        // Scroll up in tab 0, then switch to tab 1 (saved at Live).
        app.scroll_position = ScrollPosition::OffsetFromLive(12);
        app.switch_to_tab(1);
        assert_eq!(
            app.scroll_offset(),
            0,
            "incoming tab 1 was at Live → restores to bottom"
        );
        assert_eq!(
            app.tabs[0].scroll_position,
            ScrollPosition::OffsetFromLive(12),
            "outgoing tab 0's offset was parked into its slot"
        );
        // Returning to tab 0 restores its parked offset.
        app.switch_to_tab(0);
        assert_eq!(
            app.scroll_offset(),
            12,
            "returning to tab 0 restores its saved offset"
        );
    }

    #[test]
    fn switch_to_tab_live_restores_to_bottom() {
        // TS-2: a unit saved at Live restores at the bottom (offset 0).
        let mut app = app_with_two_tabs();
        // Tab 1 stays at Live; tab 0 scrolls up before we leave it.
        app.scroll_position = ScrollPosition::OffsetFromLive(5);
        app.switch_to_tab(1);
        assert_eq!(app.scroll_position, ScrollPosition::Live);
        assert_eq!(app.scroll_offset(), 0);
    }

    #[test]
    fn switch_to_tab_offset_restores_to_same_offset() {
        // TS-3: a unit saved at OffsetFromLive(n) restores at offset n.
        let mut app = app_with_two_tabs();
        // Pre-seed tab 1 with a saved offset, then switch into it.
        app.tabs[1].scroll_position = ScrollPosition::OffsetFromLive(8);
        app.switch_to_tab(1);
        assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(8));
        assert_eq!(app.scroll_offset(), 8);
    }

    #[test]
    fn switch_to_tab_all_live_introduces_no_scroll() {
        // TS-7: all tabs at Live → switching introduces no scroll.
        let mut app = app_with_two_tabs();
        assert_eq!(app.tabs[0].scroll_position, ScrollPosition::Live);
        assert_eq!(app.tabs[1].scroll_position, ScrollPosition::Live);
        app.switch_to_tab(1);
        assert_eq!(app.scroll_position, ScrollPosition::Live);
        app.switch_to_tab(0);
        assert_eq!(app.scroll_position, ScrollPosition::Live);
    }

    #[test]
    fn tab_scroll_position_default_is_live() {
        let app = app_with_two_tabs();
        assert_eq!(app.tabs[0].scroll_position, ScrollPosition::Live);
        assert_eq!(app.tabs[1].scroll_position, ScrollPosition::Live);
    }

    // ── FR4: plain-tab keyboard switch raises scroll-into-view flag ─────────

    #[test]
    fn ts7_keyboard_tab_switch_sets_scroll_into_view_flag() {
        // TS-7: NextTab / PrevTab / JumpTab each commit an active-index change
        // (all route through `switch_to_tab`), which raises the one-shot
        // scroll-into-view flag.
        let mut app = app_with_two_tabs();
        assert!(!app.scroll_active_tab_into_view());
        app.apply_action(crate::ui::AppAction::NextTab);
        assert!(
            app.scroll_active_tab_into_view(),
            "NextTab moved active → flag set"
        );

        // Clear and try PrevTab.
        app.clear_scroll_active_tab_into_view();
        app.apply_action(crate::ui::AppAction::PrevTab);
        assert!(
            app.scroll_active_tab_into_view(),
            "PrevTab moved active → flag set"
        );

        // Clear and try JumpTab to the other tab.
        app.clear_scroll_active_tab_into_view();
        // active is back at 0 (NextTab→1, PrevTab→0); jump to tab 2 (Ctrl+2)
        // which clamps to the last existing tab (idx 1), a real move.
        app.apply_action(crate::ui::AppAction::JumpTab(2));
        assert!(
            app.scroll_active_tab_into_view(),
            "JumpTab moved active → flag set"
        );
    }

    #[test]
    fn ts8_switch_to_already_active_tab_does_not_set_flag() {
        // TS-8: switching to the already-active tab (or out of range) is a
        // no-op `switch_to_tab` early-return, so the flag stays down.
        let mut app = app_with_two_tabs();
        assert_eq!(app.active, 0);
        app.clear_scroll_active_tab_into_view();
        app.switch_to_tab(0); // same index → no-op
        assert!(
            !app.scroll_active_tab_into_view(),
            "no-op switch to the active tab must not set the flag"
        );
        app.switch_to_tab(99); // out of range → no-op
        assert!(
            !app.scroll_active_tab_into_view(),
            "out-of-range switch must not set the flag"
        );
    }

    #[test]
    fn ts7b_mouse_tab_switch_does_not_set_scroll_into_view_flag() {
        // FR4 (keyboard-only): a mouse click that switches tabs routes through
        // `apply_tab_event(TabEvent::Switch)` → `switch_to_tab`, which (post-fix)
        // does NOT raise the scroll-into-view flag. The clicked tab is already
        // visible, so there is nothing to scroll into view; raising the flag
        // on the mouse path is exactly the FR4 violation this guards against.
        let mut app = app_with_two_tabs();
        assert_eq!(app.active, 0);
        app.clear_scroll_active_tab_into_view();
        let _ = app.apply_tab_event(crate::ui::TabEvent::Switch(1));
        assert_eq!(app.active, 1, "mouse switch moved the active tab");
        assert!(
            !app.scroll_active_tab_into_view(),
            "mouse-originated tab switch must NOT set the scroll-into-view flag"
        );
    }

    #[test]
    fn new_tab_sets_scroll_into_view_flag() {
        // A freshly created tab lands at the end of the strip (off-screen when
        // tabs overflow), so it raises the one-shot scroll-into-view flag and
        // surfaces next frame. This holds for every new-tab path (they all
        // funnel through `spawn_new_tab_with_overrides`), and unlike an
        // existing-tab mouse switch, it fires even though `+` is a mouse action
        // — the new tab is one the user has not seen yet.
        let mut app = app_with_two_tabs();
        app.clear_scroll_active_tab_into_view();
        let before = app.tabs.len();
        app.spawn_new_tab();
        assert_eq!(app.tabs.len(), before + 1, "spawned a new tab");
        assert_eq!(app.active, app.tabs.len() - 1, "the new tab is active");
        assert!(
            app.scroll_active_tab_into_view(),
            "a newly created tab must raise the scroll-into-view flag"
        );
    }

    #[test]
    fn auto_research_throttle_allows_then_blocks_then_allows() {
        // Pure-function policy: no prior run always allows; within the
        // window blocks; past the window allows again.
        let t0 = Instant::now();
        assert!(
            auto_research_allowed(None, t0),
            "first auto re-search always runs"
        );
        let just_under = t0 + (AUTO_RESEARCH_THROTTLE - std::time::Duration::from_millis(1));
        assert!(
            !auto_research_allowed(Some(t0), just_under),
            "a run inside the throttle window is blocked"
        );
        let just_over = t0 + AUTO_RESEARCH_THROTTLE;
        assert!(
            auto_research_allowed(Some(t0), just_over),
            "a run at/after the window elapses is allowed"
        );
    }

    #[test]
    fn auto_research_throttled_keeps_dirty_and_does_not_run() {
        let mut app = App::new();
        app.spawn_initial_tab();
        {
            let mut core = app.tabs[0].core.lock();
            core.process_pty_data(b"needle\r\n");
        }
        app.open_search();
        app.search.query = "needle".to_string();
        app.run_search();
        assert_eq!(app.search.matches.len(), 1);

        // A fresh buffer change arrives and is flagged dirty.
        {
            let mut core = app.tabs[0].core.lock();
            core.process_pty_data(b"second needle\r\n");
        }
        app.on_pty_output(true);
        assert!(app.search.needs_research());

        // Pretend an auto re-search just ran, so the gate is closed.
        app.last_auto_research = Some(Instant::now());
        let ran = app.auto_research_if_dirty();
        assert!(
            !ran,
            "throttled: auto re-search must not run within the window"
        );
        assert!(
            app.search.needs_research(),
            "dirty flag is preserved so the next frame past the gap re-resolves"
        );
        assert_eq!(
            app.search.matches.len(),
            1,
            "matches unchanged while throttled (no execute)"
        );

        // Past the throttle window the same pending dirty re-resolves.
        app.last_auto_research =
            Some(Instant::now() - AUTO_RESEARCH_THROTTLE - std::time::Duration::from_millis(1));
        let ran = app.auto_research_if_dirty();
        assert!(ran, "past the window the pending change re-resolves");
        assert_eq!(app.search.matches.len(), 2);
    }

    #[test]
    fn auto_research_preserves_current_index() {
        let mut app = App::new();
        app.spawn_initial_tab();
        {
            let mut core = app.tabs[0].core.lock();
            core.process_pty_data(b"hit\r\nhit\r\nhit\r\n");
        }
        app.open_search();
        app.search.query = "hit".to_string();
        app.run_search();
        assert_eq!(app.search.matches.len(), 3);
        // Navigate to the second hit; the auto re-search must keep it.
        app.search_next();
        assert_eq!(app.search.current_index, 1);

        {
            let mut core = app.tabs[0].core.lock();
            core.process_pty_data(b"hit\r\n");
        }
        app.on_pty_output(true);
        // No prior auto re-search → throttle allows immediately.
        let ran = app.auto_research_if_dirty();
        assert!(ran);
        assert_eq!(app.search.matches.len(), 4, "new occurrence picked up");
        assert_eq!(
            app.search.current_index, 1,
            "auto re-search preserved the navigation cursor"
        );
    }

    #[test]
    fn background_tab_output_does_not_dirty_active_search() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab(); // active is now tab 1
        {
            let mut core = app.tabs[1].core.lock();
            core.process_pty_data(b"needle\r\n");
        }
        app.open_search();
        app.search.query = "needle".to_string();
        app.run_search();
        assert!(
            !app.search.needs_research(),
            "clean immediately after search"
        );

        // A *background* tab (tab 0) produced output. Per H3 this must NOT
        // invalidate the active tab's cached search document.
        app.on_pty_output(false);
        assert!(
            !app.search.needs_research(),
            "background-tab output leaves the active search clean"
        );

        // Active-tab output does invalidate it.
        app.on_pty_output(true);
        assert!(
            app.search.needs_research(),
            "active-tab output marks the search document dirty"
        );
    }

    #[test]
    fn reap_exited_tab_closes_open_search() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab(); // two tabs; active is tab 1
        app.open_search();
        app.search.query = "x".to_string();
        assert!(app.search_visible());

        // Mark a tab exited so the reap path in `pump_all` removes it,
        // shifting the active buffer. The open overlay must close (H4).
        app.tabs[1].exited = true;
        app.pump_all();
        assert_eq!(app.tabs.len(), 1, "exited tab was reaped");
        assert!(
            !app.search_visible(),
            "reap shifted the active buffer → search closed"
        );
    }

    // ── Search fold auto-expand (SPEC: Search Integration) ──────────────

    /// Build a tab with one occurrence of "needle" in scrollback and return
    /// the absolute row the match lands on. The core is small (80×4) so a
    /// handful of `\r\n` lines push the needle into scrollback quickly.
    fn app_with_needle_in_scrollback() -> (App, u32) {
        let mut app = App::new();
        app.spawn_initial_tab();
        {
            let mut core = app.tabs[0].core.lock();
            // Write the needle line then overflow the 4-row viewport so it
            // spills into scrollback.
            core.process_pty_data(b"needle\r\n");
            for _ in 0..8 {
                core.process_pty_data(b"\r\n");
            }
        }
        // Run a search to discover the actual abs_row of the match.
        app.open_search();
        app.search.query = "needle".to_string();
        if let Some(tab) = app.tabs.get(app.active) {
            let core = tab.core.lock();
            app.search.execute(&core);
        }
        let abs_row = app.search.matches[0].segments[0].abs_row;
        // Reset navigation cursor; tests call run_search / search_next themselves.
        app.search.current_index = -1;
        (app, abs_row)
    }

    #[test]
    fn search_next_auto_expands_collapsed_region_containing_match() {
        // A collapsed fold region wrapping the needle's absolute row must be
        // expanded when `search_next` navigates to that match — mirroring the
        // WebView's `foldManager.expandRegionContaining(match.lineIndex)`
        // call in search.ts:154.
        let (mut app, abs_row) = app_with_needle_in_scrollback();
        // Wrap the needle in a collapsed fold region.
        let region_start = abs_row;
        let region_end = abs_row + 5;
        app.tabs[0].folds.register_osc133_region(
            region_start,
            region_end,
            "cmd".to_string(),
            Some(0),
        );
        app.tabs[0].folds.toggle_fold(region_start);
        assert!(
            app.tabs[0]
                .folds
                .get_region_at_line(abs_row)
                .unwrap()
                .collapsed,
            "region must be collapsed before navigation"
        );

        // Navigate to the first match. `search_next` wraps from -1 to 0.
        app.search_next();

        assert!(
            !app.tabs[0]
                .folds
                .get_region_at_line(abs_row)
                .unwrap()
                .collapsed,
            "search_next must expand the collapsed region containing the match"
        );
    }

    #[test]
    fn run_search_auto_expands_collapsed_region_on_initial_confirm() {
        // On the initial search confirm (`run_search`) the current match (first
        // hit) is also scrolled into view, so the same auto-expand must fire.
        let (mut app, abs_row) = app_with_needle_in_scrollback();
        let region_start = abs_row;
        let region_end = abs_row + 5;
        app.tabs[0].folds.register_osc133_region(
            region_start,
            region_end,
            "cmd".to_string(),
            Some(0),
        );
        app.tabs[0].folds.toggle_fold(region_start);

        app.run_search();

        assert!(
            !app.tabs[0]
                .folds
                .get_region_at_line(abs_row)
                .unwrap()
                .collapsed,
            "run_search must expand the collapsed region containing the first match"
        );
    }

    #[test]
    fn search_does_not_expand_unrelated_collapsed_regions() {
        // A collapsed region that does NOT contain the current match must stay
        // collapsed — expand_region_containing is scoped to the match row.
        let (mut app, abs_row) = app_with_needle_in_scrollback();
        // Place the fold region well away from the needle.
        let unrelated_start = abs_row.saturating_sub(1).max(1) - 1; // one before
        // Guard: skip when there's no room for a region before abs_row.
        if unrelated_start == 0 {
            return;
        }
        let unrelated_end = unrelated_start + 1;
        if unrelated_end > abs_row {
            // No room — skip rather than overlap.
            return;
        }
        app.tabs[0].folds.register_osc133_region(
            unrelated_start,
            unrelated_end,
            "other".to_string(),
            Some(0),
        );
        app.tabs[0].folds.toggle_fold(unrelated_start);
        assert!(
            app.tabs[0]
                .folds
                .get_region_at_line(unrelated_start)
                .unwrap()
                .collapsed
        );

        app.search_next();

        assert!(
            app.tabs[0]
                .folds
                .get_region_at_line(unrelated_start)
                .unwrap()
                .collapsed,
            "unrelated collapsed region must stay collapsed"
        );
    }

    #[test]
    fn visual_bell_reports_progress_while_live() {
        let mut app = App::new();
        app.visual_bell_started = Some(Instant::now());
        let t = app
            .visual_bell_progress()
            .expect("flash just started — progress must be live");
        assert!((0.0..1.0).contains(&t), "progress {t} out of range");
        assert!(app.needs_bell_repaint(), "live flash must request frames");
        // The latch survives polls while the flash is still in-flight.
        assert!(app.visual_bell_started.is_some());
    }

    #[test]
    fn visual_bell_clears_after_flash_duration() {
        let mut app = App::new();
        // Back-date the flash past its 150 ms lifetime.
        app.visual_bell_started =
            Instant::now().checked_sub(Duration::from_millis(BELL_FLASH_MS + 50));
        assert!(
            app.visual_bell_started.is_some(),
            "test clock too close to process start to back-date"
        );
        assert_eq!(app.visual_bell_progress(), None);
        // One final repaint to erase the overlay…
        assert!(app.needs_bell_repaint());
        // …then the latch is gone.
        assert!(!app.needs_bell_repaint());
        assert!(app.visual_bell_started.is_none());
    }

    #[test]
    fn dirty_set_empty_after_clear_when_nothing_moved() {
        let mut core = fresh_core(20, 5);
        let app = app_with_cleared_state(&mut core);
        // Cursor at (0,0), no selection, nothing written.
        let set = app.dirty_rows_this_frame(&core);
        // Current cursor row (0) is always in the set.
        assert_eq!(set, vec![0]);
    }

    #[test]
    fn dirty_set_includes_cursor_move_origin_and_destination() {
        let mut core = fresh_core(20, 5);
        let mut app = app_with_cleared_state(&mut core);
        // Move cursor to row 3 via CSI Cursor Position (1-based).
        core.process_pty_data(b"\x1b[4;1H");
        core.clear_dirty(); // simulate that the write itself didn't touch cells
        // App still has previous_cursor = (0, 0) from initial record.
        let set = app.dirty_rows_this_frame(&core);
        assert!(
            set.contains(&0),
            "previous cursor row should be in dirty set"
        );
        assert!(
            set.contains(&3),
            "current cursor row should be in dirty set"
        );
        // Record then ask again — cursor history is now (3, x).
        app.record_render_state(&mut core);
        let set2 = app.dirty_rows_this_frame(&core);
        assert_eq!(set2, vec![3]);
    }

    #[test]
    fn dirty_set_includes_selection_extent_rows() {
        let mut core = fresh_core(20, 5);
        let mut app = app_with_cleared_state(&mut core);
        app.selection = Some(Selection {
            anchor: Pos { row: 1, col: 0 },
            extent: Pos { row: 3, col: 0 },
            mode: SelectionMode::Character,
        });
        let set = app.dirty_rows_this_frame(&core);
        // 0 = cursor, 1..=3 = selection.
        assert!(set.contains(&1));
        assert!(set.contains(&2));
        assert!(set.contains(&3));
    }

    #[test]
    fn dirty_set_includes_previous_selection_rows_after_shrink() {
        let mut core = fresh_core(20, 5);
        let mut app = app_with_cleared_state(&mut core);
        // Frame 1: select rows 1..=3.
        app.selection = Some(Selection {
            anchor: Pos { row: 1, col: 0 },
            extent: Pos { row: 3, col: 0 },
            mode: SelectionMode::Character,
        });
        let _ = app.dirty_rows_this_frame(&core);
        app.record_render_state(&mut core);
        // Frame 2: shrink to row 1 only.
        app.selection = Some(Selection {
            anchor: Pos { row: 1, col: 0 },
            extent: Pos { row: 1, col: 0 },
            mode: SelectionMode::Character,
        });
        let set = app.dirty_rows_this_frame(&core);
        assert!(set.contains(&1));
        // Rows 2 and 3 must redraw to clear the old highlight.
        assert!(set.contains(&2), "vacated selection row 2 must redraw");
        assert!(set.contains(&3), "vacated selection row 3 must redraw");
    }

    #[test]
    fn force_full_redraw_returns_all_rows() {
        let mut core = fresh_core(10, 4);
        let mut app = app_with_cleared_state(&mut core);
        app.force_full_redraw = true;
        let set = app.dirty_rows_this_frame(&core);
        assert_eq!(set, vec![0, 1, 2, 3]);
    }

    #[test]
    fn needs_full_redraw_returns_all_rows_until_recorded() {
        let core = fresh_core(10, 3);
        let app = App::new();
        // Default state: needs_full_redraw = true.
        let set = app.dirty_rows_this_frame(&core);
        assert_eq!(set, vec![0, 1, 2]);
    }

    #[test]
    fn dirty_set_after_single_line_write() {
        let mut core = fresh_core(20, 5);
        let app = app_with_cleared_state(&mut core);
        // Write text to row 0 (cursor was at (0,0)).
        core.process_pty_data(b"hello");
        let set = app.dirty_rows_this_frame(&core);
        // Row 0 was edited → in get_dirty_rows. Cursor stayed on row 0.
        assert_eq!(set, vec![0]);
        assert!(
            set.len() < core.rows() as usize,
            "should not be full redraw"
        );
    }

    // ── Scrollback state machine ─────────────────────

    #[test]
    fn scroll_position_default_is_live() {
        let app = App::new();
        assert_eq!(app.scroll_position, ScrollPosition::Live);
        assert_eq!(app.scroll_offset(), 0);
    }

    #[test]
    fn scroll_up_by_advances_offset() {
        let mut app = App::new();
        app.scroll_up_by(3);
        assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(3));
        app.scroll_up_by(2);
        assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(5));
    }

    #[test]
    fn scroll_up_by_clamps_to_scrollback_lines() {
        let mut app = App::new();
        // Default scrollback_lines = 10_000.
        app.scroll_up_by(99_999);
        assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(10_000));
    }

    #[test]
    fn scroll_down_to_zero_snaps_to_live() {
        let mut app = App::new();
        app.scroll_up_by(5);
        app.scroll_down_by(5);
        assert_eq!(app.scroll_position, ScrollPosition::Live);
    }

    #[test]
    fn scroll_down_below_zero_saturates_at_live() {
        let mut app = App::new();
        app.scroll_up_by(3);
        app.scroll_down_by(99);
        assert_eq!(app.scroll_position, ScrollPosition::Live);
    }

    #[test]
    fn scroll_to_top_uses_scrollback_ceiling() {
        let mut app = App::new();
        app.scroll_to_top();
        assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(10_000));
    }

    #[test]
    fn scroll_to_live_clears_offset() {
        let mut app = App::new();
        app.scroll_up_by(7);
        app.scroll_to_live();
        assert_eq!(app.scroll_position, ScrollPosition::Live);
    }

    #[test]
    fn alt_screen_suppresses_scroll_up() {
        let mut app = App::new();
        app.alt_screen = true;
        app.scroll_up_by(5);
        assert_eq!(app.scroll_position, ScrollPosition::Live);
    }

    #[test]
    fn alt_screen_suppresses_scroll_to_top() {
        let mut app = App::new();
        app.alt_screen = true;
        app.scroll_to_top();
        assert_eq!(app.scroll_position, ScrollPosition::Live);
    }

    #[test]
    fn set_alt_screen_true_forces_live() {
        let mut app = App::new();
        app.scroll_up_by(5);
        assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(5));
        app.set_alt_screen(true);
        assert_eq!(app.scroll_position, ScrollPosition::Live);
        assert!(app.alt_screen);
    }

    #[test]
    fn set_alt_screen_false_preserves_live() {
        let mut app = App::new();
        app.set_alt_screen(true);
        app.set_alt_screen(false);
        assert!(!app.alt_screen);
        assert_eq!(app.scroll_position, ScrollPosition::Live);
    }

    #[test]
    fn on_pty_output_in_live_is_noop() {
        let mut app = App::new();
        app.needs_full_redraw = false;
        app.on_pty_output(true);
        // No offset change.
        assert_eq!(app.scroll_position, ScrollPosition::Live);
        // No redraw forced (already at live, nothing visual shifted).
        assert!(!app.needs_full_redraw);
    }

    // ── Prompt-to-prompt navigation (OSC 133) ────────────────

    use crate::prompts::PromptMarkKind;
    use crate::prompts::ResolvedPromptMark;

    /// Build an `App` with one initial tab whose core has `scrollback`
    /// rows pushed into scrollback (so absolute rows 0..scrollback are
    /// scrollback and scrollback.. is viewport), and the given prompt-start
    /// marks installed. The grid is tiny (4 rows) so a handful of `\r\n`
    /// lines spill into scrollback quickly.
    fn app_with_prompts(scrollback: u32, prompt_rows: &[u32]) -> App {
        let mut app = App::new();
        app.spawn_initial_tab();
        {
            let tab = &mut app.tabs[0];
            // Push `scrollback + rows` newlines so `scrollback` rows land in
            // scrollback. Tab core is 80x24-ish by default; feeding plenty
            // of newlines guarantees the requested scrollback depth.
            let mut bytes = Vec::new();
            let total = scrollback + 64; // overshoot to fill the viewport too
            for _ in 0..total {
                bytes.extend_from_slice(b"\r\n");
            }
            tab.core.lock().process_pty_data(&bytes);
            for &row in prompt_rows {
                tab.prompts.push(ResolvedPromptMark {
                    kind: PromptMarkKind::PromptStart,
                    row,
                    exit_code: None,
                });
            }
        }
        app
    }

    #[test]
    fn jump_prev_scrolls_to_mark_above_view_top() {
        // 100 scrollback rows; a prompt at absolute row 40.
        let mut app = app_with_prompts(100, &[40]);
        let scrollback_len = app.tabs[0].core.lock().get_scrollback_length();
        assert!(scrollback_len >= 100, "expected ≥100 scrollback rows");
        // Start at live (view top = scrollback_len). Prev finds row 40.
        app.jump_to_prompt(JumpDirection::Prev);
        assert_eq!(
            app.scroll_offset(),
            scrollback_len - 40,
            "mark row 40 should sit at the view top"
        );
    }

    #[test]
    fn jump_next_scrolls_to_mark_below_view_top() {
        let mut app = app_with_prompts(100, &[40, 70]);
        let scrollback_len = app.tabs[0].core.lock().get_scrollback_length();
        // Scroll so the view top is at row 50 (between the two marks).
        app.scroll_set_offset(scrollback_len - 50);
        // Next from top=50 finds row 70.
        app.jump_to_prompt(JumpDirection::Next);
        assert_eq!(app.scroll_offset(), scrollback_len - 70);
    }

    #[test]
    fn jump_prev_with_no_mark_above_goes_to_top() {
        // Mark is below the current view top, so Prev finds nothing.
        let mut app = app_with_prompts(100, &[80]);
        let scrollback_len = app.tabs[0].core.lock().get_scrollback_length();
        // View top at row 10 (offset = scrollback_len - 10). No mark < 10.
        app.scroll_set_offset(scrollback_len - 10);
        app.jump_to_prompt(JumpDirection::Prev);
        // Falls to the top — clamped to the scrollback_lines ceiling, which
        // is well above scrollback_len here, so the offset equals
        // scrollback_len (the actual top).
        assert_eq!(app.scroll_offset(), scrollback_len);
    }

    #[test]
    fn jump_next_with_no_mark_below_goes_to_live() {
        let mut app = app_with_prompts(100, &[20]);
        let scrollback_len = app.tabs[0].core.lock().get_scrollback_length();
        // View top at row 50; the only mark (20) is above, so Next finds none.
        app.scroll_set_offset(scrollback_len - 50);
        app.jump_to_prompt(JumpDirection::Next);
        // Falls to the live tail.
        assert_eq!(app.scroll_position, ScrollPosition::Live);
    }

    #[test]
    fn jump_to_viewport_mark_resolves_to_live() {
        // A mark inside the viewport (row >= scrollback_len) → offset 0.
        let mut app = app_with_prompts(100, &[]);
        let scrollback_len = app.tabs[0].core.lock().get_scrollback_length();
        app.tabs[0].prompts.push(ResolvedPromptMark {
            kind: PromptMarkKind::PromptStart,
            row: scrollback_len + 2, // inside the live viewport
            exit_code: None,
        });
        // Scroll up first so we are not already at live.
        app.scroll_set_offset(scrollback_len - 30);
        app.jump_to_prompt(JumpDirection::Next);
        assert_eq!(app.scroll_position, ScrollPosition::Live);
    }

    #[test]
    fn jump_is_noop_on_alt_screen() {
        let mut app = app_with_prompts(100, &[40]);
        app.alt_screen = true;
        let before = app.scroll_position;
        app.jump_to_prompt(JumpDirection::Prev);
        assert_eq!(app.scroll_position, before);
    }

    #[test]
    fn jump_with_no_tabs_is_noop() {
        let mut app = App::new();
        // No tabs at all.
        app.jump_to_prompt(JumpDirection::Prev);
        assert_eq!(app.scroll_position, ScrollPosition::Live);
    }

    // ── Prompt-jump fold auto-expand (Phase 2 fold step 5) ───

    #[test]
    fn jump_prev_auto_expands_collapsed_region_containing_mark() {
        // A prompt mark at absolute row 40 lives inside a collapsed fold
        // region [35, 50). Jumping back to it must expand that region so
        // the prompt is visible (mirroring the WebView
        // `expandRegionContaining(marker.lineIndex)`).
        let mut app = app_with_prompts(100, &[40]);
        let scrollback_len = app.tabs[0].core.lock().get_scrollback_length();
        app.tabs[0]
            .folds
            .register_osc133_region(35, 50, "cmd".to_string(), Some(0));
        app.tabs[0].folds.toggle_fold(40);
        assert!(app.tabs[0].folds.get_region_at_line(40).unwrap().collapsed);

        app.jump_to_prompt(JumpDirection::Prev);

        // Region expanded …
        assert!(!app.tabs[0].folds.get_region_at_line(40).unwrap().collapsed);
        // … and the scroll offset still places the mark row at the view top.
        assert_eq!(app.scroll_offset(), scrollback_len - 40);
    }

    #[test]
    fn jump_next_auto_expands_collapsed_region_containing_mark() {
        let mut app = app_with_prompts(100, &[40, 70]);
        let scrollback_len = app.tabs[0].core.lock().get_scrollback_length();
        app.tabs[0]
            .folds
            .register_osc133_region(65, 80, "cmd".to_string(), Some(0));
        app.tabs[0].folds.toggle_fold(70);
        // View top at row 50 so Next finds the mark at row 70.
        app.scroll_set_offset(scrollback_len - 50);

        app.jump_to_prompt(JumpDirection::Next);

        assert!(!app.tabs[0].folds.get_region_at_line(70).unwrap().collapsed);
        assert_eq!(app.scroll_offset(), scrollback_len - 70);
    }

    #[test]
    fn jump_does_not_touch_unrelated_collapsed_regions() {
        // A collapsed region that does NOT contain the jump target stays
        // collapsed (expand_region_containing only acts on the mark's region).
        let mut app = app_with_prompts(100, &[40]);
        app.tabs[0]
            .folds
            .register_osc133_region(60, 70, "other".to_string(), Some(0));
        app.tabs[0].folds.toggle_fold(60);

        app.jump_to_prompt(JumpDirection::Prev);

        assert!(app.tabs[0].folds.get_region_at_line(60).unwrap().collapsed);
    }

    #[test]
    fn jump_with_no_fold_region_at_mark_is_fine() {
        // No fold regions at all: jump behaves exactly as before.
        let mut app = app_with_prompts(100, &[40]);
        let scrollback_len = app.tabs[0].core.lock().get_scrollback_length();
        app.jump_to_prompt(JumpDirection::Prev);
        assert_eq!(app.scroll_offset(), scrollback_len - 40);
    }

    // ── Fold click toggle (Phase 2 fold step 5) ──────────────

    /// Build an `App` with one tab carrying `scrollback` scrollback rows and
    /// a single OSC 133 fold region `[start, end)`. The region is collapsed
    /// when `collapsed` is set. Returns the app plus the live `rows` /
    /// `scrollback_len` so tests can compute display geometry exactly.
    fn app_with_fold(scrollback: u32, region: (u32, u32), collapsed: bool) -> (App, u16, u32) {
        let mut app = app_with_prompts(scrollback, &[]);
        let (start, end) = region;
        app.tabs[0]
            .folds
            .register_osc133_region(start, end, "cmd".to_string(), Some(0));
        if collapsed {
            app.tabs[0].folds.toggle_fold(start);
        }
        let (rows, scrollback_len) = {
            let core = app.tabs[0].core.lock();
            (core.rows(), core.get_scrollback_length())
        };
        (app, rows, scrollback_len)
    }

    #[test]
    fn fold_click_on_summary_row_expands_region() {
        // Collapsed region [5, 15) (9 rows hidden). Summary sits at display
        // line 5; scroll so display_start = 5 → the summary is at the top
        // screen row (display_row 0).
        let (mut app, rows, scrollback_len) = app_with_fold(100, (5, 15), true);
        let total_display = scrollback_len + rows as u32 - 9; // hides 9 rows
        let display_start = 5u32;
        let offset = total_display - rows as u32 - display_start;
        app.scroll_set_offset(offset);

        let acted = app.handle_fold_click(0);

        assert!(acted, "clicking the summary row should act");
        assert!(
            !app.tabs[0].folds.get_region_at_line(5).unwrap().collapsed,
            "summary click must expand the region"
        );
    }

    #[test]
    fn fold_click_inside_expanded_region_collapses_with_scroll_adjust() {
        // Expanded region [5, 35) (30 rows). Scroll so display_start = 10:
        // the region start (display line 5) is above the view top, but the
        // click row 0 (display line 10) lands inside the still-visible body.
        // Collapsing it must pull the offset down by line_count - 1 = 29 to
        // keep the click visually anchored.
        let (mut app, rows, scrollback_len) = app_with_fold(100, (5, 35), false);
        // No region collapsed yet → total_display == total_actual.
        let total_display = scrollback_len + rows as u32;
        let display_start = 10u32;
        let offset = total_display - rows as u32 - display_start; // == scrollback_len - 10
        app.scroll_set_offset(offset);
        let before_offset = app.scroll_offset();

        let acted = app.handle_fold_click(0);

        assert!(
            acted,
            "clicking inside an expanded region should collapse it"
        );
        assert!(
            app.tabs[0].folds.get_region_at_line(5).unwrap().collapsed,
            "interior click must collapse the region"
        );
        // Summary (display line 5) was above the view top (display_start 10)
        // → offset shifts down by line_count - 1 = 29.
        assert_eq!(app.scroll_offset(), before_offset - 29);
    }

    #[test]
    fn fold_click_inside_region_in_viewport_does_not_adjust_scroll() {
        // Expanded region [5, 15). With display_start = 0 the region start
        // (display line 5) is at/below the view top, so collapsing it must
        // NOT shift the scroll offset (mirrors the WebView's
        // `regionDisplayLine < displayStart` guard being false).
        let (mut app, rows, scrollback_len) = app_with_fold(100, (5, 15), false);
        let total_display = scrollback_len + rows as u32; // nothing collapsed yet
        let offset = total_display - rows as u32; // display_start = 0
        app.scroll_set_offset(offset);
        let before_offset = app.scroll_offset();

        // display_start = 0, so display_row 5 = display line 5 = region start.
        let acted = app.handle_fold_click(5);

        assert!(acted);
        assert!(app.tabs[0].folds.get_region_at_line(5).unwrap().collapsed);
        assert_eq!(
            app.scroll_offset(),
            before_offset,
            "region start at/below view top must not shift the offset"
        );
    }

    #[test]
    fn fold_click_outside_any_region_is_noop() {
        // Click a screen row that maps to a buffer line outside the region.
        let (mut app, rows, scrollback_len) = app_with_fold(100, (5, 15), false);
        let total_display = scrollback_len + rows as u32;
        let offset = total_display - rows as u32; // display_start = 0
        app.scroll_set_offset(offset);

        // display_row 20 = display line 20 = actual 20, outside [5, 15).
        let acted = app.handle_fold_click(20);

        assert!(!acted, "a click outside any region must be a no-op");
        assert!(!app.tabs[0].folds.get_region_at_line(5).unwrap().collapsed);
    }

    #[test]
    fn fold_click_below_grid_is_rejected() {
        // A display_row >= rows (a click below the last grid row) is rejected,
        // matching the WebView `displayRow >= rows` guard.
        let (mut app, rows, _sb) = app_with_fold(100, (5, 15), true);
        assert!(!app.handle_fold_click(rows));
        assert!(!app.handle_fold_click(rows + 5));
        // Region unchanged (still collapsed).
        assert!(app.tabs[0].folds.get_region_at_line(5).unwrap().collapsed);
    }

    #[test]
    fn fold_click_with_no_active_tab_is_noop() {
        let mut app = App::new();
        assert!(app.tabs.is_empty());
        assert!(!app.handle_fold_click(0));
    }

    #[test]
    fn fold_click_when_folding_disabled_is_noop() {
        let (mut app, _rows, _sb) = app_with_fold(100, (5, 15), false);
        app.tabs[0].folds.set_enabled(false);
        // display_start 0 so display_row 5 would otherwise hit the region.
        let acted = app.handle_fold_click(5);
        assert!(!acted, "disabled folding must reject the click");
        assert!(!app.tabs[0].folds.get_region_at_line(5).unwrap().collapsed);
    }

    #[test]
    fn on_pty_output_preserves_offset() {
        let mut app = App::new();
        app.scroll_up_by(4);
        app.needs_full_redraw = false;
        app.on_pty_output(true);
        // Offset preserved: user is not pulled to the bottom.
        assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(4));
        // Viewport content shifted underneath us, so a repaint is needed.
        assert!(app.needs_full_redraw);
    }

    // ── Phase 4-B: tab event + AppAction routing ─────────────

    /// TS-tab-2 (App half): closing the last tab empties the tabs
    /// vector. The run loop in `window_host` translates an empty
    /// `app.tabs` into `ControlFlow::Exit` (see `run` in
    /// `window_host.rs`), so this is the `ExitWindow` signal.
    #[test]
    fn closing_last_tab_signals_exit_window() {
        let mut app = App::new();
        // Manually push a Tab-like value would require a PTY; instead
        // we exercise the `close_tab` path on a synthetic tabs vector.
        // Tab::spawn_shell is fine in tests — it returns pty=None when
        // spawn fails, but the tab itself is constructed.
        app.spawn_initial_tab();
        assert_eq!(app.tabs.len(), 1, "exactly one tab after init");
        let exit = app.close_tab(0);
        assert!(exit, "closing the last tab must return true");
        assert!(app.tabs.is_empty(), "tabs vector must be empty after close");

        // The same routing via TabEvent must agree.
        let mut app2 = App::new();
        app2.spawn_initial_tab();
        let exit2 = app2.apply_tab_event(crate::ui::TabEvent::Close(0));
        assert!(exit2);
        assert!(app2.tabs.is_empty());
    }

    #[test]
    fn close_tab_in_middle_shifts_active_left_when_needed() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab();
        app.spawn_new_tab();
        assert_eq!(app.tabs.len(), 3);
        app.active = 2;
        // Close idx 0 → active was 2, now should be 1.
        let exit = app.close_tab(0);
        assert!(!exit);
        assert_eq!(app.active, 1);
        assert_eq!(app.tabs.len(), 2);
    }

    #[test]
    fn close_tab_clamps_active_when_closing_the_active_last_one() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab();
        app.active = 1;
        let exit = app.close_tab(1);
        assert!(!exit);
        // Active falls back to the new last tab.
        assert_eq!(app.active, 0);
        assert_eq!(app.tabs.len(), 1);
    }

    // ── TabEvent::Reorder — drag-and-drop reorder ──────────

    #[test]
    fn reorder_tab_moves_first_to_end_and_keeps_active_pointing_at_moved() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab();
        app.spawn_new_tab();
        assert_eq!(app.tabs.len(), 3);
        app.active = 0;
        // Drop the first tab past the last: insertion index 3 → after
        // removal of slot 0 it lands at slot 2.
        app.reorder_tab(0, 3);
        assert_eq!(app.tabs.len(), 3, "tab count must not change");
        assert_eq!(app.active, 2, "moved tab follows its new slot");
    }

    #[test]
    fn reorder_tab_shifts_active_when_moving_a_tab_past_it() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab();
        app.spawn_new_tab();
        app.active = 1;
        // Move tab 0 (not active) past the active one to the end.
        // After removal, insert_at = 3 - 1 = 2. Active was 1, from(0)
        // < active(1) and insert_at(2) >= active(1) → active shifts to 0.
        app.reorder_tab(0, 3);
        assert_eq!(app.active, 0);
    }

    #[test]
    fn reorder_tab_ignores_no_op_targets() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab();
        app.active = 1;
        // to == from
        app.reorder_tab(0, 0);
        assert_eq!(app.active, 1);
        // to == from + 1 (would land in the same slot)
        app.reorder_tab(0, 1);
        assert_eq!(app.active, 1);
    }

    #[test]
    fn reorder_tab_ignores_out_of_range_from() {
        let mut app = App::new();
        app.spawn_initial_tab();
        let active_before = app.active;
        let len_before = app.tabs.len();
        app.reorder_tab(42, 0);
        assert_eq!(app.active, active_before);
        assert_eq!(app.tabs.len(), len_before);
    }

    #[test]
    fn apply_tab_event_routes_reorder_to_reorder_tab() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab();
        app.spawn_new_tab();
        app.active = 0;
        let exit = app.apply_tab_event(crate::ui::TabEvent::Reorder { from: 0, to: 3 });
        assert!(!exit);
        assert_eq!(app.active, 2);
    }

    #[test]
    fn next_tab_wraps_at_end() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab();
        app.active = 1;
        let exit = app.apply_action(crate::ui::AppAction::NextTab);
        assert!(!exit);
        assert_eq!(app.active, 0);
    }

    #[test]
    fn prev_tab_wraps_at_start() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab();
        app.active = 0;
        let exit = app.apply_action(crate::ui::AppAction::PrevTab);
        assert!(!exit);
        assert_eq!(app.active, 1);
    }

    #[test]
    fn jump_tab_clamps_to_existing_range() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab();
        // Only two tabs; Ctrl+9 should clamp to the last (idx 1).
        let exit = app.apply_action(crate::ui::AppAction::JumpTab(9));
        assert!(!exit);
        assert_eq!(app.active, 1);
        // Ctrl+1 jumps to idx 0.
        app.apply_action(crate::ui::AppAction::JumpTab(1));
        assert_eq!(app.active, 0);
    }

    #[test]
    fn new_tab_action_appends_and_switches() {
        let mut app = App::new();
        app.spawn_initial_tab();
        let before = app.tabs.len();
        let exit = app.apply_action(crate::ui::AppAction::NewTab);
        assert!(!exit);
        assert_eq!(app.tabs.len(), before + 1);
        assert_eq!(app.active, app.tabs.len() - 1);
    }

    #[test]
    fn close_tab_action_can_signal_exit() {
        let mut app = App::new();
        app.spawn_initial_tab();
        let exit = app.apply_action(crate::ui::AppAction::CloseTab);
        assert!(exit);
    }

    #[test]
    fn tab_event_switch_changes_active_without_exit() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab();
        app.active = 0;
        let exit = app.apply_tab_event(crate::ui::TabEvent::Switch(1));
        assert!(!exit);
        assert_eq!(app.active, 1);
    }

    // ── Phase 4-E: IME preedit/commit routing ────────────────────────

    #[test]
    fn ime_preedit_no_active_tab_is_noop() {
        let mut app = App::new();
        // No spawn → no tabs. Must not panic.
        app.on_ime_preedit("abc");
    }

    #[test]
    fn ime_preedit_updates_active_tab_state() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.on_ime_preedit("hi");
        let tab = app.active_tab().unwrap();
        assert!(tab.preedit_state.active());
        assert_eq!(tab.preedit_state.text(), "hi");
    }

    #[test]
    fn ime_preedit_anchors_to_current_cursor() {
        let mut app = App::new();
        app.spawn_initial_tab();
        // Move cursor to (row=2, col=3) via CSI CUP.
        {
            let tab = app.active_tab().unwrap();
            tab.core.lock().process_pty_data(b"\x1b[3;4H");
        }
        app.on_ime_preedit("xy");
        let tab = app.active_tab().unwrap();
        let a = tab.preedit_state.anchor();
        assert_eq!(a.row, 2);
        assert_eq!(a.col, 3);
    }

    #[test]
    fn ime_preedit_sanitizes_control_bytes() {
        let mut app = App::new();
        app.spawn_initial_tab();
        // ESC must NOT survive into the preedit overlay text.
        app.on_ime_preedit("a\x1bb");
        assert_eq!(app.active_tab().unwrap().preedit_state.text(), "ab");
    }

    #[test]
    fn ime_commit_clears_preedit_state() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.on_ime_preedit("abc");
        assert!(app.active_tab().unwrap().preedit_state.active());
        app.on_ime_commit("abc");
        assert!(!app.active_tab().unwrap().preedit_state.active());
    }

    #[test]
    fn ime_commit_no_active_tab_is_noop() {
        let mut app = App::new();
        app.on_ime_commit("abc");
    }

    #[test]
    fn ime_focus_lost_clears_preedit() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.on_ime_preedit("xy");
        assert!(app.active_tab().unwrap().preedit_state.active());
        app.on_ime_focus_lost();
        assert!(!app.active_tab().unwrap().preedit_state.active());
    }

    #[test]
    fn ime_preedit_requests_full_redraw() {
        let mut app = App::new();
        app.spawn_initial_tab();
        // Clear the initial full-redraw flag so we can observe the
        // routing-time mutation.
        {
            let arc = app.active_tab().unwrap().core.clone();
            let mut core = arc.lock();
            app.record_render_state(&mut core);
        }
        assert!(!app.needs_full_redraw);
        app.on_ime_preedit("ab");
        assert!(app.needs_full_redraw);
    }

    #[test]
    fn ime_commit_requests_full_redraw() {
        let mut app = App::new();
        app.spawn_initial_tab();
        {
            let arc = app.active_tab().unwrap().core.clone();
            let mut core = arc.lock();
            app.record_render_state(&mut core);
        }
        assert!(!app.needs_full_redraw);
        app.on_ime_commit("a");
        assert!(app.needs_full_redraw);
    }

    // ── Phase 4-C (APC redesign): mux message routing ────────────────

    /// TS-mux-msg-1: `App::on_mux_message` applies a `Snapshot` to the
    /// target tab's `TerminalCore` via `reset_and_replay`. The grid
    /// content visible afterward must reflect the replayed bytes.
    #[test]
    fn on_mux_message_snapshot_resets_and_replays_into_core() {
        use mux_ipc::protocol::{MessageType, MuxMessage};

        let mut app = App::new();
        app.spawn_initial_tab();

        // Prime the grid with something the snapshot must overwrite.
        {
            let tab = app.active_tab().unwrap();
            tab.core.lock().process_pty_data(b"BEFORE");
        }

        // Snapshot payload: clear + print "AFTER" at home.
        let mut payload: Vec<u8> = Vec::new();
        payload.extend_from_slice(b"\x1b[2J\x1b[H");
        payload.extend_from_slice(b"AFTER");

        let msg = MuxMessage {
            msg_type: MessageType::Snapshot,
            pane_id: 0,
            payload,
        };
        let changed = app.on_mux_message(0, msg);
        assert!(changed, "Snapshot must mark state changed");

        // The first 5 cells of row 0 should now spell A F T E R.
        let tab = app.active_tab().unwrap();
        let core = tab.core.lock();
        let row0: String = (0..5).map(|c| core.get_cell_char(c, 0)).collect();
        assert_eq!(row0, "AFTER");
    }

    /// TS-mux-msg-2: `App::on_mux_message` updates `mux_status_state`
    /// on the target tab when handed a `StatusUpdate`.
    #[test]
    fn on_mux_message_status_update_caches_payload_on_tab() {
        use mux_ipc::protocol::{MessageType, MuxMessage, StatusUpdateMsg};

        let mut app = App::new();
        app.spawn_initial_tab();

        let payload = StatusUpdateMsg {
            left: "[default] *win1 win2".to_string(),
            right: "12:34".to_string(),
        };
        let msg = MuxMessage::control(MessageType::StatusUpdate, 0, &payload);
        let changed = app.on_mux_message(0, msg);
        assert!(changed);

        let tab = app.active_tab().unwrap();
        let cached = tab.mux_status_state.as_ref().expect("status cached");
        assert_eq!(cached.left, payload.left);
        assert_eq!(cached.right, payload.right);
    }

    /// `App::on_mux_message` with an out-of-range tab index is a no-op
    /// (logs a warning) and never panics.
    #[test]
    fn on_mux_message_out_of_range_returns_false() {
        use mux_ipc::protocol::{MessageType, MuxMessage};

        let mut app = App::new();
        // No tabs spawned.
        let msg = MuxMessage {
            msg_type: MessageType::Snapshot,
            pane_id: 0,
            payload: b"hello".to_vec(),
        };
        assert!(!app.on_mux_message(0, msg));
    }

    // ── Phase 3/4: mux action dispatch (TS-12, TS-13, TS-14) ──────────

    use crate::mux::prefix::PrefixAction;
    use mux_ipc::protocol::{MessageType, MuxMessage, SessionInfo, WelcomeMsg, WindowInfo};

    /// An app with one tab seeded with `n` mux windows (panes 100+i, ids
    /// 0..n, active 0) via a real Welcome message.
    fn app_with_mux_windows(n: usize) -> App {
        let mut app = App::new();
        app.spawn_initial_tab();
        let windows: Vec<WindowInfo> = (0..n)
            .map(|i| WindowInfo {
                id: i as u32,
                name: format!("w{i}"),
                active_pane_id: 100 + i as u32,
            })
            .collect();
        let welcome = MuxMessage::control(
            MessageType::Welcome,
            0,
            &WelcomeMsg::Accepted {
                server_version: 1,
                sessions: vec![SessionInfo {
                    id: 1,
                    name: "main".to_string(),
                    window_count: n as u32,
                    pane_count: n as u32,
                    active_window_index: 0,
                    windows,
                }],
            },
        );
        app.on_mux_message(0, welcome);
        app
    }

    fn active_idx(app: &App) -> usize {
        app.active_tab()
            .unwrap()
            .mux_group
            .as_ref()
            .unwrap()
            .active_index()
    }

    #[test]
    fn dispatch_next_prev_wrap() {
        let mut app = app_with_mux_windows(3);
        assert_eq!(
            app.dispatch_mux_action(PrefixAction::NextWindow),
            MuxActionOutcome::Changed
        );
        assert_eq!(active_idx(&app), 1);
        app.dispatch_mux_action(PrefixAction::NextWindow);
        app.dispatch_mux_action(PrefixAction::NextWindow);
        assert_eq!(active_idx(&app), 0); // wrapped
        app.dispatch_mux_action(PrefixAction::PrevWindow);
        assert_eq!(active_idx(&app), 2); // wrapped backwards
    }

    #[test]
    fn dispatch_digit_clamps_and_noops_past_range() {
        let mut app = app_with_mux_windows(3);
        app.dispatch_mux_action(PrefixAction::SelectWindow(2));
        assert_eq!(active_idx(&app), 2);
        // digit 5 is past range → no-op, stays on 2.
        assert_eq!(
            app.dispatch_mux_action(PrefixAction::SelectWindow(5)),
            MuxActionOutcome::None
        );
        assert_eq!(active_idx(&app), 2);
    }

    // ── FR4 / TS-9: mux window switch raises scroll-into-view flag ──────────

    #[test]
    fn ts9_mux_window_switch_sets_scroll_into_view_only_on_real_change() {
        // TS-9 (option b — strict): a committed window change (next/prev/digit
        // that actually moves `active`) raises the one-shot scroll-into-view
        // flag; switching to the already-active window via a same-window digit
        // jump reports `Changed` but does not move `active`, so the flag stays
        // down. We chose the same-index guard (compare active_index before vs
        // after) so TS-9's "already-active does not set it" holds for the
        // digit path too, not only next/prev.
        let mut app = app_with_mux_windows(3);
        assert!(!app.scroll_active_tab_into_view());

        // NextWindow moves 0 → 1: flag set.
        app.dispatch_mux_action(PrefixAction::NextWindow);
        assert_eq!(active_idx(&app), 1);
        assert!(
            app.scroll_active_tab_into_view(),
            "NextWindow moved active window → flag set"
        );

        // PrevWindow moves 1 → 0: flag set.
        app.clear_scroll_active_tab_into_view();
        app.dispatch_mux_action(PrefixAction::PrevWindow);
        assert_eq!(active_idx(&app), 0);
        assert!(
            app.scroll_active_tab_into_view(),
            "PrevWindow moved active window → flag set"
        );

        // SelectWindow(2) moves 0 → 2: flag set.
        app.clear_scroll_active_tab_into_view();
        app.dispatch_mux_action(PrefixAction::SelectWindow(2));
        assert_eq!(active_idx(&app), 2);
        assert!(
            app.scroll_active_tab_into_view(),
            "digit jump to a different window → flag set"
        );

        // SelectWindow(2) again — already on window 2. dispatch reports
        // `Changed` (no same-index short-circuit before the SwitchWindow send),
        // but `active` does not move, so the strict guard keeps the flag down.
        app.clear_scroll_active_tab_into_view();
        app.dispatch_mux_action(PrefixAction::SelectWindow(2));
        assert_eq!(active_idx(&app), 2);
        assert!(
            !app.scroll_active_tab_into_view(),
            "same-window digit jump must NOT set the flag (TS-9 strict)"
        );
    }

    #[test]
    fn ts9_mux_single_window_switch_does_not_set_flag() {
        // With <2 windows, next/prev return None (no switch); the flag stays
        // down.
        let mut app = app_with_mux_windows(1);
        app.clear_scroll_active_tab_into_view();
        app.dispatch_mux_action(PrefixAction::NextWindow);
        assert!(
            !app.scroll_active_tab_into_view(),
            "single-window next is a no-op → flag stays down"
        );
    }

    #[test]
    fn dispatch_single_window_switch_is_noop() {
        let mut app = app_with_mux_windows(1);
        assert_eq!(
            app.dispatch_mux_action(PrefixAction::NextWindow),
            MuxActionOutcome::None
        );
        assert_eq!(active_idx(&app), 0);
    }

    #[test]
    fn pump_all_scrolls_new_mux_window_into_view_on_active_tab() {
        // FR6 (mux), App-level integration: a daemon `PaneCreated` on the ACTIVE
        // mux tab raises scroll-into-view through `pump_all` — the path the
        // tabs.rs latch unit tests do not reach (the `idx == active` gating plus
        // the latch → `scroll_active_tab_into_view` conversion).
        let mut app = app_with_mux_windows(2); // tab 0 is mux and active
        app.clear_scroll_active_tab_into_view();
        // The daemon confirms a new window on the active tab; the push activates
        // it, and the PaneCreated handler latches the FR6 signal.
        app.on_mux_message(
            0,
            MuxMessage {
                msg_type: MessageType::PaneCreated,
                pane_id: 200,
                payload: Vec::new(),
            },
        );
        app.pump_all();
        assert!(
            app.scroll_active_tab_into_view(),
            "a PaneCreated on the active mux tab scrolls the new sub-tab into view"
        );
    }

    #[test]
    fn pump_all_skips_scroll_for_background_tab_mux_window_and_drains_latch() {
        // FR6 (mux), App-level integration: a `PaneCreated` on a NON-active tab
        // must NOT raise scroll-into-view (the `idx == active` gate), and its
        // latch is still drained (drain-every-tab) so it cannot fire on a later
        // pump. This locks both invariants the unit tests cannot reach.
        let mut app = app_with_mux_windows(2); // tab 0 is mux, active = 0
        app.spawn_new_tab(); // tab 1 becomes active
        app.clear_scroll_active_tab_into_view(); // spawn_new_tab raises it (FR6)
        assert_eq!(app.active, 1);
        // A new window is appended to the BACKGROUND mux tab (tab 0).
        app.on_mux_message(
            0,
            MuxMessage {
                msg_type: MessageType::PaneCreated,
                pane_id: 200,
                payload: Vec::new(),
            },
        );
        app.pump_all();
        assert!(
            !app.scroll_active_tab_into_view(),
            "a background-tab window add must not scroll the active tab"
        );
        // The background latch was drained, not stranded: a later pump with no
        // new event keeps the flag down.
        app.pump_all();
        assert!(
            !app.scroll_active_tab_into_view(),
            "the drained latch does not resurface on a subsequent pump"
        );
    }

    // ── TS-5 / TS-6 / TS-7 (pane): local pane-switch scroll save/restore (FR3) ──

    #[test]
    fn local_pane_switch_round_trip_restores_scroll_position() {
        // TS-5 (local switch path): scroll up in pane A, switch to B, return
        // to A — A's saved offset is restored; B is unaffected (Live).
        let mut app = app_with_mux_windows(2);
        assert_eq!(active_idx(&app), 0);

        // Scroll up in pane A (index 0), then switch to pane B (index 1).
        app.scroll_position = ScrollPosition::OffsetFromLive(15);
        assert_eq!(
            app.dispatch_mux_action(PrefixAction::SelectWindow(1)),
            MuxActionOutcome::Changed
        );
        assert_eq!(active_idx(&app), 1);
        assert_eq!(
            app.scroll_position,
            ScrollPosition::Live,
            "incoming pane B restores to its own (Live) position"
        );
        assert!(app.needs_full_redraw, "pane switch forces a full redraw");

        // Return to pane A (index 0): its saved offset comes back.
        assert_eq!(
            app.dispatch_mux_action(PrefixAction::SelectWindow(0)),
            MuxActionOutcome::Changed
        );
        assert_eq!(active_idx(&app), 0);
        assert_eq!(
            app.scroll_position,
            ScrollPosition::OffsetFromLive(15),
            "returning to pane A restores A's saved offset"
        );
    }

    #[test]
    fn local_pane_switch_all_live_introduces_no_scroll() {
        // TS-7 (pane): all panes at Live → switching introduces no scroll.
        let mut app = app_with_mux_windows(2);
        assert_eq!(
            app.dispatch_mux_action(PrefixAction::SelectWindow(1)),
            MuxActionOutcome::Changed
        );
        assert_eq!(app.scroll_position, ScrollPosition::Live);
        assert_eq!(
            app.dispatch_mux_action(PrefixAction::SelectWindow(0)),
            MuxActionOutcome::Changed
        );
        assert_eq!(app.scroll_position, ScrollPosition::Live);
    }

    #[test]
    fn local_pane_switch_with_empty_scrollback_does_not_crash() {
        // TS-6 (switch side): switching to a pane whose shared core has no
        // scrollback succeeds and leaves the active scroll value at the
        // incoming (Live) pane's saved position with no panic.
        let mut app = app_with_mux_windows(2);
        assert_eq!(
            app.dispatch_mux_action(PrefixAction::SelectWindow(1)),
            MuxActionOutcome::Changed
        );
        assert_eq!(app.scroll_position, ScrollPosition::Live);
        assert_eq!(app.scroll_offset(), 0);
    }

    #[test]
    fn local_pane_switch_forces_full_redraw() {
        // FR2: a committed pane switch sets the renderer's full-redraw flag so
        // a shorter incoming pane leaves no residual rows.
        let mut app = app_with_mux_windows(2);
        app.needs_full_redraw = false;
        app.dispatch_mux_action(PrefixAction::SelectWindow(1));
        assert!(app.needs_full_redraw);
    }

    #[test]
    fn local_pane_switch_noop_does_not_touch_scroll_or_redraw() {
        // NFR1: a no-op switch (single window) leaves scroll + redraw flag
        // untouched (scroll-pin / single-window mux unaffected).
        let mut app = app_with_mux_windows(1);
        app.scroll_position = ScrollPosition::OffsetFromLive(9);
        app.needs_full_redraw = false;
        assert_eq!(
            app.dispatch_mux_action(PrefixAction::NextWindow),
            MuxActionOutcome::None
        );
        assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(9));
        assert!(!app.needs_full_redraw);
    }

    #[test]
    fn dispatch_new_window_increments_pending() {
        let mut app = app_with_mux_windows(2);
        assert_eq!(
            app.dispatch_mux_action(PrefixAction::NewWindow),
            MuxActionOutcome::Changed
        );
        assert_eq!(
            app.active_tab()
                .unwrap()
                .mux_group
                .as_ref()
                .unwrap()
                .pending_create(),
            1
        );
    }

    #[test]
    fn dispatch_detach_emits_detach_outcome() {
        let mut app = app_with_mux_windows(2);
        assert_eq!(
            app.dispatch_mux_action(PrefixAction::Detach),
            MuxActionOutcome::Detach
        );
    }

    #[test]
    fn dispatch_rename_opens_dialog_with_stable_id() {
        let mut app = app_with_mux_windows(2);
        app.dispatch_mux_action(PrefixAction::NextWindow); // active idx 1, id 1
        match app.dispatch_mux_action(PrefixAction::RenameWindow) {
            MuxActionOutcome::OpenRename {
                window_id,
                current_name,
            } => {
                assert_eq!(window_id, 1);
                assert_eq!(current_name, "w1");
            }
            other => panic!("expected OpenRename, got {other:?}"),
        }
    }

    #[test]
    fn dispatch_move_requires_two_windows() {
        let mut app = app_with_mux_windows(1);
        assert_eq!(
            app.dispatch_mux_action(PrefixAction::MoveWindow),
            MuxActionOutcome::None
        );
        let mut app = app_with_mux_windows(2);
        match app.dispatch_mux_action(PrefixAction::MoveWindow) {
            MuxActionOutcome::OpenMove {
                window_id,
                current_position,
                window_count,
            } => {
                assert_eq!(window_id, 0);
                assert_eq!(current_position, 1);
                assert_eq!(window_count, 2);
            }
            other => panic!("expected OpenMove, got {other:?}"),
        }
    }

    #[test]
    fn dispatch_without_mux_group_is_noop() {
        let mut app = App::new();
        app.spawn_initial_tab();
        assert_eq!(
            app.dispatch_mux_action(PrefixAction::NextWindow),
            MuxActionOutcome::None
        );
    }

    // ── FR1: tab-bar mux sub-tab click routing (MuxSwitch) ────────────────

    #[test]
    fn apply_tab_event_mux_switch_moves_active_window() {
        let mut app = app_with_mux_windows(3);
        assert_eq!(active_idx(&app), 0);
        let exit = app.apply_tab_event(crate::ui::TabEvent::MuxSwitch { tab: 0, window: 2 });
        assert!(!exit);
        assert_eq!(active_idx(&app), 2);
    }

    #[test]
    fn apply_tab_event_mux_switch_on_missing_tab_is_noop() {
        let mut app = app_with_mux_windows(2);
        // Out-of-range tab index must not panic and must leave state intact.
        assert!(!app.apply_tab_event(crate::ui::TabEvent::MuxSwitch { tab: 9, window: 1 }));
        assert_eq!(active_idx(&app), 0);
    }

    // ── TS-14: rename confirm re-resolves by stable id ────────────────

    #[test]
    fn confirm_rename_relabels_by_stable_id() {
        let mut app = app_with_mux_windows(3);
        assert!(app.confirm_mux_rename(2, "editor".to_string()));
        let g = app.active_tab().unwrap().mux_group.as_ref().unwrap();
        assert_eq!(g.windows()[2].name, "editor");
    }

    #[test]
    fn confirm_rename_empty_name_is_noop() {
        let mut app = app_with_mux_windows(2);
        assert!(!app.confirm_mux_rename(0, String::new()));
    }

    #[test]
    fn confirm_rename_closed_window_aborts() {
        let mut app = app_with_mux_windows(2);
        // window id 999 never existed → abort.
        assert!(!app.confirm_mux_rename(999, "x".to_string()));
    }

    // ── TS-13: move validation + optimistic reorder ───────────────────

    #[test]
    fn confirm_move_reorders_optimistically() {
        let mut app = app_with_mux_windows(3); // ids 0,1,2 panes 100,101,102
        // move window id 0 to position 3 → order 1,2,0
        assert!(app.confirm_mux_move(0, 3));
        let g = app.active_tab().unwrap().mux_group.as_ref().unwrap();
        assert_eq!(
            g.windows().iter().map(|w| w.id).collect::<Vec<_>>(),
            vec![1, 2, 0]
        );
    }

    #[test]
    fn confirm_move_out_of_range_is_noop() {
        let mut app = app_with_mux_windows(3);
        assert!(!app.confirm_mux_move(0, 0)); // below range
        assert!(!app.confirm_mux_move(0, 4)); // above range
    }

    #[test]
    fn confirm_move_same_position_is_noop() {
        let mut app = app_with_mux_windows(3); // active id 0 at position 1
        assert!(!app.confirm_mux_move(0, 1));
    }

    #[test]
    fn confirm_move_closed_window_aborts() {
        let mut app = app_with_mux_windows(3);
        assert!(!app.confirm_mux_move(999, 2));
    }

    #[test]
    fn confirm_move_rolls_back_on_send_failure() {
        let mut app = app_with_mux_windows(3); // ids 0,1,2
        // Drop the PTY so send_control fails.
        app.active_tab_mut().unwrap().pty = None;
        let before: Vec<u32> = app
            .active_tab()
            .unwrap()
            .mux_group
            .as_ref()
            .unwrap()
            .windows()
            .iter()
            .map(|w| w.id)
            .collect();
        // Attempt move id 0 → position 3; send fails → reverted.
        assert!(!app.confirm_mux_move(0, 3));
        let after: Vec<u32> = app
            .active_tab()
            .unwrap()
            .mux_group
            .as_ref()
            .unwrap()
            .windows()
            .iter()
            .map(|w| w.id)
            .collect();
        assert_eq!(before, after, "order reverted after send failure");
    }

    // ── observe_mux_key latch wiring + dialog reentry ─────────────────

    #[test]
    fn observe_mux_key_ignores_non_mux_tab() {
        let mut app = App::new();
        app.spawn_initial_tab();
        let t0 = Instant::now();
        let (consumed, _) = app.observe_mux_key(&crate::mux::prefix::KeyInput::letter('b'), t0);
        assert!(!consumed, "non-mux tab falls through");
    }

    #[test]
    fn observe_mux_key_arms_then_dispatches() {
        let mut app = app_with_mux_windows(3);
        let t0 = Instant::now();
        let (consumed, out) =
            app.observe_mux_key(&crate::mux::prefix::KeyInput::ctrl_letter('z'), t0);
        assert!(consumed);
        assert_eq!(out, MuxActionOutcome::None);
        let (consumed, out) =
            app.observe_mux_key(&crate::mux::prefix::KeyInput::ctrl_letter('n'), t0);
        assert!(consumed);
        assert_eq!(out, MuxActionOutcome::Changed);
        assert_eq!(active_idx(&app), 1);
    }

    #[test]
    fn observe_mux_key_unknown_followup_consumed() {
        let mut app = app_with_mux_windows(2);
        let t0 = Instant::now();
        app.observe_mux_key(&crate::mux::prefix::KeyInput::ctrl_letter('z'), t0);
        let (consumed, out) = app.observe_mux_key(&crate::mux::prefix::KeyInput::letter('q'), t0);
        assert!(consumed);
        assert_eq!(out, MuxActionOutcome::None);
    }

    #[test]
    fn observe_mux_key_rename_opens_dialog() {
        let mut app = app_with_mux_windows(2);
        let t0 = Instant::now();
        app.observe_mux_key(&crate::mux::prefix::KeyInput::ctrl_letter('z'), t0);
        let (consumed, out) =
            app.observe_mux_key(&crate::mux::prefix::KeyInput::ctrl_letter('r'), t0);
        assert!(consumed);
        assert!(matches!(out, MuxActionOutcome::OpenRename { .. }));
        app.handle_mux_outcome(out);
        assert!(matches!(
            app.mux_dialog,
            crate::mux::dialog::MuxDialogState::Rename { .. }
        ));
    }

    #[test]
    fn rename_dialog_reentry_guard() {
        let mut app = app_with_mux_windows(2);
        app.open_mux_rename_dialog(0, "a".to_string());
        // Second open with a different id must not replace the first.
        app.open_mux_rename_dialog(1, "b".to_string());
        match &app.mux_dialog {
            crate::mux::dialog::MuxDialogState::Rename { window_id, .. } => {
                assert_eq!(*window_id, 0);
            }
            other => panic!("expected Rename dialog, got {other:?}"),
        }
    }

    #[test]
    fn move_dialog_reentry_guard() {
        let mut app = app_with_mux_windows(2);
        app.open_mux_move_dialog(0, 1, 2);
        app.open_mux_move_dialog(1, 2, 2);
        match &app.mux_dialog {
            crate::mux::dialog::MuxDialogState::Move { window_id, .. } => {
                assert_eq!(*window_id, 0);
            }
            other => panic!("expected Move dialog, got {other:?}"),
        }
    }

    // ── Phase 4-G-A: ImeBackend wiring on App ────────────────────────

    use crate::ime::backend::testing::{MockBackend, MockState};
    use crate::ime::backend::{ImeEvent, KeyDispatchResult, RawKeyEvent};
    use crate::pty::input::Modifiers;
    use std::sync::{Arc, Mutex};

    fn mock_app() -> (App, Arc<Mutex<MockState>>) {
        let mut app = App::new();
        let (mock, state) = MockBackend::new();
        app.set_ime_backend(Box::new(mock));
        (app, state)
    }

    fn raw(pressed: bool) -> RawKeyEvent {
        RawKeyEvent {
            physical_key_code: 0x26,
            state_pressed: pressed,
            mods: Modifiers::NONE,
        }
    }

    // ── TS-backend-3: pump_ime routes events to on_ime_* ──────────

    #[test]
    fn pump_ime_routes_preedit_to_active_tab() {
        let (mut app, state) = mock_app();
        app.spawn_initial_tab();
        state
            .lock()
            .unwrap()
            .queue
            .push(ImeEvent::Preedit("hi".into()));
        let routed = app.pump_ime();
        assert!(routed);
        assert_eq!(app.active_tab().unwrap().preedit_state.text(), "hi");
    }

    #[test]
    fn pump_ime_routes_commit_clears_preedit() {
        let (mut app, state) = mock_app();
        app.spawn_initial_tab();
        // Stage a preedit so commit has something to clear.
        app.on_ime_preedit("ab");
        assert!(app.active_tab().unwrap().preedit_state.active());
        state
            .lock()
            .unwrap()
            .queue
            .push(ImeEvent::Commit("ab".into()));
        app.pump_ime();
        assert!(!app.active_tab().unwrap().preedit_state.active());
    }

    #[test]
    fn pump_ime_routes_focus_out_clears_preedit() {
        let (mut app, state) = mock_app();
        app.spawn_initial_tab();
        app.on_ime_preedit("xy");
        assert!(app.active_tab().unwrap().preedit_state.active());
        state.lock().unwrap().queue.push(ImeEvent::FocusOut);
        app.pump_ime();
        assert!(!app.active_tab().unwrap().preedit_state.active());
    }

    #[test]
    fn pump_ime_with_empty_queue_returns_false() {
        let (mut app, _state) = mock_app();
        app.spawn_initial_tab();
        let routed = app.pump_ime();
        assert!(!routed);
    }

    // ── TS-backend-4: Consumed result skips tao_key_to_bytes ──────

    #[test]
    fn dispatch_consumed_does_not_invoke_pty_path() {
        // We assert on the dispatch result; the App caller (window_host)
        // is the one that branches. Here we pin the contract that the
        // App's helper returns the backend's result verbatim.
        let (mut app, state) = mock_app();
        state.lock().unwrap().next_dispatch = KeyDispatchResult::Consumed;
        let r = app.dispatch_key_event_via_ime(&raw(true));
        assert_eq!(r, KeyDispatchResult::Consumed);
    }

    // ── TS-backend-5: Passthrough lets caller run encoder path ─────

    #[test]
    fn dispatch_passthrough_returns_passthrough() {
        // Default MockBackend state returns Passthrough.
        let (mut app, _state) = mock_app();
        let r = app.dispatch_key_event_via_ime(&raw(true));
        assert_eq!(r, KeyDispatchResult::Passthrough);
    }

    // ── TS-cursor-1: notify_cursor_rect rate-limited on cell change

    #[test]
    fn notify_cursor_rect_fires_once_per_cell_change() {
        let (mut app, state) = mock_app();
        app.spawn_initial_tab();
        // First call: cell (0,0) — should record one notification.
        app.notify_cursor_rect_if_changed(9, 18, 0, 28);
        assert_eq!(state.lock().unwrap().cursor_calls.len(), 1);
        // Second call without cursor movement: must NOT fire again.
        app.notify_cursor_rect_if_changed(9, 18, 0, 28);
        assert_eq!(state.lock().unwrap().cursor_calls.len(), 1);
        // Move the cursor → next call must fire.
        {
            let tab = app.active_tab().unwrap();
            tab.core.lock().process_pty_data(b"\x1b[5;3H");
        }
        app.notify_cursor_rect_if_changed(9, 18, 0, 28);
        assert_eq!(state.lock().unwrap().cursor_calls.len(), 2);
    }

    #[test]
    fn notify_cursor_rect_uses_pixel_size_from_args() {
        let (mut app, state) = mock_app();
        app.spawn_initial_tab();
        {
            let tab = app.active_tab().unwrap();
            tab.core.lock().process_pty_data(b"\x1b[3;4H"); // (row=2, col=3)
        }
        app.notify_cursor_rect_if_changed(9, 18, 4, 32);
        let calls = &state.lock().unwrap().cursor_calls;
        assert_eq!(calls.len(), 1);
        // x = col * cell_w + origin_x = 3 * 9 + 4 = 31.
        // y = row * cell_h + origin_y = 2 * 18 + 32 = 68.
        assert_eq!(calls[0].0, 31);
        assert_eq!(calls[0].1, 68);
        assert_eq!(calls[0].2, 9);
        assert_eq!(calls[0].3, 18);
    }

    #[test]
    fn notify_cursor_rect_with_no_active_tab_is_noop() {
        let (mut app, state) = mock_app();
        // No spawn → no tabs.
        app.notify_cursor_rect_if_changed(9, 18, 0, 28);
        assert!(state.lock().unwrap().cursor_calls.is_empty());
    }

    // ── TS-focus-1: notify_ime_focus + on_ime_focus_lost wiring ─────

    #[test]
    fn notify_ime_focus_propagates_to_backend() {
        let (mut app, state) = mock_app();
        app.notify_ime_focus(true);
        app.notify_ime_focus(false);
        assert_eq!(state.lock().unwrap().focus_calls, vec![true, false]);
    }

    // ── TS-route-1 (regression of Phase 4-E): Preedit via pump → sanitize

    #[test]
    fn pump_ime_preedit_with_esc_is_sanitized_via_phase4e_layer() {
        let (mut app, state) = mock_app();
        app.spawn_initial_tab();
        state
            .lock()
            .unwrap()
            .queue
            .push(ImeEvent::Preedit("a\x1bb".into()));
        app.pump_ime();
        // Phase 4-E sanitize must strip ESC (0x1b).
        assert_eq!(app.active_tab().unwrap().preedit_state.text(), "ab");
    }

    // ── TS-route-2 (regression): Commit via pump → sanitize + no
    //    bracketed-paste wrap. We can't easily inspect the real
    //    PtySession bytes here, but the route is identical to the
    //    direct `on_ime_commit` path which `ime::commit::tests`
    //    already pins. The piece we *can* verify is that the pump
    //    drops into `on_ime_commit` (preedit clears) and never panics
    //    on control bytes.
    #[test]
    fn pump_ime_commit_with_esc_does_not_panic_and_clears_overlay() {
        let (mut app, state) = mock_app();
        app.spawn_initial_tab();
        app.on_ime_preedit("draft");
        state
            .lock()
            .unwrap()
            .queue
            .push(ImeEvent::Commit("a\x1bb".into()));
        app.pump_ime();
        assert!(!app.active_tab().unwrap().preedit_state.active());
    }

    // ── set_ime_backend updates ime_is_null flag ────────────────────

    #[test]
    fn default_app_holds_null_backend() {
        let app = App::new();
        assert!(app.ime_is_null());
    }

    #[test]
    fn set_ime_backend_to_mock_clears_is_null_flag() {
        let mut app = App::new();
        assert!(app.ime_is_null());
        let (mock, _) = MockBackend::new();
        app.set_ime_backend(Box::new(mock));
        assert!(!app.ime_is_null());
    }

    // ── Zoom: clamp + runtime font size ───────────────────────────────

    #[test]
    fn clamp_font_size_pt_bounds() {
        // Below the floor clamps up; above the ceiling clamps down; a
        // value inside the range is returned unchanged.
        assert_eq!(clamp_font_size_pt(1.0), FONT_SIZE_PT_MIN);
        assert_eq!(clamp_font_size_pt(1000.0), FONT_SIZE_PT_MAX);
        assert_eq!(clamp_font_size_pt(12.0), 12.0);
        assert_eq!(clamp_font_size_pt(FONT_SIZE_PT_MIN), FONT_SIZE_PT_MIN);
        assert_eq!(clamp_font_size_pt(FONT_SIZE_PT_MAX), FONT_SIZE_PT_MAX);
    }

    #[test]
    fn zoom_seeds_runtime_size_from_settings() {
        let app = App::new();
        assert_eq!(app.runtime_font_size_pt, app.settings.font_size);
    }

    #[test]
    fn zoom_in_increments_by_one_point() {
        let mut app = App::new();
        let before = app.runtime_font_size_pt;
        assert!(app.zoom_in());
        assert!((app.runtime_font_size_pt - (before + FONT_SIZE_PT_STEP)).abs() < f32::EPSILON);
    }

    #[test]
    fn zoom_out_decrements_by_one_point() {
        let mut app = App::new();
        let before = app.runtime_font_size_pt;
        assert!(app.zoom_out());
        assert!((app.runtime_font_size_pt - (before - FONT_SIZE_PT_STEP)).abs() < f32::EPSILON);
    }

    #[test]
    fn zoom_reset_restores_settings_size() {
        let mut app = App::new();
        let baseline = app.settings.font_size;
        let _ = app.zoom_in();
        let _ = app.zoom_in();
        assert!(app.zoom_reset());
        assert!((app.runtime_font_size_pt - baseline).abs() < f32::EPSILON);
        // Resetting again is a no-op (already at the baseline).
        assert!(!app.zoom_reset());
    }

    #[test]
    fn zoom_clamps_at_ceiling_and_floor() {
        let mut app = App::new();
        // Drive up to the ceiling; the step that would exceed it returns
        // false (no change) once clamped.
        app.runtime_font_size_pt = FONT_SIZE_PT_MAX;
        assert!(!app.zoom_in(), "already at ceiling: no change expected");
        assert_eq!(app.runtime_font_size_pt, FONT_SIZE_PT_MAX);
        // Same at the floor.
        app.runtime_font_size_pt = FONT_SIZE_PT_MIN;
        assert!(!app.zoom_out(), "already at floor: no change expected");
        assert_eq!(app.runtime_font_size_pt, FONT_SIZE_PT_MIN);
    }

    #[test]
    fn set_font_size_pt_no_change_returns_false() {
        let mut app = App::new();
        let cur = app.runtime_font_size_pt;
        assert!(!app.set_font_size_pt(cur));
    }

    // ── Tab-bar runtime toggle ────────────────────────────────────────

    #[test]
    fn show_tab_bar_seeds_from_settings() {
        let app = App::new();
        assert_eq!(app.show_tab_bar, app.settings.show_tab_bar);
    }

    // ── Select-all ────────────────────────────────────────────────────

    #[test]
    fn select_all_without_active_tab_is_noop() {
        let mut app = App::new();
        // No tabs spawned (App::new does not call spawn_initial_tab).
        assert!(app.tabs.is_empty());
        app.select_all();
        assert!(
            app.selection.is_none(),
            "select_all with no active tab must not set a selection"
        );
    }

    #[test]
    fn select_all_action_routes_through_apply_action() {
        let mut app = App::new();
        // With no tabs this is a no-op, but it must not panic and must
        // report `false` (no exit request).
        let exit = app.apply_action(crate::ui::AppAction::SelectAll);
        assert!(!exit);
    }

    #[test]
    fn select_all_spans_visible_viewport_at_live() {
        // At live (offset 0) with some scrollback, select_all anchors at the
        // viewport top (= scrollback_len) and spans the on-screen rows.
        let mut app = app_with_prompts(50, &[]);
        let (cols, rows, scrollback_len) = {
            let core = app.tabs[0].core.lock();
            (core.cols(), core.rows(), core.get_scrollback_length())
        };
        app.select_all();
        let sel = app.selection.expect("select_all set a selection");
        assert_eq!(
            sel.anchor,
            Pos {
                row: scrollback_len,
                col: 0
            }
        );
        assert_eq!(
            sel.extent,
            Pos {
                row: scrollback_len + (rows - 1) as u32,
                col: cols - 1
            }
        );
    }

    #[test]
    fn select_all_uses_visible_start_when_scrolled() {
        // Scrolled back, select_all starts at the scrolled visible_start, not
        // at the live tail.
        let mut app = app_with_prompts(50, &[]);
        let (rows, scrollback_len) = {
            let core = app.tabs[0].core.lock();
            (core.rows(), core.get_scrollback_length())
        };
        app.scroll_set_offset(10);
        let visible_start = scrollback_len - 10;
        app.select_all();
        let sel = app.selection.expect("select_all set a selection");
        assert_eq!(sel.anchor.row, visible_start);
        assert_eq!(sel.extent.row, visible_start + (rows - 1) as u32);
    }

    #[test]
    fn pump_all_shifts_selection_by_eviction_delta() {
        // A selection in absolute rows is shifted down by the active tab's
        // accumulated eviction delta when `pump_all` runs.
        let mut app = App::new();
        app.spawn_initial_tab();
        app.selection = Some(Selection {
            anchor: Pos { row: 20, col: 0 },
            extent: Pos { row: 24, col: 3 },
            mode: SelectionMode::Character,
        });
        // Drive an eviction of 5 rows through the prompt-mark backfill, which
        // is what `pump` calls in production. This populates the tab's
        // `pending_eviction_delta`.
        app.tabs[0].test_backfill_eviction(5);
        app.pump_all();
        let sel = app.selection.expect("selection survives the shift");
        assert_eq!(sel.anchor, Pos { row: 15, col: 0 });
        assert_eq!(sel.extent, Pos { row: 19, col: 3 });
    }

    #[test]
    fn pump_all_drops_selection_when_fully_evicted() {
        // When the eviction delta exceeds both endpoints, the selection is
        // dropped entirely.
        let mut app = App::new();
        app.spawn_initial_tab();
        app.selection = Some(Selection {
            anchor: Pos { row: 2, col: 0 },
            extent: Pos { row: 6, col: 3 },
            mode: SelectionMode::Character,
        });
        app.tabs[0].test_backfill_eviction(10);
        app.pump_all();
        assert!(
            app.selection.is_none(),
            "fully-evicted selection must be dropped"
        );
    }

    #[test]
    fn pump_all_clears_selection_on_frame_reset() {
        // A core reset (RIS) makes the eviction counter go backwards, latching
        // a frame reset that drops the absolute-row selection.
        let mut app = App::new();
        app.spawn_initial_tab();
        // Establish a non-zero eviction baseline first.
        app.tabs[0].test_backfill_eviction(8);
        // Drain the resulting delta so it does not also shift the selection.
        let _ = app.tabs[0].take_eviction_delta();
        app.selection = Some(Selection {
            anchor: Pos { row: 4, col: 0 },
            extent: Pos { row: 9, col: 3 },
            mode: SelectionMode::Character,
        });
        // Counter goes backwards → frame reset latch.
        app.tabs[0].test_backfill_eviction(0);
        app.pump_all();
        assert!(
            app.selection.is_none(),
            "frame reset must clear the selection"
        );
    }

    /// TS-8 (integration): an off-thread snapshot swap completing on the
    /// active tab during a single `pump_all` reconciles like the synchronous
    /// path — the absolute-row selection is dropped (frame reset) and a full
    /// redraw is forced (FR2: a shorter incoming pane leaves no residual
    /// rows). No `pump_all` polling loop: the worker is blocked-ready first,
    /// then `pump_all` is called exactly once.
    #[test]
    fn ts8_offthread_swap_reconciles_active_tab_on_pump() {
        use mux_ipc::protocol::{MessageType, MuxMessage};

        let mut app = App::new();
        app.spawn_initial_tab();
        // Seed a 2-pane mux group, active pane = 10.
        {
            let group = app.tabs[0]
                .mux_group
                .get_or_insert_with(crate::mux::window_group::MuxWindowGroup::new);
            group.seed(
                vec![
                    crate::mux::window_group::MuxWindow {
                        id: 1,
                        name: "a".into(),
                    },
                    crate::mux::window_group::MuxWindow {
                        id: 2,
                        name: "b".into(),
                    },
                ],
                vec![10, 20],
                0,
            );
        }
        // A stale absolute-row selection that the frame reset must drop.
        app.selection = Some(Selection {
            anchor: Pos { row: 2, col: 0 },
            extent: Pos { row: 6, col: 3 },
            mode: SelectionMode::Character,
        });
        app.needs_full_redraw = false;

        // Dispatch a large snapshot off-thread for the active pane.
        let threshold = crate::tabs::OFFTHREAD_REPLAY_THRESHOLD_BYTES;
        let mut payload = b"SWAPPED-IN\r\n".to_vec();
        payload.resize(threshold + 16, 0);
        app.tabs[0].apply_mux_message(MuxMessage {
            msg_type: MessageType::Snapshot,
            pane_id: 10,
            payload,
        });
        assert!(app.tabs[0].test_has_pending_switch());

        // Block until the worker is ready (re-staged for try_recv), then pump
        // exactly once.
        app.tabs[0].test_block_worker_ready();
        app.pump_all();

        // Swap completed: no pending switch, content replaced, selection
        // dropped by the frame reset, full redraw forced.
        assert!(!app.tabs[0].test_has_pending_switch());
        assert_eq!(app.tabs[0].test_row_text(0), "SWAPPED-IN");
        assert!(
            app.selection.is_none(),
            "off-thread swap frame reset must drop the stale selection"
        );
        assert!(
            app.needs_full_redraw,
            "off-thread swap on the active tab must force a full redraw (FR2)"
        );
    }

    #[test]
    fn switch_to_tab_clears_selection() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab(); // now 2 tabs, active = 1
        app.selection = Some(Selection {
            anchor: Pos { row: 0, col: 0 },
            extent: Pos { row: 2, col: 3 },
            mode: SelectionMode::Character,
        });
        app.switch_to_tab(0);
        assert!(
            app.selection.is_none(),
            "switching tabs clears the active-tab-scoped selection"
        );
    }

    /// Seed one tab with a selection, a pending anchor, an OSC 133 prompt
    /// mark, and a fold region, after normalizing the grid to a known width.
    fn app_with_seeded_trackers() -> App {
        let mut app = App::new();
        app.spawn_initial_tab();
        // Normalize to a known width first; the very first set_grid_size may
        // itself be a width change from the default, which would clear the
        // (still empty) trackers — harmless, but we seed afterward.
        app.set_grid_size(80, 24);
        app.selection = Some(Selection {
            anchor: Pos { row: 1, col: 0 },
            extent: Pos { row: 3, col: 4 },
            mode: SelectionMode::Character,
        });
        app.pending_selection_anchor = Some(Pos { row: 2, col: 1 });
        app.tabs[0]
            .prompts
            .push(crate::prompts::ResolvedPromptMark {
                kind: crate::prompts::PromptMarkKind::PromptStart,
                row: 5,
                exit_code: None,
            });
        app.tabs[0]
            .folds
            .register_osc133_region(5, 8, "cmd".to_string(), None);
        app
    }

    #[test]
    fn width_change_clears_absolute_row_trackers() {
        // A column-width change reflows the buffer (rewriting the line
        // mapping without moving the eviction counter), so every absolute-row
        // tracker must be dropped (N3).
        let mut app = app_with_seeded_trackers();
        app.set_grid_size(40, 24); // width 80 -> 40
        assert!(app.selection.is_none(), "selection dropped on reflow");
        assert!(
            app.pending_selection_anchor.is_none(),
            "pending anchor dropped on reflow"
        );
        assert!(
            app.tabs[0].prompts.find_prev_prompt(u32::MAX).is_none(),
            "prompt marks cleared on reflow"
        );
        assert!(
            app.tabs[0].folds.get_region_at_line(5).is_none(),
            "fold regions cleared on reflow"
        );
    }

    #[test]
    fn height_only_change_keeps_absolute_row_trackers() {
        // A height-only resize does not reflow (resize_same_width keeps the
        // wrap boundaries), so the absolute-row trackers stay valid.
        let mut app = app_with_seeded_trackers();
        app.set_grid_size(80, 30); // same width 80, taller
        assert!(
            app.selection.is_some(),
            "selection kept on height-only resize"
        );
        assert!(
            app.pending_selection_anchor.is_some(),
            "pending anchor kept on height-only resize"
        );
        assert_eq!(
            app.tabs[0].prompts.find_prev_prompt(u32::MAX),
            Some(5),
            "prompt marks kept on height-only resize"
        );
        assert!(
            app.tabs[0].folds.get_region_at_line(5).is_some(),
            "fold regions kept on height-only resize"
        );
    }

    #[test]
    fn switch_to_tab_clears_pending_selection_anchor() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab(); // now 2 tabs, active = 1
        app.pending_selection_anchor = Some(Pos { row: 3, col: 2 });
        app.switch_to_tab(0);
        assert!(
            app.pending_selection_anchor.is_none(),
            "switching tabs clears the pending press anchor"
        );
    }

    #[test]
    fn set_alt_screen_true_clears_selection_and_anchor() {
        // Toggling onto the alt screen changes the buffer the absolute-row
        // selection addresses, so both the selection and a pending press
        // anchor must be dropped.
        let mut app = App::new();
        app.selection = Some(Selection {
            anchor: Pos { row: 1, col: 0 },
            extent: Pos { row: 2, col: 3 },
            mode: SelectionMode::Character,
        });
        app.pending_selection_anchor = Some(Pos { row: 1, col: 1 });
        app.set_alt_screen(true);
        assert!(
            app.selection.is_none(),
            "alt-screen toggle clears selection"
        );
        assert!(
            app.pending_selection_anchor.is_none(),
            "alt-screen toggle clears the pending press anchor"
        );
    }

    #[test]
    fn pump_all_shifts_pending_anchor_by_eviction_delta() {
        // A pending press anchor in absolute rows is shifted down by the
        // active tab's accumulated eviction delta, exactly like `selection`.
        let mut app = App::new();
        app.spawn_initial_tab();
        app.pending_selection_anchor = Some(Pos { row: 20, col: 4 });
        app.tabs[0].test_backfill_eviction(5);
        app.pump_all();
        assert_eq!(
            app.pending_selection_anchor,
            Some(Pos { row: 15, col: 4 }),
            "pending anchor shifts with the eviction delta"
        );
    }

    #[test]
    fn pump_all_drops_pending_anchor_when_scrolled_off_top() {
        // When the eviction delta exceeds the anchor's row, the anchor scrolled
        // off the top of scrollback and is dropped.
        let mut app = App::new();
        app.spawn_initial_tab();
        app.pending_selection_anchor = Some(Pos { row: 3, col: 0 });
        app.tabs[0].test_backfill_eviction(10);
        app.pump_all();
        assert!(
            app.pending_selection_anchor.is_none(),
            "a fully-evicted pending anchor is dropped"
        );
    }

    #[test]
    fn pump_all_clears_pending_anchor_on_frame_reset() {
        // A frame reset (RIS) drops the absolute-row pending anchor alongside
        // the selection.
        let mut app = App::new();
        app.spawn_initial_tab();
        app.tabs[0].test_backfill_eviction(8);
        let _ = app.tabs[0].take_eviction_delta();
        app.pending_selection_anchor = Some(Pos { row: 4, col: 0 });
        // Counter goes backwards → frame reset latch.
        app.tabs[0].test_backfill_eviction(0);
        app.pump_all();
        assert!(
            app.pending_selection_anchor.is_none(),
            "frame reset must clear the pending press anchor"
        );
    }

    #[test]
    fn select_all_uses_fold_layout_visible_span_when_collapsed() {
        // With a collapsed fold region in view, the screen rows are
        // non-contiguous in absolute space. select_all must take its
        // anchor/extent from the layout's first/last visible rows rather than
        // the linear `visible_start + (rows - 1)` model.
        let mut app = app_with_prompts(100, &[]);
        let (cols, scrollback_len) = {
            let core = app.tabs[0].core.lock();
            (core.cols(), core.get_scrollback_length())
        };
        // Collapse a region near the live tail so its summary survives in the
        // visible window.
        let region_start = scrollback_len + 1;
        let region_end = region_start + 5;
        app.tabs[0].folds.register_osc133_region(
            region_start,
            region_end,
            "cmd".to_string(),
            Some(0),
        );
        app.tabs[0].folds.toggle_fold(region_start);
        // Build the per-frame layout the renderer / select_all consult.
        app.refresh_fold_layout();
        let layout = app
            .fold_layout()
            .expect("collapsed region produces a layout")
            .clone();
        let expected_first = match layout.rows.first().unwrap() {
            crate::fold::FoldRowKind::Cells { actual_line } => *actual_line,
            crate::fold::FoldRowKind::Summary { region } => region.start_line,
        };
        let expected_last = match layout.rows.last().unwrap() {
            crate::fold::FoldRowKind::Cells { actual_line } => *actual_line,
            crate::fold::FoldRowKind::Summary { region } => region.start_line,
        };

        app.select_all();
        let sel = app.selection.expect("select_all set a selection");
        assert_eq!(
            sel.anchor,
            Pos {
                row: expected_first,
                col: 0
            }
        );
        assert_eq!(
            sel.extent,
            Pos {
                row: expected_last,
                col: cols - 1
            }
        );
    }

    #[test]
    fn dirty_set_maps_scrolled_selection_to_screen_rows() {
        // A selection in absolute rows is dirtied at the screen rows it
        // currently occupies, honoring scroll_offset.
        let mut app = app_with_prompts(50, &[]);
        // Clone the core Arc so the lock guard doesn't borrow `app` while we
        // need `&mut app` for record_render_state.
        let core_arc = app.tabs[0].core.clone();
        let scrollback_len = core_arc.lock().get_scrollback_length();
        // Scroll back by 10 → visible_start = scrollback_len - 10. Clear the
        // full-redraw latch so the union path runs.
        app.scroll_set_offset(10);
        {
            let mut core = core_arc.lock();
            app.record_render_state(&mut core);
        }
        let visible_start = scrollback_len - 10;
        // Select two absolute rows that fall on screen rows 3 and 4.
        app.selection = Some(Selection {
            anchor: Pos {
                row: visible_start + 3,
                col: 0,
            },
            extent: Pos {
                row: visible_start + 4,
                col: 5,
            },
            mode: SelectionMode::Character,
        });
        let set = {
            let core = core_arc.lock();
            app.dirty_rows_this_frame(&core)
        };
        assert!(set.contains(&3), "abs row visible_start+3 → screen row 3");
        assert!(set.contains(&4), "abs row visible_start+4 → screen row 4");
        // Screen row 0 holds neither a selected row nor the cursor (which sits
        // at the viewport bottom after the newline fill), and the core was
        // cleared of dirty bits by record_render_state, so it is absent.
        assert!(!set.contains(&0), "unselected screen row 0 stays clean");
    }

    // ── Settings window (child-process launcher) ───────────────

    /// Counting launcher double: records `open()` calls instead of
    /// spawning a real `--settings` child.
    struct CountingLauncher(std::rc::Rc<std::cell::Cell<usize>>);
    impl crate::settings_launcher::SettingsWindowLauncher for CountingLauncher {
        fn open(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    fn install_counting_launcher(app: &mut App) -> std::rc::Rc<std::cell::Cell<usize>> {
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        app.settings_launcher = Box::new(CountingLauncher(count.clone()));
        count
    }

    #[test]
    fn open_settings_action_spawns_the_settings_window() {
        let mut app = App::new();
        let opened = install_counting_launcher(&mut app);
        assert!(!app.apply_action(crate::ui::AppAction::OpenSettings));
        assert_eq!(opened.get(), 1);
    }

    #[test]
    fn open_settings_tab_event_spawns_the_settings_window() {
        let mut app = App::new();
        app.spawn_initial_tab();
        let opened = install_counting_launcher(&mut app);
        assert!(!app.apply_tab_event(crate::ui::TabEvent::OpenSettings));
        assert_eq!(opened.get(), 1);
        // The terminal pane keeps focus; no in-app tab is created.
        assert!(app.active_tab().is_some());
        assert_eq!(app.tabs.len(), 1);
    }

    // ── apply_settings ─────────────────────────────────────────

    #[test]
    fn apply_settings_rebuilds_keybinds_and_locale() {
        let mut app = App::new();
        let mut new = Settings::default();
        new.keybinds.new_tab = "Ctrl+Shift+N".to_string();
        new.language = crate::settings::Language::Ja;

        app.apply_settings(new);
        let expected = crate::ui::keybinds::parse_chord("Ctrl+Shift+N").unwrap();
        assert_eq!(app.keybinds.new_tab, expected);
        assert_eq!(app.locale, crate::i18n::Locale::Ja);
    }

    #[test]
    fn apply_settings_font_size_change_requests_resize_and_tracks_runtime() {
        let mut app = App::new();
        let mut new = Settings::default();
        new.font_size = 18.0;

        assert!(
            app.apply_settings(new),
            "font size change needs a grid reshape"
        );
        assert_eq!(app.runtime_font_size_pt, 18.0);
        assert_eq!(app.settings.font_size, 18.0);
    }

    #[test]
    fn apply_settings_behavior_only_change_needs_no_resize() {
        let mut app = App::new();
        let mut new = Settings::default();
        new.scroll_speed = 9;
        new.copy_on_select = true;

        assert!(!app.apply_settings(new));
        assert_eq!(app.settings.scroll_speed, 9);
        assert!(app.settings.copy_on_select);
    }

    #[test]
    fn apply_settings_clamps_to_save_ranges() {
        let mut app = App::new();
        let mut new = Settings::default();
        new.font_size = 500.0;
        new.scroll_speed = 99;

        app.apply_settings(new);
        assert_eq!(app.settings.font_size, 32.0);
        assert_eq!(app.settings.scroll_speed, 10);
    }

    #[test]
    fn apply_settings_updates_tab_theme_and_fold_gate() {
        let mut app = App::new();
        app.spawn_initial_tab();
        let mut new = Settings::default();
        new.terminal_color_scheme = "dracula".to_string();
        new.fold_enabled = false;
        new.cursor_blink = false;

        app.apply_settings(new);
        let theme = app.tabs[0].theme.lock().clone();
        let expected = crate::render::theme::Theme::from_settings(app.settings.as_ref());
        assert_eq!(
            theme.bg, expected.bg,
            "dracula background applied to live tab"
        );
        assert!(
            !app.tabs[0].folds.is_enabled(),
            "fold gate pushed into live manager"
        );
    }

    // ── SFTP close-guard / identity-capture regression tests ──────

    #[test]
    fn close_guard_resolves_by_stable_id_after_reorder() {
        // #7: the guard holds a stable_id, so a roster change between arming
        // and confirming must not close the wrong (or a missing) tab.
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_initial_tab();
        app.spawn_initial_tab();
        assert_eq!(app.tabs.len(), 3);

        // Arm the guard on the *middle* tab's stable_id.
        let target_id = app.tabs[1].stable_id;
        app.sftp_ui.close_guard = Some(target_id);

        // Reorder so the target is no longer at index 1.
        app.reorder_tab(1, 3); // middle tab moves to the end
        let new_idx = app
            .tabs
            .iter()
            .position(|t| t.stable_id == target_id)
            .expect("target still present");
        assert_eq!(new_idx, 2, "target moved to the end");

        // Confirming resolves by id and closes exactly the target tab.
        app.confirm_close_guard();
        assert_eq!(app.tabs.len(), 2);
        assert!(
            app.tabs.iter().all(|t| t.stable_id != target_id),
            "the guarded tab (by id) was the one closed"
        );
        assert!(app.sftp_ui.close_guard.is_none(), "guard cleared");
    }

    #[test]
    fn close_guard_missing_tab_aborts_cleanly() {
        // If the guarded tab vanished, confirm must not panic or close an
        // unrelated tab.
        let mut app = App::new();
        app.spawn_initial_tab();
        let only_id = app.tabs[0].stable_id;
        // Arm the guard on a stable_id that does not exist.
        app.sftp_ui.close_guard = Some(only_id.wrapping_add(9999));

        app.confirm_close_guard();

        // The unrelated tab is untouched; guard is cleared.
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.tabs[0].stable_id, only_id);
        assert!(app.sftp_ui.close_guard.is_none());
    }

    #[test]
    fn confirm_overwrite_uses_captured_tab_not_active() {
        // #4: confirm_overwrite_dialog must drive uploads against the dialog's
        // captured tab_id, and abort (no panic, error toast) when that tab is
        // gone instead of redirecting to the active tab.
        let mut app = App::new();
        app.spawn_initial_tab();
        let live_id = app.tabs[0].stable_id;

        // Overwrite dialog captured for a now-missing tab.
        app.sftp_ui.overwrite_dialog = Some(crate::sftp::ui::OverwriteDialog {
            paths: vec![std::path::PathBuf::from("/a/f.txt")],
            remote_dir: "/remote".to_string(),
            duplicates: vec!["f.txt".to_string()],
            tab_id: live_id.wrapping_add(7777),
            connection: crate::sftp::service::SftpConnection {
                hostname: "h".to_string(),
                port: 22,
                username: String::new(),
                identity_file: String::new(),
                ssh_options: Vec::new(),
            },
        });

        // Should abort with an error toast (the live non-SSH tab is not a
        // valid redirect target).
        app.confirm_overwrite_dialog(0.0);
        assert!(app.sftp_ui.overwrite_dialog.is_none(), "dialog consumed");
        assert!(
            app.sftp_ui
                .toasts
                .toasts
                .iter()
                .any(|t| t.status == crate::sftp::SftpUploadStatus::Failed),
            "an error toast was surfaced instead of redirecting the upload"
        );
    }
}
