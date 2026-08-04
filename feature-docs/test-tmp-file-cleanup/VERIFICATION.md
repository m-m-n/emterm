# Verification Document: test-tmp-file-cleanup

## Overview

**Feature**: test-tmp-file-cleanup / **SPEC.md**: `feature-docs/test-tmp-file-cleanup/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/test-tmp-file-cleanup/IMPLEMENTATION.md`

## Build Verification

- rust: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- typescript: `bun run typecheck`
- Expected: いずれも exit code 0、エラーなし

## Test Verification

- rust: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- typescript: `bun test`
- Coverage target: 既存カバレッジの維持（本機能はテストコードの後始末修正であり、新規プロダクションコードはない）
- 注: `tabs.rs` の replay テストには既知の並列実行フレークがある（本機能と無関係の既存ベースライン）。失敗した場合は `--test-threads=1` を付けて再実行して判定する。

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | `/tmp` 直下のエントリを記録 → 上記 rust `--lib` テストを成功完走 → 前後比較 | 実行起因の `emterm-*` 新規エントリが `/tmp` に残らない | Integration（`/tmp` 目視/リスト比較） |
| TS2 | 同様の前後比較を `--test cli_subcommands`、`crates/{term_core,term_images,app_settings,mux_ipc}` の各 `--lib`、`bun test` について実施 | 残留ゼロ | Integration（`/tmp` 目視/リスト比較） |
| TS3 | FR1〜FR4 の各修正テストを個別実行し、成功後に当該一時パス（`emterm-settings-store-test-*` / `emterm-settings-window-test-*` / `emterm-tmux-import-test-*` / `emterm-viewer-*.json`）を確認 | 該当パスが存在しない | Unit |
| TS4 | 修正後の全対象テストの回帰実行（rust `--lib` / 統合テスト / `bun test`） | 全件成功（`tabs.rs` フレーク時は `--test-threads=1` で再判定） | Unit/Integration |
| TS5 | 依存関係の差分確認: `src-tauri/Cargo.toml`・`Cargo.lock`・`package.json` に新規依存エントリがない（既存 dev-dependency `tempfile = "3"` の利用は可） | 新規依存なし（NFR2） | Inspection |
| TS6 | 変更差分の位置確認: 全変更ハンクが 4 対象ファイル（`settings_store.rs` / `settings_window/commands.rs` / `mux/tmux_import.rs` / `viewer/launch.rs`）のテスト専用モジュール（`cfg(test)` 配下）内にある | プロダクション関数に差分なし（NFR3） | Inspection |

## Code Quality Verification

- Format: 未設定（`format_command` なし）。crate 全体の一括フォーマットは実行しない（プロジェクト規約）。
- Static analysis: Build Verification の `cargo check` / `bun run typecheck` で代替。

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1〜FR5 が実装・検証されている | TS1〜TS4 + 下表のカバレッジ |
| SC-2 | TS1〜TS4 が合格する | 各シナリオの実施 |
| SC-3 | AC-1: FR1〜FR4 の各テスト成功後に対応する `/tmp` パスが存在しない | TS3 |
| SC-4 | AC-2: 文書化された全テストコマンドの成功実行前後で `/tmp` に実行起因の新規残留がない | TS1, TS2 |
| SC-5 | AC-4: 既存スイート（rust `--lib` / 統合テスト / `bun test`）が全件成功する | TS4 |
| SC-6 | NFR2: 新規依存が追加されていない | TS5 |
| SC-7 | NFR3: プロダクションの `/tmp` 書き込み挙動が不変 | TS6 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS3, TS4 |
| FR2 | task0002 | TS1, TS3, TS4 |
| FR3 | task0003 | TS1, TS3, TS4 |
| FR4 | task0004 | TS1, TS3, TS4 |
| FR5 | task0001, task0002, task0003, task0004 | TS1, TS2, TS4 |
| NFR1 | task0001, task0002, task0003, task0004 | 検証項目なし（スコープ限定要件: 異常終了・panic 経路の残留を許容する「非要求」の宣言であり、能動的に検証すべき振る舞いが存在しない。各タスクの AC が正常終了経路のみに削除を要求していることが整合の確認になる） |
| NFR2 | task0001, task0002, task0003, task0004 | TS5 |
| NFR3 | task0001, task0002, task0003, task0004 | TS6 |

## E2E Testing

E2E 基盤なし（`e2e_test_command` 未設定）— 対象外。

## Manual Testing (E2E Not Possible)

- [ ] M1: TS1 / TS2 の `/tmp` 前後比較 — 実行前に `/tmp` 直下のエントリ一覧を記録し、各テストコマンドの成功完走後に再取得して差分を確認する（`emterm-*` パターンの新規エントリがないこと）
- [ ] M2: TS5 — 依存ファイル（`src-tauri/Cargo.toml` / `Cargo.lock` / `package.json`）の差分に新規依存がないことを確認する
- [ ] M3: TS6 — 統合差分の全ハンクが 4 対象ファイルのテスト専用モジュール内にあることを確認する

## Performance / Security Verification

該当なし（SPEC.md セキュリティ考慮: 削除対象はテスト自身が構築した一時パスに限られ、外部由来のパスは削除しない — TS6 / 各タスク AC で担保）。

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios | 6 (TS1〜TS6) | 2 (TS3, TS4) | 0 | 4 (TS1, TS2, TS5, TS6) |
| Success criteria | 7 (SC-1〜SC-7) | — | — | — |
