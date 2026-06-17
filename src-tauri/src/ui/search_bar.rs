//! Floating in-terminal search bar (egui overlay).
//!
//! Port of the WebView build's `src/terminal/search/search-bar.ts` +
//! `search-bar.css`. Renders a dark, standalone-themed bar pinned to the
//! top-right of the terminal area (`top: 4px; right: 8px`) holding, in
//! order: a 200 px text field, a hit-count label, a separator, the `".*"`
//! regex toggle, the `"Aa"` case toggle, a separator, `↑` prev / `↓` next
//! navigation, a separator, and an `×` close button.
//!
//! The bar is **not** themed with MD3 tokens — like the WebView, it uses
//! the fixed dark palette spelled out in `search-bar.css`
//! (`doc/UI-DESIGN-GUIDELINES.yaml:702-715`). The color constants below
//! are the `Color32` equivalents of those CSS `rgba()` values.
//!
//! The widget is pure-ish: it reads/writes the borrowed
//! [`SearchState`] (query + toggle flags) and returns a [`SearchBarEvent`]
//! describing the interaction the app loop must act on. Keyboard
//! navigation (Enter / Shift+Enter / Esc) is handled one layer up in
//! `window_host` so it works regardless of egui focus; the buttons here
//! provide the pointer-driven equivalents.

use egui::{
    Align2, Area, Color32, FontId, Frame, Id, Margin, Order, Rounding, Sense, Stroke, Vec2,
};

use crate::search::SearchState;

// ── Palette (search-bar.css → Color32) ───────────────────────
//
// egui's `from_rgba_unmultiplied` is not `const`, so each color is given
// in the equivalent premultiplied form (rgb · alpha, alpha · 255). The
// doc comment carries the source CSS `rgba()` straight-alpha literal so
// the mapping stays auditable against `search-bar.css`.

/// `rgba(30, 30, 30, 0.92)` → premultiplied `(28, 28, 28, 235)`.
const BAR_BG: Color32 = Color32::from_rgba_premultiplied(28, 28, 28, 235);
/// `rgba(255, 255, 255, 0.15)` → `(38, 38, 38, 38)`.
const BAR_BORDER: Color32 = Color32::from_rgba_premultiplied(38, 38, 38, 38);
/// `rgba(255, 255, 255, 0.08)` → `(20, 20, 20, 20)`.
const INPUT_BG: Color32 = Color32::from_rgba_premultiplied(20, 20, 20, 20);
/// `rgba(255, 255, 255, 0.12)` → `(31, 31, 31, 31)`.
const INPUT_BORDER: Color32 = Color32::from_rgba_premultiplied(31, 31, 31, 31);
/// `rgba(100, 150, 255, 0.5)` → `(50, 75, 128, 128)`.
const INPUT_BORDER_FOCUS: Color32 = Color32::from_rgba_premultiplied(50, 75, 128, 128);
/// `rgba(255, 80, 80, 0.6)` → `(153, 48, 48, 153)`.
const INPUT_BORDER_ERROR: Color32 = Color32::from_rgba_premultiplied(153, 48, 48, 153);
/// Input text `#eee`.
const INPUT_TEXT: Color32 = Color32::from_rgb(238, 238, 238);
/// Button glyph at rest `#aaa`.
const BTN_FG: Color32 = Color32::from_rgb(170, 170, 170);
/// Button glyph / bar text on hover `#ddd`.
const BTN_FG_HOVER: Color32 = Color32::from_rgb(221, 221, 221);
/// `rgba(255, 255, 255, 0.1)` → `(26, 26, 26, 26)`.
const BTN_BG_HOVER: Color32 = Color32::from_rgba_premultiplied(26, 26, 26, 26);
/// `rgba(100, 150, 255, 0.25)` → `(25, 38, 64, 64)`.
const TOGGLE_ACTIVE_BG: Color32 = Color32::from_rgba_premultiplied(25, 38, 64, 64);
/// `rgba(100, 150, 255, 0.4)` → `(40, 60, 102, 102)`.
const TOGGLE_ACTIVE_BORDER: Color32 = Color32::from_rgba_premultiplied(40, 60, 102, 102);
/// Active toggle glyph `#fff`.
const TOGGLE_ACTIVE_FG: Color32 = Color32::WHITE;
/// Hit-count label `#888`.
const COUNT_FG: Color32 = Color32::from_rgb(136, 136, 136);
/// `rgba(255, 255, 255, 0.12)` → `(31, 31, 31, 31)`.
const SEP: Color32 = Color32::from_rgba_premultiplied(31, 31, 31, 31);

/// Input field width in logical px (`search-bar-input { width: 200px }`).
const INPUT_W: f32 = 200.0;
/// Button square size (`search-bar-btn { width/height: 24px }`).
const BTN_SIZE: f32 = 24.0;
/// Input field height (3px vertical padding around 13px text ≈ 22px).
const INPUT_H: f32 = 22.0;
/// Separator height (`search-bar-separator { height: 16px }`).
const SEP_H: f32 = 16.0;
/// Offsets from the terminal area corner (`top: 4px; right: 8px`).
const TOP_OFFSET: f32 = 4.0;
const RIGHT_OFFSET: f32 = 8.0;

/// The single interaction a search-bar frame can yield. `None` (no
/// variant) is represented by `Option<SearchBarEvent>` at the call site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchBarEvent {
    /// The query text changed (incremental search). Carries the new text.
    QueryChanged(String),
    /// A toggle (regex / case) flipped; the flags on [`SearchState`] are
    /// already updated. The app loop re-runs the search.
    OptionsChanged,
    /// `↓` button — go to the next match.
    Next,
    /// `↑` button — go to the previous match.
    Prev,
    /// `×` button — close the search bar.
    Close,
}

/// Draw the search bar for one frame. Mutates `state.query` /
/// `state.is_regex` / `state.case_sensitive` in place and returns the
/// interaction the caller must act on (re-search / navigate / close).
///
/// `top_inset` is the logical-px height of the chrome stacked above the
/// terminal area (CSD title bar + tab strip); the bar is pinned
/// `TOP_OFFSET` below it so it floats just inside the terminal viewport,
/// matching the WebView's `top: 4px` relative to the terminal root.
///
/// `focus_request` forces keyboard focus into the field and selects all
/// of its text — set on the frame the bar is first shown (or re-opened)
/// so the user can type immediately, mirroring `SearchBar.show()`'s
/// `focus()` + `select()`.
pub fn draw(
    ctx: &egui::Context,
    state: &mut SearchState,
    top_inset: f32,
    focus_request: bool,
) -> Option<SearchBarEvent> {
    let mut event: Option<SearchBarEvent> = None;

    // Pin the bar's top-right corner to the screen's top-right, inset
    // `RIGHT_OFFSET` left and `top_inset + TOP_OFFSET` down — the
    // `top: 4px; right: 8px` rule measured from below the chrome.
    Area::new(Id::new("emterm-search-bar"))
        .order(Order::Foreground)
        .anchor(Align2::RIGHT_TOP, [-RIGHT_OFFSET, top_inset + TOP_OFFSET])
        .show(ctx, |ui| {
            Frame::none()
                .fill(BAR_BG)
                .stroke(Stroke::new(1.0, BAR_BORDER))
                .rounding(Rounding::same(6.0))
                .inner_margin(Margin::symmetric(8.0, 4.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(4.0, 0.0);

                        // ── Text field ──────────────────────────
                        // Reserve the input rect, paint the bg + a border
                        // whose color reflects focus (blue) / error (red) /
                        // rest, then place a frameless TextEdit inside it.
                        // egui's default TextEdit frame can't swap the
                        // border color the way the CSS focus / error rules
                        // do, so we draw it ourselves.
                        let before = state.query.clone();
                        let field_id = Id::new("emterm-search-input");
                        let (outer, _) =
                            ui.allocate_exact_size(Vec2::new(INPUT_W, INPUT_H), Sense::hover());
                        ui.painter()
                            .rect_filled(outer, Rounding::same(3.0), INPUT_BG);
                        let text_edit = egui::TextEdit::singleline(&mut state.query)
                            .id(field_id)
                            .hint_text("Search...")
                            .text_color(INPUT_TEXT)
                            .frame(false)
                            .vertical_align(egui::Align::Center)
                            .margin(Margin::symmetric(6.0, 3.0));
                        let resp = ui.put(outer, text_edit);
                        // Focus invariant: while the bar is open the input
                        // field must always hold keyboard focus. Without
                        // this, clicking the terminal area (or a toolbar
                        // button) moves egui focus off the TextEdit, after
                        // which `handle_search_key` forwards keystrokes to a
                        // non-focused field that silently drops them — they
                        // reach neither the search box nor the PTY. Re-grab
                        // focus every frame it is missing; because this runs
                        // *after* the toggle / nav buttons are laid out
                        // below, a button click in the same frame still
                        // fires, then focus snaps back to the input.
                        let had_focus = resp.has_focus();
                        if !had_focus {
                            resp.request_focus();
                        }
                        // `request_focus` takes effect next frame, but the
                        // field is focused for *display* purposes as soon as
                        // we ask: treat focus as held whenever the field has
                        // it or we just re-armed it. This keeps the border on
                        // its focused color instead of flashing the rest
                        // color for a frame after a click-away. The rest
                        // border (`INPUT_BORDER`) only shows in the degenerate
                        // case where focus could not be acquired at all.
                        let focused = had_focus || resp.has_focus();
                        let border = if state.error.is_some() {
                            INPUT_BORDER_ERROR
                        } else if focused {
                            INPUT_BORDER_FOCUS
                        } else {
                            INPUT_BORDER
                        };
                        ui.painter().rect_stroke(
                            outer,
                            Rounding::same(3.0),
                            Stroke::new(1.0, border),
                        );
                        if focus_request {
                            // Select-all on (re)show: full-range selection
                            // so the next keystroke replaces the old query,
                            // mirroring `SearchBar.show()`'s `select()`.
                            // `request_focus` above already armed focus.
                            select_all(ui, field_id, &state.query);
                        }
                        if state.query != before {
                            event = Some(SearchBarEvent::QueryChanged(state.query.clone()));
                        }

                        // ── Hit count ───────────────────────────
                        // Hidden when the query is empty (WebView shows the
                        // count element but it reads "No results"; we keep
                        // a label so the bar width stays stable).
                        let count_text = if state.query.is_empty() {
                            String::new()
                        } else if state.matches.is_empty() {
                            "No results".to_string()
                        } else {
                            format!("{}/{}", state.current_index.max(0) + 1, state.matches.len())
                        };
                        ui.add_sized(
                            Vec2::new(56.0, BTN_SIZE),
                            egui::Label::new(
                                egui::RichText::new(count_text)
                                    .color(COUNT_FG)
                                    .font(FontId::proportional(11.0)),
                            ),
                        );

                        separator(ui);

                        // ── Regex toggle ".*" ───────────────────
                        if toggle_button(ui, ".*", state.is_regex, "Regular expression") {
                            state.is_regex = !state.is_regex;
                            event = Some(SearchBarEvent::OptionsChanged);
                        }
                        // ── Case toggle "Aa" ────────────────────
                        if toggle_button(ui, "Aa", state.case_sensitive, "Match case") {
                            state.case_sensitive = !state.case_sensitive;
                            event = Some(SearchBarEvent::OptionsChanged);
                        }

                        separator(ui);

                        // ── Prev / Next ─────────────────────────
                        if glyph_button(ui, "\u{2191}", "Previous match (Shift+Enter)") {
                            event = Some(SearchBarEvent::Prev);
                        }
                        if glyph_button(ui, "\u{2193}", "Next match (Enter)") {
                            event = Some(SearchBarEvent::Next);
                        }

                        separator(ui);

                        // ── Close ───────────────────────────────
                        if glyph_button(ui, "\u{2715}", "Close (Esc)") {
                            event = Some(SearchBarEvent::Close);
                        }
                    });
                });
        });

    event
}

/// Select the entire contents of the text field identified by `id`. Used
/// on (re)show so the existing query is replaced by the next keystroke.
fn select_all(ui: &mut egui::Ui, id: Id, text: &str) {
    if let Some(mut state) = egui::text_edit::TextEditState::load(ui.ctx(), id) {
        let range = egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(text.chars().count()),
        );
        state.cursor.set_char_range(Some(range));
        state.store(ui.ctx(), id);
    }
}

/// A flat 24×24 glyph button (prev / next / close). Returns `true` on
/// click. Hover lightens the glyph + paints the faint hover background,
/// matching `.search-bar-btn:hover`.
fn glyph_button(ui: &mut egui::Ui, glyph: &str, tooltip: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(BTN_SIZE), Sense::click());
    let hovered = resp.hovered();
    if hovered {
        ui.painter()
            .rect_filled(rect, Rounding::same(3.0), BTN_BG_HOVER);
    }
    let fg = if hovered { BTN_FG_HOVER } else { BTN_FG };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(12.0),
        fg,
    );
    resp.on_hover_text(tooltip).clicked()
}

/// A 24×24 toggle button (`.*` / `Aa`). `active` paints the blue-tinted
/// active background + border + white glyph; otherwise it behaves like a
/// [`glyph_button`]. Returns `true` on click.
fn toggle_button(ui: &mut egui::Ui, label: &str, active: bool, tooltip: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(BTN_SIZE), Sense::click());
    let hovered = resp.hovered();
    if active {
        ui.painter()
            .rect_filled(rect, Rounding::same(3.0), TOGGLE_ACTIVE_BG);
        ui.painter().rect_stroke(
            rect,
            Rounding::same(3.0),
            Stroke::new(1.0, TOGGLE_ACTIVE_BORDER),
        );
    } else if hovered {
        ui.painter()
            .rect_filled(rect, Rounding::same(3.0), BTN_BG_HOVER);
    }
    let fg = if active {
        TOGGLE_ACTIVE_FG
    } else if hovered {
        BTN_FG_HOVER
    } else {
        BTN_FG
    };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(12.0),
        fg,
    );
    resp.on_hover_text(tooltip).clicked()
}

/// A 1×16 vertical separator (`.search-bar-separator`).
fn separator(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, SEP_H), Sense::hover());
    ui.painter().rect_filled(rect, Rounding::ZERO, SEP);
}
