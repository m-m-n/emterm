# Verification Document: wide-pair-overflow-tests

## Overview

**Feature**: wide-pair-overflow-tests /
**SPEC.md**: `feature-docs/wide-pair-overflow-tests/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/wide-pair-overflow-tests/IMPLEMENTATION.md`

## Build Verification

workflow.yaml `project.components` の承認済みコマンドを一字一句そのまま使う。

- term_core:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml`
- main:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: 両方とも exit code 0、エラーなし。

## Test Verification

- term_core（主対象）:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- main（workspace 回帰）:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Expected: 両方とも全件パス（失敗 0）。
- カバレッジ目標: 数値目標は設定しない（プロジェクトにカバレッジ計測ツール
  未導入）。本 feature の目標は「overflow 分岐（`char_len == 0xFF`）が
  新規テスト TS1–TS3 で実際に実行・検証されること」であり、達成は各テストの
  事前/事後 assert（FR3）で判定する。

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | 家族絵文字を col0 に書き、overflow 状態を事前 assert 後、col0 を ASCII で上書き（print / base 上書き） | col1 spacer が `" "` / 幅 1。overflow・`overflow_ridx` から該当エントリ除去 | Unit |
| TS2 | 同ペアの spacer（col1）を ASCII で上書き（print / spacer 上書き） | col0（overflow base）が `" "` / 幅 1。overflow・`overflow_ridx` の同期維持 | Unit |
| TS3 | spacer 位置（col1）で DCH count=1（`handle_delete_characters`） | col0 の `get_cell_char` が `" "`（空文字ではない）。overflow エントリ除去済み | Unit |
| TS4 | spacer 位置（col1）で ECH count=1（`handle_erase_characters`） | start-1 = col0 が `" "` を返す（`csi_screen.rs:155` の `blank_wide_pair_split` 呼び出しを通る） | Unit |
| TS5 | 既存の `term_core --lib` スイート全件と workspace テストの実行 | 全件パス（回帰なし） | Unit (Regression) |

## Code Quality Verification

- Format: 該当なし（workflow.yaml の format_command は未設定）。
- Static analysis: 該当なし（未設定）。
- **NFR2 差分確認（必須）**: feature ブランチの差分が以下のみに閉じている
  ことを git diff で確認する。
  - `crates/term_core/src/print_handler/tests.rs`
  - `crates/term_core/src/csi_edit.rs`（`#[cfg(test)] mod tests` 内のみ）
  - `crates/term_core/src/csi_screen.rs`（`#[cfg(test)] mod tests` 内のみ）
  - `feature-docs/` 配下のドキュメント

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | print 経路のテストが存在し、overflow 行き確認のうえ ASCII 上書き後に spacer が `" "` / 幅 1 | TS1 / TS2 のテスト実体とその assert 内容を確認し、term_core `--lib` でパス |
| AC2 | DCH / ECH 経路のテストが存在し、実行後に col-1 の `get_cell_char` が `" "`（空文字ではない） | TS3 / TS4 のテスト実体とその assert 内容を確認し、term_core `--lib` でパス |
| AC3 | overflow 分岐（`char_len == 0xFF`）の実行が assert で証明されている（事前 assert 含む） | TS1–TS3 に掃除前の overflow 状態 assert と掃除後の overflow・`overflow_ridx` 除去 assert があることを確認 |
| AC4 | term_core `--lib` および workspace のテストが全件通る | Test Verification の 2 コマンドを実行し全件パス |
| NFR1 | inline `#[cfg(test)]` 配置・既存命名パターン準拠・新規 dev-dependency なし | テスト配置と命名の目視確認 + `crates/term_core/Cargo.toml` に差分がないことを確認 |
| NFR2 | `crates/term_core` の非テストコードに差分なし | Code Quality Verification の NFR2 差分確認 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS2（term_core `--lib`） |
| FR2 | task0002 | TS3, TS4（term_core `--lib`） |
| FR3 | task0001, task0002 | TS1, TS2, TS3 の事前/事後 assert |
| FR4 | task0001, task0002 | TS5（両テストコマンド全件パス） |
| NFR1 | task0001, task0002 | TS5 + 配置・命名・依存の確認 |
| NFR2 | task0001, task0002 | TS5 + git diff による非テストコード差分ゼロの確認 |

## E2E Testing

該当なし（プロジェクトに E2E フレームワークなし。test/README.md 参照）。

## Manual Testing (E2E Not Possible)

なし（テスト追加のみの feature であり、UI・ユーザー向け挙動の変更がない。
design ステップは skipped のためモック照合も対象外）。

## Performance / Security Verification (if applicable)

該当なし（SPEC.md にパフォーマンス・セキュリティ要件なし）。

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit tests（新規） | TS1–TS4 | 4 | 0 | 0 |
| Regression | TS5 | 1 | 0 | 0 |
| Code quality（NFR2 差分確認） | 1 | 1 | 0 | 0 |
| 合計 | 6 | 6 | 0 | 0 |
