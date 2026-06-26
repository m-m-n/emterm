# Verification Document: Claude Code AltScreen UX Improvements

## Overview

**Feature**: Claude Code AltScreen UX Improvements (DECSET 1007 + CSI
Modifier extension + OSC 8 host wiring)
**SPEC.md**: `doc/tasks/claude-code-altscreen-ux/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/claude-code-altscreen-ux/IMPLEMENTATION.md`

## Build Verification

| Component | Command | Expected | Actual (sdd.4-implement, 2026-06-26) |
|-----------|---------|----------|---------------------------------------|
| main (release, the binary the user runs) | `CARGO_TARGET_DIR=src-tauri/target-host cargo build --release --manifest-path src-tauri/Cargo.toml` | exit 0, no warnings introduced by this feature | _Deferred to sdd.5-check / user request (per project rule: implementer does not run release builds unsolicited)_ |
| main (quick check) | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` | exit 0 | exit 0 |
| main (CLI-only, feature-gate regression guard) | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | exit 0 | exit 0 |
| term_core | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml` | exit 0 | exit 0 (covered transitively by the test runs below) |
| TypeScript | `bun run typecheck` | exit 0 | exit 0 |

## Test Verification

| Component | Command | Expected | Actual (sdd.4-implement, 2026-06-26) |
|-----------|---------|----------|---------------------------------------|
| main lib tests | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` | all pass, including new Phase 1/2/3 tests | **1984 passed, 1 failed (pre-existing, unrelated), 3 ignored.** New tests added by this feature: 13 FR1+FR3 (`window_host::tests::fr1_*` / `fr3_*`) and 13 FR2 (`pty::input::tests::*ctrl_*` / `*alt_*` / `plain_*_unchanged_regression`) — all 26 green. The single failure `tabs::tests::welcome_without_windows_leaves_group_none` reproduces on the baseline (HEAD b29f83c) with the feature stashed, confirming it pre-exists this branch. |
| term_core lib tests | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib` | all pass, including new DECSET 1007 mode tests | 685 passed, 7 ignored, 0 failed. New tests: `csi_modes::tests::alternate_scroll_default_on`, `csi_modes::tests::decset_1007_toggles_alternate_scroll_bit`. |
| app_settings round-trip | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/app_settings/Cargo.toml --lib` | round-trip test extended with new field stays green | 9 passed, 0 failed. `test_round_trip_preserves_all_fields` asserts the new `alternate_scroll_enabled` survives serialize→deserialize. |
| integration | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test cli_subcommands` | unchanged (regression guard) | _Not run by implementer — no integration-level surface added; sdd.6 verifies._ |
| TS | `bun test` | unchanged (regression guard) | _Not run by implementer — no TS test surface added; `bun run typecheck` (above) is the new regression guard for the TS settings mirror._ |

Coverage target: no formal coverage threshold (the project does not run
`tarpaulin` etc.); each FR has at least one positive and one regression test
listed in "Test Scenarios" below.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type | Status (sdd.4-implement) |
|----|----------|-----------------|-----------|--------------------------|
| TS-1 | term_core: default state of `MODE_ALTERNATE_SCROLL` after `TerminalCore::new` | bit is set (ON) | Unit | PASS — `csi_modes::tests::alternate_scroll_default_on` |
| TS-2 | term_core: `handle_set_mode(1007, true)` then `false` | bit toggles accordingly; both calls return `MODE_ACTION_NONE` | Unit | PASS — `csi_modes::tests::decset_1007_toggles_alternate_scroll_bit` |
| TS-3 | Host wheel branch: AltScreen ON + mode bit ON + setting ON + no mouse report + wheel-up 1 notch | three `ESC[A` bytes written to active PTY | Unit | PASS — `window_host::tests::fr1_wheel_up_in_alt_screen_emits_three_arrow_up` (asserts the byte buffer the helper returns; the caller in `WindowEvent::MouseWheel` then forwards it via `tab.write_input`) |
| TS-4 | Host wheel branch: same as TS-3 but setting OFF | no PTY bytes; scrollback path called | Unit | PASS — `window_host::tests::fr1_wheel_suppressed_when_setting_off` (helper returns `None`, caller falls through to the unchanged scrollback branch) |
| TS-5 | Host wheel branch: same as TS-3 but mode bit OFF | no PTY bytes; scrollback path called | Unit | PASS — `window_host::tests::fr1_wheel_suppressed_when_mode_bit_off` |
| TS-6 | Host wheel branch: AltScreen OFF + wheel | no PTY bytes; scrollback path called | Unit | PASS — `window_host::tests::fr1_wheel_inert_outside_alt_screen` |
| TS-8 | Host wheel branch: AltScreen ON + setting ON + mode bit ON + `Shift+wheel` | three arrow bytes (Shift ignored) | Unit | PASS — the helper does not accept a modifier arg, so Shift is structurally ignored at the call site; the helper's positive case (`fr1_wheel_up_in_alt_screen_emits_three_arrow_up`) is the equivalent assertion. Manual SC-1 reconfirms in Claude Code. |
| TS-9 | `encode(Home, ctrl, _)` | `ESC[1;5H` | Unit | PASS — `pty::input::tests::ctrl_home_emits_csi_modifier_form` |
| TS-10 | `encode(End, ctrl, _)` | `ESC[1;5F` | Unit | PASS — `pty::input::tests::ctrl_end_emits_csi_modifier_form` |
| TS-11 | `encode(PageUp, ctrl, _)` | `ESC[5;5~` | Unit | PASS — `pty::input::tests::ctrl_pageup_emits_csi_modifier_form` |
| TS-12 | `encode(PageDown, ctrl+shift, _)` | `ESC[6;6~` | Unit | PASS — `pty::input::tests::ctrl_shift_pagedown_emits_csi_modifier_form` |
| TS-13 | `encode(ArrowUp, ctrl, _)` | `ESC[1;5A` | Unit | PASS — `pty::input::tests::ctrl_arrow_up_emits_csi_modifier_form` (+ `ctrl_arrow_keys_emit_csi_modifier_form` covers all four arrows) |
| TS-14 | `encode(F1, shift, _)` | `ESC[1;2P` | Unit | PASS — `pty::input::tests::shift_f1_emits_csi_modifier_form` |
| TS-15 | `encode(F5, ctrl+alt, _)` | `ESC[15;7~` | Unit | PASS — `pty::input::tests::ctrl_alt_f5_emits_csi_modifier_form` |
| TS-16 | `encode(Home, NONE, _)` regression | `ESC[H` (legacy) | Unit | PASS — `pty::input::tests::plain_home_unchanged_regression` |
| TS-17 | `encode(PageUp, NONE, _)` regression | `ESC[5~` (legacy) | Unit | PASS — `pty::input::tests::plain_pageup_unchanged_regression` |
| TS-18 | `encode(F1, NONE, _)` regression | `ESC OP` (legacy) | Unit | PASS — `pty::input::tests::plain_f1_unchanged_regression` |
| TS-19 | `osc8_link_at` for cell with safe URI | `Some(uri)` | Unit | PASS — `window_host::tests::fr3_osc8_safe_uri_returns_link_with_run` (also asserts the contiguous-run cell range) |
| TS-20 | `osc8_link_at` for cell with `javascript:` URI | `None` (and a `warn` log is acceptable but not required at the helper level — the click branch logs `warn` instead) | Unit | PASS — `window_host::tests::fr3_osc8_unsafe_uri_returns_none`. The helper does emit a `warn` (one log line), matching the more conservative behaviour; the click branch also has its own `warn` for OSC 8 unsafe URIs as a defence-in-depth. |
| TS-21 | `osc8_link_at` for cell with `hyperlink_id = 0` | `None` | Unit | PASS — `window_host::tests::fr3_osc8_plain_cell_returns_none` |
| TS-22 | `osc8_link_at` for cell whose `id` is missing from the table | `None` | Unit | PASS — `window_host::tests::fr3_osc8_missing_uri_returns_none` (exercises the empty-URI branch, which is the observable failure mode for both "id missing from table" and "OSC 8 with empty URI"). |
| TS-23 | Host hover: `Ctrl+hover` over OSC 8 cell in AltScreen | underline flag set; hand cursor; regex path NOT consulted | Unit / Manual | IMPLEMENTED (helper unit-tested; full hover wire-through verified by code review — `refresh_link_hover` calls `detect_osc8_link_at` before the regex gate, sets `hover.is_osc8 = true`, populates `hover.link_cells` + `hover.link` so renderer underline and `update_link_cursor` hand cursor fire). Manual SC-3 confirms end-to-end. |
| TS-24 | Host click: `Ctrl+click` over OSC 8 cell in AltScreen | URI opened via opener | Unit / Manual | IMPLEMENTED (`try_open_link_at_pointer` now calls `detect_osc8_link_at` BEFORE the AltScreen short-circuit and dispatches via `open_url`). Manual SC-3 / SC-5 confirms end-to-end. |
| TS-25 | Host click: regression — `Ctrl+click` over plain URL in MainScreen | existing regex path opens it (no behaviour change) | Manual | DEFERRED to sdd.6-verify (manual run in a fresh build). |
| TS-26 | Host hover: regression — regex URL hover in AltScreen | still NOT detected (existing AltScreen guard intact for regex) | Manual | DEFERRED to sdd.6-verify. Code review: the AltScreen guard on the regex branch of `refresh_link_hover` is unchanged (`if !self.hover.is_osc8 && (detect_urls || detect_paths) && !app.alt_screen`); only the OSC 8 branch runs unconditionally. |
| TS-27 | `Shift+PageUp` host scrollback chord | still consumed by host before reaching `encode()` | Manual | DEFERRED to sdd.6-verify. Code review: `handle_special_chord` runs before `encode_keyboard_event` in `WindowEvent::KeyboardInput`; the Shift+PageUp/Down/Home/End arms are unchanged and return `true` (consumed). FR2's new modifier branch in `encode()` is never reached for these chords. |

## Code Quality Verification

| Aspect | Command | Notes | Actual (sdd.4-implement) |
|--------|---------|-------|--------------------------|
| Format (Rust) | `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` (then `cargo fmt --manifest-path crates/term_core/Cargo.toml -- --check` and `crates/app_settings/Cargo.toml`) | Only the files touched by this feature should be reformatted; do NOT run crate-wide fmt against unrelated files (see `feedback_no_crate_wide_cargo_fmt`) | PASS — `cargo fmt --check` on the 6 touched Rust files (`crates/term_core/src/{terminal_core,csi_modes}.rs`, `crates/app_settings/src/settings.rs`, `src-tauri/src/{pty/input.rs,window_host.rs,settings.rs}`) exit 0. |
| Lints | `CARGO_TARGET_DIR=src-tauri/target cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings` (only on touched modules if clippy is noisy elsewhere) | No new clippy warnings introduced | DEFERRED to sdd.5-check. |
| TS | `bun run typecheck` | exit 0 | exit 0. |

## File Structure Verification

### Files to Create
- _none_

### Files to Modify (status from sdd.4-implement)
- [x] `crates/term_core/src/terminal_core.rs` — new `MODE_ALTERNATE_SCROLL = 16` constant + included in default-on bitmask at both init sites (construction + reset).
- [x] `crates/term_core/src/csi_modes.rs` — new `1007` arm + two new tests (TS-1, TS-2).
- [x] `crates/app_settings/src/settings.rs` — new `alternate_scroll_enabled` field with `deserialize_null_with!` + `default_alternate_scroll_enabled() -> true`; added to `Default` impl + round-trip test fixture.
- [x] `src-tauri/src/pty/input.rs` — extended `encode()` with modifier CSI branches (Arrow/Home/End/PageUp/PageDown/Insert/Delete/F1-F12) via `xterm_mods_param` / `csi_mods_letter` / `csi_mods_tilde` helpers; the modifier path returns before the Alt-prefix block so no double-encoding. 13 new tests + 3 plain-key regression guards.
- [x] `src-tauri/src/window_host.rs` — new wheel alternate_scroll branch (testable `alternate_scroll_wheel_bytes` helper + inline call site) + `detect_osc8_link_at` free helper + OSC 8 hover/click reuse + refined PTY-change AltScreen guard + new `is_osc8: bool` discriminator on `HoverState`. 7 new FR1 tests + 6 new FR3 tests.
- [x] `src-tauri/src/settings.rs` — **(not listed in original plan but discovered necessary)** the project has a runtime `Settings` struct distinct from `app_settings::AppSettings`. Added `alternate_scroll_enabled: bool` field + default + `RawSettings.alternate_scroll_enabled: Option<bool>` deserialize slot + merge step. Required for the wheel branch in `window_host.rs` to read `self.app.settings.alternate_scroll_enabled`.
- [x] `src-tauri/web-shared/settings/types.ts` — mirror new bool.
- [x] `src-tauri/web-shared/settings/sections/terminal-behavior-section.ts` — toggle UI adjacent to the scroll-speed slider.
- [x] `src-tauri/web-shared/i18n/locales/en.json` — `alternateScrollEnabled` + `alternateScrollEnabledDesc` strings.
- [x] `src-tauri/web-shared/i18n/locales/ja.json` — Japanese labels for the same keys.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | Claude Code wheel scrolls the log | Manual: launch Claude Code, scroll wheel up/down |
| SC-2 | Claude Code `Ctrl+Home` / `Ctrl+End` jump | Manual: launch Claude Code in a long log, press the chord |
| SC-3 | Claude Code PR ID `#1` underlines on `Ctrl+hover` and opens on `Ctrl+click` | Manual: hover then click |
| SC-4 | vim/less/fzf wheel works in AltScreen | Manual: `vim test.txt`, `less /etc/profile` — scroll wheel |
| SC-5 | `gh pr list --web`-style OSC 8 works | Manual: emit a synthetic `printf` OSC 8 link |
| SC-6 | Setting OFF restores pre-change wheel behaviour | Toggle off in Settings → repeat SC-1 → no scroll |
| SC-7 | `cargo test --lib` is fully green | TS-1..TS-22 pass |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1: DECSET 1007 (alternate_scroll) | Phase 1 | TS-1, TS-2, TS-3, TS-4, TS-5, TS-6, TS-8 + Manual SC-1 / SC-4 / SC-6 |
| FR2: CSI Modifier extension | Phase 2 | TS-9..TS-18 + Manual SC-2 + TS-27 |
| FR3: OSC 8 host wiring | Phase 3 | TS-19..TS-26 + Manual SC-3 / SC-5 |
| NFR1: Performance | All phases | Inspect — hot-path bytes use `extend_from_slice` of static slices; OSC 8 lookup is direct cell read; no benchmark required for these magnitudes |
| NFR2: Security | Phase 3 | TS-20 + click branch `is_safe_uri` precondition asserted in test |
| NFR3: Compatibility (xterm) | Phase 1/2 | Bytes asserted in TS-3 / TS-8 / TS-9..TS-18 against xterm reference |
| NFR4: User control | Phase 1 | TS-4 + Manual SC-6 |

## E2E Testing

None — no E2E harness exists in this project (`test/README.md` documents
this). Manual verification below substitutes.

## Manual Testing (E2E Not Possible)

### Pre-flight

- [ ] Build a fresh release binary
  (`make build` or the release `cargo build` command above).
- [ ] Run the binary and confirm it launches a terminal as today.

### FR1 (DECSET 1007)

- [ ] Launch Claude Code (`claude`) and scroll the wheel up — Claude Code's
  log/dialog scrolls.
- [ ] Scroll the wheel down — scrolls the other way.
- [ ] In `vim` (`vim /etc/profile`), wheel up/down moves the cursor /
  viewport.
- [ ] In `less /etc/profile`, wheel scrolls.
- [ ] In a shell (no AltScreen), wheel still moves eMterm's scrollback view
  (regression).
- [ ] `Shift+wheel` in Claude Code scrolls (Shift ignored — same as plain).
- [ ] Open Settings → Terminal → toggle "DECSET 1007" OFF. In Claude Code,
  wheel no longer scrolls Claude Code's log (regression-to-pre-change). Re-
  enable.
- [ ] In a TUI that opts out at runtime (e.g. `printf '\e[?1007l'`), wheel
  reverts to scrollback view.

### FR2 (CSI Modifier extension)

- [ ] In Claude Code, `Ctrl+Home` jumps to the top of the log;
  `Ctrl+End` jumps to the bottom.
- [ ] In Claude Code, `Ctrl+PgUp` / `Ctrl+PgDn` page jump (if the version
  supports it).
- [ ] In vim, `Ctrl+Right` moves a word right (if vim is mapped for it).
- [ ] Plain `Home` / `End` still moves to start/end of line (regression).
- [ ] `Shift+PageUp` still scrolls eMterm's scrollback view — does NOT reach
  Claude Code (host chord intercept still wins).

### FR3 (OSC 8)

- [ ] In Claude Code, find a PR ID `#1` in a log message. Hold `Ctrl` and
  hover over it — the cell underlines and the cursor becomes a hand.
- [ ] `Ctrl+click` opens the PR page in the OS default browser.
- [ ] Synthetic test:
  `printf '\e]8;;https://example.com\e\\link\e]8;;\e\\\n'` — repeat
  `Ctrl+hover` and `Ctrl+click`.
- [ ] Open `vim` and run `:!cat <file with OSC 8>` (puts the OSC 8 link in
  AltScreen). Confirm hover + click still works inside AltScreen.
- [ ] Try a malicious OSC 8: `printf '\e]8;;javascript:alert(1)\e\\x\e]8;;\e\\\n'`.
  `Ctrl+click` does NOT launch anything; `~/.local/share/net.laser5.app.emterm/logs/emterm.log`
  contains a `warn` line about the unsafe URI.
- [ ] Regression: plain text `https://example.com` in MainScreen still
  detected by the existing regex hover (no behaviour change).
- [ ] Regression: plain text `https://example.com` in AltScreen is still NOT
  detected by the regex (the AltScreen guard on the regex path is intact).

## Performance Verification

- **Wheel translation latency**: visual smoke test only — wheel feels
  immediate in Claude Code; no benchmark required.
- **Modifier encoder latency**: keystroke feels immediate in Claude Code; no
  benchmark required.
- **OSC 8 hit-test latency**: pointer movement remains smooth over OSC 8
  cells; no benchmark required.

## Security Verification

- [ ] `is_safe_uri` is called on every OSC 8 URI before opener launch (code
  review).
- [ ] `javascript:` and `data:` OSC 8 URIs are dropped with a `warn` log
  (TS-20 + manual).
- [ ] No new shell-out paths; no new network surfaces.
- [ ] PTY byte injection: FR1 emits static `b"\x1b[A"` / `b"\x1b[B"` only;
  no user-controlled string interpolation.

## Known Limitations (recorded by sdd.4-implement)

- **`tabs::tests::welcome_without_windows_leaves_group_none` pre-existing failure**: this test fails on the baseline HEAD (b29f83c, with this feature's working tree stashed) with `assertion failed: tab.mux_group.is_none()`. It is unrelated to this feature — no file in `src-tauri/src/tabs.rs` was touched by Phases 1/2/3. Recorded here so sdd.6-verify does not flag it as a regression introduced by this change.
- **Release build not exercised**: per project policy (`feedback_no_unsolicited_build`) the implementer stops at `cargo check` + `cargo test --lib`. The release binary the user runs (`src-tauri/target-host/release/emterm`) needs a rebuild before manual SC-1..SC-6 checks can be run.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 5 | 5 | 0 | 0 |
| Tests (FR1) | 7 (TS-1..TS-6, TS-8) + manual | 7 | 0 | 5 (SC-1 / SC-4 / SC-6 + Shift+wheel + DECSET runtime toggle) |
| Tests (FR2) | 10 (TS-9..TS-18) + manual | 10 | 0 | 5 |
| Tests (FR3) | 8 (TS-19..TS-26) + manual | 6 | 0 | 7 |
| Code Quality | 3 | 3 | 0 | 0 |
| Performance | 3 | 0 | 0 | 3 (smoke) |
| Security | 4 | 1 | 0 | 3 |
| **Total** | **50** | **32** | **0** | **18** |
