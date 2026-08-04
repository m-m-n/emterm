# Design: agent-badge-emoji-distinction

Design-token SSOT: `doc/UI-DESIGN-GUIDELINES.yaml` (project-native design
system, `project.design_system.kind: project_native`). No feature-local
tokens are minted; every value below is grounded in that file or in the
existing badge constants it already governs.

Mockup: [design/mockups/screen-agent-badges.html](design/mockups/screen-agent-badges.html)

## Decisions

1. **Glyph assignment**: `working` = ⚡ (U+26A1 HIGH VOLTAGE), `idle` = 💤
   (U+1F4A4 ZZZ). Both are single-codepoint, default-emoji-presentation
   clusters (no VS-16 needed), pass `cluster_is_emoji` in
   `src-tauri/src/ui/emoji_cache.rs`, and are covered by the bundled Noto
   Color Emoji CBDT strikes.
2. **Replace, not combine**: for `working` / `idle` the emoji **replaces**
   the current filled dot entirely. No circle underlay, no side-by-side
   pairing. States other than `working` / `idle` (`blocked` / `done`,
   filled-dot-when-unseen / 1.5px-ring-when-seen) follow existing — their
   rendering code paths are untouched (SPEC edge case; REQUIREMENTS 14.1).
3. **Size**: the emoji renders aspect-fit inside a **12×12 logical-px box**
   (12px = `tokens.typography.label-small` font-size, the design system's
   badge scale). Rasterization request to `EmojiTextureCache` is
   `12.0 * pixels_per_point` physical px; the cache's existing
   supersample + Lanczos3 downscale path handles tiny-size quality.
4. **Placement / layout**: the badge slot stays where it is today in both
   surfaces (tab bar: leftmost element of the centered
   `[badge][activity-dot][title]` group in `tab_bar.rs`; sidebar: between
   the number column and the name in `mux_sidebar.rs`). The slot width
   when a badge is present becomes a **unified 12px** for ALL states
   (emoji fills it; the unchanged 8px blocked/done dot is centered within
   it) so a pane transitioning working → done causes **no title shift**.
   The 6px gap after the badge and the "no slot reserved when no badge has
   ever been reported" behavior are unchanged.
5. **Color**: the emoji is blitted untinted (its own color table).
   `agent_state_color` remains the SSOT for the blocked/done circles and
   for any non-emoji fallback; its `Working → primary` / `Idle →
   on_surface_variant` mappings stay in place for the fallback path.
6. **Fallback**: if `EmojiTextureCache::get_or_rasterize` returns `None`
   (emoji font lacks the glyph), the badge falls back to the **current**
   filled-circle rendering for that state — never a blank slot, never a
   tofu via egui's text path (FR3: ab_glyph is not used for these glyphs).
7. **Cross-surface consistency (NFR1)**: glyphs, box size, slot width, gap
   and fallback rule are identical in `tab_bar.rs` and `mux_sidebar.rs`.
   The state → presentation choice (Emoji(cluster) vs Circle) should live
   in one shared pure function next to `agent_state_color` /
   `agent_badge_filled` so both painters consume the same decision and TS1
   can unit-test it without egui.
8. **No animation**: the badge is a static glyph; no blink/pulse for
   `working`. Motion tokens are not engaged.

## Rationale

- ⚡ vs 💤: maximally different silhouettes (bolt vs diagonal Z's) that
  survive 12px rendering — the distinction holds in shape alone, satisfying
  FR1's "non-color-dependent means"; the color difference (yellow vs blue)
  is a bonus, not the mechanism. Semantics are direct: energy/activity vs
  sleep.
- Replace over combine: an 8px dot cannot host a legible overlay, and a
  side-by-side pair doubles badge width in two already-compact surfaces
  (tab label area, 40px sidebar rows) while adding a second visual to
  parse — against "at a glance".
- 12px box: the largest size that stays subordinate to the 14px labels
  beside it (tab_bar `TAB_FONT_SIZE`, sidebar `NAME_FONT_SIZE`) while
  keeping tiny-emoji legibility; anchored to the `label-small` (12px)
  badge scale in `doc/UI-DESIGN-GUIDELINES.yaml` rather than a novel value.
- Unified 12px slot: a per-state slot width (12px emoji / 8px dot) would
  shift the title on every state transition; the constant slot trades a
  one-time 2px shift vs today for stability across transitions.
- Untinted blit: tinting a color emoji fights its palette and re-couples
  the distinction to theme colors — exactly what FR1 moves away from.
- Fallback to the current circle keeps the badge meaningful on systems
  where the emoji font is unavailable, and reuses already-reviewed code.
- No animation: `working` can persist for minutes; a pulsing tab bar would
  compete with the activity dot's existing 250ms show/hide animation and
  the render loop's wait-driven design.
- Mockup provided despite the native (egui) target: comparing the rejected
  same-family dots against the emoji pair at actual size is the core of
  this feature's "at a glance" claim, and a browser renders that comparison
  faithfully enough (Noto-family color emoji) to be worth agreeing on
  before implementation.

## Open items

- Final glyph pair sign-off is inherently subjective ("distinguishable at
  a glance", REQUIREMENTS 10.1). Resolution: the user's on-device visual
  check (SPEC TS3) after implementation; if ⚡/💤 reads poorly at 12px on
  real hardware, revisit via `/em-workflow:design` (candidate alternates
  considered and held in reserve: ⏳/🔄 for working, ⏸️ for idle).
