//! Resize application and grid geometry: pending-resize handling, the
//! status-bar / mux-sidebar inset refresh, cell metrics, grid sizing,
//! and pixel -> cell / screen-row -> absolute-row conversion.

use std::time::Instant;

use winit::dpi::PhysicalPosition;

use crate::app::App;

use super::WindowHost;
use super::frame_pacing::{resize_settle_self_wake_due, status_bar_insets_changed};

impl WindowHost {
    /// Mark the surface as needing a reconfigure on the next render.
    ///
    /// Alacritty-style deferred resize: the caller (winit `Resized` /
    /// `ScaleFactorChanged` handlers) only flips the flag and requests a
    /// redraw. The actual `surface.configure()` + PTY grid resize happens
    /// once in [`apply_pending_resize`] at the head of [`render`].
    pub fn request_resize(&mut self) {
        self.pending_resize = true;
    }

    /// Consume `pending_resize` and apply the latest `window.surface_size()`.
    ///
    /// Called once per `render()` so a burst of compositor resize events
    /// produces a single configure + PTY resize cycle aligned with the
    /// frame boundary. Zero-sized windows (Windows minimize, Wayland hidden)
    /// just clear the flag without reconfiguring.
    ///
    /// task0005 (findings `0029db1c89ab226f` / `5b2f22c5a14f7364`):
    /// `pending_resize` can be set by sources OTHER than
    /// [`ResizeSettler`]'s own forwarded decision — the mux-sidebar inset
    /// refresh, or a compositor `Resized` / `ScaleFactorChanged` — so this
    /// method must not assume the status-bar height has settled just
    /// because it is running. [`Self::grid_size`] enforces that: it reads
    /// [`Self::status_bar_bot_inset_settled_logical`], never the immediate
    /// `status_bar_bot_inset_logical`, so whichever source triggered this
    /// call, the size actually applied and broadcast is always derived
    /// from a settler-forwarded inset.
    pub(super) fn apply_pending_resize(&mut self, app: &mut App) {
        if !self.pending_resize {
            return;
        }
        self.pending_resize = false;
        let size = self.window.surface_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.surface_config.width = size.width;
        self.surface_config.height = size.height;
        self.surface.configure(&self.device, &self.surface_config);
        let (cols, rows) = self.grid_size(app);
        app.set_grid_size(cols, rows);
    }

    /// Refresh `status_bar_*_inset_logical` from the active view model.
    /// Called at the head of each `render()` (before
    /// [`apply_pending_resize`]) so the PTY row count and the grid
    /// origin stay in sync with the currently-rendered status-bar
    /// panel. Routes every computed candidate grid size through
    /// [`ResizeSettler`] rather than applying it immediately (FR6,
    /// mux-tab-switch-replay-latency task0002) — a startup/reattach
    /// settling storm otherwise flags a pending resize (and, once
    /// applied, broadcasts a group-wide `Resize`) once per transient
    /// transition instead of once for the settled size.
    ///
    /// task0005: the candidate is fed to the settler UNCONDITIONALLY on
    /// every render, even when it matches the currently-applied inset —
    /// filtering that upstream used to starve the settler of whichever
    /// side of a 2-state oscillation happened to match the still-applied
    /// value, biasing quiescence detection (finding 02546e5e10deb500-c).
    /// `ResizeSettler` itself now decides whether a candidate is a no-op
    /// repeat.
    ///
    /// mux-tab-switch-bypass-refix task0002 D-D: the drawing insets
    /// (below) and the PTY reshape trigger (`pending_resize`) are two
    /// separate concerns from here on. Change 2 (findings
    /// `a82206113b8160fd` / `aba5ebbdf9a9addb`) applies
    /// `status_bar_*_inset_logical` whenever the VALUES themselves change
    /// ([`status_bar_insets_changed`]), independent of whether
    /// `ResizeSettler::observe` forwards a grid-size decision this frame —
    /// a status-bar height change whose derived `(cols, rows)` candidate
    /// happens to be unchanged (cell height above `ROW_HEIGHT` at larger
    /// font sizes, or row clamping) previously left the insets stale
    /// forever, permanently mis-routing mux-sidebar pointer input which
    /// reads the same fields. No extra repaint is requested for this: the
    /// height change that drives a new inset value always changes
    /// `App::status_bar_view_model`, which `render()`'s `should_skip_frame`
    /// check already treats as a mandatory-paint signal via
    /// `status_bar_changed`, so the fresh inset lands in the very frame
    /// that computed it. `pending_resize` stays tied to the settler's
    /// forwarded decision only (unchanged from before this task), so an
    /// inset-only change never re-triggers FR6's reshape-storm guard.
    ///
    /// Change 1 (finding `81507f39e384b34e`): while the settler is still
    /// debouncing ([`ResizeSettler::awaiting_decision`]), this requests
    /// another redraw so a fully idle window does not strand a pending
    /// settle indefinitely (findings 02546e5e10deb500 /
    /// 5b1878c41d3e02d6-perf-P2) — but now rate-limited via
    /// [`resize_settle_self_wake_due`] instead of firing unconditionally
    /// on every frame; see that function's doc comment and
    /// [`RESIZE_SETTLE_SELF_WAKE_INTERVAL`] for why an unconditional wake
    /// spins the render loop at full speed, and
    /// [`next_resize_settle_wake_deadline`] / `PocApp::about_to_wait` for
    /// how the rate-limited wake still keeps arriving with zero other
    /// activity in the window.
    ///
    /// task0005 round-1 rework (findings `0029db1c89ab226f` /
    /// `5b2f22c5a14f7364`): D-D above split WHICH EVENTS set
    /// `pending_resize` from the settler, but left `grid_size()` reading
    /// `status_bar_bot_inset_logical` — the same field this method now
    /// writes immediately, every render, regardless of settler state. The
    /// traced firing order this closes: a fresh mux attach/reattach
    /// resets the settler (above) and this method writes the transient,
    /// first-frame status-bar height into `status_bar_bot_inset_logical`
    /// in the SAME call; `refresh_mux_sidebar_inset` (called next, in
    /// `render`) then raises `pending_resize` on its own, unrelated
    /// inset change; `apply_pending_resize` (called after that) used to
    /// compute the PTY grid from whatever inset was currently sitting in
    /// `status_bar_bot_inset_logical` — the just-written transient value,
    /// not anything the settler had judged stable. Sharpening the D-D
    /// boundary: drawing and pointer-routing insets stay immediate
    /// (unchanged, right above); grid computation and the `Resize`
    /// broadcast now consume ONLY [`Self::status_bar_bot_inset_settled_
    /// logical`], advanced below via [`resolve_grid_bot_inset`] exactly
    /// when the settler is not withholding judgment — never on a bare
    /// `pending_resize` flip from a non-settler source.
    pub(super) fn refresh_status_bar_insets(&mut self, app: &App) {
        // FR5 (mux-status-bar-removal task0001): `vm` — and therefore
        // `height`, the bottom inset, and the grid-size candidate computed
        // below — is a pure function of `app.settings.statusbar` and the
        // OSC `777;statusbar` dispatcher's own state. `App::status_bar_view_model`
        // takes no mux-attach input at all, so attaching/detaching a mux
        // session cannot change the row count or grid size this method
        // derives (see `status_bar/runtime.rs::build_view_model`).
        let vm = app.status_bar_view_model();
        let height = crate::ui::status_bar::panel_height_logical(&vm);
        // The status bar is fixed at the bottom; reserve its height there.
        let (top, bot) = (0.0, height);

        // A fresh mux attach/reattach reopens the settling window (unrelated
        // to the status-bar row count above): `mux_session_name` transitioning
        // from absent to present is the same `first_welcome` moment
        // `Tab::apply_mux_message` uses to seed the mux group and push its
        // own one-time Resize — that seed does not go through
        // `ResizeSettler` at all, so this reset only affects how quickly a
        // SEPARATE, unrelated settle (e.g. a concurrent window resize)
        // converges around the same moment.
        let mux_attached = app
            .active_tab()
            .is_some_and(|t| t.mux_session_name.is_some());
        if mux_attached && !self.mux_was_attached {
            self.resize_settler.reset();
            // A fresh settling window's first self-wake must fire
            // immediately, not be delayed by a stale timestamp left over
            // from a previously-closed window (see `last_resize_settle_
            // wake`'s doc comment).
            self.last_resize_settle_wake = None;
        }
        self.mux_was_attached = mux_attached;

        // Change 2: apply the inset values independent of the settler
        // below (D-D) — see this method's doc comment.
        if status_bar_insets_changed(
            self.status_bar_top_inset_logical,
            self.status_bar_bot_inset_logical,
            top,
            bot,
        ) {
            self.status_bar_top_inset_logical = top;
            self.status_bar_bot_inset_logical = bot;
        }

        let candidate = self.grid_size_for_bot_inset(app, bot);
        let now = Instant::now();
        let forwarded = self.resize_settler.observe(candidate, now).is_some();
        // task0005 (findings `0029db1c89ab226f` / `5b2f22c5a14f7364`): the
        // settled bot inset [`Self::grid_size`] consumes only ever tracks
        // `bot` when the settler is not mid-settle, evaluated AFTER this
        // render's `observe` call above — see `resolve_grid_bot_inset`'s
        // doc comment for the exact rule.
        self.status_bar_bot_inset_settled_logical = resolve_grid_bot_inset(
            self.status_bar_bot_inset_settled_logical,
            bot,
            self.resize_settler.awaiting_decision(),
        );
        if forwarded {
            self.pending_resize = true;
        } else if resize_settle_self_wake_due(
            self.resize_settler.awaiting_decision(),
            self.last_resize_settle_wake,
            now,
        ) {
            self.last_resize_settle_wake = Some(now);
            self.window().request_redraw();
        }
    }

    /// Refresh `mux_sidebar_inset_logical` from [`App::mux_sidebar_visibility`]
    /// (task0005 D2). Called at the head of each `render()` alongside
    /// [`Self::refresh_status_bar_insets`], mirroring its change-detection /
    /// `pending_resize` flip so the PTY reshapes exactly once when the inset
    /// actually moves — entering/leaving a mux-attached tab in persistent
    /// mode, or flipping `mux.window_sidebar_overlay` (NFR1). Overlay
    /// open/close never changes this value (overlay always resolves to 0
    /// inset — [`crate::app::mux_sidebar_grid_inset`]), so toggling it never
    /// touches `pending_resize` here.
    pub(super) fn refresh_mux_sidebar_inset(&mut self, app: &App) {
        let scale = self.pixels_per_point.max(1.0) as f64;
        let window_width_logical = (self.surface_config.width.max(1) as f64 / scale) as f32;
        let inset =
            crate::app::mux_sidebar_grid_inset(app.mux_sidebar_visibility(), window_width_logical);
        if (self.mux_sidebar_inset_logical - inset).abs() > f32::EPSILON {
            self.mux_sidebar_inset_logical = inset;
            self.pending_resize = true;
        }
    }

    /// Cell metrics in **physical pixels**, matching what
    /// `TerminalGridPass::prepare` is fed (see render path:
    /// `app.cell_w_logical * scale`, `app.cell_h_logical * scale`,
    /// origin = `(padding * scale, (TITLE_BAR + TAB_BAR +
    /// STATUS_BAR_TOP + padding) * scale)`).
    ///
    /// Returns `(cell_w_px, cell_h_px, origin_x_px, origin_y_px)`. All
    /// values are floats so the per-row stepping stays sub-pixel
    /// accurate — using rounded integers causes the click-to-cell hit
    /// test to drift further from the visual cell every row, which is
    /// exactly the bug `pixel_to_cell` used to hit by dividing by 18
    /// while cells were drawn at 17 px.
    pub(super) fn cell_metrics_px(&self, app: &App) -> (f64, f64, f64, f64) {
        let scale = self.pixels_per_point.max(1.0) as f64;
        // Cell dims come from the App's startup measurement of the
        // base font (see `App::with_settings` → `compute_cell_dims`)
        // so they track `settings.font_size` instead of the legacy
        // hard-coded 8.5×17.
        let cell_w = (app.cell_w_logical as f64) * scale;
        let cell_h = (app.cell_h_logical as f64) * scale;
        // Inner padding comes from settings.padding (logical pixels);
        // falls back to the renderer's default constants when the
        // user hasn't overridden them. task0006 (right-edge persistent
        // placement): the persistent mux sidebar reserves usable grid
        // WIDTH only (see `grid_size`) — it never contributes to
        // `origin_x`, so this is the same formula as before the sidebar
        // existed.
        let pad = (app.settings.padding as f64).max(0.0);
        let origin_x = pad * scale;
        // Vertical origin reserves room for every panel stacked above
        // the terminal grid: the CSD title bar, the tab strip, an
        // optional status-bar row pinned to the top, and the same
        // user-configured padding inset. Forgetting the title bar
        // here makes the first cell render behind the tab strip.
        let tab_h = crate::ui::tab_bar::effective_tab_bar_height(app.show_tab_bar) as f64;
        let origin_y = ((crate::ui::title_bar::TITLE_BAR_HEIGHT as f64)
            + tab_h
            + (self.status_bar_top_inset_logical as f64)
            + pad)
            * scale;
        (cell_w, cell_h, origin_x, origin_y)
    }

    /// Compute grid (cols, rows) from the current window pixel size,
    /// using the real cell metrics so the PTY size agrees with the
    /// number of cells the renderer actually paints.
    ///
    /// task0005 (findings `0029db1c89ab226f` / `5b2f22c5a14f7364`): reads
    /// [`Self::status_bar_bot_inset_settled_logical`], NOT the immediate
    /// `status_bar_bot_inset_logical` — grid computation (and, through
    /// [`Self::apply_pending_resize`], the group-wide `Resize` broadcast)
    /// must consume only a settler-forwarded inset value, so a transient
    /// height mid-settle can never reach the PTYs regardless of which
    /// source raised `pending_resize` this frame.
    pub fn grid_size(&self, app: &App) -> (u16, u16) {
        self.grid_size_for_bot_inset(app, self.status_bar_bot_inset_settled_logical)
    }

    /// Pure grid-size computation, factored out of [`Self::grid_size`]
    /// (FR6, mux-tab-switch-replay-latency task0002) so
    /// [`Self::refresh_status_bar_insets`] can compute the CANDIDATE grid
    /// size for a not-yet-applied `status_bar_bot_inset_logical` value —
    /// feeding it to [`ResizeSettler`] — without first mutating
    /// `self.status_bar_bot_inset_logical`. `grid_size` itself passes the
    /// settled inset through unchanged (task0005), so its behavior is
    /// identical to before this refactor except for WHICH inset field
    /// that is.
    pub(super) fn grid_size_for_bot_inset(&self, app: &App, bot_inset_logical: f32) -> (u16, u16) {
        let w = self.surface_config.width.max(1) as f64;
        let h = self.surface_config.height.max(1) as f64;
        let (cell_w, cell_h, origin_x, origin_y) = self.cell_metrics_px(app);
        let scale = self.pixels_per_point.max(1.0) as f64;
        let bottom_inset_px = (bot_inset_logical as f64) * scale;
        // task0006 (right-edge persistent placement): the persistent mux
        // sidebar reserves usable WIDTH from the right edge — subtracted
        // here directly, not folded into `origin_x` (which stays at the
        // pre-sidebar pad-only value). `mux_sidebar_inset_logical` is
        // `0.0` in overlay mode / on a local tab, reproducing the
        // pre-sidebar usable width exactly.
        let sidebar_inset_px = (self.mux_sidebar_inset_logical as f64).max(0.0) * scale;
        // Usable area starts after the top bar (+ top status bar) +
        // top pad and the left pad, ends above the bottom status bar,
        // and is narrowed on the right by the persistent sidebar (if
        // any). Floor the resulting cell count so partial trailing
        // cells (which would clip at the surface edge) don't get
        // reported as a writable row/col.
        let usable_w = (w - origin_x - sidebar_inset_px).max(cell_w);
        let usable_h = (h - origin_y - bottom_inset_px).max(cell_h);
        let cols = (usable_w / cell_w).floor().clamp(20.0, 500.0) as u16;
        let rows = (usable_h / cell_h).floor().clamp(5.0, 200.0) as u16;
        (cols, rows)
    }

    /// Map a physical pixel position to a grid cell `(row, col)`,
    /// honoring the same origin + cell metrics the renderer uses.
    pub(super) fn pixel_to_cell(&self, pos: PhysicalPosition<f64>, app: &App) -> (u16, u16) {
        let (cell_w, cell_h, origin_x, origin_y) = self.cell_metrics_px(app);
        let x = ((pos.x - origin_x).max(0.0)) / cell_w;
        let y = ((pos.y - origin_y).max(0.0)) / cell_h;
        let cols = app.cell_size.cols.max(1);
        let rows = app.cell_size.rows.max(1);
        let col = (x as u32).min((cols - 1) as u32) as u16;
        let row = (y as u32).min((rows - 1) as u32) as u16;
        (row, col)
    }

    /// Resolve the absolute buffer row shown at `screen_row`, honoring the
    /// current fold layout (collapsed bodies skew the linear mapping; a
    /// summary row maps to its region's start line).
    pub(super) fn screen_row_to_abs(&self, screen_row: u16, app: &App) -> u32 {
        if let Some(layout) = app.fold_layout() {
            match layout.rows.get(screen_row as usize) {
                Some(crate::fold::FoldRowKind::Cells { actual_line }) => return *actual_line,
                Some(crate::fold::FoldRowKind::Summary { region }) => return region.start_line,
                None => {} // past the layout → fall through to the linear map
            }
        }
        // No fold layout (or screen_row past it): the linear scrollback
        // model. `visible_start = scrollback_len - scroll_offset` (saturating)
        // is the absolute row at the top of the viewport.
        let scrollback_len = app
            .active_tab()
            .map(|t| t.core.lock().get_scrollback_length())
            .unwrap_or(0);
        scrollback_len.saturating_sub(app.scroll_offset()) + screen_row as u32
    }
}

/// Resolve the bottom inset value [`WindowHost::grid_size`] may consume
/// after THIS render's [`ResizeSettler::observe`] call (task0005 round-1
/// rework, findings `0029db1c89ab226f` / `5b2f22c5a14f7364`): grid
/// computation, and the group-wide `Resize` broadcast it feeds through
/// [`WindowHost::apply_pending_resize`], must consume only a
/// settler-forwarded inset value — never the transient one
/// `WindowHost::refresh_status_bar_insets` writes immediately into
/// `status_bar_bot_inset_logical` for drawing / pointer-routing (Change
/// 2, D-D, mux-tab-switch-bypass-refix task0002).
///
/// `settler_awaiting_decision` is [`ResizeSettler::awaiting_decision`]
/// evaluated immediately AFTER `observe` runs this render:
///
/// - `true` — the settling window is still open: either genuinely
///   mid-storm (candidate not yet held stable), or freshly reopened by
///   `ResizeSettler::reset` on a mux attach/reattach. `previously_settled`
///   is returned unchanged, so [`WindowHost::apply_pending_resize`] can
///   never pick up `transient` this frame — regardless of WHICH source
///   (the settler itself, the mux-sidebar inset refresh, or a compositor
///   `Resized` / `ScaleFactorChanged`) is what raised `pending_resize`.
///   This is what closes the traced firing order: a fresh attach resets
///   the settler and writes a transient inset in the same call, a
///   sidebar-driven `pending_resize` follows, and without this gate the
///   apply that comes after would have used the transient value.
/// - `false` — the settler is not withholding judgment: either it just
///   forwarded `transient` this render (closing the window on exactly the
///   value that produced the forwarded candidate — this is what keeps
///   [`ResizeSettler::last_forwarded`] and the size
///   `apply_pending_resize` actually computes in agreement, no
///   divergence), or the window was already closed from a prior render
///   (the steady-state case: every render's `bot` IS the current,
///   non-transient inset, so tracking it here with no lag avoids stale
///   lock-in across a run of inset changes whose derived grid-size
///   candidate happens not to move — the FR4 case `status_bar_insets_
///   changed` alone already applies for drawing).
pub(super) fn resolve_grid_bot_inset(
    previously_settled: f32,
    transient: f32,
    settler_awaiting_decision: bool,
) -> f32 {
    if settler_awaiting_decision {
        previously_settled
    } else {
        transient
    }
}
