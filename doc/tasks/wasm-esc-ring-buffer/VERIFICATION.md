# Verification Document: WASM ESC Handlers + Ring Buffer Integration (Sprint 5)

## Overview

**Feature**: WASM ESC Handlers + Ring Buffer Integration (Sprint 5)
**SPEC.md**: `doc/tasks/wasm-esc-ring-buffer/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/wasm-esc-ring-buffer/IMPLEMENTATION.md`

## Build Verification

### WASM Build
```bash
cd wasm && wasm-pack build --target web --out-dir pkg
```

### Expected Result
- Exit code: 0
- No error messages
- `.wasm` file < 70KB

### Rust Build
```bash
cargo test --manifest-path src-tauri/Cargo.toml --no-run
```

### TypeScript Build
```bash
bun run typecheck
```

## Test Verification

### Rust Test Command
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"
```

### TypeScript Test Command
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"
```

### Coverage Target
- **Minimum**: 80%
- **Target**: 90% (Ring Buffer, ESC handlers, reflow)

### Test Scenarios from SPEC.md

#### ESC Handlers (FR1-FR9)

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-01 | handle_esc SaveCursor saves position and attributes | Cursor state stored in saved_cursor | Unit (Rust) |
| TS-02 | handle_esc RestoreCursor restores saved state | Cursor position + attrs restored | Unit (Rust) |
| TS-03 | handle_esc RestoreCursor with no saved state | Cursor reset to (0,0) with defaults | Unit (Rust) |
| TS-04 | handle_esc Index mid-screen | Cursor row incremented | Unit (Rust) |
| TS-05 | handle_esc Index at scroll region bottom (full screen) | scroll_up_internal called, top line to scrollback | Unit (Rust) |
| TS-06 | handle_esc Index at scroll region bottom (partial region) | Scroll within region, no scrollback | Unit (Rust) |
| TS-07 | handle_esc NextLine | cursor.col=0 + index behavior | Unit (Rust) |
| TS-08 | handle_esc ReverseIndex mid-screen | Cursor row decremented | Unit (Rust) |
| TS-09 | handle_esc ReverseIndex at scroll region top | scroll_down_internal called | Unit (Rust) |
| TS-10 | handle_esc HTS | Tab stop set at cursor.col | Unit (Rust) |
| TS-11 | handle_esc RIS | All state reset (cursor, modes, tabs, ring buffer) | Unit (Rust) |
| TS-12 | handle_esc SetG0CharSet ASCII | g0_charset = 0 | Unit (Rust) |
| TS-13 | handle_esc SetG0CharSet DecLineDrawing | g0_charset = 1 | Unit (Rust) |
| TS-14 | handle_esc SetG1CharSet | g1_charset updated | Unit (Rust) |

#### Ring Buffer (FR10-FR16)

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-15 | Constructor with scrollback_lines | ring_capacity = scrollback_lines + rows | Unit (Rust) |
| TS-16 | ring_push_blank adds line | ring_size increases by 1 | Unit (Rust) |
| TS-17 | ring_push_blank at capacity | ring_head advances, oldest evicted | Unit (Rust) |
| TS-18 | viewport_abs mapping | Correct index for all viewport rows | Unit (Rust) |
| TS-19 | scrollback_abs mapping | Correct index for scrollback lines | Unit (Rust) |
| TS-20 | get_scrollback_length initially 0 | Returns 0 when no scrollback | Unit (Rust) |
| TS-21 | get_scrollback_length after scroll | Returns correct count | Unit (Rust) |
| TS-22 | Ring buffer wrap-around | head + size > capacity works correctly | Unit (Rust) |
| TS-23 | Viewport cell access after wrap-around | Correct cells returned | Unit (Rust) |
| TS-24 | get_row_packed with ring buffer | Packed data matches expected | Unit (Rust) |
| TS-25 | get_scrollback_row_packed | Correct scrollback data returned | Unit (Rust) |
| TS-26 | Dirty tracking covers viewport only | Dirty bits match viewport rows | Unit (Rust) |

#### Scroll Operations (FR17-FR22)

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-27 | scroll_up_internal full screen | Top line to scrollback, bottom cleared | Unit (Rust) |
| TS-28 | scroll_up_internal scroll region | Lines shift within region, no scrollback | Unit (Rust) |
| TS-29 | scroll_up_internal count=3 | 3 lines scrolled | Unit (Rust) |
| TS-30 | scroll_up_internal count > region height | Clamped to region height | Unit (Rust) |
| TS-31 | scroll_down_internal | Top cleared, content shifts down | Unit (Rust) |
| TS-32 | handle_print with wrap-scroll | Returns 0, scroll internal | Unit (Rust) |
| TS-33 | handle_execute LF at bottom | Returns 0, scroll internal | Unit (Rust) |
| TS-34 | handle_execute BEL | Returns 0xFE sentinel | Unit (Rust) |
| TS-35 | handle_scroll_up full screen | Returns 0, scroll internal | Unit (Rust) |
| TS-36 | handle_scroll_up scroll region | Returns 0, scroll internal | Unit (Rust) |

#### Reflow (FR23-FR27)

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-37 | resize_reflow same width | Row count change only, no reflow | Unit (Rust) |
| TS-38 | resize_reflow wider | Wrapped lines merge | Unit (Rust) |
| TS-39 | resize_reflow narrower | Long lines split | Unit (Rust) |
| TS-40 | resize_reflow cursor tracking | Cursor position correctly adjusted | Unit (Rust) |
| TS-41 | resize_reflow empty lines trimmed | Trailing empties removed | Unit (Rust) |
| TS-42 | resize_reflow with scrollback | Scrollback lines included | Unit (Rust) |
| TS-43 | resize_reflow capacity overflow | Oldest scrollback evicted | Unit (Rust) |
| TS-44 | resize_no_reflow | Simple resize without reflow | Unit (Rust) |
| TS-45 | Scroll region invalidated after resize | Region reset to full screen | Unit (Rust) |

#### Scrollback Access (FR28-FR30)

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-46 | get_scrollback_row_packed format | Same packed format as get_row_packed | Unit (Rust) |
| TS-47 | get_scrollback_length accuracy | Matches ring_size - rows | Unit (Rust) |
| TS-48 | get_scrollback_text content | Trimmed text of scrollback line | Unit (Rust) |

#### syncCursorAttrsToWasm Removal (FR31-FR33)

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-49 | syncCursorAttrsToWasm method removed | Not in TerminalState class | Code review |
| TS-50 | syncCursorAttrsToWasm removed from interface | Not in TerminalStateAccessor | Code review |
| TS-51 | No syncCursorAttrsToWasm call sites | grep returns 0 matches | Automated |

#### Integration (FR34-FR37)

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-52 | handleEscWasm dispatches SaveCursor | WASM handle_esc called, cursor synced | Integration (TS) |
| TS-53 | handleEscWasm dispatches Index | WASM handle_esc called, row synced | Integration (TS) |
| TS-54 | UnifiedBuffer scrollUp WASM mode | Delegates to WASM | Integration (TS) |
| TS-55 | UnifiedBuffer resize WASM mode | Calls resize_reflow, unpacks cursor | Integration (TS) |
| TS-56 | JS fallback path unchanged | TS handlers work when WASM unavailable | Integration (TS) |

#### Overflow Table Migration (Phase 1)

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-67 | Overflow cell write uses absolute ring index | Overflow key uses viewport_abs, not viewport row | Unit (Rust) |
| TS-68 | Overflow cell read after scroll | Scrollback overflow entries still accessible | Unit (Rust) |
| TS-69 | Overflow entry cleanup on eviction | Evicted line's overflow entries removed | Unit (Rust) |

#### resize() Removal (Phase 5)

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-70 | Old resize() method removed | Compilation fails if resize(cols, rows) called | Code review |

#### Wide Character Reflow (Phase 5)

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-71 | Reflow CJK text narrower | Wide chars not split across lines, padding added | Unit (Rust) |
| TS-72 | Reflow CJK text wider | Wide chars merge correctly, placeholders removed | Unit (Rust) |

#### Constructor Migration (Phase 1)

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-73 | All existing tests compile with new(cols, rows, 0) | 174 call sites updated, all tests pass | Automated |

#### Edge Cases

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-57 | Ring buffer with 0 scrollback | No scrollback, scroll discards | Unit (Rust) |
| TS-58 | Ring buffer with 1 scrollback line | Single line stored then evicted | Unit (Rust) |
| TS-59 | Scrollback at max capacity | Eviction works correctly | Unit (Rust) |
| TS-60 | Reflow very long line (>1000 cols → 80) | Splits into many physical lines | Unit (Rust) |
| TS-61 | Reflow cursor on split line | Cursor tracks to correct line | Unit (Rust) |
| TS-62 | Reflow cursor past trimmed content | Clamped to end | Unit (Rust) |
| TS-63 | RIS during scroll region | Region cleared, ring reset | Unit (Rust) |
| TS-64 | SaveCursor → resize → RestoreCursor | Saved cursor clamped to new dims | Unit (Rust) |
| TS-65 | Index in origin mode | Respects scroll region boundaries | Unit (Rust) |
| TS-66 | Scrollback with overflow cells | Overflow graphemes stored/retrieved | Unit (Rust) |

## Code Quality Verification

### Format Check
```bash
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
```

### TypeScript Type Check
```bash
bun run typecheck
```

## File Structure Verification

### Files to Create
- `wasm/src/ring_buffer.rs` — Ring Buffer operations, scroll internal, reflow
- `wasm/src/esc_handler.rs` — ESC dispatch and handler implementations

### Files to Modify
- `wasm/src/lib.rs` — Add ring_buffer, esc_handler modules
- `wasm/src/terminal_core.rs` — Ring Buffer fields, constructor, index mapping
- `wasm/src/print_handler.rs` — Use scroll_up_internal, return 0
- `wasm/src/c0_handler.rs` — Use scroll_up_internal, return 0
- `wasm/src/csi_scroll.rs` — Use scroll_up/down_internal, return 0
- `wasm/src/csi_cursor.rs` — viewport_abs for row calculations
- `wasm/src/csi_screen.rs` — viewport_abs for row calculations
- `wasm/src/csi_edit.rs` — viewport_abs for row calculations
- `src/terminal/state.ts` — handleEscWasm, remove syncCursorAttrsToWasm
- `src/terminal/unified-buffer.ts` — Thin WASM wrapper
- `src/terminal/wasm/terminal-core.ts` — Constructor + scrollback APIs
- `src/terminal/handlers/esc_handlers.ts` — Remove syncCursorAttrsToWasm call
- `src/terminal/handlers/types.ts` — Remove syncCursorAttrsToWasm from interface
- `src/terminal/handlers/csi_char_attrs.ts` — Remove syncCursorAttrsToWasm call
- `src/terminal/handlers/csi_modes.ts` — Remove syncCursorAttrsToWasm call

## SPEC.md Compliance

### Success Criteria

| ID | Criterion from SPEC.md | How to Verify |
|----|------------------------|---------------|
| SC-1 | `wasm-pack build` succeeds with Sprint 5 additions | Build command exit code 0 |
| SC-2 | WASM binary < 70KB | Check file size of .wasm output |
| SC-3 | All Rust unit tests pass (Sprint 1-5) | `cargo test` exit code 0 |
| SC-4 | All TS tests pass (1824+) | `bun test` exit code 0 |
| SC-5 | ESC operations match TS handler results | Cross-validation tests |
| SC-6 | Scrollback in WASM linear memory | Verify ring_cells usage, no JS Line objects |
| SC-7 | Scroll operations WASM-internal (0 bridge) | Verify handle_print/execute return 0 |
| SC-8 | Reflow matches TS implementation | Reflow comparison tests |
| SC-9 | syncCursorAttrsToWasm completely removed | grep returns 0 matches |
| SC-10 | `bun tauri dev` shows working terminal | Manual test |
| SC-11 | vttest basic tests unchanged | Manual test |
| SC-12 | vim/less/top switch correctly | Manual test |
| SC-13 | Scrollback view works | Manual test |
| SC-14 | Resize preserves scrollback | Manual + unit test |

### Functional Requirements Coverage

| Requirement | Implementation Phase | Verification |
|-------------|---------------------|--------------|
| FR1 (handle_esc dispatcher) | Phase 4 | TS-01 to TS-14 |
| FR2 (SaveCursor) | Phase 4 | TS-01 |
| FR3 (RestoreCursor) | Phase 4 | TS-02, TS-03 |
| FR4 (Index with scroll) | Phase 4 | TS-04, TS-05, TS-06 |
| FR5 (NextLine) | Phase 4 | TS-07 |
| FR6 (ReverseIndex) | Phase 4 | TS-08, TS-09 |
| FR7 (HTS) | Phase 4 | TS-10 |
| FR8 (RIS with Ring Buffer) | Phase 4 | TS-11, TS-63 |
| FR9 (SetG0/SetG1) | Phase 4 | TS-12, TS-13, TS-14 |
| FR10 (Ring Buffer structure) | Phase 1 | TS-15, TS-67, TS-68, TS-69, TS-73 |
| FR11 (Viewport mapping) | Phase 1 | TS-18, TS-23, TS-24 |
| FR12 (Scrollback mapping) | Phase 1 | TS-19 |
| FR13 (ring_push) | Phase 2 | TS-16, TS-17 |
| FR14 (get_scrollback_length) | Phase 3 | TS-20, TS-21, TS-47 |
| FR15 (Ring capacity) | Phase 1 | TS-15 |
| FR16 (Dirty tracking) | Phase 1 | TS-26 |
| FR17 (Full-screen scroll up) | Phase 2 | TS-27 |
| FR18 (Region scroll up) | Phase 2 | TS-28 |
| FR19 (Scroll down) | Phase 2 | TS-31 |
| FR20 (handle_print returns 0) | Phase 2 | TS-32 |
| FR21 (handle_execute returns BEL/0) | Phase 2 | TS-33, TS-34 |
| FR22 (handle_scroll_up returns 0) | Phase 2 | TS-35, TS-36 |
| FR23 (resize_reflow) | Phase 5 | TS-37 to TS-43 |
| FR24 (Reflow algorithm) | Phase 5 | TS-38, TS-39, TS-40, TS-71, TS-72 |
| FR25 (Same-width resize) | Phase 5 | TS-37 |
| FR26 (resize_no_reflow) | Phase 5 | TS-44 |
| FR27 (Scroll region after resize) | Phase 5 | TS-45 |
| FR28 (get_scrollback_row_packed) | Phase 3 | TS-25, TS-46 |
| FR29 (get_scrollback_length) | Phase 3 | TS-47 |
| FR30 (get_scrollback_text) | Phase 3 | TS-48 |
| FR31 (Remove syncCursorAttrsToWasm method) | Phase 6 | TS-49 |
| FR32 (Remove from interface) | Phase 6 | TS-50 |
| FR33 (Remove all call sites) | Phase 6 | TS-51 |
| FR34 (handleEscWasm dispatch) | Phase 6 | TS-52, TS-53 |
| FR35 (UnifiedBuffer WASM scroll) | Phase 6 | TS-54 |
| FR36 (UnifiedBuffer WASM resize) | Phase 6 | TS-55 |
| FR37 (JS fallback unchanged) | Phase 6 | TS-56 |

### Non-Functional Requirements Coverage

| Requirement | Verification |
|-------------|--------------|
| NFR1 (0 boundary crossings for scroll) | TS-32, TS-33, TS-35: return values are 0 |
| NFR2 (Reflow >= TS speed) | Manual benchmark comparison |
| NFR3 (Scrollback in WASM memory) | Code review: ring_cells in WASM linear memory |
| NFR4 (All TS tests pass) | `bun test` exit code 0 |
| NFR5 (JS fallback unchanged) | TS-56 |
| NFR6 (vttest unchanged) | Manual test: SC-11 |
| NFR7 (WASM < 70KB) | SC-2: file size check |
| NFR8 (scrollback_lines setting) | Constructor test with setting value |

## Manual Testing

Items requiring human judgment or real terminal interaction:

- [ ] `bun tauri dev` shows working terminal with typing
- [ ] vim opens, edits, saves, and exits correctly
- [ ] less scrolls content and exits cleanly
- [ ] top displays and updates in real-time
- [ ] Scrollback: scroll up to view history, content is correct
- [ ] Resize: terminal content reflows correctly with scrollback present
- [ ] vttest: basic tests produce expected output
- [ ] Large output (e.g., `find /`): scrollback fills, oldest lines evicted
- [ ] Alternate buffer apps (vim, less): scrollback not affected

## Performance Verification

### Binary Size
- **Requirement**: WASM binary < 70KB
- **Baseline**: Sprint 4 = 51.4KB
- **Expected**: ~59KB (51.4 + ~7.5KB for Ring Buffer + reflow)
- **Command**: `ls -la wasm/pkg/*.wasm`

### Scroll Performance
- **Requirement**: 0 WASM-TS boundary crossings for scroll
- **Verification**: handle_print, handle_execute, handle_scroll_up all return 0

### Memory Usage
- **Reference**: 80 cols × 10,024 lines × 32B = ~25MB for default config
- **Command**: Check WASM memory usage in browser DevTools

## Automated Verification Commands

### syncCursorAttrsToWasm Removal Check
```bash
# Should return 0 matches (excluding test files and comments)
grep -rn "syncCursorAttrsToWasm" src/terminal/ --include="*.ts" | grep -v "test" | grep -v "//"
```

### WASM Binary Size Check
```bash
# Should be < 70KB (71680 bytes)
wasm_size=$(stat -c%s wasm/pkg/emterm_wasm_bg.wasm 2>/dev/null || stat -f%z wasm/pkg/emterm_wasm_bg.wasm)
[ "$wasm_size" -lt 71680 ] && echo "PASS: ${wasm_size} bytes" || echo "FAIL: ${wasm_size} bytes"
```

### Return Value Check (Scroll Bridge Elimination)
```bash
# Verify handle_scroll_up always returns 0 in tests
cargo test --manifest-path src-tauri/Cargo.toml -- scroll_up 2>&1 | grep -E "test result|FAILED"
```

## Verification Summary

| Category | Items | Automated | Manual |
|----------|-------|-----------|--------|
| Build | 3 | 3 | 0 |
| Unit Tests (Rust) | 55 | 55 | 0 |
| Integration Tests (TS) | 5 | 5 | 0 |
| Code Quality | 4 | 4 | 0 |
| File Structure | 17 | 17 | 0 |
| SPEC Compliance | 14 | 9 | 5 |
| Edge Cases | 10 | 10 | 0 |
| Performance | 3 | 2 | 1 |
| Manual Testing | 9 | 0 | 9 |

**Total**: 105 automated items, 15 manual items

Note: Unit Tests (Rust) = TS-01 to TS-48 (48) + TS-67 to TS-69 (3) + TS-71 to TS-72 (2) + TS-51 (1) + TS-73 (1) = 55. Edge Cases (TS-57 to TS-66) counted separately. Code Quality includes fmt, typecheck, TS-70 (resize removal check), TS-73 (constructor migration check).
