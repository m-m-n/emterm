//! Terminal scrollback scrollbar.
//!
//! The WebView build delegates `settings.show_scrollbar` to the
//! browser's native scrollbar (`overflow-y: auto / scroll / hidden` in
//! `settings-applier.ts`); the native build draws its own minimal
//! track + thumb overlay on the right edge of the terminal area.
//!
//! Geometry model: the scrollable content is `scrollback_len +
//! viewport_rows` rows. Offset `0` (live tail) pins the thumb to the
//! track bottom; offset `scrollback_len` (oldest row visible) pins it
//! to the top — matching `App::{scroll_up_by, scroll_down_by}`
//! semantics where the offset counts rows *back from live*.

use egui::{Color32, Rect, Rounding, Sense, Ui};

use crate::settings::ScrollbarMode;
use crate::ui::md3;

/// Track width in logical pixels. The interact target spans the whole
/// track; the visible thumb is inset for breathing room.
pub const TRACK_W: f32 = 10.0;
/// Horizontal inset of the thumb within the track.
const THUMB_INSET: f32 = 2.0;
/// Minimum thumb height so a deep scrollback stays grabbable.
const MIN_THUMB_H: f32 = 24.0;
/// Thumb fill alpha at rest / while hovered or dragged.
const THUMB_ALPHA_IDLE: u8 = 96;
const THUMB_ALPHA_ACTIVE: u8 = 168;

/// Per-frame view model resolved by the renderer.
pub struct ScrollbarView {
    pub mode: ScrollbarMode,
    /// Rows actually held in the scrollback buffer (not the configured
    /// `scrollback_lines` cap).
    pub scrollback_len: u32,
    pub viewport_rows: u32,
    /// Current offset in rows back from the live tail (`0` = live).
    pub scroll_offset: u32,
    /// Alt-screen sessions suppress scrollback; the bar hides with it.
    pub alt_screen: bool,
}

impl ScrollbarView {
    /// Visibility policy. `Auto` mirrors the browser's `overflow-y:
    /// auto`: show only when there is content beyond the viewport.
    pub fn visible(&self) -> bool {
        match self.mode {
            ScrollbarMode::Never => false,
            _ if self.alt_screen => false,
            ScrollbarMode::Always => true,
            ScrollbarMode::Auto => self.scrollback_len > 0,
        }
    }
}

/// Compute `(thumb_top_within_track, thumb_height)` for the current
/// scroll state.
fn thumb_geometry(
    track_h: f32,
    scrollback_len: u32,
    viewport_rows: u32,
    offset: u32,
) -> (f32, f32) {
    let viewport = viewport_rows.max(1) as f32;
    let total = scrollback_len as f32 + viewport;
    let h = (track_h * (viewport / total)).max(MIN_THUMB_H).min(track_h);
    let scrollable = track_h - h;
    if scrollback_len == 0 {
        // Nothing to scroll: thumb fills / sits at the bottom (live).
        return (scrollable, h);
    }
    // 0 = live (bottom), 1 = oldest (top).
    let ratio = offset.min(scrollback_len) as f32 / scrollback_len as f32;
    ((1.0 - ratio) * scrollable, h)
}

/// Invert [`thumb_geometry`]: map a thumb-top position back to a scroll
/// offset in rows.
fn offset_for_thumb_top(track_h: f32, thumb_h: f32, scrollback_len: u32, thumb_top: f32) -> u32 {
    let scrollable = track_h - thumb_h;
    if scrollable <= 0.0 || scrollback_len == 0 {
        return 0;
    }
    let ratio = 1.0 - (thumb_top / scrollable).clamp(0.0, 1.0);
    (ratio * scrollback_len as f32).round() as u32
}

/// Draw the scrollbar over the right edge of `ui`'s max rect and handle
/// thumb dragging / track clicks. Returns `Some(new_offset)` when the
/// user moved the thumb this frame; the caller routes it to
/// `App::scroll_set_offset` after the egui pass (the renderer only
/// holds `&App`).
pub fn draw(ui: &mut Ui, view: &ScrollbarView) -> Option<u32> {
    if !view.visible() {
        return None;
    }
    let area = ui.max_rect();
    let track = Rect::from_min_max(
        egui::pos2(area.right() - TRACK_W, area.top()),
        area.right_bottom(),
    );
    if track.height() <= 0.0 {
        return None;
    }

    let id = ui.id().with("terminal-scrollbar");
    let response = ui.interact(track, id, Sense::click_and_drag());

    let (thumb_top, thumb_h) = thumb_geometry(
        track.height(),
        view.scrollback_len,
        view.viewport_rows,
        view.scroll_offset,
    );

    // Pointer press: remember where within the thumb the user grabbed
    // so dragging doesn't snap the thumb center to the pointer. A press
    // outside the thumb jumps the thumb center there first (browser /
    // VS Code track-click behavior) and then drags from its middle.
    //
    // The gesture is anchored to the `scrollback_len` captured at press
    // time: while the user holds the thumb during continuous PTY
    // output, the live length grows every frame, and mapping the same
    // pointer position against the growing total would make the
    // viewport drift toward older content under a stationary pointer.
    // The anchored length keeps pointer → offset stable for the whole
    // gesture; the next press re-reads the live length.
    let mut new_offset = None;
    if view.scrollback_len > 0 {
        if let Some(pos) = response.interact_pointer_pos() {
            let pointer_y = pos.y - track.top();
            if response.drag_started() || response.clicked() {
                let within = pointer_y - thumb_top;
                let grab = if (0.0..thumb_h).contains(&within) {
                    within
                } else {
                    thumb_h / 2.0
                };
                ui.data_mut(|d| d.insert_temp(id, (grab, view.scrollback_len)));
            }
            if response.clicked() || response.dragged() {
                let (grab, anchor_len): (f32, u32) = ui
                    .data(|d| d.get_temp(id))
                    .unwrap_or((thumb_h / 2.0, view.scrollback_len));
                // Thumb height depends on the content total, so it is
                // re-derived from the anchored length too (offset does
                // not influence the height — any offset argument works).
                let (_, anchor_thumb_h) =
                    thumb_geometry(track.height(), anchor_len, view.viewport_rows, 0);
                let target_top = pointer_y - grab;
                let offset =
                    offset_for_thumb_top(track.height(), anchor_thumb_h, anchor_len, target_top);
                if offset != view.scroll_offset {
                    new_offset = Some(offset);
                }
            }
        }
    }

    // Paint after interaction so the active state reflects this frame's
    // pointer. Track stays invisible (the terminal background shows
    // through); only the thumb is drawn, MD3 secondary-text tinted.
    let active = response.hovered() || response.dragged();
    let alpha = if active {
        THUMB_ALPHA_ACTIVE
    } else {
        THUMB_ALPHA_IDLE
    };
    let tint = md3::on_surface_variant();
    let thumb = Rect::from_min_max(
        egui::pos2(track.left() + THUMB_INSET, track.top() + thumb_top),
        egui::pos2(
            track.right() - THUMB_INSET,
            track.top() + thumb_top + thumb_h,
        ),
    );
    ui.painter().rect_filled(
        thumb,
        Rounding::same((TRACK_W - THUMB_INSET * 2.0) / 2.0),
        Color32::from_rgba_unmultiplied(tint.r(), tint.g(), tint.b(), alpha),
    );

    new_offset
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_offset_pins_thumb_to_bottom() {
        let (top, h) = thumb_geometry(400.0, 100, 40, 0);
        assert!((top + h - 400.0).abs() < 0.01, "top={top} h={h}");
    }

    #[test]
    fn max_offset_pins_thumb_to_top() {
        let (top, _h) = thumb_geometry(400.0, 100, 40, 100);
        assert!(top.abs() < 0.01, "top={top}");
    }

    #[test]
    fn empty_scrollback_fills_track() {
        let (top, h) = thumb_geometry(400.0, 0, 40, 0);
        assert_eq!(top, 0.0);
        assert_eq!(h, 400.0);
    }

    #[test]
    fn thumb_height_respects_minimum() {
        // 1M scrollback rows over a short track would yield a sub-pixel
        // thumb without the floor.
        let (_top, h) = thumb_geometry(300.0, 1_000_000, 40, 0);
        assert_eq!(h, MIN_THUMB_H);
    }

    #[test]
    fn geometry_roundtrips_through_offset() {
        let track_h = 400.0;
        let (scrollback, viewport) = (1000_u32, 40_u32);
        for offset in [0_u32, 1, 250, 500, 999, 1000] {
            let (top, h) = thumb_geometry(track_h, scrollback, viewport, offset);
            let back = offset_for_thumb_top(track_h, h, scrollback, top);
            assert_eq!(back, offset, "offset {offset} did not roundtrip");
        }
    }

    #[test]
    fn offset_clamps_outside_track() {
        // Above the track top → oldest row; below the bottom → live.
        assert_eq!(offset_for_thumb_top(400.0, 50.0, 100, -25.0), 100);
        assert_eq!(offset_for_thumb_top(400.0, 50.0, 100, 1000.0), 0);
    }

    #[test]
    fn degenerate_track_returns_live() {
        // Thumb as tall as the track (or taller) leaves nothing to scroll.
        assert_eq!(offset_for_thumb_top(100.0, 100.0, 50, 0.0), 0);
        assert_eq!(offset_for_thumb_top(100.0, 120.0, 50, 0.0), 0);
    }

    fn view(mode: ScrollbarMode, scrollback_len: u32, alt_screen: bool) -> ScrollbarView {
        ScrollbarView {
            mode,
            scrollback_len,
            viewport_rows: 40,
            scroll_offset: 0,
            alt_screen,
        }
    }

    #[test]
    fn visibility_matrix() {
        use ScrollbarMode::*;
        // Auto: only with scrollback content.
        assert!(!view(Auto, 0, false).visible());
        assert!(view(Auto, 1, false).visible());
        // Always: even when empty.
        assert!(view(Always, 0, false).visible());
        // Never: never.
        assert!(!view(Never, 500, false).visible());
        // Alt-screen suppresses everything except Never's no-op.
        assert!(!view(Auto, 500, true).visible());
        assert!(!view(Always, 500, true).visible());
    }
}
