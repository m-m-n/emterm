# Verification Document: HTML Viewer

## Overview
**Feature**: html-viewer / **SPEC.md**: `feature-docs/html-viewer/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/html-viewer/IMPLEMENTATION.md`

## Build Verification
- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (cli): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Command (web): `bun run build:viewer`
- Expected: exit code 0, no errors

## Test Verification
- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command (web): `bun run typecheck && bun test`
- Note: integration tests (`tests/cli_subcommands.rs`) run in the same cargo test invocation family; `tabs.rs` replay tests are order-sensitive (`--test-threads=1` if flaky).

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | OSC generator: single/multi chunk, basedir present/absent, sanitized basedir | Well-formed OSC 777 `html` frames | Unit |
| TS-2 | Extension validation `.html`/`.htm` case-insensitive; others rejected | Accept/reject as specified, non-zero exit on reject | Unit |
| TS-3 | Size boundary: exactly 10MB accepted, over rejected | Boundary honored | Unit |
| TS-4 | Missing file / directory input | Correct error variants, stderr message, non-zero exit | Unit |
| TS-5 | `REPLAYABLE_VIEWER_KINDS` contains `html`; route + scrollback drift tests pass | SSOT and consumers in lockstep | Unit |
| TS-6 | `emterm html <fixture>` end-to-end: valid → stdout sequence + exit 0; invalid → non-zero | CLI behavior verified through the binary | Integration |
| TS-7 | Accumulator: begin/chunk/end (incl. out-of-order seq) → one render request; malformed input dropped | Ingest correctness | Unit |
| TS-8 | Resolver: allowed types served with MIME; absolute / traversal / symlink-escape / oversize / disallowed-type denied | Basedir confinement | Unit |
| TS-9 | Navigation predicate + popup decision: viewer scheme (both platform forms) allowed; http(s) delegated externally; file/javascript/data denied; popups never open in-WebView | Navigation policy | Unit |
| TS-10 | Document response: payload HTML served verbatim with network-blocking CSP header (scheme forms + inline + data: only) | CSP present and restrictive | Unit |
| TS-11 | Payload writer: create-new 0600 JSON, round-trip of document + basedir | Hand-off integrity | Unit |

## Code Quality Verification
- Format: （プロジェクト方針により crate 全体の fmt は実行しない）
- Static analysis: cargo check（上記 Build Verification に含む）

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All FR1–FR6 implemented and tested | TS-1..TS-11 + requirements coverage below |
| SC-2 | Unit + integration scenarios pass | cargo test (main), bun (web) |
| SC-3 | CLI-only build passes | cli build command above |
| SC-4 | Linux GUI manual scenario confirmed | M-1..M-5 below |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-3, TS-4, TS-6 |
| FR2 | task0002, task0004 | TS-5, TS-7, TS-11, M-1 |
| FR3 | task0004 | M-2 |
| FR4 | task0004 | TS-10, M-3 |
| FR5 | task0003, task0004 | TS-8, M-4 |
| FR6 | task0004 | TS-9, M-5 |
| NFR1 | task0003, task0004 | TS-8, TS-9, TS-10, M-3, M-5 |
| NFR2 | task0001, task0004 | TS-6 (CLI build), M-1 (Linux); Windows は後日手動確認 |

## E2E Testing
（プロジェクトに自動 E2E 基盤なし — 省略）

## Manual Testing (E2E Not Possible)
- [ ] M-1: eMterm GUI 内で `emterm html sample.html` を実行 → 子ウィンドウが開き、HTML が素のまま（eMterm テーマなしで）描画される
- [ ] M-2: インライン `<script>` を含む HTML → JavaScript が実行される（例: DOM 書き換えが反映される）
- [ ] M-3: `https://` の画像 / CSS / fetch を参照する HTML → 外部リソースは読み込まれず、ページ自体は描画される
- [ ] M-4: 相対パスのローカル画像・CSS を参照する HTML → 表示される。`../` で basedir 外を参照するものは表示されない
- [ ] M-5: `<a href="https://...">` をクリック → 既定ブラウザで開き、WebView 内は遷移しない。ページ内アンカーは WebView 内で動作する

## Performance / Security Verification
- ネットワーク遮断（FR4/NFR1）: TS-10 の CSP ヘッダ検証 + M-3 の実機確認
- basedir 封じ込め（FR5/NFR1）: TS-8 の traversal / symlink 拒否テスト

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 3 | 3 | 0 | 0 |
| Unit/Integration | 11 (TS-1..TS-11) | 11 | 0 | 0 |
| Manual | 5 (M-1..M-5) | 0 | 0 | 5 |
