//! Custom (client-side) title bar.
//!
//! Renders a single 32 px [`egui::TopBottomPanel::top`] above the tab
//! strip when the window runs with `with_decorations(false)`. Layout
//! mirrors the conventional Linux/Windows CSD pattern:
//!
//! - Left: app icon (supplied by the caller as a prepared
//!   [`egui::TextureId`]) followed by the app title (`title` argument).
//! - Right: three primitive-drawn glyph buttons — minimize, maximize
//!   (switches to a restore-style overlapped pair when the window is
//!   already maximized), and close. The close button gets a red
//!   hover overlay so it visually separates from the surface-tinted
//!   ones.
//! - Center / remainder: an invisible drag affordance. Press-and-drag
//!   emits [`TitleBarEvent::DragStart`] so the caller can invoke
//!   [`winit::window::Window::drag_window`]. Double-click on the same
//!   region emits [`TitleBarEvent::MaximizeToggle`].
//!
//! All icons are painted with `egui::Painter` primitives
//! (`line_segment` / `rect_stroke`) instead of font glyphs so the
//! visual is independent of the user's installed fonts.
//!
//! The widget is pure over the inputs (no state mutation, no winit
//! calls). The caller owns the `Window` handle and translates the
//! returned [`TitleBarEvent`] into the corresponding winit API.

use egui::{
    Align, Align2, Color32, FontId, Layout, Rect, Rounding, Sense, Stroke, TextureId, Ui, Vec2,
};

use super::md3;
use super::TitleBarEvent;

/// Title-bar height in egui logical points. 32 pt matches the common
/// CSD bar height on Linux GTK / Windows 10.
pub const TITLE_BAR_HEIGHT: f32 = 32.0;
/// Per-button width. Square so the glyph stays optically centered on
/// the bar's 32 pt vertical axis.
const BUTTON_WIDTH: f32 = 46.0;
/// Title font size (egui logical points).
const TITLE_FONT_SIZE: f32 = 12.0;
/// Side length of the bounding box every icon is drawn inside,
/// centred on the button rect.
const ICON_SIZE: f32 = 10.0;
/// Stroke width for the icon primitives. 1.0 reads crisply on 1.0×
/// and 2.0× HiDPI scales without looking too thin or too heavy.
const ICON_STROKE_WIDTH: f32 = 1.0;
/// Pixel offset between the two overlapped squares of the restore
/// icon. Small enough that both squares still fit inside `ICON_SIZE`.
const RESTORE_OFFSET: f32 = 2.5;
/// Left inset for the icon so it doesn't sit flush with the window
/// edge. The title text follows the icon.
const TITLE_LEFT_PAD: f32 = 12.0;
/// On-screen size of the square app icon, in egui logical points.
/// Smaller than `TITLE_BAR_HEIGHT` so it reads as an icon with breathing
/// room rather than filling the bar edge-to-edge.
const ICON_DISPLAY_SIZE: f32 = 18.0;
/// Gap between the app icon and the title text.
const ICON_TITLE_GAP: f32 = 8.0;
/// Hover overlay for the close button — saturated red so a stray
/// pointer doesn't accidentally dismiss the window without a clear
/// affordance change.
const CLOSE_HOVER_BG: Color32 = Color32::from_rgb(0xC4, 0x2B, 0x1C);
/// Hover overlay for the minimize / maximize buttons. The MD3 8 %
/// state-layer is too subtle on the dark `SURFACE_CONTAINER_LOW`
/// title-bar background, so we use the next-brighter surface step
/// outright. Reads as a clear "I'm hoverable" cue without competing
/// visually with the red close affordance.
/// Hover overlay for the minimize / maximize buttons. Resolved at
/// call-time via [`md3::surface_container_highest`] so the user's
/// `ui_theme_preset` choice tints the hover step alongside the rest of
/// the chrome.
fn control_hover_bg() -> Color32 {
    md3::surface_container_highest()
}

/// One of the four CSD button glyphs. The maximize button switches
/// between [`Maximize`] and [`Restore`] based on the current window
/// state so the user can read the destination at a glance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonKind {
    /// Horizontal line at the bottom of the icon bbox.
    Minimize,
    /// Single hollow square — the action that would maximize the
    /// window from its current floating size.
    Maximize,
    /// Two overlapped squares — the action that would restore the
    /// previously-floating size from a maximized window. Mirrors the
    /// Windows / GTK convention where "two windows behind each
    /// other" means "one big window will become multiple smaller
    /// ones."
    Restore,
    /// Two diagonal lines forming an X.
    Close,
}

/// Render the title bar and return at most one event for the frame.
///
/// `is_maximized` switches the middle button between the maximize
/// and restore glyphs but does NOT change which event is emitted —
/// `MaximizeToggle` always fires and the caller picks the
/// destination state via `winit::Window::is_maximized()`.
///
/// `icon` is the caller-prepared app-icon texture (see
/// [`crate::render::app_icon`]). When `Some`, it is drawn at the left of
/// the bar ahead of the title; when `None`, only the title shows. The
/// widget never loads or rasterizes the asset itself, keeping it pure
/// over its inputs.
pub fn draw(
    ctx: &egui::Context,
    title: &str,
    is_maximized: bool,
    icon: Option<TextureId>,
) -> Option<TitleBarEvent> {
    let mut event: Option<TitleBarEvent> = None;

    let frame = egui::Frame::none()
        .fill(md3::surface_container_low())
        .inner_margin(egui::Margin::ZERO);

    egui::TopBottomPanel::top("native-poc-title-bar")
        .frame(frame)
        .exact_height(TITLE_BAR_HEIGHT)
        .show_separator_line(false)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let panel_rect = ui.max_rect();
            let panel_bg = md3::surface_container_low();

            // Right-to-left layout so the controls cluster on the right
            // edge and we can give the title / drag affordance the
            // remaining space on the left.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // Close — rightmost.
                if draw_button(ui, ButtonKind::Close, true, panel_bg).clicked() && event.is_none() {
                    event = Some(TitleBarEvent::Close);
                }
                // Maximize / restore — switches glyph but always
                // emits MaximizeToggle. The caller flips
                // `Window::set_maximized` based on the current state.
                let middle_kind = if is_maximized {
                    ButtonKind::Restore
                } else {
                    ButtonKind::Maximize
                };
                if draw_button(ui, middle_kind, false, panel_bg).clicked() && event.is_none() {
                    event = Some(TitleBarEvent::MaximizeToggle);
                }
                // Minimize — leftmost of the three.
                if draw_button(ui, ButtonKind::Minimize, false, panel_bg).clicked()
                    && event.is_none()
                {
                    event = Some(TitleBarEvent::Minimize);
                }

                // Drag / double-click affordance fills the remainder.
                // `allocate_rect` with the available_rect keeps the
                // input live without painting anything on top.
                let drag_rect = ui.available_rect_before_wrap();
                if drag_rect.width() > 0.0 {
                    let drag_resp = ui.allocate_rect(drag_rect, Sense::click_and_drag());
                    if drag_resp.double_clicked() && event.is_none() {
                        event = Some(TitleBarEvent::MaximizeToggle);
                    } else if drag_resp.drag_started() && event.is_none() {
                        event = Some(TitleBarEvent::DragStart);
                    }

                    // App icon + title painted directly via the painter
                    // so the layout cursor stays put and the drag rect
                    // covers the whole left half. The icon texture is
                    // prepared by the caller (render::app_icon).
                    let painter = ui.painter();
                    let mut text_left = drag_rect.left() + TITLE_LEFT_PAD;
                    if let Some(tex_id) = icon {
                        let icon_rect = Rect::from_center_size(
                            egui::pos2(text_left + ICON_DISPLAY_SIZE / 2.0, drag_rect.center().y),
                            Vec2::splat(ICON_DISPLAY_SIZE),
                        );
                        painter.image(
                            tex_id,
                            icon_rect,
                            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                        text_left = icon_rect.right() + ICON_TITLE_GAP;
                    }
                    painter.text(
                        egui::pos2(text_left, drag_rect.center().y),
                        Align2::LEFT_CENTER,
                        title,
                        FontId::proportional(TITLE_FONT_SIZE),
                        md3::on_surface_variant(),
                    );
                }
            });

            // 1 px hairline at the bottom — visually separates the bar
            // from the tab strip / status row below it.
            let painter = ui.painter();
            painter.hline(
                panel_rect.left()..=panel_rect.right(),
                panel_rect.bottom() - 0.5,
                Stroke::new(1.0, md3::outline_variant()),
            );
        });

    event
}

/// Draw one CSD control button: opaque hover fill (red for `danger`,
/// `SURFACE_CONTAINER_HIGHEST` otherwise) plus the primitive-drawn
/// icon glyph.
///
/// `panel_bg` is the title-bar background colour. The restore glyph
/// "knocks out" the part of the back square that the front square
/// sits on top of using whatever bg is currently visible (panel bg
/// when idle, hover fill when hovered) so the result reads as two
/// distinct overlapping windows instead of a stacked outline.
fn draw_button(ui: &mut Ui, kind: ButtonKind, danger: bool, panel_bg: Color32) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(BUTTON_WIDTH, TITLE_BAR_HEIGHT), Sense::click());
    let painter = ui.painter();

    let hovered = resp.hovered();
    let bg_now = if hovered {
        let bg = if danger {
            CLOSE_HOVER_BG
        } else {
            control_hover_bg()
        };
        painter.rect_filled(rect, Rounding::ZERO, bg);
        bg
    } else {
        panel_bg
    };

    // Hover lifts the icon colour from the muted variant to the
    // full on-surface tone, so the change is felt as both a
    // background fill AND a sharper glyph.
    let icon_color = if hovered {
        if danger {
            Color32::WHITE
        } else {
            md3::on_surface()
        }
    } else {
        md3::on_surface_variant()
    };
    let stroke = Stroke::new(ICON_STROKE_WIDTH, icon_color);
    let bbox = Rect::from_center_size(rect.center(), Vec2::splat(ICON_SIZE));

    match kind {
        ButtonKind::Minimize => {
            // Horizontal stroke near the icon's bottom edge.
            let y = bbox.center().y + ICON_SIZE / 2.0 - ICON_STROKE_WIDTH / 2.0;
            painter.line_segment(
                [egui::pos2(bbox.left(), y), egui::pos2(bbox.right(), y)],
                stroke,
            );
        }
        ButtonKind::Maximize => {
            painter.rect_stroke(bbox, Rounding::ZERO, stroke);
        }
        ButtonKind::Restore => {
            // Back square (offset up + right).
            let back = Rect::from_min_size(
                egui::pos2(bbox.left() + RESTORE_OFFSET, bbox.top()),
                Vec2::splat(ICON_SIZE - RESTORE_OFFSET),
            );
            painter.rect_stroke(back, Rounding::ZERO, stroke);
            // Knock the part of the back square that the front
            // square will cover back out to the current button bg,
            // so the icon reads as two distinct overlapping
            // windows rather than a crossed outline.
            let front = Rect::from_min_size(
                egui::pos2(bbox.left(), bbox.top() + RESTORE_OFFSET),
                Vec2::splat(ICON_SIZE - RESTORE_OFFSET),
            );
            painter.rect_filled(front, Rounding::ZERO, bg_now);
            // Front square — drawn on top so its stroke wins where
            // it crosses the back square.
            painter.rect_stroke(front, Rounding::ZERO, stroke);
        }
        ButtonKind::Close => {
            painter.line_segment([bbox.left_top(), bbox.right_bottom()], stroke);
            painter.line_segment([bbox.left_bottom(), bbox.right_top()], stroke);
        }
    }

    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect};

    fn screen() -> Rect {
        Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 100.0))
    }

    /// Drive two frames so egui's click state transitions
    /// (hover → pressed → released → clicked) settle, returning the
    /// event emitted on the release frame.
    fn run_with_click(click_pos: Pos2, is_maximized: bool) -> Option<TitleBarEvent> {
        let ctx = egui::Context::default();

        let mut input1 = RawInput::default();
        input1.screen_rect = Some(screen());
        input1.events.push(Event::PointerMoved(click_pos));
        let mut ev1: Option<TitleBarEvent> = None;
        let _ = ctx.run(input1, |ctx| {
            ev1 = draw(ctx, "eMterm", is_maximized, None);
        });

        let mut input2 = RawInput::default();
        input2.screen_rect = Some(screen());
        input2.events.push(Event::PointerMoved(click_pos));
        input2.events.push(Event::PointerButton {
            pos: click_pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::default(),
        });
        input2.events.push(Event::PointerButton {
            pos: click_pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::default(),
        });
        let mut ev2: Option<TitleBarEvent> = None;
        let _ = ctx.run(input2, |ctx| {
            ev2 = draw(ctx, "eMterm", is_maximized, None);
        });
        ev2.or(ev1)
    }

    #[test]
    fn close_button_at_right_edge_emits_close() {
        let click = Pos2::new(
            screen().right() - BUTTON_WIDTH / 2.0,
            TITLE_BAR_HEIGHT / 2.0,
        );
        assert_eq!(run_with_click(click, false), Some(TitleBarEvent::Close));
    }

    #[test]
    fn maximize_button_emits_maximize_toggle_when_floating() {
        let click = Pos2::new(
            screen().right() - BUTTON_WIDTH * 1.5,
            TITLE_BAR_HEIGHT / 2.0,
        );
        assert_eq!(
            run_with_click(click, false),
            Some(TitleBarEvent::MaximizeToggle)
        );
    }

    #[test]
    fn maximize_button_emits_maximize_toggle_when_already_maximized() {
        // Same hit-box, but the glyph swaps to the restore overlap.
        // The event must still be MaximizeToggle so the caller's
        // `set_maximized(!is_maximized)` handler stays symmetric.
        let click = Pos2::new(
            screen().right() - BUTTON_WIDTH * 1.5,
            TITLE_BAR_HEIGHT / 2.0,
        );
        assert_eq!(
            run_with_click(click, true),
            Some(TitleBarEvent::MaximizeToggle)
        );
    }

    #[test]
    fn minimize_button_emits_minimize() {
        let click = Pos2::new(
            screen().right() - BUTTON_WIDTH * 2.5,
            TITLE_BAR_HEIGHT / 2.0,
        );
        assert_eq!(run_with_click(click, false), Some(TitleBarEvent::Minimize));
    }

    #[test]
    fn left_area_click_alone_is_not_an_event() {
        // A bare click (no drag, no double-click) over the title /
        // drag affordance must NOT fire an event — the caller would
        // otherwise see a spurious drag every time the user clicked
        // somewhere benign.
        let click = Pos2::new(TITLE_LEFT_PAD + 40.0, TITLE_BAR_HEIGHT / 2.0);
        assert_eq!(run_with_click(click, false), None);
    }
}
