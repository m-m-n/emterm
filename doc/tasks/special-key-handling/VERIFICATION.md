# Verification Document: Comprehensive Special Key Mapping

## Overview

**Feature**: special-key-handling
**SPEC.md**: `doc/tasks/special-key-handling/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/special-key-handling/IMPLEMENTATION.md`

## Build Verification

- Command: `bun tauri build`
- Expected: exit code 0, no errors

## Test Verification

- Command (TypeScript): `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"`
- Command (Rust): `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"`
- Command (Typecheck): `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"`
- Coverage target: 90%+ for new keyboard mapping code

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-01 | Ctrl+[ with key="[" (WebKitGTK) | Produces 0x1B (ESC) | Unit |
| TS-02 | Ctrl+[ with key="Escape" (Chromium) | Produces 0x1B (ESC) | Unit |
| TS-03 | Ctrl+] | Produces 0x1D (GS) | Unit |
| TS-04 | Ctrl+\ | Produces 0x1C (FS) | Unit |
| TS-05 | Ctrl+^ | Produces 0x1E (RS) | Unit |
| TS-06 | Ctrl+_ | Produces 0x1F (US) | Unit |
| TS-07 | Ctrl+@ | Produces 0x00 (NUL) | Unit |
| TS-08 | Ctrl+Space | Produces 0x00 (NUL) | Unit |
| TS-09 | Shift+Tab | Produces ESC [ Z | Unit |
| TS-10 | Ctrl+ArrowUp | Produces ESC [1;5A | Unit |
| TS-11 | Ctrl+ArrowRight | Produces ESC [1;5C | Unit |
| TS-12 | Shift+ArrowUp | Produces ESC [1;2A | Unit |
| TS-13 | Alt+ArrowUp | Produces ESC [1;3A | Unit |
| TS-14 | Ctrl+Shift+ArrowRight | Produces ESC [1;6C | Unit |
| TS-15 | Ctrl+Home | Produces ESC [1;5H | Unit |
| TS-16 | Ctrl+End | Produces ESC [1;5F | Unit |
| TS-17 | Shift+Home | Produces ESC [1;2H | Unit |
| TS-18 | Ctrl+Delete | Produces ESC [3;5~ | Unit |
| TS-19 | Ctrl+PageUp | Produces ESC [5;5~ | Unit |
| TS-20 | Shift+Insert | Produces ESC [2;2~ | Unit |
| TS-21 | Shift+F1 | Produces ESC [1;2P | Unit |
| TS-22 | Ctrl+F5 | Produces ESC [15;5~ | Unit |
| TS-23 | Ctrl+F12 | Produces ESC [24;5~ | Unit |
| TS-24 | Ctrl+Alt+C | Produces ESC 0x03 | Unit |
| TS-25 | Ctrl+Alt+A | Produces ESC 0x01 | Unit |
| TS-26 | Modifier param: no modifiers | Returns 0 | Unit |
| TS-27 | Modifier param: Shift | Returns 2 | Unit |
| TS-28 | Modifier param: Alt | Returns 3 | Unit |
| TS-29 | Modifier param: Ctrl | Returns 5 | Unit |
| TS-30 | Modifier param: Ctrl+Shift | Returns 6 | Unit |
| TS-31 | Modifier param: Ctrl+Alt+Shift | Returns 8 | Unit |
| TS-32 | Ctrl+J with skk_mode=true | Blocked (not sent to PTY) | Unit |
| TS-33 | Ctrl+J with skk_mode=false | Produces 0x0A (LF) | Unit |

## Code Quality Verification

- Typecheck: `bun run typecheck`
- No new warnings or errors expected

## File Structure Verification

### Files to Modify

- `src/pty/keyboard.ts` - Ctrl+symbol handler, modifier param system, Ctrl+Alt handler
- `src/pty/keyboard.test.ts` - All new test cases (TS-01 through TS-33)
- `src/terminal-app/handlers/keyboard.ts` - Conditional Ctrl+J blocking
- `src/terminal-app/handlers/keyboard.test.ts` - Ctrl+J config tests
- `src/settings/types.ts` - Add `skk_mode` field
- `src/settings/settings-sections.ts` - Settings UI toggle
- `src-tauri/src/commands/config.rs` - Add `skk_mode` to AppSettings
- `src-tauri/locales/en.json` - i18n label
- `src-tauri/locales/ja.json` - i18n label
- `src/i18n/locales/en.json` - Frontend i18n label
- `src/i18n/locales/ja.json` - Frontend i18n label

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-01 | All functional requirements implemented and tested | All TS-01 through TS-33 pass |
| SC-02 | `Ctrl+z Ctrl+[` enters tmux copy-mode on Linux | Manual test on WebKitGTK |
| SC-03 | `Ctrl+Left/Right` performs word movement in bash/zsh | Manual test |
| SC-04 | `Shift+Tab` performs reverse tab completion | Manual test |
| SC-05 | Ctrl+J configurable via settings | Unit test + manual test |
| SC-06 | No regression in existing key handling | Existing tests still pass |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1: Ctrl+symbol control character mapping | Phase 1 | TS-01 through TS-08 |
| FR2: WebKitGTK compatibility | Phase 1 | TS-01 (key="["), TS-02 (key="Escape") |
| FR3: Shift+Tab back-tab | Phase 2 | TS-09 |
| FR4: Modified arrow keys | Phase 2 | TS-10 through TS-14 |
| FR5: Modified Home/End | Phase 2 | TS-15 through TS-17 |
| FR6: Modified Delete/Insert/PageUp/PageDown | Phase 2 | TS-18 through TS-20 |
| FR7: Modified F1-F4 | Phase 2 | TS-21 |
| FR8: Modified F5-F12 | Phase 2 | TS-22, TS-23 |
| FR9: Ctrl+Alt+letter | Phase 3 | TS-24, TS-25 |
| FR10: Ctrl+J configurable | Phase 3 | TS-32, TS-33 |
| FR11: Existing keybinds priority | All | Existing tests pass, keybind checks run before keyEventToBytes |
| NFR1: No latency increase | All | No new async operations in key path |
| NFR2: WebKitGTK + Chromium | Phase 1 | TS-01, TS-02 cover both |
| NFR3: Backward compatibility | All | All existing tests pass |

## Manual Testing

- [ ] `Ctrl+z Ctrl+[` enters tmux copy-mode on Linux (emterm with WebKitGTK)
- [ ] `Ctrl+Left/Right` moves cursor by word in bash/zsh
- [ ] `Shift+Tab` performs reverse tab completion in bash/zsh
- [ ] `Ctrl+]` works in telnet or vim ctag navigation
- [ ] `Ctrl+\` sends SIGQUIT to foreground process
- [ ] `Ctrl+J` works as fzf accept when skk_mode is disabled
- [ ] Settings UI shows SKK mode toggle in Terminal section
- [ ] Ctrl+Shift+C/V (copy/paste) still works
- [ ] Ctrl+Shift+ArrowUp/Down (prompt jump) still works

## Verification Summary

| Category | Items | Automated | Manual |
|----------|-------|-----------|--------|
| Ctrl+symbol keys | 8 | 8 (TS-01~08) | 3 |
| Shift+Tab | 1 | 1 (TS-09) | 1 |
| Modified special keys | 14 | 14 (TS-10~23) | 1 |
| Modifier calculator | 6 | 6 (TS-26~31) | 0 |
| Ctrl+Alt | 2 | 2 (TS-24~25) | 0 |
| Ctrl+J config | 2 | 2 (TS-32~33) | 1 |
| Regression | - | Existing suite | 2 |
| **Total** | **33** | **33** | **8** |
