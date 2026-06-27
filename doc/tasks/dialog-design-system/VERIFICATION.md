# Verification Document: Dialog Design System

## Implementation Results (filled by sdd.4-implement)

### Build Verification (actual)

- `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --lib` — exit 0
- `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` — exit 0 (NFR2 confirmed)
- `bun run typecheck` — exit 0

### Test Verification (actual)

- `cargo test --lib ui::dialog::` — 12 passed / 0 failed (TS-9, TS-10, TS-11, TS-12, TS-21)
- `cargo test --lib ui::md3::` — 9 passed / 0 failed (TS-20, TS-23)
- `cargo test --lib ui::mux_dialogs::` — 2 passed / 0 failed (TS-24)
- `cargo test --lib ui::` — 143 passed / 0 failed
- `bun test` — 20 passed / 0 failed (TS-6, TS-7, TS-14–TS-18, TS-22)
- `cargo test --lib` (full) — 2001 passed / 1 failed
  - **Known pre-existing failure (not introduced by this task)**:
    `tabs::tests::welcome_without_windows_leaves_group_none` — failure
    is in the mux fresh-start bootstrap path (recent commit `1d9ec54
    fix(mux): bootstrap initial window on fresh-start attach`), independent
    of the dialog subsystem.

### Code Quality Verification (actual)

- `rustfmt` applied to modified Rust files (helper module + refactored callers); crate-wide `cargo fmt` was intentionally not run
- `bunx biome format --write` applied to modified TS / CSS files (9 files, no fixes needed)

### File Structure Verification

Files to Create — all created:
- src-tauri/src/ui/dialog/mod.rs ✓
- src-tauri/src/ui/dialog/kinds.rs ✓
- src-tauri/src/ui/dialog/tokens.rs ✓
- src-tauri/src/ui/dialog/buttons.rs ✓
- src-tauri/src/ui/dialog/focus.rs ✓
- src-tauri/src/ui/dialog/tests.rs ✓
- src-tauri/web-shared/dialog/dialog-shell.ts ✓
- src-tauri/web-shared/dialog/dialog-shell.css ✓
- src-tauri/web-shared/dialog/dialog-shell.test.ts ✓

Files to Modify — all modified:
- doc/UI-DESIGN-GUIDELINES.yaml ✓
- src-tauri/web-shared/styles.css ✓
- src-tauri/web-shared/settings/ui-theme-presets.ts ✓
- src-tauri/src/ui/md3.rs ✓
- src-tauri/src/ui/mod.rs ✓
- src-tauri/src/ui/mux_dialogs.rs ✓
- src-tauri/src/render/mod.rs ✓
- src-tauri/src/ui/profile_selector.rs ✓
- src-tauri/web-shared/profile/profile-editor.ts ✓
- src-tauri/web-shared/ssh/ssh-editor.ts ✓
- src-tauri/web-shared/styles/settings-panel.css ✓ (`.profile-editor-*` blocks deleted)
- src-tauri/web-shared/components/md3-select.css ✓ (`.profile-editor-field` → `.dialog-field`)
- src-tauri/src/mux/dialog.rs ✓ (helper subsumed `focused_once`; removed)
- src-tauri/src/app.rs ✓ (Rename constructor: dropped `focused_once`)

### `profile-editor-*` Audit (TS-26)

`grep -rln "profile-editor-" src-tauri/ --include='*.ts' --include='*.css' --include='*.tsx' | grep -v dist`
returns no hits.

### Existing E2E Regression (Phase 3.8)

No E2E test suite is defined for this feature (`sdd.yaml.project.components.*.e2e_test_command` empty); skipped.

---

## Overview

**Feature**: Dialog Design System
**SPEC.md**: `doc/tasks/dialog-design-system/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/dialog-design-system/IMPLEMENTATION.md`

## Build Verification

### Native (Rust)

- Quick check (default features):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- CLI-only check (asserts NFR2): 
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Release (user-facing binary): 
  `CARGO_TARGET_DIR=src-tauri/target-host cargo build --release --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no warnings introduced by the helper module

### WebView (TypeScript)

- Bundle: `bun run build:viewer && bun run build:settings`
- Typecheck: `bun run typecheck`
- Expected: exit code 0, no `profile-editor-*` references remaining

## Test Verification

### Rust unit tests

- Command:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Notes: replay-like tests that are flaky under parallelism can be
  re-run with `--test-threads=1` if needed (consistent with
  project notes).
- Coverage target: focus on the new `ui::dialog::*` modules
  (drift / OK-label / kind rules) and the extended `ui::md3::tests`.

### TypeScript unit tests

- Command: `bun test`
- Scope: `src-tauri/web-shared/dialog/dialog-shell.test.ts` plus any
  existing tests touching profile/SSH editors.

### Test Scenarios

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1  | Rename window dialog: type a name, press Enter | Confirms rename; helper returns `Confirmed(name)`; primary label is "変更"/"Rename"; no "OK" anywhere | Unit (Rust) + Manual |
| TS-2  | Move window dialog: focus, press ArrowUp / Down to change target, press Enter | Move target increments / decrements; Enter confirms; primary label is "移動"/"Move" | Unit (Rust) + Manual |
| TS-3  | SFTP upload confirm: present file list, press Enter | Emits `SftpFrameEvent::ConfirmUpload`; primary label is "アップロード"/"Upload" | Unit (Rust) + Manual |
| TS-4  | SFTP overwrite confirm (destructive): press Enter | Emits `SftpFrameEvent::CancelOverwrite` (Enter → Cancel); initial focus is on Cancel; primary button renders in destructive colors (`error_container` bg) | Unit (Rust) + Manual |
| TS-5  | Close-tab guard (destructive): press Enter | Emits `SftpFrameEvent::CancelClose` (Enter → Cancel); initial focus on Cancel; primary label "閉じる"/"Close" | Unit (Rust) + Manual |
| TS-6  | Profile editor opens, user types into a field, presses Enter | Save invoked; helper-built shell has `.dialog-*` classes; no `.profile-editor-*` classes on the document | Unit (TS) + Manual |
| TS-7  | SSH editor opens, user types, presses Enter | Save invoked; same helper-built shell; no `.profile-editor-*` classes | Unit (TS) + Manual |
| TS-8  | Profile selector renders with shared layout constants | No literal scrim / radius / padding / shadow values in `profile_selector.rs`; visual appearance matches prior baseline | Unit (Rust grep-style) + Manual |
| TS-9  | YAML ↔ Rust constants drift detection (scrim / corner-radius / padding) | Test fails if a value is changed in yaml without updating `dialog::tokens`, and vice versa | Unit (Rust) |
| TS-10 | YAML ↔ CSS variables drift detection (every `tokens.color-roles` entry is defined in `styles.css :root`) | Test fails if a role is added in yaml but not declared in `:root` | Unit (Rust) |
| TS-11 | Constructing a Dialog with primary label "OK" panics in debug builds | Panic-asserting test passes; helper rustdoc documents the contract | Unit (Rust) |
| TS-12 | `kinds::initial_focus(DestructiveConfirm)` returns cancel | Equality assertion passes; mirrors FR5 table | Unit (Rust) |
| TS-13 | Helper-built Window applies enforced chrome | `collapsible=false`, `resizable=false`, anchor=CENTER_CENTER; observed via builder-state introspection (no real egui frame required) | Unit (Rust) |
| TS-14 | WebView shell: Esc keydown triggers cancel callback | happy-dom dispatch of `Escape` calls the registered cancel handler | Unit (TS) |
| TS-15 | WebView shell: Enter on `input` kind triggers primary callback | happy-dom dispatch fires primary; `event.isComposing=false` | Unit (TS) |
| TS-16 | WebView shell: Enter on `destructive-confirm` triggers cancel callback | happy-dom dispatch fires cancel; primary remains untouched | Unit (TS) |
| TS-17 | WebView shell: scrim click triggers cancel when `scrimClickCancels=true` | Click on overlay (not surface) fires cancel | Unit (TS) |
| TS-18 | WebView shell: IME-composing Enter is ignored | Dispatch with `isComposing=true` is a no-op | Unit (TS) |
| TS-19 | CLI build (`--no-default-features`) does not pull in the new `ui::dialog` helper module or its drift test | `cargo check --no-default-features` succeeds. (Note: `serde_yml` is already in the production graph at `Cargo.toml:74` for the GUI-only viewer; this test does NOT regress that, but it also does not assert serde_yml absence.) | Unit (Build) |
| TS-20 | All 5 light-theme presets have `error_container` / `on_error_container` / `surface_variant` populated | `md3::tests` cover one entry per preset; values match §FR6 table | Unit (Rust) |
| TS-21 | YAML `known-issues:` no longer lists `--md-sys-color-surface-variant` | Drift test reads `known-issues:` block and asserts the entry is absent | Unit (Rust) |
| TS-22 | WebView shell: scrim click does NOT cancel when `scrimClickCancels=false` | happy-dom dispatch: cancel handler not called | Unit (TS) |
| TS-23 | `md3::error_container()` / `on_error_container()` / `surface_variant()` accessors return preset-specific values | Spot-check Purple-dark, Blue-dark, Orange-light, Pink-light | Unit (Rust) |
| TS-24 | Existing `mux_dialogs::tests` (rename trim, move range) pass after rewrite | `resolve_rename_confirm` / `resolve_move_confirm` test cases unchanged and green | Unit (Rust) |
| TS-25 | Visual smoke: Purple-dark vs Purple-light theme switch shows correct dialog tokens | Switching theme in settings panel re-skins both editors and SFTP confirm dialogs without missing variables | Manual |
| TS-26 | `rg "profile-editor-"` returns zero hits in `src-tauri/web-shared/` and `src-tauri/{viewer,settings}/web/` | Empty grep result | Unit (grep-style) |

## Code Quality Verification

- Rust format: only touch the files this task edits; do NOT run
  `cargo fmt` crate-wide (per project feedback "cargo fmt をクレート全体に走らせない").
- TS format: `bunx biome format --write src-tauri/web-shared src-tauri/viewer/web src-tauri/settings/web`
  (limit to the touched files where possible).
- Static analysis: `bun run typecheck` (TS); `cargo check` (Rust).

## File Structure Verification

### Files to Create

- `src-tauri/src/ui/dialog/mod.rs` — Dialog builder + DialogOutcome
- `src-tauri/src/ui/dialog/kinds.rs` — per-kind keymap/focus rules
- `src-tauri/src/ui/dialog/tokens.rs` — shared layout constants
- `src-tauri/src/ui/dialog/buttons.rs` — role-colored button helpers
- `src-tauri/src/ui/dialog/focus.rs` — first-frame focus helper
- `src-tauri/src/ui/dialog/tests.rs` — drift / OK-label / kind tests
- `src-tauri/web-shared/dialog/dialog-shell.ts` — createDialogShell
- `src-tauri/web-shared/dialog/dialog-shell.css` — `.dialog-*` classes
- `src-tauri/web-shared/dialog/dialog-shell.test.ts` — happy-dom tests

### Files to Modify

- `doc/UI-DESIGN-GUIDELINES.yaml` — add `dialogs:` + `tokens.elevation`; remove known-issue
- `src-tauri/web-shared/styles.css` — new `:root` vars + `@import` for dialog-shell.css
- `src-tauri/web-shared/settings/ui-theme-presets.ts` — per-preset `errorContainer` / `onErrorContainer` + variable map
- `src-tauri/src/ui/md3.rs` — Palette fields + accessors
- `src-tauri/src/ui/mod.rs` — register `dialog` module under `#[cfg(feature = "gui")]`
- `src-tauri/src/ui/mux_dialogs.rs` — rewrite rename + move
- `src-tauri/src/render/mod.rs` — rewrite sftp upload / overwrite / close-guard
- `src-tauri/src/ui/profile_selector.rs` — adopt shared tokens
- `src-tauri/web-shared/profile/profile-editor.ts` — migrate to helper
- `src-tauri/web-shared/ssh/ssh-editor.ts` — migrate to helper
- `src-tauri/web-shared/styles/settings-panel.css` — remove `.profile-editor-*` blocks
- (no Cargo.toml change: existing `serde_yml = "0.0.11"` is reused by the drift test)

## SPEC.md Compliance

### Success Criteria

(From 要件定義書.md §10)

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1  | `UI-DESIGN-GUIDELINES.yaml` has `dialogs:` + `tokens.elevation` | Manual diff inspection + TS-9 |
| SC-2  | `styles.css` defines `--md-sys-color-surface-variant`, `--md-sys-color-error-container`, `--md-sys-color-on-error-container`, `--md-sys-typescale-*` | Grep + TS-10 |
| SC-3  | `Palette` has new fields filled for all 10 presets | TS-20 + TS-23 |
| SC-4  | `crate::ui::dialog` exists with three factories | TS-11 + TS-12 + TS-13 |
| SC-5  | `createDialogShell` exists in `web-shared/dialog/dialog-shell.ts` | TS-14 through TS-18, TS-22 |
| SC-6  | All 8 dialogs go through helpers (or shared constants for profile_selector) | TS-1 through TS-8 + TS-26 |
| SC-7  | No "OK" label remains | TS-1, TS-3, TS-4, TS-5, TS-11 + manual code search |
| SC-8  | destructive-confirm initial focus is cancel | TS-4 + TS-5 + TS-12 |
| SC-9  | Drift detection unit test passes under `cargo test --lib` | TS-9 + TS-10 |
| SC-10 | `bun test` and `bun run typecheck` pass | Build verification step |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 (yaml + tokens SSOT) | Phase 1 | TS-9, TS-10, TS-20, TS-21 |
| FR2 (native helper) | Phase 2 | TS-11, TS-12, TS-13, TS-19 |
| FR3 (webview helper) | Phase 3 | TS-14, TS-15, TS-16, TS-17, TS-18, TS-22 |
| FR4 (refactor 8 dialogs) | Phase 4, 5, 7 | TS-1, TS-2, TS-3, TS-4, TS-5, TS-6, TS-7, TS-8, TS-26 |
| FR5 (keyboard rules) | Phase 2, 3 | TS-1, TS-2, TS-3, TS-4, TS-5, TS-12, TS-14, TS-15, TS-16, TS-18 |
| FR6 (color rules) | Phase 1, 2 | TS-4, TS-20, TS-23 |
| FR7 (drift test) | Phase 6 | TS-9, TS-10, TS-21 |

### Non-Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| NFR1 (compatibility) | Phase 4, 5 | TS-1, TS-3, TS-24, manual SPEC §7.3 |
| NFR2 (CLI build) | Phase 1, 2, 6 | TS-19 |
| NFR3 (i18n) | Phase 2, 3 | TS-1 through TS-5 pass `(ja, en)` pairs into the native `Dialog` helper + `crate::i18n::Locale`; TS-6/TS-7 resolve their titles/labels through `t()` keys defined in `src-tauri/web-shared/i18n/locales/{en,ja}.json` (the WebView helper API takes a single resolved `title: string`). |
| NFR4 (workflow rules) | Phase 6 | Test command uses `CARGO_TARGET_DIR=src-tauri/target` + `--manifest-path` per `.claude/rules/build-location.md` |

## E2E Testing

`sdd.yaml.project.components.*.e2e_test_command` is empty. There is
no project-defined E2E framework for this feature. Manual verification
covers UX-level checks; the drift test covers structural integrity.

## Manual Testing (E2E Not Possible)

Run via `make dev` and exercise each dialog. Items below cannot be
automated because they involve real egui focus / IME / scrim
interaction or are subjective visual confirmation.

- [ ] Rename window: Enter confirms, Esc cancels, label reads "変更"/"Rename"
- [ ] Move window: ArrowUp/Down still works, label reads "移動"/"Move"
- [ ] Upload confirm: Enter confirms, label reads "アップロード"/"Upload"
- [ ] Overwrite confirm: Enter cancels, initial focus on Cancel,
      Overwrite button visibly red/destructive
- [ ] Close-tab guard: Enter cancels, initial focus on Cancel
- [ ] Profile editor: Enter submits via primary; visual layout matches
      previous baseline
- [ ] SSH editor: same as profile editor
- [ ] Profile selector: Enter picks highlighted row; chrome matches
      shared dialog corner radius / shadow / scrim
- [ ] Switch theme between Purple-dark and Purple-light; verify
      dialog tokens resolve in both
- [ ] IME smoke (Japanese): start composing in rename / profile name,
      press Enter — composition commits without firing primary
      (WebView side); native side respects `lost_focus + Enter`

## Performance Verification

Not applicable. No performance budget is defined for dialogs beyond
the < 5KB minified bundle target noted in 要件定義 §NFR2 for the
WebView helper. Verify with the bundle output of
`bun run build:settings` (the dialog helper bundles into the same
output as the settings panel).

## Security Verification

- [ ] WebView shell sets `role="dialog"`, `aria-modal="true"`, and
      `aria-label`; no innerHTML interpolation of caller-supplied
      content (caller composes nodes through `appendChild`)
- [ ] Scrim does not swallow drag events that would break terminal
      input (overlay only mounted when a dialog is open)

## Verification Summary

| Category | Items | Automated (Unit) | E2E | Manual |
|----------|-------|------------------|-----|--------|
| Functional | 26 | 22 (TS-1–TS-26 with manual components for visual checks) | 0 | 10 |
| Non-Functional | 4 | TS-19 + build commands | 0 | 1 (theme switch) |
| Security | 2 | 1 (helper ARIA attribute assertion in TS-14 area) | 0 | 1 |
| Performance | 0 | 0 | 0 | 0 |
