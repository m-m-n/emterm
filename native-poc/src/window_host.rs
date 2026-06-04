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
use std::time::{Duration, Instant};

use egui::ViewportId;
use egui_wgpu::wgpu::SurfaceError;
use egui_wgpu::ScreenDescriptor;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};
use winit::window::{CursorIcon, ResizeDirection, Window, WindowAttributes, WindowId};

use crate::app::App;
use crate::image::overlay::OverlayPipeline;
use crate::image::ImageLayer;
use crate::ime::backend::{build_backend_with_window, KeyDispatchResult, ProcessEnv, RawKeyEvent};
use crate::pty::input::{encode, Key, Modifiers};
use crate::render::terminal_grid_pass::TerminalGridPass;
use crate::selection::{Pos, Selection, SelectionMode};
use crate::ui::keybinds::Chord;

/// Maximum time between successive clicks that still counts as a "multi-click".
/// Within this window the click counter increments; beyond it the counter
/// resets to 1. 500 ms matches xterm's `multiClickTime` default.
const MULTI_CLICK_WINDOW_MS: u128 = 500;

/// Hit-zone width for CSD edge / corner resize, expressed in egui logical
/// points. 8 pt is the smallest band that's reliably grabbable with a
/// mouse — the user's hand can overshoot the edge band before the
/// cursor icon flips, so anything narrower than this manifested as
/// "上下リサイズが効かない" because the pointer ended up in the
/// title-bar / status-bar interior before reaching the resize zone.
const RESIZE_EDGE_PX: f32 = 8.0;

/// Tracks last-click metadata so a double / triple click can be detected by
/// comparing time + position against the next press.
#[derive(Debug, Clone, Copy, Default)]
struct ClickTracker {
    last_press_at: Option<Instant>,
    last_press_pos: Option<(u16, u16)>,
    /// Click counter: 1 → Character, 2 → Word, 3 → Line. After 3, the next
    /// press resets to 1.
    count: u32,
}

/// Output of the click classifier: the click count and the matching
/// selection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClickClassification {
    count: u32,
    mode: SelectionMode,
}

impl ClickTracker {
    /// Classify a new press at `(row, col)` happening at `now`. The internal
    /// state is updated for the next call.
    fn classify(&mut self, now: Instant, row: u16, col: u16) -> ClickClassification {
        let mut count = 1u32;
        if let (Some(prev_at), Some(prev_pos)) = (self.last_press_at, self.last_press_pos) {
            let elapsed_ms = now.duration_since(prev_at).as_millis();
            if elapsed_ms <= MULTI_CLICK_WINDOW_MS && prev_pos == (row, col) && self.count < 3 {
                count = self.count + 1;
            }
        }
        self.last_press_at = Some(now);
        self.last_press_pos = Some((row, col));
        self.count = count;
        ClickClassification {
            count,
            mode: match count {
                1 => SelectionMode::Character,
                2 => SelectionMode::Word,
                _ => SelectionMode::Line, // 3 or more
            },
        }
    }
}

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
    /// Frame draw order is `clear → TerminalGridPass → egui (LoadOp::Load)
    /// → ImageOverlayPass (LoadOp::Load)`.
    grid_pass: Option<TerminalGridPass>,
    /// Reusable wgpu pipeline that draws every visible placement after
    /// the egui pass on the same swapchain texture (`LoadOp::Load`).
    overlay_pipeline: OverlayPipeline,
    /// Phase 5: inline-image overlay (Kitty Graphics + SIXEL). Single
    /// instance shared by all tabs — the per-tab `ImageProcessor` lives in
    /// `Tab::image_proc` and produces `ImageEvent`s which are forwarded
    /// here once per frame via `Tab::drain_image_events`.
    image_layer: ImageLayer,
    egui_renderer: egui_wgpu::Renderer,
    egui_ctx: egui::Context,
    surface_config: wgpu::SurfaceConfiguration,
    queue: wgpu::Queue,
    device: wgpu::Device,
    surface: wgpu::Surface<'static>,
    instance: wgpu::Instance,
    window: Arc<Window>,
    pixels_per_point: f32,
    /// True when the surface must be recreated on the next frame (e.g. after
    /// `SurfaceError::Lost`).
    surface_dirty: bool,
    /// Alacritty-style deferred resize: `WindowEvent::Resized` only flips
    /// this flag and requests a redraw; the next `render()` call reads the
    /// current `window.inner_size()` once and runs `surface.configure()` +
    /// `app.set_grid_size()` together. This coalesces bursts of compositor
    /// resize events (one configure per frame instead of one per event) and
    /// avoids back-buffer locking when configure and draw happen out of
    /// order on Wayland / X11. See
    /// `wezterm/window/src/os/x11/window.rs:298` (coalesce) and
    /// `alacritty/src/display/mod.rs:739` (defer-to-render).
    pending_resize: bool,
    /// Cached status-bar panel insets in egui logical points, refreshed
    /// each frame from `App::status_bar_view_model`. Subtracted from the
    /// usable grid area in [`grid_size`] (and added to `origin_y` when
    /// the panel sits on top) so the terminal's bottom/top row never
    /// renders behind the status-bar panel.
    status_bar_top_inset_logical: f32,
    status_bar_bot_inset_logical: f32,
    current_mods: Modifiers,
    /// Last cursor position in physical pixels (updated on `CursorMoved`).
    cursor_pos: PhysicalPosition<f64>,
    /// Whether the left button is currently held — used as the gate for
    /// turning subsequent `CursorMoved` events into selection extends.
    dragging: bool,
    /// Cell position at which the left button went down, kept until
    /// either the pointer moves (the press is upgraded to a drag and a
    /// Character-mode `Selection` is created) or the button is released
    /// (treated as a click — no selection is materialized). Word and
    /// line selections from double / triple click bypass this and
    /// commit immediately, just like before.
    pending_press_cell: Option<Pos>,
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
    /// Set when the user clicks the CSD title-bar's `×` button. The
    /// `about_to_wait` handler picks this up and runs the same
    /// teardown handshake (drop `host` → `event_loop.exit()`) used
    /// for the last-tab-closed path, so the wgpu / X11 resources
    /// unwind in the same order regardless of the close path.
    pending_close: bool,
    /// Cached CSD resize direction under the pointer. Refreshed on
    /// every `CursorMoved` (when not selection-dragging) so the next
    /// left-press can hand the matching [`ResizeDirection`] to
    /// `Window::drag_resize_window` without re-running the hit test.
    /// `None` means the pointer is in the window interior — a press
    /// falls through to the existing selection / tab-bar handlers.
    current_resize_dir: Option<ResizeDirection>,
    /// Last cursor icon pushed to winit. Cached so [`update_resize_hint`]
    /// can skip the IPC round-trip when the icon would not change —
    /// `set_cursor` is otherwise called on every `CursorMoved`, which
    /// floods the compositor with redundant requests.
    current_cursor: CursorIcon,
}

impl WindowHost {
    /// Build the window + GPU resources.
    ///
    /// `image_quota_bytes` is the per-process cap on inline-image GPU
    /// memory (sourced from `Settings::image_memory_quota_mb`); when the
    /// cap is hit, the LRU-front image is evicted before any new upload.
    pub fn new(event_loop: &ActiveEventLoop, image_quota_bytes: u64) -> Self {
        let attrs = WindowAttributes::default()
            .with_title("eMterm PoC")
            .with_decorations(false)
            .with_inner_size(LogicalSize::new(960.0, 600.0))
            .with_min_inner_size(LogicalSize::new(320.0, 200.0));
        let window = event_loop
            .create_window(attrs)
            .expect("native-poc: failed to create winit window");
        let window = Arc::new(window);

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        // SAFETY: `window` is kept alive in `Arc<Window>` and stored
        // alongside the surface for the whole `WindowHost` lifetime.
        let surface: wgpu::Surface<'static> = unsafe {
            instance
                .create_surface_unsafe(
                    wgpu::SurfaceTargetUnsafe::from_window(&*window).expect("surface target"),
                )
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

        let size = window.inner_size();
        let surface_caps = surface.get_capabilities(&adapter);
        let format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
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
        configure_egui_emoji_fallback(&egui_ctx);
        let egui_renderer = egui_wgpu::Renderer::new(&device, format, None, 1, false);

        let pixels_per_point = window.scale_factor() as f32;

        let clipboard = match arboard::Clipboard::new() {
            Ok(c) => Some(c),
            Err(e) => {
                log::warn!("arboard clipboard unavailable: {e}");
                None
            }
        };

        let image_layer = ImageLayer::new(image_quota_bytes);
        let overlay_pipeline = OverlayPipeline::new(&device, format);

        Self {
            window,
            instance,
            surface,
            surface_config,
            device,
            queue,
            egui_ctx,
            egui_renderer,
            pixels_per_point,
            surface_dirty: true,
            pending_resize: false,
            status_bar_top_inset_logical: 0.0,
            status_bar_bot_inset_logical: 0.0,
            current_mods: Modifiers::NONE,
            cursor_pos: PhysicalPosition::new(0.0, 0.0),
            dragging: false,
            pending_press_cell: None,
            click_tracker: ClickTracker::default(),
            clipboard,
            image_layer,
            overlay_pipeline,
            grid_pass: None,
            pending_egui_events: Vec::new(),
            pending_close: false,
            current_resize_dir: None,
            current_cursor: CursorIcon::Default,
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

    pub fn window(&self) -> &Window {
        &self.window
    }

    /// Hand a clone of the `Arc<Window>` to callers that need to retain
    /// the handle themselves (Phase 4-G-3 passes this to
    /// `WinitImeBridge::init` so the bridge can call
    /// `Window::set_ime_cursor_area`).
    pub fn window_arc(&self) -> Arc<Window> {
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
            .inner_size()
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
    /// pointer position. Cheap to call on every `CursorMoved`: skips
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
        self.window.set_cursor(icon);
    }

    /// Reconfigure the wgpu surface for the current window size.
    fn reconfigure_surface(&mut self) {
        let size = self.window.inner_size();
        self.surface_config.width = size.width.max(1);
        self.surface_config.height = size.height.max(1);
        self.surface.configure(&self.device, &self.surface_config);
        self.surface_dirty = false;
    }

    /// Fully recreate the surface from the underlying instance. Reserved
    /// for severe failure modes (e.g. `OutOfMemory`) where simply
    /// reconfiguring the current handle is insufficient. The Lost/Outdated
    /// recovery path uses [`reconfigure_surface`] driven by `surface_dirty`.
    #[allow(dead_code)]
    fn recreate_surface(&mut self) {
        log::warn!("recreating wgpu surface after device/surface loss");
        let new_surface: wgpu::Surface<'static> = unsafe {
            self.instance
                .create_surface_unsafe(
                    wgpu::SurfaceTargetUnsafe::from_window(&*self.window).expect("surface target"),
                )
                .expect("recreate surface")
        };
        self.surface = new_surface;
        self.reconfigure_surface();
    }

    /// Mark the surface as needing a reconfigure on the next render.
    ///
    /// Alacritty-style deferred resize: the caller (winit `Resized` /
    /// `ScaleFactorChanged` handlers) only flips the flag and requests a
    /// redraw. The actual `surface.configure()` + PTY grid resize happens
    /// once in [`apply_pending_resize`] at the head of [`render`].
    pub fn request_resize(&mut self) {
        self.pending_resize = true;
    }

    /// Consume `pending_resize` and apply the latest `window.inner_size()`.
    ///
    /// Called once per `render()` so a burst of compositor resize events
    /// produces a single configure + PTY resize cycle aligned with the
    /// frame boundary. Zero-sized windows (Windows minimize, Wayland hidden)
    /// just clear the flag without reconfiguring.
    fn apply_pending_resize(&mut self, app: &mut App) {
        if !self.pending_resize {
            return;
        }
        self.pending_resize = false;
        let size = self.window.inner_size();
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
    /// panel. When the inset changes, flag a pending resize so the PTY
    /// is reshaped before the next frame paints — otherwise the bottom
    /// row would be hidden behind the panel for one frame.
    fn refresh_status_bar_insets(&mut self, app: &App) {
        let vm = app.status_bar_view_model();
        let height = crate::ui::status_bar::panel_height_logical(&vm);
        let (top, bot) = match vm.position {
            crate::settings::StatusBarPosition::Top => (height, 0.0),
            crate::settings::StatusBarPosition::Bottom => (0.0, height),
        };
        if (self.status_bar_top_inset_logical - top).abs() > f32::EPSILON
            || (self.status_bar_bot_inset_logical - bot).abs() > f32::EPSILON
        {
            self.status_bar_top_inset_logical = top;
            self.status_bar_bot_inset_logical = bot;
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
    fn cell_metrics_px(&self, app: &App) -> (f64, f64, f64, f64) {
        let scale = self.pixels_per_point.max(1.0) as f64;
        // Cell dims come from the App's startup measurement of the
        // base font (see `App::with_settings` → `compute_cell_dims`)
        // so they track `settings.font_size` instead of the legacy
        // hard-coded 8.5×17.
        let cell_w = (app.cell_w_logical as f64) * scale;
        let cell_h = (app.cell_h_logical as f64) * scale;
        // Inner padding comes from settings.padding (logical pixels);
        // falls back to the renderer's default constants when the
        // user hasn't overridden them.
        let pad = (app.settings.padding as f64).max(0.0);
        let origin_x = pad * scale;
        // Vertical origin reserves room for every panel stacked above
        // the terminal grid: the CSD title bar, the tab strip, an
        // optional status-bar row pinned to the top, and the same
        // user-configured padding inset. Forgetting the title bar
        // here makes the first cell render behind the tab strip.
        let tab_h = crate::ui::tab_bar::effective_tab_bar_height(app.settings.show_tab_bar) as f64;
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
    pub fn grid_size(&self, app: &App) -> (u16, u16) {
        let w = self.surface_config.width.max(1) as f64;
        let h = self.surface_config.height.max(1) as f64;
        let (cell_w, cell_h, origin_x, origin_y) = self.cell_metrics_px(app);
        let scale = self.pixels_per_point.max(1.0) as f64;
        let bottom_inset_px = (self.status_bar_bot_inset_logical as f64) * scale;
        // Usable area starts after the top bar (+ top status bar) +
        // top pad and the left pad, and ends above the bottom status
        // bar. Floor the resulting cell count so partial trailing
        // cells (which would clip at the surface edge) don't get
        // reported as a writable row/col.
        let usable_w = (w - origin_x).max(cell_w);
        let usable_h = (h - origin_y - bottom_inset_px).max(cell_h);
        let cols = (usable_w / cell_w).floor().clamp(20.0, 500.0) as u16;
        let rows = (usable_h / cell_h).floor().clamp(5.0, 200.0) as u16;
        (cols, rows)
    }

    /// Map a physical pixel position to a grid cell `(row, col)`,
    /// honoring the same origin + cell metrics the renderer uses.
    fn pixel_to_cell(&self, pos: PhysicalPosition<f64>, app: &App) -> (u16, u16) {
        let (cell_w, cell_h, origin_x, origin_y) = self.cell_metrics_px(app);
        let x = ((pos.x - origin_x).max(0.0)) / cell_w;
        let y = ((pos.y - origin_y).max(0.0)) / cell_h;
        let cols = app.cell_size.cols.max(1);
        let rows = app.cell_size.rows.max(1);
        let col = (x as u32).min((cols - 1) as u32) as u16;
        let row = (y as u32).min((rows - 1) as u32) as u16;
        (row, col)
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
        if let Some(pty) = &tab.pty {
            pty.write_paste(text, bracketed);
        }
    }

    /// Run a single egui frame and present.
    pub fn render(&mut self, app: &mut App) {
        // Refresh the cached status-bar insets first: the deferred-
        // resize path below reads them to compute the PTY grid size,
        // and the grid-pass origin in this same frame also reads them.
        // A change here flips `pending_resize` so the PTY is reshaped
        // in step with the panel growing / shrinking (e.g. when the
        // mux session attaches and the OSC row pops in).
        self.refresh_status_bar_insets(app);

        // Alacritty-style deferred resize: apply pending window-size changes
        // here, not inside the winit `Resized` handler. This coalesces
        // bursts of compositor resize events into a single configure +
        // PTY-resize per frame, and keeps `surface.configure()` paired with
        // the swapchain acquire that follows so Wayland / X11 don't lock
        // the back buffer between the two calls. Done before `surface_dirty`
        // so a pending resize subsumes any prior Lost/Outdated reconfigure.
        let had_pending_resize = self.pending_resize;
        self.apply_pending_resize(app);
        if had_pending_resize {
            // Resize changes the swapchain extent; everything must repaint.
            app.mark_full_redraw();
            // We just reconfigured to the new size, so any earlier
            // Lost/Outdated recovery request is now redundant.
            self.surface_dirty = false;
        }

        // Phase 0: lazy first-frame configure + recovery from Lost/Outdated.
        // `surface_dirty` is true on construction (deferred configure) and
        // whenever a previous frame returned `Lost` / `Outdated`. We
        // reconfigure with the current physical size before acquiring the
        // next swapchain texture.
        let was_surface_dirty = self.surface_dirty || had_pending_resize;
        if self.surface_dirty {
            self.reconfigure_surface();
            // Reconfiguring the swapchain produces a fresh-but-uninitialized
            // surface texture; the next present needs a full paint.
            app.mark_full_redraw();
        }

        // Phase 5: drain image events from every tab into the shared
        // GPU `ImageLayer`. The drain must happen *before* the skip-frame
        // check below because a new image arriving alone (no row dirty)
        // still requires a frame to paint the overlay quad.
        let mut have_pending_images = false;
        for tab in app.tabs.iter_mut() {
            let events = tab.drain_image_events();
            if !events.is_empty() {
                have_pending_images = true;
                self.image_layer.ingest(events, &self.device, &self.queue);
            }
        }
        // Keep image placements anchored to the current cell metrics —
        // must match what the grid pass actually draws, otherwise image
        // overlays drift relative to text on HiDPI.
        let (cell_w_px, cell_h_px, _, _) = self.cell_metrics_px(app);
        self.image_layer.recompute_pixel_dims(
            cell_w_px.round().max(1.0) as u32,
            cell_h_px.round().max(1.0) as u32,
        );

        // Sub-phase 2 dirty-row diff: skip the entire egui+wgpu cycle when
        // nothing in the active tab needs to repaint. The first frame
        // (or any frame that follows a surface reconfigure) bypasses this
        // skip because `App::mark_full_redraw()` forces the dirty set to
        // the full row range. Phase 5: also bypass when there are pending
        // image events to draw or any placements (re-paint over text).
        //
        // The status-bar carve-out: provider-owned wake chains
        // (`TimeProvider` timer thread, `GitBranchProvider` worker,
        // OSC 777 push) drive `EventLoopProxy::send_event(())` ->
        // `user_event` -> `request_redraw`, but on an idle shell the
        // resulting `render()` would observe `dirty_rows_this_frame()
        // == 0` and short-circuit here, freezing the `{time}` display.
        // `App::status_bar_view_model_changed` does the comparison
        // against the previous frame's view model so the skip path
        // only triggers when neither the terminal nor the status bar
        // moved.
        if !was_surface_dirty && !have_pending_images {
            let dirty_count = if let Some(tab) = app.active_tab() {
                let core = tab.core.lock();
                let dirty = app.dirty_rows_this_frame(&core);
                let rows = core.rows();
                log::debug!(
                    "native-poc: dirty rows this frame = {} / {}",
                    dirty.len(),
                    rows
                );
                Some(dirty.len())
            } else {
                // No tab: still render once to draw the hint message; rely
                // on the `needs_full_redraw` flag bookkeeping for that.
                None
            };
            let status_bar_changed = app.status_bar_view_model_changed();
            if matches!(dirty_count, Some(0))
                && self.image_layer.state.placement_count() == 0
                && !status_bar_changed
            {
                return;
            }
        }

        let raw_input = self.build_raw_input();
        let mut frame_events = crate::render::FrameEvents {
            title: None,
            tab: None,
        };
        // Snapshot the current maximized state so the title bar can
        // swap its middle glyph between Maximize and Restore. Reading
        // the window here (instead of inside `draw_placeholder`)
        // keeps the render module free of winit dependencies.
        let window_maximized = self.window.is_maximized();
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            frame_events = crate::render::draw_placeholder(ctx, app, window_maximized);
        });
        // CSD title-bar actions hit `winit::Window` directly except
        // for Close, which defers to `about_to_wait` via
        // `pending_close` so teardown follows the same handshake as
        // the last-tab-closed path.
        if let Some(evt) = frame_events.title {
            self.apply_title_bar_event(evt);
        }
        // Apply any tab bar interaction emitted this frame. Closing
        // the last tab returns `true` and the next event loop tick
        // observes `app.tabs.is_empty()` to exit the window.
        if let Some(evt) = frame_events.tab {
            let _ = app.apply_tab_event(evt);
            // Tab roster changed; force a full redraw next frame.
            app.mark_full_redraw();
        }
        let paint_jobs = self
            .egui_ctx
            .tessellate(full_output.shapes, self.pixels_per_point);
        let textures_delta = full_output.textures_delta;

        let surface_texture = match self.surface.get_current_texture() {
            Ok(tex) => tex,
            Err(SurfaceError::Lost) | Err(SurfaceError::Outdated) => {
                // Mark dirty so the next frame reconfigures before acquire,
                // and request a redraw so the event loop schedules one.
                log::warn!("wgpu surface Lost/Outdated; will reconfigure next frame");
                self.surface_dirty = true;
                self.window.request_redraw();
                return;
            }
            Err(SurfaceError::OutOfMemory) => {
                log::error!("wgpu surface out of memory; will recreate next frame");
                self.surface_dirty = true;
                self.window.request_redraw();
                return;
            }
            Err(SurfaceError::Timeout) => {
                log::warn!("wgpu surface timeout; skipping frame");
                return;
            }
        };
        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("native-poc-encoder"),
            });

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [self.surface_config.width, self.surface_config.height],
            pixels_per_point: self.pixels_per_point,
        };

        for (id, image_delta) in &textures_delta.set {
            self.egui_renderer
                .update_texture(&self.device, &self.queue, *id, image_delta);
        }
        self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &paint_jobs,
            &screen_descriptor,
        );

        // Phase 4-H (FR12): build the per-cell input list against the
        // active tab's core + selection + theme, then hand it to
        // `TerminalGridPass::prepare`. The pass clears the swapchain to
        // the theme background and emits one instanced draw call for
        // every visible cell (background + glyph + decorations). egui
        // runs second with `LoadOp::Load` and draws the UI overlay
        // (tab bar / status bar / cursor / IME preedit) on top.
        //
        // Origin/metrics captured before `grid_pass.as_mut()` so the
        // mutable borrow doesn't conflict with `&self` on the metrics
        // call inside the branch (both go through `cell_metrics_px`).
        let (_, _, origin_x_px, origin_y_px) = self.cell_metrics_px(app);
        let prepared_grid = if let Some(pass) = self.grid_pass.as_mut() {
            // Theme is seeded from settings (font_size_pt + cursor
            // style) and then overlaid with the active tab's OSC
            // mutations when a tab is present, mirroring the layering
            // `render::draw_terminal` uses for the egui overlay.
            let theme = match app.active_tab() {
                Some(tab) => tab.theme.lock().clone(),
                None => crate::render::theme::Theme::from_settings(app.settings.as_ref()),
            };
            let width_mode = app.settings.ambiguous_width_mode;
            let cell_inputs = if let Some(tab) = app.active_tab() {
                let core = tab.core.lock();
                // Decide whether to bake a filled block cursor into the
                // grid: only when the cursor is terminal-visible, in
                // block style (style != underline/bar), and currently in
                // the "on" blink phase. Underline / bar shapes stay on
                // the egui overlay side and pass `None`.
                // Filled (reverse-video) block cursor is reserved for
                // the focused window — matches WezTerm, where an
                // unfocused window degrades to a hollow outline drawn
                // by `draw_cursor`. Underline / bar shapes always go
                // through the egui overlay and pass `None`.
                let cursor_style = core.get_cursor_style();
                let is_block_style = cursor_style != 1 && cursor_style != 2;
                let block_cursor_cell = if app.window_focused
                    && core.get_cursor_visible()
                    && is_block_style
                    && app.blink_visible_now(core.get_cursor_blink())
                {
                    Some((core.get_cursor_col(), core.get_cursor_row()))
                } else {
                    None
                };
                let mut inputs = crate::render::collect_cell_inputs(
                    &core,
                    &theme,
                    app.selection.as_ref(),
                    width_mode,
                    block_cursor_cell,
                );
                // IME preedit overlay (Phase 4-G): paint composition
                // glyphs inline at the anchor so the user can see what
                // they are typing. Without this only fcitx5's candidate
                // window hints at composition state.
                if tab.preedit_state.active() {
                    // No bg extension: glyph is clamped inside the
                    // cell rect by `fit_glyph_to_cell` so the
                    // reverse-video bg never has to spill into the
                    // next row to cover descenders.
                    crate::render::apply_preedit_overlay(
                        &mut inputs,
                        tab.preedit_state.anchor(),
                        tab.preedit_state.text(),
                        &theme,
                        core.cols(),
                        core.rows(),
                        0.0,
                    );
                }
                inputs
            } else {
                Vec::new()
            };
            // Cell metrics come from `App::cell_w_logical` /
            // `App::cell_h_logical` so the wgpu-rendered cells line up
            // with the egui-side cursor and preedit overlays. The
            // vertical origin reserves the same logical-px the tab bar
            // widget actually occupies (see
            // `crate::ui::tab_bar::TAB_BAR_HEIGHT`) plus the
            // `settings.padding` strip applied inside `cell_metrics_px`.
            //
            // HiDPI: the swapchain is sized in physical pixels while
            // `cell_w_logical` / `cell_h_logical` / `padding` are logical
            // pixels. egui scales its pass via `pixels_per_point` in
            // the `ScreenDescriptor`; we apply the same scale to every
            // length we hand wgpu (cell rect + origin + glyph
            // rasterize size) so cells line up with the egui-side
            // cursor / preedit on 2.0× hosts.
            let scale = self.pixels_per_point.max(1.0);
            // Origin already captured above and lines up with
            // `cell_metrics_px` so the status-bar top inset (when
            // configured) shifts cells down to sit below the panel —
            // otherwise the top row would paint behind the egui
            // status-bar.
            Some(pass.prepare(
                &self.device,
                &self.queue,
                &cell_inputs,
                crate::render::terminal_grid_pass::CellMetrics {
                    cell_w: app.cell_w_logical * scale,
                    cell_h: app.cell_h_logical * scale,
                    origin: [origin_x_px as f32, origin_y_px as f32],
                    // `theme.font_size_pt` is in CSS-compatible points;
                    // the rasterizer takes pixels, so apply the same
                    // `pt → px` conversion the legacy WebView build
                    // does (96/72). Without this the glyph atlas is
                    // built at ~75% of the cell size.
                    font_size_px: theme.font_size_px() * scale,
                },
                self.surface_config.width,
                self.surface_config.height,
            ))
        } else {
            None
        };

        {
            // Clear to the active theme's bg so the padding strip around
            // the cell grid (TOP_PAD / LEFT_PAD and the right/bottom
            // remainder rows) blends into the terminal background instead
            // of showing as a visible rim around the content.
            let theme_bg = match app.active_tab() {
                Some(tab) => tab.theme.lock().bg,
                None => crate::render::theme::Theme::from_settings(app.settings.as_ref()).bg,
            };
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("native-poc-terminal-grid-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: theme_bg.0 as f64 / 255.0,
                                g: theme_bg.1 as f64 / 255.0,
                                b: theme_bg.2 as f64 / 255.0,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    occlusion_query_set: None,
                    timestamp_writes: None,
                })
                .forget_lifetime();
            if let (Some(grid), Some(frame)) = (self.grid_pass.as_ref(), prepared_grid.as_ref()) {
                grid.draw(&mut pass, frame);
            }
        }

        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("native-poc-egui-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    occlusion_query_set: None,
                    timestamp_writes: None,
                })
                .forget_lifetime();
            self.egui_renderer
                .render(&mut pass, &paint_jobs, &screen_descriptor);
        }

        // Phase 5: image-overlay pass. Runs *after* egui with
        // `LoadOp::Load` so the terminal cells egui just drew underneath
        // are preserved, and the placement quads composit over them
        // using premultiplied alpha-blend (configured on the pipeline).
        if self.image_layer.state.placement_count() > 0 {
            let commands = self.overlay_pipeline.build_frame(
                &self.device,
                &self.queue,
                &self.image_layer,
                self.surface_config.width,
                self.surface_config.height,
            );
            if !commands.is_empty() {
                let mut pass = encoder
                    .begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("native-poc-image-overlay-pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        occlusion_query_set: None,
                        timestamp_writes: None,
                    })
                    .forget_lifetime();
                self.overlay_pipeline.draw(&mut pass, &commands);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();

        for id in &textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        // Sub-phase 2: snapshot cursor/selection, clear the core's dirty
        // bits, and drop the `needs_full_redraw` flag. The next frame will
        // be skipped entirely unless something dirties a row again. We
        // clone the `Arc<Mutex<TerminalCore>>` first so the immutable
        // borrow of `app` ends before the mutable `record_render_state`
        // call.
        let core_arc = app.active_tab().map(|t| t.core.clone());
        match core_arc {
            Some(arc) => {
                let mut core = arc.lock();
                app.record_render_state(&mut core);
            }
            None => app.record_render_state_no_tab(),
        }
    }

    /// Translate winit state into a minimal `egui::RawInput`. Phase 1 only
    /// needs screen-rect + pixels-per-point; later phases populate events.
    fn build_raw_input(&mut self) -> egui::RawInput {
        let size = self.window.inner_size();
        let logical = size.to_logical::<f32>(self.pixels_per_point as f64);
        egui::RawInput {
            viewport_id: ViewportId::ROOT,
            viewports: std::iter::once((
                ViewportId::ROOT,
                egui::ViewportInfo {
                    native_pixels_per_point: Some(self.pixels_per_point),
                    ..Default::default()
                },
            ))
            .collect(),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(logical.width, logical.height),
            )),
            time: None,
            predicted_dt: 1.0 / 60.0,
            modifiers: Default::default(),
            events: std::mem::take(&mut self.pending_egui_events),
            hovered_files: Vec::new(),
            dropped_files: Vec::new(),
            focused: true,
            max_texture_side: Some(8192),
            system_theme: None,
        }
    }
}

/// Translate a winit `MouseButton` to its `egui::PointerButton`
/// equivalent. Returns `None` for buttons egui does not model (e.g.
/// extra side buttons).
fn winit_to_egui_button(b: MouseButton) -> Option<egui::PointerButton> {
    match b {
        MouseButton::Left => Some(egui::PointerButton::Primary),
        MouseButton::Right => Some(egui::PointerButton::Secondary),
        MouseButton::Middle => Some(egui::PointerButton::Middle),
        _ => None,
    }
}

/// Translate a winit logical key into the `egui::Key` consumed by
/// `crate::ui::keybinds::dispatch` and `handle_special_chord`. Returns
/// `None` for keys that no chord can reference (the caller falls through
/// to PTY input).
///
/// The mapped set covers every main key the settings-driven keybind
/// parser can produce (`parse_main_key`): ASCII letters / digits, the
/// symbol keys, the navigation / editing named keys, and F1..F12.
fn winit_key_to_egui(logical: &WinitKey) -> Option<egui::Key> {
    match logical {
        WinitKey::Character(s) => {
            let mut chars = s.chars();
            let c = chars.next()?;
            let lower = c.to_ascii_lowercase();
            if lower.is_ascii_alphabetic() {
                // Allocation-free mapping — avoids a heap String per keystroke.
                return match lower {
                    'a' => Some(egui::Key::A),
                    'b' => Some(egui::Key::B),
                    'c' => Some(egui::Key::C),
                    'd' => Some(egui::Key::D),
                    'e' => Some(egui::Key::E),
                    'f' => Some(egui::Key::F),
                    'g' => Some(egui::Key::G),
                    'h' => Some(egui::Key::H),
                    'i' => Some(egui::Key::I),
                    'j' => Some(egui::Key::J),
                    'k' => Some(egui::Key::K),
                    'l' => Some(egui::Key::L),
                    'm' => Some(egui::Key::M),
                    'n' => Some(egui::Key::N),
                    'o' => Some(egui::Key::O),
                    'p' => Some(egui::Key::P),
                    'q' => Some(egui::Key::Q),
                    'r' => Some(egui::Key::R),
                    's' => Some(egui::Key::S),
                    't' => Some(egui::Key::T),
                    'u' => Some(egui::Key::U),
                    'v' => Some(egui::Key::V),
                    'w' => Some(egui::Key::W),
                    'x' => Some(egui::Key::X),
                    'y' => Some(egui::Key::Y),
                    'z' => Some(egui::Key::Z),
                    _ => None,
                };
            }
            match lower {
                '0' => Some(egui::Key::Num0),
                '1' => Some(egui::Key::Num1),
                '2' => Some(egui::Key::Num2),
                '3' => Some(egui::Key::Num3),
                '4' => Some(egui::Key::Num4),
                '5' => Some(egui::Key::Num5),
                '6' => Some(egui::Key::Num6),
                '7' => Some(egui::Key::Num7),
                '8' => Some(egui::Key::Num8),
                '9' => Some(egui::Key::Num9),
                '+' => Some(egui::Key::Plus),
                '-' => Some(egui::Key::Minus),
                ',' => Some(egui::Key::Comma),
                '.' => Some(egui::Key::Period),
                '/' => Some(egui::Key::Slash),
                '\\' => Some(egui::Key::Backslash),
                '=' => Some(egui::Key::Equals),
                ';' => Some(egui::Key::Semicolon),
                ':' => Some(egui::Key::Colon),
                _ => None,
            }
        }
        WinitKey::Named(named) => match named {
            NamedKey::Tab => Some(egui::Key::Tab),
            NamedKey::PageUp => Some(egui::Key::PageUp),
            NamedKey::PageDown => Some(egui::Key::PageDown),
            NamedKey::Home => Some(egui::Key::Home),
            NamedKey::End => Some(egui::Key::End),
            NamedKey::ArrowUp => Some(egui::Key::ArrowUp),
            NamedKey::ArrowDown => Some(egui::Key::ArrowDown),
            NamedKey::ArrowLeft => Some(egui::Key::ArrowLeft),
            NamedKey::ArrowRight => Some(egui::Key::ArrowRight),
            NamedKey::Enter => Some(egui::Key::Enter),
            NamedKey::Escape => Some(egui::Key::Escape),
            NamedKey::Backspace => Some(egui::Key::Backspace),
            NamedKey::Delete => Some(egui::Key::Delete),
            NamedKey::Insert => Some(egui::Key::Insert),
            NamedKey::Space => Some(egui::Key::Space),
            NamedKey::F1 => Some(egui::Key::F1),
            NamedKey::F2 => Some(egui::Key::F2),
            NamedKey::F3 => Some(egui::Key::F3),
            NamedKey::F4 => Some(egui::Key::F4),
            NamedKey::F5 => Some(egui::Key::F5),
            NamedKey::F6 => Some(egui::Key::F6),
            NamedKey::F7 => Some(egui::Key::F7),
            NamedKey::F8 => Some(egui::Key::F8),
            NamedKey::F9 => Some(egui::Key::F9),
            NamedKey::F10 => Some(egui::Key::F10),
            NamedKey::F11 => Some(egui::Key::F11),
            NamedKey::F12 => Some(egui::Key::F12),
            // F13–F20 are accepted by parse_main_key in keybinds.rs;
            // extend here so a configured F13–F20 chord can reach dispatch
            // at runtime instead of silently falling through to PTY input.
            NamedKey::F13 => Some(egui::Key::F13),
            NamedKey::F14 => Some(egui::Key::F14),
            NamedKey::F15 => Some(egui::Key::F15),
            NamedKey::F16 => Some(egui::Key::F16),
            NamedKey::F17 => Some(egui::Key::F17),
            NamedKey::F18 => Some(egui::Key::F18),
            NamedKey::F19 => Some(egui::Key::F19),
            NamedKey::F20 => Some(egui::Key::F20),
            _ => None,
        },
        _ => None,
    }
}

/// Extend egui's default font families with the bundled CJK and
/// outline-emoji fonts so the status bar (drawn with
/// `FontFamily::Monospace`) can render Japanese and pictographs
/// instead of tofu.
///
/// Two problems are addressed:
///
/// 1. egui's `FontDefinitions::default()` ships only `Hack` on the
///    `Monospace` family, which covers ASCII + Latin extensions but
///    no CJK. Any 日本語 in a `{cmd:…}` script's output would
///    therefore render as tofu.
/// 2. The same default registers BW emoji fonts on `Proportional`
///    only; pictographs (✅ 🟢 ☂ etc.) fall off the end of the
///    Monospace chain.
///
/// We register the bundled `NotoSansCJK-JP` and `NotoColorEmoji.ttf`
/// (already linked in via [`crate::render::font::resolver`] for the
/// terminal grid) and append them to both `Monospace` and
/// `Proportional` so other egui surfaces (tab bar, title bar)
/// inherit the same coverage.
///
/// Caveat: egui 0.29 / ab_glyph cannot raster color emoji
/// (CBDT / COLR v1) — for the emoji font only the *monochrome
/// outline* layer is reachable. Full color-emoji parity with the
/// WebView build requires switching the status-bar text path to a
/// swash-based custom painter, which is out of scope here.
fn configure_egui_emoji_fallback(ctx: &egui::Context) {
    use crate::render::font::resolver::{BUNDLED_CJK_FONT, BUNDLED_EMOJI_FONT};

    const CJK_KEY: &str = "EmtermBundledCJK";
    const EMOJI_KEY: &str = "EmtermBundledEmoji";

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        CJK_KEY.to_string(),
        egui::FontData::from_static(BUNDLED_CJK_FONT),
    );
    fonts.font_data.insert(
        EMOJI_KEY.to_string(),
        egui::FontData::from_static(BUNDLED_EMOJI_FONT),
    );

    for family in [egui::FontFamily::Monospace, egui::FontFamily::Proportional] {
        let chain = fonts.families.entry(family).or_default();
        for name in [CJK_KEY, EMOJI_KEY] {
            if !chain.iter().any(|n| n == name) {
                chain.push(name.to_string());
            }
        }
    }
    ctx.set_fonts(fonts);
}

/// Extract the OS-level physical key / scan code from a winit `KeyEvent`.
/// Phase 4-G-A captures it into [`RawKeyEvent`] so any future IME backend
/// can stash the original scan code without re-querying winit internals.
///
/// winit does not expose the raw scancode publicly on every platform, so
/// we hash the `PhysicalKey` debug representation as a stable stand-in.
/// The exact value is opaque to the App; backends that actually need a
/// real X11 keycode reconstruct it from their own platform layer. The
/// Phase 4-G-3 `WinitImeBridge` ignores this field — winit hands `KeyEvent`
/// directly through `dispatch_key_event_via_ime` if/when needed.
fn winit_physical_key_code(event: &KeyEvent) -> u32 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    format!("{:?}", event.physical_key).hash(&mut h);
    h.finish() as u32
}

/// Translate a winit `KeyEvent` into the PoC's `(Key, Modifiers)` pair and
/// produce the PTY byte sequence. Returns `None` for events that should be
/// ignored (e.g. modifier-only presses).
///
/// On winit the printable text of a key press is exposed via
/// `KeyEvent::text` (already UTF-8). For non-chord plain text (no
/// Ctrl/Alt held) we forward that string verbatim so layout-specific
/// glyphs, dead-key composition results, and shifted symbols all reach
/// the PTY. For chords (Ctrl+C, Alt+b) we go through the `encode`
/// path with the named-key dispatch table.
fn winit_key_to_bytes(event: &KeyEvent, mods: Modifiers) -> Option<Vec<u8>> {
    // Fast path for plain printable text — winit already accounts for the
    // current keyboard layout (X11 / Wayland / Win32). When IME is
    // composing, winit suppresses `text` and routes the result via
    // `WindowEvent::Ime` instead, so this branch never double-delivers.
    if !mods.ctrl && !mods.alt {
        if let Some(text) = &event.text {
            if !text.is_empty() {
                return Some(text.as_bytes().to_vec());
            }
        }
    }

    let key = match &event.logical_key {
        WinitKey::Character(s) => {
            let mut chars = s.chars();
            let c = chars.next()?;
            Key::Char(c)
        }
        WinitKey::Named(NamedKey::Enter) => Key::Enter,
        WinitKey::Named(NamedKey::Tab) => Key::Tab,
        WinitKey::Named(NamedKey::Backspace) => Key::Backspace,
        WinitKey::Named(NamedKey::Escape) => Key::Escape,
        WinitKey::Named(NamedKey::Space) => Key::Char(' '),
        WinitKey::Named(NamedKey::ArrowUp) => Key::Up,
        WinitKey::Named(NamedKey::ArrowDown) => Key::Down,
        WinitKey::Named(NamedKey::ArrowLeft) => Key::Left,
        WinitKey::Named(NamedKey::ArrowRight) => Key::Right,
        WinitKey::Named(NamedKey::Home) => Key::Home,
        WinitKey::Named(NamedKey::End) => Key::End,
        WinitKey::Named(NamedKey::PageUp) => Key::PageUp,
        WinitKey::Named(NamedKey::PageDown) => Key::PageDown,
        WinitKey::Named(NamedKey::Delete) => Key::Delete,
        WinitKey::Named(NamedKey::Insert) => Key::Insert,
        WinitKey::Named(NamedKey::F1) => Key::F(1),
        WinitKey::Named(NamedKey::F2) => Key::F(2),
        WinitKey::Named(NamedKey::F3) => Key::F(3),
        WinitKey::Named(NamedKey::F4) => Key::F(4),
        WinitKey::Named(NamedKey::F5) => Key::F(5),
        WinitKey::Named(NamedKey::F6) => Key::F(6),
        WinitKey::Named(NamedKey::F7) => Key::F(7),
        WinitKey::Named(NamedKey::F8) => Key::F(8),
        WinitKey::Named(NamedKey::F9) => Key::F(9),
        WinitKey::Named(NamedKey::F10) => Key::F(10),
        WinitKey::Named(NamedKey::F11) => Key::F(11),
        WinitKey::Named(NamedKey::F12) => Key::F(12),
        _ => return None,
    };
    let bytes = encode(key, mods);
    if bytes.is_empty() {
        None
    } else {
        Some(bytes)
    }
}

/// `ApplicationHandler` impl driving the App + WindowHost on winit 0.30.
///
/// winit 0.30 replaced the closure-based event-loop API with the
/// `ApplicationHandler` trait. `resumed` creates the window the first
/// time the platform is ready, `window_event` mirrors what used to be
/// the inner `match event` arm, and `about_to_wait` does the periodic
/// pump (PTY drain, IME pump, cursor-rect notification) that the old
/// `StartCause::Poll` path handled.
struct PocApp {
    app: App,
    host: Option<WindowHost>,
}

impl ApplicationHandler for PocApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.host.is_some() {
            // Re-entering `Resumed` is an Android lifecycle artifact;
            // desktop winit only fires this once at startup. Keep the
            // existing host so a stray resume does not reinitialize the
            // surface (the PoC has no Android target).
            return;
        }
        let image_quota_bytes = (self.app.settings.image_memory_quota_mb as u64) * 1024 * 1024;
        let mut host = WindowHost::new(event_loop, image_quota_bytes);

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
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
        self.host = Some(host);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
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
                self.app.tabs.clear();
                // Drop the wgpu Surface (and the rest of WindowHost) while
                // winit's EventLoop is still alive. The Vulkan WSI surface
                // is tied to the X11 display connection that EventLoop
                // owns; if we let WindowHost outlive the EventLoop, the
                // surface destructor calls into a freed display and
                // segfaults. Same reason applies to the egui-wgpu
                // Renderer, ImageLayer textures, and the Window arc.
                self.host = None;
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
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
            }
            // Phase 4-G-3: winit surfaces composition events via
            // `WindowEvent::Ime { Enabled, Preedit, Commit, Disabled }`.
            // Route them to the active backend; `WinitImeBridge`
            // translates each variant into `ImeEvent`s consumed by
            // `App::pump_ime` on the next tick. `NullBackend`
            // overrides the trait default with a no-op, so this is
            // safe to call unconditionally.
            WindowEvent::Ime(ime) => {
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
                } else {
                    // Drop the user back into the cursor's "on" half-
                    // cycle on focus regain so the filled block appears
                    // immediately instead of waiting up to 530 ms for
                    // the next blink boundary.
                    self.app.reset_blink_phase();
                }
                // Cursor shape switches between filled (focused) and
                // outline (unfocused), so we need a repaint on every
                // focus transition.
                host.window().request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
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
                if !handled {
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
                        let _ = self.app.apply_action(act);
                        self.app.mark_full_redraw();
                    } else {
                        // `shift_enter_as_alt_enter`: when the user has
                        // opted in, present `Shift+Enter` to the shell
                        // as `Alt+Enter` (M-RET) so editor multi-line
                        // continuation bindings fire. Only the bare
                        // Shift-on-Enter case is rewritten — Ctrl/Alt
                        // already pass through unchanged.
                        let mut mods = host.current_mods;
                        if self.app.settings.shift_enter_as_alt_enter
                            && mods.shift
                            && !mods.ctrl
                            && !mods.alt
                            && matches!(event.logical_key, WinitKey::Named(NamedKey::Enter))
                        {
                            mods.shift = false;
                            mods.alt = true;
                        }
                        if let Some(bytes) = winit_key_to_bytes(&event, mods) {
                            if let Some(tab) = self.app.active_tab() {
                                tab.write(bytes);
                            }
                        }
                    }
                }
                host.window().request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Released => {
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
            WindowEvent::CursorLeft { .. } => {
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
                    host.window.set_cursor(CursorIcon::Default);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                host.cursor_pos = position;
                // Forward to egui so the tab bar / status bar widgets
                // observe hover + drag motion.
                let logical = position.to_logical::<f32>(host.pixels_per_point as f64);
                let egui_pos = egui::pos2(logical.x, logical.y);
                host.pending_egui_events
                    .push(egui::Event::PointerMoved(egui_pos));
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
                }
                host.window().request_redraw();
                if host.dragging {
                    let (row, col) = host.pixel_to_cell(position, &self.app);
                    // First motion since the press in Character mode
                    // upgrades the pending click into a real Selection.
                    // Word / line selections (double / triple click)
                    // were already committed at press time and the
                    // pending_press_cell was cleared there.
                    if self.app.selection.is_none() {
                        if let Some(anchor) = host.pending_press_cell.take() {
                            self.app.selection =
                                Some(Selection::new_with_mode(anchor, SelectionMode::Character));
                        }
                    }
                    if let Some(sel) = self.app.selection.as_mut() {
                        if let Some(tab) = self.app.tabs.get(self.app.active) {
                            let core = tab.core.lock();
                            sel.extend(Pos { row, col }, &core);
                        }
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
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
                host.window().request_redraw();

                // Clicks that land on the egui-owned strip (CSD title
                // bar + tab bar at the top, status bar at the bottom
                // when enabled) must not also kick off a terminal
                // selection — otherwise pressing the × on a tab (or
                // the close button on the title bar) would
                // simultaneously start a selection on the cell behind
                // it.
                let top_strip_h = crate::ui::title_bar::TITLE_BAR_HEIGHT
                    + crate::ui::tab_bar::effective_tab_bar_height(self.app.settings.show_tab_bar);
                let if_in_egui_strip = egui_pos.y < top_strip_h;
                if if_in_egui_strip {
                    return;
                }

                match (button, state) {
                    (MouseButton::Left, ElementState::Pressed) => {
                        let (row, col) = host.pixel_to_cell(host.cursor_pos, &self.app);
                        let cls = host.click_tracker.classify(Instant::now(), row, col);
                        if cls.mode == SelectionMode::Character {
                            // Single click in character mode: do not
                            // materialize a one-cell selection yet — the
                            // user may just be moving the cursor / focus
                            // / clearing a prior selection. Record the
                            // press cell so the first motion (if any)
                            // can upgrade this into a real drag-select.
                            self.app.selection = None;
                            host.pending_press_cell = Some(Pos { row, col });
                            host.window().request_redraw();
                        } else {
                            // Word (double click) / line (triple click)
                            // commit immediately so a static click still
                            // selects the targeted word or line.
                            let mut sel = Selection::new_with_mode(Pos { row, col }, cls.mode);
                            if let Some(tab) = self.app.tabs.get(self.app.active) {
                                let core = tab.core.lock();
                                sel.extend(Pos { row, col }, &core);
                            }
                            self.app.selection = Some(sel);
                            host.pending_press_cell = None;
                        }
                        host.dragging = true;
                    }
                    (MouseButton::Left, ElementState::Released) => {
                        host.dragging = false;
                        // A press with no motion in Character mode left
                        // selection == None (see the Pressed branch);
                        // there is nothing to copy in that case.
                        host.pending_press_cell = None;
                        if let Some(sel) = self.app.selection {
                            if let Some(tab) = self.app.tabs.get(self.app.active) {
                                let core = tab.core.lock();
                                let text = sel.resolve(&core);
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
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => {
                        let (_, cell_h_px, _, _) = host.cell_metrics_px(&self.app);
                        (p.y as f32) / (cell_h_px.max(1.0) as f32)
                    }
                };
                // `settings.scroll_speed` is clamped to 1..=10 by the
                // loader, so it's safe to feed directly into the scroll
                // helpers (a runaway typo can't fly the viewport 1000
                // rows per notch).
                let step = self.app.settings.scroll_speed.max(1);
                if lines > 0.0 {
                    self.app.scroll_up_by(step);
                    host.window().request_redraw();
                } else if lines < 0.0 {
                    self.app.scroll_down_by(step);
                    host.window().request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                host.render(&mut self.app);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
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
        // Cursor blink advances on a 530 ms half-cycle (BLINK_HALF_MS).
        // egui's request_repaint_after is silent (no callback bridges
        // it back to winit), so we have to detect the phase flip
        // ourselves and request a redraw — otherwise the cursor freezes
        // at whatever phase the last paint landed on.
        let blink_due = self.app.needs_blink_repaint();
        if ime_changed || pty_changed || blink_due {
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
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
    }

    /// Phase E (TS-32): winit `EventLoopProxy::send_event(())` calls land
    /// here. Without this override, the trait-default `user_event` is a
    /// no-op and the provider-owned wake chain (`TimeProvider` timer
    /// thread → `WakeFn` → `EventLoopProxy::send_event(())` → here →
    /// `request_redraw`) is silently broken, freezing the status-bar
    /// clock when the shell is idle.
    ///
    /// Defensive: if `self.host` is `None` (we are between
    /// `Resumed`-time construction failure and process exit, or already
    /// torn down in `CloseRequested`), this is a no-op.
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        request_redraw_on_user_event(self.host.as_ref(), |host| {
            host.window().request_redraw();
        });
    }

    /// winit calls this once after `event_loop.exit()` and before
    /// `run_app` returns. Use it as a defense-in-depth shutdown step
    /// for any code path that flagged exit without zeroing `self.host`
    /// (e.g. future error-path exits). The Vulkan / X11 teardown must
    /// happen while EventLoop is still alive — see the field-order
    /// note on `WindowHost`.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if self.host.is_some() {
            log::info!("native-poc: exiting handler dropping WindowHost");
            self.host = None;
        }
    }
}

/// Classify a pointer position against a window of the given logical
/// size as one of the eight CSD resize directions, or `None` when the
/// pointer is inside the interior (away from every edge by at least
/// `edge_px`) or outside the window entirely.
///
/// Pure over the inputs so the resize hot-zone math can be unit-tested
/// without instantiating a real `winit::Window`. The caller layers the
/// "maximized → never resize" rule on top — that condition is not
/// expressible in terms of the geometry alone.
fn classify_resize_edge(
    width: f32,
    height: f32,
    x: f32,
    y: f32,
    edge_px: f32,
) -> Option<ResizeDirection> {
    // Reject negative coords (Wayland delivers them briefly on pointer
    // leave) and positions past the far edge so we never latch a
    // phantom direction with the pointer outside the window.
    if x < 0.0 || y < 0.0 || x > width || y > height {
        return None;
    }
    let near_left = x < edge_px;
    let near_right = x > width - edge_px;
    let near_top = y < edge_px;
    let near_bottom = y > height - edge_px;
    use ResizeDirection::*;
    match (near_top, near_bottom, near_left, near_right) {
        (true, _, true, _) => Some(NorthWest),
        (true, _, _, true) => Some(NorthEast),
        (_, true, true, _) => Some(SouthWest),
        (_, true, _, true) => Some(SouthEast),
        (true, _, _, _) => Some(North),
        (_, true, _, _) => Some(South),
        (_, _, true, _) => Some(West),
        (_, _, _, true) => Some(East),
        _ => None,
    }
}

/// Pure-logic decision for `PocApp::user_event` (TS-32).
///
/// Extracted as a free function so unit tests can exercise the
/// "redraw if host is present, no-op otherwise" contract without
/// instantiating a real winit window (which requires an active
/// event loop and a display). The `redraw` callback is invoked at
/// most once, and only when `host` is `Some`.
fn request_redraw_on_user_event<H, F>(host: Option<&H>, redraw: F)
where
    F: FnOnce(&H),
{
    if let Some(h) = host {
        redraw(h);
    }
}

/// Run the event loop until the window is closed. Owns the App.
pub fn run(event_loop: EventLoop<()>, app: App) -> ! {
    let mut handler = PocApp { app, host: None };
    if let Err(e) = event_loop.run_app(&mut handler) {
        log::error!("native-poc: winit event loop returned an error: {e}");
    }
    // Drop the PTY-owning tabs explicitly before the process exits so
    // reader/writer threads can shut down cleanly; without this they
    // outlive `main` and produce noisy platform-specific cleanup
    // warnings.
    drop(handler);
    std::process::exit(0);
}

/// Intercept Phase 4 chords. Returns `true` when the event was consumed
/// (the generic encoder should not run).
///
/// `egui_key` is the pre-computed result of `winit_key_to_egui` for this
/// event, passed in so the caller can reuse the same value for the
/// keybinds dispatch path without a second translation.
fn handle_special_chord(
    event: &KeyEvent,
    mods: Modifiers,
    egui_key: Option<egui::Key>,
    host: &mut WindowHost,
    app: &mut App,
) -> bool {
    // Clipboard chords are settings-driven (`keybinds.copy` /
    // `keybinds.paste`). Build the incoming chord from the winit event +
    // modifiers and compare against the resolved table.
    if let Some(key) = egui_key {
        let chord = Chord {
            ctrl: mods.ctrl,
            shift: mods.shift,
            alt: mods.alt,
            key,
        };
        if chord == app.keybinds.copy {
            // Copy current selection to CLIPBOARD. We consume the chord
            // even when there is no active selection so the configured
            // copy key never leaks through to the PTY.
            if let Some(sel) = app.selection {
                if let Some(tab) = app.tabs.get(app.active) {
                    let core = tab.core.lock();
                    let text = sel.resolve(&core);
                    drop(core);
                    host.set_clipboard(&text);
                }
            }
            return true;
        }
        if chord == app.keybinds.paste {
            if let Some(text) = host.get_clipboard() {
                host.deliver_paste(app, &text);
            }
            return true;
        }
    }

    // Scrollback chords use Shift + nav keys.
    if mods.shift && !mods.ctrl && !mods.alt {
        match &event.logical_key {
            WinitKey::Named(NamedKey::PageUp) => {
                let rows = app.cell_size.rows.max(1) as u32;
                app.scroll_up_by(rows);
                return true;
            }
            WinitKey::Named(NamedKey::PageDown) => {
                let rows = app.cell_size.rows.max(1) as u32;
                app.scroll_down_by(rows);
                return true;
            }
            WinitKey::Named(NamedKey::Home) => {
                app.scroll_to_top();
                return true;
            }
            WinitKey::Named(NamedKey::End) => {
                app.scroll_to_live();
                return true;
            }
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn click_classifier_single_click_is_character() {
        let mut t = ClickTracker::default();
        let now = Instant::now();
        let cls = t.classify(now, 5, 10);
        assert_eq!(cls.count, 1);
        assert_eq!(cls.mode, SelectionMode::Character);
    }

    #[test]
    fn click_classifier_double_click_within_window_at_same_cell() {
        let mut t = ClickTracker::default();
        let t0 = Instant::now();
        let _ = t.classify(t0, 5, 10);
        let t1 = t0 + Duration::from_millis(200);
        let cls = t.classify(t1, 5, 10);
        assert_eq!(cls.count, 2);
        assert_eq!(cls.mode, SelectionMode::Word);
    }

    #[test]
    fn click_classifier_triple_click_at_same_cell() {
        let mut t = ClickTracker::default();
        let t0 = Instant::now();
        let _ = t.classify(t0, 5, 10);
        let _ = t.classify(t0 + Duration::from_millis(100), 5, 10);
        let cls = t.classify(t0 + Duration::from_millis(200), 5, 10);
        assert_eq!(cls.count, 3);
        assert_eq!(cls.mode, SelectionMode::Line);
    }

    #[test]
    fn click_classifier_resets_after_triple() {
        let mut t = ClickTracker::default();
        let t0 = Instant::now();
        let _ = t.classify(t0, 5, 10);
        let _ = t.classify(t0 + Duration::from_millis(100), 5, 10);
        let _ = t.classify(t0 + Duration::from_millis(200), 5, 10);
        // Fourth click within window collapses back to Character.
        let cls = t.classify(t0 + Duration::from_millis(300), 5, 10);
        assert_eq!(cls.count, 1);
        assert_eq!(cls.mode, SelectionMode::Character);
    }

    #[test]
    fn click_classifier_resets_when_position_changes() {
        let mut t = ClickTracker::default();
        let t0 = Instant::now();
        let _ = t.classify(t0, 5, 10);
        let cls = t.classify(t0 + Duration::from_millis(100), 5, 11);
        // Different cell → back to single click.
        assert_eq!(cls.count, 1);
        assert_eq!(cls.mode, SelectionMode::Character);
    }

    #[test]
    fn click_classifier_resets_when_window_expires() {
        let mut t = ClickTracker::default();
        let t0 = Instant::now();
        let _ = t.classify(t0, 5, 10);
        // 600 ms > MULTI_CLICK_WINDOW_MS (500 ms) → reset.
        let cls = t.classify(t0 + Duration::from_millis(600), 5, 10);
        assert_eq!(cls.count, 1);
        assert_eq!(cls.mode, SelectionMode::Character);
    }

    /// TS-32 (host=Some): `PocApp::user_event` must call `request_redraw`
    /// on the active window exactly once. We exercise the extracted
    /// `request_redraw_on_user_event` helper because constructing a
    /// real `winit::Window` here would require an active event loop +
    /// display, which is unavailable in `cargo test`.
    ///
    /// A `Cell<u32>` counter stands in for the winit window's
    /// `request_redraw()` side effect. Without the `user_event`
    /// override the provider-owned wake chain (`WakeFn` →
    /// `EventLoopProxy::send_event(())` → `user_event`) was silently
    /// dropped, freezing the status-bar clock on idle (release-build
    /// regression observed twice during sdd.6-verify).
    #[test]
    fn user_event_dispatches_redraw_when_host_present() {
        use std::cell::Cell;
        let redraws: Cell<u32> = Cell::new(0);
        let host_stub: u8 = 0;
        request_redraw_on_user_event(Some(&host_stub), |_| {
            redraws.set(redraws.get() + 1);
        });
        assert_eq!(redraws.get(), 1);
    }

    #[test]
    fn resize_edge_interior_is_none() {
        // Dead-center of a 800×600 window: nowhere near any edge.
        assert_eq!(classify_resize_edge(800.0, 600.0, 400.0, 300.0, 6.0), None);
    }

    #[test]
    fn resize_edge_corners_classify_to_diagonals() {
        use ResizeDirection::*;
        // Each corner pixel grabs the diagonal direction so the user
        // can resize width + height together.
        assert_eq!(
            classify_resize_edge(800.0, 600.0, 1.0, 1.0, 6.0),
            Some(NorthWest)
        );
        assert_eq!(
            classify_resize_edge(800.0, 600.0, 799.0, 1.0, 6.0),
            Some(NorthEast)
        );
        assert_eq!(
            classify_resize_edge(800.0, 600.0, 1.0, 599.0, 6.0),
            Some(SouthWest)
        );
        assert_eq!(
            classify_resize_edge(800.0, 600.0, 799.0, 599.0, 6.0),
            Some(SouthEast)
        );
    }

    #[test]
    fn resize_edge_sides_classify_to_cardinals() {
        use ResizeDirection::*;
        // Mid-edge sample on each of the four sides.
        assert_eq!(
            classify_resize_edge(800.0, 600.0, 400.0, 1.0, 6.0),
            Some(North)
        );
        assert_eq!(
            classify_resize_edge(800.0, 600.0, 400.0, 599.0, 6.0),
            Some(South)
        );
        assert_eq!(
            classify_resize_edge(800.0, 600.0, 1.0, 300.0, 6.0),
            Some(West)
        );
        assert_eq!(
            classify_resize_edge(800.0, 600.0, 799.0, 300.0, 6.0),
            Some(East)
        );
    }

    #[test]
    fn resize_edge_outside_window_is_none() {
        // Wayland can deliver negative or past-edge coords during
        // pointer leave; both must yield `None` so the hot-zone
        // cache doesn't latch a stale direction.
        assert_eq!(classify_resize_edge(800.0, 600.0, -1.0, 300.0, 6.0), None);
        assert_eq!(classify_resize_edge(800.0, 600.0, 400.0, 700.0, 6.0), None);
    }

    /// TS-32 (host=None): before `Resumed` constructs the `WindowHost`
    /// or after `CloseRequested` tears it down, `self.host` is `None`.
    /// In that window `user_event` must be a no-op rather than panic.
    #[test]
    fn user_event_is_noop_when_host_absent() {
        use std::cell::Cell;
        let redraws: Cell<u32> = Cell::new(0);
        let host: Option<&u8> = None;
        request_redraw_on_user_event(host, |_| {
            redraws.set(redraws.get() + 1);
        });
        assert_eq!(redraws.get(), 0);
    }

    /// Verify that winit_key_to_egui covers every function key F1..=F20.
    ///
    /// parse_main_key in keybinds.rs accepts F1..=F20 as valid chord keys.
    /// This test keeps the two domains in sync: if either side drifts, this
    /// test will catch it before a user-configured F13–F20 shortcut silently
    /// falls through to PTY input at runtime.
    #[test]
    fn winit_key_to_egui_covers_f1_through_f20() {
        let pairs: &[(WinitKey, egui::Key)] = &[
            (WinitKey::Named(NamedKey::F1), egui::Key::F1),
            (WinitKey::Named(NamedKey::F2), egui::Key::F2),
            (WinitKey::Named(NamedKey::F3), egui::Key::F3),
            (WinitKey::Named(NamedKey::F4), egui::Key::F4),
            (WinitKey::Named(NamedKey::F5), egui::Key::F5),
            (WinitKey::Named(NamedKey::F6), egui::Key::F6),
            (WinitKey::Named(NamedKey::F7), egui::Key::F7),
            (WinitKey::Named(NamedKey::F8), egui::Key::F8),
            (WinitKey::Named(NamedKey::F9), egui::Key::F9),
            (WinitKey::Named(NamedKey::F10), egui::Key::F10),
            (WinitKey::Named(NamedKey::F11), egui::Key::F11),
            (WinitKey::Named(NamedKey::F12), egui::Key::F12),
            (WinitKey::Named(NamedKey::F13), egui::Key::F13),
            (WinitKey::Named(NamedKey::F14), egui::Key::F14),
            (WinitKey::Named(NamedKey::F15), egui::Key::F15),
            (WinitKey::Named(NamedKey::F16), egui::Key::F16),
            (WinitKey::Named(NamedKey::F17), egui::Key::F17),
            (WinitKey::Named(NamedKey::F18), egui::Key::F18),
            (WinitKey::Named(NamedKey::F19), egui::Key::F19),
            (WinitKey::Named(NamedKey::F20), egui::Key::F20),
        ];
        for (winit_key, expected) in pairs {
            assert_eq!(
                winit_key_to_egui(winit_key),
                Some(*expected),
                "winit_key_to_egui({winit_key:?}) did not return {expected:?}"
            );
        }
    }
}
