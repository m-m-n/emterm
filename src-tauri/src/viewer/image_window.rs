//! Native image-viewer child window (`--image-viewer <payload-path>`).
//!
//! Unlike the Markdown viewer (Wry/WebKitGTK child), the image viewer is
//! rendered natively: a standalone winit window with a wgpu surface where
//! egui paints the decoded RGBA texture plus the viewer chrome. The
//! window chrome matches the terminal: `with_decorations(false)` plus the
//! shared CSD title bar (`ui::title_bar` — icon, drag-to-move, minimize /
//! maximize / close) themed by the user's `ui_theme` / `ui_theme_preset`,
//! with the same 8 pt edge / corner drag-resize zones.
//!
//! The in-canvas behavior mirrors the WebView build's `src/image-viewer/`:
//!
//! - Two display modes: **pixel** (100%, the initial mode) and **fit**
//!   (95% viewport padding, scale clamped to 0.25..=1.0, never upscaled).
//! - `f` toggles the mode, `Esc` closes, `Space` / `Shift+Space` scroll
//!   by 85% of the viewport height.
//! - Drag-to-pan when the canvas exceeds the viewport, clamped to the
//!   centered excess and rounded to integer pixels.
//! - Chrome: a mode toggle button (bottom-right, labeled `100%` / `Fit`)
//!   and an info line (bottom-center, `{w} x {h} | {mode} | f:toggle
//!   Esc:close`). The WebView overlay's `×` button is dropped — the CSD
//!   title bar's close button replaces it.
//!
//! Intentional divergences from the WebView overlay: this is its own OS
//! window (no 150ms overlay fade / 100ms transform transition — window
//! mapping replaces them), and animation playback is not implemented
//! (the parent only forwards static images; see `viewer/image.rs`).

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{ResizeDirection, WindowAttributes};

use super::image_payload::{ImagePayload, read_image_payload};
use super::shell::{GpuShell, payload_path_is_in_temp_dir};
use crate::ui::TitleBarEvent;
use crate::ui::title_bar::{self, TITLE_BAR_HEIGHT};

/// Fit mode leaves this fraction of the viewport around the image
/// (WebView `VIEWPORT_PADDING`).
const VIEWPORT_PADDING: f32 = 0.95;
/// Fit mode never scales below this (WebView `MIN_SCALE`).
const MIN_SCALE: f32 = 0.25;
/// `Space` scrolls by this fraction of the viewport height.
const SPACE_SCROLL_FRACTION: f32 = 0.85;

/// Display mode (WebView `DisplayMode`); pixel = 100% is the initial mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisplayMode {
    Pixel,
    Fit,
}

/// Run the viewer window event loop until the user closes it.
pub fn run(payload_path: &str) -> Result<(), String> {
    // The parent always writes payloads into the OS temp dir; reject
    // anything else so directly invoking `--image-viewer` cannot be used
    // to read (and unlink) an arbitrary local file.
    let path = std::path::Path::new(payload_path);
    if !payload_path_is_in_temp_dir(path) {
        return Err(format!(
            "image viewer: payload path is not inside the OS temp dir: {payload_path}"
        ));
    }
    let payload = read_image_payload(path)
        .map_err(|e| format!("image viewer: failed to read payload {payload_path}: {e}"))?;
    log::info!(
        "image viewer: showing {}x{} image",
        payload.width,
        payload.height
    );
    // Match the terminal chrome: the MD3 palette (title bar / hover
    // tints) and UI font come from the PARENT's resolved settings,
    // carried in the payload header — the child never re-reads
    // `settings.json`, so it cannot drift from the parent's in-memory
    // state (same design as the Markdown viewer's `PayloadAppearance`).
    let theme = crate::settings::UiTheme::parse_or_warn(&payload.chrome.theme);
    let preset = crate::settings::UiThemePreset::parse_or_warn(&payload.chrome.preset);
    crate::ui::md3::set_preset(preset, theme);
    let event_loop =
        EventLoop::new().map_err(|e| format!("image viewer: failed to create event loop: {e}"))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let ui_font_family = payload.chrome.ui_font_family.clone();
    let terminal_font_family = payload.chrome.terminal_font_family.clone();
    let app = ViewerApp::new(payload, ui_font_family, terminal_font_family);
    event_loop
        .run_app(app)
        .map_err(|e| format!("image viewer: event loop error: {e}"))?;
    Ok(())
}

/// View-model half of the viewer: mode, pan, and the egui chrome. Kept
/// separate from [`Gpu`] so `egui::Context::run`'s closure can borrow it
/// mutably while the GPU resources stay borrowed by the render code, and
/// so the layout math is unit-testable without a window.
struct ViewerState {
    img_w: u32,
    img_h: u32,
    mode: DisplayMode,
    /// Pan offset in physical pixels, relative to the centered position.
    pan_x: f32,
    pan_y: f32,
    /// Set by the title bar's close button (or `Esc`); the event loop
    /// exits on the next pass.
    close_requested: bool,
    /// Title-bar intents latched during `build_ui`, consumed by the
    /// caller after the egui frame (the widget is pure — winit calls
    /// happen outside).
    minimize_requested: bool,
    maximize_toggle_requested: bool,
    drag_window_requested: bool,
}

impl ViewerState {
    fn new(img_w: u32, img_h: u32) -> Self {
        Self {
            img_w,
            img_h,
            mode: DisplayMode::Pixel,
            pan_x: 0.0,
            pan_y: 0.0,
            close_requested: false,
            minimize_requested: false,
            maximize_toggle_requested: false,
            drag_window_requested: false,
        }
    }

    /// Fit-mode scale for the given viewport (physical px): 95% padding,
    /// clamped to `MIN_SCALE..=1.0` (small images are never upscaled).
    fn fit_scale(&self, vw: f32, vh: f32) -> f32 {
        if vw <= 0.0 || vh <= 0.0 || self.img_w == 0 || self.img_h == 0 {
            return MIN_SCALE;
        }
        let sx = vw * VIEWPORT_PADDING / self.img_w as f32;
        let sy = vh * VIEWPORT_PADDING / self.img_h as f32;
        sx.min(sy).min(1.0).max(MIN_SCALE)
    }

    fn current_scale(&self, vw: f32, vh: f32) -> f32 {
        match self.mode {
            DisplayMode::Pixel => 1.0,
            DisplayMode::Fit => self.fit_scale(vw, vh),
        }
    }

    /// Clamp the pan offset to the centered excess (WebView
    /// `PanController` bounds): no panning along an axis where the canvas
    /// fits the viewport.
    fn clamp_pan(&mut self, canvas_w: f32, canvas_h: f32, vw: f32, vh: f32) {
        let ex = ((canvas_w - vw) / 2.0).max(0.0);
        let ey = ((canvas_h - vh) / 2.0).max(0.0);
        self.pan_x = self.pan_x.clamp(-ex, ex);
        self.pan_y = self.pan_y.clamp(-ey, ey);
    }

    /// Toggle pixel ↔ fit. The pan offset resets so the image re-centers
    /// in the new mode.
    fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            DisplayMode::Pixel => DisplayMode::Fit,
            DisplayMode::Fit => DisplayMode::Pixel,
        };
        self.pan_x = 0.0;
        self.pan_y = 0.0;
    }

    fn mode_label(&self) -> &'static str {
        match self.mode {
            DisplayMode::Pixel => "100%",
            DisplayMode::Fit => "Fit",
        }
    }

    /// `Space` / `Shift+Space`: scroll down/up by 85% of the viewport
    /// height (positive DOM scroll moves the content up → pan decreases).
    fn space_scroll(&mut self, vw: f32, vh: f32, up: bool) {
        let step = vh * SPACE_SCROLL_FRACTION;
        self.pan_y += if up { step } else { -step };
        let scale = self.current_scale(vw, vh);
        let canvas_w = self.img_w as f32 * scale;
        let canvas_h = self.img_h as f32 * scale;
        self.clamp_pan(canvas_w, canvas_h, vw, vh);
    }

    /// Build the whole egui frame: the shared CSD title bar, the black
    /// image area (with drag-to-pan), and the mode / info chrome. The
    /// viewport for the layout math is the area *below* the title bar.
    fn build_ui(
        &mut self,
        ctx: &egui::Context,
        tex_id: egui::TextureId,
        ppp: f32,
        is_maximized: bool,
    ) {
        // Shared CSD title bar — same widget, icon, and MD3 tints as the
        // terminal window.
        let icon = crate::render::app_icon::texture_id(ctx);
        match title_bar::draw(ctx, "eMterm Image Viewer", is_maximized, icon) {
            Some(TitleBarEvent::Close) => self.close_requested = true,
            Some(TitleBarEvent::Minimize) => self.minimize_requested = true,
            Some(TitleBarEvent::MaximizeToggle) => self.maximize_toggle_requested = true,
            Some(TitleBarEvent::DragStart) => self.drag_window_requested = true,
            None => {}
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::BLACK))
            .show(ctx, |ui| {
                // Viewport = the panel area below the title bar, in
                // physical pixels.
                let panel_rect = ui.max_rect();
                let vw = panel_rect.width() * ppp;
                let vh = panel_rect.height() * ppp;

                let scale = self.current_scale(vw, vh);
                let canvas_w = self.img_w as f32 * scale;
                let canvas_h = self.img_h as f32 * scale;
                self.clamp_pan(canvas_w, canvas_h, vw, vh);
                let pannable = canvas_w > vw || canvas_h > vh;

                let resp = ui.interact(
                    panel_rect,
                    egui::Id::new("viewer-pan"),
                    egui::Sense::click_and_drag(),
                );
                if pannable {
                    if resp.dragged() {
                        let d = resp.drag_delta();
                        self.pan_x += d.x * ppp;
                        self.pan_y += d.y * ppp;
                        self.clamp_pan(canvas_w, canvas_h, vw, vh);
                    }
                    ctx.set_cursor_icon(if resp.dragged() {
                        egui::CursorIcon::Grabbing
                    } else {
                        egui::CursorIcon::Grab
                    });
                }
                // Integer physical-pixel origin (WebView rounds the pan
                // translation for the same sub-pixel-blur reason).
                let x = ((vw - canvas_w) / 2.0 + self.pan_x).round();
                let y = ((vh - canvas_h) / 2.0 + self.pan_y).round();
                let rect = egui::Rect::from_min_size(
                    panel_rect.min + egui::vec2(x / ppp, y / ppp),
                    egui::vec2(canvas_w / ppp, canvas_h / ppp),
                );
                ui.painter().image(
                    tex_id,
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            });

        // Mode toggle (bottom-right, shows the CURRENT mode like the
        // WebView's `.viewer-mode-toggle`).
        egui::Area::new(egui::Id::new("viewer-mode"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
            .show(ctx, |ui| {
                let btn = egui::Button::new(
                    egui::RichText::new(self.mode_label())
                        .size(12.0)
                        .color(egui::Color32::WHITE),
                )
                .min_size(egui::vec2(50.0, 24.0))
                .fill(egui::Color32::from_black_alpha(128));
                if ui.add(btn).clicked() {
                    self.toggle_mode();
                }
            });

        // Info line (bottom-center).
        egui::Area::new(egui::Id::new("viewer-info"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -20.0))
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{} x {} | {} | f:toggle Esc:close",
                        self.img_w,
                        self.img_h,
                        self.mode_label()
                    ))
                    .monospace()
                    .size(12.0)
                    .color(egui::Color32::from_white_alpha(179)),
                );
            });
    }
}

struct ViewerApp {
    payload: ImagePayload,
    ui_font_family: String,
    terminal_font_family: String,
    state: ViewerState,
    shell: Option<GpuShell>,
    /// Keeps the uploaded image alive in egui's texture manager.
    texture: Option<egui::TextureHandle>,
    pending_egui_events: Vec<egui::Event>,
    modifiers: winit::event::Modifiers,
    /// Last cursor position in egui points.
    cursor_pos: egui::Pos2,
    /// CSD resize direction under the pointer (None in the interior /
    /// while maximized). A left press while `Some` hands off to the WM
    /// via `drag_resize_window` instead of reaching egui.
    current_resize_dir: Option<ResizeDirection>,
}

impl ViewerApp {
    fn new(payload: ImagePayload, ui_font_family: String, terminal_font_family: String) -> Self {
        let state = ViewerState::new(payload.width, payload.height);
        Self {
            payload,
            ui_font_family,
            terminal_font_family,
            state,
            shell: None,
            texture: None,
            pending_egui_events: Vec::new(),
            modifiers: winit::event::Modifiers::default(),
            cursor_pos: egui::Pos2::ZERO,
            current_resize_dir: None,
        }
    }

    fn egui_modifiers(&self) -> egui::Modifiers {
        let s = self.modifiers.state();
        egui::Modifiers {
            alt: s.alt_key(),
            ctrl: s.control_key(),
            shift: s.shift_key(),
            mac_cmd: false,
            command: s.control_key(),
        }
    }

    /// Image-area viewport in physical pixels (the window minus the
    /// title bar). 0×0 before `resumed`.
    fn image_viewport(&self) -> (f32, f32) {
        match &self.shell {
            Some(s) => (
                s.surface_config.width as f32,
                (s.surface_config.height as f32 - TITLE_BAR_HEIGHT * s.pixels_per_point).max(0.0),
            ),
            None => (0.0, 0.0),
        }
    }

    fn request_redraw(&self) {
        if let Some(s) = &self.shell {
            s.window.request_redraw();
        }
    }

    fn render(&mut self) {
        let modifiers = self.egui_modifiers();
        let events = std::mem::take(&mut self.pending_egui_events);
        let resize_hover = self.current_resize_dir;
        let (Some(shell), Some(texture)) = (self.shell.as_mut(), self.texture.as_ref()) else {
            return;
        };
        let raw_input = shell.build_raw_input(modifiers, events);
        let tex_id = texture.id();
        let ppp = shell.pixels_per_point;
        let is_maximized = shell.window.is_maximized();
        let state = &mut self.state;
        let repaint_now = shell.render_frame(raw_input, &mut |ctx| {
            state.build_ui(ctx, tex_id, ppp, is_maximized)
        });
        // The CSD edge hint owns the cursor while the pointer is in a
        // resize zone; otherwise egui's per-frame output applies.
        shell.apply_cursor(resize_hover);

        // Title-bar intents latched during the frame.
        if std::mem::take(&mut state.minimize_requested) {
            shell.window.set_minimized(true);
        }
        if std::mem::take(&mut state.maximize_toggle_requested) {
            shell.window.set_maximized(!is_maximized);
        }
        if std::mem::take(&mut state.drag_window_requested) {
            if let Err(e) = shell.window.drag_window() {
                log::warn!("image viewer: drag_window failed: {e}");
            }
        }
        if repaint_now {
            shell.window.request_redraw();
        }
    }
}

impl ApplicationHandler for ViewerApp {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.shell.is_some() {
            return;
        }

        // Size the window to the image, capped to 90% of the monitor and
        // floored at 320×240 so tiny images still get usable chrome.
        let monitor = event_loop
            .primary_monitor()
            .or_else(|| event_loop.available_monitors().next());
        let (max_w, max_h) = match monitor.as_ref().and_then(|m| m.current_video_mode()) {
            Some(vm) => {
                let s = vm.size();
                (
                    (s.width as f32 * 0.9) as u32,
                    (s.height as f32 * 0.9) as u32,
                )
            }
            None => (1280, 800),
        };
        let win_w = self.payload.width.clamp(320, max_w.max(320));
        let win_h = self.payload.height.clamp(240, max_h.max(240));

        // CSD like the terminal window: no WM decorations; the egui
        // title bar + edge zones replace them.
        let attrs = WindowAttributes::default()
            .with_title("eMterm Image Viewer")
            .with_decorations(false)
            // FR4: deliberately NOT maximized — the image viewer keeps its
            // image-fit sizing (`win_w`/`win_h` above).
            .with_surface_size(PhysicalSize::new(win_w, win_h))
            .with_min_surface_size(LogicalSize::new(320.0, 240.0));
        // FR5: stamp the canonical dock-grouping identifier (X11
        // `WM_CLASS` / Wayland `app_id`) — grouping only, no maximize.
        #[cfg(target_os = "linux")]
        let attrs = crate::linux_wm::with_app_id(event_loop, attrs);
        let shell = GpuShell::new(
            event_loop,
            attrs,
            &self.ui_font_family,
            &self.terminal_font_family,
        );

        // Nearest magnification keeps pixel mode (100%) exact; linear
        // minification keeps fit mode (<100%) smooth.
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [self.payload.width as usize, self.payload.height as usize],
            &self.payload.rgba,
        );
        self.texture = Some(shell.egui_ctx.load_texture(
            "viewer-image",
            color,
            egui::TextureOptions {
                magnification: egui::TextureFilter::Nearest,
                minification: egui::TextureFilter::Linear,
                ..Default::default()
            },
        ));
        self.shell = Some(shell);
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::SurfaceResized(_) => {
                if let Some(s) = self.shell.as_mut() {
                    s.surface_dirty = true;
                }
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(s) = self.shell.as_mut() {
                    s.pixels_per_point = scale_factor as f32;
                    s.surface_dirty = true;
                }
                self.request_redraw();
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                match &event.logical_key {
                    Key::Named(NamedKey::Escape) => event_loop.exit(),
                    // winit 0.31 removed `NamedKey::Space`; match on the
                    // literal character instead.
                    Key::Character(c) if c.as_str() == " " => {
                        let (vw, vh) = self.image_viewport();
                        let up = self.modifiers.state().shift_key();
                        self.state.space_scroll(vw, vh, up);
                        self.request_redraw();
                    }
                    Key::Character(c) if c.eq_ignore_ascii_case("f") => {
                        self.state.toggle_mode();
                        self.request_redraw();
                    }
                    _ => {}
                }
            }
            WindowEvent::PointerMoved { position, .. } => {
                let ppp = self
                    .shell
                    .as_ref()
                    .map(|s| s.pixels_per_point)
                    .unwrap_or(1.0);
                let logical = position.to_logical::<f32>(ppp as f64);
                self.cursor_pos = egui::pos2(logical.x, logical.y);
                self.current_resize_dir = self
                    .shell
                    .as_ref()
                    .and_then(|s| s.resize_direction_at(logical.x, logical.y));
                self.pending_egui_events
                    .push(egui::Event::PointerMoved(self.cursor_pos));
                self.request_redraw();
            }
            WindowEvent::PointerLeft { .. } => {
                self.current_resize_dir = None;
                self.pending_egui_events.push(egui::Event::PointerGone);
                self.request_redraw();
            }
            WindowEvent::PointerButton { state, button, .. } => {
                let Some(button) = button.mouse_button() else {
                    return;
                };
                // A left press on a CSD edge hands off to the WM resize
                // loop instead of reaching egui.
                if button == MouseButton::Left && state == ElementState::Pressed {
                    if let Some(dir) = self.current_resize_dir {
                        if let Some(s) = &self.shell {
                            if let Err(e) = s.window.drag_resize_window(dir) {
                                log::warn!("image viewer: drag_resize_window failed: {e}");
                            }
                        }
                        return;
                    }
                }
                let egui_button = match button {
                    MouseButton::Left => egui::PointerButton::Primary,
                    MouseButton::Right => egui::PointerButton::Secondary,
                    MouseButton::Middle => egui::PointerButton::Middle,
                    _ => return,
                };
                self.pending_egui_events.push(egui::Event::PointerButton {
                    pos: self.cursor_pos,
                    button: egui_button,
                    pressed: state == ElementState::Pressed,
                    modifiers: self.egui_modifiers(),
                });
                self.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                self.render();
                if self.state.close_requested {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── fit_scale (WebView display-mode.ts:79-117 parity) ───────────────

    #[test]
    fn fit_scale_uses_95_percent_padding_and_min_axis() {
        let s = ViewerState::new(1000, 800);
        // 800×600 viewport: sx = 800*0.95/1000 = 0.76, sy = 600*0.95/800
        // = 0.7125 → min wins.
        let scale = s.fit_scale(800.0, 600.0);
        assert!((scale - 0.7125).abs() < 1e-5);
    }

    #[test]
    fn fit_scale_never_upscales_small_images() {
        let s = ViewerState::new(100, 100);
        assert_eq!(s.fit_scale(800.0, 600.0), 1.0);
    }

    #[test]
    fn fit_scale_clamps_to_min_scale() {
        let s = ViewerState::new(10000, 10000);
        assert_eq!(s.fit_scale(800.0, 600.0), MIN_SCALE);
    }

    #[test]
    fn fit_scale_invalid_viewport_returns_min_scale() {
        let s = ViewerState::new(1000, 800);
        assert_eq!(s.fit_scale(0.0, 600.0), MIN_SCALE);
    }

    // ── pan clamping (WebView pan-controller.ts:193-210 parity) ─────────

    #[test]
    fn clamp_pan_bounds_to_half_excess() {
        let mut s = ViewerState::new(1000, 800);
        // 1000×800 canvas in an 800×600 viewport → excess 200×200 →
        // bounds ±100.
        s.pan_x = 500.0;
        s.pan_y = -500.0;
        s.clamp_pan(1000.0, 800.0, 800.0, 600.0);
        assert_eq!(s.pan_x, 100.0);
        assert_eq!(s.pan_y, -100.0);
    }

    #[test]
    fn clamp_pan_zero_when_canvas_fits() {
        let mut s = ViewerState::new(100, 100);
        s.pan_x = 50.0;
        s.pan_y = 50.0;
        s.clamp_pan(100.0, 100.0, 800.0, 600.0);
        assert_eq!(s.pan_x, 0.0);
        assert_eq!(s.pan_y, 0.0);
    }

    // ── mode toggle ──────────────────────────────────────────────────────

    #[test]
    fn initial_mode_is_pixel_at_100_percent() {
        let s = ViewerState::new(1000, 800);
        assert_eq!(s.mode, DisplayMode::Pixel);
        assert_eq!(s.current_scale(800.0, 600.0), 1.0);
        assert_eq!(s.mode_label(), "100%");
    }

    #[test]
    fn toggle_switches_mode_and_resets_pan() {
        let mut s = ViewerState::new(1000, 800);
        s.pan_x = 50.0;
        s.toggle_mode();
        assert_eq!(s.mode, DisplayMode::Fit);
        assert_eq!(s.mode_label(), "Fit");
        assert_eq!(s.pan_x, 0.0);
        s.toggle_mode();
        assert_eq!(s.mode, DisplayMode::Pixel);
    }

    // ── space scroll (85% viewport height) ──────────────────────────────

    #[test]
    fn space_scroll_moves_85_percent_of_viewport_and_clamps() {
        // 1000-px-tall image in a 100-px viewport → excess 900 → bounds
        // ±450. One Space = -85.
        let mut s = ViewerState::new(50, 1000);
        s.space_scroll(800.0, 100.0, false);
        assert_eq!(s.pan_y, -85.0);
        // Scroll up past the bound clamps at +450.
        for _ in 0..20 {
            s.space_scroll(800.0, 100.0, true);
        }
        assert_eq!(s.pan_y, 450.0);
    }

    #[test]
    fn space_scroll_noop_when_image_fits() {
        let mut s = ViewerState::new(50, 50);
        s.space_scroll(800.0, 600.0, false);
        assert_eq!(s.pan_y, 0.0);
    }

    // ── payload-path containment ─────────────────────────────────────────

    #[test]
    fn payload_path_inside_temp_dir_is_accepted() {
        let path = std::env::temp_dir().join(format!(
            "emterm-image-viewer-containment-test-{}",
            std::process::id()
        ));
        std::fs::write(&path, b"x").unwrap();
        assert!(payload_path_is_in_temp_dir(&path));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    #[cfg(unix)]
    fn payload_path_outside_temp_dir_is_rejected() {
        // An existing, readable file that is definitely not under the OS
        // temp dir.
        assert!(!payload_path_is_in_temp_dir(std::path::Path::new(
            "/etc/hostname"
        )));
    }

    #[test]
    fn payload_path_nonexistent_is_rejected() {
        let path = std::env::temp_dir().join("emterm-image-viewer-no-such-file.bin");
        assert!(!payload_path_is_in_temp_dir(&path));
    }

    #[test]
    #[cfg(unix)]
    fn payload_path_symlink_escaping_temp_dir_is_rejected() {
        // A symlink INSIDE the temp dir pointing OUTSIDE must be rejected
        // (containment is checked post-canonicalize).
        let link = std::env::temp_dir().join(format!(
            "emterm-image-viewer-escape-link-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink("/etc/hostname", &link).unwrap();
        assert!(!payload_path_is_in_temp_dir(&link));
        let _ = std::fs::remove_file(&link);
    }

    // ── title-bar event latching ─────────────────────────────────────────

    /// Drive a real egui frame with a click on the title bar's close
    /// button and verify the latch (the widget itself is covered by
    /// `ui::title_bar` tests; this checks the viewer's wiring).
    #[test]
    fn title_bar_close_click_latches_close_requested() {
        let ctx = egui::Context::default();
        let mut s = ViewerState::new(100, 100);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let click = egui::pos2(800.0 - 23.0, TITLE_BAR_HEIGHT / 2.0);
        let tex_id = egui::TextureId::default();

        // Frame 1: hover. Frame 2: press + release → clicked.
        let mut input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        input.events.push(egui::Event::PointerMoved(click));
        let _ = ctx.run(input, |ctx| s.build_ui(ctx, tex_id, 1.0, false));

        let mut input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        input.events.push(egui::Event::PointerButton {
            pos: click,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        });
        input.events.push(egui::Event::PointerButton {
            pos: click,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        });
        let _ = ctx.run(input, |ctx| s.build_ui(ctx, tex_id, 1.0, false));

        assert!(s.close_requested);
        assert!(!s.minimize_requested);
        assert!(!s.maximize_toggle_requested);
    }
}
