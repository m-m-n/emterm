# Verification Document: Keycap Cluster Composition

## Overview

**Feature**: keycap-cluster-composition /
**SPEC.md**: `feature-docs/keycap-cluster-composition/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/keycap-cluster-composition/IMPLEMENTATION.md`

## Build Verification

- Command (term_core): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml`
- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification

- Command (term_core): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: every acceptance criterion in task0001/task0002 has at
  least one test

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `5 FE0F 20E3` then text | One base cell with full cluster, width 2, spacer, text after spacer | Unit (term_core) |
| TS-2 | `5 20E3` then text | One width-1 cell with the cluster, text in next column | Unit (term_core) |
| TS-3 | `e 0301` then text | é composed in one cell, accent survives | Unit (term_core) |
| TS-4 | Width-0 char with no base (start of line / invalidated) | Dropped: grid unchanged, cursor unchanged | Unit (term_core) |
| TS-5 | Wide char then U+FE0F | Merged into wide base cell across the spacer | Unit (term_core) |
| TS-6 | VS16 merge at last column | Width-2 expansion follows existing end-of-line wide-char semantics | Unit (term_core) |
| TS-7 | Existing grapheme-buffer / ZWJ / ExtPict tests | All pass unchanged | Unit (regression) |
| TS-8 | Emoji-routed keycap cluster shaping input | Contains no U+FE0F; single ligature glyph | Unit (renderer) |
| TS-9 | Font selection for cluster including VS16 | Color emoji font selected; existing presentation/fallback tests pass | Unit (renderer) |
| TS-10 | ASCII fast path | Untouched by the change (code inspection + TS-7 suite green) | Inspection |

## Code Quality Verification

- Format: (none — PostToolUse hook formats edited files automatically)
- Static analysis: build commands above (rustc warnings)

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1–FR5 implemented and covered by tests | TS-1..TS-9 green |
| SC-2 | Full existing test suite passes (NFR1) | TS-7 + both lib test commands green |
| SC-3 | 5️⃣ renders as one keycap glyph on a release build | Manual (human) |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-3 |
| FR2 | task0001 | TS-1, TS-6 |
| FR3 | task0001 | TS-5 |
| FR4 | task0001 | TS-4 |
| FR5 | task0002 | TS-8, TS-9 |
| NFR1 | task0001, task0002 | TS-7, TS-9 |
| NFR2 | task0001 | TS-10 |

## Manual Testing (E2E Not Possible)

No E2E infrastructure for the GUI terminal path. Human verification on a
release build (built only on the user's explicit instruction):

- [ ] M-1: `5️⃣`（U+0035 U+FE0F U+20E3）を実機 eMterm で出力し、1つの keycap
      絵文字グリフ（幅2）で描画されることを目視確認する
- [ ] M-2: `5⃣`（VS16 なし）が幅1で合成描画されることを目視確認する
- [ ] M-3: `é`（e + U+0301 分離到着）が正しく合成表示されることを目視確認する

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| term_core unit | TS-1..TS-7 | 7 | 0 | 0 |
| renderer unit | TS-8, TS-9 | 2 | 0 | 0 |
| inspection | TS-10 | 0 | 0 | 1 |
| visual | M-1..M-3 | 0 | 0 | 3 |
