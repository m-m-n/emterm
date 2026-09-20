# Verification Document: example-font-absolute-path

## Overview

**Feature**: example-font-absolute-path /
**SPEC.md**: `feature-docs/example-font-absolute-path/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/example-font-absolute-path/IMPLEMENTATION.md`

統合後のフィーチャー全体を検証する。タスク単位の受け入れ基準は
`feature-docs/example-font-absolute-path/tasks/task0001.md` にある。
すべてのコマンドはプロジェクトルートから実行する (`.claude/rules/core-build-location.md`)。

## Build Verification

- Command:
  `bash scripts/fetch-fonts.sh && bun install && bun run build:viewer && bun run build:settings && CARGO_TARGET_DIR=src-tauri/target cargo build --manifest-path src-tauri/Cargo.toml --examples`
- Expected: exit code 0、エラーなし。example 3 本がビルド対象に含まれること

## Test Verification

- Command:
  `bash scripts/fetch-fonts.sh && bun install && bun run build:viewer && bun run build:settings && CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1`
- Coverage target: 本プロジェクトはカバレッジ目標を定義していない。該当なし

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | `src-tauri/examples/` の全 Rust ソースを読み、`/home/` 始まりの絶対パスリテラルが 0 件であることを assert する<br>`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test examples_source_hygiene` | 該当ケースが緑。走査件数が 0 のときは失敗する | Integration |
| TS2 | 同ソース群に `native-poc/` 文字列が 0 件であることを assert する<br>`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test examples_source_hygiene` | 該当ケースが緑 | Integration |
| TS3 | 既存 Rust テスト一式が緑のままであること<br>`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1` | 全テスト緑。既存テストの新規失敗なし | Integration (regression) |
| TS4 | example 3 本が `gui` feature 有効でコンパイルできること<br>`CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --examples` | exit code 0、警告によるビルド失敗なし | Build check |
| TS5 | `scripts/fetch-fonts.sh` 実行後に `bold_raster_probe` / `m_placement_probe` を実行し、フォント読み込みが成功して従来と同等の dump が出ること | 2 本とも panic せず、coverage dump / placement dump が従来書式で出力される | Manual |

NFR1（テストのフォント非依存）の確認だけは、フォント取得に依存しない形で追加実行する:
`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test examples_source_hygiene`
（既定 feature のままだと本体のフォント埋め込みでビルドが止まりうるため。IMPLEMENTATION.md D2 / SPEC A7）

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
- Static analysis: 本プロジェクトは workflow.yaml に静的解析コマンドを定義していない。
  `cargo check --examples` (TS4) をコンパイル時検査として用いる

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | `src-tauri/examples/` の 3 ファイルいずれにも `/home/` で始まる文字列リテラルが存在しない | TS1 |
| SC-2 | `src-tauri/examples/` の 3 ファイルいずれにも `native-poc/` 文字列が存在しない | TS2 |
| SC-3 | `bold_raster_probe.rs` / `m_placement_probe.rs` が crate マニフェストディレクトリ起点で `src-tauri/assets/fonts/` 配下のフォントを実行時に読む | 差分レビュー + TS5 |
| SC-4 | `m_placement_probe.rs` の変更が絶対パス箇所のみで、ファイル先頭に新規コメントが追加されていない | 差分レビュー |
| SC-5 | `src-tauri/tests/examples_source_hygiene.rs` が緑になる | TS1, TS2 |
| SC-6 | 既存テストを含めて Rust テスト一式が緑のまま | TS3 |
| SC-7 | `src-tauri/src/render/font/resolver.rs` が無変更 | 差分レビュー（統合差分に同ファイルが現れないこと） |
| SC-8 | `scripts/fetch-fonts.sh` 実行後、3 つの probe が `cargo run --example` で実際に動作する | TS5 + `font_select_probe` の手動実行 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS4, TS5 |
| FR2 | task0001 | TS1, TS4, TS5 |
| FR3 | task0001 | TS2 |
| FR4 | task0001 | TS2 |
| FR5 | task0001 | TS1, TS2 |
| FR6 | task0001 | TS3 + 差分レビュー (SC-7) |
| NFR1 | task0001 | TS1（`--no-default-features` での追加実行を含む） |
| NFR2 | task0001 | TS4 + 差分レビュー（区切り文字の直書きがないこと） |
| NFR3 | task0001 | TS3, TS4 + 差分レビュー（`Cargo.toml` 無変更・新規依存なし） |
| NFR4 | task0001 | TS5 |

## E2E Testing

該当なし。本プロジェクトに E2E フレームワークは無く、workflow.yaml の
`project.components.main.e2e_test_command` は空。

## Manual Testing (E2E Not Possible)

- [ ] TS5-a: `bash scripts/fetch-fonts.sh` 実行後、
      `CARGO_TARGET_DIR=src-tauri/target cargo run --manifest-path src-tauri/Cargo.toml --example bold_raster_probe`
      が panic せず、Regular / Bold それぞれの coverage dump が従来と同じ書式で出ること
- [ ] TS5-b: `CARGO_TARGET_DIR=src-tauri/target cargo run --manifest-path src-tauri/Cargo.toml --example m_placement_probe`
      が panic せず、placement dump が従来と同じ書式で出ること
- [ ] SC-8: `CARGO_TARGET_DIR=src-tauri/target cargo run --manifest-path src-tauri/Cargo.toml --example font_select_probe -- Inconsolata`
      が face 列挙を従来どおり出力すること（システムに Inconsolata が無い場合は 0 件表示で正常）
- [ ] 各 probe のファイル先頭コメントに書かれた実行手順を、そのままプロジェクトルートで実行できること

モックとの目視照合は不要（design ステップは skipped で、UI 表示面に触れない）。

## Performance / Security Verification (if applicable)

該当なし。SPEC.md に性能目標・セキュリティ要件の記載がない。

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Test scenarios (TS1-TS5) | 5 | 4 (TS1-TS4) | 0 | 1 (TS5) |
| Success criteria (SC-1-SC-8) | 8 | 4 (SC-1, SC-2, SC-5, SC-6) | 0 | 4 (SC-3, SC-4, SC-7, SC-8: 差分レビュー / 手動実行) |
| Requirements (FR1-FR6, NFR1-NFR4) | 10 | 8 | 0 | 2 (NFR4, FR6 の一部) |
