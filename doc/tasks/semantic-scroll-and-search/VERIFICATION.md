# Verification Document: Semantic Scroll / Prompt Jump & In-Terminal Text Search

## Overview
**Feature**: Semantic Scroll / Prompt Jump & In-Terminal Text Search
**SPEC.md**: `doc/tasks/semantic-scroll-and-search/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/semantic-scroll-and-search/IMPLEMENTATION.md`

## Build Verification

### Build Command
```bash
# Rust build
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo build --manifest-path src-tauri/Cargo.toml"

# TypeScript typecheck
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

### Expected Result
- Exit code: 0
- No error messages
- No type errors

## Test Verification

### Test Command
```bash
# Rust tests
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"

# TypeScript tests
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"
```

### Coverage Target
- **Minimum**: 80%
- **Target**: 90% for new modules (SemanticZoneTracker, SearchStateManager)

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-R1 | Parse OSC 133;A | SemanticPrompt { zone_type: "A", exit_code: None } | Rust unit |
| TS-R2 | Parse OSC 133;B | SemanticPrompt { zone_type: "B", exit_code: None } | Rust unit |
| TS-R3 | Parse OSC 133;C | SemanticPrompt { zone_type: "C", exit_code: None } | Rust unit |
| TS-R4 | Parse OSC 133;D;0 | SemanticPrompt { zone_type: "D", exit_code: Some(0) } | Rust unit |
| TS-R5 | Parse OSC 133;D without exit code | SemanticPrompt { zone_type: "D", exit_code: Some(0) } | Rust unit |
| TS-R6 | Unknown subcommand (e.g., 133;X) | Ignored (no action emitted or Unknown) | Rust unit |
| TS-R7 | SemanticPrompt JSON serialization | Valid JSON with zone_type and exit_code fields | Rust unit |
| TS-T1 | SemanticZoneTracker: add and retrieve markers | Markers stored in order | TS unit |
| TS-T2 | findPrevPrompt returns correct marker | Returns nearest "A" marker above given line | TS unit |
| TS-T3 | findNextPrompt returns correct marker | Returns nearest "A" marker below given line | TS unit |
| TS-T4 | findPrevPrompt returns null when none above | null returned | TS unit |
| TS-T5 | findNextPrompt returns null when none below | null returned | TS unit |
| TS-T6 | pruneBeforeLine removes old markers | Markers below threshold removed, indices adjusted | TS unit |
| TS-S1 | Plain text search finds matches | All occurrences found with correct positions | TS unit |
| TS-S2 | Case-insensitive search (default) | "Hello" matches "hello", "HELLO" | TS unit |
| TS-S3 | Case-sensitive search | "Hello" does not match "hello" | TS unit |
| TS-S4 | Regex search | Pattern matches correctly across lines | TS unit |
| TS-S5 | Invalid regex does not crash | Error state set, no matches returned | TS unit |
| TS-S6 | nextMatch wraps around | After last match, returns first match | TS unit |
| TS-S7 | prevMatch wraps around | Before first match, returns last match | TS unit |
| TS-S8 | getVisibleMatches returns correct range | Only matches within line range returned | TS unit |
| TS-S9 | Empty query returns no matches | Empty matches array, no error | TS unit |

## Integration Test Scenarios

| ID | Scenario | Expected Result | Verification |
|----|----------|-----------------|--------------|
| TS-I1 | OSC 133 sequence → marker recorded in zone tracker | Marker appears in SemanticZoneTracker with correct type and line | Manual (debug log) |
| TS-I2 | Prompt jump scrolls to correct position | Scroll offset updated to target prompt line | Manual |
| TS-I3 | Search highlights visible on canvas | Yellow/orange rectangles drawn over matching cells | Manual (visual) |
| TS-I4 | Search bar open/close lifecycle | Open → input → highlights → close → highlights cleared | Manual |

## Code Quality Verification

### Format Check
```bash
# TypeScript (no formatter configured, but typecheck covers type correctness)
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

### Static Analysis
```bash
# Rust
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings"
```

## File Structure Verification

### Files to Create
- `src/terminal/semantic-zone.ts` - SemanticZoneTracker class
- `src/terminal/semantic-zone.test.ts` - Zone tracker unit tests
- `src/terminal/search/search-state.ts` - SearchStateManager class
- `src/terminal/search/search-state.test.ts` - Search state unit tests
- `src/terminal/search/search-bar.ts` - Search bar DOM component
- `src/terminal/search/search-bar.css` - Search bar styles

### Files to Modify
- `src-tauri/src/ansi/sequence.rs` - Add SemanticPrompt variant to OscAction
- `src-tauri/src/ansi/parser.rs` - Add OSC 133 handling in dispatch_osc()
- `src/types/terminal.ts` - Add SemanticPrompt to OscAction type
- `src/terminal/handlers/osc_handlers.ts` - Add SemanticPrompt dispatch case
- `src/terminal/handlers/types.ts` - Add zone tracker to TerminalStateAccessor
- `src/terminal/state.ts` - Integrate SemanticZoneTracker, scrollback sync
- `src/terminal/canvas-renderer.ts` - Add search highlight rendering, setScrollOffset()
- `src/terminal-app/index.ts` - Wire search bar to TerminalApp
- `src/terminal-app/handlers/keyboard.ts` - Add prompt jump and search keybinds
- `src-tauri/src/commands/config.rs` - Add jump keybinds to define_keybinds!
- `src/settings/types.ts` - Add jump keybinds to KeybindSettings
- `src/i18n/locales/en.json` - Add keybind labels
- `src/i18n/locales/ja.json` - Add keybind labels

## SPEC.md Compliance

### Success Criteria

| ID | Criterion from SPEC.md | How to Verify |
|----|------------------------|---------------|
| SC-1 | All functional requirements implemented and tested | Run all unit tests (Rust + TS) |
| SC-2 | All unit test scenarios pass | `cargo test` + `bun test` exit 0 |
| SC-3 | Performance: search < 50ms on 10k lines | Manual benchmark or test with large buffer |
| SC-4 | OSC 133 correctly parsed from bash/zsh output | Manual test with real shell |
| SC-5 | Search bar UI is usable and responsive | Manual visual inspection |
| SC-6 | Keybinds configurable via settings | Verify settings UI and config file |
| SC-7 | No regressions in existing terminal functionality | Run full test suite, manual smoke test |

### Functional Requirements Coverage

| Requirement | Implementation Phase | Verification |
|-------------|---------------------|--------------|
| FR1: Rust parser recognizes OSC 133 | Phase 1 | Rust unit tests TS-R1 through TS-R7 |
| FR2: TypeScript records zone markers | Phase 1 | TS unit tests TS-T1 through TS-T6 |
| FR3: Zone markers stored with line indices | Phase 1 | TS unit test TS-T1 |
| FR4: Zone markers pruned on scrollback trim | Phase 1 | TS unit test TS-T6 |
| FR5: Prompt jump to nearest marker | Phase 2 | Manual test with shell output |
| FR6: Search over scrollback + screen | Phase 3 | TS unit tests TS-S1, TS-S8 |
| FR7: Plain text and regex modes | Phase 3 | TS unit tests TS-S1, TS-S4, TS-S5 |
| FR8: Case-insensitive and case-sensitive | Phase 3 | TS unit tests TS-S2, TS-S3 |
| FR9: Matches highlighted on canvas | Phase 4 | Manual visual inspection |
| FR10: Current match visually distinct | Phase 4 | Manual visual inspection |

## E2E Testing (Docker)

### Setup
Docker testing uses existing `docker-compose.e2e.yml`:
```bash
# Run all automated tests
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml && bun test && bun run typecheck"
```

### Basic Functionality (Automated)
- [ ] OSC 133 parsing: All Rust unit tests pass
- [ ] SemanticZoneTracker: All TS unit tests pass
- [ ] SearchStateManager: All TS unit tests pass
- [ ] TypeScript typecheck passes with no errors
- [ ] Rust build succeeds with no warnings

### Edge Cases (Automated)
- [ ] Empty SemanticZoneTracker returns null for find operations
- [ ] Search with empty query returns no matches
- [ ] Invalid regex pattern sets error state, no crash
- [ ] Wide characters (CJK) in search handled correctly
- [ ] Scrollback pruning removes markers correctly
- [ ] Alternate buffer active: OSC 133 markers are not recorded

## Manual Testing (E2E Not Possible)

Items requiring the actual Tauri app running with real shell interaction:

### Phase 1: OSC 133 Foundation
- [ ] bash/zsh emitting OSC 133 markers correctly recorded (debug log)

### Phase 2: Prompt Jump
- [ ] Ctrl+Shift+Up jumps to previous prompt in scrollback
- [ ] Ctrl+Shift+Down jumps to next prompt
- [ ] Prompt jump when no markers exist: nothing happens
- [ ] Prompt jump at first marker (Up): scrolls to top
- [ ] Prompt jump at last marker (Down): scrolls to bottom
- [ ] Keybinds customizable in settings panel

### Phase 4: Search UI
- [ ] Ctrl+Shift+F opens floating search bar at top-right
- [ ] Typing in search input highlights matches incrementally
- [ ] All matches shown with yellow-ish background on canvas
- [ ] Current match shown with orange-ish background (visually distinct)
- [ ] Hit count "N/M" displays correctly and updates
- [ ] Enter moves to next match
- [ ] Shift+Enter moves to previous match
- [ ] Wrap-around: last→first, first→last
- [ ] Off-screen match: scroll adjusts to show match
- [ ] Regex toggle button activates/deactivates regex mode
- [ ] Case sensitivity toggle button works
- [ ] Invalid regex shows error indicator in search bar
- [ ] Esc closes search bar and clears all highlights
- [ ] Close button (×) closes search bar
- [ ] Focus: search bar captures input while focused
- [ ] Focus: terminal input works when search bar loses focus
- [ ] Search bar re-open: focuses input, selects existing text

### Performance
- [ ] Prompt jump responds instantly (< 10ms perceived)
- [ ] Search on 10,000 line scrollback: no perceived lag (< 50ms)
- [ ] Highlight rendering: no frame drops during scrolling with active search

### Regression
- [ ] Terminal input (keyboard, IME) works normally
- [ ] Copy/paste works normally
- [ ] Tab switching works normally
- [ ] Scrollback scrolling (mouse wheel) works normally
- [ ] Selection works normally
- [ ] Existing keybinds work normally
- [ ] Settings panel opens and functions normally

## Performance Verification

### Benchmarks
- **Prompt jump**: < 10ms for navigation with ~1000 markers
  - Verify: Use `performance.now()` around jump operation in debug mode
- **Search execution**: < 50ms for 10,000 lines
  - Verify: Use `performance.now()` around `executeSearch()` with large buffer
- **Highlight rendering**: < 16ms per frame
  - Verify: Check frame timing in browser dev tools during search scroll

## Security Verification

### Security Checks
- [ ] Regex execution does not hang on pathological patterns (ReDoS protection)
- [ ] Search input is not used in any DOM innerHTML operations
- [ ] OSC 133 data does not inject into DOM (canvas rendering only)
- [ ] No XSS vectors in search bar UI (text input → value property only)

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Build | 2 | ✅ | - | - |
| Rust Tests | 7 | ✅ | - | - |
| TS Tests | 15 | ✅ | - | - |
| Integration Tests | 4 | - | - | ✅ |
| Code Quality | 2 | ✅ | - | - |
| File Structure | 19 | ✅ | - | - |
| SPEC Compliance | 7 | Partial | - | ✅ |
| E2E (Docker) | 11 | - | ✅ | - |
| Manual Testing | 26 | - | - | ✅ |
| Performance | 3 | - | - | ✅ |
| Security | 4 | - | - | ✅ |

**Total**: 46 automated items, 11 E2E items, 37 manual items
