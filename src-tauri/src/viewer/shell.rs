//! Shared native-viewer GPU shell.
//!
//! Owns the winit window + wgpu surface + egui renderer plumbing common
//! to every native child-viewer window (image viewer, JSON/YAML data
//! viewer). The per-viewer modules keep only their state, UI build, and
//! input handling; the shell provides window/GPU construction, the
//! raw-input builder, and the frame render path (tessellate → upload →
//! single egui pass over a black clear → present).

use std::sync::Arc;

use winit::event_loop::ActiveEventLoop;
use winit::window::{CursorIcon, Window, WindowAttributes};

use egui_wgpu::ScreenDescriptor;

use crate::ui::chrome::configure_egui_fonts;

/// Window + GPU resources for one native viewer window.
pub(crate) struct GpuShell {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    pub surface_config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pub egui_ctx: egui::Context,
    egui_renderer: egui_wgpu::Renderer,
    pub pixels_per_point: f32,
    pub surface_dirty: bool,
}

impl GpuShell {
    /// Create the window and the full wgpu/egui stack. `ui_font_family`
    /// skins the egui chrome (title-bar text) like the terminal window.
    pub fn new(
        event_loop: &ActiveEventLoop,
        attrs: WindowAttributes,
        ui_font_family: &str,
    ) -> Self {
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("viewer shell: failed to create window"),
        );

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        // SAFETY: `window` is kept alive in `Arc<Window>` next to the
        // surface for the whole `GpuShell` lifetime (same pattern as
        // `WindowHost::new`).
        let surface: wgpu::Surface<'static> = unsafe {
            instance
                .create_surface_unsafe(
                    wgpu::SurfaceTargetUnsafe::from_window(&*window)
                        .expect("viewer shell: surface target"),
                )
                .expect("viewer shell: create surface")
        };
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }))
        .expect("viewer shell: no compatible wgpu adapter found");
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("viewer-shell-device"),
                required_features: wgpu::Features::empty(),
                required_limits:
                    wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .expect("viewer shell: failed to request wgpu device");

        let size = window.inner_size();
        let surface_caps = surface.get_capabilities(&adapter);
        // Non-sRGB surface for the same verbatim-bytes reason as the
        // terminal window (see `WindowHost::new`).
        let format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
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

        let egui_ctx = egui::Context::default();
        configure_egui_fonts(&egui_ctx, ui_font_family);
        let egui_renderer = egui_wgpu::Renderer::new(&device, format, None, 1, false);
        let pixels_per_point = window.scale_factor() as f32;

        window.request_redraw();
        Self {
            window,
            surface,
            surface_config,
            device,
            queue,
            egui_ctx,
            egui_renderer,
            pixels_per_point,
            surface_dirty: true,
        }
    }

    /// Build the standard viewer `RawInput` from the live window size and
    /// caller-collected events/modifiers.
    pub fn build_raw_input(
        &self,
        modifiers: egui::Modifiers,
        events: Vec<egui::Event>,
    ) -> egui::RawInput {
        let size = self.window.inner_size();
        let logical = size.to_logical::<f32>(self.pixels_per_point as f64);
        egui::RawInput {
            viewport_id: egui::ViewportId::ROOT,
            viewports: std::iter::once((
                egui::ViewportId::ROOT,
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
            modifiers,
            events,
            focused: true,
            max_texture_side: Some(8192),
            ..Default::default()
        }
    }

    /// Run one egui frame and present it: reconfigure-if-dirty, `ctx.run`
    /// with `run_ui`, tessellate, texture upload, a single egui pass over
    /// a black clear, present, texture free. Returns `true` when egui
    /// asked for an immediate repaint (e.g. hover animation).
    pub fn render_frame(
        &mut self,
        raw_input: egui::RawInput,
        run_ui: &mut dyn FnMut(&egui::Context),
    ) -> bool {
        if self.surface_dirty {
            self.reconfigure_surface();
        }

        let ctx = self.egui_ctx.clone();
        let full_output = ctx.run(raw_input, |ctx| run_ui(ctx));

        let ppp = self.pixels_per_point;
        let paint_jobs = ctx.tessellate(full_output.shapes, ppp);
        let textures_delta = full_output.textures_delta;

        let surface_texture = match self.acquire_surface_texture() {
            Some(tex) => tex,
            None => return false,
        };
        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("viewer-shell-encoder"),
            });
        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [self.surface_config.width, self.surface_config.height],
            pixels_per_point: ppp,
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
                    label: Some("viewer-shell-egui-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
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
        self.queue.submit(std::iter::once(encoder.finish()));
        self.window.pre_present_notify();
        surface_texture.present();
        for id in &textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        full_output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .is_some_and(|v| v.repaint_delay.is_zero())
    }

    /// Reconfigure the wgpu surface for the current window size.
    fn reconfigure_surface(&mut self) {
        let size = self.window.inner_size();
        self.surface_config.width = size.width.max(1);
        self.surface_config.height = size.height.max(1);
        self.surface.configure(&self.device, &self.surface_config);
        self.surface_dirty = false;
    }

    /// Acquire the next swapchain texture, transparently recovering from
    /// `suboptimal` results by reconfiguring once and re-acquiring before
    /// returning. Mirrors [`crate::window_host::WindowHost::acquire_surface_texture`]
    /// so the viewer windows don't spam `wgpu_hal`'s "Suboptimal present
    /// of frame N" warn on every frame whose swapchain happens to be
    /// suboptimal (typically after a resize the surface hasn't observed
    /// yet).
    fn acquire_surface_texture(&mut self) -> Option<wgpu::SurfaceTexture> {
        let mut tries: u8 = 0;
        loop {
            match self.surface.get_current_texture() {
                Ok(tex) if tex.suboptimal && tries == 0 => {
                    drop(tex);
                    log::debug!("viewer shell: surface suboptimal; reconfiguring before retry");
                    self.reconfigure_surface();
                    tries += 1;
                    continue;
                }
                Ok(tex) => return Some(tex),
                Err(wgpu::SurfaceError::Lost) | Err(wgpu::SurfaceError::Outdated) => {
                    self.surface_dirty = true;
                    self.window.request_redraw();
                    return None;
                }
                Err(e) => {
                    log::warn!("viewer shell: surface error {e:?}; skipping frame");
                    self.surface_dirty = true;
                    self.window.request_redraw();
                    return None;
                }
            }
        }
    }

    /// Apply the egui-reported cursor unless the caller's CSD resize hint
    /// owns the pointer.
    pub fn apply_cursor(&self, resize_hover: Option<winit::window::ResizeDirection>) {
        let cursor = match resize_hover {
            Some(dir) => CursorIcon::from(dir),
            None => egui_to_winit_cursor(self.egui_ctx.output(|o| o.cursor_icon)),
        };
        self.window.set_cursor(cursor);
    }
}

impl GpuShell {
    /// Classify the pointer position (egui points) against the CSD edge
    /// zones, with the terminal's title-bar carve-out: inside the bar
    /// (below the outermost strip) the move / button semantics win over
    /// the North resize.
    pub fn resize_direction_at(
        &self,
        logical_x: f32,
        logical_y: f32,
    ) -> Option<winit::window::ResizeDirection> {
        use crate::ui::chrome::{RESIZE_EDGE_PX, classify_resize_edge};
        use crate::ui::title_bar::TITLE_BAR_HEIGHT;
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
        if logical_y >= RESIZE_EDGE_PX && logical_y < TITLE_BAR_HEIGHT {
            use winit::window::ResizeDirection::*;
            if matches!(dir, North | NorthEast | NorthWest) {
                return None;
            }
        }
        Some(dir)
    }
}

/// True iff `path` resolves (symlinks included) to a file inside the OS
/// temp dir. Containment is checked AFTER `canonicalize`, so a symlink
/// planted inside the temp dir cannot point a viewer at an outside file.
/// A non-existent path is rejected (canonicalize fails). Every child
/// viewer validates its payload path through this before reading.
pub(crate) fn payload_path_is_in_temp_dir(path: &std::path::Path) -> bool {
    let Ok(real) = std::fs::canonicalize(path) else {
        return false;
    };
    let Ok(tmp) = std::fs::canonicalize(std::env::temp_dir()) else {
        return false;
    };
    real.starts_with(&tmp)
}

/// Map the egui cursor icons the viewers use onto winit cursors.
fn egui_to_winit_cursor(icon: egui::CursorIcon) -> CursorIcon {
    match icon {
        egui::CursorIcon::Grab => CursorIcon::Grab,
        egui::CursorIcon::Grabbing => CursorIcon::Grabbing,
        egui::CursorIcon::PointingHand => CursorIcon::Pointer,
        egui::CursorIcon::Text => CursorIcon::Text,
        egui::CursorIcon::ResizeHorizontal => CursorIcon::EwResize,
        _ => CursorIcon::Default,
    }
}
