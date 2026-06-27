# SPEC: Dialog Design System

## 1. Summary

Establish a normative Material Design 3 (MD3) based design system for all
modal dialogs in eMterm, covering both the native egui side and the child
WebView (HTML/CSS) side. Refactor all eight existing dialogs to consume
the shared system through new helpers. Promote
`doc/UI-DESIGN-GUIDELINES.yaml` to the single source of truth (SSOT) for
dialog tokens and rules, and add a drift-detection test.

## 2. Background

### 2.1 Existing assets

| File | Role |
|---|---|
| `doc/UI-DESIGN-GUIDELINES.yaml` | MD3 tokens and a descriptive snapshot of component CSS |
| `src-tauri/src/ui/md3.rs` | Native-side MD3 palette (5 presets × dark/light) for egui |
| `src-tauri/web-shared/styles.css` (`:root`) | MD3 color / shape / motion CSS variables |
| `src-tauri/src/ui/profile_selector.rs` | Existing manual MD3-shaped egui dialog (reference impl) |
| `src-tauri/web-shared/styles/settings-panel.css` `.profile-editor-*` | Existing dialog CSS (will be renamed) |

### 2.2 Existing dialogs (eight total)

| # | File | Kind | Notes |
|---|---|---|---|
| 1 | `src-tauri/src/ui/mux_dialogs.rs::draw_rename` | input | text input; first-focus already handled ad-hoc |
| 2 | `src-tauri/src/ui/mux_dialogs.rs::draw_move` | input | `DragValue`; first-focus never wired, fixed via custom Arrow keys |
| 3 | `src-tauri/src/render/mod.rs` upload | confirm | non-destructive |
| 4 | `src-tauri/src/render/mod.rs` overwrite | destructive-confirm | currently maps Enter to Cancel (intentional) |
| 5 | `src-tauri/src/render/mod.rs` close_guard | destructive-confirm | currently maps Enter to Close (inconsistent with #4) |
| 6 | `src-tauri/src/ui/profile_selector.rs` | input (list-select) | already MD3-shaped via manual `Frame + Shadow + Rounding` |
| 7 | `src-tauri/web-shared/profile/profile-editor.ts` | input | uses `.profile-editor-*` classes |
| 8 | `src-tauri/web-shared/ssh/ssh-editor.ts` | input | borrows `.profile-editor-*` classes verbatim |

### 2.3 Problem statement

The audit in `tmp/dialog-design-system-audit.md` lists the concrete
divergences (`Window` setup duplicated five times, button label
granularity inconsistent, Enter-key behavior implicit, destructive-button
coloring absent, first-focus rule absent, child-WebView class names
leaked across editors, design tokens not shared between native and
WebView).

## 3. Requirements (mapped to 要件定義書)

### FR1: Dialog design tokens & spec in `UI-DESIGN-GUIDELINES.yaml`

Extend `doc/UI-DESIGN-GUIDELINES.yaml` so that it is the **normative** SSOT
for dialog implementations.

Required additions / changes:

- New top-level section `dialogs:` (deprecates and supersedes the
  existing `components.modals:` section).
- `tokens.elevation:` new section. Define `elevation-3` for dialogs as
  `box-shadow: 0 8px 32px rgba(0,0,0,0.30)` (mirrors existing dialog
  shadow already used).
- `tokens.typography` entries that dialogs use must also be exposed as
  CSS custom properties at `:root`:
    - `--md-sys-typescale-title-large-size: 22px`
    - `--md-sys-typescale-title-large-line-height: 28px`
    - `--md-sys-typescale-body-medium-size: 14px`
    - `--md-sys-typescale-body-medium-line-height: 20px`
- Promote `error-container` / `on-error-container` / `surface-variant`
  to first-class roles in `tokens.color-roles` and add them to both
  `:root` and `md3.rs::Palette`.
- New `dialogs:` content (normative):
    - `dialogs.kinds`: `input`, `confirm`, `destructive-confirm`
    - `dialogs.anatomy`: `overlay` (scrim), `surface` (dialog box),
      `header` (title), `body` (content), `actions` (button row)
    - `dialogs.layout`:
        - corner-radius: `corner-extra-large` (28px)
        - padding: `24px` (token `spacing.lg`)
        - max-width: `480px` (standard) / `400px` (compact)
        - max-height: `80vh` / `60vh`
        - actions-gap: `8px` (token `spacing.xs`)
        - title→body margin: `16px` (`spacing.md`)
        - actions-top-margin: `16px`
    - `dialogs.scrim`: `rgba(0, 0, 0, 0.50)`
    - `dialogs.elevation`: `elevation-3`
    - `dialogs.actions`:
        - `primary`: bg `primary`, fg `on-primary`
        - `cancel`: bg transparent, fg `primary`
        - `destructive`: bg `error-container`, fg `on-error-container`
    - `dialogs.keyboard`: per-kind Enter/Esc/Tab behavior (see FR5)
    - `dialogs.focus`: per-kind initial focus rule (see FR5)
    - `dialogs.labels`: rules — generic "OK" forbidden, Cancel uses
      "キャンセル" / "Cancel" verbatim, primary uses a verb
- Remove from `known-issues` any entry resolved by this work.

### FR2: Native helper module — `src-tauri/src/ui/dialog/`

New module with feature gate `#[cfg(feature = "gui")]`.

Public API sketch:

```rust
// src-tauri/src/ui/dialog/mod.rs
pub enum DialogOutcome<T> {
    Pending,
    Confirmed(T),
    Cancelled,
}

pub struct Dialog<'a, T> {
    title: (&'a str, &'a str),                              // (ja, en)
    locale: crate::i18n::Locale,
    kind: DialogKind,
    primary: Option<(&'a str, &'a str)>,
    cancel: (&'a str, &'a str),                             // defaults: "キャンセル"/"Cancel"
    body: Option<Box<dyn FnMut(&mut egui::Ui) + 'a>>,
    on_confirm: Option<Box<dyn FnOnce() -> T + 'a>>,
    // Input-kind initial focus: static id known at builder time, OR a
    // shared slot that the body closure populates with `Response::id`
    // during draw (needed when the focus target's id is only available
    // after `text_edit_singleline` runs).
    initial_focus_id: Option<egui::Id>,
    initial_focus_slot: Option<Rc<Cell<Option<egui::Id>>>>,
    window_id: Option<egui::Id>,                            // override the auto-derived id
    width: f32,                                             // WIDTH_COMPACT by default
}

pub enum DialogKind {
    Input,
    Confirm,
    DestructiveConfirm,
}

impl<'a, T> Dialog<'a, T> {
    pub fn input(title_ja: &'a str, title_en: &'a str, locale: Locale) -> Self;
    pub fn confirm(title_ja: &'a str, title_en: &'a str, locale: Locale) -> Self;
    pub fn destructive_confirm(title_ja: &'a str, title_en: &'a str, locale: Locale) -> Self;
    pub fn body(self, body: impl FnMut(&mut egui::Ui) + 'a) -> Self;
    pub fn primary_button(self, ja: &'a str, en: &'a str, on_confirm: impl FnOnce() -> T + 'a) -> Self;
    pub fn cancel_button(self, ja: &'a str, en: &'a str) -> Self; // defaults: "キャンセル"/"Cancel"
    pub fn initial_focus(self, id: egui::Id) -> Self;
    pub fn initial_focus_slot(self, slot: Rc<Cell<Option<egui::Id>>>) -> Self;
    pub fn window_id(self, id: egui::Id) -> Self;
    pub fn standard_width(self) -> Self;            // 480px + 80vh cap (default: 400px + 60vh)
    pub fn show(self, ctx: &egui::Context) -> DialogOutcome<T>;
}
```

Helper-enforced contract (caller cannot opt out):

- Surface assembled from `egui::Area` (foreground order,
  `anchor=CENTER_CENTER`) wrapping an `egui::Frame` — `egui::Window` is
  intentionally avoided because its persisted Resize state fights
  `auto_sized` / `default_size` / `max_width` and re-opens at a stale
  size. A separate Middle-order `Area` paints the `dialogs.scrim` and
  treats outside-click as cancel.
- `Frame` with `md3::surface_container_high()` fill, `corner-extra-large`
  rounding, MD3 elevation-3 shadow, and `padding` inner-margin
- Body content sits inside an `egui::ScrollArea::vertical()` bounded
  by the `dialogs.layout.max-height-*` token so tall content scrolls
  inside the surface instead of pushing the actions row off-screen
- Title rendered with title-large typescale and `md3::on_surface()` color
- Buttons rendered with role-specific colors (FR6), a single
  helper-provided horizontal spacing, and the
  `components.buttons.modal-actions` min-size token
- Keyboard handling (FR5) is applied inside `show()`
- Initial focus (FR5) is applied on the first frame of every open
  epoch (initial open OR close → reopen); subsequent contiguous frames
  are no-op

Caller responsibilities (kept):

- Provide a body closure that draws the content
- Provide labels (helper rejects "OK" via debug assertion in tests)
- Translate the resulting `DialogOutcome<T>` into the existing
  `MuxDialogOutcome` / `SftpFrameEvent` enums

Reject-"OK" rule: `Dialog::primary_button` should `debug_assert!` that
the label is not "OK" / "Ok" / "ok"; a Rust unit test enumerates this
constraint to keep production builds from regressing.

### FR3: WebView helper module — `src-tauri/web-shared/dialog/`

New module containing:

- `dialog-shell.ts` exporting `createDialogShell(opts)`
- `dialog-shell.css` (imported from `styles.css`) with `dialog-*` classes

API sketch:

```ts
// src-tauri/web-shared/dialog/dialog-shell.ts
export type DialogKind = 'input' | 'confirm' | 'destructive-confirm';

export interface DialogShellOptions {
  title: string;
  ariaLabel: string;
  kind: DialogKind;
  /** When true, scrim click acts as cancel. Default true. */
  scrimClickCancels?: boolean;
}

export interface DialogShell {
  overlay: HTMLDivElement;
  surface: HTMLDivElement;
  body: HTMLDivElement;
  actions: HTMLDivElement;
  /** Append a button; helper applies role-specific class. */
  addButton(opts: {
    label: string;
    role: 'primary' | 'cancel' | 'destructive';
    onClick: () => void;
  }): HTMLButtonElement;
  /** Remove from DOM and detach listeners. */
  close: () => void;
}

export function createDialogShell(opts: DialogShellOptions): DialogShell;
```

CSS classes (replaces `.profile-editor-*`):

- `.dialog-overlay` (was `.profile-editor-overlay`)
- `.dialog-surface` (was `.profile-editor-dialog`)
- `.dialog-title` (was `.profile-editor-title`)
- `.dialog-body` (form container — was `.profile-editor-form`)
- `.dialog-actions` (was `.profile-editor-buttons`)
- `.dialog-button`, `.dialog-button-primary`, `.dialog-button-cancel`,
  `.dialog-button-destructive`
- `.dialog-error` (was `.profile-editor-error`)

Helper-enforced contract:

- Sets `role="dialog"`, `aria-modal="true"`, applies `aria-label`
- Sets `z-index: 3000` (token `z-index.profile-modal`)
- Esc closes via the helper's keydown listener; helper calls the
  cancel-button callback if one was registered, otherwise calls `close`
- Enter dispatches per kind:
    - `input`: triggers the primary button callback
    - `confirm`: triggers the primary button callback
    - `destructive-confirm`: triggers the cancel callback
- Initial focus on first paint:
    - `input`: helper focuses the first `<input>` / `<textarea>` /
      `<select>` inside `body` (caller-appended order)
    - `confirm`: helper focuses the primary button
    - `destructive-confirm`: helper focuses the cancel button
- Scrim click triggers cancel callback when `scrimClickCancels` is true
- `close()` removes overlay and detaches keydown listener

### FR4: Refactor existing dialogs

All eight dialogs from §2.2 must route through the helpers above.

Allowed exception: `profile_selector.rs` does not need to call the
`Dialog` builder if doing so would force a synthetic primary button (the
list-row click IS confirmation). It MUST still consume the shared layout
tokens (corner radius, padding, scrim alpha, shadow) by sharing constants
with the helper. Specifically, expose `pub mod tokens` inside
`src-tauri/src/ui/dialog/` containing `pub const SCRIM_ALPHA`,
`pub const CORNER_RADIUS`, `pub const PADDING`, etc., and let
`profile_selector.rs` import these instead of defining its own.

Label table (final):

| Dialog | ja primary | en primary |
|---|---|---|
| Rename window | `変更` | `Rename` |
| Move window | `移動` | `Move` |
| Upload (sftp) | `アップロード` | `Upload` |
| Overwrite (sftp) | `上書き` | `Overwrite` |
| Close tab guard | `閉じる` | `Close` |
| Profile selector | (list-row click; no primary button) | (same) |
| Profile editor | `保存` | `Save` |
| SSH editor | `保存` | `Save` |

The "OK" label is removed everywhere.

### FR5: Keyboard rules (normative)

| Kind | Enter | Esc | Tab order | Initial focus |
|---|---|---|---|---|
| input | primary (only when no text widget owns focus OR the primary button owns focus — guards IME-composition Enter) | cancel | inputs → primary → cancel | first input |
| confirm | primary | cancel | primary → cancel | primary |
| destructive-confirm | cancel | cancel | primary → cancel | cancel |

Notes:

- Native side: helper detects first-frame-of-open via
  `Context::cumulative_pass_nr()` continuity (a window-scoped
  `last_drawn_pass_nr` flag in egui memory; the +1 delta breaks
  whenever the dialog skipped a frame, i.e. closed → reopened). This
  preserves the "fire once per open" semantics WITHOUT the persisted-
  flag bug where re-opening the same dialog would skip restoration.
- Native side: helper applies `ui.input(|i| i.key_pressed(Enter))` for
  ALL kinds. For `Input` kind it additionally guards on the currently-
  focused widget id: Enter maps to primary only when nothing owns
  focus or the primary button owns focus, so a text-field's IME
  commit Enter is not stolen as a dialog confirm.
- WebView side: helper attaches `keydown` to the overlay (capture phase)
  and respects `event.isComposing` to avoid stealing IME-commit Enter.

### FR6: Coloring rules (normative)

| Role | Native (`md3::*`) | CSS variable |
|---|---|---|
| primary | bg `primary()`, fg `on_primary()` | `--md-sys-color-primary` / `--md-sys-color-on-primary` |
| cancel | bg transparent, fg `primary()`, optional hover `color-mix(primary 8%, transparent)` | `--md-sys-color-primary` / transparent |
| destructive | bg `error_container()`, fg `on_error_container()` | `--md-sys-color-error-container` / `--md-sys-color-on-error-container` |

Adding `error_container()`, `on_error_container()`, `surface_variant()`
accessors and `Palette` fields requires hex values for all 10 presets
(5 hues × dark/light). Source the hex values from MD3 reference palettes
matching each hue. Specific hex values to be set during implementation
(IMPLEMENTATION.md will own these tables).

### FR7: Drift-detection test

Add a Rust unit test under `src-tauri/src/ui/dialog/tests.rs` (or
inline). The test:

1. Loads `doc/UI-DESIGN-GUIDELINES.yaml` via `include_str!` + a YAML
   parser (use `serde_yaml` which is already a dependency in
   `app_settings`; pull it into `src-tauri` if not already there — see
   §6).
2. Asserts that `dialogs.scrim` matches `SCRIM_ALPHA` constant.
3. Asserts that `dialogs.layout.corner-radius` matches `CORNER_RADIUS`.
4. Asserts that `dialogs.layout.padding` matches `PADDING`.
5. Asserts that every `--md-sys-color-*` referenced in
   `tokens.color-roles` is also defined as a CSS variable in
   `src-tauri/web-shared/styles.css :root` (regex scan of
   `include_str!("../../web-shared/styles.css")`).

The test is feature-gated `#[cfg(feature = "gui")]` (helper itself is
`gui`-only).

## 4. Non-Functional Requirements

- **NFR1 (compatibility)**: Esc=cancel and Enter-on-primary-non-
  destructive behaviors are preserved. Button label changes are
  intentional UX changes.
- **NFR2 (build)**: CLI build (`--no-default-features`) must not pull
  in the `dialog` helper or yaml-parsing dependency.
- **NFR3 (i18n)**: Helper takes `(ja, en)` pairs; runtime locale comes
  from `crate::i18n::Locale` (native) or the existing `t()` (TS).
- **NFR4 (workflow rules)**: Tests use
  `CARGO_TARGET_DIR=src-tauri/target` and `--manifest-path
  src-tauri/Cargo.toml --lib` per
  `.claude/rules/build-location.md`.

## 5. Architecture

### 5.1 Native side

```
src-tauri/src/ui/
  dialog/
    mod.rs          - Dialog builder + DialogOutcome
    kinds.rs        - DialogKind, label/key/focus rules per kind
    tokens.rs       - SCRIM_ALPHA, CORNER_RADIUS, PADDING, ELEVATION shadow
    tests.rs        - drift test, OK-label rejection test
  md3.rs            - extended: error_container, on_error_container, surface_variant
  mux_dialogs.rs    - rewritten to use Dialog::input
  profile_selector.rs - imports tokens from dialog::tokens
render/mod.rs       - rewritten to use Dialog::confirm and Dialog::destructive_confirm
```

`Dialog::show()` returns `DialogOutcome<T>` to the caller, which is then
mapped to `MuxDialogOutcome::Confirm*` / `SftpFrameEvent::*` as before.

### 5.2 WebView side

```
src-tauri/web-shared/
  dialog/
    dialog-shell.ts - createDialogShell()
    dialog-shell.css - .dialog-* classes (imported from styles.css)
  profile/profile-editor.ts - rewritten to use createDialogShell
  ssh/ssh-editor.ts - rewritten to use createDialogShell
  styles/settings-panel.css - .profile-editor-* removed (or aliased)
styles.css         - imports dialog/dialog-shell.css + new tokens
```

The CSS for `.dialog-*` lives in a new file
`src-tauri/web-shared/dialog/dialog-shell.css` so the helper module is
self-contained. `styles.css` adds an `@import "./dialog/dialog-shell.css"`
line.

### 5.3 Token flow

```
doc/UI-DESIGN-GUIDELINES.yaml  ← SSOT (descriptive + normative for dialogs)
        │
        ├── manually mirrored ──→ src-tauri/web-shared/styles.css (:root)
        │                                   │
        │                                   └─→ consumed by .dialog-* CSS classes
        │
        └── manually mirrored ──→ src-tauri/src/ui/md3.rs (Palette + accessors)
                                            │
                                            └─→ consumed by src-tauri/src/ui/dialog/
        ↑
   drift-check (Rust test) reads yaml + styles.css + dialog::tokens
```

No codegen. The drift test prevents silent divergence on values used by
dialogs; broader token drift (e.g. typography in non-dialog components)
is out of scope.

## 6. Dependencies

- Rust: `serde_yaml` (or `serde_yml`). The workspace already declares
  yaml-related crates indirectly; confirm via
  `cargo tree --manifest-path src-tauri/Cargo.toml | grep -i yaml`
  during implementation. If absent, add it as a `dev-dependency` to
  `src-tauri/Cargo.toml` (test-only, no production code path) — this
  keeps CLI builds untouched.
- TS: none new. Helper is < 200 LOC of vanilla TS.

## 7. Test Plan

### 7.1 Rust unit tests (mandatory)

- `dialog::tests::label_ok_is_rejected` — building a `Dialog` with
  primary label "OK" panics in debug.
- `dialog::tests::yaml_dialog_tokens_match_constants` — yaml ↔ Rust
  constants.
- `dialog::tests::yaml_color_roles_defined_in_styles_css` — yaml ↔ CSS.
- `dialog::tests::destructive_confirm_initial_focus_is_cancel` — assert
  via builder state inspection.
- Existing `mux_dialogs::tests` must keep passing after rewrite.

Run via:

```bash
CARGO_TARGET_DIR=src-tauri/target cargo test \
  --manifest-path src-tauri/Cargo.toml --lib
```

### 7.2 TypeScript unit tests

Add `src-tauri/web-shared/dialog/dialog-shell.test.ts` covering:

- `createDialogShell` returns the expected structure
- Esc keydown triggers cancel callback
- Enter keydown on `input` kind triggers primary callback
- Enter keydown on `destructive-confirm` kind triggers cancel callback
- Scrim click triggers cancel when `scrimClickCancels: true`
- IME-composing Enter is ignored (uses `event.isComposing = true` in
  happy-dom)

Run via:

```bash
bun test
bun run typecheck
```

### 7.3 Manual verification

Visual confirmation by the developer running `make dev` and exercising:

- Rename window dialog (Enter confirms, Esc cancels, OK gone)
- Move window dialog (↑↓ still works, primary label = "移動")
- Upload confirm (Enter confirms)
- Overwrite confirm (Enter cancels, default focus on Cancel,
  Overwrite button visibly red/destructive)
- Close-tab guard (Enter cancels, default focus on Cancel)
- Profile editor (Enter on form submits via primary)
- SSH editor (same)
- Profile selector (Enter picks highlighted row)

## 8. Out of Scope

- Replacing egui primitives (`Area` / `Frame` / `ScrollArea` / `Button`)
  with a hand-rolled widget toolkit; we accept egui's limits (no MD3
  state-layer button hover, focus ring style differs)
- Non-dialog components (tabs, navigation, settings rows) — they
  already use the same MD3 palette and are not the source of the
  complaint
- Markdown viewer's `.link-confirm-dialog-*` is currently dead code in
  the `dist/` bundle (per audit §1.3); cleaning it up is a separate
  task
- Animation polish beyond the existing `--md-motion-*` tokens

## 9. Open Questions

| ID | Question | Default for implementation if unresolved |
|---|---|---|
| Q1 | Should the WebView helper expose `addInput()` / `addSelect()` factories, or only `body` container? | Body container only; caller composes form |
| Q2 | Should `dialog-shell.css` retain backward-compat aliases like `.profile-editor-overlay { @extend .dialog-overlay; }` during this PR, or fully drop them? | Fully drop them; complete the rename in this task |
| Q3 | Should `profile_selector.rs` fully migrate to the `Dialog` builder, or keep its bespoke render and only adopt shared constants? | Keep bespoke render, adopt `dialog::tokens` for sizes/colors |
| Q4 | Should `destructive-confirm` allow Tab to reach the primary button at all? | Yes — Tab still cycles to primary, but never via Enter |
| Q5 | Light theme palette values for `error_container` / `on_error_container` / `surface_variant` per preset | Reference values from MD3 baseline; documented in IMPLEMENTATION.md |

## 10. References

- Audit: `tmp/dialog-design-system-audit.md`
- Requirements: `doc/tasks/dialog-design-system/要件定義書.md`
- Existing tokens (CSS): `src-tauri/web-shared/styles.css`
- Existing tokens (native): `src-tauri/src/ui/md3.rs`
- Existing yaml: `doc/UI-DESIGN-GUIDELINES.yaml`
- Reference egui MD3 dialog: `src-tauri/src/ui/profile_selector.rs`
- Reference CSS dialog: `src-tauri/web-shared/styles/settings-panel.css`
  (lines 1400–1700)
