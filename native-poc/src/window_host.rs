//! tao window + wgpu surface + egui integration.
//!
//! This module owns the GPU surface lifecycle and translates tao events into
//! egui inputs. The egui<->tao glue is intentionally minimal because no
//! published crate covers the tao+wgpu+egui combination as of this writing.
//!
//! Phase 1 responsibilities:
//! - Create a tao window via the supplied event loop.
//! - Acquire a wgpu adapter/device and attach a surface.
//! - Recreate the surface on `SurfaceError::Lost` / `OutOfMemory`.
//! - Drive a per-frame egui pass that renders a placeholder UI.
//!
//! Phase 2 additions:
//! - Translate tao `KeyboardInput` events to PTY bytes via `pty::input`.
//! - Forward bytes to the active tab.
//! - Compute grid (cols, rows) from the window's pixel size and propagate to
//!   PTYs on resize.
//!
//! Later phases extend this with:
//! - Driving the Grid render call (Phase 4).
//! - Mouse selection + clipboard (Phase 4).
//! - IME hooks (Phase 7).

use std::sync::Arc;
use std::time::Duration;

use egui::ViewportId;
use egui_wgpu::wgpu::SurfaceError;
use egui_wgpu::ScreenDescriptor;
use tao::dpi::PhysicalSize;
use tao::event::{ElementState, Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget};
use tao::keyboard::Key as TaoKey;
use tao::window::{Window, WindowBuilder};

use crate::app::App;
use crate::pty::input::{encode, Key, Modifiers};

/// Fallback cell size in physical pixels. Phase 4 replaces this with a real
/// font-metrics-derived value.
const FALLBACK_CELL_W: u32 = 9;
const FALLBACK_CELL_H: u32 = 18;

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
}

impl WindowHost {
    /// Build the window + GPU resources.
    pub fn new(event_loop: &EventLoopWindowTarget<()>) -> Self {
        let window = WindowBuilder::new()
            .with_title("eMterm PoC")
            .with_inner_size(tao::dpi::LogicalSize::new(960.0, 600.0))
            .with_min_inner_size(tao::dpi::LogicalSize::new(320.0, 200.0))
            .build(event_loop)
            .expect("native-poc: failed to create tao window");
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
        }
    }

    pub fn window(&self) -> &Window {
        &self.window
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
        let cols = (w / FALLBACK_CELL_W).max(20).min(500) as u16;
        let rows = (usable_h / FALLBACK_CELL_H).max(5).min(200) as u16;
        (cols, rows)
    }

    /// Run a single egui frame and present.
    pub fn render(&mut self, app: &mut App) {
        // Phase 0: lazy first-frame configure + recovery from Lost/Outdated.
        // `surface_dirty` is true on construction (deferred configure) and
        // whenever a previous frame returned `Lost` / `Outdated`. We
        // reconfigure with the current physical size before acquiring the
        // next swapchain texture.
        if self.surface_dirty {
            self.reconfigure_surface();
        }

        let raw_input = self.build_raw_input();
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            crate::render::draw_placeholder(ctx, app);
        });
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

        self.queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();

        for id in &textures_delta.free {
            self.egui_renderer.free_texture(id);
        }
    }

    /// Translate tao state into a minimal `egui::RawInput`. Phase 1 only
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

/// Translate a tao `KeyEvent` into the PoC's `(Key, Modifiers)` pair and
/// produce the PTY byte sequence. Returns `None` for events that should be
/// ignored (e.g. modifier-only presses).
fn tao_key_to_bytes(event: &tao::event::KeyEvent, mods: Modifiers) -> Option<Vec<u8>> {
    let key = match &event.logical_key {
        TaoKey::Character(s) => {
            let mut chars = s.chars();
            let c = chars.next()?;
            // Multi-character "Character" entries (rare; dead keys) — only
            // forward the first codepoint in Phase 2. Phase 7 IME path
            // handles composed input.
            Key::Char(c)
        }
        TaoKey::Enter => Key::Enter,
        TaoKey::Tab => Key::Tab,
        TaoKey::Backspace => Key::Backspace,
        TaoKey::Escape => Key::Escape,
        TaoKey::Space => Key::Char(' '),
        TaoKey::ArrowUp => Key::Up,
        TaoKey::ArrowDown => Key::Down,
        TaoKey::ArrowLeft => Key::Left,
        TaoKey::ArrowRight => Key::Right,
        TaoKey::Home => Key::Home,
        TaoKey::End => Key::End,
        TaoKey::PageUp => Key::PageUp,
        TaoKey::PageDown => Key::PageDown,
        TaoKey::Delete => Key::Delete,
        TaoKey::Insert => Key::Insert,
        TaoKey::F1 => Key::F(1),
        TaoKey::F2 => Key::F(2),
        TaoKey::F3 => Key::F(3),
        TaoKey::F4 => Key::F(4),
        TaoKey::F5 => Key::F(5),
        TaoKey::F6 => Key::F(6),
        TaoKey::F7 => Key::F(7),
        TaoKey::F8 => Key::F(8),
        TaoKey::F9 => Key::F(9),
        TaoKey::F10 => Key::F(10),
        TaoKey::F11 => Key::F(11),
        TaoKey::F12 => Key::F(12),
        _ => return None,
    };
    let bytes = encode(key, mods);
    if bytes.is_empty() {
        None
    } else {
        Some(bytes)
    }
}

/// Run the event loop until the window is closed. Owns the App.
pub fn run(event_loop: EventLoop<()>, mut app: App) -> ! {
    let mut host = WindowHost::new(&event_loop);

    // Push the initial grid size into the App before the first tab spawn.
    let (cols, rows) = host.grid_size();
    app.cell_size = crate::app::GridDims { cols, rows };
    app.spawn_initial_tab();

    // Poll the PTY event channels at a steady cadence even when there are no
    // window events, so shell output is rendered promptly.
    event_loop.run(move |event, _, control_flow| match event {
        Event::NewEvents(StartCause::Init) => {
            *control_flow =
                ControlFlow::WaitUntil(std::time::Instant::now() + Duration::from_millis(16));
        }
        Event::NewEvents(StartCause::ResumeTimeReached { .. })
        | Event::NewEvents(StartCause::Poll) => {
            if app.pump_all() {
                host.window().request_redraw();
            }
            if app.tabs.is_empty() {
                *control_flow = ControlFlow::Exit;
            } else {
                *control_flow =
                    ControlFlow::WaitUntil(std::time::Instant::now() + Duration::from_millis(16));
            }
        }
        Event::WindowEvent { event, .. } => match event {
            WindowEvent::CloseRequested => {
                *control_flow = ControlFlow::Exit;
            }
            WindowEvent::Resized(new_size) => {
                host.resize(new_size);
                let (cols, rows) = host.grid_size();
                app.set_grid_size(cols, rows);
                host.window().request_redraw();
            }
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                new_inner_size,
                ..
            } => {
                host.pixels_per_point = scale_factor as f32;
                host.resize(*new_inner_size);
                let (cols, rows) = host.grid_size();
                app.set_grid_size(cols, rows);
                host.window().request_redraw();
            }
            WindowEvent::ModifiersChanged(state) => {
                host.current_mods = Modifiers {
                    ctrl: state.control_key(),
                    shift: state.shift_key(),
                    alt: state.alt_key(),
                };
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if let Some(bytes) = tao_key_to_bytes(&event, host.current_mods) {
                    if let Some(tab) = app.active_tab() {
                        tab.write(bytes);
                    }
                }
            }
            _ => {}
        },
        Event::MainEventsCleared if app.pump_all() => {
            host.window().request_redraw();
        }
        Event::RedrawRequested(_) => {
            host.render(&mut app);
        }
        _ => {}
    });
}
