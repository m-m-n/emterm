//! Agent-status badge: sizing constants, per-state colors, emoji /
//! dot presentation resolution, and the badge painter shared by the
//! tab strip.

use super::*;

/// Agent-status badge dot diameter (task0006, `IMPLEMENTATION.md`
/// Conventions: "Badge: 8px dot, 6px gap before title"). Still the size
/// of every state's fallback circle form (agent-badge-emoji task0001
/// D3); the badge *slot* itself is [`AGENT_BADGE_SLOT_WIDTH`]
/// (agent-badge-emoji-distinction task0001 Design 4 — the slot widened
/// to fit the emoji forms, the dot stays 8px and centers within it).
pub(in crate::ui::tab_bar) const AGENT_BADGE_DIAMETER: f32 = 8.0;
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
pub(in crate::ui::tab_bar) const AGENT_BADGE_GAP: f32 = 6.0;
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

/// Color role for a semantic agent state (task0006 AC-4, `IMPLEMENTATION.md`
/// Conventions): blocked -> `on_error_container`, working -> `primary`,
/// done -> `on_secondary_container`, idle -> `on_surface_variant`. Shared by
/// [`ui::mux_sidebar`](crate::ui::mux_sidebar) and
/// [`ui::status_bar`](crate::ui::status_bar) so the mapping lives in one place.
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
        BadgePresentation::Emoji {
            fallback_filled, ..
        } => BadgeRenderMode::Circle {
            filled: fallback_filled,
        },
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
