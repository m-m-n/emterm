# Verification Document: Cursor Settings Fix (style / blink / color)

## Overview
**Feature**: cursor-settings-fix / **SPEC.md**:
`feature-docs/cursor-settings-fix/SPEC.md` / **IMPLEMENTATION.md**:
`feature-docs/cursor-settings-fix/IMPLEMENTATION.md`

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Also: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` (NFR2)
- Expected: exit code 0, no errors

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Coverage target: every TS below covered by at least one test (except the
  manual items)

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Tab spawned with `cursor_style: bar` | Core effective style = bar (2) | Unit |
| TS-2 | Settings apply with new style/blink, multiple tabs open | Every existing tab's core reports the new effective style and blink | Unit |
| TS-3 | `cursor_blink: false`, then DECSC/DECRC round-trip | Effective blink stays false | Unit |
| TS-4 | `cursor_blink: false`, then cursor-state reset path (restore with no saved state) | Effective blink stays false | Unit |
| TS-5 | DECSCUSR Ps=3 then Ps=0 (byte-fed through parser) | Override wins (underline + blink), then settings-derived defaults return | Unit |
| TS-6 | OSC 22 "underline" then OSC 22 "" | Shape override wins, then default shape returns; blink untouched | Unit |
| TS-7 | Cursor color: scheme default → OSC 12 → OSC 112 | Scheme cursor color → OSC value → scheme cursor color (never theme fg / pen fg) | Unit |
| TS-8 | Settings apply while DECSCUSR override active | Defaults update; effective getters keep the override | Unit |
| TS-9 | Alt-screen enter/exit (mode 1049 bytes) | Effective shape/blink unchanged | Unit |
| TS-10 | RIS full reset with overrides active | Overrides cleared; defaults survive | Unit |
| TS-11 | Unknown `cursor_style` string in settings.json | Warn-once + block fallback preserved | Unit (existing) |
| TS-12 | Block cursor glyph legibility | Covered glyph painted in cell's resolved background color | Unit |
| TS-13 | OSC 12 override, then settings apply (no scheme change) | Resolved cursor color stays at the OSC 12 value | Unit |
| TS-14 | OSC 12 override, then RIS | Resolved cursor color returns to the scheme cursor color; shape/blink overrides also cleared | Unit |

## Code Quality Verification
- Format: (none configured — rustfmt is not enforced crate-wide in this
  project)
- Static analysis: build verification commands above

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | Style change reaches all tabs' rendered cursor | TS-1, TS-2 + MT-1 |
| SC-2 | Blink-off survives tab switches / save-restore / reset | TS-3, TS-4, TS-9 + MT-2 |
| SC-3 | Cursor drawn in scheme cursor color | TS-7, TS-12 + MT-3 |
| SC-4 | Sequences override settings; resets restore defaults | TS-5, TS-6, TS-8, TS-10 + MT-4 |
| SC-5 | Existing suite passes; CLI build compiles | Test + build commands above |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001, task0002 | TS-1, TS-2 |
| FR2 | task0001 | TS-3, TS-4, TS-9 |
| FR3 | task0003 | TS-7, TS-12 |
| FR4 | task0001, task0003, task0004 | TS-5, TS-6, TS-7, TS-10, TS-14 |
| FR5 | task0001, task0002, task0004 | TS-2, TS-8, TS-13 |
| NFR1 | task0003 | Review: no added locking/allocation on the per-frame path (see Manual/Review below) |
| NFR2 | task0001 | `--no-default-features` build command above |

## E2E Testing
(no E2E framework in this project — omitted)

## Manual Testing (E2E Not Possible)
- [ ] MT-1: 設定画面でカーソルスタイルを block → bar に変更 → 全タブで即時反映され、タブ切り替え後も維持される
- [ ] MT-2: カーソル点滅をオフ → 別タブへ切り替えて戻っても点滅しない
- [ ] MT-3: カラースキームの cursor 色でカーソルが描画される（テキスト fg 色ではない）
- [ ] MT-4: vim 起動時の DECSCUSR が設定より優先され、vim 終了後に設定値へ戻る
- [ ] MT-5: ブロックカーソル下の文字が判読できる（セル背景色で再描画）

## Performance / Security Verification
- NFR1: render-path diff review — cursor color/style resolution adds no
  lock acquisition or per-frame allocation beyond field reads.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios | 14 | 14 | 0 | 0 |
| Manual | 5 | 0 | 0 | 5 |
| Performance (NFR1) | 1 | 0 | 0 | 1 (review) |
