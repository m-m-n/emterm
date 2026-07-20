# Design: mux-vertical-tabs

Design system: project-native MD3 (`doc/UI-DESIGN-GUIDELINES.yaml` mirrored by
`src-tauri/src/ui/md3.rs`). No `design-system/tokens.yaml` is created — the
native system stays the SSOT. Mockup:
`design/mockups/screen-mux-vertical-tabs.html`.

## Decisions

- **Persistent placement: RIGHT fixed panel** (changed 2026-07-20 by user
  request; both placement modes sit on the same side). Background
  `surface-container-low`, LEFT border 1px `outline-variant` on the
  terminal-facing edge — the settings-nav surface recipe with the separator
  mirrored to the right-side placement.
- **Overlay placement: RIGHT edge panel over the terminal** (SPEC FR5).
  Background `surface-container-high`, left border 1px `outline-variant`,
  `elevation-3` shadow (0 8px 32px rgba(0,0,0,0.30)). No scrim — the terminal
  stays fully visible except under the panel.
- **Sidebar width** = clamp(180px, 22% of app window width, 320px). One
  formula for both placements ("20–25% 程度" requirement → 22% with sane
  floor/ceiling).
- **Window entry**: 40px-high full-radius pill row, horizontal padding 12px,
  gap 8px, `body-medium` (14px) window name. Leading window number in
  `label-small` (12px, weight 500), right-aligned in a 16px column,
  `on-surface-variant`.
- **Active mark = MD3 active-pill**: `secondary-container` background +
  `on-secondary-container` text (identical to `.settings-nav-item.active`).
  No extra dot/bar indicator.
- **Hover (inactive entries)**: 8% `on-surface` state layer, matching nav
  items and top tabs.
- **Long names**: single line, ellipsis truncation. No wrapping, no tooltip.
- **Many windows**: vertical scroll (egui `ScrollArea`); rows keep 40px
  height, never shrink.
- **Empty list (transient attach/detach state)**: bare panel surface, no
  placeholder text.
- **No open/close animation** for the overlay — it appears/disappears in one
  frame.
- **Top tab title** `mux: <active window name>` uses the existing tab
  component unchanged (typography, active indicator, truncation all
  inherited). No new design for the top bar.
- **Settings toggle** 「オーバーレイで表示」 follows the existing
  `settings-row-toggle` + MD3 switch pattern unchanged.
- **List padding**: 12px vertical / 8px horizontal panel padding, 4px gap
  between entries (spacing tokens sm / xs / xxs).

## Rationale

- Right side for persistent: user decision (REQUIREMENTS §6.1 追記
  2026-07-20) — both modes on the same side keeps the sidebar's screen
  position identical across the placement-setting switch, and the terminal's
  grid origin never moves (only the width changes). The earlier left-side
  choice (settings-nav symmetry) was superseded by this preference.
- Same width formula in both modes keeps the A↔C settings switch visually
  stable (only placement changes), matching the shared-component decision
  (REQUIREMENTS §5.3).
- `secondary-container` pill as the sole active mark: it is the established
  "active item in a vertical list" signal in this product; adding a dot would
  duplicate meaning (FR2's active mark).
- `surface-container-high` + elevation-3 for the overlay: floating surfaces
  above content use the high container tone (dialog anatomy in
  UI-DESIGN-GUIDELINES `dialogs:`); the shadow separates it from arbitrary
  terminal content underneath.
- No animation: consistent with the status bar's no-open/close-animation
  precedent, and avoids egui repaint churn on a latency-sensitive surface
  (NFR1's spirit).
- 40px rows instead of nav's 48px: denser list is appropriate for a
  terminal-side utility panel; still comfortably clickable.

## Open items

- None. Judgment calls above (width clamp bounds, 40px row
  height, no animation) are revisitable via `/em-workflow:design` after the
  user sees the running result.
