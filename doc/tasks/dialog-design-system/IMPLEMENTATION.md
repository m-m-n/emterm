# Implementation Plan: Dialog Design System

## Overview

Promote `doc/UI-DESIGN-GUIDELINES.yaml` to the normative single source of
truth (SSOT) for modal dialogs, build a shared dialog helper on both the
native egui side and the child WebView side, and route all eight
existing dialogs through those helpers. Add a Rust unit test that
detects drift between the yaml, the Rust constants, and the CSS
variables.

## Objectives

- Establish dialog tokens (kinds, anatomy, layout, scrim, elevation,
  actions, keyboard, focus, labels) as normative entries in
  `UI-DESIGN-GUIDELINES.yaml`
- Resolve missing tokens (`error-container` / `on-error-container` /
  `surface-variant` in `md3.rs::Palette`; typography / elevation as CSS
  custom properties)
- Provide a native helper (`src-tauri/src/ui/dialog/`) that enforces
  Window setup, MD3 styling, role-based button colors, keyboard rules,
  and initial focus
- Provide a WebView helper (`src-tauri/web-shared/dialog/`) that
  produces a `dialog-*`-classed overlay/surface/body/actions structure
  with Esc / Enter / scrim semantics and a11y attributes
- Rename all `.profile-editor-*` CSS classes to `.dialog-*` and remove
  the old names in one pass (per Q2 default)
- Add a drift-detection unit test (`cargo test --lib`) covering scrim
  alpha, corner radius, padding, and color-role coverage in CSS
- Keep `profile_selector.rs` bespoke render but route its tokens
  through the shared helper module (per Q3 default)

## Prerequisites

### Development Environment

- Rust toolchain pinned by the workspace `rust-toolchain.toml`
- `bun` for child WebView bundles
- `serde_yml` already available to `src-tauri` as a regular
  dependency (`src-tauri/Cargo.toml:74` — consumed by the GUI-only
  `viewer::data_model`). The drift test reuses this existing crate;
  no Cargo.toml change is required.

### Dependencies

- Existing assets that MUST be present: `doc/UI-DESIGN-GUIDELINES.yaml`,
  `src-tauri/src/ui/md3.rs`, `src-tauri/web-shared/styles.css`,
  `src-tauri/web-shared/settings/ui-theme-presets.ts`,
  `src-tauri/web-shared/styles/settings-panel.css`,
  `src-tauri/src/ui/mux_dialogs.rs`, `src-tauri/src/render/mod.rs`,
  `src-tauri/src/ui/profile_selector.rs`,
  `src-tauri/web-shared/profile/profile-editor.ts`,
  `src-tauri/web-shared/ssh/ssh-editor.ts`
- `crate::i18n::Locale` enum (already present)
- `egui` (already a workspace dependency)

## Architecture Overview

### Technology Stack

- **Language**: Rust 2024 (native helper, drift test) + TypeScript
  (WebView helper, child WebView dialog refactor)
- **Framework**: egui (native), vanilla TS / DOM (WebView)
- **Key Libraries**:
  - `egui` — modal Window rendering, focus / keyboard input
  - `serde_yml` (existing workspace dep) — parse the SSOT yaml in
    the drift test; the helper module wraps the test with
    `#[cfg(all(test, feature = "gui"))]`
  - `happy-dom` (existing) — TS helper unit tests
- **Feature gates**: native helper is `#[cfg(feature = "gui")]`. The
  drift test is gated identically; CLI builds (`--no-default-features`)
  do not compile the helper module. Note that `serde_yml` itself is
  already in the production dependency graph (non-`gui` consumer:
  none today; non-optional in `Cargo.toml`), so the
  `cargo check --no-default-features` baseline only verifies that the
  new helper module + test are not pulled into the CLI build.

### Design Approach

- Manual mirror, **not** code generation. The yaml is the spec; native
  Rust constants and the CSS `:root` variables mirror it by hand. The
  drift test forbids silent divergence on the small set of tokens
  dialogs depend on (scrim, corner radius, padding, color roles
  presence).
- The helper modules enforce the dialog contract; callers cannot opt
  out of Window setup, MD3 styling, or keyboard handling. Caller
  responsibility is reduced to: body content closure, `(ja, en)` label
  pairs, and an outcome callback.
- `profile_selector.rs` keeps its bespoke render because its UX is
  "click row = confirm"; it only consumes shared layout constants
  (`SCRIM_ALPHA`, `CORNER_RADIUS`, `PADDING`, `ELEVATION shadow`) so
  size / shape / shadow stay locked to the rest of the dialogs.
- WebView CSS lives in a dedicated `dialog/dialog-shell.css`
  `@import`-ed from `styles.css`; the old `.profile-editor-*` classes
  are removed in this same task (Q2 default = no aliases).

### Component Interaction

```
doc/UI-DESIGN-GUIDELINES.yaml  ← SSOT (normative for dialogs)
        ├── manually mirrored ──→ src-tauri/web-shared/styles.css (:root)
        │                                   │
        │                                   └─→ .dialog-* CSS classes
        │                                          ↑ consumed by
        │                               src-tauri/web-shared/dialog/
        │                                          ↑ consumed by
        │                               profile/profile-editor.ts
        │                               ssh/ssh-editor.ts
        │
        └── manually mirrored ──→ src-tauri/src/ui/md3.rs (Palette)
                                            │
                                            └─→ src-tauri/src/ui/dialog/
                                                   ↑ consumed by
                                            ui/mux_dialogs.rs
                                            render/mod.rs (sftp dialogs)
                                            ui/profile_selector.rs
                                                (tokens-only adoption)
        ↑
   drift test reads yaml + styles.css + dialog::tokens
```

## Resolved Open Questions (locked in for this plan)

Auto-mode resolution; each follows the `default_if_unresolved` in
`sdd.yaml`. Treat these as assumptions; if the user redirects, sdd.3
verification will surface the disagreement.

| ID | Resolution |
|---|---|
| Q1 | WebView helper exposes the body container only. Callers compose form elements directly. |
| Q2 | `.profile-editor-*` classes are removed in this task. No aliases. |
| Q3 | `profile_selector.rs` keeps its bespoke render; only consumes shared layout / shadow constants from `crate::ui::dialog::tokens`. |
| Q4 | On `destructive-confirm`, Tab cycles through both buttons and reaches the primary; Enter is bound to Cancel regardless. |
| Q5 | Use the MD3 baseline error palette (hue-agnostic) and per-preset `surface-variant`; final hex table is in §10. |

## FR6 — Confirmed Hex Value Tables

The values below resolve FR6 and remove the `tbd` status. Each
preset × brightness row supplies the three roles required by the
helper.

### Dark presets (hue-derived `surface-variant`, hue-agnostic error palette)

| Preset | error-container | on-error-container | surface-variant |
|---|---|---|---|
| Purple | #8C1D18 | #F9DEDC | #49454F |
| Blue   | #8C1D18 | #F9DEDC | #44464F |
| Green  | #8C1D18 | #F9DEDC | #404943 |
| Orange | #8C1D18 | #F9DEDC | #524436 |
| Pink   | #8C1D18 | #F9DEDC | #514349 |

### Light presets (hue-derived `surface-variant`, hue-agnostic error palette)

| Preset | error-container | on-error-container | surface-variant |
|---|---|---|---|
| Purple | #F9DEDC | #410E0B | #E7E0EC |
| Blue   | #F9DEDC | #410E0B | #E1E2EC |
| Green  | #F9DEDC | #410E0B | #DBE5DD |
| Orange | #F9DEDC | #410E0B | #F0E0CD |
| Pink   | #F9DEDC | #410E0B | #F0DBE1 |

Notes:

- `error-container` / `on-error-container` follow MD3's hue-agnostic
  semantic for "error": danger reads the same across accent presets.
- `surface-variant` per preset mirrors the values already encoded for
  the WebView preset table in `web-shared/settings/ui-theme-presets.ts`
  (lines around 64–270), avoiding webview / native drift.

## Implementation Phases

### Phase 1: Token Foundation (SSOT + CSS + Rust Palette)

**Goal**: `doc/UI-DESIGN-GUIDELINES.yaml` becomes normative for dialogs,
`styles.css :root` gains the missing typescale / elevation /
error-container tokens, and `md3.rs::Palette` gains
`error_container` / `on_error_container` / `surface_variant` fields with
values populated for all 10 presets.

**Files to Create**:

- (none — all token additions land in existing files)

**Files to Modify**:

- `doc/UI-DESIGN-GUIDELINES.yaml` — add normative `dialogs:` section
  (kinds, anatomy, layout, scrim, elevation, actions, keyboard, focus,
  labels); add `tokens.elevation.elevation-3`; promote
  `error-container` / `on-error-container` / `surface-variant` to
  first-class entries under `tokens.color-roles`; remove the
  `surface-variant` entry from `known-issues`; deprecate the
  `components.modals:` section (kept as historical, but marked
  `superseded-by: dialogs`)
- `src-tauri/web-shared/styles.css` — add CSS variables
  `--md-sys-color-error-container`, `--md-sys-color-on-error-container`,
  `--md-sys-typescale-title-large-size`,
  `--md-sys-typescale-title-large-line-height`,
  `--md-sys-typescale-body-medium-size`,
  `--md-sys-typescale-body-medium-line-height`,
  `--md-sys-elevation-3` (the box-shadow value)
- `src-tauri/web-shared/settings/ui-theme-presets.ts` — extend the
  `ThemeColors` interface with `errorContainer` / `onErrorContainer`;
  add the corresponding values to each preset row using the hex table
  in §FR6; extend the existing `COLOR_TO_CSS_VAR` map (the actual
  export name; SPEC's "CSS_VARIABLE_MAP" naming is updated here) so
  `applyPresetColors` writes the new variables to `:root`
- `src-tauri/src/ui/md3.rs` — add three fields to `Palette` and to all
  ten preset constants using the FR6 table; add `pub fn
  error_container()`, `pub fn on_error_container()`, `pub fn
  surface_variant()` accessors; extend the existing
  `light_palette_matches_webview` test with a spot check on each new
  field

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `UI-DESIGN-GUIDELINES.yaml :: dialogs` | SSOT for dialog tokens + behavior | yaml file exists | downstream artifacts can be cross-checked against this section |
| `styles.css :root` | CSS variable surface for tokens | `:root` block exists | dialog-related variables resolve in all WebView bundles |
| `ui-theme-presets.ts` | Per-preset CSS variable values | preset entries exist | error/surface-variant variables are written when preset switches |
| `md3.rs::Palette` | Native MD3 token bundle | dark/light × 5 presets defined | `error_container()` / `surface_variant()` accessors return preset-specific values |

**Processing Flow**:

1. Edit yaml to insert `dialogs:` and `tokens.elevation:`.
2. Mirror new CSS variables into `:root` (dark defaults).
3. Mirror per-preset values into `ui-theme-presets.ts`.
4. Mirror per-preset values into `md3.rs`.
5. Remove the `surface-variant` known-issue.

**Implementation Steps**:

1. **Author normative `dialogs:` yaml section** — describe kinds,
   anatomy, layout numbers, scrim alpha, elevation reference, action
   role colors, keyboard table, focus table, label rules.
2. **Add `tokens.elevation` and CSS variable contracts** — record
   `elevation-3` shadow value and the typescale CSS variable names the
   helpers depend on.
3. **Add CSS variables** — write the new `--md-sys-*` entries to
   `styles.css :root` so unstyled / dark builds resolve them.
4. **Extend preset CSS map** — register the new variable names in
   `ui-theme-presets.ts` so theme switches keep them in sync; populate
   `errorContainer` / `onErrorContainer` on every preset (all 10 use
   the MD3 baseline values).
5. **Extend `Palette`** — add three fields; populate all 10 const
   palettes using the §FR6 table.
6. **Remove resolved known-issue** — delete the
   `--md-sys-color-surface-variant` known-issue entry from the yaml
   (it is now first-class).

**Dependencies**: Blocks Phase 2, Phase 3, Phase 6.

**Testing Approach**:

- Unit: extend `md3::tests` with spot checks on the new accessors
  across multiple presets (one dark + one light, plus the resolved
  `surface_variant` for one preset).
- Manual: render the settings panel under Purple-dark and Purple-light;
  confirm tokens resolve to non-empty in DevTools-less builds via log
  output if needed (existing settings panel renders are sufficient).

**Acceptance Criteria**:

- [ ] `dialogs:` section is present in yaml with all 9 sub-fields
- [ ] `tokens.elevation.elevation-3` is present in yaml
- [ ] `--md-sys-color-error-container`, `--md-sys-color-on-error-container`,
      typescale and elevation CSS variables exist in `styles.css`
- [ ] All 10 preset rows in `ui-theme-presets.ts` have
      `errorContainer` / `onErrorContainer` set, and the variable map
      includes them
- [ ] `Palette` has the 3 new fields populated for all 10 presets
- [ ] `known-issues:` no longer lists `--md-sys-color-surface-variant`
- [ ] `cargo check --no-default-features` still passes (no GUI
      dependency introduced into CLI build)

**Estimated Effort**: medium

---

### Phase 2: Native Helper Module — `src-tauri/src/ui/dialog/`

**Goal**: Provide a `Dialog` builder under
`#[cfg(feature = "gui")]` that enforces Window setup, MD3 styling, role
colors, keyboard rules, and initial focus, and that exposes a shared
`tokens` submodule of `pub const` constants for other call sites.

**Files to Create**:

- `src-tauri/src/ui/dialog/mod.rs` — `Dialog` builder, `DialogOutcome`,
  `DialogKind`, `show()` entry point, helper-enforced Window setup
- `src-tauri/src/ui/dialog/kinds.rs` — per-kind keyboard / focus / Tab
  rules (single source for the FR5 table)
- `src-tauri/src/ui/dialog/tokens.rs` — `pub const SCRIM_ALPHA`,
  `pub const CORNER_RADIUS`, `pub const PADDING`,
  `pub const ACTIONS_GAP`, `pub const TITLE_TO_BODY_MARGIN`,
  `pub const ACTIONS_TOP_MARGIN`, `pub const MAX_WIDTH_STANDARD`,
  `pub const MAX_WIDTH_COMPACT`, `pub const ELEVATION_SHADOW_*`
- `src-tauri/src/ui/dialog/buttons.rs` — role-colored button helpers
  (primary / cancel / destructive) using `md3::*`
- `src-tauri/src/ui/dialog/focus.rs` — `first_frame`-style helper state
  for "request focus once" semantics

**Files to Modify**:

- `src-tauri/src/ui/mod.rs` — register the new `dialog` module under
  `#[cfg(feature = "gui")]`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `Dialog<T>` builder | Configure kind, title, labels, body, focus | none | `show()` returns `DialogOutcome<T>` |
| `Dialog::show()` | Render egui Window with enforced contract | builder fully populated | window drawn this frame; outcome reflects user input |
| `kinds::keymap()` | Resolve Enter / Esc semantics per kind | kind known | returns which role each key targets |
| `kinds::initial_focus()` | Resolve first-frame focus target per kind | kind known | returns role/widget that receives focus |
| `tokens::*` constants | Shared layout / shadow / color values | (none) | numeric mirror of yaml `dialogs.layout` |
| `buttons::draw_role()` | Render button with role-specific MD3 colors | role known, label provided | button drawn with `bg` and `fg` from FR6 |

**Processing Flow** (`Dialog::show`):

1. Open a centered, non-collapsible, non-resizable modal Window
   (helper applies the chrome attributes; caller cannot override).
2. Inside Window: draw `Frame` with `surface_container_high()` fill,
   `CORNER_RADIUS` rounding, `ELEVATION_SHADOW_*` shadow.
3. Render title (title-large typescale, `on_surface()` color).
4. Invoke caller body closure.
5. Render actions row: primary first (or cancel-first for
   destructive-confirm) using `buttons::draw_role`.
6. Apply per-kind keyboard rule:
   - `input`: text input widgets observe an IME-safe Enter
     (lost-focus + Enter on the same frame); non-input widgets observe
     a frame-level Enter event
   - `confirm`: frame-level Enter targets primary
   - `destructive-confirm`: frame-level Enter targets cancel
   - All kinds: frame-level Escape targets cancel
7. Apply first-frame focus:
   - `input` → first focusable body widget (caller registers via
     `initial_focus(id)`)
   - `confirm` → primary button
   - `destructive-confirm` → cancel button
8. Returns `DialogOutcome::{Pending|Confirmed(T)|Cancelled}`.

**Implementation Steps**:

1. **Scaffold module + tokens constants** — create
   `src-tauri/src/ui/dialog/` with `mod.rs`, `tokens.rs`, `kinds.rs`,
   `buttons.rs`, `focus.rs`; mirror yaml `dialogs.layout` numbers into
   `tokens` constants; gate the module behind
   `#[cfg(feature = "gui")]`; register in `ui/mod.rs`.
2. **Define `DialogKind`, `DialogOutcome<T>`, builder shape** —
   author the `Dialog<'a, T>` struct and the three factory functions
   (`input`, `confirm`, `destructive_confirm`); accept `(ja, en)`
   pairs and `Locale`.
3. **Implement Window + Frame + title rendering** — apply
   `collapsible(false).resizable(false).anchor(CENTER_CENTER)` plus the
   MD3 Frame chrome; title uses title-large typescale and
   `md3::on_surface()`.
4. **Implement role-button rendering** — `buttons::draw_role` returns
   click + focus response per role; primary uses
   `md3::primary()` / `on_primary()`; cancel uses transparent bg +
   `md3::primary()` fg; destructive uses
   `md3::error_container()` / `on_error_container()`.
5. **Implement per-kind keymap and initial focus** — keep the rules
   table in `kinds.rs` so the drift test and humans both see the same
   source; respect Q4 (Tab still reaches primary on destructive-confirm
   but Enter never triggers it).
6. **Add OK-label rejection** — `primary_button` performs a
   debug-only assertion that neither locale label normalizes to "ok"
   (case-insensitive); document the contract in the helper's rustdoc.
7. **Add minimal builder-state unit tests** — verify
   `destructive_confirm` reports initial focus = cancel and Enter
   semantics = cancel (without spinning a real egui frame, by inspecting
   the resolved `kinds` rules).

**Dependencies**: Requires Phase 1 (Palette extension). Blocks Phase 4
and Phase 7.

**Testing Approach**:

- Unit: `kinds` rule table introspection; OK-label rejection
  (panic-asserting test in debug); `tokens` constants reachable.
- Integration: covered indirectly by Phase 4 refactors and Phase 6
  drift test.
- Manual: covered by Phase 4 end-to-end.

**Acceptance Criteria**:

- [ ] `crate::ui::dialog` exports `Dialog`, `DialogOutcome`,
      `DialogKind`, `tokens` module
- [ ] All `Window::new(...)` chrome attributes are applied internally
- [ ] Building `Dialog` with primary label "OK" panics in debug
      builds
- [ ] `cargo check --no-default-features` still passes (helper is
      `gui`-only)
- [ ] `cargo test --lib` passes

**Estimated Effort**: large

---

### Phase 3: WebView Helper Module — `src-tauri/web-shared/dialog/`

**Goal**: Provide `createDialogShell(opts)` returning overlay /
surface / body / actions and an `addButton` method that applies role
classes, enforces Esc / Enter behavior per kind, sets a11y attributes,
and handles scrim click + IME-safe Enter.

**Files to Create**:

- `src-tauri/web-shared/dialog/dialog-shell.ts` — exports
  `createDialogShell`, `DialogKind`, `DialogShell`,
  `DialogShellOptions`, `addButton` helper
- `src-tauri/web-shared/dialog/dialog-shell.css` — `.dialog-overlay`,
  `.dialog-surface`, `.dialog-title`, `.dialog-body`, `.dialog-actions`,
  `.dialog-button`, `.dialog-button-primary`, `.dialog-button-cancel`,
  `.dialog-button-destructive`, `.dialog-error`
- `src-tauri/web-shared/dialog/dialog-shell.test.ts` — happy-dom unit
  tests for kind-specific Enter / Esc, scrim cancel,
  `isComposing` guard

**Files to Modify**:

- `src-tauri/web-shared/styles.css` — add
  `@import "./dialog/dialog-shell.css";`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `createDialogShell(opts)` | Build overlay DOM, attach keydown / scrim listeners | document body present | returns `DialogShell` + a `close()` that detaches listeners |
| `addButton({role, label, onClick})` | Append role-classed button to actions | shell created | button appended; helper tracks which is primary / cancel |
| Per-kind keymap | Translate Enter / Esc to primary or cancel callback | role buttons registered | Enter / Esc dispatch matches FR5 table |
| Initial focus | Set focus on first paint per kind | mount complete | input → first form control; confirm → primary; destructive → cancel |
| Scrim handler | Cancel on scrim click when `scrimClickCancels` true | option set | cancel callback fires; helper does not call `close()` itself unless caller wires it |

**Processing Flow** (`createDialogShell`):

1. Create overlay `div` (.dialog-overlay) with `role="dialog"`,
   `aria-modal="true"`, `aria-label=opts.ariaLabel`, `z-index: 3000`.
2. Create surface `div` (.dialog-surface) inside overlay.
3. Create title element (`.dialog-title`) with `opts.title`.
4. Create body container (`.dialog-body`).
5. Create actions container (`.dialog-actions`).
6. Attach a single keydown listener (capture phase) on the overlay:
   - If `event.isComposing` → ignore.
   - If `Escape` → call cancel callback (or `close()` if none).
   - If `Enter`:
     - `input` / `confirm` → trigger primary callback.
     - `destructive-confirm` → trigger cancel callback.
7. Attach scrim click handler (overlay element only, not surface).
8. Apply initial focus on next animation frame per kind.
9. `close()` removes overlay + detaches listeners.

**Implementation Steps**:

1. **Build DOM structure** — overlay + surface + title + body +
   actions; apply ARIA attributes; set z-index from the existing
   `profile-modal: 3000` token.
2. **Implement `addButton`** — accept `{role, label, onClick}`; apply
   `.dialog-button` + `.dialog-button-${role}`; remember which button
   is primary and which is cancel for keymap dispatch.
3. **Implement keymap with IME-safety** — single keydown on overlay,
   guarded by `event.isComposing`; dispatch per kind.
4. **Implement scrim click → cancel** — overlay-element click handler;
   guard so clicks inside surface do not cancel.
5. **Implement initial focus per kind** — at next animation frame,
   focus first focusable form control inside `body` for `input`;
   focus primary for `confirm`; focus cancel for `destructive-confirm`.
6. **Author CSS** — port `.profile-editor-*` styling into
   `.dialog-*` classes; reference `--md-sys-color-*`,
   `--md-sys-shape-corner-*`, and the new `--md-sys-elevation-3`,
   `--md-sys-typescale-*` variables.
7. **Author happy-dom unit tests** — cover the six scenarios listed in
   `SPEC.md §7.2`.

**Dependencies**: Requires Phase 1 (CSS variables in place). Blocks
Phase 5.

**Testing Approach**:

- Unit: `bun test` against happy-dom for Esc / Enter / scrim /
  IME-composing / structure shape.
- Manual: covered by Phase 5 end-to-end.

**Acceptance Criteria**:

- [ ] `createDialogShell` returns the documented shape
- [ ] Esc, Enter (per kind), scrim, and IME-safety unit tests pass
- [ ] `bun run typecheck` passes
- [ ] `dialog-shell.css` is imported from `styles.css`

**Estimated Effort**: medium

---

### Phase 4: Refactor Native Dialogs

**Goal**: Rewrite the five native egui dialogs to call the `Dialog`
builder. Outcomes are mapped back to the existing
`MuxDialogOutcome` / `SftpFrameEvent` enums in the caller.

**Files to Create**:

- (none)

**Files to Modify**:

- `src-tauri/src/ui/mux_dialogs.rs` — rewrite `draw_rename` (kind=input,
  primary "変更"/"Rename") and `draw_move` (kind=input, primary
  "移動"/"Move"); the DragValue + arrow-key logic stays inside the body
  closure
- `src-tauri/src/render/mod.rs` (sftp dialog block) — rewrite the three
  dialogs:
  - upload: kind=confirm, primary "アップロード"/"Upload"
  - overwrite: kind=destructive-confirm, primary "上書き"/"Overwrite"
  - close_guard: kind=destructive-confirm, primary "閉じる"/"Close"
- `src-tauri/src/mux/dialog.rs` (if `focused_once` becomes redundant) —
  remove fields the helper now owns; otherwise leave unchanged

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `mux_dialogs::draw_rename` | Build `Dialog::input(...)` and translate `DialogOutcome` to `MuxDialogOutcome::ConfirmRename` | state == Rename | returns `MuxDialogOutcome` |
| `mux_dialogs::draw_move` | Build `Dialog::input(...)` with DragValue body + Arrow handling | state == Move | returns `MuxDialogOutcome` |
| `render::draw_sftp_overlay` (upload) | Build `Dialog::confirm(...)` and emit `SftpFrameEvent::Confirm/CancelUpload` | upload_dialog present | event populated |
| `render::draw_sftp_overlay` (overwrite) | Build `Dialog::destructive_confirm(...)` and emit `SftpFrameEvent::Confirm/CancelOverwrite` | overwrite_dialog present | event populated; Enter → Cancel |
| `render::draw_sftp_overlay` (close_guard) | Build `Dialog::destructive_confirm(...)` and emit `SftpFrameEvent::Confirm/CancelClose` | close_guard present | event populated; Enter → Cancel |

**Processing Flow** (per dialog):

1. Resolve title strings from `(ja, en)` pair and the current locale.
2. Pass a body closure that draws the dialog-specific content (rename
   text field, move DragValue + label, upload destination summary,
   overwrite file list, close-guard warning).
3. Pass primary + cancel labels (no "OK") and the on-confirm closure
   that returns the dialog-specific value (e.g. trimmed name).
4. Map `DialogOutcome::Confirmed(value)` → existing domain enum;
   `Cancelled` → existing cancel variant; `Pending` → leave state open.

**Implementation Steps**:

1. **Rewrite rename** — first-focus is the text field; primary label
   "変更" / "Rename"; reuse existing `resolve_rename_confirm` to keep
   empty-string handling.
2. **Rewrite move** — primary label "移動" / "Move"; ArrowUp / ArrowDown
   handling stays inside body closure; first focus is the DragValue.
3. **Rewrite upload** — `Dialog::confirm` with primary
   "アップロード" / "Upload" and cancel "キャンセル" / "Cancel"; Enter
   on primary fires `ConfirmUpload`.
4. **Rewrite overwrite** — `Dialog::destructive_confirm` with primary
   "上書き" / "Overwrite"; Enter triggers Cancel; helper renders
   primary button in destructive colors.
5. **Rewrite close_guard** — `Dialog::destructive_confirm` with primary
   "閉じる" / "Close"; Enter triggers Cancel.
6. **Reconcile `focused_once` state** — remove any state the helper
   subsumes; keep state that still belongs to the domain (e.g. the
   value being edited).

**Dependencies**: Requires Phase 2.

**Testing Approach**:

- Unit: existing `mux_dialogs::tests` (rename / move resolution) must
  keep passing.
- Integration: smoke-test via `cargo test --lib` (helper rules indirectly
  validated through `kinds.rs` and the drift test).
- Manual (per SPEC §7.3): exercise each dialog with `make dev`.

**Acceptance Criteria**:

- [ ] No `Window::new(...)` chrome literals remain in
      `mux_dialogs.rs` / sftp dialog block
- [ ] No `"OK"` button label remains
- [ ] Existing `mux_dialogs::tests` pass without modification
- [ ] `cargo test --lib` passes

**Estimated Effort**: large

---

### Phase 5: Refactor WebView Dialogs + CSS Cleanup

**Goal**: Route `profile-editor.ts` and `ssh-editor.ts` through
`createDialogShell`, rename CSS classes to `.dialog-*` everywhere they
appear (per Q2 default), and delete the legacy `.profile-editor-*`
blocks from `settings-panel.css`.

**Files to Create**:

- (none new beyond Phase 3)

**Files to Modify**:

- `src-tauri/web-shared/profile/profile-editor.ts` — replace manual
  overlay / dialog / button DOM with `createDialogShell` + `addButton`;
  inputs / labels / hint elements use new `.dialog-*` class names
- `src-tauri/web-shared/ssh/ssh-editor.ts` — same migration; remove all
  `profile-editor-*` references
- `src-tauri/web-shared/styles/settings-panel.css` — remove
  `.profile-editor-*` rule blocks; keep tab / section CSS that is not
  dialog-specific; if existing settings panel uses some classes
  (e.g. for sub-tabs) move them to a new `.dialog-tabs-*` namespace
- `src-tauri/web-shared/dialog/dialog-shell.css` — house the migrated
  rules from `settings-panel.css` for overlay / surface / body /
  actions / buttons / error
- Any TS callers that reference the legacy class names — search and
  update (the existing audit notes only profile-editor + ssh-editor)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `showProfileEditor` | Create dialog shell, mount form, attach Save / Cancel | helper imported | dialog rendered with .dialog-* classes; Enter triggers Save |
| `showSshEditor` | Same pattern for SSH editor | helper imported | dialog rendered with .dialog-* classes; Enter triggers Save |
| Migrated CSS rules | Provide the visual styling under new class names | `.dialog-*` selectors reachable | identical or near-identical visual outcome |

**Processing Flow** (per editor):

1. Call `createDialogShell({title, ariaLabel, kind: 'input'})`.
2. Append form rows to `shell.body` using the new
   `.dialog-field` / `.dialog-label` / `.dialog-input` /
   `.dialog-textarea` / `.dialog-hint` classes.
3. Register Save and Cancel via `shell.addButton({role: 'primary', ...})`
   and `shell.addButton({role: 'cancel', ...})`.
4. Helper handles Esc / Enter / focus / scrim; editor only owns
   business logic (validation, save).

**Implementation Steps**:

1. **Migrate `profile-editor.ts`** — replace overlay/dialog/title/form
   construction with `createDialogShell`; rename internal class names;
   keep validation and save logic intact.
2. **Migrate `ssh-editor.ts`** — same; ensure SSH-specific list rows
   (`ssh-option-*`) remain functional under the new container classes.
3. **Move CSS rules** — port the styles that match the new class names
   into `dialog-shell.css`; delete the old `.profile-editor-*` blocks
   from `settings-panel.css`.
4. **Search for stragglers** — grep the codebase for
   `profile-editor-` to ensure no lingering references in TS or other
   CSS files.
5. **Update tests / snapshots** — re-run `bun test` and
   `bun run typecheck`; fix any selector references in existing tests.

**Dependencies**: Requires Phase 3.

**Testing Approach**:

- Unit: existing TS tests for profile / SSH editors continue to pass
  with updated selectors.
- Manual: exercise Profile editor and SSH editor in `make dev`; verify
  Save / Cancel buttons, focus, Esc.

**Acceptance Criteria**:

- [ ] `rg "profile-editor-"` returns no hits in
      `src-tauri/web-shared/` or `src-tauri/{viewer,settings}/web/`
- [ ] `bun test` and `bun run typecheck` pass
- [ ] Profile and SSH editors render with the new helper

**Estimated Effort**: large

---

### Phase 6: Drift-Detection Tests

**Goal**: Add Rust unit tests that detect divergence between
`UI-DESIGN-GUIDELINES.yaml`, `dialog::tokens` constants, and
`web-shared/styles.css :root`. Also add OK-label rejection test.

**Files to Create**:

- `src-tauri/src/ui/dialog/tests.rs` (or `#[cfg(test)] mod tests` in
  `mod.rs`)

**Files to Modify**:

- (none — `serde_yml` is already a regular workspace dependency at
  `src-tauri/Cargo.toml:74`; reuse it from the test module instead of
  adding a new `[dev-dependencies]` entry)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `yaml_dialog_tokens_match_constants` test | Parse yaml + assert `scrim` / `corner-radius` / `padding` match `tokens::*` | yaml has `dialogs:` block | mismatched value fails the test with a descriptive message |
| `yaml_color_roles_defined_in_styles_css` test | Verify every `tokens.color-roles` entry has a `--md-sys-color-{role}:` line in styles.css | yaml has color-roles list + styles.css present | missing variable fails the test |
| `label_ok_is_rejected` test | Construct `Dialog` builder with primary label "OK" and assert it panics in debug | builder available | panic-asserting test catches the assertion in debug builds |
| `destructive_confirm_initial_focus_is_cancel` test | Inspect `kinds::initial_focus(DialogKind::DestructiveConfirm)` | `kinds` exported | returns the cancel role |
| `destructive_confirm_enter_targets_cancel` test | Inspect `kinds::enter_target(DialogKind::DestructiveConfirm)` | `kinds` exported | returns the cancel role |

**Processing Flow** (drift test):

1. The yaml file is embedded into the test binary at compile time
   (compile-time string embed) so the test does not depend on the
   runtime CWD.
2. The yaml is deserialized into a tolerant struct that only models
   the fields the test reads (all other entries ignored).
3. Cross-check three values:
   - `dialogs.scrim` decodes to the same alpha as `SCRIM_ALPHA`
   - `dialogs.layout.corner-radius` decodes to `CORNER_RADIUS`
   - `dialogs.layout.padding` decodes to `PADDING`
4. `styles.css` is embedded the same way and scanned as text.
5. For each role in `tokens.color-roles`, assert the corresponding
   `--md-sys-color-{role}:` declaration is present in the embedded
   CSS; failures aggregate the missing role names into a single
   error message.

**Implementation Steps**:

1. **Confirm `serde_yml` is already available** — `src-tauri/Cargo.toml`
   already lists `serde_yml = "0.0.11"` as a regular dependency. No
   Cargo.toml edit; re-run `cargo check --no-default-features` after the
   helper / test land to confirm the CLI build is still green.
2. **Author tolerant deserialization structs** — model only the
   fields the test reads; mark unknown fields as defaulted so the
   yaml shape can grow without breaking the test.
3. **Author scrim / radius / padding asserts** — parse the yaml strings
   ("rgba(0,0,0,0.50)", "28px", "24px") into numeric forms and compare
   to `tokens::*`.
4. **Author CSS coverage assert** — regex-scan `styles.css` for each
   role name; aggregate missing roles and fail with a combined
   message.
5. **Author OK-label rejection test** — declare it as a panic-
   asserting test so debug runs verify the assertion fires; gate so
   release builds do not execute the panic path.
6. **Author kind-rule introspection tests** — call `kinds::*` for each
   kind and assert against the FR5 table.

**Dependencies**: Requires Phase 1 (yaml dialogs section), Phase 2
(tokens constants).

**Testing Approach**:

- Unit: tests run under
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path
  src-tauri/Cargo.toml --lib`.

**Acceptance Criteria**:

- [ ] Drift test fails when yaml `dialogs.layout.padding` is changed
      without updating `PADDING`
- [ ] Drift test fails when a new role is added to yaml but not
      defined in styles.css
- [ ] OK-label test panics in debug builds
- [ ] `cargo check --no-default-features` still passes (helper module
      and its drift test are not pulled into the CLI build; note that
      `serde_yml` itself is already in the production graph and is not
      newly introduced by this task)

**Estimated Effort**: medium

---

### Phase 7: `profile_selector.rs` Shared-Constants Adoption

**Goal**: Make `profile_selector.rs` import sizing / shadow / scrim
constants from `crate::ui::dialog::tokens` instead of defining its own.
Keep the bespoke list-row render (Q3 default).

**Files to Create**:

- (none)

**Files to Modify**:

- `src-tauri/src/ui/profile_selector.rs` — remove hard-coded literal
  values for scrim alpha, corner radius, padding, shadow; import them
  from `crate::ui::dialog::tokens`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Selector chrome | Apply shared scrim / corner / padding / shadow | tokens module available | values mirror the rest of the dialog system |

**Processing Flow**:

1. Replace each local literal with the matching
   `dialog::tokens::*` constant.
2. Run unit + manual smoke-test.

**Implementation Steps**:

1. **Identify literal hits** — grep `profile_selector.rs` for `28.0`,
   `24.0`, `0.5`, `0.30`, etc.
2. **Wire the imports** — `use crate::ui::dialog::tokens;` and replace
   literals.
3. **Visual smoke check** — confirm the profile selector still renders
   identically under `make dev`.

**Dependencies**: Requires Phase 2.

**Testing Approach**:

- Unit: existing `profile_selector` tests (if any).
- Manual: smoke-test profile selector via `make dev`.

**Acceptance Criteria**:

- [ ] No hard-coded scrim / radius / padding / shadow literals remain
      in `profile_selector.rs`
- [ ] Visual rendering matches the previous appearance to within the
      shared-token precision

**Estimated Effort**: small

---

## Complete File Structure

```
doc/
  UI-DESIGN-GUIDELINES.yaml      ← extended: dialogs:, tokens.elevation
                                          (SSOT)
  tasks/dialog-design-system/
    SPEC.md                      (existing)
    要件定義書.md                (existing)
    IMPLEMENTATION.md            (this file)
    VERIFICATION.md              (verification doc)
    sdd.yaml                     (workflow + requirements + tasks/tests)
    tasks.yaml                   (phase / task index)

src-tauri/
  Cargo.toml                     (unchanged — existing serde_yml 0.0.11 reused by the drift test)
  src/
    i18n.rs                      (existing, unchanged)
    ui/
      mod.rs                     ← +pub mod dialog under #[cfg(feature="gui")]
      dialog/
        mod.rs                   (NEW)   Dialog builder + DialogOutcome
        kinds.rs                 (NEW)   per-kind keymap / focus rules
        tokens.rs                (NEW)   SCRIM_ALPHA / CORNER_RADIUS / ...
        buttons.rs               (NEW)   role-colored button helpers
        focus.rs                 (NEW)   first-frame focus helper
        tests.rs                 (NEW)   drift + OK-label + kind tests
      md3.rs                     ← +Palette fields + accessors
      mux_dialogs.rs             ← rewritten to use Dialog::input
      profile_selector.rs        ← imports tokens from dialog::tokens
    render/
      mod.rs                     ← sftp dialogs rewritten to use Dialog

  web-shared/
    styles.css                   ← +typescale / elevation / error-container
                                     :root vars; +@import dialog-shell.css
    dialog/
      dialog-shell.ts            (NEW)   createDialogShell
      dialog-shell.css           (NEW)   .dialog-* class definitions
      dialog-shell.test.ts       (NEW)   happy-dom unit tests
    settings/
      ui-theme-presets.ts        ← +errorContainer / onErrorContainer
                                     per preset; +COLOR_TO_CSS_VAR entries
    profile/
      profile-editor.ts          ← rewritten to use createDialogShell
    ssh/
      ssh-editor.ts              ← rewritten to use createDialogShell
    styles/
      settings-panel.css         ← .profile-editor-* removed
```

## Testing Strategy

- **Unit (Rust)**: `dialog::tests` cover drift + OK-label rejection +
  kind rule introspection; `md3::tests` cover new accessors; existing
  `mux_dialogs::tests` continue to pass.
- **Unit (TS)**: `dialog-shell.test.ts` covers helper structure, Esc /
  Enter per kind, scrim cancel, `isComposing` guard.
- **Integration**: indirectly via Phase 4 refactors — Rust tests under
  `--lib` exercise the helper through the rewritten call sites.
- **E2E**: project-defined E2E framework field is empty in `sdd.yaml`;
  no E2E suite to run for this task.
- **Manual** (per SPEC §7.3 + UC1 / UC2): exercise all eight dialogs
  via `make dev`, plus a Purple-light theme spot-check for the new
  light-theme tokens.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| `serde_yml` (existing) | `0.0.11` (already pinned in `src-tauri/Cargo.toml`) | Reused by the drift test to parse the SSOT yaml. Not newly added. |

No new TypeScript dependencies. No new Rust dependencies (existing
`serde_yml` is reused).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| New helper module or its drift test accidentally pulled into CLI build | Low | Medium | Gate the helper with `#[cfg(feature = "gui")]` and the test with `#[cfg(all(test, feature = "gui"))]`; re-run `cargo check --no-default-features` in Phase 6 acceptance. `serde_yml` itself is already a production dep (`Cargo.toml:74`), so it is not the regression risk here. |
| Renaming `.profile-editor-*` breaks an unrelated dependent (selector hidden in unrelated TS) | Medium | Medium | Codebase-wide `rg "profile-editor-"` in Phase 5 step 4; fix or document any remaining hit before closing the phase |
| Existing settings-panel CSS uses class names that overlap the new `.dialog-*` namespace | Low | Medium | Phase 5 step 3 explicitly moves only the editor-related rules; subtab and field rules stay in `settings-panel.css` if they are reused outside dialogs |
| MD3 baseline error palette feels off-brand on Orange / Pink presets | Medium | Low | Documented in §FR6; users can later add preset-specific reds; the drift test does not constrain hue, only token presence |
| `Dialog::primary_button` debug assertion triggers in legitimate use due to label normalization | Low | Low | Match only against `"OK"` / `"Ok"` / `"ok"` exact strings; document the rule in `dialog/mod.rs` rustdoc |
| `surface_variant` adoption in non-dialog components shifts their appearance | Low | Low | Out-of-scope but allowed per 要件定義 §5.2; spot-check via `make dev` |

## Open Questions

- [ ] Should the destructive primary button additionally render a small
      icon (e.g. warning glyph) to reinforce destructiveness? (Out of
      scope for this task; revisit if visual contrast proves
      insufficient.)
- [ ] Should the drift test also verify `dialogs.elevation` shadow
      string ↔ Rust shadow constants byte-equal? (Optional; not
      required by FR7 wording.)

## Success Metrics

- [ ] All eight dialogs reach the user through the new helpers (or, for
      `profile_selector.rs`, share the same layout constants)
- [ ] No literal `"OK"` button label remains in any dialog
- [ ] `cargo test --lib` passes with the drift + OK-label + kind tests
      included
- [ ] `bun test` and `bun run typecheck` pass with the new TS helper
      and refactored editors
- [ ] `cargo check --no-default-features` still passes (CLI build
      unaffected)
- [ ] `rg "profile-editor-"` returns no hits in `src-tauri/web-shared/`
