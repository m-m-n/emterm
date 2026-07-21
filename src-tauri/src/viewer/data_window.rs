//! Native JSON/YAML data-viewer child window (`--data-viewer <path>`).
//!
//! Rendered natively on the shared viewer shell (winit + wgpu + egui,
//! `viewer::shell`) with the terminal's CSD title bar. The in-window
//! behavior mirrors the WebView build's `src/data-viewer/`:
//!
//! - **Outline** (default): left tree pane (always fully expanded,
//!   280pt initial width, 200–600pt resizable) + right detail pane that
//!   re-serializes the selected subtree in the source format with
//!   2-space indent and syntax highlighting.
//! - **RAW**: full source with syntax highlighting, a Copy button
//!   (top-right), and the JSON pretty-print toggle (`p`).
//! - Keys: `Esc` close, `r` outline↔RAW (locked on parse error), `p`
//!   pretty (JSON RAW only), `Up`/`Down`/`PageUp`/`PageDown`/`Home`/`End`
//!   tree navigation in outline or scrolling in RAW, `Space` /
//!   `Shift+Space` RAW page scroll (85% viewport).
//! - Parse errors: red banner (`Parse error: …`), plain-text RAW only.
//!
//! Colors follow the WebView `data-viewer.css` (dark fixed palette);
//! the CSD chrome follows the user's `ui_theme` via the payload tokens.

use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{ResizeDirection, WindowAttributes};

use super::data::DataFormat;
use super::data_model::{DataViewerState, TokKind, ViewMode};
use super::data_payload::read_data_payload;
use super::shell::{GpuShell, payload_path_is_in_temp_dir};
use crate::ui::TitleBarEvent;
use crate::ui::title_bar::{self, TITLE_BAR_HEIGHT};

// WebView `data-viewer.css` palette (dark fixed).
const BG: egui::Color32 = egui::Color32::from_rgb(0x1e, 0x1e, 0x1e);
const FG: egui::Color32 = egui::Color32::from_rgb(0xd4, 0xd4, 0xd4);
const PANEL_BG: egui::Color32 = egui::Color32::from_rgb(0x25, 0x25, 0x26);
const MUTED: egui::Color32 = egui::Color32::from_rgb(0x88, 0x88, 0x88);
const SELECT_BG: egui::Color32 = egui::Color32::from_rgb(0x09, 0x47, 0x71);
const HOVER_BG: egui::Color32 = egui::Color32::from_rgb(0x2a, 0x2d, 0x2e);
const ERROR_BG: egui::Color32 = egui::Color32::from_rgb(0x4a, 0x1c, 0x1c);
const ERROR_FG: egui::Color32 = egui::Color32::from_rgb(0xff, 0x88, 0x88);
const COPY_BG: egui::Color32 = egui::Color32::from_rgb(0x33, 0x33, 0x33);
const COPY_FG: egui::Color32 = egui::Color32::from_rgb(0xcc, 0xcc, 0xcc);
// `.dv-*` token colors.
const COL_KEY: egui::Color32 = egui::Color32::from_rgb(0x9c, 0xdc, 0xfe);
const COL_STR: egui::Color32 = egui::Color32::from_rgb(0xce, 0x91, 0x78);
const COL_NUM: egui::Color32 = egui::Color32::from_rgb(0xb5, 0xce, 0xa8);
const COL_BOOL: egui::Color32 = egui::Color32::from_rgb(0x56, 0x9c, 0xd6);
const COL_PUNCT: egui::Color32 = egui::Color32::from_rgb(0x80, 0x80, 0x80);
const COL_COMMENT: egui::Color32 = egui::Color32::from_rgb(0x6a, 0x99, 0x55);

/// RAW-view keyboard scroll steps (WebView fullscreen.ts).
const ARROW_SCROLL_PT: f32 = 40.0;
const SPACE_SCROLL_FRACTION: f32 = 0.85;
/// Far-enough delta for Home/End absolute scrolls.
const SCROLL_TO_END: f32 = 1.0e9;
/// Content font size (egui points).
const CONTENT_FONT: f32 = 12.0;

/// Run the viewer window event loop until the user closes it.
pub fn run(payload_path: &str) -> Result<(), String> {
    let path = std::path::Path::new(payload_path);
    if !payload_path_is_in_temp_dir(path) {
        return Err(format!(
            "data viewer: payload path is not inside the OS temp dir: {payload_path}"
        ));
    }
    let payload = read_data_payload(path)
        .map_err(|e| format!("data viewer: failed to read payload {payload_path}: {e}"))?;
    log::info!(
        "data viewer: showing {} document ({} bytes)",
        payload.format.as_str(),
        payload.text.len()
    );
    // CSD chrome follows the parent's resolved theme (payload tokens),
    // same as the image viewer.
    let theme = crate::settings::UiTheme::parse_or_warn(&payload.chrome.theme);
    let preset = crate::settings::UiThemePreset::parse_or_warn(&payload.chrome.preset);
    crate::ui::md3::set_preset(preset, theme);

    let event_loop =
        EventLoop::new().map_err(|e| format!("data viewer: failed to create event loop: {e}"))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let state = DataViewerState::new(payload.format, payload.text);
    let app = ViewerApp::new(
        state,
        payload.chrome.ui_font_family.clone(),
        payload.chrome.terminal_font_family.clone(),
    );
    event_loop
        .run_app(app)
        .map_err(|e| format!("data viewer: event loop error: {e}"))?;
    Ok(())
}

/// Title-bar intents latched during `build_ui`, consumed after the
/// frame (the widget is pure — winit calls happen outside).
#[derive(Default)]
struct ChromeLatches {
    close: bool,
    minimize: bool,
    maximize_toggle: bool,
    drag_window: bool,
}

/// Window-side state that is not part of the testable model: scroll
/// requests, caches keyed to the display variant, and the Copy feedback.
struct WindowUi {
    latches: ChromeLatches,
    /// Pending RAW-view vertical scroll in points (+down). Applied (and
    /// cleared) inside the RAW scroll area on the next frame.
    raw_scroll: f32,
    /// Scroll the selected outline row into view on the next frame.
    scroll_selection_into_view: bool,
    /// RAW line cache keyed by the pretty flag.
    raw_lines: Option<(bool, Vec<String>)>,
    /// Highlighted detail galley, cached per `(selected, pixels_per_point
    /// bits)` so the tokenizer + text layout run only when the selection
    /// (or DPI) changes — never per frame.
    detail_galley: Option<(usize, u32, std::sync::Arc<egui::Galley>)>,
    /// Tree scroll offset from the previous frame (virtualized rows need
    /// manual scroll-into-view math; `scroll_to_me` can't reach rows that
    /// were never rendered).
    tree_scroll_offset: f32,
    /// Tree viewport height from the previous frame.
    tree_viewport_h: f32,
    /// Last central-content height in points (keyboard page scrolls).
    content_height: f32,
    /// `Some(t)` while the Copy button shows "Copied!" (2 s).
    copied_at: Option<Instant>,
    /// Set by the Copy button; consumed by the winit side (clipboard).
    copy_requested: bool,
}

impl WindowUi {
    fn new() -> Self {
        Self {
            latches: ChromeLatches::default(),
            raw_scroll: 0.0,
            scroll_selection_into_view: false,
            raw_lines: None,
            detail_galley: None,
            tree_scroll_offset: 0.0,
            tree_viewport_h: 0.0,
            content_height: 400.0,
            copied_at: None,
            copy_requested: false,
        }
    }
}

/// Minimal scroll adjustment that brings row `selected` (of `stride`
/// height) into a viewport of `viewport_h` currently scrolled to
/// `current`: scroll up to the row's top when it is above, down to its
/// bottom when below, otherwise keep the offset.
fn scroll_offset_for_row(selected: usize, stride: f32, viewport_h: f32, current: f32) -> f32 {
    let top = selected as f32 * stride;
    let bottom = top + stride;
    if top < current {
        top
    } else if bottom > current + viewport_h && viewport_h > 0.0 {
        bottom - viewport_h
    } else {
        current
    }
}

struct ViewerApp {
    state: DataViewerState,
    ui: WindowUi,
    ui_font_family: String,
    terminal_font_family: String,
    shell: Option<GpuShell>,
    clipboard: Option<arboard::Clipboard>,
    pending_egui_events: Vec<egui::Event>,
    modifiers: winit::event::Modifiers,
    cursor_pos: egui::Pos2,
    current_resize_dir: Option<ResizeDirection>,
}

impl ViewerApp {
    fn new(state: DataViewerState, ui_font_family: String, terminal_font_family: String) -> Self {
        Self {
            state,
            ui: WindowUi::new(),
            ui_font_family,
            terminal_font_family,
            shell: None,
            clipboard: None,
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

    fn request_redraw(&self) {
        if let Some(s) = &self.shell {
            s.window.request_redraw();
        }
    }

    fn window_title(format: DataFormat) -> &'static str {
        match format {
            DataFormat::Json => "eMterm JSON Viewer",
            DataFormat::Yaml => "eMterm YAML Viewer",
        }
    }

    fn render(&mut self) {
        let modifiers = self.egui_modifiers();
        let events = std::mem::take(&mut self.pending_egui_events);
        let resize_hover = self.current_resize_dir;
        let Some(shell) = self.shell.as_mut() else {
            return;
        };
        let raw_input = shell.build_raw_input(modifiers, events);
        let is_maximized = shell.window.is_maximized();
        let state = &mut self.state;
        let win_ui = &mut self.ui;
        let repaint_now = shell.render_frame(raw_input, &mut |ctx| {
            build_ui(ctx, state, win_ui, is_maximized);
        });
        shell.apply_cursor(resize_hover);

        // Latched chrome intents.
        if std::mem::take(&mut win_ui.latches.minimize) {
            shell.window.set_minimized(true);
        }
        if std::mem::take(&mut win_ui.latches.maximize_toggle) {
            shell.window.set_maximized(!is_maximized);
        }
        if std::mem::take(&mut win_ui.latches.drag_window) {
            if let Err(e) = shell.window.drag_window() {
                log::warn!("data viewer: drag_window failed: {e}");
            }
        }
        // Copy button → clipboard (arboard lives outside the egui pass).
        if std::mem::take(&mut win_ui.copy_requested) {
            if self.clipboard.is_none() {
                self.clipboard = match arboard::Clipboard::new() {
                    Ok(c) => Some(c),
                    Err(e) => {
                        log::warn!("data viewer: clipboard unavailable: {e}");
                        None
                    }
                };
            }
            if let Some(cb) = self.clipboard.as_mut() {
                let text = self.state.raw_display_text().to_string();
                if let Err(e) = cb.set_text(text) {
                    log::warn!("data viewer: clipboard write failed: {e}");
                } else {
                    self.ui.copied_at = Some(Instant::now());
                }
            }
        }
        if repaint_now {
            if let Some(s) = &self.shell {
                s.window.request_redraw();
            }
        }
    }

    /// Keyboard dispatch (WebView fullscreen.ts:273-335 parity).
    fn handle_key(&mut self, key: &Key, event_loop: &dyn ActiveEventLoop) {
        let outline = self.state.mode == ViewMode::Outline;
        let page = self.ui.content_height;
        match key {
            Key::Named(NamedKey::Escape) => {
                event_loop.exit();
                return;
            }
            Key::Character(c) if c.eq_ignore_ascii_case("r") => self.state.toggle_mode(),
            Key::Character(c) if c.eq_ignore_ascii_case("p") => self.state.toggle_pretty(),
            Key::Named(NamedKey::ArrowUp) => {
                if outline {
                    self.state.navigate(-1);
                    self.ui.scroll_selection_into_view = true;
                } else {
                    self.ui.raw_scroll -= ARROW_SCROLL_PT;
                }
            }
            Key::Named(NamedKey::ArrowDown) => {
                if outline {
                    self.state.navigate(1);
                    self.ui.scroll_selection_into_view = true;
                } else {
                    self.ui.raw_scroll += ARROW_SCROLL_PT;
                }
            }
            Key::Named(NamedKey::PageUp) => {
                if outline {
                    self.state.navigate(-10);
                    self.ui.scroll_selection_into_view = true;
                } else {
                    self.ui.raw_scroll -= page;
                }
            }
            Key::Named(NamedKey::PageDown) => {
                if outline {
                    self.state.navigate(10);
                    self.ui.scroll_selection_into_view = true;
                } else {
                    self.ui.raw_scroll += page;
                }
            }
            Key::Named(NamedKey::Home) => {
                if outline {
                    self.state.select_first();
                    self.ui.scroll_selection_into_view = true;
                } else {
                    self.ui.raw_scroll -= SCROLL_TO_END;
                }
            }
            Key::Named(NamedKey::End) => {
                if outline {
                    self.state.select_last();
                    self.ui.scroll_selection_into_view = true;
                } else {
                    self.ui.raw_scroll += SCROLL_TO_END;
                }
            }
            // winit 0.31 removed `NamedKey::Space`; match on the literal
            // character instead (matches `Key::Character(" ")`).
            Key::Character(c) if c.as_str() == " " => {
                if !outline {
                    let up = self.modifiers.state().shift_key();
                    let step = page * SPACE_SCROLL_FRACTION;
                    self.ui.raw_scroll += if up { -step } else { step };
                }
            }
            _ => return,
        }
        self.request_redraw();
    }
}

impl ApplicationHandler for ViewerApp {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.shell.is_some() {
            return;
        }
        let attrs = WindowAttributes::default()
            .with_title(Self::window_title(self.state.format))
            .with_decorations(false)
            // FR3: open maximized; `with_surface_size` below is the restore
            // size the window returns to when un-maximized.
            .with_maximized(true)
            .with_surface_size(LogicalSize::new(960.0, 640.0))
            .with_min_surface_size(LogicalSize::new(320.0, 240.0));
        // FR5: stamp the canonical dock-grouping identifier (X11
        // `WM_CLASS` / Wayland `app_id`).
        #[cfg(target_os = "linux")]
        let attrs = crate::linux_wm::with_app_id(event_loop, attrs);
        self.shell = Some(GpuShell::new(
            event_loop,
            attrs,
            &self.ui_font_family,
            &self.terminal_font_family,
        ));
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
                let key = event.logical_key.clone();
                self.handle_key(&key, event_loop);
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
            WindowEvent::MouseWheel { delta, .. } => {
                // Forward wheel to egui so both panes scroll natively.
                let d = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => {
                        egui::vec2(x * 20.0, y * 20.0)
                    }
                    winit::event::MouseScrollDelta::PixelDelta(p) => {
                        let ppp = self
                            .shell
                            .as_ref()
                            .map(|s| s.pixels_per_point)
                            .unwrap_or(1.0);
                        egui::vec2(p.x as f32 / ppp, p.y as f32 / ppp)
                    }
                };
                self.pending_egui_events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: d,
                    modifiers: self.egui_modifiers(),
                });
                self.request_redraw();
            }
            WindowEvent::PointerButton { state, button, .. } => {
                let Some(button) = button.mouse_button() else {
                    return;
                };
                if button == MouseButton::Left && state == ElementState::Pressed {
                    if let Some(dir) = self.current_resize_dir {
                        if let Some(s) = &self.shell {
                            if let Err(e) = s.window.drag_resize_window(dir) {
                                log::warn!("data viewer: drag_resize_window failed: {e}");
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
                if self.ui.latches.close {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }
}

// ── egui frame ─────────────────────────────────────────────────────────

fn build_ui(
    ctx: &egui::Context,
    state: &mut DataViewerState,
    win: &mut WindowUi,
    is_maximized: bool,
) {
    // Shared CSD title bar (same widget + MD3 tints as the terminal).
    let icon = crate::render::app_icon::texture_id(ctx);
    let title = ViewerApp::window_title(state.format);
    match title_bar::draw(ctx, title, is_maximized, icon) {
        Some(TitleBarEvent::Close) => win.latches.close = true,
        Some(TitleBarEvent::Minimize) => win.latches.minimize = true,
        Some(TitleBarEvent::MaximizeToggle) => win.latches.maximize_toggle = true,
        Some(TitleBarEvent::DragStart) => win.latches.drag_window = true,
        None => {}
    }

    // Header: mode badge.
    egui::TopBottomPanel::top("dv-header")
        .frame(
            egui::Frame::none()
                .fill(PANEL_BG)
                .inner_margin(egui::Margin::symmetric(16.0, 6.0)),
        )
        .show_separator_line(false)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(state.badge())
                    .size(12.0)
                    .strong()
                    .color(FG),
            );
        });

    // Parse-error banner (FR9).
    if let Some(err) = state.parse_error().map(|s| s.to_string()) {
        egui::TopBottomPanel::top("dv-error")
            .frame(
                egui::Frame::none()
                    .fill(ERROR_BG)
                    .inner_margin(egui::Margin::symmetric(16.0, 6.0)),
            )
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(format!("Parse error: {err}"))
                        .size(12.0)
                        .color(ERROR_FG),
                );
            });
    }

    // Footer: key hints.
    egui::TopBottomPanel::bottom("dv-footer")
        .frame(
            egui::Frame::none()
                .fill(PANEL_BG)
                .inner_margin(egui::Margin::symmetric(16.0, 4.0)),
        )
        .show_separator_line(false)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(state.footer_hint())
                    .size(11.0)
                    .color(MUTED),
            );
        });

    match state.mode {
        ViewMode::Outline => outline_ui(ctx, state, win),
        ViewMode::Raw => raw_ui(ctx, state, win),
    }
}

/// Outline mode: resizable tree side panel + detail central panel.
fn outline_ui(ctx: &egui::Context, state: &mut DataViewerState, win: &mut WindowUi) {
    egui::SidePanel::left("dv-tree")
        .frame(egui::Frame::none().fill(PANEL_BG))
        .resizable(true)
        .default_width(280.0)
        .width_range(200.0..=600.0)
        .show(ctx, |ui| {
            tree_rows(ui, state, win);
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(BG))
        .show(ctx, |ui| {
            win.content_height = ui.available_height();
            // Highlighted detail galley, cached per (selection, DPI):
            // the tokenizer + text layout run only when those change.
            // Painting a cached `Arc<Galley>` per frame is O(visible),
            // not O(document).
            let sel = state.selected;
            let ppp_bits = ctx.pixels_per_point().to_bits();
            let cached =
                matches!(&win.detail_galley, Some((s, p, _)) if *s == sel && *p == ppp_bits);
            if !cached {
                let job = highlight_job(state.format, state.detail());
                let galley = ctx.fonts(|f| f.layout_job(job));
                win.detail_galley = Some((sel, ppp_bits, galley));
            }
            let galley = win.detail_galley.as_ref().expect("filled above").2.clone();
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        let (rect, _) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
                        ui.painter().galley(rect.min, galley.clone(), FG);
                    });
                    ui.add_space(8.0);
                });
        });
}

/// Virtualized tree rows (WebView `.tree-item` visuals): only the rows in
/// the visible range are laid out and painted, like the RAW view's
/// `show_rows`. Keyboard navigation adjusts the scroll offset directly —
/// `scroll_to_me` cannot reach rows that were never rendered.
fn tree_rows(ui: &mut egui::Ui, state: &mut DataViewerState, win: &mut WindowUi) {
    let row_h = ui.fonts(|f| f.row_height(&egui::FontId::monospace(CONTENT_FONT))) * 1.6;
    // `show_rows` strides by row_h + item_spacing.y; zero the spacing so
    // the offset arithmetic below stays exact.
    ui.spacing_mut().item_spacing.y = 0.0;
    let scroll_into_view = std::mem::take(&mut win.scroll_selection_into_view);
    let mut clicked: Option<usize> = None;

    let mut area = egui::ScrollArea::both().auto_shrink([false, false]);
    if scroll_into_view {
        let target = scroll_offset_for_row(
            state.selected,
            row_h,
            win.tree_viewport_h,
            win.tree_scroll_offset,
        );
        area = area.vertical_scroll_offset(target);
    }
    let out = area.show_rows(ui, row_h, state.nodes.len(), |ui, range| {
        for i in range {
            let Some(node) = state.nodes.get(i) else {
                continue;
            };
            let selected = i == state.selected;
            let indent = (node.depth as f32 + 1.0) * 16.0 + 8.0;
            let marker = if node.has_children { "▸ " } else { "" };
            let text = format!("{marker}{}", node.label);
            let galley = ui.fonts(|f| {
                f.layout_no_wrap(
                    text,
                    egui::FontId::monospace(CONTENT_FONT),
                    egui::Color32::WHITE, // recolored below
                )
            });
            let row_w = (indent + galley.size().x + 16.0).max(ui.available_width());
            let (rect, resp) =
                ui.allocate_exact_size(egui::vec2(row_w, row_h), egui::Sense::click());
            if selected {
                ui.painter().rect_filled(rect, 0.0, SELECT_BG);
            } else if resp.hovered() {
                ui.painter().rect_filled(rect, 0.0, HOVER_BG);
            }
            let color = if selected {
                egui::Color32::WHITE
            } else if node.depth == 0 {
                MUTED // "(root)" — WebView renders it muted italic.
            } else {
                FG
            };
            let font = egui::FontId::monospace(CONTENT_FONT);
            let pos = egui::pos2(rect.left() + indent, rect.center().y);
            if node.depth == 0 {
                let mut job = egui::text::LayoutJob::default();
                job.append(
                    &format!(
                        "{}{}",
                        if node.has_children { "▸ " } else { "" },
                        node.label
                    ),
                    0.0,
                    egui::TextFormat {
                        font_id: font,
                        color,
                        italics: true,
                        ..Default::default()
                    },
                );
                let galley = ui.fonts(|f| f.layout_job(job));
                ui.painter().galley(
                    egui::pos2(pos.x, pos.y - galley.size().y / 2.0),
                    galley,
                    color,
                );
            } else {
                ui.painter().text(
                    pos,
                    egui::Align2::LEFT_CENTER,
                    format!(
                        "{}{}",
                        if node.has_children { "▸ " } else { "" },
                        node.label
                    ),
                    font,
                    color,
                );
            }
            if resp.clicked() {
                clicked = Some(i);
            }
        }
    });
    win.tree_scroll_offset = out.state.offset.y;
    win.tree_viewport_h = out.inner_rect.height();

    if let Some(i) = clicked {
        state.select(i);
    }
}

/// RAW mode: virtualized highlighted source + Copy button.
fn raw_ui(ctx: &egui::Context, state: &mut DataViewerState, win: &mut WindowUi) {
    let plain = state.parse_error().is_some();
    let format = state.format;
    let pretty = state.pretty;

    // (Re)build the line cache when the display variant changed.
    let needs_lines = !matches!(&win.raw_lines, Some((p, _)) if *p == pretty);
    if needs_lines {
        let lines: Vec<String> = state
            .raw_display_text()
            .lines()
            .map(|l| l.to_string())
            .collect();
        win.raw_lines = Some((pretty, lines));
    }

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(BG))
        .show(ctx, |ui| {
            win.content_height = ui.available_height();
            let row_h = ui.fonts(|f| f.row_height(&egui::FontId::monospace(CONTENT_FONT)));
            let lines = &win.raw_lines.as_ref().expect("filled above").1;
            let pending = std::mem::take(&mut win.raw_scroll);
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show_rows(ui, row_h, lines.len().max(1), |ui, range| {
                    if pending != 0.0 {
                        ui.scroll_with_delta(egui::vec2(0.0, -pending));
                    }
                    for i in range {
                        let Some(line) = lines.get(i) else { continue };
                        let job = if plain {
                            plain_job(line)
                        } else {
                            highlight_job_line(format, line)
                        };
                        ui.horizontal(|ui| {
                            ui.add_space(16.0);
                            ui.add(egui::Label::new(job).wrap_mode(egui::TextWrapMode::Extend));
                        });
                    }
                });
        });

    // Copy button — floats over the content's top-right (WebView
    // `.dv-copy-button`).
    let banner_h = if plain { 28.0 } else { 0.0 };
    let y = TITLE_BAR_HEIGHT + 28.0 + banner_h + 8.0;
    egui::Area::new(egui::Id::new("dv-copy"))
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-16.0, y))
        .show(ctx, |ui| {
            let label = match win.copied_at {
                Some(t) if t.elapsed().as_secs_f32() < 2.0 => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                    "Copied!"
                }
                _ => "Copy",
            };
            let btn = egui::Button::new(egui::RichText::new(label).size(11.0).color(COPY_FG))
                .fill(COPY_BG)
                .stroke(egui::Stroke::new(
                    1.0,
                    egui::Color32::from_rgb(0x55, 0x55, 0x55),
                ));
            if ui.add(btn).clicked() {
                win.copy_requested = true;
            }
        });
}

// ── highlight → LayoutJob ──────────────────────────────────────────────

fn tok_format(kind: TokKind) -> egui::TextFormat {
    let (color, italics) = match kind {
        TokKind::Key => (COL_KEY, false),
        TokKind::Str => (COL_STR, false),
        TokKind::Num => (COL_NUM, false),
        TokKind::Bool => (COL_BOOL, false),
        TokKind::Null => (COL_BOOL, true),
        TokKind::Punct => (COL_PUNCT, false),
        TokKind::Comment => (COL_COMMENT, true),
        TokKind::Plain => (FG, false),
    };
    egui::TextFormat {
        font_id: egui::FontId::monospace(CONTENT_FONT),
        color,
        italics,
        ..Default::default()
    }
}

/// Highlight one line into a LayoutJob.
fn highlight_job_line(format: DataFormat, line: &str) -> egui::text::LayoutJob {
    let toks = match format {
        DataFormat::Json => super::data_model::highlight_json_line(line),
        DataFormat::Yaml => super::data_model::highlight_yaml_line(line),
    };
    let mut job = egui::text::LayoutJob::default();
    for (kind, text) in toks {
        job.append(&text, 0.0, tok_format(kind));
    }
    if job.sections.is_empty() {
        job.append(" ", 0.0, tok_format(TokKind::Plain));
    }
    job
}

/// Highlight a multi-line block (detail pane).
fn highlight_job(format: DataFormat, text: &str) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    for (i, line) in text.lines().enumerate() {
        if i > 0 {
            job.append("\n", 0.0, tok_format(TokKind::Plain));
        }
        let toks = match format {
            DataFormat::Json => super::data_model::highlight_json_line(line),
            DataFormat::Yaml => super::data_model::highlight_yaml_line(line),
        };
        for (kind, t) in toks {
            job.append(&t, 0.0, tok_format(kind));
        }
    }
    if job.sections.is_empty() {
        job.append(" ", 0.0, tok_format(TokKind::Plain));
    }
    job
}

fn plain_job(line: &str) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        if line.is_empty() { " " } else { line },
        0.0,
        tok_format(TokKind::Plain),
    );
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── virtualized tree scroll-into-view math ───────────────────────────

    #[test]
    fn scroll_offset_keeps_visible_row_in_place() {
        // Row 10 of 20pt rows in a 200pt viewport scrolled to 100pt:
        // rows 5..15 are visible → no adjustment.
        assert_eq!(scroll_offset_for_row(10, 20.0, 200.0, 100.0), 100.0);
    }

    #[test]
    fn scroll_offset_scrolls_up_to_row_above_viewport() {
        // Row 2 (top = 40) above the 100pt offset → snap to its top.
        assert_eq!(scroll_offset_for_row(2, 20.0, 200.0, 100.0), 40.0);
    }

    #[test]
    fn scroll_offset_scrolls_down_to_row_below_viewport() {
        // Row 20 (bottom = 420) below 100+200 → align bottom to viewport.
        assert_eq!(scroll_offset_for_row(20, 20.0, 200.0, 100.0), 220.0);
    }

    #[test]
    fn scroll_offset_handles_unknown_viewport() {
        // First frame: viewport height not yet measured (0) → only the
        // scroll-up branch may fire; never a negative/odd offset.
        assert_eq!(scroll_offset_for_row(0, 20.0, 0.0, 100.0), 0.0);
        assert_eq!(scroll_offset_for_row(10, 20.0, 0.0, 100.0), 100.0);
    }
}
