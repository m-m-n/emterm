# Verification Document: Toggle Tab Bar Visibility

## Overview
**Feature**: Toggle Tab Bar Visibility
**SPEC.md**: `doc/tasks/toggle-tab-bar/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/toggle-tab-bar/IMPLEMENTATION.md`

## Build Verification

### Build Command
```bash
# TypeScript type check
bun run typecheck

# Rust build
cargo build --manifest-path src-tauri/Cargo.toml
```

### Expected Result
- Exit code: 0
- No error messages

## Test Verification

### Test Command
```bash
# TypeScript tests
bun test

# Rust tests
cargo test --manifest-path src-tauri/Cargo.toml
```

### Coverage Target
- **Minimum**: 80%
- **Target**: 90%

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `matchKeybindStr` matches `Ctrl+Shift+B` correctly | Returns true for matching event | Unit |
| TS-2 | `TabBarUI.setVisible(false)` adds `hidden` class | `.tab-bar` has `.hidden` class | Unit |
| TS-3 | `TabBarUI.setVisible(true)` removes `hidden` class | `.tab-bar` does not have `.hidden` class | Unit |
| TS-4 | Default `show_tab_bar` is `true` when missing | Settings returns `show_tab_bar: true` | Unit |
| TS-5 | Default `toggle_tab_bar` is `"Ctrl+Shift+B"` when missing | Keybinds returns correct default | Unit |
| TS-6 | Toggle keybind triggers visibility change | Tab bar visibility toggles | Integration |
| TS-7 | Toggle keybind triggers settings save | Settings file updated | Integration |
| TS-8 | App startup restores saved visibility state | Tab bar matches saved state | Integration |
| TS-9 | Settings file missing `show_tab_bar`: defaults to `true` | Tab bar visible on start | Edge Case |
| TS-10 | Rapid toggling: animation handles interruption | No visual glitches | Manual |
| TS-11 | Tab bar hidden + new tab: tab bar remains hidden | Tab created, bar still hidden | Edge Case |
| TS-12 | Tab bar hidden + settings tab opened: bar remains hidden | Settings tab opens, bar hidden | Edge Case |

## Code Quality Verification

### Format Check
```bash
# TypeScript (if biome configured)
bun run lint

# Rust
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
```

### Static Analysis
```bash
# Rust
cargo clippy --manifest-path src-tauri/Cargo.toml
```

## File Structure Verification

### Files to Modify
- `src/settings/types.ts` - Add `show_tab_bar: boolean` to AppSettings, `toggle_tab_bar: string` to KeybindSettings
- `src-tauri/src/commands/config.rs` - Add Rust fields with serde attributes and defaults
- `src/styles/tab-bar.css` - Add `.tab-bar.hidden` class and transition
- `src/tab-bar/tab-bar-ui.ts` - Add `setVisible()` and `isVisible()` methods
- `src/tab-bar/keyboard-handler.ts` - Add toggle keybind handler and callback
- `src/main.ts` - Apply initial visibility, wire toggle callback

### Files to Create
- None (only modifications to existing files)

## SPEC.md Compliance

### Success Criteria

| ID | Criterion from SPEC.md | How to Verify |
|----|------------------------|---------------|
| SC-1 | All functional requirements implemented and tested | Run test suite, verify all pass |
| SC-2 | `Ctrl+Shift+B` toggles tab bar visibility with smooth animation | Manual E2E test |
| SC-3 | Visibility state persists across app restarts | Restart app, verify state matches |
| SC-4 | Keybinding is configurable via settings | Change keybind in settings.json, verify new keybind works |
| SC-5 | Tab operations work regardless of tab bar visibility | Hide tab bar, use Ctrl+Shift+T to create tab |
| SC-6 | No regression in existing tab bar functionality | Run existing tests, manual smoke test |

### Functional Requirements Coverage

| Requirement | Implementation Phase | Verification |
|-------------|---------------------|--------------|
| FR1: Add `toggle_tab_bar` to KeybindSettings (default: "Ctrl+Shift+B") | Phase 1 | Unit test: check default value |
| FR2: Add `show_tab_bar` to AppSettings (default: true) | Phase 1 | Unit test: check default value |
| FR3: Handle keybinding in TabKeyboardHandler.handleKeyDown() | Phase 3 | Unit/integration test: keybind triggers callback |
| FR4: Toggle tab bar visibility via CSS class | Phase 2, 3 | Unit test: class toggle, Manual: visual check |
| FR5: Save state to settings on each toggle | Phase 3 | Integration test: settings file updated |
| FR6: Restore state from settings on app initialization | Phase 3 | Integration test: app starts with correct state |
| FR7: Tab operation keybindings work when tab bar hidden | Phase 3 | E2E test: hide bar, create new tab |

### User Story Coverage

| User Story | Acceptance Criteria | Verification |
|------------|---------------------|--------------|
| US1: Toggle with Keyboard Shortcut | Ctrl+Shift+B toggles, content expands/shrinks, configurable | E2E + Unit tests |
| US2: Persist Visibility | State saved on toggle, restored on startup, default true | Integration tests |
| US3: Smooth Animation | Slides up/down, MD3 Motion, 60fps | Manual visual inspection |

## E2E Testing (Docker)

Docker environment ref: `~/.claude/skills/docker-e2e-testing/SKILL.md`

### Setup
- Dockerfile: `Dockerfile.e2e`
- Compose: `docker-compose.e2e.yml`
- Run: `./scripts/run-e2e-docker.sh`

### Basic Functionality
- [ ] Press `Ctrl+Shift+B` → tab bar slides up and hides
- [ ] Press `Ctrl+Shift+B` again → tab bar slides down and shows
- [ ] Verify content area fills full height when tab bar hidden

### State Persistence
- [ ] Hide tab bar, close app
- [ ] Restart app → tab bar should still be hidden
- [ ] Show tab bar, close app
- [ ] Restart app → tab bar should be visible

### Edge Cases
- [ ] Hide tab bar, press `Ctrl+Shift+T` → new tab created, bar stays hidden
- [ ] Hide tab bar, click settings button (via keybind) → settings opens, bar stays hidden
- [ ] Tab bar hidden with multiple tabs → all tabs functional

### Configuration
- [ ] Edit settings.json to change `keybinds.toggle_tab_bar` to "Ctrl+Shift+H"
- [ ] Restart app → `Ctrl+Shift+H` should now toggle tab bar
- [ ] `Ctrl+Shift+B` should no longer work for toggle

## Manual Testing (E2E Not Possible)

Items that cannot be automated via Docker E2E:

- [ ] Animation smoothness: Tab bar slides at 60fps without jank
- [ ] Animation timing: Transition feels natural (not too fast/slow)
- [ ] No visual artifacts during animation (no flicker, jump, or content flash)
- [ ] Animation interruption: Rapidly pressing toggle doesn't cause glitches
- [ ] Focus behavior: Keyboard focus remains appropriate after toggle

## Performance Verification

### Animation Performance
- Requirement: 60fps during tab bar show/hide animation
- Verification method: Chrome DevTools Performance panel, observe frame rate during toggle
- Expected: No dropped frames, stable 16.6ms frame time

### Responsiveness
- Requirement: No perceptible delay on keybind press
- Verification method: User perception test
- Expected: Immediate response (<100ms to start animation)

## Security Verification

No security requirements for this feature (purely UI, no external data).

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Build | 2 | ✅ | - | - |
| Tests | 12 | ✅ (8) | ✅ (4) | - |
| Code Quality | 2 | ✅ | - | - |
| File Structure | 6 | ✅ | - | - |
| SPEC Compliance | 6 | Partial | ✅ | ✅ |
| E2E Testing | 9 | - | ✅ | - |
| Manual Testing | 5 | - | - | ✅ |
| Performance | 2 | - | - | ✅ |

**Total**: 12 automated unit/integration items, 13 E2E items, 7 manual items
