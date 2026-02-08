# Verification Document: Command Output Folding

## Overview

**Feature**: Command Output Folding
**SPEC.md**: `doc/tasks/output-folding/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/output-folding/IMPLEMENTATION.md`

## Build Verification

### Build Command
```bash
# TypeScript type check
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"

# Rust build
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml --no-run"
```

### Expected Result
- Exit code: 0
- No error messages

## Test Verification

### Test Command
```bash
# TypeScript unit tests
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"

# Rust unit tests
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"
```

### Coverage Target
- **Minimum**: 80%
- **Target**: 90% (for FoldManager core)

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Register OSC 133 region and retrieve by line index | Region found with correct properties | Unit |
| TS-2 | Register custom OSC region and retrieve by line index | Region found with label, no exitCode | Unit |
| TS-3 | Toggle fold state (collapse and expand) | collapsed flips on each toggle | Unit |
| TS-4 | Calculate fold offset with multiple collapsed regions | Cumulative offset correct | Unit |
| TS-5 | Display line to actual line mapping | Correct conversion accounting for folds | Unit |
| TS-6 | Actual line to display line mapping | Reverse of display-to-actual | Unit |
| TS-7 | Prune regions when scrollback is trimmed | Old regions removed, indices adjusted | Unit |
| TS-8 | Unfold all regions | All regions set collapsed=false | Unit |
| TS-9 | Disabled state prevents toggle | toggleFold returns false | Unit |
| TS-10 | Region with 0 lines is not registered | Registration rejected | Unit |
| TS-11 | Region with 1 line is registered | Registration accepted | Unit |
| TS-12 | fold;begin;{label} creates pending fold | Pending state stored | Unit |
| TS-13 | fold;end completes pending fold | Region registered in FoldManager | Unit |
| TS-14 | fold;end without begin is ignored | No error, no region created | Unit |
| TS-15 | Consecutive begin invalidates previous begin | Only latest begin used | Unit |
| TS-16 | OSC 133 C→D region becomes foldable after D marker | Region auto-registered | Integration |
| TS-17 | C marker without matching D | Region NOT foldable | Unit |
| TS-18 | D marker without preceding C | Ignored | Unit |
| TS-19 | Very long command text | Truncated in summary | Unit |
| TS-20 | Very long label | Truncated in summary | Unit |
| TS-21 | Multiple fold regions adjacent | Each independently foldable | Unit |
| TS-22 | Scrollback pruning removes fold region | Region removed from FoldManager | Unit |
| TS-23 | Alternate buffer: fold markers preserved | Primary buffer folds intact | Integration |

## Code Quality Verification

### TypeScript Type Check
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

### Rust Checks
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"
```

## File Structure Verification

### Files to Create
- `src/terminal/fold-manager.ts` - FoldManager class with region management and line mapping
- `src/terminal/fold-manager.test.ts` - Comprehensive unit tests for FoldManager

### Files to Modify
- `src/terminal/handlers/osc_handlers.ts` - Add fold verb routing in handleEmtermExtension
- `src/terminal/handlers/types.ts` - Add getFoldManager() to TerminalStateAccessor
- `src/terminal/state.ts` - Instantiate FoldManager, expose accessor, wire pruning
- `src/terminal/canvas-renderer.ts` - Fold-aware getVisibleLines, summary line rendering
- `src/terminal-app/index.ts` - Click handler, search/prompt integration
- `src/settings/types.ts` - Add fold_enabled to AppSettings
- `src/settings/settings-sections.ts` - Add fold toggle in Terminal section
- `src/settings/settings-applier.ts` - Apply fold_enabled to FoldManager
- `src/i18n/locales/en.json` - Add fold setting translation
- `src/i18n/locales/ja.json` - Add fold setting translation
- `src-tauri/src/commands/config.rs` - Add fold_enabled to AppSettings struct
- `src-tauri/locales/en.json` - Add fold setting validation key
- `src-tauri/locales/ja.json` - Add fold setting validation key

## SPEC.md Compliance

### Success Criteria

| ID | Criterion from SPEC.md | How to Verify |
|----|------------------------|---------------|
| SC-1 | All functional requirements implemented and tested | Run test suite, check all FR items |
| SC-2 | All unit test scenarios pass | `bun test` exits 0, all TS-* pass |
| SC-3 | Performance meets goals (< 16ms fold toggle) | Manual timing or console.time measurement |
| SC-4 | OSC 133 fold regions correctly identified | Test with bash/zsh that emits OSC 133 |
| SC-5 | Custom OSC fold regions work with emterm CLI | Test with `emterm fold` CLI command |
| SC-6 | Summary line visually clear and informative | Visual inspection |
| SC-7 | No regressions in search, prompt jump, general operation | Run full test suite, manual verification |
| SC-8 | Setting toggle works correctly | Toggle in settings UI, verify behavior |

### Functional Requirements Coverage

| Requirement | Implementation Phase | Verification |
|-------------|---------------------|--------------|
| FR1: Identify OSC 133 C→D regions | Phase 2 | TS-16, TS-17 |
| FR2: Identify custom OSC fold regions | Phase 2 | TS-12, TS-13, TS-14, TS-15 |
| FR3: Render summary line on Canvas | Phase 3 | Manual visual check |
| FR4: Toggle fold via click | Phase 4 | Manual click test |
| FR5: Maintain fold state per session | Phase 1 | TS-3, tab switch test |
| FR6: Adjust scroll offset on fold/unfold | Phase 3 | Manual scroll stability test |
| FR7: Prune fold state on scrollback discard | Phase 1 | TS-7, TS-22 |
| FR8: Extract command text from B marker | Phase 2 | TS-6 (integration) |
| FR9: Parse OSC 777;emterm;fold in Rust | Phase 2 | Existing EmtermExtension routing |
| FR10: Global enable/disable setting | Phase 5 | TS-9, Settings UI test |

## E2E Testing (Docker)

### Setup
- Docker compose: `docker-compose.e2e.yml`
- Run: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"`

### Basic Functionality
- [ ] FoldManager unit tests all pass
- [ ] OSC handler fold routing tests pass
- [ ] TypeScript type check passes
- [ ] Rust tests pass (settings deserialization)

### Edge Cases
- [ ] Region with 0 lines rejected
- [ ] Region with 1 line accepted
- [ ] Prune handles partial overlap
- [ ] Disabled FoldManager rejects toggles
- [ ] Consecutive begin discards previous
- [ ] Orphaned end ignored silently

### Error Handling
- [ ] Empty fold label uses fallback
- [ ] Long label truncated (no crash)
- [ ] Alternate buffer markers ignored for folding

## Manual Testing (E2E Not Possible)

Items requiring visual verification or live terminal interaction:

- [ ] Summary line renders correctly (semi-transparent bar, ▶ icon, text)
- [ ] Exit code 0: dim/normal text color
- [ ] Non-zero exit code: red text color
- [ ] Click on summary line expands the fold
- [ ] Click on output zone header collapses the fold
- [ ] Mouse hover shows pointer cursor on foldable areas
- [ ] Scroll position stable on fold/unfold (no viewport jump)
- [ ] Search match inside folded region auto-expands the region
- [ ] Prompt jump into folded region auto-expands the region
- [ ] Ctrl+click on URL in fold area opens URL (not toggle fold)
- [ ] Text selection in fold area works (not toggle fold)
- [ ] Tab switch preserves fold state
- [ ] Settings toggle: ON/OFF works, OFF unfolds all
- [ ] Multiple adjacent fold regions fold/unfold independently
- [ ] Custom OSC fold regions display label instead of command name
- [ ] Custom OSC fold regions don't show exit code

## Performance Verification

### Benchmarks
- **Fold/unfold toggle**: < 16ms per operation
  - Measure with `performance.now()` around toggle + re-render
- **100+ fold regions**: Render within frame budget (< 16ms)
  - Create 100 fold regions, measure forceRender time
- **Line mapping with 50 collapsed regions**: < 1ms
  - Benchmark displayLineToActual with many folds

## Security Verification

### Security Checks
- [ ] Fold labels rendered only on Canvas (no DOM injection)
- [ ] Long label strings truncated at display time
- [ ] No user input directly rendered to DOM
- [ ] OSC input validation consistent with existing EmtermExtension pattern

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Build | 2 | ✅ | - | - |
| Unit Tests | 23 | ✅ | - | - |
| Code Quality | 2 | ✅ | - | - |
| File Structure | 15 | ✅ | - | - |
| SPEC Compliance | 8 | Partial | - | ✅ |
| E2E Testing (Docker) | 9 | - | ✅ | - |
| Manual Testing | 16 | - | - | ✅ |
| Performance | 3 | - | - | ✅ |
| Security | 4 | - | - | ✅ |

**Total**: 27 automated items, 9 E2E items, 23 manual items
