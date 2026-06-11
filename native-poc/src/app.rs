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
use crate::ime::backend::{ImeBackend, ImeEvent, KeyDispatchResult, RawKeyEvent, PUMP_BUDGET};
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
/// `Live` means the user is tracking new output (auto-follow). When PTY
/// output arrives in this state, the viewport advances with it.
///
/// `OffsetFromLive(n)` means the user has scrolled back `n` rows into
/// scrollback. New PTY output preserves this offset so the user does not
/// get yanked back to the bottom mid-read.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ScrollPosition {
    #[default]
    Live,
    OffsetFromLive(u32),
}

/// Direction for [`App::jump_to_prompt`]: `Prev` scrolls toward older
/// prompts (up), `Next` toward newer prompts (down).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpDirection {
    Prev,
    Next,
}

pub struct App {
    pub tabs: Vec<Tab>,
    pub active: usize,
    /// In-app settings panel state. `Some` while the Settings tab is
    /// open (it occupies the last slot of the tab strip); `None` once
    /// closed. Port of the WebView's `createTab({ type: "settings" })`
    /// tab, except the native strip pins it to the end.
    pub settings_panel: Option<crate::ui::settings_panel::PanelState>,
    /// Whether the Settings tab is the active pane. While `true`,
    /// [`App::active_tab`] reports no active terminal tab (key input,
    /// selection, scroll, IME, and the grid renderer all observe a
    /// tabless state) and the settings panel owns the central area.
    /// `self.active` keeps pointing at the terminal tab to return to.
    pub settings_active: bool,
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
        // Resolve the user-configured chord table once. Unparseable
        // specs fall back to their built-in defaults with a warn log
        // (see `KeybindTable::from_settings`).
        let keybinds = crate::ui::keybinds::KeybindTable::from_settings(&settings.keybinds);
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

        Self {
            tabs: Vec::new(),
            active: 0,
            settings_panel: None,
            settings_active: false,
            cell_size: GridDims::default(),
            selection: None,
            pending_selection_anchor: None,
            search: crate::search::SearchState::default(),
            search_focus_request: false,
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
        // Host-installed emoji font (if any) takes precedence over the
        // bundled one — both go in the chain so a codepoint absent
        // from the host font still falls through to bundled.
        if let Some(id) = host_emoji_id {
            extras.push(id);
        }
        extras.push(emoji_id);
        let preferred_emoji_id = host_emoji_id.unwrap_or(emoji_id);
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
            (Some(id), Some(family)) => log::info!("font.emoji = {} (id={:?})", family, id),
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
        );
        self.tabs.push(tab);
        self.active = 0;
        // A brand-new tab populated rows; ensure the first frame draws them.
        self.needs_full_redraw = true;
    }

    /// Spawn an additional shell tab, switch to it, and request a
    /// repaint. Used by `AppAction::NewTab` and `TabEvent::New`.
    pub fn spawn_new_tab(&mut self) {
        // A new tab always becomes the active pane; leave the Settings
        // tab open in the strip but drop its focus.
        if self.settings_active {
            self.settings_active = false;
            self.needs_full_redraw = true;
        }
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
        self.needs_full_redraw = true;
    }

    /// Close the tab at `idx`. Returns `true` when the close emptied
    /// the tabs vector, signaling the app loop that the window should
    /// exit. `tabs.is_empty()` is the same signal; this is a
    /// convenience for code that needs to branch immediately after
    /// the close.
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
        self.active = idx;
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

    // ── Settings tab (strip-index model) ─────────────────────────
    //
    // The tab strip shows the terminal tabs (indices `0..tabs.len()`)
    // plus, when open, the Settings tab pinned at index `tabs.len()`.
    // Tab-bar events arrive in strip indices and are translated here.

    /// `true` while the Settings tab is the active pane.
    pub fn settings_panel_active(&self) -> bool {
        self.settings_active
    }

    /// Strip index of the Settings tab when it is open.
    fn settings_strip_index(&self) -> Option<usize> {
        self.settings_panel.is_some().then_some(self.tabs.len())
    }

    /// Number of entries in the tab strip (terminal tabs + Settings).
    pub fn tab_strip_len(&self) -> usize {
        self.tabs.len() + usize::from(self.settings_panel.is_some())
    }

    /// Strip index of the active pane.
    pub fn active_strip_index(&self) -> usize {
        if self.settings_active {
            self.tabs.len()
        } else {
            self.active
        }
    }

    /// Open the Settings tab (creating its state on first use) and make
    /// it the active pane. Mirrors the WebView `open_settings` handler:
    /// re-invoking switches to the existing tab instead of duplicating.
    pub fn activate_settings_tab(&mut self) {
        if self.settings_panel.is_none() {
            self.settings_panel = Some(crate::ui::settings_panel::PanelState::new(
                &self.settings,
                self.locale,
            ));
        }
        if !self.settings_active {
            self.settings_active = true;
            // Same invalidation set as a terminal tab switch: the search
            // overlay / selection address the outgoing tab's buffer.
            if self.search.visible {
                self.search.close();
            }
            self.selection = None;
            self.pending_selection_anchor = None;
            self.needs_full_redraw = true;
        }
    }

    /// Switch the active pane to the terminal tab at `idx`, leaving the
    /// Settings tab (if any) open in the strip.
    pub fn activate_terminal_tab(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        if self.settings_active {
            self.settings_active = false;
            self.needs_full_redraw = true;
            // `switch_to_tab` early-returns on idx == active; the flag
            // flip above already restored the terminal view for that case.
            if idx != self.active {
                self.switch_to_tab(idx);
            }
        } else {
            self.switch_to_tab(idx);
        }
    }

    /// Close the Settings tab. When it was the active pane, focus
    /// returns to the terminal tab `self.active` kept pointing at.
    pub fn close_settings_tab(&mut self) {
        self.settings_panel = None;
        if self.settings_active {
            self.settings_active = false;
            self.needs_full_redraw = true;
        }
    }

    /// Activate the strip entry at `idx` (terminal tab or Settings).
    fn activate_strip(&mut self, idx: usize) {
        if Some(idx) == self.settings_strip_index() {
            self.activate_settings_tab();
        } else {
            self.activate_terminal_tab(idx);
        }
    }

    /// Apply a [`crate::ui::TabEvent`] emitted by the tab bar widget.
    /// Returns `true` when the resulting state should exit the window
    /// (i.e. the last tab was closed).
    ///
    /// Event indices are **strip** indices: the Settings tab (when
    /// open) occupies the last slot after the terminal tabs.
    pub fn apply_tab_event(&mut self, evt: crate::ui::TabEvent) -> bool {
        match evt {
            crate::ui::TabEvent::New => {
                self.spawn_new_tab();
                false
            }
            crate::ui::TabEvent::OpenSettings => {
                self.activate_settings_tab();
                false
            }
            crate::ui::TabEvent::Close(idx) => {
                if Some(idx) == self.settings_strip_index() {
                    self.close_settings_tab();
                    false
                } else {
                    self.close_tab(idx)
                }
            }
            crate::ui::TabEvent::Switch(idx) => {
                self.activate_strip(idx);
                false
            }
            crate::ui::TabEvent::Reorder { from, to } => {
                // The Settings tab is pinned to the strip's end: a drag
                // that starts on it is ignored, and a drop past the last
                // terminal slot clamps to the terminal range.
                if from >= self.tabs.len() {
                    return false;
                }
                self.reorder_tab(from, to.min(self.tabs.len()));
                false
            }
        }
    }

    /// Apply a global keybind [`crate::ui::AppAction`]. Returns `true`
    /// when the resulting state should exit the window.
    pub fn apply_action(&mut self, action: crate::ui::AppAction) -> bool {
        match action {
            crate::ui::AppAction::NewTab => {
                self.spawn_new_tab();
                false
            }
            crate::ui::AppAction::CloseTab => {
                if self.settings_active {
                    self.close_settings_tab();
                    return false;
                }
                let idx = self.active;
                self.close_tab(idx)
            }
            crate::ui::AppAction::NextTab => {
                let total = self.tab_strip_len();
                if total == 0 {
                    return false;
                }
                let next = (self.active_strip_index() + 1) % total;
                self.activate_strip(next);
                false
            }
            crate::ui::AppAction::PrevTab => {
                let total = self.tab_strip_len();
                if total == 0 {
                    return false;
                }
                let cur = self.active_strip_index();
                let prev = if cur == 0 { total - 1 } else { cur - 1 };
                self.activate_strip(prev);
                false
            }
            crate::ui::AppAction::JumpTab(n) => {
                let total = self.tab_strip_len();
                if total == 0 {
                    return false;
                }
                // n is 1-based and clamped to the existing strip range.
                let idx = (n.saturating_sub(1) as usize).min(total - 1);
                self.activate_strip(idx);
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
                self.activate_settings_tab();
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

        // UI chrome palette: preset × brightness swaps live (the md3
        // slot is process-wide, so the next frame re-skins every
        // widget).
        crate::ui::md3::set_preset(new.ui_theme_preset, new.ui_theme);
        // Keybinds / locale resolve the same way as startup.
        self.keybinds = crate::ui::keybinds::KeybindTable::from_settings(&new.keybinds);
        self.locale = crate::i18n::resolve(new.language);

        let font_families_changed = new.font_family_fallback != old.font_family_fallback
            || new.emoji_font != old.emoji_font;
        let font_size_changed = (new.font_size - old.font_size).abs() >= f32::EPSILON;
        let padding_changed = new.padding != old.padding;

        self.settings = Arc::new(new);

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
        // While the Settings tab is the active pane there is no active
        // *terminal* tab: key input, selection, scroll, IME, and the
        // grid renderer all see a tabless state, which is exactly the
        // set of no-ops the settings view needs.
        if self.settings_active {
            return None;
        }
        self.tabs.get(self.active)
    }

    #[allow(dead_code)] // retained for future mutation paths / tests
    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        if self.settings_active {
            return None;
        }
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
        // While the Settings tab is the active pane every terminal tab
        // counts as inactive (matching the WebView, where the settings
        // tab being active makes all terminal tabs background tabs for
        // the activity tracker). `usize::MAX` never matches an index.
        let active = if self.settings_active {
            usize::MAX
        } else {
            self.active
        };
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
            if idx == active {
                active_eviction_delta = eviction_delta;
                active_frame_reset = frame_reset;
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
        // Mirror the active tab's alt-screen flag onto the App so the
        // scroll input routes can suppress wheel / Shift+Page during
        // alt-screen sessions.
        let active_alt = self.tabs.get(self.active).map(|t| t.alt_screen);
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
        self.cell_size = GridDims { cols, rows };
        // A column-width change triggers a `term_core` reflow that rewrites
        // the logical↔physical line mapping (a height-only change does not —
        // `resize_same_width` keeps the wrap boundaries). Detect it before
        // the resize so the now-stale absolute-row trackers can be dropped
        // afterward (N3). All tabs share `cell_size`, so any one differing is
        // enough; checking all is harmless.
        let mut width_changed = false;
        for tab in &self.tabs {
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
        if let Some(pty) = tab.pty.as_ref() {
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

    // ── Settings tab (strip model) ─────────────────────────────

    #[test]
    fn activate_settings_tab_creates_panel_and_hides_active_tab() {
        let mut app = App::new();
        app.spawn_initial_tab();
        assert!(app.active_tab().is_some());

        app.activate_settings_tab();
        assert!(app.settings_panel.is_some());
        assert!(app.settings_panel_active());
        // The settings pane reports a tabless state to every consumer.
        assert!(app.active_tab().is_none());
        // The strip shows tabs + the pinned settings slot, which is active.
        assert_eq!(app.tab_strip_len(), 2);
        assert_eq!(app.active_strip_index(), 1);
    }

    #[test]
    fn activate_settings_tab_twice_keeps_existing_panel() {
        let mut app = App::new();
        app.activate_settings_tab();
        app.settings_panel.as_mut().unwrap().draft.font_size = 20.0;
        // Re-invoking (the WebView "switch to existing tab" path) must
        // not recreate the panel state.
        app.activate_settings_tab();
        assert_eq!(app.settings_panel.as_ref().unwrap().draft.font_size, 20.0);
    }

    #[test]
    fn activate_terminal_tab_returns_focus_without_closing_panel() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.activate_settings_tab();

        app.activate_terminal_tab(0);
        assert!(!app.settings_panel_active());
        assert!(app.settings_panel.is_some(), "panel stays open in strip");
        assert!(app.active_tab().is_some());
        assert_eq!(app.active_strip_index(), 0);
    }

    #[test]
    fn close_settings_tab_restores_terminal_focus() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.activate_settings_tab();

        app.close_settings_tab();
        assert!(app.settings_panel.is_none());
        assert!(!app.settings_panel_active());
        assert!(app.active_tab().is_some());
        assert_eq!(app.tab_strip_len(), 1);
    }

    #[test]
    fn tab_event_switch_and_close_map_settings_strip_index() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.activate_settings_tab();
        app.activate_terminal_tab(0);

        // Strip index 1 (= tabs.len()) is the settings slot.
        assert!(!app.apply_tab_event(crate::ui::TabEvent::Switch(1)));
        assert!(app.settings_panel_active());

        // Closing the settings slot never exits the window.
        assert!(!app.apply_tab_event(crate::ui::TabEvent::Close(1)));
        assert!(app.settings_panel.is_none());
        assert!(!app.settings_panel_active());
    }

    #[test]
    fn next_prev_tab_cycle_includes_settings_slot() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.activate_settings_tab();
        app.activate_terminal_tab(0);

        assert!(!app.apply_action(crate::ui::AppAction::NextTab));
        assert!(
            app.settings_panel_active(),
            "next from last tab wraps to settings"
        );
        assert!(!app.apply_action(crate::ui::AppAction::NextTab));
        assert!(
            !app.settings_panel_active(),
            "next from settings wraps to tab 0"
        );
        assert!(!app.apply_action(crate::ui::AppAction::PrevTab));
        assert!(
            app.settings_panel_active(),
            "prev from tab 0 wraps to settings"
        );
    }

    #[test]
    fn close_tab_action_closes_settings_when_focused() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.activate_settings_tab();

        // CloseTab on the focused settings tab closes the panel, not a
        // terminal tab, and never signals window exit.
        assert!(!app.apply_action(crate::ui::AppAction::CloseTab));
        assert!(app.settings_panel.is_none());
        assert_eq!(app.tabs.len(), 1);
    }

    #[test]
    fn reorder_from_settings_slot_is_ignored() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab();
        app.activate_settings_tab();
        let first_id = app.tabs[0].stable_id;

        // `from` = strip index of the settings tab (tabs.len() == 2).
        assert!(!app.apply_tab_event(crate::ui::TabEvent::Reorder { from: 2, to: 0 }));
        assert_eq!(app.tabs[0].stable_id, first_id, "terminal order unchanged");
        assert!(app.settings_panel.is_some());
    }

    #[test]
    fn spawn_new_tab_drops_settings_focus() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.activate_settings_tab();

        app.spawn_new_tab();
        assert!(!app.settings_panel_active());
        assert!(app.settings_panel.is_some(), "panel stays open in strip");
        assert_eq!(app.active, 1);
    }

    #[test]
    fn open_settings_action_activates_panel() {
        let mut app = App::new();
        assert!(!app.apply_action(crate::ui::AppAction::OpenSettings));
        assert!(app.settings_panel_active());
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
}
