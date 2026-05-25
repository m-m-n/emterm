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

pub struct App {
    pub tabs: Vec<Tab>,
    pub active: usize,
    /// Last known grid size in cells. Updated by `window_host` whenever the
    /// window resizes; PTYs are resized to match.
    pub cell_size: GridDims,
    /// Active mouse selection on the active tab, if any. Phase 4 owns this;
    /// later phases may move it per-tab when tabs preserve selection.
    pub selection: Option<Selection>,
    /// Runtime settings (ambiguous-width policy, OSC 52 policy, and
    /// future fields). Loaded from `settings.json` in Phase 7; today
    /// initialized to default. Wrapped in `Arc` so the per-tab
    /// `NativeCallbacks` can share an immutable view without copying.
    pub settings: Arc<Settings>,
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
    /// Cursor row/col from the previous rendered frame. The renderer dirties
    /// this row so a moved cursor doesn't ghost the old position.
    previous_cursor: Option<(u16, u16)>,
    /// Selection from the previous rendered frame. Vacated selection rows
    /// must be repainted to clear highlight.
    previous_selection: Option<Selection>,
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
        let settings = Arc::new(settings);
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

        Self {
            tabs: Vec::new(),
            active: 0,
            cell_size: GridDims::default(),
            selection: None,
            settings,
            scroll_position: ScrollPosition::Live,
            alt_screen: false,
            window_focused: true,
            blink_started: Instant::now(),
            previous_blink_visible: true,
            previous_cursor: None,
            previous_selection: None,
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
            emoji_texture_cache: Arc::new(Mutex::new(EmojiTextureCache::new())),
            status_bar_runtime,
            active_cwd,
            previous_status_bar_view_model: None,
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

        #[cfg(not(test))]
        let noto_sans_jp_id = resolver.register_system_family(&cjk_family, FontRole::Cjk);
        #[cfg(test)]
        let noto_sans_jp_id: Option<FontId> = None;

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
                let swash = Arc::new(crate::render::font::swash_adapter::SwashRasterizer::new());
                swash.ingest_resolver(&resolver);
                swash
            }
            FontEngine::AbGlyph => {
                // ab_glyph escape hatch: we wrap the bundled CJK font
                // (which carries a Latin sub-set) so ASCII still
                // renders. CJK / emoji return None and the fallback
                // chain stops — that is the documented degradation
                // path (FR5).
                match crate::render::font::ab_glyph_adapter::AbGlyphRasterizer::from_static_bytes(
                    crate::render::font::resolver::BUNDLED_CJK_FONT,
                    bundled_cjk_id,
                ) {
                    Some(r) => {
                        log::info!("font_engine = ab_glyph (escape hatch); CJK / emoji may tofu");
                        Arc::new(r)
                    }
                    None => {
                        log::warn!(
                            "font.unknown_engine: ab_glyph failed to parse bundled CJK; falling back to swash"
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
        );
        self.tabs.push(tab);
        self.active = 0;
        // A brand-new tab populated rows; ensure the first frame draws them.
        self.needs_full_redraw = true;
    }

    /// Spawn an additional shell tab, switch to it, and request a
    /// repaint. Used by `AppAction::NewTab` and `TabEvent::New`.
    pub fn spawn_new_tab(&mut self) {
        let dims = self.cell_size;
        let tab = Tab::spawn_shell(
            "shell",
            dims.cols,
            dims.rows,
            self.settings.scrollback_lines,
            self.settings.clone(),
            Some(self.status_bar_runtime.dispatcher()),
            Some(self.status_bar_runtime.cwd_provider()),
        );
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

    /// Apply a [`crate::ui::TabEvent`] emitted by the tab bar widget.
    /// Returns `true` when the resulting state should exit the window
    /// (i.e. the last tab was closed).
    pub fn apply_tab_event(&mut self, evt: crate::ui::TabEvent) -> bool {
        match evt {
            crate::ui::TabEvent::New => {
                self.spawn_new_tab();
                false
            }
            crate::ui::TabEvent::Close(idx) => self.close_tab(idx),
            crate::ui::TabEvent::Switch(idx) => {
                self.switch_to_tab(idx);
                false
            }
            crate::ui::TabEvent::Reorder { from, to } => {
                self.reorder_tab(from, to);
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
                let idx = self.active;
                self.close_tab(idx)
            }
            crate::ui::AppAction::NextTab => {
                if self.tabs.is_empty() {
                    return false;
                }
                let next = (self.active + 1) % self.tabs.len();
                self.switch_to_tab(next);
                false
            }
            crate::ui::AppAction::PrevTab => {
                if self.tabs.is_empty() {
                    return false;
                }
                let prev = if self.active == 0 {
                    self.tabs.len() - 1
                } else {
                    self.active - 1
                };
                self.switch_to_tab(prev);
                false
            }
            crate::ui::AppAction::JumpTab(n) => {
                if self.tabs.is_empty() {
                    return false;
                }
                // n is 1-based and clamped to the existing range.
                let idx = (n.saturating_sub(1) as usize).min(self.tabs.len() - 1);
                self.switch_to_tab(idx);
                false
            }
        }
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
        self.tabs.get(self.active)
    }

    #[allow(dead_code)] // retained for future mutation paths / tests
    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active)
    }

    /// Drain PTY events on every tab. Returns true if any tab produced
    /// new bytes (caller schedules a redraw).
    pub fn pump_all(&mut self) -> bool {
        let mut changed = false;
        for tab in &mut self.tabs {
            // Phase 4-C (APC redesign): `Tab::pump` already routes
            // APC-encoded mux messages into the tab's own state via
            // `apply_mux_message` (see `crate::mux::apc`). There is no
            // separate `pump_mux` pass — the bridge CLI runs inside the
            // same PTY, so a single drain is sufficient.
            if tab.pump() {
                changed = true;
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
        // the auto-follow rule can preserve the off-tail offset.
        if changed {
            self.on_pty_output();
        }
        // Reap exited tabs (Phase 5 will refine the policy).
        let before = self.tabs.len();
        self.tabs.retain(|t| !t.exited);
        if self.tabs.len() != before {
            if self.active >= self.tabs.len() && !self.tabs.is_empty() {
                self.active = self.tabs.len() - 1;
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
        for tab in &self.tabs {
            tab.resize(cols, rows);
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

    /// React to new PTY output. When already at the live tail, no-op (the
    /// renderer will pick up the new rows automatically). When sitting at an
    /// offset, preserve the offset so the user is not yanked to the bottom.
    /// Visually, the offset stays anchored because `term_core`'s ring buffer
    /// shifts the old content into scrollback under us.
    pub fn on_pty_output(&mut self) {
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

        // Selection history: union of previous + current.
        let selection_range = |sel: &Selection| -> (u16, u16) {
            let (start, end) = sel.ordered();
            (start.row, end.row)
        };
        if let Some(sel) = &self.selection {
            let (s, e) = selection_range(sel);
            for r in s..=e.min(rows - 1) {
                push_unique(&mut set, r);
            }
        }
        if let Some(sel) = &self.previous_selection {
            let (s, e) = selection_range(sel);
            for r in s..=e.min(rows - 1) {
                push_unique(&mut set, r);
            }
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
        app.on_pty_output();
        // No offset change.
        assert_eq!(app.scroll_position, ScrollPosition::Live);
        // No redraw forced (already at live, nothing visual shifted).
        assert!(!app.needs_full_redraw);
    }

    #[test]
    fn on_pty_output_preserves_offset() {
        let mut app = App::new();
        app.scroll_up_by(4);
        app.needs_full_redraw = false;
        app.on_pty_output();
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
}
