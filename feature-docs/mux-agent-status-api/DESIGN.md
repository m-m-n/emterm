# Design: mux-agent-status-api

## Decisions

- **Design system**: follows the project-native MD3 system
  (doc/UI-DESIGN-GUIDELINES.yaml + src-tauri/src/ui/md3.rs role accessors).
  No design-system/tokens.yaml is created. All colors below are MD3 roles,
  never hex literals.
- **State color mapping** (role-based, preset-agnostic):
  - blocked → `on-error-container`
  - working → `primary`
  - done → `on-secondary-container`
  - idle → `on-surface-variant`
- **Tab/window badge** (FR7): an 8px round dot placed before the tab title
  with a 6px gap (mockup: design/mockups/screen-tab-badges.html).
  - Unseen blocked/done: filled dot. Seen blocked/done: 1.5px ring, no
    fill, same role color. working/idle have no seen distinction (always
    filled / always muted).
  - No badge at all for panes/tabs without a reported state — layout is
    unchanged from today.
  - No animation (matches the status-bar precedent: no open/close or
    attention animation).
  - The window list reuses the same dot, aggregated over the window's
    panes with the FR7 priority.
- **Status-bar summary** (FR8): right-aligned segment in the app row
  (row 1 of the 3-row status bar), per-state groups of dot + count in
  label-extra-small (11px/16, weight 500), 8px between groups, 4px inside
  a group. Order: blocked, working, done, idle. Zero-count states are
  omitted; the whole segment is hidden when no pane reports a state
  (mockup: design/mockups/screen-status-bar-summary.html).
- **Pane ID copy affordance** (FR13): an entry in the pane/tab right-click
  context menu — 「pane ID をコピー」/ "Copy pane ID" via inline t(ja,en).
- **Notification appearance** (FR9): OS-native notification, no custom
  visuals. Title: `eMterm`. Body: `{sanitized name or "agent"}: {blocked|
  done} — {tab title}`.
- **Settings UI**: the agent-notification toggle reuses the existing
  settings-panel toggle row pattern (no new component).

## Rationale

- Dot-only badges keep tab width stable and read at a glance; MD3 has no
  success role, so `on-secondary-container` stands in for done — distinct
  from primary (working) in every preset (FR7, NFR4).
- `on-error-container` is the most salient role available on dark surfaces
  and matches blocked = needs-attention semantics (FR7).
- Filled-vs-ring for unseen/seen encodes the attention flag without color
  changes or motion, honoring the semantic-state/seen separation (FR7) and
  the project's no-animation stance.
- Counts-by-dot in the app row keeps the summary inside the existing
  status-bar visual language (single background, no floating chips) (FR8).
- Right-click context menu is the lowest-friction discoverable spot for an
  occasional action like copying a pane ID (FR13).

## Open items

- If the tab bar has no existing right-click context menu infrastructure,
  the fallback for pane-ID copy is a click-to-copy ID row in the mux
  window chooser — the planner verifies which infrastructure exists and
  picks the concrete host (resolved during create-plan).
