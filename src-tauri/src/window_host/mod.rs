//! winit window + wgpu surface + egui integration.
//!
//! This module owns the GPU surface lifecycle and translates winit events
//! into egui inputs. The egui<->winit glue is intentionally minimal because
//! no published crate covers the winit+wgpu+egui combination as of this
//! writing (egui_winit exists but pulls in trackpad/file-drop helpers we
//! do not need).
//!
//! Phase 1 responsibilities:
//! - Create a winit window via the supplied event loop.
//! - Acquire a wgpu adapter/device and attach a surface.
//! - Recreate the surface on `SurfaceError::Lost` / `OutOfMemory`.
//! - Drive a per-frame egui pass that renders a placeholder UI.
//!
//! Phase 2 additions:
//! - Translate winit `KeyboardInput` events to PTY bytes via `pty::input`.
//! - Forward bytes to the active tab.
//! - Compute grid (cols, rows) from the window's pixel size and propagate to
//!   PTYs on resize.
//!
//! Phase 4-G redesign (2026-05-14): migrated from tao 0.34 to winit 0.30.
//! tao does not expose XKB keycodes, breaking self-built XIM. winit hosts
//! IME natively via `WindowEvent::Ime`; the Phase 4-G-3 bridge wires those
//! events into the existing Phase 4-E `on_ime_preedit / on_ime_commit /
//! on_ime_focus_lost` plumbing.

use std::sync::Arc;
use std::time::Instant;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::cursor::CursorIcon;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{ResizeDirection, Window, WindowAttributes};

use crate::app::App;
use crate::pty::input::Modifiers;

mod event_loop;

use event_loop::PocApp;
mod frame_pacing;
mod input_translate;
mod key_routing;
mod link_hover;
mod render_surface;
mod resize_layout;

use link_hover::{ClickTracker, HoverState};

use frame_pacing::{FrameCounter, ResizeSettler, RowsRebuiltCounter};

use crate::render::terminal_grid_pass::TerminalGridPass;

use crate::ui::chrome::{RESIZE_EDGE_PX, classify_resize_edge, configure_egui_fonts};

/// Owns the window, the wgpu surface, the egui context, and the
/// egui-wgpu renderer.
///
/// Field declaration order is also the drop order — wgpu resources that
/// depend on the surface / device / window are declared first so they
/// run their destructors before the underlying `Surface`, `Instance`,
/// and `Window` go away. In particular: the `Surface<'static>` we
/// constructed via `create_surface_unsafe` from `&Window` references the
/// window's native handle; tearing the window down before the surface
/// produces a use-after-free on the Vulkan WSI side and was the cause
/// of the segfault observed when closing the title-bar X button. See
/// `Drop` impl below for the explicit shutdown handshake.
pub struct WindowHost {
    /// Phase 4-H (font-swash-migration FR12): custom wgpu pass that
    /// draws terminal cells (foreground glyph + background fill +
    /// underline / strikethrough). Constructed lazily once the App is
    /// available so the font stack can be taken from `App::font_*`.
    /// Frame draw order is `clear → TerminalGridPass → egui (LoadOp::Load)`.
    grid_pass: Option<TerminalGridPass>,
    egui_renderer: egui_wgpu::Renderer,
    egui_ctx: egui::Context,
    /// Construction instant, fed to egui as `RawInput::time` every frame so
    /// egui's clock advances in real time. With `time: None` egui substitutes
    /// `previous_time + predicted_dt` (a fixed 1/60 s per pass), turning its
    /// clock into a frame counter: anything scheduled against it — the
    /// restart/SFTP toast auto-dismiss in `App::pump_sftp`, double-click
    /// detection — then runs fast whenever frames outpace 60 Hz (Mailbox
    /// present never blocks) and stalls when frames are skipped.
    egui_start: Instant,
    /// Last time `about_to_wait` requested a redraw on behalf of an active
    /// toast. Rate-limits that request to the `TOAST_POLL_MS` cadence:
    /// an unconditional request would re-enter `RedrawRequested` immediately
    /// (never reaching the `WaitUntil` timer), and with a non-blocking
    /// present mode the loop then spins at full speed for the toast's
    /// entire lifetime.
    last_toast_redraw: Option<Instant>,
    surface_config: wgpu::SurfaceConfiguration,
    queue: wgpu::Queue,
    device: wgpu::Device,
    surface: wgpu::Surface<'static>,
    instance: wgpu::Instance,
    window: Arc<dyn Window>,
    pixels_per_point: f32,
    /// True when the surface must be recreated on the next frame (e.g. after
    /// `SurfaceError::Lost`).
    surface_dirty: bool,
    /// Alacritty-style deferred resize: `WindowEvent::SurfaceResized` only flips
    /// this flag and requests a redraw; the next `render()` call reads the
    /// current `window.surface_size()` once and runs `surface.configure()` +
    /// `app.set_grid_size()` together. This coalesces bursts of compositor
    /// resize events (one configure per frame instead of one per event) and
    /// avoids back-buffer locking when configure and draw happen out of
    /// order on Wayland / X11. See
    /// `wezterm/window/src/os/x11/window.rs:298` (coalesce) and
    /// `alacritty/src/display/mod.rs:739` (defer-to-render).
    pending_resize: bool,
    /// Cached status-bar panel insets in egui logical points, refreshed
    /// each frame from `App::status_bar_view_model`. Drawing / pointer-
    /// routing values ONLY (task0005 D-D sharpening, findings
    /// `0029db1c89ab226f` / `5b2f22c5a14f7364`) — [`Self::grid_size`]
    /// must NOT read `status_bar_bot_inset_logical` directly; it reads
    /// [`Self::status_bar_bot_inset_settled_logical`] below instead, so a
    /// transient (not-yet-settled) height written here can never reach
    /// the PTY grid computation or the group-wide `Resize` broadcast.
    status_bar_top_inset_logical: f32,
    status_bar_bot_inset_logical: f32,
    /// task0005 (findings `0029db1c89ab226f` / `5b2f22c5a14f7364`): the
    /// bottom inset value [`Self::grid_size`] actually consumes,
    /// decoupled from `status_bar_bot_inset_logical` above. Advanced by
    /// [`resolve_grid_bot_inset`] — see that function's doc comment for
    /// the exact update rule and why it closes the defect (a
    /// non-settler `pending_resize` source, e.g. the mux-sidebar inset
    /// refresh or a compositor `Resized` / `ScaleFactorChanged`, could
    /// otherwise apply a size derived from a not-yet-settled status-bar
    /// height during the settling window).
    status_bar_bot_inset_settled_logical: f32,
    /// Cached persistent mux-sidebar horizontal grid inset in egui logical
    /// points (task0005 D2), refreshed each frame from
    /// [`App::mux_sidebar_visibility`] via [`Self::refresh_mux_sidebar_inset`].
    /// Subtracted from the usable WIDTH in [`Self::grid_size`] so the
    /// terminal grid never renders behind the right-edge persistent
    /// sidebar (task0006 update) — it does NOT touch `origin_x` in
    /// [`Self::cell_metrics_px`]; the grid's x-origin is identical with
    /// and without the sidebar. Always `0.0` in overlay mode or when the
    /// active tab is not mux-attached (NFR1: those cases must not reshape
    /// the PTY).
    mux_sidebar_inset_logical: f32,
    /// FR6 (mux-tab-switch-replay-latency task0002): debounces the
    /// status-bar-height-driven grid-size candidate computed in
    /// [`Self::refresh_status_bar_insets`] so a startup/reattach settling
    /// storm reaches [`Tab::resize`](crate::tabs::Tab::resize)'s
    /// group-wide broadcast at most once, for the settled size — see
    /// [`ResizeSettler`].
    resize_settler: ResizeSettler,
    /// Last-observed "is the active tab mux-attached" state, used to
    /// detect the `None` → `Some` `mux_session_name` transition (a fresh
    /// attach or reattach) and reopen `resize_settler`'s settling window.
    mux_was_attached: bool,
    /// Last time a resize-settle self-wake actually requested a redraw
    /// (mux-tab-switch-bypass-refix task0002 Change 1, finding
    /// `81507f39e384b34e`). Rate-limits
    /// [`Self::refresh_status_bar_insets`]'s awaiting-decision wake and
    /// `PocApp::about_to_wait`'s `WaitUntil`-driven follow-up to
    /// [`RESIZE_SETTLE_SELF_WAKE_INTERVAL`], mirroring `last_toast_redraw`'s
    /// role for `toast_redraw_due`. `None` means no self-wake has fired yet
    /// in the currently-open settling window, so the first one fires
    /// immediately. Reset to `None` alongside `resize_settler.reset()` so a
    /// fresh settling window's first wake is never delayed by a stale
    /// timestamp from a previously-closed one.
    last_resize_settle_wake: Option<Instant>,
    current_mods: Modifiers,
    /// Last cursor position in physical pixels (updated on `PointerMoved`).
    cursor_pos: PhysicalPosition<f64>,
    /// Whether the left button is currently held — used as the gate for
    /// turning subsequent `PointerMoved` events into selection extends.
    dragging: bool,
    /// Click tracker for double / triple click detection.
    click_tracker: ClickTracker,
    /// Lazily-initialized arboard clipboard. We only fail-loud once if the
    /// platform clipboard cannot be acquired (X11 without display, etc.).
    clipboard: Option<arboard::Clipboard>,
    /// Pointer / scroll events accumulated between renders, drained by
    /// `build_raw_input` so the egui-side widgets (tab bar, status bar,
    /// future settings panel) can observe clicks / hovers / drags.
    /// Without this, `build_raw_input` ships `events: vec![]` and egui
    /// never sees pointer input even though winit already delivered it.
    pending_egui_events: Vec<egui::Event>,
    /// Pointer buttons currently held (pressed-minus-released count).
    /// While non-zero, `PointerMoved` counts as actionable input for the
    /// frame-skip veto: egui chrome drags (scrollbar thumb, tab reorder)
    /// are driven purely by motion between press and release, so skipping
    /// motion-only frames mid-drag would freeze their live tracking on an
    /// idle terminal. Reset on focus loss (the release may never arrive).
    pointer_buttons_down: u8,
    /// Set when the user clicks the CSD title-bar's `×` button. The
    /// `about_to_wait` handler picks this up and runs the same
    /// teardown handshake (drop `host` → `event_loop.exit()`) used
    /// for the last-tab-closed path, so the wgpu / X11 resources
    /// unwind in the same order regardless of the close path.
    pending_close: bool,
    /// Cached CSD resize direction under the pointer. Refreshed on
    /// every `PointerMoved` (when not selection-dragging) so the next
    /// left-press can hand the matching [`ResizeDirection`] to
    /// `Window::drag_resize_window` without re-running the hit test.
    /// `None` means the pointer is in the window interior — a press
    /// falls through to the existing selection / tab-bar handlers.
    current_resize_dir: Option<ResizeDirection>,
    /// Last cursor icon pushed to winit. Cached so [`update_resize_hint`]
    /// can skip the IPC round-trip when the icon would not change —
    /// `set_cursor` is otherwise called on every `PointerMoved`, which
    /// floods the compositor with redundant requests.
    current_cursor: CursorIcon,
    /// Link-hover state (URL / file-path auto-detection). Refreshed only
    /// when the pointer crosses into a new grid cell so the detection
    /// regex doesn't run per pixel.
    hover: HoverState,
    /// Set by [`WindowHost::refresh_link_hover`] /
    /// [`WindowHost::invalidate_link_hover`] when the hovered link
    /// cell-span actually changed (appear, move, disappear) since the
    /// last frame (task0005 AC-1, finding 63c273cd4e0d66b1). `render`
    /// consumes this once per frame and forces a full redraw so the
    /// affected rows — baked into the grid instances as the hover
    /// underline — actually rebuild in the row cache instead of the
    /// honest-dirty-set skip serving a stale cached row. Hover spans
    /// change on enter/leave-style events (not per-pixel), so a full
    /// redraw on change is cheap.
    hover_span_changed: bool,
    /// True while the pointer is inside the window. Set to `true` by
    /// `PointerMoved` (there is no `PointerEntered` handler) and to `false`
    /// by `PointerLeft`. Used to gate PTY-output re-detection in
    /// `about_to_wait`: when the pointer has left the window there is
    /// nothing to underline, so we skip the `find_link_at` work entirely.
    pointer_in_window: bool,
    /// Accumulated sub-notch wheel delta for the alternate-scroll path
    /// (DECSET 1007 / FR1). Carries fractional pixel-delta lines across
    /// events so trackpad micro-scrolls eventually resolve to one arrow
    /// key per whole-line boundary.
    alt_scroll_accum: f32,
    /// `EMTERM_RENDER_PERF=1` gate (task0002 FR6-half). Read once at
    /// construction and cached here — mirrors the `perf_log` field idiom
    /// in `render::font::cache::GlyphCache::new`.
    render_perf_enabled: bool,
    /// Frames-drawn counter, active only while `render_perf_enabled`.
    frame_counter: FrameCounter,
    /// Rows-rebuilt counter (task0003 FR6-half), active only while
    /// `render_perf_enabled`. Same env gate as `frame_counter`.
    rows_rebuilt_counter: RowsRebuiltCounter,
}

/// Terminal font family used to skin the egui `Monospace` chain
/// (status-bar text). Runtime settings flatten `font_family_primary`
/// into `font_family_fallback[0]`, so we read the first entry — empty
/// when the user did not configure one, in which case chrome falls
/// back to egui's default Hack.
fn terminal_font_family(settings: &crate::settings::Settings) -> &str {
    settings
        .font_family_fallback
        .first()
        .map(String::as_str)
        .unwrap_or("")
}

impl WindowHost {
    /// Build the window + GPU resources.
    pub fn new(
        event_loop: &dyn ActiveEventLoop,
        ui_font_family: &str,
        terminal_font_family: &str,
    ) -> Self {
        let attrs = WindowAttributes::default()
            .with_title("eMterm PoC")
            .with_decorations(false)
            .with_surface_size(LogicalSize::new(960.0, 600.0))
            .with_min_surface_size(LogicalSize::new(320.0, 200.0))
            .with_maximized(true)
            // FR2: attach the bundled app icon to the main winit window so
            // the title bar and taskbar (Windows fallbacks) render the
            // eMterm glyph. `None` from `app_icon()` is a clean no-op.
            .with_window_icon(crate::window_icon::app_icon());
        // FR5: report the canonical dock-grouping identifier (X11
        // `WM_CLASS` / Wayland `app_id`) so every window groups under one
        // `emterm` dock icon. The active event loop decides between the
        // X11 / Wayland platform-attribute builders (see `linux_wm`).
        #[cfg(target_os = "linux")]
        let attrs = crate::linux_wm::with_app_id(event_loop, attrs);
        let window: Arc<dyn Window> = Arc::from(
            event_loop
                .create_window(attrs)
                .expect("native-poc: failed to create winit window"),
        );

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        // SAFETY: `window` is kept alive in `Arc<dyn Window>` and stored
        // alongside the surface for the whole `WindowHost` lifetime.
        let surface: wgpu::Surface<'static> = unsafe {
            instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: window.display_handle().expect("display handle").as_raw(),
                    raw_window_handle: window.window_handle().expect("window handle").as_raw(),
                })
                .expect("create surface")
        };

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }))
        .expect("native-poc: no compatible wgpu adapter found");

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("native-poc-device"),
                required_features: wgpu::Features::empty(),
                required_limits:
                    wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .expect("native-poc: failed to request wgpu device");

        let size = window.surface_size();
        let surface_caps = surface.get_capabilities(&adapter);
        // Prefer a NON-sRGB surface. All of our color sources (theme
        // palette, egui paint, glyph fg/bg) are sRGB-encoded byte values;
        // we want them written to the framebuffer verbatim, matching the
        // WebView build's Canvas 2D gamma-space pipeline. An *sRGB*
        // surface would treat shader output as linear and re-encode it,
        // brightening every mid-tone on screen (and egui itself warns
        // "egui prefers Rgba8Unorm or Bgra8Unorm" for the same reason).
        let format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        // Prefer Mailbox over Fifo so window resize doesn't lag the mouse:
        // Fifo blocks `Present` on the next vsync (≈16.7 ms at 60 Hz), so
        // each compositor resize event has to wait a full frame before the
        // window catches up. Mailbox queues the most recent submission
        // non-blocking, replacing any older queued frame, which removes the
        // vsync wall while still avoiding tearing on most desktops.
        // Fall back to Immediate (allows tearing but never blocks) and
        // finally Fifo (always supported by spec) when Mailbox is absent —
        // some Mesa drivers / proprietary stacks only expose Fifo.
        let present_mode = if surface_caps
            .present_modes
            .contains(&wgpu::PresentMode::Mailbox)
        {
            wgpu::PresentMode::Mailbox
        } else if surface_caps
            .present_modes
            .contains(&wgpu::PresentMode::Immediate)
        {
            wgpu::PresentMode::Immediate
        } else {
            wgpu::PresentMode::Fifo
        };
        log::info!(
            "native-poc: surface present mode = {:?} (available: {:?})",
            present_mode,
            surface_caps.present_modes
        );

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            // Mailbox / Immediate keep the GPU queue shallow on their own;
            // 2 keeps Fifo's existing pipelining intact.
            desired_maximum_frame_latency: 2,
        };
        // Phase 0: defer the very first `surface.configure` to the first
        // redraw. tao reports an initial size before the surface is fully
        // ready on some Linux/Vulkan stacks, producing
        // `ERROR_SURFACE_LOST_KHR` when configure runs in `new()`. By marking
        // `surface_dirty = true` here, the first `render()` call will run
        // `reconfigure_surface()` under the same dirty/lost recovery path
        // used for in-flight surface loss, which is already covered.

        let egui_ctx = egui::Context::default();
        configure_egui_fonts(&egui_ctx, ui_font_family, terminal_font_family);
        let egui_renderer = egui_wgpu::Renderer::new(&device, format, None, 1, false);

        let pixels_per_point = window.scale_factor() as f32;

        let clipboard = match arboard::Clipboard::new() {
            Ok(c) => Some(c),
            Err(e) => {
                log::warn!("arboard clipboard unavailable: {e}");
                None
            }
        };

        Self {
            window,
            instance,
            surface,
            surface_config,
            device,
            queue,
            egui_ctx,
            egui_renderer,
            egui_start: Instant::now(),
            last_toast_redraw: None,
            pixels_per_point,
            surface_dirty: true,
            pending_resize: false,
            status_bar_top_inset_logical: 0.0,
            status_bar_bot_inset_logical: 0.0,
            status_bar_bot_inset_settled_logical: 0.0,
            mux_sidebar_inset_logical: 0.0,
            resize_settler: ResizeSettler::new(),
            mux_was_attached: false,
            last_resize_settle_wake: None,
            current_mods: Modifiers::NONE,
            cursor_pos: PhysicalPosition::new(0.0, 0.0),
            dragging: false,
            click_tracker: ClickTracker::default(),
            clipboard,
            grid_pass: None,
            pending_egui_events: Vec::new(),
            pointer_buttons_down: 0,
            pending_close: false,
            current_resize_dir: None,
            current_cursor: CursorIcon::Default,
            hover: HoverState::default(),
            hover_span_changed: false,
            pointer_in_window: false,
            alt_scroll_accum: 0.0,
            render_perf_enabled: std::env::var("EMTERM_RENDER_PERF")
                .map(|v| v == "1")
                .unwrap_or(false),
            frame_counter: FrameCounter::default(),
            rows_rebuilt_counter: RowsRebuiltCounter::default(),
        }
    }

    /// Phase 4-H: lazily construct the `TerminalGridPass` once the App
    /// is available. Called from `PocApp::resumed` after the App's
    /// font stack has been built (`App::new` already constructs it).
    /// Idempotent — repeated calls keep the existing pass.
    pub fn ensure_grid_pass(&mut self, app: &App) {
        if self.grid_pass.is_some() {
            return;
        }
        let pass = TerminalGridPass::new(
            &self.device,
            self.surface_config.format,
            app.font_cache.clone(),
            app.font_fallback.clone(),
            app.font_rasterizer.clone(),
        );
        self.grid_pass = Some(pass);
    }

    pub fn window(&self) -> &dyn Window {
        self.window.as_ref()
    }

    /// Hand a clone of the `Arc<dyn Window>` to callers that need to retain
    /// the handle themselves (Phase 4-G-3 passes this to
    /// `WinitImeBridge::init` so the bridge can call
    /// `Window::set_ime_cursor_area`).
    pub fn window_arc(&self) -> Arc<dyn Window> {
        self.window.clone()
    }

    /// True when the CSD title bar's `×` was clicked this frame and
    /// `about_to_wait` should drive the teardown handshake.
    pub fn pending_close(&self) -> bool {
        self.pending_close
    }

    /// Translate a [`crate::ui::TitleBarEvent`] into the matching
    /// `winit::Window` action. `Close` is deferred — it just flips
    /// `pending_close` so `about_to_wait` can run the teardown +
    /// `event_loop.exit()` handshake in the same place as the
    /// last-tab-closed path.
    fn apply_title_bar_event(&mut self, evt: crate::ui::TitleBarEvent) {
        use crate::ui::TitleBarEvent;
        match evt {
            TitleBarEvent::Minimize => {
                self.window.set_minimized(true);
            }
            TitleBarEvent::MaximizeToggle => {
                let was_maximized = self.window.is_maximized();
                self.window.set_maximized(!was_maximized);
            }
            TitleBarEvent::Close => {
                self.pending_close = true;
                self.window.request_redraw();
            }
            TitleBarEvent::DragStart => {
                // X11 / Wayland-backed winit hands the move loop to
                // the WM. The Err arm covers headless / unsupported
                // backends — log and continue rather than panic.
                if let Err(e) = self.window.drag_window() {
                    log::warn!("native-poc: drag_window failed: {e}");
                }
            }
        }
    }

    /// Toggle borderless full-screen for the window. When already
    /// full-screen, restore windowed mode (`None`); otherwise enter
    /// `Borderless(None)` so winit picks the window's current monitor.
    /// The resulting `WindowEvent::SurfaceResized` drives the deferred
    /// `apply_pending_resize`, so the grid reshapes on the next frame
    /// without any extra plumbing here.
    fn toggle_fullscreen(&self) {
        use winit::monitor::Fullscreen;
        if self.window.fullscreen().is_some() {
            self.window.set_fullscreen(None);
        } else {
            self.window
                .set_fullscreen(Some(Fullscreen::Borderless(None)));
        }
    }

    /// Classify a pointer position (in egui logical points, relative to
    /// the window's top-left) as one of the eight CSD resize directions,
    /// or `None` when the pointer is in the window interior. The window
    /// being maximized always returns `None` so a click on the title
    /// bar of a maximized window stays a move/drag gesture rather than
    /// a phantom resize against the screen edge.
    fn resize_direction_at(&self, logical_x: f32, logical_y: f32) -> Option<ResizeDirection> {
        if self.window.is_maximized() {
            return None;
        }
        let size = self
            .window
            .surface_size()
            .to_logical::<f32>(self.pixels_per_point as f64);
        let dir = classify_resize_edge(
            size.width,
            size.height,
            logical_x,
            logical_y,
            RESIZE_EDGE_PX,
        )?;
        // CSD title-bar carve-out: the title bar spans y ∈ [0,
        // TITLE_BAR_HEIGHT) and hosts the Minimize / Maximize / Close
        // buttons on its right side plus a drag-to-move affordance in
        // the middle. If we let `North`/`NE`/`NW` fire anywhere inside
        // that band, the top-right 8×8 corner would hijack the Close
        // button and the rest of the bar would lose the move gesture.
        // Restrict the top-edge resize to the outermost `RESIZE_EDGE_PX`
        // strip — past that, the title bar wins and the user gets the
        // expected drag-to-move / button-click semantics.
        let title_bar_h = crate::ui::title_bar::TITLE_BAR_HEIGHT;
        if logical_y >= RESIZE_EDGE_PX && logical_y < title_bar_h {
            use ResizeDirection::*;
            if matches!(dir, North | NorthEast | NorthWest) {
                return None;
            }
        }
        Some(dir)
    }

    /// Refresh the cached resize direction + pointer icon for a new
    /// pointer position. Cheap to call on every `PointerMoved`: skips
    /// the `set_cursor` IPC round-trip when the resulting icon would
    /// match the one already in flight.
    fn update_resize_hint(&mut self, logical_x: f32, logical_y: f32) {
        let dir = self.resize_direction_at(logical_x, logical_y);
        if dir == self.current_resize_dir {
            return;
        }
        self.current_resize_dir = dir;
        let icon = dir.map(CursorIcon::from).unwrap_or(CursorIcon::Default);
        if icon == self.current_cursor {
            return;
        }
        self.current_cursor = icon;
        self.window.set_cursor(icon.into());
    }

    /// Map a physical pixel position to a grid cell, returning `None`
    /// when the pointer is outside the terminal grid area (over the CSD
    /// title bar / tab strip, in the left/top padding, or below/right of
    /// the last cell). Unlike [`pixel_to_cell`] this does *not* clamp to
    /// the grid — a clamped row/col would make the top strip read as
    /// row 0 col 0 and falsely underline a link there. Mirrors the
    /// WebView `LinkHandler`'s `displayRow < 0 || >= rows` guard.
    fn pixel_to_grid_cell(&self, pos: PhysicalPosition<f64>, app: &App) -> Option<(u16, u16)> {
        let (cell_w, cell_h, origin_x, origin_y) = self.cell_metrics_px(app);
        if cell_w <= 0.0 || cell_h <= 0.0 {
            return None;
        }
        if pos.x < origin_x || pos.y < origin_y {
            return None;
        }
        let cols = app.cell_size.cols;
        let rows = app.cell_size.rows;
        if cols == 0 || rows == 0 {
            return None;
        }
        let col = ((pos.x - origin_x) / cell_w).floor() as i64;
        let row = ((pos.y - origin_y) / cell_h).floor() as i64;
        if col < 0 || row < 0 || col >= cols as i64 || row >= rows as i64 {
            return None;
        }
        Some((row as u16, col as u16))
    }

    /// Write `text` to the X11 PRIMARY selection (auto-copy on mouse-up).
    /// No-op when arboard is unavailable.
    fn set_primary(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let cb = match &mut self.clipboard {
            Some(c) => c,
            None => return,
        };
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            use arboard::{LinuxClipboardKind, SetExtLinux};
            if let Err(e) = cb.set().clipboard(LinuxClipboardKind::Primary).text(text) {
                log::warn!("arboard PRIMARY set failed: {e}");
            }
        }
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        {
            // PRIMARY is X11-only; fall through to CLIPBOARD on other
            // platforms (Phase 4 targets Linux; this keeps the type checked).
            if let Err(e) = cb.set_text(text) {
                log::warn!("clipboard set_text fallback failed: {e}");
            }
        }
    }

    /// Write `text` to the CLIPBOARD selection (Ctrl+Shift+C).
    fn set_clipboard(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let cb = match &mut self.clipboard {
            Some(c) => c,
            None => return,
        };
        if let Err(e) = cb.set_text(text) {
            log::warn!("arboard CLIPBOARD set failed: {e}");
        }
    }

    /// Read the CLIPBOARD selection (Ctrl+Shift+V).
    fn get_clipboard(&mut self) -> Option<String> {
        let cb = self.clipboard.as_mut()?;
        match cb.get_text() {
            Ok(s) => Some(s),
            Err(e) => {
                log::warn!("arboard CLIPBOARD get failed: {e}");
                None
            }
        }
    }

    /// Read the PRIMARY selection (middle-click paste).
    fn get_primary(&mut self) -> Option<String> {
        let cb = self.clipboard.as_mut()?;
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            use arboard::{GetExtLinux, LinuxClipboardKind};
            match cb.get().clipboard(LinuxClipboardKind::Primary).text() {
                Ok(s) => Some(s),
                Err(e) => {
                    log::warn!("arboard PRIMARY get failed: {e}");
                    None
                }
            }
        }
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        {
            match cb.get_text() {
                Ok(s) => Some(s),
                Err(e) => {
                    log::warn!("clipboard get_text fallback failed: {e}");
                    None
                }
            }
        }
    }

    /// Feed pasted text to the active tab, wrapping it in bracketed paste
    /// when DECSET 2004 is on.
    fn deliver_paste(&self, app: &App, text: &str) {
        if text.is_empty() {
            return;
        }
        let tab = match app.active_tab() {
            Some(t) => t,
            None => return,
        };
        let bracketed = tab
            .core
            .lock()
            .get_mode(term_core::terminal_core::MODE_BRACKETED_PASTE);
        // mux-aware: in mux mode the paste is wrapped as a PtyInput frame
        // (the bridge drops raw stdin); otherwise it is a plain bracketed
        // PTY write.
        tab.write_paste_input(text, bracketed);
    }

    /// Reload `settings.json` and apply it to the running app. Called
    /// from `about_to_wait` when the child settings window reports a
    /// persisted save (see [`crate::settings_launcher`]); the child
    /// already validated and wrote the file, so the parent only loads
    /// and applies.
    fn reload_settings_from_disk(&mut self, app: &mut App) {
        let new = crate::settings::Settings::load_or_default();
        let old_ui_font = app.settings.ui_font_family.clone();
        let old_terminal_font = terminal_font_family(&app.settings).to_string();
        let needs_resize = app.apply_settings(new);
        // File-log recording is owned by the logging module, not App.
        crate::logging::set_recording_enabled(app.settings.log_recording_enabled);
        // The egui chrome font chain is owned by the host context, not
        // the App; rebuild it when either the UI font (Proportional
        // head) or the terminal font (Monospace head; drives status-bar
        // glyph shape) changed.
        if app.settings.ui_font_family != old_ui_font
            || terminal_font_family(&app.settings) != old_terminal_font
        {
            configure_egui_fonts(
                &self.egui_ctx,
                &app.settings.ui_font_family,
                terminal_font_family(&app.settings),
            );
        }
        if needs_resize {
            // Cell metrics / padding changed: reshape the PTY grid for
            // the unchanged window pixel size on the next frame.
            self.request_resize();
        }
        app.mark_full_redraw();
        self.window.request_redraw();
        crate::wakeup::wake();
    }
}

/// Run the event loop until the window is closed. Owns the App.
pub fn run(event_loop: EventLoop, app: App) -> ! {
    let handler = PocApp { app, host: None };
    // winit 0.31's `run_app` takes the handler by value (no more
    // `&mut app`) and explicitly drops it before returning, so the
    // PTY-owning tabs' reader/writer threads shut down cleanly here —
    // no separate `drop(handler)` needed after this call.
    if let Err(e) = event_loop.run_app(handler) {
        log::error!("native-poc: winit event loop returned an error: {e}");
    }
    std::process::exit(0);
}

#[cfg(test)]
mod tests;
