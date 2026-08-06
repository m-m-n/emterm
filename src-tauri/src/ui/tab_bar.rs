//! Tab bar widget (Phase 4-B; MD3-aligned 2026-05-21).
//!
//! Renders a top panel one row of tabs + a trailing "+" button, mirroring
//! the WebView build's Material Design 3 tab strip (`src/styles/tab-bar.css`):
//!
//! - 48 px tall strip with `surface-container` background and a 1 px
//!   `outline-variant` bottom hairline.
//! - Tabs distribute equally with a 120 px minimum width, padding 24 px
//!   horizontally; horizontal scroll kicks in when the floor would
//!   overflow.
//! - Inactive tabs render with `on-surface-variant`; the active tab
//!   switches to `primary` and grows a 3 px bottom indicator
//!   (`width = cell - 32 px`, 3 px corner radius at the top).
//! - Hover overlays a state-layer (currentColor at 8 % alpha) — same
//!   formula as the WebView `.tab::before`.
//!
//! Title rendering (TS-tab-3):
//!
//! - When `mux_session_name` is `Some(name)`, the rendered title is
//!   `[mux:name] <title>` (single space). When `None`, the title is
//!   rendered verbatim.
//!
//! No per-tab close affordance: the WebView build does not show one
//! either; tabs close via `Ctrl+Shift+W` (the keybind layer emits
//! [`crate::ui::AppAction::CloseTab`]). The trailing `+` icon is
//! drawn with `Painter::line_segment` so the visual is font-independent.

use egui::scroll_area::ScrollBarVisibility;
use egui::{
    Align, Color32, FontId, Image, Layout, Rect, Rounding, ScrollArea, Sense, Stroke, Ui, Vec2,
};

use crate::agent_status::AgentState;
use crate::agent_status_model::Aggregated;

use super::TabEvent;
use super::emoji_cache::EmojiResources;
use super::md3;

/// Fixed visual height of the tab strip in egui logical points.
/// Matches `.tab-bar { height: 48px }` in the WebView build.
pub const TAB_BAR_HEIGHT: f32 = 48.0;

/// Effective tab-bar height for layout math, accounting for the runtime
/// tab-bar visibility (`App::show_tab_bar`, seeded from
/// `settings.show_tab_bar` and flipped by the `ToggleTabBar` keybind).
/// Returns 0 when the bar is hidden so the terminal grid / cursor
/// overlay use the freed vertical space.
pub fn effective_tab_bar_height(show_tab_bar: bool) -> f32 {
    if show_tab_bar { TAB_BAR_HEIGHT } else { 0.0 }
}
/// Minimum width of a single tab before horizontal scroll kicks in.
/// Matches `.tab { min-width: 120px }`.
const MIN_TAB_WIDTH: f32 = 120.0;
/// Maximum width of a single tab.
/// Matches `.tab { max-width: 300px }`.
const MAX_TAB_WIDTH: f32 = 300.0;
/// Horizontal padding inside each tab — matches `.tab { padding: 0 24px }`.
const TAB_HORIZONTAL_PAD: f32 = 24.0;
/// Diameter of the trailing "+" icon button. Matches `.tab-button { 40x40 }`.
const NEW_TAB_BUTTON_SIZE: f32 = 40.0;
/// Side length of the "+" glyph drawn inside the new-tab button.
const PLUS_ICON_SIZE: f32 = 12.0;
/// Stroke width of the "+" glyph. Matches `title_bar`'s icon stroke
/// so the two affordances feel visually paired.
const PLUS_ICON_STROKE_WIDTH: f32 = 1.0;
/// Horizontal padding either side of the fixed-button area.
/// Matches `.tab-fixed-area { padding: 0 8px }`.
const FIXED_AREA_PAD: f32 = 8.0;
/// Height of the bottom 1 px hairline drawn under the strip.
const HAIRLINE_HEIGHT: f32 = 1.0;
/// Tab font size — matches `.tab { font-size: 14px }`.
const TAB_FONT_SIZE: f32 = 14.0;
/// Active-tab underline thickness. Matches `.tab.active::after { height: 3px }`.
const ACTIVE_INDICATOR_HEIGHT: f32 = 3.0;
/// Margin between the left/right edges of the cell and the active
/// indicator, so its width matches the CSS `calc(100% - 32px)`.
const ACTIVE_INDICATOR_SIDE_MARGIN: f32 = 16.0;
/// Corner radius of the active indicator, mirroring `border-radius: 3px 3px 0 0`.
const ACTIVE_INDICATOR_RADIUS: f32 = 3.0;
/// Diameter of the unread-activity dot. Matches `.tab-activity-dot { width/height: 8px }`.
const ACTIVITY_DOT_DIAMETER: f32 = 8.0;
/// Gap between the activity dot and the title. Matches
/// `.tab-activity-dot { margin-right: 6px }`.
const ACTIVITY_DOT_MARGIN: f32 = 6.0;
/// Activity-dot show/hide animation duration in seconds. Matches the
/// WebView's `--md-motion-duration-short4` (250 ms) opacity/scale
/// transition.
const ACTIVITY_DOT_ANIM_SECS: f32 = 0.25;
/// Icon-button (state-layer) corner radius — MD3 uses a full pill so the
/// 8 % overlay forms a circle inside the 40 px square.
const ICON_BUTTON_RADIUS: f32 = NEW_TAB_BUTTON_SIZE / 2.0;

/// Agent-status badge dot diameter (task0006, `IMPLEMENTATION.md`
/// Conventions: "Badge: 8px dot, 6px gap before title"). Still the size
/// of every state's fallback circle form (agent-badge-emoji task0001
/// D3); the badge *slot* itself is [`AGENT_BADGE_SLOT_WIDTH`]
/// (agent-badge-emoji-distinction task0001 Design 4 — the slot widened
/// to fit the emoji forms, the dot stays 8px and centers within it).
const AGENT_BADGE_DIAMETER: f32 = 8.0;
/// Unified badge slot width when any badge is present (task0001 Design 4:
/// "Unified badge slot") — the reserved layout width for ALL agent
/// states, in both the tab bar and the mux sidebar, so a state
/// transition never shifts the title. Every state's emoji renders
/// aspect-fit inside this 12px slot; the 8px fallback circle (filled dot
/// or ring — agent-badge-emoji task0001 D3) centers within it when no
/// emoji texture is obtainable.
pub const AGENT_BADGE_SLOT_WIDTH: f32 = 12.0;
/// Gap between the agent badge and whatever follows it (the activity dot,
/// or directly the title when no activity dot is present).
const AGENT_BADGE_GAP: f32 = 6.0;
/// Ring stroke width for a *seen* blocked/done badge's fallback circle
/// (AC-1: "seen render as ring"). `IMPLEMENTATION.md` Conventions pins
/// 1.5px.
const AGENT_BADGE_RING_WIDTH: f32 = 1.5;
/// Grapheme cluster rendered for the `working` state — U+26A1 HIGH
/// VOLTAGE SIGN. Single-codepoint, default-emoji-presentation (no VS-16),
/// covered by the bundled Noto Color Emoji bitmap strikes (task0001
/// Design 1).
pub const WORKING_BADGE_EMOJI: &str = "\u{26A1}";
/// Grapheme cluster rendered for the `idle` state, and for `done` once
/// seen (agent-badge-emoji task0001 FR2) — U+1F4A4 ZZZ.
pub const IDLE_BADGE_EMOJI: &str = "\u{1F4A4}";
/// Grapheme cluster rendered for the `blocked` state while unseen
/// (agent-badge-emoji task0001 FR1) — U+2753 BLACK QUESTION MARK
/// ORNAMENT. Single-codepoint, default-emoji-presentation (no VS-16),
/// same format as [`WORKING_BADGE_EMOJI`] / [`IDLE_BADGE_EMOJI`] (FR6).
pub const BLOCKED_BADGE_EMOJI_UNSEEN: &str = "\u{2753}";
/// Grapheme cluster rendered for the `blocked` state once seen
/// (agent-badge-emoji task0001 FR1) — U+2754 WHITE QUESTION MARK
/// ORNAMENT.
pub const BLOCKED_BADGE_EMOJI_SEEN: &str = "\u{2754}";
/// Grapheme cluster rendered for the `done` state while unseen
/// (agent-badge-emoji task0001 FR2) — U+2705 WHITE HEAVY CHECK MARK.
/// `done` once seen reuses [`IDLE_BADGE_EMOJI`] rather than a dedicated
/// constant (FR6).
pub const DONE_BADGE_EMOJI_UNSEEN: &str = "\u{2705}";

/// Minimal projection of [`crate::tabs::Tab`] used by the tab bar.
///
/// Constructed once per frame by the app loop. Tests construct these
/// directly.
#[derive(Debug, Clone)]
pub struct TabBarItem {
    /// PTY title (OSC-supplied) or `"shell"` fallback.
    pub title: String,
    /// When `Some`, the tab is in mux mode and the title is prefixed
    /// with `[mux:<session>]` before rendering. Populated by Phase 4-C
    /// once the mux client is wired; Phase 4-B leaves this `None`.
    pub mux_session_name: Option<String>,
    /// When `true`, an unread-activity dot renders left of the title
    /// (mirrors `.tab-activity-dot.visible`). The view-model builder
    /// applies the `settings.tab_activity_indicator` gate, so the
    /// widget just draws what it is told.
    pub has_activity: bool,
    /// Stable per-tab identity (`crate::tabs::Tab::stable_id`) keying
    /// egui animation state. Positional indices shift on tab close /
    /// drag-reorder, which would bleed in-flight dot animations between
    /// tabs; titles are not unique (every fresh tab is "shell"). The
    /// view-model builder MUST set this via `with_stable_id`.
    pub stable_id: u64,
    /// When `Some`, this tab is a mux tab group and renders one sub-tab per
    /// window (`[N] name`) instead of the plain title cell. Built by
    /// [`mux_group_render_model`] from the tab's
    /// [`crate::mux::window_group::MuxWindowGroup`] whenever the group holds
    /// at least one window (FR1, WebView parity). `None` leaves the plain-tab
    /// path untouched.
    ///
    /// mux-vertical-tabs task0005: production code (`render::draw_terminal`)
    /// no longer populates this field — the mux tab group's window list
    /// moved to the `ui::mux_sidebar` widget, and the tab-bar cell collapses
    /// to a single cell labelled via [`Self::mux_active_window_name`]
    /// instead (IMPLEMENTATION.md D1). The field, [`with_mux_cells`], and
    /// the inline sub-tab expansion below are kept in place per that same
    /// decision (reusable render-model, exercised by the tests in this
    /// module) but are inert against any `TabBarItem` the app actually
    /// constructs.
    ///
    /// [`with_mux_cells`]: Self::with_mux_cells
    pub mux_cells: Option<Vec<MuxSubTabCell>>,
    /// task0006 AC-1/AC-2: this tab's aggregated agent-status badge —
    /// highest-priority state across the tab's own status and (for a
    /// mux-attached tab) every pane in its window group. `None` when
    /// nothing has ever reported a state: no badge renders and no layout
    /// space is reserved for it (unlike [`Self::has_activity`]'s dot,
    /// which always occupies its slot). The view-model builder sets this
    /// via [`with_agent_badge`](Self::with_agent_badge).
    pub agent_badge: Option<Aggregated>,
    /// When `Some(name)`, this tab is a mux tab group collapsed to a single
    /// cell (task0005 AC-1): the rendered label becomes `mux: <name>`,
    /// overriding both the plain `title` and the `mux_session_name` prefix
    /// format. `name` is the group's active window's display name, already
    /// live (OSC-renamed) since the caller rebuilds it every frame from
    /// [`crate::mux::window_group::MuxWindowGroup::active_window`].
    pub mux_active_window_name: Option<String>,
}

impl TabBarItem {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            mux_session_name: None,
            has_activity: false,
            stable_id: 0,
            mux_cells: None,
            agent_badge: None,
            mux_active_window_name: None,
        }
    }

    pub fn with_mux_session(mut self, name: impl Into<String>) -> Self {
        self.mux_session_name = Some(name.into());
        self
    }

    pub fn with_activity(mut self, has_activity: bool) -> Self {
        self.has_activity = has_activity;
        self
    }

    pub fn with_stable_id(mut self, id: u64) -> Self {
        self.stable_id = id;
        self
    }

    /// Mark this tab as a mux tab group rendered from `cells`. An empty
    /// vec is treated as "not a group" (the plain title renders).
    pub fn with_mux_cells(mut self, cells: Vec<MuxSubTabCell>) -> Self {
        self.mux_cells = if cells.is_empty() { None } else { Some(cells) };
        self
    }

    /// Mark this tab as a collapsed mux tab group whose single cell is
    /// labelled `mux: <name>` (task0005 AC-1).
    pub fn with_mux_active_window_name(mut self, name: impl Into<String>) -> Self {
        self.mux_active_window_name = Some(name.into());
        self
    }

    /// Attach this tab's aggregated agent-status badge (task0006 AC-1).
    pub fn with_agent_badge(mut self, badge: Option<Aggregated>) -> Self {
        self.agent_badge = badge;
        self
    }
}

/// Color role for a semantic agent state (task0006 AC-4, `IMPLEMENTATION.md`
/// Conventions): blocked -> `on_error_container`, working -> `primary`,
/// done -> `on_secondary_container`, idle -> `on_surface_variant`. Shared by
/// [`ui::mux_sidebar`](super::mux_sidebar) and
/// [`ui::status_bar`](super::status_bar) so the mapping lives in one place.
pub fn agent_state_color(state: AgentState) -> Color32 {
    match state {
        AgentState::Blocked => md3::on_error_container(),
        AgentState::Working => md3::primary(),
        AgentState::Done => md3::on_secondary_container(),
        AgentState::Idle => md3::on_surface_variant(),
    }
}

/// Whether a badge for `agg` renders as a filled dot (`true`) or a
/// [`AGENT_BADGE_RING_WIDTH`] ring (`false`) — task0006 AC-1: "unseen
/// blocked/done render filled, seen render as ring; working / idle have a
/// single (filled / muted) form" (idle's "muted" look comes from its color,
/// `on_surface_variant`, not from a different dot shape — both working and
/// idle always render filled).
pub fn agent_badge_filled(agg: Aggregated) -> bool {
    match agg.state {
        AgentState::Blocked | AgentState::Done => agg.unseen,
        AgentState::Working | AgentState::Idle => true,
    }
}

/// Presentation kind a badge value renders as (task0001 Design 1, AC-1;
/// agent-badge-emoji task0001 D2 unifies all four states onto this single
/// variant). `cluster` is the color emoji to render; `fallback_filled`
/// carries the circle SHAPE (filled dot vs. ring) to fall back to when no
/// emoji texture is obtainable — it mirrors [`agent_badge_filled`]'s
/// semantics for `agg`'s state (agent-badge-emoji task0001 D3). The ONE
/// shared decision, consumed by both the tab bar and the mux sidebar
/// painters (NFR1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgePresentation {
    /// Render `cluster` as color emoji; fall back to a
    /// `fallback_filled`-shaped circle when no texture is obtainable.
    Emoji {
        cluster: &'static str,
        fallback_filled: bool,
    },
}

/// Choose the presentation for `agg` (task0001 Design 1, AC-1) — total
/// over all four agent states, no side effects, callable from unit tests
/// without any UI context (TS1). All four states resolve to
/// [`BadgePresentation::Emoji`] (agent-badge-emoji task0001 D2): working
/// (`WORKING_BADGE_EMOJI`) / idle (`IDLE_BADGE_EMOJI`) unseen/seen alike;
/// blocked (`BLOCKED_BADGE_EMOJI_UNSEEN` / `_SEEN`); done
/// (`DONE_BADGE_EMOJI_UNSEEN` unseen, `IDLE_BADGE_EMOJI` once seen — FR2).
pub fn badge_presentation(agg: Aggregated) -> BadgePresentation {
    let cluster = match agg.state {
        AgentState::Working => WORKING_BADGE_EMOJI,
        AgentState::Idle => IDLE_BADGE_EMOJI,
        AgentState::Blocked => {
            if agg.unseen {
                BLOCKED_BADGE_EMOJI_UNSEEN
            } else {
                BLOCKED_BADGE_EMOJI_SEEN
            }
        }
        AgentState::Done => {
            if agg.unseen {
                DONE_BADGE_EMOJI_UNSEEN
            } else {
                IDLE_BADGE_EMOJI
            }
        }
    };
    BadgePresentation::Emoji {
        cluster,
        fallback_filled: agent_badge_filled(agg),
    }
}

/// Final render mode resolved from a selected [`BadgePresentation`] plus
/// texture availability (task0001 Design 2, AC-2, FR3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeRenderMode {
    /// Blit the emoji texture.
    EmojiTexture,
    /// Draw the fallback circle form.
    Circle { filled: bool },
}

/// Resolve a [`BadgePresentation`] plus whether a texture was obtainable
/// (the cache produced one, or not — including "no emoji resources
/// supplied", the test-context case) into the final render mode
/// (task0001 Design 2, AC-2). Never resolves to a blank slot and never
/// the toolkit's default text path (FR3): an `Emoji` presentation with no
/// obtainable texture falls back to its carried `fallback_filled` circle
/// shape (agent-badge-emoji task0001 D3) — always `filled: true` for
/// working/idle, and unseen/seen-dependent for blocked/done.
pub fn resolve_badge_render_mode(
    presentation: BadgePresentation,
    texture_available: bool,
) -> BadgeRenderMode {
    match presentation {
        BadgePresentation::Emoji { .. } if texture_available => BadgeRenderMode::EmojiTexture,
        BadgePresentation::Emoji { fallback_filled, .. } => {
            BadgeRenderMode::Circle { filled: fallback_filled }
        }
    }
}

/// Paint one badge in `slot_center`'s [`AGENT_BADGE_SLOT_WIDTH`]-wide slot
/// (task0001 Design 4: "Unified badge slot"), choosing an untinted emoji
/// texture blit or the fallback circle per [`badge_presentation`] /
/// [`resolve_badge_render_mode`]. `emoji` is `None` in tests that don't
/// stand up a font stack — that always resolves to the fallback circle
/// (AC-2), matching the established status-bar pattern.
pub fn paint_agent_badge(
    ui: &Ui,
    slot_center: egui::Pos2,
    agg: Aggregated,
    emoji: Option<&EmojiResources<'_>>,
) {
    let presentation = badge_presentation(agg);

    let BadgePresentation::Emoji { cluster, .. } = presentation;
    let texture = emoji.and_then(|em| {
        // Design 4 "Size": rasterize at 12 × the display's
        // pixels-per-point, in physical px.
        let ppp = ui.ctx().pixels_per_point();
        let raster_px = AGENT_BADGE_SLOT_WIDTH * ppp;
        em.cache
            .lock()
            .get_or_rasterize(ui.ctx(), em.rasterizer, em.fallback, cluster, raster_px)
    });

    match resolve_badge_render_mode(presentation, texture.is_some()) {
        BadgeRenderMode::EmojiTexture => {
            let texture = texture.expect("EmojiTexture mode implies a resolved texture");
            let ppp = ui.ctx().pixels_per_point();
            // Draw at the texture's exact integer physical size (texels /
            // ppp) rather than a fractional aspect-fit scale: rasterization
            // is already requested at `AGENT_BADGE_SLOT_WIDTH * ppp`, so
            // the texture already fits the slot almost exactly. A second
            // non-integer downscale on top of the cache's own Lanczos3
            // downscale would needlessly blur a 12px glyph.
            let mut draw_size = texture.size_vec2() / ppp;
            // Safety clamp: aspect-fit into the slot. Divide by the EXACT
            // overflow ratio — `ceil()` would halve a bitmap that is only
            // one texel wider than the slot (ratio ~1.08), which is the
            // common bundled-Noto-Color-Emoji case (non-square strike).
            let overflow_ratio =
                (draw_size.x / AGENT_BADGE_SLOT_WIDTH).max(draw_size.y / AGENT_BADGE_SLOT_WIDTH);
            if overflow_ratio > 1.0 {
                draw_size /= overflow_ratio;
            }
            // Snap the paint rect's origin to the physical-pixel grid
            // (mirrors `status_bar.rs::emit_emoji_cluster_chain`'s `snap`
            // closure): `draw_size` is an exact-integer physical size, so
            // snapping the min corner lands both edges on pixel
            // boundaries. Without this the sub-pixel offset of
            // `slot_center` (derived from fractional layout coordinates)
            // would blend with neighbouring texels under
            // `TextureOptions::LINEAR`.
            let snap = |v: f32| (v * ppp).round() / ppp;
            let unsnapped = Rect::from_center_size(slot_center, draw_size);
            let rect = Rect::from_min_size(
                egui::pos2(snap(unsnapped.min.x), snap(unsnapped.min.y)),
                draw_size,
            );
            // Untinted (Design 4): no color argument — the emoji's own
            // color table is blitted as-is.
            Image::new(&texture).paint_at(ui, rect);
        }
        BadgeRenderMode::Circle { filled } => {
            let color = agent_state_color(agg.state);
            let radius = AGENT_BADGE_DIAMETER / 2.0;
            if filled {
                ui.painter().circle_filled(slot_center, radius, color);
            } else {
                ui.painter().circle_stroke(
                    slot_center,
                    radius - AGENT_BADGE_RING_WIDTH / 2.0,
                    Stroke::new(AGENT_BADGE_RING_WIDTH, color),
                );
            }
        }
    }
}

/// Compute the displayed label for a tab. Pure helper, kept public so
/// TS-tab-3 can exercise it directly without driving egui.
///
/// task0005 AC-1: a collapsed mux tab group (`mux_active_window_name`
/// `Some`) renders `mux: <active window name>`, taking precedence over both
/// the plain title and the `[mux:<session>]` prefix format below (the
/// latter only remains visible for the brief pre-window-list-populated
/// window, where `mux_session_name` is set but the group has no windows
/// yet).
pub fn render_label(item: &TabBarItem) -> String {
    if let Some(name) = &item.mux_active_window_name {
        return format!("mux: {name}");
    }
    match &item.mux_session_name {
        Some(session) => format!("[mux:{}] {}", session, item.title),
        None => item.title.clone(),
    }
}

/// Render the tab bar into a top panel, returning at most one
/// [`TabEvent`] this frame.
///
/// `scroll_active_into_view` (FR4) is a one-shot signal raised by the app's
/// keyboard tab/window switch handlers: when `true`, the strip scrolls the
/// active visual cell into view exactly once this frame. The caller
/// (`render::draw_terminal`) reads the value from `App`; the flag is cleared
/// post-frame in `window_host` (where `&mut App` is available), so passing a
/// stale `true` here is never a problem — it only matters for the frame the
/// app raised it.
pub fn draw(
    ctx: &egui::Context,
    items: &[TabBarItem],
    active_idx: usize,
    scroll_active_into_view: bool,
    emoji: Option<&EmojiResources<'_>>,
) -> Option<TabEvent> {
    let mut event: Option<TabEvent> = None;

    let frame = egui::Frame::none()
        .fill(md3::surface_container())
        .inner_margin(egui::Margin::ZERO);

    egui::TopBottomPanel::top("native-poc-tab-bar")
        .frame(frame)
        .exact_height(TAB_BAR_HEIGHT)
        .show_separator_line(false)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;

            // Total room for the scrollable tab strip (everything minus the
            // fixed "+" / gear area on the right).
            let panel_w = ui.available_width();
            let fixed_w = NEW_TAB_BUTTON_SIZE * 2.0 + FIXED_AREA_PAD * 2.0;
            let scroll_w = (panel_w - fixed_w).max(0.0);

            // Hairline at the very bottom — drawn last so it stays on top
            // of the per-tab fills.
            let panel_rect = ui.max_rect();

            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                // ── Tab strip ───────────────────────────────────────
                // A mux tab group expands into multiple visual cells
                // (compact → 1, expanded → header + one per window), so the
                // width math counts cells, not roster entries.
                let n = visual_cell_count(items).max(1) as f32;
                let ideal_w = (scroll_w / n).clamp(MIN_TAB_WIDTH, MAX_TAB_WIDTH);
                let needed_w = MIN_TAB_WIDTH * n;

                // Horizontal scroll only engages when the floor (MIN ×
                // count) exceeds the available strip width. Keeping the
                // common path scroll-free preserves a predictable cell
                // origin for the click-to-tab tests below.
                // The strip always occupies the full scroll_w span (even
                // when the tabs need less) so the fixed "+" / gear area
                // that follows stays pinned to the panel's right edge,
                // mirroring the WebView's `.tab-fixed-area`.
                if needed_w > scroll_w {
                    ui.allocate_ui_with_layout(
                        Vec2::new(scroll_w, TAB_BAR_HEIGHT),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            // FR2: the strip is a horizontal-only `ScrollArea`,
                            // so egui's default (`always_scroll_the_only_direction
                            // = false`) ignores a plain (no-modifier) vertical
                            // wheel delta. Enabling it on this scope folds the
                            // vertical wheel onto the single (horizontal) axis,
                            // so a hovered wheel scrolls the strip. egui reads
                            // this flag from the `ui` that `ScrollArea::show` is
                            // called on, so set it here before `.show`.
                            // FR3 (Shift+wheel) folds onto the horizontal axis
                            // via this same flag: the tab-bar wheel forward in
                            // `window_host` strips the modifier, so egui's
                            // input-layer shift→horizontal swap never fires —
                            // the horizontal scroll comes purely from this
                            // fold, shift or not.
                            ui.style_mut().always_scroll_the_only_direction = true;
                            ScrollArea::horizontal()
                                .id_salt("native-poc-tab-strip")
                                .auto_shrink([false, false])
                                // FR1: keep the strip horizontally scrollable but
                                // never paint a scrollbar (WebView parity — the
                                // CSS strip hides its scrollbar too).
                                .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                        event = layout_tab_strip(
                                            ui,
                                            items,
                                            active_idx,
                                            MIN_TAB_WIDTH,
                                            scroll_active_into_view,
                                            emoji,
                                        );
                                    });
                                });
                        },
                    );
                } else {
                    ui.allocate_ui_with_layout(
                        Vec2::new(scroll_w, TAB_BAR_HEIGHT),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            // `allocate_ui_with_layout` only advances the
                            // parent cursor by what the child actually
                            // used; pin the child's min width so the
                            // fixed-button area lands at the right edge
                            // even when the tabs need less room.
                            ui.set_min_width(scroll_w);
                            ui.spacing_mut().item_spacing = Vec2::ZERO;
                            event = layout_tab_strip(
                                ui,
                                items,
                                active_idx,
                                ideal_w,
                                scroll_active_into_view,
                                emoji,
                            );
                        },
                    );
                }

                // ── Fixed-button area ("+") ─────────────────────────
                ui.add_space(FIXED_AREA_PAD);
                // 1 px vertical separator on the left edge of the fixed area
                // mirrors `.tab-fixed-area { border-left }`.
                let sep_x = ui.cursor().min.x - FIXED_AREA_PAD;
                ui.painter().vline(
                    sep_x,
                    panel_rect.top()..=(panel_rect.bottom() - HAIRLINE_HEIGHT),
                    Stroke::new(1.0, md3::outline_variant()),
                );

                let plus_resp = draw_icon_button(ui, NEW_TAB_BUTTON_SIZE);
                #[cfg(test)]
                {
                    tests::LAST_PLUS_RECT.with(|c| c.set(Some(plus_resp.rect)));
                }
                if plus_resp.clicked() && event.is_none() {
                    event = Some(TabEvent::New);
                }
                // Gear button — open (or focus) the Settings tab.
                // Mirrors the WebView `.tab-button-settings` next to "+".
                let gear_resp = draw_gear_button(ui, NEW_TAB_BUTTON_SIZE);
                if gear_resp.clicked() && event.is_none() {
                    event = Some(TabEvent::OpenSettings);
                }
                ui.add_space(FIXED_AREA_PAD);
            });

            // Bottom 1 px hairline (outline-variant).
            let painter = ui.painter();
            let y = panel_rect.bottom() - HAIRLINE_HEIGHT / 2.0;
            painter.hline(
                panel_rect.left()..=panel_rect.right(),
                y,
                Stroke::new(HAIRLINE_HEIGHT, md3::outline_variant()),
            );
        });

    event
}

/// Persistent key under which the current drag origin (`Option<usize>`)
/// is stored in egui's frame memory. Survives across frames so the
/// pending drag is observed by every layout pass until the pointer is
/// released.
const DRAG_FROM_KEY: &str = "native-poc-tab-drag-from";

fn drag_state_id() -> egui::Id {
    egui::Id::new(DRAG_FROM_KEY)
}

/// One drawable cell in the strip: either a plain roster tab or a single
/// cell of a mux tab group belonging to a roster tab.
enum Visual {
    /// Plain roster tab at index `item`.
    Tab { item: usize },
    /// Mux group cell at position `cell` within `items[tab].mux_cells`.
    Mux { tab: usize, cell: usize },
}

/// Count the visual cells the strip renders: a plain tab is one cell; a
/// mux group expands to its cell count (compact → 1, expanded → header +
/// one per window). Used so the equal-width layout math accounts for the
/// expansion. With no mux groups this equals `items.len()`.
fn visual_cell_count(items: &[TabBarItem]) -> usize {
    items
        .iter()
        .map(|it| match &it.mux_cells {
            Some(cells) if !cells.is_empty() => cells.len(),
            _ => 1,
        })
        .sum()
}

/// Flatten the roster into the ordered visual cells the strip draws.
fn build_visuals(items: &[TabBarItem]) -> Vec<Visual> {
    let mut visuals = Vec::with_capacity(visual_cell_count(items));
    for (i, item) in items.iter().enumerate() {
        match &item.mux_cells {
            Some(cells) if !cells.is_empty() => {
                for c in 0..cells.len() {
                    visuals.push(Visual::Mux { tab: i, cell: c });
                }
            }
            _ => visuals.push(Visual::Tab { item: i }),
        }
    }
    visuals
}

/// Draw the active-tab / active-sub-tab indicator: a 3 px primary bar at
/// the bottom, side-margined to match the WebView `width: calc(100% - 32px)`.
fn paint_active_indicator(ui: &Ui, rect: Rect) {
    #[cfg(test)]
    tests::LAST_INDICATOR_RECTS.with(|c| c.borrow_mut().push(rect));
    let bar = Rect::from_min_max(
        egui::pos2(
            rect.left() + ACTIVE_INDICATOR_SIDE_MARGIN,
            rect.bottom() - ACTIVE_INDICATOR_HEIGHT - HAIRLINE_HEIGHT,
        ),
        egui::pos2(
            rect.right() - ACTIVE_INDICATOR_SIDE_MARGIN,
            rect.bottom() - HAIRLINE_HEIGHT,
        ),
    );
    ui.painter().rect_filled(
        bar,
        Rounding {
            nw: ACTIVE_INDICATOR_RADIUS,
            ne: ACTIVE_INDICATOR_RADIUS,
            sw: 0.0,
            se: 0.0,
        },
        md3::primary(),
    );
}

/// Lay out `text` in `font_id` / `color`, ellipsizing with `…` when it
/// overflows `max_w`. Uses a binary search over char boundaries (O(log N)
/// layouts, one allocation for the winning candidate) instead of the
/// naive char-pop loop (O(N²) plus N `format!` allocations per frame). The
/// tab strip calls this on every cell every frame, so the cost matters
/// when window names are long.
fn layout_ellipsized(
    fonts: &egui::text::Fonts,
    text: &str,
    font_id: &FontId,
    color: egui::Color32,
    max_w: f32,
) -> std::sync::Arc<egui::Galley> {
    let full = fonts.layout_no_wrap(text.to_string(), font_id.clone(), color);
    if full.size().x <= max_w || text.is_empty() {
        return full;
    }
    let ell = "…";
    let char_offsets: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect();
    let mut lo = 0usize;
    let mut hi = char_offsets.len().saturating_sub(2);
    let mut best: Option<std::sync::Arc<egui::Galley>> = None;
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        let candidate = &text[..char_offsets[mid]];
        let g = fonts.layout_no_wrap(format!("{candidate}{ell}"), font_id.clone(), color);
        if g.size().x <= max_w {
            best = Some(g);
            lo = mid + 1;
        } else {
            if mid == 0 {
                break;
            }
            hi = mid - 1;
        }
    }
    best.unwrap_or(full)
}

/// Paint a single-line label centred in `rect`, ellipsizing when it
/// overflows the horizontal padding box. Used for mux group cells (which
/// carry no activity dot, unlike the plain-tab path).
fn paint_centered_label(ui: &Ui, rect: Rect, text: &str, color: egui::Color32) {
    let font_id = FontId::proportional(TAB_FONT_SIZE);
    let label_left = rect.left() + TAB_HORIZONTAL_PAD;
    let label_right = rect.right() - TAB_HORIZONTAL_PAD;
    let label_rect = Rect::from_min_max(
        egui::pos2(label_left, rect.top()),
        egui::pos2(label_right.max(label_left), rect.bottom()),
    );
    let max_w = label_rect.width().max(0.0);
    let galley = ui.fonts(|fonts| layout_ellipsized(fonts, text, &font_id, color, max_w));
    let pos = egui::pos2(
        label_rect.center().x - galley.size().x / 2.0,
        label_rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(pos, galley, color);
}

/// Inner layout: lay out one tab cell per item. Returns at most one
/// [`TabEvent`] this frame.
fn layout_tab_strip(
    ui: &mut Ui,
    items: &[TabBarItem],
    active_idx: usize,
    tab_width: f32,
    scroll_active_into_view: bool,
    emoji: Option<&EmojiResources<'_>>,
) -> Option<TabEvent> {
    let mut event: Option<TabEvent> = None;
    let drag_id = drag_state_id();
    let mut drag_from: Option<usize> = ui.ctx().memory(|m| m.data.get_temp(drag_id));

    // FR4: the active visual cell's rect, captured during the layout pass so a
    // single `scroll_to_rect` can pull it into view when the flag is set. The
    // active cell is the plain-tab cell at `active_idx`, or — inside the active
    // mux tab — the active window's sub-tab cell.
    let mut active_cell_rect: Option<Rect> = None;

    let visuals = build_visuals(items);
    // Drag-reorder applies to plain-tab cells only — mux sub-tab cells (a
    // group's expanded `[N] name` cells) carry `Sense::click()` not
    // `click_and_drag()`, so the cell-level drag is naturally restricted
    // there. The post-loop drop math works over `cell_rects` (plain tabs
    // only) and uses `cell_rosters[cell_idx]` to map the drop-target cell
    // index back to a roster insertion point. With this mapping, plain-tab
    // reorder stays live even when one tab is expanded as a mux group
    // (pre-fix the whole drag path was disabled globally when any tab was
    // mux-attached — coarse coupling between unrelated features).
    let mut cell_rects: Vec<Rect> = Vec::with_capacity(items.len());
    let mut cell_rosters: Vec<usize> = Vec::with_capacity(items.len());

    #[cfg(test)]
    tests::LAST_TAB_CELLS.with(|c| c.borrow_mut().clear());
    #[cfg(test)]
    tests::LAST_MUX_CELLS.with(|c| c.borrow_mut().clear());
    #[cfg(test)]
    tests::LAST_INDICATOR_RECTS.with(|c| c.borrow_mut().clear());

    for visual in &visuals {
        let i = match *visual {
            Visual::Tab { item } => item,
            Visual::Mux { tab, cell } => {
                // ── mux window sub-tab (`[N] name`) ─────────────────
                let mux_cell = &items[tab].mux_cells.as_ref().expect("mux group cells")[cell];
                let cell_size = Vec2::new(tab_width, TAB_BAR_HEIGHT);
                let (rect, cell_resp) = ui.allocate_exact_size(cell_size, Sense::click());

                #[cfg(test)]
                tests::LAST_MUX_CELLS.with(|c| c.borrow_mut().push(rect));

                let is_active_cell = mux_cell.active;
                let color = if is_active_cell {
                    md3::primary()
                } else {
                    md3::on_surface_variant()
                };

                if cell_resp.hovered() {
                    ui.painter().rect_filled(
                        rect,
                        Rounding::ZERO,
                        md3::state_layer(color, md3::STATE_LAYER_HOVER),
                    );
                }
                paint_centered_label(ui, rect, &mux_sub_tab_label(mux_cell), color);
                // FR5: paint the sub-tab active-indicator bar only when this
                // mux group's parent tab is the active tab. A non-active mux
                // parent shows no bar, so exactly one indicator is visible
                // across the whole strip. The label color above keeps its
                // existing `mux_cell.active`-based emphasis (only the bar is
                // gated). `tab` and `active_idx` are plain indices and
                // `mux_cell.active` is a copied bool, so the gate never touches
                // `MuxWindowGroup` (active-window state is unchanged).
                if tab == active_idx && is_active_cell {
                    paint_active_indicator(ui, rect);
                    // FR4: the active visual cell inside the active mux tab.
                    active_cell_rect = Some(rect);
                }

                // Click switches to this window (WebView parity: sub-tab
                // click → switch; there is no compact/expand toggle).
                if cell_resp.clicked() && event.is_none() {
                    event = Some(TabEvent::MuxSwitch {
                        tab,
                        window: mux_cell.index,
                    });
                }
                continue;
            }
        };

        let item = &items[i];
        let is_active = i == active_idx;
        let cell_size = Vec2::new(tab_width, TAB_BAR_HEIGHT);
        let (rect, cell_resp) = ui.allocate_exact_size(cell_size, Sense::click_and_drag());

        cell_rects.push(rect);
        cell_rosters.push(i);
        #[cfg(test)]
        tests::LAST_TAB_CELLS.with(|c| c.borrow_mut().push(rect));

        // Detect drag start. egui's `drag_started_by` fires the frame
        // after the pointer exceeds the click-vs-drag distance, so a
        // simple click does not enter drag mode.
        if drag_from.is_none() && cell_resp.drag_started_by(egui::PointerButton::Primary) {
            drag_from = Some(i);
            ui.ctx().memory_mut(|m| m.data.insert_temp(drag_id, i));
        }

        // Background — the strip itself inherits `surface-container` from
        // the parent panel frame; we only paint the hover state-layer.
        // Tabs currently being dragged dim slightly so the user knows
        // which one they picked up.
        let painter = ui.painter();
        if drag_from == Some(i) {
            painter.rect_filled(
                rect,
                Rounding::ZERO,
                md3::state_layer(md3::primary(), md3::STATE_LAYER_HOVER),
            );
        } else if cell_resp.hovered() {
            painter.rect_filled(
                rect,
                Rounding::ZERO,
                md3::state_layer(
                    if is_active {
                        md3::primary()
                    } else {
                        md3::on_surface_variant()
                    },
                    md3::STATE_LAYER_HOVER,
                ),
            );
        }

        // Label sub-rect. Drawn via the painter directly so the
        // parent layout's cursor is not perturbed (ui.put would
        // shift subsequent allocations).
        let label_left = rect.left() + TAB_HORIZONTAL_PAD;
        let label_right = rect.right() - TAB_HORIZONTAL_PAD;
        let label_rect = Rect::from_min_max(
            egui::pos2(label_left, rect.top()),
            egui::pos2(label_right.max(label_left), rect.bottom()),
        );

        let label_text = render_label(item);
        let text_color = if is_active {
            md3::primary()
        } else {
            md3::on_surface_variant()
        };
        let font_id = FontId::proportional(TAB_FONT_SIZE);
        // Agent-status badge slot (task0006 AC-1/AC-2): unlike the activity
        // dot below, this slot is only reserved when a badge is present —
        // no reserved space and no layout shift for a tab that has never
        // reported a state.
        let agent_dot_space = if item.agent_badge.is_some() {
            AGENT_BADGE_SLOT_WIDTH + AGENT_BADGE_GAP
        } else {
            0.0
        };
        // Activity-dot slot. Like the WebView flexbox (`.tab-activity-dot`
        // hides via opacity/scale, not display:none), the 8 px dot +
        // 6 px gap always occupy layout space so the title does not
        // shift when the dot appears.
        let dot_space = ACTIVITY_DOT_DIAMETER + ACTIVITY_DOT_MARGIN;
        // egui has no native truncation helper for direct painter text,
        // so we measure with `Fonts::layout_no_wrap` and ellipsize when
        // the result overflows the label rect.
        let max_w = (label_rect.width() - agent_dot_space - dot_space).max(0.0);
        let galley =
            ui.fonts(|fonts| layout_ellipsized(fonts, &label_text, &font_id, text_color, max_w));
        // Centre the [agent badge][dot][gap][title] group as one unit,
        // mirroring the WebView's `justify-content: center` flex row.
        let group_w = agent_dot_space + dot_space + galley.size().x;
        let group_left = label_rect.center().x - group_w / 2.0;

        if let Some(badge) = item.agent_badge {
            let badge_center = egui::pos2(
                group_left + AGENT_BADGE_SLOT_WIDTH / 2.0,
                label_rect.center().y,
            );
            paint_agent_badge(ui, badge_center, badge, emoji);
        }
        let after_agent_badge = group_left + agent_dot_space;

        // Dot show/hide animates scale + opacity over 250 ms — the
        // `.tab-activity-dot` transition. `animate_bool_with_time`
        // requests repaints while in flight, so the fade plays out
        // without an explicit redraw hook. Keyed on the tab's stable
        // identity (NOT the positional index, which shifts on tab
        // close / reorder and would bleed animation state across tabs).
        let dot_t = ui.ctx().animate_bool_with_time(
            egui::Id::new(("native-poc-tab-activity-dot", item.stable_id)),
            item.has_activity,
            ACTIVITY_DOT_ANIM_SECS,
        );
        if dot_t > 0.0 {
            let dot_center = egui::pos2(
                after_agent_badge + ACTIVITY_DOT_DIAMETER / 2.0,
                label_rect.center().y,
            );
            ui.painter().circle_filled(
                dot_center,
                (ACTIVITY_DOT_DIAMETER / 2.0) * dot_t,
                md3::primary().gamma_multiply(dot_t),
            );
        }

        let text_x = after_agent_badge + dot_space;
        let text_y = label_rect.center().y - galley.size().y / 2.0;
        ui.painter()
            .galley(egui::pos2(text_x, text_y), galley, text_color);

        // Single click responder for the whole cell switches tabs.
        // Skip when a drag is in flight — the release at the end of a
        // drag must not double-fire a click. Close lives on the
        // `Ctrl+Shift+W` keybind path; the WebView build has no
        // per-tab `×` either, so we keep the cell click-surface
        // dedicated to switching.
        if cell_resp.clicked() && drag_from.is_none() && event.is_none() && !is_active {
            event = Some(TabEvent::Switch(i));
        }

        // Active-tab indicator: 3 px bar at the bottom, side-margined to
        // match `width: calc(100% - 32px)`.
        if is_active {
            paint_active_indicator(ui, rect);
            // FR4: the active plain-tab cell.
            active_cell_rect = Some(rect);
        }
    }

    // Post-loop: handle drag-in-progress (indicator) and drop (event).
    // `cell_rects` holds only plain-tab cells; we use `cell_rosters` to map
    // a drop-target cell index back to a roster insertion point so a mux
    // group expanding in the middle of the strip doesn't break drag math.
    if cell_rects.is_empty() {
        // Strip is all mux cells (no plain tabs); clean up any latched drag.
        if drag_from.is_some() && ui.input(|i| i.pointer.any_released()) {
            ui.ctx().memory_mut(|m| m.data.remove::<usize>(drag_id));
        }
    } else if let Some(from) = drag_from {
        // `latest_pos` survives across release frames, unlike
        // `interact_pos` which returns `None` once the pointer leaves
        // the interaction state (e.g. on the release frame itself).
        let pointer_pos = ui.input(|i| i.pointer.latest_pos());
        let target_cell = pointer_pos.map(|p| drop_target_index(&cell_rects, p.x));

        // Draw a vertical primary-coloured indicator at the drop slot.
        if let Some(target_cell) = target_cell {
            if let Some(indicator_x) = drop_indicator_x(&cell_rects, target_cell) {
                let y0 = cell_rects[0].top();
                let y1 = cell_rects[0].bottom() - HAIRLINE_HEIGHT;
                ui.painter()
                    .vline(indicator_x, y0..=y1, Stroke::new(2.0, md3::primary()));
            }
        }

        // Release ends the drag. `drag_started_by` already guards the
        // click-vs-drag threshold (egui's default 4 px), so by the time
        // `drag_from` is set we know this was an actual drag.
        let released = ui.input(|i| i.pointer.any_released());
        if released {
            if let Some(target_cell) = target_cell {
                // Map the cell-space drop target back to a roster insertion
                // point: a drop before `cell_rects[c]` inserts before the
                // roster index `cell_rosters[c]`; a drop at `cell_rects.len()`
                // (past the rightmost plain-tab cell) inserts at the end of
                // the roster (past any trailing mux group too).
                let to = if target_cell < cell_rosters.len() {
                    cell_rosters[target_cell]
                } else {
                    items.len()
                };
                if to != from && to != from + 1 {
                    event = Some(TabEvent::Reorder { from, to });
                }
            }
            ui.ctx().memory_mut(|m| m.data.remove::<usize>(drag_id));
        }
    }

    // FR4: scroll the active visual cell into view exactly once when the
    // keyboard-switch flag is set. `scroll_to_rect` is a no-op when the rect is
    // already visible, so an already-on-screen active cell stays put (the
    // harmless same-window-digit case). Best-effort: if the active cell was not
    // laid out this frame (no rect captured), nothing happens.
    if scroll_active_into_view {
        if let Some(rect) = active_cell_rect {
            #[cfg(test)]
            tests::LAST_SCROLL_INTO_VIEW_RECT.with(|c| c.set(Some(rect)));
            ui.scroll_to_rect(rect, None);
        }
    }

    event
}

/// Compute the drop-target insertion index given the strip's cell
/// rects and the pointer's current `x`. The result lies in
/// `0..=cells.len()`. The pointer is considered to drop "before" a
/// cell if it sits in that cell's left half, and "after" if it sits
/// in the right half. Outside the strip, drops clamp to the closest
/// edge.
fn drop_target_index(cells: &[Rect], pointer_x: f32) -> usize {
    if cells.is_empty() {
        return 0;
    }
    if pointer_x < cells[0].left() {
        return 0;
    }
    if pointer_x > cells[cells.len() - 1].right() {
        return cells.len();
    }
    for (i, rect) in cells.iter().enumerate() {
        if pointer_x < rect.center().x {
            return i;
        }
    }
    cells.len()
}

/// X position of the drop indicator for the given insertion index.
/// `index == 0` → left edge of the first cell; `index == cells.len()`
/// → right edge of the last cell; otherwise the boundary between
/// `cells[index - 1]` and `cells[index]`.
fn drop_indicator_x(cells: &[Rect], index: usize) -> Option<f32> {
    if cells.is_empty() {
        return None;
    }
    if index == 0 {
        return Some(cells[0].left());
    }
    if index >= cells.len() {
        return Some(cells[cells.len() - 1].right());
    }
    Some(cells[index].left())
}

/// Draw the trailing 40 px "+" icon button. The "+" is composed of
/// two `line_segment` calls (vertical + horizontal stroke) so the
/// glyph is font-independent and aligns visually with the
/// `title_bar` icons. Hover swaps in the MD3 state-layer overlay
/// inside a full-radius pill so the layer reads as a circle.
fn draw_icon_button(ui: &mut Ui, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let painter = ui.painter();

    if resp.hovered() {
        painter.rect_filled(
            rect,
            Rounding::same(ICON_BUTTON_RADIUS),
            md3::state_layer(md3::on_surface_variant(), md3::STATE_LAYER_HOVER),
        );
    }

    let bbox = Rect::from_center_size(rect.center(), Vec2::splat(PLUS_ICON_SIZE));
    let stroke = Stroke::new(PLUS_ICON_STROKE_WIDTH, md3::on_surface_variant());
    let cx = bbox.center().x;
    let cy = bbox.center().y;
    painter.line_segment(
        [egui::pos2(bbox.left(), cy), egui::pos2(bbox.right(), cy)],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(cx, bbox.top()), egui::pos2(cx, bbox.bottom())],
        stroke,
    );

    resp
}

/// Circular hover-highlight button with a line-drawn gear glyph.
/// Painter-rendered (like the "+" button) so it follows the md3 tokens
/// without shipping an icon font.
fn draw_gear_button(ui: &mut Ui, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let painter = ui.painter();

    if resp.hovered() {
        painter.rect_filled(
            rect,
            Rounding::same(ICON_BUTTON_RADIUS),
            md3::state_layer(md3::on_surface_variant(), md3::STATE_LAYER_HOVER),
        );
    }

    let center = rect.center();
    let color = md3::on_surface_variant();
    // Gear glyph: hub ring + outer ring + 8 radial teeth.
    let hub_r = 2.5;
    let ring_r = 5.0;
    let tooth_r = 7.5;
    painter.circle_stroke(center, hub_r, Stroke::new(1.2, color));
    painter.circle_stroke(center, ring_r, Stroke::new(1.6, color));
    for i in 0..8 {
        let angle = (i as f32) * std::f32::consts::FRAC_PI_4;
        let dir = Vec2::new(angle.cos(), angle.sin());
        painter.line_segment(
            [center + dir * ring_r, center + dir * tooth_r],
            Stroke::new(2.0, color),
        );
    }

    resp
}

// ── mux tab group render-model ───────────────────────────────────────────

/// One window sub-tab in a mux tab group, in left-to-right order. The widget
/// draws each cell labelled `[N] name` and a click switches to that window.
/// The model is built from the tab's
/// [`crate::mux::window_group::MuxWindowGroup`] by [`mux_group_render_model`].
///
/// WebView parity: an attached mux tab always renders one sub-tab per window
/// (no compact `mux (N)` cell, no expand/collapse toggle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MuxSubTabCell {
    /// Window position (0-based) within the group; the click target.
    pub index: usize,
    /// Window display name.
    pub name: String,
    /// Whether this is the active window (highlighted).
    pub active: bool,
}

/// Render-model for the mux tab group: one [`MuxSubTabCell`] per window, in
/// order, with the active window marked. Mirrors the WebView
/// `renderMuxSubTabs` (always one numbered sub-tab per window).
pub fn mux_group_render_model(
    group: &crate::mux::window_group::MuxWindowGroup,
) -> Vec<MuxSubTabCell> {
    let active = group.active_index();
    group
        .windows()
        .iter()
        .enumerate()
        .map(|(i, w)| MuxSubTabCell {
            index: i,
            name: w.name.clone(),
            active: i == active,
        })
        .collect()
}

/// The `[N] name` label shown on a sub-tab (number badge + window name),
/// mirroring the WebView `mux-window-number` + `tab-title` spans.
fn mux_sub_tab_label(cell: &MuxSubTabCell) -> String {
    format!("[{}] {}", cell.index + 1, cell.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Modifiers, PointerButton, Pos2, RawInput};
    use std::cell::{Cell, RefCell};

    thread_local! {
        pub(super) static LAST_PLUS_RECT: Cell<Option<Rect>> = const { Cell::new(None) };
        pub(super) static LAST_TAB_CELLS: RefCell<Vec<Rect>> = const { RefCell::new(Vec::new()) };
        pub(super) static LAST_MUX_CELLS: RefCell<Vec<Rect>> = const { RefCell::new(Vec::new()) };
        /// Rects that received `paint_active_indicator` during the last
        /// `layout_tab_strip` pass (both plain-tab and mux sub-tab cells).
        /// FR5 asserts the indicator is painted for exactly the expected
        /// cell(s) without GPU readback.
        pub(super) static LAST_INDICATOR_RECTS: RefCell<Vec<Rect>> =
            const { RefCell::new(Vec::new()) };
        /// The rect passed to `ui.scroll_to_rect` during the last
        /// `layout_tab_strip` pass when the FR4 flag was set, or `None` if the
        /// flag was down / the active cell had no captured rect. TS-5 / TS-6
        /// assert the strip requests scroll-into-view for the correct cell.
        pub(super) static LAST_SCROLL_INTO_VIEW_RECT: Cell<Option<Rect>> =
            const { Cell::new(None) };
    }

    fn item(title: &str) -> TabBarItem {
        TabBarItem::new(title)
    }

    fn screen_rect() -> Rect {
        Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 100.0))
    }

    fn pos_of_first_hovered(ctx: &egui::Context) -> Option<Pos2> {
        ctx.pointer_latest_pos()
    }

    fn run_with_click(
        items: &[TabBarItem],
        active_idx: usize,
        click_pos: Pos2,
    ) -> Option<TabEvent> {
        let ctx = egui::Context::default();

        let mut input1 = RawInput::default();
        input1.screen_rect = Some(screen_rect());
        input1.events.push(Event::PointerMoved(click_pos));
        let mut captured: Option<TabEvent> = None;
        let _ = ctx.run(input1, |ctx| {
            captured = draw(ctx, items, active_idx, false, None);
        });

        let mut input2 = RawInput::default();
        input2.screen_rect = Some(screen_rect());
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
        let mut second: Option<TabEvent> = None;
        let _ = ctx.run(input2, |ctx| {
            second = draw(ctx, items, active_idx, false, None);
            let _ = pos_of_first_hovered(ctx);
        });

        second.or(captured)
    }

    // ── TS-tab-3: mux mode title prefix ─────────────────────

    #[test]
    fn render_label_passthrough_when_no_mux_session() {
        let it = item("zsh");
        assert_eq!(render_label(&it), "zsh");
    }

    #[test]
    fn render_label_prepends_mux_prefix_when_session_present() {
        let it = TabBarItem::new("nvim").with_mux_session("foo");
        assert_eq!(render_label(&it), "[mux:foo] nvim");
    }

    #[test]
    fn render_label_keeps_default_shell_title() {
        let it = item("shell");
        assert_eq!(render_label(&it), "shell");
    }

    // ── task0005 AC-1: collapsed mux tab label ──────────────────────────

    #[test]
    fn render_label_collapsed_mux_tab_shows_active_window_name() {
        let it = TabBarItem::new("ignored").with_mux_active_window_name("editor");
        assert_eq!(render_label(&it), "mux: editor");
    }

    #[test]
    fn render_label_collapsed_mux_tab_takes_precedence_over_session_prefix() {
        // A tab can carry both `mux_session_name` (set on attach) and
        // `mux_active_window_name` (set once the window list is populated,
        // same frame in practice) — the collapsed label wins.
        let it = TabBarItem::new("ignored")
            .with_mux_session("main")
            .with_mux_active_window_name("editor");
        assert_eq!(render_label(&it), "mux: editor");
    }

    #[test]
    fn render_label_follows_active_window_rename() {
        // AC-1: the label follows a rename of the active window — since
        // `render_label` is pure over the current `TabBarItem`, a fresh
        // view-model built with the new name changes the label with no
        // extra plumbing.
        let before = TabBarItem::new("ignored").with_mux_active_window_name("logs");
        let after = TabBarItem::new("ignored").with_mux_active_window_name("logs-renamed");
        assert_eq!(render_label(&before), "mux: logs");
        assert_eq!(render_label(&after), "mux: logs-renamed");
    }

    #[test]
    fn render_label_pre_group_mux_session_keeps_prefix_format() {
        // Transitional state: `mux_session_name` set on attach, before the
        // window list (and thus `mux_active_window_name`) is populated.
        let it = TabBarItem::new("nvim").with_mux_session("main");
        assert_eq!(render_label(&it), "[mux:main] nvim");
    }

    // ── task0006 AC-1/AC-4: agent-status badge color / form ─────────────

    #[test]
    fn agent_state_color_maps_every_variant_to_its_md3_role() {
        assert_eq!(
            agent_state_color(AgentState::Blocked),
            md3::on_error_container()
        );
        assert_eq!(agent_state_color(AgentState::Working), md3::primary());
        assert_eq!(
            agent_state_color(AgentState::Done),
            md3::on_secondary_container()
        );
        assert_eq!(
            agent_state_color(AgentState::Idle),
            md3::on_surface_variant()
        );
    }

    #[test]
    fn agent_badge_filled_blocked_and_done_follow_unseen() {
        assert!(agent_badge_filled(Aggregated {
            state: AgentState::Blocked,
            unseen: true
        }));
        assert!(!agent_badge_filled(Aggregated {
            state: AgentState::Blocked,
            unseen: false
        }));
        assert!(agent_badge_filled(Aggregated {
            state: AgentState::Done,
            unseen: true
        }));
        assert!(!agent_badge_filled(Aggregated {
            state: AgentState::Done,
            unseen: false
        }));
    }

    #[test]
    fn agent_badge_filled_working_and_idle_are_always_filled_regardless_of_unseen() {
        for unseen in [true, false] {
            assert!(agent_badge_filled(Aggregated {
                state: AgentState::Working,
                unseen
            }));
            assert!(agent_badge_filled(Aggregated {
                state: AgentState::Idle,
                unseen
            }));
        }
    }

    // ── task0006 AC-2: badge absence reserves no layout space ───────────

    fn collect_text_shapes_by_x(shapes: &[egui::epaint::ClippedShape]) -> Vec<(f32, String)> {
        fn walk(shape: &egui::epaint::Shape, out: &mut Vec<(f32, String)>) {
            use egui::epaint::Shape;
            match shape {
                Shape::Text(t) => {
                    let mut s = String::new();
                    for row in &t.galley.rows {
                        for g in &row.glyphs {
                            s.push(g.chr);
                        }
                    }
                    if !s.is_empty() {
                        out.push((t.pos.x, s));
                    }
                }
                Shape::Vec(v) => {
                    for s in v {
                        walk(s, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for cs in shapes {
            walk(&cs.shape, &mut out);
        }
        out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        out
    }

    fn title_text_x(items: &[TabBarItem]) -> f32 {
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        let output = ctx.run(input, |ctx| {
            let _ = draw(ctx, items, 0, false, None);
        });
        collect_text_shapes_by_x(&output.shapes)
            .into_iter()
            .find(|(_, s)| s.contains("shell"))
            .map(|(x, _)| x)
            .expect("title text shape present")
    }

    #[test]
    fn agent_badge_present_shifts_title_right_when_reserving_its_space() {
        let without = title_text_x(&[item("shell")]);
        let with = title_text_x(&[item("shell").with_agent_badge(Some(Aggregated {
            state: AgentState::Working,
            unseen: true,
        }))]);
        // The `[badge][dot][gap][title]` group is centred as a unit, so
        // reserving `agent_dot_space` extra width shifts the group's
        // (and thus the title's) center by half that amount — the other
        // half is absorbed by the group's left edge moving left. The
        // reserved width is the unified badge SLOT (task0001 Design 4),
        // not the bare circle diameter.
        let expected_shift = (AGENT_BADGE_SLOT_WIDTH + AGENT_BADGE_GAP) / 2.0;
        assert!(
            (with - without - expected_shift).abs() < 0.5,
            "badge presence should shift the title right by half its reserved space \
             ({expected_shift}px): \
             without={without}, with={with}"
        );
    }

    #[test]
    fn agent_badge_absent_matches_pre_feature_title_position_across_two_renders() {
        // AC-2: two independently-built items with no agent badge (one
        // freshly constructed, one via the builder passing `None`
        // explicitly) must paint the title at the identical x — proving
        // `agent_dot_space` contributes nothing when the badge is absent.
        let plain = title_text_x(&[item("shell")]);
        let explicit_none = title_text_x(&[item("shell").with_agent_badge(None)]);
        assert_eq!(plain, explicit_none);
    }

    // ── TS-tab-1: simulated interaction → TabEvent ──────────

    /// Drive one layout-only frame to populate the test thread_local
    /// rects (`+` button, tab cells), then return the requested
    /// rect. Eliminates hard-coded screen coordinates and keeps the
    /// tests robust against panel padding tweaks.
    fn capture_rect_with<F>(items: &[TabBarItem], active_idx: usize, pick: F) -> Rect
    where
        F: Fn() -> Option<Rect>,
    {
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        let _ = ctx.run(input, |ctx| {
            let _ = draw(ctx, items, active_idx, false, None);
        });
        pick().expect("layout hook should have captured the rect")
    }

    fn plus_rect(items: &[TabBarItem], active_idx: usize) -> Rect {
        capture_rect_with(items, active_idx, || LAST_PLUS_RECT.with(|c| c.get()))
    }

    fn tab_cell_rect(items: &[TabBarItem], active_idx: usize, i: usize) -> Rect {
        capture_rect_with(items, active_idx, || {
            LAST_TAB_CELLS.with(|c| c.borrow().get(i).copied())
        })
    }

    #[test]
    fn clicking_plus_emits_new() {
        let items = vec![item("a"), item("b")];
        let target = plus_rect(&items, 0).center();
        let ev = run_with_click(&items, 0, target);
        assert_eq!(ev, Some(TabEvent::New));
    }

    #[test]
    fn clicking_inactive_tab_emits_switch() {
        // Click the centre of tab 1's label sub-rect (i.e. the cell
        // centre, well clear of the close column on the right).
        let items = vec![item("alpha"), item("beta")];
        let cell = tab_cell_rect(&items, 0, 1);
        let click = Pos2::new(cell.left() + TAB_HORIZONTAL_PAD + 4.0, cell.center().y);
        let ev = run_with_click(&items, 0, click);
        assert_eq!(ev, Some(TabEvent::Switch(1)));
    }

    #[test]
    fn tab_cells_lay_out_side_by_side_without_overlap() {
        let items = vec![item("alpha"), item("beta")];
        let _ = plus_rect(&items, 0);
        let cells: Vec<_> = LAST_TAB_CELLS.with(|c| c.borrow().clone());
        assert_eq!(cells.len(), 2);
        assert!(
            (cells[0].right() - cells[1].left()).abs() < 0.5,
            "tab cells should sit edge-to-edge; got {:?} / {:?}",
            cells[0],
            cells[1]
        );
    }

    // ── drop_target_index / drop_indicator_x ────────────────

    fn rect(x0: f32, x1: f32) -> Rect {
        Rect::from_min_max(Pos2::new(x0, 0.0), Pos2::new(x1, 48.0))
    }

    #[test]
    fn drop_target_left_of_strip_clamps_to_zero() {
        let cells = vec![rect(0.0, 100.0), rect(100.0, 200.0)];
        assert_eq!(drop_target_index(&cells, -10.0), 0);
    }

    #[test]
    fn drop_target_right_of_strip_returns_len() {
        let cells = vec![rect(0.0, 100.0), rect(100.0, 200.0)];
        assert_eq!(drop_target_index(&cells, 300.0), 2);
    }

    #[test]
    fn drop_target_uses_cell_centre_as_boundary() {
        // Cells: [0, 100] (centre 50), [100, 200] (centre 150)
        let cells = vec![rect(0.0, 100.0), rect(100.0, 200.0)];
        assert_eq!(drop_target_index(&cells, 49.0), 0, "left half of cell 0");
        assert_eq!(drop_target_index(&cells, 51.0), 1, "right half of cell 0");
        assert_eq!(drop_target_index(&cells, 149.0), 1, "left half of cell 1");
        assert_eq!(drop_target_index(&cells, 151.0), 2, "right half of cell 1");
    }

    #[test]
    fn drop_indicator_x_pins_to_cell_boundaries() {
        let cells = vec![rect(0.0, 100.0), rect(100.0, 200.0)];
        assert_eq!(drop_indicator_x(&cells, 0), Some(0.0));
        assert_eq!(drop_indicator_x(&cells, 1), Some(100.0));
        assert_eq!(drop_indicator_x(&cells, 2), Some(200.0));
    }

    #[test]
    fn drop_indicator_x_empty_is_none() {
        let cells: Vec<Rect> = Vec::new();
        assert_eq!(drop_indicator_x(&cells, 0), None);
    }

    #[test]
    fn draw_with_single_tab_does_not_panic_and_emits_nothing_without_click() {
        let items = vec![item("solo")];
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        let mut captured: Option<TabEvent> = None;
        let _ = ctx.run(input, |ctx| {
            captured = draw(ctx, &items, 0, false, None);
        });
        assert_eq!(captured, None);
    }

    // ── TS-15: mux tab-group render model (always sub-tabs) ───────────────

    use crate::mux::window_group::{MuxWindow, MuxWindowGroup};

    fn group_with(n: usize, active: usize) -> MuxWindowGroup {
        let mut g = MuxWindowGroup::new();
        let windows: Vec<MuxWindow> = (0..n)
            .map(|i| MuxWindow {
                id: i as u32,
                name: format!("w{i}"),
            })
            .collect();
        let panes: Vec<u32> = (0..n).map(|i| 100 + i as u32).collect();
        g.seed(windows, panes, active);
        g
    }

    #[test]
    fn render_model_is_one_subtab_per_window_with_active_marker() {
        let cells = mux_group_render_model(&group_with(3, 1));
        assert_eq!(cells.len(), 3);
        let actives: Vec<bool> = cells.iter().map(|c| c.active).collect();
        assert_eq!(actives, vec![false, true, false]);
        // Index + name carried through for the click target / label.
        assert_eq!(cells[0].index, 0);
        assert_eq!(cells[2].name, "w2");
    }

    #[test]
    fn render_model_single_window_still_renders_one_subtab() {
        // WebView parity: a single mux window still renders as a sub-tab.
        let cells = mux_group_render_model(&group_with(1, 0));
        assert_eq!(cells.len(), 1);
        assert!(cells[0].active);
    }

    #[test]
    fn sub_tab_label_is_numbered() {
        let cells = mux_group_render_model(&group_with(2, 0));
        assert_eq!(mux_sub_tab_label(&cells[0]), "[1] w0");
        assert_eq!(mux_sub_tab_label(&cells[1]), "[2] w1");
    }

    // ── FR1: mux group is rendered by `draw` and clicks route correctly ───

    /// A tab whose roster entry carries mux cells (built from a group).
    fn mux_item(group: &MuxWindowGroup) -> TabBarItem {
        TabBarItem::new("ignored").with_mux_cells(mux_group_render_model(group))
    }

    /// Lay out one frame and return the captured mux cell rects in order.
    fn mux_cell_rects(items: &[TabBarItem]) -> Vec<Rect> {
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        let _ = ctx.run(input, |ctx| {
            let _ = draw(ctx, items, 0, false, None);
        });
        LAST_MUX_CELLS.with(|c| c.borrow().clone())
    }

    #[test]
    fn draw_renders_one_cell_per_window() {
        // 3-window group → 3 sub-tab cells (no compact/header cell).
        let items = vec![mux_item(&group_with(3, 0))];
        let rects = mux_cell_rects(&items);
        assert_eq!(rects.len(), 3);
    }

    #[test]
    fn clicking_subtab_emits_mux_switch_to_that_window() {
        // Cells: [sub0, sub1, sub2]; clicking sub2 → switch window 2.
        let items = vec![mux_item(&group_with(3, 0))];
        let sub2 = mux_cell_rects(&items)[2].center();
        let ev = run_with_click(&items, 0, sub2);
        assert_eq!(ev, Some(TabEvent::MuxSwitch { tab: 0, window: 2 }));
    }

    #[test]
    fn clicking_first_subtab_switches_to_window_zero() {
        let items = vec![mux_item(&group_with(3, 1))];
        let sub0 = mux_cell_rects(&items)[0].center();
        let ev = run_with_click(&items, 0, sub0);
        assert_eq!(ev, Some(TabEvent::MuxSwitch { tab: 0, window: 0 }));
    }

    #[test]
    fn mux_group_cell_count_drives_strip_width_math() {
        // A 3-window group counts as 3 visual cells, not 1 roster entry, so
        // the equal-width layout reserves room for every sub-tab.
        let items = vec![mux_item(&group_with(3, 0))];
        assert_eq!(visual_cell_count(&items), 3);
        // Plain tabs still count one-each (Phase 4-B path unchanged).
        let plain = vec![item("a"), item("b")];
        assert_eq!(visual_cell_count(&plain), 2);
    }

    // ── FR5: unique active indicator across mixed tabs ───────────────────

    /// Lay out one frame and return the rects that received the active
    /// indicator (both plain-tab and mux sub-tab cells).
    fn indicator_rects(items: &[TabBarItem], active_idx: usize) -> Vec<Rect> {
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        let _ = ctx.run(input, |ctx| {
            let _ = draw(ctx, items, active_idx, false, None);
        });
        LAST_INDICATOR_RECTS.with(|c| c.borrow().clone())
    }

    #[test]
    fn ts2_non_active_mux_parent_paints_no_subtab_indicator() {
        // Roster: [plain "a" (active), mux group]. The mux group has an
        // active window (cell 1), but its parent tab (index 1) is NOT the
        // active tab, so no sub-tab indicator is painted. The only indicator
        // is the active plain tab's.
        let items = vec![item("a"), mux_item(&group_with(3, 1))];
        let mux_cells = mux_cell_rects(&items);
        let bars = indicator_rects(&items, 0);
        // No painted indicator coincides with any mux sub-tab cell.
        for cell in &mux_cells {
            assert!(
                !bars.iter().any(|b| b.left() == cell.left()),
                "no sub-tab indicator should be painted for a non-active mux parent; \
                 got bars {bars:?} overlapping mux cell {cell:?}"
            );
        }
    }

    #[test]
    fn ts3_active_mux_parent_paints_active_window_subtab_indicator() {
        // Roster: [plain "a", mux group (active)]. Parent tab index 1 is
        // active and window 1 is the active window, so exactly the window-1
        // sub-tab gets the indicator.
        let items = vec![item("a"), mux_item(&group_with(3, 1))];
        let mux_cells = mux_cell_rects(&items);
        let bars = indicator_rects(&items, 1);
        // Exactly one bar, aligned to the active window's sub-tab (cell 1).
        assert_eq!(bars.len(), 1, "exactly one indicator across the strip");
        assert!(
            (bars[0].left() - mux_cells[1].left()).abs() < 0.5,
            "the single indicator should sit on the active window's sub-tab; \
             bar {:?} vs cell {:?}",
            bars[0],
            mux_cells[1]
        );
    }

    // ── FR4: active-cell scroll-into-view selection ──────────────────────

    /// Lay out one frame with the FR4 flag set and return the rect the strip
    /// requested scroll-into-view for (if any).
    fn scroll_into_view_rect(items: &[TabBarItem], active_idx: usize) -> Option<Rect> {
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        LAST_SCROLL_INTO_VIEW_RECT.with(|c| c.set(None));
        let _ = ctx.run(input, |ctx| {
            let _ = draw(ctx, items, active_idx, true, None);
        });
        LAST_SCROLL_INTO_VIEW_RECT.with(|c| c.get())
    }

    #[test]
    fn ts5_flag_set_requests_scroll_into_view_for_active_cell() {
        // TS-5: with the flag set and an active plain tab, the strip selects
        // that cell's rect and requests scroll-into-view exactly once.
        // Many tabs so the strip overflows (the off-screen case is the point);
        // the active cell is captured regardless of visibility.
        let items: Vec<TabBarItem> = (0..12).map(|i| item(&format!("t{i}"))).collect();
        let want = tab_cell_rect(&items, 7, 7);
        let got = scroll_into_view_rect(&items, 7).expect("flag set → a rect is requested");
        assert!(
            (got.left() - want.left()).abs() < 0.5 && (got.right() - want.right()).abs() < 0.5,
            "scroll-into-view should target the active cell; got {got:?} want {want:?}"
        );
    }

    #[test]
    fn ts5_flag_unset_requests_no_scroll_into_view() {
        // Companion to TS-5: with the flag down, no scroll-into-view request is
        // made (the mouse-scroll / unrelated-repaint case).
        let items: Vec<TabBarItem> = (0..12).map(|i| item(&format!("t{i}"))).collect();
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        LAST_SCROLL_INTO_VIEW_RECT.with(|c| c.set(None));
        let _ = ctx.run(input, |ctx| {
            let _ = draw(ctx, &items, 7, false, None);
        });
        assert_eq!(
            LAST_SCROLL_INTO_VIEW_RECT.with(|c| c.get()),
            None,
            "flag down → no scroll-into-view request"
        );
    }

    #[test]
    fn ts6_active_cell_selection_picks_active_mux_subtab_in_active_tab() {
        // TS-6: the active visual cell inside an active mux tab is the active
        // window's sub-tab, not the tab as a whole. Roster: [plain "a", mux
        // group (window 1 active)]. With the mux tab (index 1) active, the
        // requested rect is the window-1 sub-tab cell.
        let items = vec![item("a"), mux_item(&group_with(3, 1))];
        let mux_cells = mux_cell_rects(&items);
        let got = scroll_into_view_rect(&items, 1).expect("flag set → a rect is requested");
        assert!(
            (got.left() - mux_cells[1].left()).abs() < 0.5,
            "scroll-into-view should target the active window's sub-tab; \
             got {:?} want {:?}",
            got,
            mux_cells[1]
        );
    }

    #[test]
    fn ts6_active_cell_selection_picks_plain_cell_at_active_idx() {
        // TS-6 (plain side): with a plain tab active, the active cell is the
        // plain-tab cell at `active_idx`.
        let items = vec![item("a"), item("b"), item("c")];
        let want = tab_cell_rect(&items, 2, 2);
        let got = scroll_into_view_rect(&items, 2).expect("flag set → a rect is requested");
        assert!(
            (got.left() - want.left()).abs() < 0.5,
            "scroll-into-view should target the active plain cell; got {got:?} want {want:?}"
        );
    }

    #[test]
    fn ts4_draw_with_non_active_mux_parent_leaves_active_window_unchanged() {
        // The gate reads `mux_cell.active` (a copied bool) and `tab` /
        // `active_idx` (plain indices); it never touches `MuxWindowGroup`.
        // Drawing with a non-active parent must not change the render model's
        // active window.
        let group = group_with(3, 1);
        let before = mux_group_render_model(&group);
        let items = vec![item("a"), mux_item(&group)];
        let _ = indicator_rects(&items, 0); // parent (index 1) not active
        let after = mux_group_render_model(&group);
        assert_eq!(
            before, after,
            "drawing with a non-active mux parent must not mutate the active window"
        );
    }

    // ── agent-badge-emoji task0001 AC-1: 4 states × unseen/seen table ───

    #[test]
    fn badge_presentation_resolves_all_eight_state_unseen_combinations() {
        // Table mirrors SPEC 4.2 / IMPLEMENTATION.md's presentation table:
        // working/idle unseen-independent, blocked ❓/❔, done ✅/💤 (the
        // 💤 alias of IDLE_BADGE_EMOJI). `fallback_filled` mirrors
        // `agent_badge_filled`'s per-state semantics (D3).
        let cases: [(AgentState, bool, &str, bool); 8] = [
            (AgentState::Working, true, WORKING_BADGE_EMOJI, true),
            (AgentState::Working, false, WORKING_BADGE_EMOJI, true),
            (AgentState::Idle, true, IDLE_BADGE_EMOJI, true),
            (AgentState::Idle, false, IDLE_BADGE_EMOJI, true),
            (AgentState::Blocked, true, BLOCKED_BADGE_EMOJI_UNSEEN, true),
            (AgentState::Blocked, false, BLOCKED_BADGE_EMOJI_SEEN, false),
            (AgentState::Done, true, DONE_BADGE_EMOJI_UNSEEN, true),
            (AgentState::Done, false, IDLE_BADGE_EMOJI, false),
        ];
        for (state, unseen, expected_cluster, expected_fallback_filled) in cases {
            assert_eq!(
                badge_presentation(Aggregated { state, unseen }),
                BadgePresentation::Emoji {
                    cluster: expected_cluster,
                    fallback_filled: expected_fallback_filled,
                },
                "{state:?} unseen={unseen}"
            );
        }
    }

    // ── agent-badge-emoji task0001 AC-2: done+seen aliases idle ─────────

    #[test]
    fn badge_presentation_done_seen_is_the_same_cluster_constant_as_idle() {
        let BadgePresentation::Emoji { cluster, .. } = badge_presentation(Aggregated {
            state: AgentState::Done,
            unseen: false,
        });
        assert_eq!(cluster, IDLE_BADGE_EMOJI);
        assert!(
            std::ptr::eq(cluster, IDLE_BADGE_EMOJI),
            "done+seen must reuse the IDLE_BADGE_EMOJI constant itself, not a \
             duplicate literal with the same content"
        );
    }

    // ── agent-badge-emoji task0001 AC-3: new cluster constant format ────

    #[test]
    fn new_badge_emoji_constants_are_single_codepoint_no_vs16() {
        let cases: [(&str, char); 3] = [
            (BLOCKED_BADGE_EMOJI_UNSEEN, '\u{2753}'),
            (BLOCKED_BADGE_EMOJI_SEEN, '\u{2754}'),
            (DONE_BADGE_EMOJI_UNSEEN, '\u{2705}'),
        ];
        for (cluster, expected_codepoint) in cases {
            let chars: Vec<char> = cluster.chars().collect();
            assert_eq!(
                chars,
                vec![expected_codepoint],
                "cluster must be exactly the expected single codepoint, with \
                 no VS-16 (U+FE0F) suffix"
            );
        }
    }

    // ── agent-badge-emoji task0001 AC-4/AC-5: fallback circle resolution ─

    #[test]
    fn resolve_badge_render_mode_emoji_with_texture_blits_the_texture() {
        for state in [
            AgentState::Working,
            AgentState::Idle,
            AgentState::Blocked,
            AgentState::Done,
        ] {
            for unseen in [true, false] {
                let presentation = badge_presentation(Aggregated { state, unseen });
                assert_eq!(
                    resolve_badge_render_mode(presentation, true),
                    BadgeRenderMode::EmojiTexture,
                    "{state:?} unseen={unseen}"
                );
            }
        }
    }

    #[test]
    fn resolve_badge_render_mode_working_idle_fallback_is_always_filled() {
        // AC-5: texture unavailable, working/idle fallback stays filled
        // regardless of unseen (unchanged from before this task).
        for state in [AgentState::Working, AgentState::Idle] {
            for unseen in [true, false] {
                let presentation = badge_presentation(Aggregated { state, unseen });
                assert_eq!(
                    resolve_badge_render_mode(presentation, false),
                    BadgeRenderMode::Circle { filled: true },
                    "{state:?} unseen={unseen}"
                );
            }
        }
    }

    #[test]
    fn resolve_badge_render_mode_blocked_done_fallback_follows_unseen() {
        // AC-4: texture unavailable, blocked/done fall back to a circle
        // that preserves the existing unseen=filled / seen=ring shape —
        // never a blank slot, never the toolkit's default text path.
        for state in [AgentState::Blocked, AgentState::Done] {
            for unseen in [true, false] {
                let presentation = badge_presentation(Aggregated { state, unseen });
                assert_eq!(
                    resolve_badge_render_mode(presentation, false),
                    BadgeRenderMode::Circle { filled: unseen },
                    "{state:?} unseen={unseen}"
                );
            }
        }
    }

    // ── task0001 AC-3: emoji texture blit via a stub rasterizer ─────────
    //
    // Standing up the REAL swash + bundled-font stack in a unit test is
    // impractical (per the task's Test Notes) — this stub proves the
    // paint path picks the texture branch and aspect-fits it into the
    // slot; actual rasterization quality is a manual check (TS3).

    struct StubEmojiRasterizer;

    impl crate::render::font::traits::GlyphRasterizer for StubEmojiRasterizer {
        fn shape(
            &self,
            _cluster: &str,
            font: crate::render::font::traits::FontId,
            size_px: f32,
        ) -> Vec<crate::render::font::traits::ShapedGlyph> {
            vec![crate::render::font::traits::ShapedGlyph {
                font,
                glyph_id: 1,
                size_px,
            }]
        }

        fn raster(
            &self,
            _font: crate::render::font::traits::FontId,
            _glyph_id: u32,
            size_px: f32,
        ) -> Option<crate::render::font::traits::GlyphBitmap> {
            let n = size_px.round().max(1.0) as u32;
            Some(crate::render::font::traits::GlyphBitmap {
                format: crate::render::font::traits::AtlasFormat::Rgba,
                width: n,
                height: n,
                bearing: (0, 0),
                advance: size_px,
                pixels: vec![255u8; (n as usize) * (n as usize) * 4],
            })
        }

        fn has_codepoint(&self, _font: crate::render::font::traits::FontId, _cp: u32) -> bool {
            true
        }
    }

    /// A fallback chain that resolves every cluster through the "emoji"
    /// font id — pairs with [`StubEmojiRasterizer`], which covers every
    /// codepoint.
    fn stub_emoji_fallback() -> crate::render::font::fallback::FallbackChain {
        use crate::render::font::traits::FontId;
        let mut chain = crate::render::font::fallback::FallbackChain::new(FontId(1), [FontId(2)]);
        chain.set_emoji(FontId(2));
        chain
    }

    /// Collect the rects of every textured (image-blit) `Shape::Rect` —
    /// egui's `Image::paint_at` emits a `RectShape` with a non-default
    /// `fill_texture_id`, distinguishing an emoji blit from any other
    /// filled rect the strip paints.
    fn collect_textured_rects(shapes: &[egui::epaint::ClippedShape]) -> Vec<Rect> {
        fn walk(shape: &egui::epaint::Shape, out: &mut Vec<Rect>) {
            use egui::epaint::Shape;
            match shape {
                Shape::Rect(r) if r.fill_texture_id != egui::TextureId::default() => {
                    out.push(r.rect);
                }
                Shape::Vec(v) => {
                    for s in v {
                        walk(s, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for cs in shapes {
            walk(&cs.shape, &mut out);
        }
        out
    }

    /// Collect the radii of every `Shape::Circle` painted this frame.
    fn collect_circle_radii(shapes: &[egui::epaint::ClippedShape]) -> Vec<f32> {
        fn walk(shape: &egui::epaint::Shape, out: &mut Vec<f32>) {
            use egui::epaint::Shape;
            match shape {
                Shape::Circle(c) => out.push(c.radius),
                Shape::Vec(v) => {
                    for s in v {
                        walk(s, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for cs in shapes {
            walk(&cs.shape, &mut out);
        }
        out
    }

    #[test]
    fn ac3_working_and_idle_badges_with_emoji_resources_paint_texture_not_circle() {
        for state in [AgentState::Working, AgentState::Idle] {
            let items = vec![item("shell").with_agent_badge(Some(Aggregated {
                state,
                unseen: true,
            }))];
            let rasterizer = StubEmojiRasterizer;
            let fallback = stub_emoji_fallback();
            let cache = parking_lot::Mutex::new(crate::ui::emoji_cache::EmojiTextureCache::new());
            let emoji = EmojiResources {
                rasterizer: &rasterizer,
                fallback: &fallback,
                cache: &cache,
            };
            let ctx = egui::Context::default();
            let mut input = RawInput::default();
            input.screen_rect = Some(screen_rect());
            let output = ctx.run(input, |ctx| {
                let _ = draw(ctx, &items, 0, false, Some(&emoji));
            });

            let textured = collect_textured_rects(&output.shapes);
            assert!(
                !textured.is_empty(),
                "{state:?}: expected a textured rect (emoji blit) for the badge"
            );
            for r in &textured {
                assert!(
                    r.width() <= AGENT_BADGE_SLOT_WIDTH + 0.01
                        && r.height() <= AGENT_BADGE_SLOT_WIDTH + 0.01,
                    "{state:?}: emoji blit must aspect-fit inside the \
                     {AGENT_BADGE_SLOT_WIDTH}px slot; got {r:?}"
                );
            }

            // Replace, not combine (Design 4): no badge-sized (4px radius)
            // circle should also be painted for this state.
            let radii = collect_circle_radii(&output.shapes);
            let badge_radius = AGENT_BADGE_DIAMETER / 2.0;
            assert!(
                !radii.iter().any(|r| (*r - badge_radius).abs() < 0.01),
                "{state:?}: the emoji must replace the dot entirely, not paint \
                 alongside it; circle radii found: {radii:?}"
            );
        }
    }

    // ── task0001 AC-5: unified 12px badge slot ───────────────────────────

    #[test]
    fn ac5_working_to_done_transition_causes_no_title_shift() {
        // The reserved slot width is unified across ALL states (Design
        // 4), so a badge state transition must never move the title even
        // though `working` and `done` render different emoji clusters
        // (agent-badge-emoji task0001) and (with no emoji resources
        // supplied here) different fallback-circle shapes.
        let working = title_text_x(&[item("shell").with_agent_badge(Some(Aggregated {
            state: AgentState::Working,
            unseen: true,
        }))]);
        let done = title_text_x(&[item("shell").with_agent_badge(Some(Aggregated {
            state: AgentState::Done,
            unseen: true,
        }))]);
        assert_eq!(
            working, done,
            "a working -> done badge transition must cause no title x-shift"
        );
    }
}
