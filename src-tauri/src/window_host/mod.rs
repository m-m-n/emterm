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
use winit::application::ApplicationHandler;
use winit::cursor::CursorIcon;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};
use winit::window::{ResizeDirection, Window, WindowAttributes, WindowId};

use crate::app::App;
use crate::ime::backend::{KeyDispatchResult, ProcessEnv, RawKeyEvent, build_backend_with_window};
use crate::pty::input::{Modifiers, Target as EncodeTarget};
#[cfg(test)]
use crate::settings::ShiftEnterBehavior;

mod frame_pacing;
mod input_translate;
mod key_routing;
mod link_hover;
mod render_surface;
mod resize_layout;

#[cfg(test)]
use resize_layout::resolve_grid_bot_inset;

use key_routing::{
    egui_to_mux_input, handle_mux_dialog_key, handle_profile_selector_key, handle_search_key,
    handle_special_chord,
};

use link_hover::{ClickTracker, HoverState};
#[cfg(test)]
use link_hover::{detect_osc8_link_at, hover_link_cells_changed};

use frame_pacing::{
    FrameCounter, ResizeSettler, RowsRebuiltCounter, control_flow_for,
    next_resize_settle_wake_deadline, resize_settle_self_wake_due, toast_redraw_due,
};
#[cfg(test)]
use frame_pacing::{
    RESIZE_SETTLE_MAX_DURATION, RESIZE_SETTLE_QUIET_DURATION, RESIZE_SETTLE_SELF_WAKE_INTERVAL,
    has_actionable_egui_input, next_wait_deadline, preedit_effective_dirty_rows,
    record_drawn_frame, record_rebuilt_rows, resolve_build_dirty_rows,
    should_rotate_row_cache_for_scroll_event, should_skip_frame, status_bar_insets_changed,
};

#[cfg(test)]
use input_translate::MAX_ALT_SCROLL_NOTCHES;
use input_translate::{
    ShiftEnterRewrite, accumulate_alt_scroll_lines, alternate_scroll_wheel_bytes,
    is_skk_swallowed_chord, shift_enter_rewrite, should_drop_synthetic_key_event,
    winit_key_to_bytes, winit_key_to_egui, winit_physical_key_code, winit_to_egui_button,
};

use crate::render::terminal_grid_pass::TerminalGridPass;
use crate::selection::{Pos, Selection, SelectionMode};

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

/// `ApplicationHandler` impl driving the App + WindowHost on winit 0.31.
///
/// `can_create_surfaces` creates the window the first time the platform
/// is ready to accept a render surface (the only lifecycle hook winit
/// 0.31 guarantees on desktop platforms — `resumed`/`suspended` are now
/// iOS/Web/Android-only), `window_event` mirrors what used to be the
/// inner `match event` arm, and `about_to_wait` does the periodic pump
/// (PTY drain, IME pump, cursor-rect notification) that the old
/// `StartCause::Poll` path handled.
pub(super) struct PocApp {
    pub(super) app: App,
    pub(super) host: Option<WindowHost>,
}

impl ApplicationHandler for PocApp {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.host.is_some() {
            // Back-to-back `can_create_surfaces()` calls are expected
            // per the trait's portability contract; keep the existing
            // host so a stray re-entry does not reinitialize the
            // surface (the PoC has no Android target).
            return;
        }
        let mut host = WindowHost::new(
            event_loop,
            &self.app.settings.ui_font_family,
            terminal_font_family(&self.app.settings),
        );

        // Phase 4-H: construct the TerminalGridPass against the wgpu
        // device now that the surface exists. The App owns the font
        // stack; the pass borrows clones of each `Arc`.
        host.ensure_grid_pass(&self.app);

        // Push the initial grid size into the App before the first tab spawn.
        let (cols, rows) = host.grid_size(&self.app);
        self.app.cell_size = crate::app::GridDims { cols, rows };
        self.app.spawn_initial_tab();

        // Phase 4-G-3: resolve the IME backend now that the winit
        // window exists. The factory consults `EMTERM_NATIVE_IME` and
        // `settings.ime.native_integration`, then either installs a
        // `WinitImeBridge` (real backend) or falls back to
        // `NullBackend` on init failure.
        let backend =
            build_backend_with_window(host.window_arc(), &self.app.settings.ime, &ProcessEnv);
        self.app.set_ime_backend(backend);

        host.window().request_redraw();
        // task0004 D4: the initial control flow follows the same
        // pending-timed-work rule `about_to_wait` uses below, rather than
        // unconditionally rearming a 16 ms `WaitUntil`. `ResizeSettler::new`
        // opens its settling window immediately, so this also arms the
        // resize-settle self-wake deadline from construction (mux-tab-
        // switch-bypass-refix task0002) — both places computing
        // `ControlFlow` agree on the same rule.
        event_loop.set_control_flow(control_flow_for(
            &self.app,
            next_resize_settle_wake_deadline(
                host.resize_settler.awaiting_decision(),
                Instant::now(),
            ),
        ));
        self.host = Some(host);
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        let Some(host) = self.host.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => {
                // winit's `EventLoop::run_app` returns control to the
                // caller, but PTY-owning tabs would otherwise be dropped
                // on the unwind. Tear them down explicitly so the kill
                // + reader/writer thread join from `PtySession::Drop`
                // happens before the WM destroys the window.
                log::info!("native-poc: CloseRequested → shutting down PTY tabs");
                // FR5 / NFR4: signal cancel on every in-flight 2nd-pass
                // scrollback restore worker BEFORE dropping the tabs.
                // Dropping the receiver does not fire the worker's cancel
                // flag (the worker holds an `Arc<AtomicBool>` independently
                // of the channel), so an explicit cancel store bounds
                // wasted worker CPU on shutdown. Best-effort: no join.
                for tab in self.app.tabs.iter() {
                    tab.cancel_pending_scrollback_restore();
                }
                self.app.tabs.clear();
                // Drop the wgpu Surface (and the rest of WindowHost) while
                // winit's EventLoop is still alive. The Vulkan WSI surface
                // is tied to the X11 display connection that EventLoop
                // owns; if we let WindowHost outlive the EventLoop, the
                // surface destructor calls into a freed display and
                // segfaults. Same reason applies to the egui-wgpu
                // Renderer and the Window arc.
                self.host = None;
                event_loop.exit();
            }
            WindowEvent::SurfaceResized(new_size) => {
                // Alacritty-style deferral: do not call `surface.configure()`
                // or resize the PTY here. Both run together at the head of
                // the next `render()` so a burst of compositor resize events
                // collapses to one configure + one PTY ioctl per frame.
                // Zero-size events (Windows minimize) are silently ignored
                // by `apply_pending_resize`.
                if new_size.width == 0 || new_size.height == 0 {
                    return;
                }
                host.request_resize();
                host.window().request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                // pixels_per_point is consumed by `cell_metrics_px` (IME
                // spot, hit-test) which can run from `about_to_wait` between
                // events, so update it immediately. The expensive surface
                // configure + PTY grid resize stays deferred to render time
                // for the same reasons as `Resized`.
                host.pixels_per_point = scale_factor as f32;
                host.request_resize();
                host.window().request_redraw();
            }
            WindowEvent::ModifiersChanged(state) => {
                let s: ModifiersState = state.state();
                host.current_mods = Modifiers {
                    ctrl: s.contains(ModifiersState::CONTROL),
                    shift: s.contains(ModifiersState::SHIFT),
                    alt: s.contains(ModifiersState::ALT),
                };
                // Pressing / releasing Ctrl toggles the hand cursor over a
                // hovered link without any pointer motion. The cell hasn't
                // moved, so detection is reused — only the icon updates.
                host.update_link_cursor();
            }
            // Phase 4-G-3: winit surfaces composition events via
            // `WindowEvent::Ime { Enabled, Preedit, Commit, Disabled }`.
            // Route them to the active backend; `WinitImeBridge`
            // translates each variant into `ImeEvent`s consumed by
            // `App::pump_ime` on the next tick. `NullBackend`
            // overrides the trait default with a no-op, so this is
            // safe to call unconditionally.
            WindowEvent::Ime(ime) => {
                // While the profile-selector modal owns the keyboard,
                // swallow IME events entirely — the modal has no text
                // field, and a commit must not leak into the PTY.
                if self.app.profile_selector.visible {
                    host.window().request_redraw();
                    return;
                }
                // While the search bar owns the keyboard, route IME
                // commits into egui's TextEdit instead of the terminal
                // IME backend so Japanese / CJK input lands in the
                // focused field. Only `Commit` carries text we forward;
                // preedit display in the field is omitted (best-effort
                // CJK support per spec).
                if self.app.search_visible() {
                    if let winit::event::Ime::Commit(text) = &ime {
                        if !text.is_empty() {
                            host.pending_egui_events
                                .push(egui::Event::Text(text.clone()));
                        }
                    }
                    host.window().request_redraw();
                    return;
                }
                // Same capture for an open mux dialog: route CJK commits into
                // the dialog's TextEdit, never the terminal IME backend.
                if self.app.mux_dialog_open() {
                    if let winit::event::Ime::Commit(text) = &ime {
                        if !text.is_empty() {
                            host.pending_egui_events
                                .push(egui::Event::Text(text.clone()));
                        }
                    }
                    host.window().request_redraw();
                    return;
                }
                self.app.pass_winit_ime(&ime);
                host.window().request_redraw();
            }
            // Focus loss / window deactivation → clear any in-progress
            // preedit overlay so a stale composition doesn't ghost the
            // cursor after the user tabs away. Also forward focus
            // state to the IME backend so it can disable/enable IME on
            // the IM-server side.
            WindowEvent::Focused(focused) => {
                self.app.window_focused = focused;
                self.app.notify_ime_focus(focused);
                if !focused {
                    self.app.on_ime_focus_lost();
                    // ModifiersChanged is not guaranteed to fire while
                    // the window is unfocused, so a Ctrl held across an
                    // Alt+Tab would stay latched and arm the link hand
                    // cursor / Ctrl+click-open on return. Drop all
                    // modifiers on focus loss; the next real
                    // ModifiersChanged re-seeds them.
                    host.current_mods = Modifiers::default();
                    // Same staleness class for the held-button count: the
                    // matching Released may never arrive once focus is
                    // gone, and a latched count would keep treating every
                    // hover motion as an actionable drag forever.
                    host.pointer_buttons_down = 0;
                    host.update_link_cursor();
                } else {
                    // Drop the user back into the cursor's "on" half-
                    // cycle on focus regain so the filled block appears
                    // immediately instead of waiting up to 530 ms for
                    // the next blink boundary.
                    self.app.reset_blink_phase();
                }
                // Cursor shape switches between filled (focused) and
                // outline (unfocused), so we need a repaint on every
                // focus transition. The overlay cursor's filled/hollow
                // state depends on `window_focused`, which the dirty-row
                // tracking never sees, so a plain request_redraw() would
                // be skipped by should_skip_frame; force a full redraw.
                self.app.mark_full_redraw();
                host.window().request_redraw();
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } if event.state == ElementState::Pressed => {
                // Synthetic key press gate (task0002): drop X11 FocusIn
                // replay presses before any state mutation, keybinding
                // dispatch, IME forwarding, or PTY write. See
                // `should_drop_synthetic_key_event`.
                if should_drop_synthetic_key_event(is_synthetic) {
                    log::warn!(
                        "native-poc: dropping synthetic key press: physical_key={:?}",
                        event.physical_key
                    );
                    return;
                }
                // Search overlay capture: while the search bar is visible
                // it owns the keyboard. Navigation / close chords are
                // handled here directly; copy / paste are translated to
                // egui clipboard events; everything else is forwarded to
                // egui's TextEdit (bypassing the terminal IME dispatch and
                // the PTY encoder entirely). Returns early so the normal
                // Phase 4 key path below never runs while searching.
                // Profile-selector capture: the modal owns the keyboard
                // entirely (navigation / confirm / cancel); nothing
                // reaches the search overlay, the IME, or the PTY.
                if self.app.profile_selector.visible {
                    handle_profile_selector_key(&event, &mut self.app);
                    host.window().request_redraw();
                    return;
                }

                if self.app.search_visible() {
                    handle_search_key(&event, host.current_mods, host, &mut self.app);
                    host.window().request_redraw();
                    return;
                }

                // While a mux rename / move dialog owns the keyboard, forward
                // keys into egui (its TextEdit / DragValue / Enter / Escape)
                // and return early so the chord never reaches the terminal
                // IME, the keybind dispatcher, or the PTY encoder.
                if self.app.mux_dialog_open() {
                    handle_mux_dialog_key(&event, host.current_mods, host);
                    host.window().request_redraw();
                    return;
                }

                // Phase 4-G: offer the raw key event to the IME backend
                // first. `Consumed` means the IM server swallowed the
                // key (composition open, candidate chosen) and we must
                // skip both the keybinds dispatcher and the generic
                // encoder; the resulting `ImeEvent::Commit` / `Preedit`
                // will arrive via `pump_ime` on the next tick.
                // `Passthrough` lets the existing Phase 4 path run
                // unchanged.
                let raw_key = RawKeyEvent {
                    physical_key_code: winit_physical_key_code(&event),
                    state_pressed: true,
                    mods: host.current_mods,
                };
                if matches!(
                    self.app.dispatch_key_event_via_ime(&raw_key),
                    KeyDispatchResult::Consumed
                ) {
                    host.window().request_redraw();
                    return;
                }

                // Translate the logical key once; the result is shared by
                // `handle_special_chord` (clipboard chords) and the
                // settings-driven keybinds dispatch that follows, avoiding
                // a second translation on every keystroke.
                //
                // Special chords intercept the generic encoder path. The
                // clipboard chords are settings-driven (`keybinds.copy` /
                // `keybinds.paste`, defaults shown); the scrollback chords
                // are fixed native-poc conventions:
                //   keybinds.copy  (default Ctrl+Shift+C) → copy selection to CLIPBOARD
                //   keybinds.paste (default Ctrl+Shift+V) → paste CLIPBOARD into PTY (bracketed if 2004)
                //   Shift+PageUp   → scroll back one page
                //   Shift+PageDown → scroll forward one page
                //   Shift+Home     → scroll to top of scrollback
                //   Shift+End      → scroll back to live tail
                let egui_key = winit_key_to_egui(&event.logical_key);
                let handled =
                    handle_special_chord(&event, host.current_mods, egui_key, host, &mut self.app);
                // mux prefix latch: intercept keys for the active mux tab
                // ahead of the keybind dispatch / PTY passthrough. Only fires
                // when the active tab is mux-attached.
                let mut mux_consumed = false;
                if !handled {
                    if let Some(k) = egui_key {
                        // Convert the framework-native (egui::Modifiers, egui::Key)
                        // into the framework-agnostic mux::prefix::KeyInput right
                        // here at the UI boundary so the domain layer never sees
                        // egui types (gpt-architecture #4). `command` is folded
                        // into `ctrl` because egui aliases Cmd to Ctrl on non-mac.
                        let input = egui_to_mux_input(host.current_mods, k);
                        let (consumed, outcome) =
                            self.app.observe_mux_key(&input, std::time::Instant::now());
                        mux_consumed = consumed;
                        self.app.handle_mux_outcome(outcome);
                        if consumed {
                            self.app.mark_full_redraw();
                        }
                    }
                }
                if !handled && !mux_consumed {
                    // Settings-driven global keybinds (tab roster) take
                    // priority over the generic PTY encoder. The chord
                    // table comes from `settings.keybinds` (resolved into
                    // `App::keybinds` at startup).
                    let egui_mods = egui::Modifiers {
                        ctrl: host.current_mods.ctrl,
                        shift: host.current_mods.shift,
                        alt: host.current_mods.alt,
                        command: false,
                        mac_cmd: false,
                    };
                    let action = egui_key.and_then(|k| {
                        crate::ui::keybinds::dispatch(&self.app.keybinds, egui_mods, k)
                    });
                    if let Some(act) = action {
                        // View-level actions that need the window handle
                        // or the deferred-resize machinery are applied
                        // against `host` here; everything else routes
                        // through `App::apply_action`.
                        match act {
                            crate::ui::AppAction::ToggleFullscreen => {
                                host.toggle_fullscreen();
                                self.app.mark_full_redraw();
                            }
                            crate::ui::AppAction::ZoomIn => {
                                if self.app.zoom_in() {
                                    host.request_resize();
                                    self.app.mark_full_redraw();
                                }
                            }
                            crate::ui::AppAction::ZoomOut => {
                                if self.app.zoom_out() {
                                    host.request_resize();
                                    self.app.mark_full_redraw();
                                }
                            }
                            crate::ui::AppAction::ZoomReset => {
                                if self.app.zoom_reset() {
                                    host.request_resize();
                                    self.app.mark_full_redraw();
                                }
                            }
                            crate::ui::AppAction::OpenSearch => {
                                // Open (or re-focus) the search overlay. The
                                // overlay then captures keystrokes via the
                                // `search_visible()` branch at the top of the
                                // KeyboardInput handler on subsequent presses.
                                self.app.open_search();
                            }
                            crate::ui::AppAction::ToggleTabBar => {
                                self.app.show_tab_bar = !self.app.show_tab_bar;
                                // The tab strip's row count changed, so the
                                // grid origin / available rows shift: defer
                                // a resize so the PTY is reshaped before the
                                // next frame paints.
                                host.request_resize();
                                self.app.mark_full_redraw();
                            }
                            other => {
                                let _ = self.app.apply_action(other);
                                self.app.mark_full_redraw();
                                // Tab-switch actions (NextTab/PrevTab/JumpTab/
                                // NewTab/CloseTab) change the active grid; drop
                                // the hover so the stale underline / hand cursor
                                // from the old tab doesn't bleed into the new one.
                                host.invalidate_link_hover();
                            }
                        }
                    } else if self.app.settings.skk_mode
                        && is_skk_swallowed_chord(&event.logical_key, host.current_mods)
                    {
                        // `skk_mode` (default on): swallow bare Ctrl+J so
                        // SKK-style IMEs keep their mode-switch chord (see
                        // `is_skk_swallowed_chord`).
                    } else {
                        // `shift_enter_behavior`: three-way rewrite decision
                        // (task0001 design D1) for the bare Shift+Enter
                        // chord. Only the bare Shift-on-Enter case is
                        // rewritten — Ctrl/Alt already pass through
                        // unchanged (see `shift_enter_rewrite`).
                        let is_enter =
                            matches!(event.logical_key, WinitKey::Named(NamedKey::Enter));
                        let rewrite = shift_enter_rewrite(
                            is_enter,
                            host.current_mods,
                            self.app.settings.shift_enter_behavior,
                        );
                        // FR2 (key-resume): capture whether the key was
                        // forwarded to the PTY into a local flag. The
                        // `active_tab()` borrow holds `&self.app`, so we
                        // cannot call the `&mut self`-taking
                        // `scroll_to_live` until after the block ends.
                        let forwarded = if let Some(tab) = self.app.active_tab() {
                            // In mux mode the bytes will be wrapped as a
                            // `PtyInput` frame and reach a remote (canonically
                            // Linux) daemon, so we must skip the Windows-host
                            // Win32 Input Mode shim or the remote shell sees
                            // unknown CSI for Backspace / Escape / Ctrl+[.
                            let target = if tab.mux_session_name.is_some() {
                                EncodeTarget::PosixPty
                            } else {
                                EncodeTarget::HostPty
                            };
                            if let ShiftEnterRewrite::RawBytes(bytes) = rewrite {
                                // `kitty_csi_u`: bypass the key encoder
                                // entirely and write the literal CSI u
                                // sequence through the same output path as
                                // encoder-produced bytes (host-PTY raw
                                // write / mux PtyInput frame), per D1 —
                                // the encoder cannot express CSI u.
                                tab.write_input(bytes.to_vec());
                                true
                            } else {
                                let mods = match rewrite {
                                    ShiftEnterRewrite::Modifiers(m) => m,
                                    _ => host.current_mods,
                                };
                                if let Some(bytes) = winit_key_to_bytes(&event, mods, target) {
                                    // mux-aware: wraps as PtyInput in mux mode so the
                                    // bridge forwards it (raw stdin is dropped there).
                                    tab.write_input(bytes);
                                    true
                                } else {
                                    false
                                }
                            }
                        } else {
                            false
                        };
                        if forwarded {
                            // FR2: any key we forward to the PTY also snaps
                            // the viewport back to live tail. Bare modifiers
                            // return `None` (so `forwarded == false`); search
                            // overlay / profile selector / mux dialog / IME
                            // consume / special chord / mux prefix latch /
                            // settings keybinds all early-return before
                            // reaching here, so they never snap.
                            self.app.scroll_to_live();
                        }
                    }
                }
                host.window().request_redraw();
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } if event.state == ElementState::Released => {
                // Synthetic key press gate (task0002): a synthetic
                // release is dropped by the same rule as a synthetic
                // press (see `should_drop_synthetic_key_event`).
                if should_drop_synthetic_key_event(is_synthetic) {
                    log::warn!(
                        "native-poc: dropping synthetic key release: physical_key={:?}",
                        event.physical_key
                    );
                    return;
                }
                // Phase 4-G-3: forward releases too so the
                // WinitImeBridge can observe Ghostty-style
                // modifier-only release events (fcitx5 toggles on bare
                // modifier release). The Phase 4-G-1 NullBackend
                // ignores releases.
                let raw_key = RawKeyEvent {
                    physical_key_code: winit_physical_key_code(&event),
                    state_pressed: false,
                    mods: host.current_mods,
                };
                let _ = self.app.dispatch_key_event_via_ime(&raw_key);
            }
            WindowEvent::PointerLeft { .. } => {
                // Mark the pointer as outside the window so PTY-output
                // re-detection in `about_to_wait` is suppressed — there
                // is nothing to underline when no pointer is inside.
                host.pointer_in_window = false;
                // Reset the resize hint when the pointer leaves the
                // window so the cached direction doesn't outlive its
                // hit zone — without this, re-entering the interior
                // through a non-edge route keeps the last edge's
                // cursor + direction stuck (since `update_resize_hint`
                // short-circuits when the new dir matches the cached
                // one).
                if host.current_resize_dir.is_some() || host.current_cursor != CursorIcon::Default {
                    host.current_resize_dir = None;
                    host.current_cursor = CursorIcon::Default;
                    host.window.set_cursor(CursorIcon::Default.into());
                }
                // Drop any link-hover underline + hand cursor when the
                // pointer leaves the window.
                host.invalidate_link_hover();
                // task0002 FR3: the pointer can't be inside the overlay
                // card if it isn't even inside the window.
                self.app.set_mux_sidebar_hovered(false);
            }
            WindowEvent::PointerMoved { position, .. } => {
                host.pointer_in_window = true;
                host.cursor_pos = position;
                // Forward to egui so the tab bar / status bar widgets
                // observe hover + drag motion.
                let logical = position.to_logical::<f32>(host.pixels_per_point as f64);
                let egui_pos = egui::pos2(logical.x, logical.y);
                // Coalesce consecutive `PointerMoved`s: motion-only frames
                // are skippable (see `has_actionable_egui_input`), so
                // without coalescing a sustained motion burst with no
                // drawn frame in between (cursor_blink=false, or an
                // unfocused window — nothing else forces a drain) would
                // grow this queue one entry per motion event and rescan
                // it per event. Only the latest position matters to egui.
                if let Some(egui::Event::PointerMoved(last)) = host.pending_egui_events.last_mut() {
                    *last = egui_pos;
                } else {
                    host.pending_egui_events
                        .push(egui::Event::PointerMoved(egui_pos));
                }
                // task0002 FR3 / D5 "Hover feed": maintain the overlay
                // card's hover flag with the SAME hit test the press/wheel
                // routing below already query
                // (`ui::mux_sidebar::point_in_sidebar`, evaluated against
                // the `Overlay` placement only — the persistent panel and
                // hidden state never dim, and `point_in_sidebar` already
                // answers `false` for a `None` placement). Sharing the
                // derivation means hover and click can never disagree
                // about the boundary (IMPLEMENTATION.md cross-task
                // decision 3.5).
                {
                    let overlay_visible = matches!(
                        self.app.mux_sidebar_visibility(),
                        crate::app::MuxSidebarVisibility::Overlay
                    );
                    let placement =
                        overlay_visible.then_some(crate::ui::mux_sidebar::Placement::Overlay);
                    let window_size_logical = host
                        .window
                        .surface_size()
                        .to_logical::<f32>(host.pixels_per_point as f64);
                    let top_chrome =
                        crate::ui::mux_sidebar::top_chrome_inset(self.app.show_tab_bar);
                    let in_overlay = crate::ui::mux_sidebar::point_in_sidebar(
                        egui_pos,
                        placement,
                        egui::vec2(window_size_logical.width, window_size_logical.height),
                        top_chrome,
                        host.status_bar_bot_inset_logical,
                    );
                    self.app.set_mux_sidebar_hovered(in_overlay);
                }
                // CSD edge-resize hot zone: refresh the cached
                // ResizeDirection + pointer icon so the next left-press
                // can hand the matching direction to
                // `Window::drag_resize_window`. Skipped while a
                // terminal selection drag is in flight — the pointer
                // can pass through an edge band on its way to the
                // selection target, and swapping to a resize icon
                // mid-drag would be jarring.
                if !host.dragging {
                    host.update_resize_hint(logical.x, logical.y);
                    // Link hover: skipped while selection-dragging so a
                    // drag through a link doesn't flip to a hand cursor /
                    // underline mid-selection.
                    host.refresh_link_hover(&self.app);
                }
                host.window().request_redraw();
                if host.dragging {
                    let (screen_row, col) = host.pixel_to_cell(position, &self.app);
                    // Convert the screen row to its absolute buffer row so the
                    // extended endpoint stays pinned to the content as the
                    // viewport scrolls.
                    let abs_row = host.screen_row_to_abs(screen_row, &self.app);
                    // First motion since the press in Character mode
                    // upgrades the pending click into a real Selection.
                    // Word / line selections (double / triple click)
                    // were already committed at press time and the
                    // pending anchor was cleared there.
                    if self.app.selection.is_none() {
                        if let Some(anchor) = self.app.pending_selection_anchor.take() {
                            self.app.selection =
                                Some(Selection::new_with_mode(anchor, SelectionMode::Character));
                        }
                    }
                    if let Some(sel) = self.app.selection.as_mut() {
                        if let Some(tab) = self.app.tabs.get(self.app.active) {
                            let core = tab.core.lock();
                            sel.extend(Pos { row: abs_row, col }, &core);
                        }
                    }
                }
            }
            WindowEvent::PointerButton { state, button, .. } => {
                // winit 0.31's pointer-event overhaul folds mouse/touch/
                // pen buttons into `ButtonSource`; normalize to the plain
                // `MouseButton` this handler already speaks. Non-mouse
                // sources with no natural `MouseButton` mapping are
                // ignored (touch already normalizes to `Left` inside
                // `mouse_button()`).
                let Some(button) = button.mouse_button() else {
                    return;
                };
                // CSD edge-resize: a left press on the edge hot zone
                // hands off to the WM via `drag_resize_window`. Run
                // before the egui forward so the tab bar / title bar
                // never see a phantom click on the corner pixel they
                // happen to overlap with the resize gutter, and skip
                // the rest of this handler so no terminal selection
                // gets started under the cursor.
                if button == MouseButton::Left && state == ElementState::Pressed {
                    if let Some(dir) = host.current_resize_dir {
                        if let Err(e) = host.window.drag_resize_window(dir) {
                            log::warn!("native-poc: drag_resize_window failed: {e}");
                        }
                        return;
                    }
                }
                // Forward to egui first so the tab bar / status bar can
                // see the click before we decide whether to start a
                // terminal selection.
                let logical = host
                    .cursor_pos
                    .to_logical::<f32>(host.pixels_per_point as f64);
                let egui_pos = egui::pos2(logical.x, logical.y);
                if let Some(eb) = winit_to_egui_button(button) {
                    host.pending_egui_events.push(egui::Event::PointerButton {
                        pos: egui_pos,
                        button: eb,
                        pressed: matches!(state, ElementState::Pressed),
                        modifiers: egui::Modifiers::default(),
                    });
                }
                // Held-button bookkeeping for the frame-skip veto: while
                // an egui-mapped button is down, `PointerMoved` counts as
                // actionable so egui chrome drags keep their live tracking
                // (see `has_actionable_egui_input`). Only buttons egui can
                // observe are counted — a held side button can't drive a
                // chrome drag, so it must not defeat the idle skip during
                // motion. Saturating on both edges — a stray release (e.g.
                // after focus loss reset the count) must not underflow.
                if winit_to_egui_button(button).is_some() {
                    match state {
                        ElementState::Pressed => {
                            host.pointer_buttons_down = host.pointer_buttons_down.saturating_add(1);
                        }
                        ElementState::Released => {
                            host.pointer_buttons_down = host.pointer_buttons_down.saturating_sub(1);
                        }
                    }
                }
                host.window().request_redraw();

                // Clicks that land on the egui-owned strip (CSD title
                // bar + tab bar at the top, status bar at the bottom
                // when enabled) must not also kick off a terminal
                // selection — otherwise pressing the × on a tab (or
                // the close button on the title bar) would
                // simultaneously start a selection on the cell behind
                // it.
                let top_strip_h = crate::ui::title_bar::TITLE_BAR_HEIGHT
                    + crate::ui::tab_bar::effective_tab_bar_height(self.app.show_tab_bar);
                let if_in_egui_strip = egui_pos.y < top_strip_h;
                if if_in_egui_strip {
                    return;
                }
                // Same rule for the bottom status-bar panel and the
                // right-edge scrollbar overlay: a press on either
                // would otherwise drag-select the terminal row that
                // happens to sit under the bar. Gated to the Pressed
                // edge only so a drag that *started* inside the
                // terminal still gets its Released event processed
                // (clears `host.dragging`, commits selection) when the
                // user happens to lift the button over the strip.
                if button == MouseButton::Left && state == ElementState::Pressed {
                    let window_size_logical = host
                        .window
                        .surface_size()
                        .to_logical::<f32>(host.pixels_per_point as f64);
                    let bottom_strip_top =
                        window_size_logical.height - host.status_bar_bot_inset_logical;
                    let in_bottom_strip =
                        host.status_bar_bot_inset_logical > 0.0 && egui_pos.y >= bottom_strip_top;
                    let scrollbar_visible = self
                        .app
                        .active_tab()
                        .map(|tab| {
                            let core = tab.core.lock();
                            crate::ui::scrollbar::ScrollbarView {
                                mode: self.app.settings.show_scrollbar,
                                scrollback_len: core.get_scrollback_length(),
                                viewport_rows: core.rows() as u32,
                                scroll_offset: self.app.scroll_offset(),
                                alt_screen: self.app.alt_screen,
                            }
                            .visible()
                        })
                        .unwrap_or(false);
                    let central_right = window_size_logical.width - host.mux_sidebar_inset_logical;
                    let in_scrollbar = scrollbar_visible
                        && egui_pos.x >= central_right - crate::ui::scrollbar::TRACK_W
                        && egui_pos.x < central_right;
                    // AC-1/AC-4: query the SAME shared hit-region helper the
                    // MouseWheel guard below uses (IMPLEMENTATION.md cross-
                    // task decision 3.5), instead of the persistent-only
                    // width test above — that test's inset is 0 for the
                    // overlay placement, so a press on the floating overlay
                    // card used to fall through this guard and start a
                    // terminal selection on the cell underneath it.
                    let visible_placement = match self.app.mux_sidebar_visibility() {
                        crate::app::MuxSidebarVisibility::Hidden => None,
                        crate::app::MuxSidebarVisibility::Persistent => {
                            Some(crate::ui::mux_sidebar::Placement::Persistent)
                        }
                        crate::app::MuxSidebarVisibility::Overlay => {
                            Some(crate::ui::mux_sidebar::Placement::Overlay)
                        }
                    };
                    let top_chrome =
                        crate::ui::mux_sidebar::top_chrome_inset(self.app.show_tab_bar);
                    let in_sidebar = crate::ui::mux_sidebar::point_in_sidebar(
                        egui_pos,
                        visible_placement,
                        egui::vec2(window_size_logical.width, window_size_logical.height),
                        top_chrome,
                        host.status_bar_bot_inset_logical,
                    );
                    if in_bottom_strip || in_scrollbar || in_sidebar {
                        return;
                    }
                }

                // While the profile-selector modal is up, every click
                // belongs to egui (a row, or the scrim which dismisses);
                // never start a terminal selection underneath it.
                if self.app.profile_selector.visible {
                    return;
                }

                match (button, state) {
                    (MouseButton::Left, ElementState::Pressed) => {
                        // Ctrl+click opens a hovered URL / file path and
                        // skips starting a selection. Reuses the cached
                        // hover detection for the cell under the pointer
                        // (refreshed on the PointerMoved that brought us
                        // here), re-detecting only if the cached cell no
                        // longer matches the click cell.
                        if host.current_mods.ctrl && host.try_open_link_at_pointer(&self.app) {
                            return;
                        }
                        let (screen_row, col) = host.pixel_to_cell(host.cursor_pos, &self.app);
                        // Anchor the press at its absolute buffer row so the
                        // selection (and double / triple-click classification)
                        // tracks the content across scrolls.
                        let abs_row = host.screen_row_to_abs(screen_row, &self.app);
                        let cls = host.click_tracker.classify(Instant::now(), abs_row, col);
                        if cls.mode == SelectionMode::Character {
                            // Single click in character mode: do not
                            // materialize a one-cell selection yet — the
                            // user may just be moving the cursor / focus
                            // / clearing a prior selection. Record the
                            // press cell so the first motion (if any)
                            // can upgrade this into a real drag-select.
                            self.app.selection = None;
                            self.app.pending_selection_anchor = Some(Pos { row: abs_row, col });
                            host.window().request_redraw();
                        } else {
                            // Word (double click) / line (triple click)
                            // commit immediately so a static click still
                            // selects the targeted word or line.
                            let mut sel =
                                Selection::new_with_mode(Pos { row: abs_row, col }, cls.mode);
                            if let Some(tab) = self.app.tabs.get(self.app.active) {
                                let core = tab.core.lock();
                                sel.extend(Pos { row: abs_row, col }, &core);
                            }
                            self.app.selection = Some(sel);
                            self.app.pending_selection_anchor = None;
                        }
                        host.dragging = true;
                    }
                    (MouseButton::Left, ElementState::Released) => {
                        host.dragging = false;
                        // A press with no motion in Character mode left
                        // selection == None (see the Pressed branch);
                        // there is nothing to copy in that case. `pending`
                        // is `Some` exactly for that case: a single (not
                        // word/line) press whose motion never upgraded it to
                        // a drag-select. Capture it before the reset so the
                        // fold-click path below can detect a plain click.
                        let pending = self.app.pending_selection_anchor.take();
                        // Plain left-click (no Ctrl; meta does not exist on
                        // Linux/Windows), no active selection, no drag: this
                        // is a candidate for a fold toggle. Mirrors the
                        // WebView `input-wiring.ts` routing (Ctrl/Meta →
                        // URL, else → handleFoldClick) plus
                        // `handleFoldClick`'s own "no text selection" guard.
                        // `handle_fold_click` is a no-op (returns false)
                        // when the click is not over a foldable region, so
                        // ordinary clicks-to-deselect fall through unchanged.
                        if pending.is_some()
                            && self.app.selection.is_none()
                            && !host.current_mods.ctrl
                        {
                            if let Some((row, _col)) =
                                host.pixel_to_grid_cell(host.cursor_pos, &self.app)
                            {
                                if self.app.handle_fold_click(row) {
                                    host.invalidate_link_hover();
                                    host.window().request_redraw();
                                    return;
                                }
                            }
                        }
                        if let Some(sel) = self.app.selection {
                            if let Some(tab) = self.app.tabs.get(self.app.active) {
                                let core = tab.core.lock();
                                let text = sel.resolve(&core, self.app.fold_layout());
                                drop(core);
                                host.set_primary(&text);
                                // `copy_on_select` opts into mirroring the
                                // selection to the system CLIPBOARD as
                                // well, matching the WebView build's
                                // toggle. PRIMARY is always updated above
                                // so the middle-click flow keeps working
                                // regardless.
                                if self.app.settings.copy_on_select && !text.is_empty() {
                                    host.set_clipboard(&text);
                                }
                            }
                        }
                    }
                    (MouseButton::Middle, ElementState::Pressed) => {
                        if self.app.settings.middle_click_paste {
                            if let Some(text) = host.get_primary() {
                                host.deliver_paste(&self.app, &text);
                            }
                        }
                    }
                    _ => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // While the profile-selector modal is up, the wheel
                // scrolls the modal's list: translate to an egui
                // MouseWheel event (the raw-input builder does not
                // forward wheel deltas on the terminal path) and skip
                // the terminal viewport scroll.
                if self.app.profile_selector.visible {
                    let (unit, delta) = match delta {
                        MouseScrollDelta::LineDelta(x, y) => {
                            (egui::MouseWheelUnit::Line, egui::vec2(x, y))
                        }
                        MouseScrollDelta::PixelDelta(p) => (
                            egui::MouseWheelUnit::Point,
                            egui::vec2(p.x as f32, p.y as f32),
                        ),
                    };
                    host.pending_egui_events.push(egui::Event::MouseWheel {
                        unit,
                        delta,
                        modifiers: egui::Modifiers::default(),
                    });
                    host.window().request_redraw();
                    return;
                }
                // FR2/FR3: a wheel over the tab-bar strip scrolls the tab
                // strip horizontally instead of the terminal scrollback.
                // Forward the wheel to egui — the tab strip's horizontal
                // ScrollArea consumes it, and with
                // `always_scroll_the_only_direction` set both bare and
                // Shift+wheel fold onto the horizontal axis. egui hit-tests
                // against the hover position kept current by the
                // `PointerMoved` events forwarded on every winit
                // `WindowEvent::PointerMoved`, so the wheel only reaches
                // the strip when the pointer is over it.
                // Restricted to the tab-bar band (below the CSD title bar);
                // the title bar's existing wheel behaviour is left untouched.
                {
                    let logical = host
                        .cursor_pos
                        .to_logical::<f32>(host.pixels_per_point as f64);
                    let top_strip_h = crate::ui::title_bar::TITLE_BAR_HEIGHT
                        + crate::ui::tab_bar::effective_tab_bar_height(self.app.show_tab_bar);
                    if logical.y >= crate::ui::title_bar::TITLE_BAR_HEIGHT
                        && logical.y < top_strip_h
                    {
                        let (unit, ev_delta) = match delta {
                            MouseScrollDelta::LineDelta(x, y) => {
                                (egui::MouseWheelUnit::Line, egui::vec2(x, y))
                            }
                            MouseScrollDelta::PixelDelta(p) => (
                                egui::MouseWheelUnit::Point,
                                egui::vec2(p.x as f32, p.y as f32),
                            ),
                        };
                        host.pending_egui_events.push(egui::Event::MouseWheel {
                            unit,
                            delta: ev_delta,
                            modifiers: egui::Modifiers::default(),
                        });
                        host.window().request_redraw();
                        return;
                    }
                }
                // task0010 FR2/NFR2: a wheel over the mux sidebar
                // (persistent panel OR overlay card) scrolls the sidebar's
                // window list instead of the terminal scrollback /
                // AltScreen arrow-scroll path. `point_in_sidebar` is the
                // SAME hit-region derivation `ui::mux_sidebar`'s draw path
                // uses (IMPLEMENTATION.md cross-task decision 3.5), so this
                // guard can never independently drift from what's actually
                // painted — the round-2 lesson a manual, re-derived
                // winit-side guard caused. `visible_placement` resolves to
                // `None` on local tabs and sidebar-hidden states, so
                // `point_in_sidebar` always answers `false` there and this
                // block is a complete no-op (NFR2).
                {
                    let visible_placement = match self.app.mux_sidebar_visibility() {
                        crate::app::MuxSidebarVisibility::Hidden => None,
                        crate::app::MuxSidebarVisibility::Persistent => {
                            Some(crate::ui::mux_sidebar::Placement::Persistent)
                        }
                        crate::app::MuxSidebarVisibility::Overlay => {
                            Some(crate::ui::mux_sidebar::Placement::Overlay)
                        }
                    };
                    if visible_placement.is_some() {
                        let logical = host
                            .cursor_pos
                            .to_logical::<f32>(host.pixels_per_point as f64);
                        let window_size_logical = host
                            .window
                            .surface_size()
                            .to_logical::<f32>(host.pixels_per_point as f64);
                        let top_chrome =
                            crate::ui::mux_sidebar::top_chrome_inset(self.app.show_tab_bar);
                        if crate::ui::mux_sidebar::point_in_sidebar(
                            egui::pos2(logical.x, logical.y),
                            visible_placement,
                            egui::vec2(window_size_logical.width, window_size_logical.height),
                            top_chrome,
                            host.status_bar_bot_inset_logical,
                        ) {
                            let (unit, ev_delta) = match delta {
                                MouseScrollDelta::LineDelta(x, y) => {
                                    (egui::MouseWheelUnit::Line, egui::vec2(x, y))
                                }
                                MouseScrollDelta::PixelDelta(p) => (
                                    egui::MouseWheelUnit::Point,
                                    egui::vec2(p.x as f32, p.y as f32),
                                ),
                            };
                            host.pending_egui_events.push(egui::Event::MouseWheel {
                                unit,
                                delta: ev_delta,
                                modifiers: egui::Modifiers::default(),
                            });
                            host.window().request_redraw();
                            return;
                        }
                    }
                }
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => {
                        let (_, cell_h_px, _, _) = host.cell_metrics_px(&self.app);
                        (p.y as f32) / (cell_h_px.max(1.0) as f32)
                    }
                };

                // FR1 (DECSET 1007): in alternate screen, when the
                // terminal-side mode bit AND the user setting are both
                // ON, translate the wheel notches into arrow-key bytes
                // sent to the active PTY so AltScreen apps (Claude
                // Code, vim, less) scroll their own log instead of
                // moving eMterm's scrollback view. xterm convention:
                // 3 arrow bytes per notch; Shift is ignored.
                let mode_bit_on = self
                    .app
                    .active_tab()
                    .map(|t| {
                        t.core
                            .lock()
                            .get_mode(term_core::terminal_core::MODE_ALTERNATE_SCROLL)
                    })
                    .unwrap_or(false);
                // FR1 accumulator: reset fractional state when not in AltScreen
                // so entering AltScreen always starts clean.
                if !self.app.alt_screen {
                    host.alt_scroll_accum = 0.0;
                }
                let (whole, new_frac) = accumulate_alt_scroll_lines(host.alt_scroll_accum, lines);
                host.alt_scroll_accum = new_frac;
                if whole != 0.0 {
                    if let Some(buf) = alternate_scroll_wheel_bytes(
                        whole,
                        self.app.alt_screen,
                        mode_bit_on,
                        self.app.settings.alternate_scroll_enabled,
                    ) {
                        if let Some(tab) = self.app.active_tab() {
                            tab.write_input(buf);
                        }
                        // Visible content may shift under the pointer;
                        // drop the cached hover so the next PointerMoved
                        // re-detects.
                        host.invalidate_link_hover();
                        host.window().request_redraw();
                        return;
                    }
                }

                // `settings.scroll_speed` is clamped to 1..=10 by the
                // loader, so it's safe to feed directly into the scroll
                // helpers (a runaway typo can't fly the viewport 1000
                // rows per notch).
                let step = self.app.settings.scroll_speed.max(1);
                if lines > 0.0 {
                    self.app.scroll_up_by(step);
                    // Scrollback content shifts under the pointer, so the
                    // cached hover no longer maps to the same text. Drop
                    // it; the next PointerMoved re-detects.
                    host.invalidate_link_hover();
                    host.window().request_redraw();
                } else if lines < 0.0 {
                    self.app.scroll_down_by(step);
                    host.invalidate_link_hover();
                    host.window().request_redraw();
                }
            }
            WindowEvent::DragEntered { .. } => {
                // A drag entered the window: show the drop overlay. The
                // message depends on whether the active tab is an SSH tab
                // (upload) or not (paste).
                let overlay = if self
                    .app
                    .active_tab()
                    .map(|t| t.is_ssh_tab())
                    .unwrap_or(false)
                {
                    crate::sftp::ui::HoverOverlay::SshUpload
                } else {
                    crate::sftp::ui::HoverOverlay::Paste
                };
                self.app.sftp_ui.hover = Some(overlay);
                host.window().request_redraw();
            }
            // The pointer moving while files are dragged over the window
            // carries no paths and needs no state change here — the
            // `DragEntered` overlay set above stays up until `DragLeft` /
            // `DragDropped`.
            WindowEvent::DragMoved { .. } => {}
            WindowEvent::DragLeft { .. } => {
                self.app.sftp_ui.hover = None;
                host.window().request_redraw();
            }
            WindowEvent::DragDropped { paths, .. } => {
                // winit 0.31 delivers the whole drag session's paths in
                // one event (FR3 / IMPLEMENTATION.md D3) — no cross-event
                // batching needed.
                self.app.sftp_ui.hover = None;
                if let Some(batch) = crate::sftp::ui::drop_batch_from_paths(paths) {
                    self.app.dispatch_drop(batch);
                    host.window().request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                host.render(&mut self.app);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        let Some(host) = self.host.as_mut() else {
            return;
        };
        // Phase 4-G: drain any pending IME events from the active
        // backend into the existing on_ime_* routes before touching
        // PTY output. A real backend may have queued events while we
        // were idle; the NullBackend always returns an empty drain so
        // this is a cheap no-op when disabled.
        let ime_changed = self.app.pump_ime();
        let pty_changed = self.app.pump_all();
        // The child settings window reported a persisted save (its stdout
        // watcher raised the flag and woke this loop via the proxy):
        // reload settings.json and apply it live.
        if crate::settings_launcher::take_saved() {
            host.reload_settings_from_disk(&mut self.app);
        }
        // If the search overlay is open with live results, the pumps (PTY
        // output / resize) may have shifted matched text into scrollback,
        // staling the cached document and the matches' absolute rows. Re-run
        // the search once here so highlights track the text without yanking
        // the viewport. Per-frame cadence throttles bursts of PTY chunks.
        let search_changed = self.app.auto_research_if_dirty();
        // Cursor blink advances on a 530 ms half-cycle (BLINK_HALF_MS).
        // egui's request_repaint_after is silent (no callback bridges
        // it back to winit), so we have to detect the phase flip
        // ourselves and request a redraw — otherwise the cursor freezes
        // at whatever phase the last paint landed on.
        let blink_due = self.app.needs_blink_repaint();
        // Visual-bell flash decays over 150 ms; like blink, nothing
        // else would schedule the intermediate frames, so poll it here.
        let bell_due = self.app.needs_bell_repaint();
        // PTY content may have changed under a stationary pointer. Only
        // re-run detection when the pointer is inside the window and no
        // selection drag is in progress — PTY output during a drag must
        // not flip the cursor to a hand or underline a link mid-selection.
        // The staleness comparison + cache invalidation lives in
        // `refresh_link_hover_on_pty_change` so the `HoverState` fields
        // are never poked from the event loop body.
        if pty_changed && host.pointer_in_window && !host.dragging {
            host.refresh_link_hover_on_pty_change(&self.app);
        }
        // Toasts auto-dismiss on frame time, but nothing else schedules the
        // intermediate frames: on an idle / unfocused terminal the redraw
        // triggers above can all be false, so a visible toast would never be
        // pruned until an unrelated event. While any toast is up, keep frames
        // flowing so the restart / SFTP toasts dismiss on schedule.
        //
        // Rate-limited to the `TOAST_POLL_MS` cadence via `last_toast_redraw`:
        // the `WaitUntil` timer below does NOT bound this by itself, because
        // an unconditional `request_redraw()` here re-enters
        // `RedrawRequested` → `about_to_wait` immediately (the loop never
        // becomes idle enough to reach the timer), and with a non-blocking
        // present mode (Mailbox/Immediate, see `WindowHost::new`) nothing
        // else brakes the cycle — the loop spins at full speed for the
        // toast's entire lifetime.
        let toast_pending =
            self.app.restart_toast.active() || !self.app.sftp_ui.toasts.toasts.is_empty();
        let toast_due = toast_redraw_due(toast_pending, host.last_toast_redraw, Instant::now());
        if toast_due {
            host.last_toast_redraw = Some(Instant::now());
        }
        // task0002 D5: the overlay card's dim/fade needs a redraw request
        // at the same two junctures blink/bell/toast do — nothing else
        // would wake the loop at the bright-hold expiry or during the
        // fade, since `ControlFlow::WaitUntil` only re-enters this
        // function, it does not itself trigger a repaint. The actual
        // state mutation (arming/resetting the fade-bookkeeping instant)
        // happens in `WindowHost::render` via
        // `App::resolve_mux_sidebar_opacity`, once the redraw this
        // requests actually runs — this check is read-only.
        let mux_sidebar_dim_due = self.app.mux_sidebar_dim_due(Instant::now());
        // mux-tab-switch-bypass-refix task0002 Change 1 (finding
        // `81507f39e384b34e`): the fallback path for the resize-settle
        // self-wake — `refresh_status_bar_insets` (inside `render()`)
        // handles the fast path when a render is already happening; this
        // is what fires the wake when NOTHING else does (the exact
        // fully-idle-window case findings 02546e5e10deb500 /
        // 5b1878c41d3e02d6-perf-P2 describe), reached via the
        // `next_resize_settle_wake_deadline`-armed `WaitUntil` below. Both
        // sites read/write the same `last_resize_settle_wake`, so whichever
        // runs first for a given tick gates the other out.
        let resize_settle_wake_due = resize_settle_self_wake_due(
            host.resize_settler.awaiting_decision(),
            host.last_resize_settle_wake,
            Instant::now(),
        );
        if resize_settle_wake_due {
            host.last_resize_settle_wake = Some(Instant::now());
        }
        if ime_changed
            || pty_changed
            || search_changed
            || blink_due
            || bell_due
            || toast_due
            || mux_sidebar_dim_due
            || resize_settle_wake_due
        {
            host.window().request_redraw();
        }
        // Cursor cell may have moved as a side effect of pumps; notify
        // the IME backend if the (row, col) changed. Use the same
        // physical-pixel metrics + origin as the grid renderer so the
        // IME spot lands on the actual cursor cell, not a HiDPI-off
        // approximation.
        let (cell_w_px, cell_h_px, origin_x_px, origin_y_px) = host.cell_metrics_px(&self.app);
        self.app.notify_cursor_rect_if_changed(
            cell_w_px.round().max(1.0) as u32,
            cell_h_px.round().max(1.0) as u32,
            origin_x_px.round() as i32,
            origin_y_px.round() as i32,
        );
        // task0001 (windows-skk-ime-hang) FR1: flush any IME requests
        // recorded this turn (construction-time enable, notify_focus,
        // notify_cursor_rect_if_changed above) here — outside any
        // wndproc/event-dispatch frame — instead of calling the OS IME
        // APIs synchronously from inside `window_event`.
        self.app.flush_ime();
        if self.app.tabs.is_empty() || host.pending_close() {
            // Same teardown handshake as the CloseRequested path: drop
            // the wgpu / window resources before EventLoop unwinds so
            // the Vulkan WSI surface destructor sees a live X11
            // connection. Two close paths converge here:
            //   - the last tab closed (`apply_tab_event` removed it), or
            //   - the user clicked the CSD title-bar `×` (which sets
            //     `pending_close` rather than touching the event loop
            //     directly, so the drop order matches both other paths).
            self.host = None;
            event_loop.exit();
            return;
        }
        // task0004 D4: stop unconditionally rearming a 16 ms `WaitUntil`.
        // With no timed work pending (blink disabled or unfocused, no bell
        // decay, no toast) the loop drops to a true `ControlFlow::Wait` —
        // every producer that used to rely on this 60 Hz pump now wakes the
        // loop explicitly (PTY reader threads / mux off-thread workers via
        // `crate::wakeup::wake()`, IME/input via winit's native wake, this
        // turn's own blink/bell/toast deadlines via `control_flow_for`).
        event_loop.set_control_flow(control_flow_for(
            &self.app,
            next_resize_settle_wake_deadline(
                host.resize_settler.awaiting_decision(),
                Instant::now(),
            ),
        ));
    }

    /// Phase E (TS-32): winit `EventLoopProxy::wake_up()` calls land
    /// here (renamed from `user_event` in winit 0.31 — wake-ups no
    /// longer carry a payload). Without this override, the trait-default
    /// `proxy_wake_up` is a no-op and the provider-owned wake chain
    /// (`TimeProvider` timer thread → `WakeFn` → `EventLoopProxy::wake_up()`
    /// → here → `request_redraw`) is silently broken, freezing the
    /// status-bar clock when the shell is idle.
    ///
    /// Defensive: if `self.host` is `None` (we are between
    /// `can_create_surfaces`-time construction failure and process exit,
    /// or already torn down in `CloseRequested`), this is a no-op.
    fn proxy_wake_up(&mut self, _event_loop: &dyn ActiveEventLoop) {
        request_redraw_on_user_event(self.host.as_ref(), |host| {
            host.window().request_redraw();
        });
    }
}

impl Drop for PocApp {
    /// winit 0.31 removed `ApplicationHandler::exiting`; the same
    /// defense-in-depth shutdown step (for any code path that flagged
    /// exit without zeroing `self.host`, e.g. future error-path exits)
    /// now runs in `Drop`, called once `run_app` unwinds after
    /// `event_loop.exit()`. The Vulkan / X11 teardown must happen while
    /// EventLoop is still alive — see the field-order note on
    /// `WindowHost`.
    fn drop(&mut self) {
        if self.host.is_some() {
            log::info!("native-poc: exiting handler dropping WindowHost");
            self.host = None;
        }
    }
}

/// Pure-logic decision for `PocApp::user_event` (TS-32).
///
/// Extracted as a free function so unit tests can exercise the
/// "redraw if host is present, no-op otherwise" contract without
/// instantiating a real winit window (which requires an active
/// event loop and a display). The `redraw` callback is invoked at
/// most once, and only when `host` is `Some`.
pub(super) fn request_redraw_on_user_event<H, F>(host: Option<&H>, redraw: F)
where
    F: FnOnce(&H),
{
    if let Some(h) = host {
        redraw(h);
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
