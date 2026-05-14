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
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::app::App;
use crate::image::overlay::OverlayPipeline;
use crate::image::ImageLayer;
use crate::ime::backend::{build_backend, KeyDispatchResult, ProcessEnv, RawKeyEvent};
use crate::pty::input::{encode, Key, Modifiers};
use crate::selection::{Pos, Selection, SelectionMode};

/// Fallback cell size in physical pixels. Phase 4 replaces this with a real
/// font-metrics-derived value.
const FALLBACK_CELL_W: u32 = 9;
const FALLBACK_CELL_H: u32 = 18;

/// Maximum time between successive clicks that still counts as a "multi-click".
/// Within this window the click counter increments; beyond it the counter
/// resets to 1. 500 ms matches xterm's `multiClickTime` default.
const MULTI_CLICK_WINDOW_MS: u128 = 500;

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
pub struct WindowHost {
    window: Arc<Window>,
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    egui_ctx: egui::Context,
    egui_renderer: egui_wgpu::Renderer,
    pixels_per_point: f32,
    /// True when the surface must be recreated on the next frame (e.g. after
    /// `SurfaceError::Lost`).
    surface_dirty: bool,
    current_mods: Modifiers,
    /// Last cursor position in physical pixels (updated on `CursorMoved`).
    cursor_pos: PhysicalPosition<f64>,
    /// Whether a left-button drag is in progress.
    dragging: bool,
    /// Click tracker for double / triple click detection.
    click_tracker: ClickTracker,
    /// Lazily-initialized arboard clipboard. We only fail-loud once if the
    /// platform clipboard cannot be acquired (X11 without display, etc.).
    clipboard: Option<arboard::Clipboard>,
    /// Phase 5: inline-image overlay (Kitty Graphics + SIXEL). Single
    /// instance shared by all tabs — the per-tab `ImageProcessor` lives in
    /// `Tab::image_proc` and produces `ImageEvent`s which are forwarded
    /// here once per frame via `Tab::drain_image_events`.
    image_layer: ImageLayer,
    /// Reusable wgpu pipeline that draws every visible placement after
    /// the egui pass on the same swapchain texture (`LoadOp::Load`).
    overlay_pipeline: OverlayPipeline,
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

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
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
            current_mods: Modifiers::NONE,
            cursor_pos: PhysicalPosition::new(0.0, 0.0),
            dragging: false,
            click_tracker: ClickTracker::default(),
            clipboard,
            image_layer,
            overlay_pipeline,
        }
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    /// Hand a clone of the `Arc<Window>` to callers that need to retain
    /// the handle themselves (Phase 4-G-3 passes this to
    /// `WinitImeBridge::init` so the bridge can call
    /// `Window::set_ime_cursor_area`).
    #[allow(dead_code)]
    pub fn window_arc(&self) -> Arc<Window> {
        self.window.clone()
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

    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        self.surface_config.width = new_size.width;
        self.surface_config.height = new_size.height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    /// Compute grid (cols, rows) from the current window pixel size using a
    /// fallback cell size. Phase 4 replaces this with the real font metrics.
    pub fn grid_size(&self) -> (u16, u16) {
        let w = self.surface_config.width.max(1);
        let h = self.surface_config.height.max(1);
        // Reserve ~36 px for the top bar.
        let usable_h = h.saturating_sub(36).max(FALLBACK_CELL_H);
        let cols = (w / FALLBACK_CELL_W).clamp(20, 500) as u16;
        let rows = (usable_h / FALLBACK_CELL_H).clamp(5, 200) as u16;
        (cols, rows)
    }

    /// Map a physical pixel position to a grid cell `(row, col)`. The top
    /// bar reserved by `grid_size` accounts for `~36 px`; we use the same
    /// offset so the cursor lands on the visually-correct row.
    fn pixel_to_cell(&self, pos: PhysicalPosition<f64>, app: &App) -> (u16, u16) {
        let top_bar_px = 36.0_f64;
        let x = (pos.x.max(0.0)) / (FALLBACK_CELL_W as f64);
        let y = ((pos.y - top_bar_px).max(0.0)) / (FALLBACK_CELL_H as f64);
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
        // Phase 0: lazy first-frame configure + recovery from Lost/Outdated.
        // `surface_dirty` is true on construction (deferred configure) and
        // whenever a previous frame returned `Lost` / `Outdated`. We
        // reconfigure with the current physical size before acquiring the
        // next swapchain texture.
        let was_surface_dirty = self.surface_dirty;
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
        // the renderer uses FALLBACK_CELL_W/H (Phase 4 placeholder).
        self.image_layer
            .recompute_pixel_dims(FALLBACK_CELL_W, FALLBACK_CELL_H);

        // Sub-phase 2 dirty-row diff: skip the entire egui+wgpu cycle when
        // nothing in the active tab needs to repaint. The first frame
        // (or any frame that follows a surface reconfigure) bypasses this
        // skip because `App::mark_full_redraw()` forces the dirty set to
        // the full row range. Phase 5: also bypass when there are pending
        // image events to draw or any placements (re-paint over text).
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
            if matches!(dirty_count, Some(0)) && self.image_layer.state.placement_count() == 0 {
                return;
            }
        }

        let raw_input = self.build_raw_input();
        let mut tab_event: Option<crate::ui::TabEvent> = None;
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            tab_event = crate::render::draw_placeholder(ctx, app);
        });
        // Apply any tab bar interaction emitted this frame. Closing
        // the last tab returns `true` and the next event loop tick
        // observes `app.tabs.is_empty()` to exit the window.
        if let Some(evt) = tab_event {
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

        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("native-poc-egui-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.05,
                                g: 0.05,
                                b: 0.05,
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
    fn build_raw_input(&self) -> egui::RawInput {
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
            events: Vec::new(),
            hovered_files: Vec::new(),
            dropped_files: Vec::new(),
            focused: true,
            max_texture_side: Some(8192),
            system_theme: None,
        }
    }
}

/// Translate a winit logical key into the subset of `egui::Key` consumed
/// by `crate::ui::keybinds::dispatch`. Returns `None` for keys that the
/// dispatcher does not bind (the caller falls through to PTY input).
///
/// Only the keys referenced by the Phase 4-B keybind table are mapped
/// (T, W, Tab, Num0..Num9); everything else is intentionally unmapped
/// so the PTY passthrough path stays the default.
fn winit_key_to_egui(logical: &WinitKey) -> Option<egui::Key> {
    match logical {
        WinitKey::Named(NamedKey::Tab) => Some(egui::Key::Tab),
        WinitKey::Character(s) => {
            let mut chars = s.chars();
            let c = chars.next()?;
            match c.to_ascii_lowercase() {
                't' => Some(egui::Key::T),
                'w' => Some(egui::Key::W),
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
                _ => None,
            }
        }
        _ => None,
    }
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
        let host = WindowHost::new(event_loop, image_quota_bytes);

        // Push the initial grid size into the App before the first tab spawn.
        let (cols, rows) = host.grid_size();
        self.app.cell_size = crate::app::GridDims { cols, rows };
        self.app.spawn_initial_tab();

        // Phase 4-G: resolve the IME backend now that the winit window
        // (and therefore the raw-window/display handle pair) exists.
        // Phase 4-G-1 reduced the factory to always return Unavailable
        // → NullBackend; Phase 4-G-3 swaps in the WinitImeBridge once
        // it lands.
        let window_handle = host.window().window_handle().ok().map(|h| h.as_raw());
        let display_handle = host.window().display_handle().ok().map(|h| h.as_raw());
        let backend = build_backend(
            window_handle,
            display_handle,
            &self.app.settings.ime,
            &ProcessEnv,
        );
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
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                host.resize(new_size);
                let (cols, rows) = host.grid_size();
                self.app.set_grid_size(cols, rows);
                host.window().request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                host.pixels_per_point = scale_factor as f32;
                let size = host.window().inner_size();
                host.resize(size);
                let (cols, rows) = host.grid_size();
                self.app.set_grid_size(cols, rows);
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
            // Phase 4-G-2: winit no longer surfaces a `ReceivedImeText`
            // variant; printable text is delivered via `KeyEvent::text`
            // (handled in `winit_key_to_bytes`). For composition winit
            // emits `WindowEvent::Ime { Preedit, Commit, Disabled }`,
            // which Phase 4-G-3's `WinitImeBridge` will route into the
            // existing `on_ime_*` plumbing. Until that phase lands we
            // ignore IME events here so NullBackend stays passive.
            WindowEvent::Ime(_) => {
                // Phase 4-G-3 hook point — left as a no-op until the
                // WinitImeBridge lands. NullBackend cannot do anything
                // useful with these events.
            }
            // Focus loss / window deactivation → clear any in-progress
            // preedit overlay so a stale composition doesn't ghost the
            // cursor after the user tabs away. Also forward focus
            // state to the IME backend so it can disable/enable IME on
            // the IM-server side.
            WindowEvent::Focused(focused) => {
                self.app.notify_ime_focus(focused);
                if !focused {
                    self.app.on_ime_focus_lost();
                }
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

                // Phase 4 chords intercept the generic encoder path:
                //   Ctrl+Shift+C  → copy current selection to CLIPBOARD
                //   Ctrl+Shift+V  → paste CLIPBOARD into PTY (bracketed if 2004)
                //   Shift+PageUp  → scroll back one page
                //   Shift+PageDown → scroll forward one page
                //   Shift+Home    → scroll to top of scrollback
                //   Shift+End     → scroll back to live tail
                let handled = handle_special_chord(&event, host.current_mods, host, &mut self.app);
                if !handled {
                    // Phase 4-B: global keybinds (tab roster) take
                    // priority over the generic PTY encoder.
                    let egui_mods = egui::Modifiers {
                        ctrl: host.current_mods.ctrl,
                        shift: host.current_mods.shift,
                        alt: host.current_mods.alt,
                        command: false,
                        mac_cmd: false,
                    };
                    let action = winit_key_to_egui(&event.logical_key)
                        .and_then(|k| crate::ui::keybinds::dispatch(egui_mods, k));
                    if let Some(act) = action {
                        let _ = self.app.apply_action(act);
                        self.app.mark_full_redraw();
                    } else if let Some(bytes) = winit_key_to_bytes(&event, host.current_mods) {
                        if let Some(tab) = self.app.active_tab() {
                            tab.write(bytes);
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
            WindowEvent::CursorMoved { position, .. } => {
                host.cursor_pos = position;
                if host.dragging {
                    let (row, col) = host.pixel_to_cell(position, &self.app);
                    if let Some(sel) = self.app.selection.as_mut() {
                        if let Some(tab) = self.app.tabs.get(self.app.active) {
                            let core = tab.core.lock();
                            sel.extend(Pos { row, col }, &core);
                        }
                    }
                    host.window().request_redraw();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => match (button, state) {
                (MouseButton::Left, ElementState::Pressed) => {
                    let (row, col) = host.pixel_to_cell(host.cursor_pos, &self.app);
                    let cls = host.click_tracker.classify(Instant::now(), row, col);
                    let mut sel = Selection::new_with_mode(Pos { row, col }, cls.mode);
                    if cls.mode != SelectionMode::Character {
                        if let Some(tab) = self.app.tabs.get(self.app.active) {
                            let core = tab.core.lock();
                            sel.extend(Pos { row, col }, &core);
                        }
                    }
                    self.app.selection = Some(sel);
                    host.dragging = true;
                    host.window().request_redraw();
                }
                (MouseButton::Left, ElementState::Released) => {
                    host.dragging = false;
                    if let Some(sel) = self.app.selection {
                        if let Some(tab) = self.app.tabs.get(self.app.active) {
                            let core = tab.core.lock();
                            let text = sel.resolve(&core);
                            drop(core);
                            host.set_primary(&text);
                        }
                    }
                }
                (MouseButton::Middle, ElementState::Pressed) => {
                    if let Some(text) = host.get_primary() {
                        host.deliver_paste(&self.app, &text);
                    }
                }
                _ => {}
            },
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => (p.y as f32) / (FALLBACK_CELL_H as f32),
                };
                let step = 3u32;
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
        if self.app.pump_ime() {
            host.window().request_redraw();
        }
        if self.app.pump_all() {
            host.window().request_redraw();
        }
        // Cursor cell may have moved as a side effect of pumps; notify
        // the IME backend if the (row, col) changed.
        self.app
            .notify_cursor_rect_if_changed(FALLBACK_CELL_W, FALLBACK_CELL_H);
        if self.app.tabs.is_empty() {
            event_loop.exit();
            return;
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
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
fn handle_special_chord(
    event: &KeyEvent,
    mods: Modifiers,
    host: &mut WindowHost,
    app: &mut App,
) -> bool {
    // Clipboard chords need Ctrl+Shift+<char>. winit reports the
    // Character logical key for letter keys.
    if mods.ctrl && mods.shift {
        if let WinitKey::Character(s) = &event.logical_key {
            let lower = s.to_ascii_lowercase();
            match lower.as_str() {
                "c" => {
                    // Copy current selection to CLIPBOARD.
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
                "v" => {
                    if let Some(text) = host.get_clipboard() {
                        host.deliver_paste(app, &text);
                    }
                    return true;
                }
                _ => {}
            }
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
}
