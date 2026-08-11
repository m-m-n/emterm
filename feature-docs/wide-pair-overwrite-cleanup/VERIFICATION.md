# Verification Document: wide-pair-overwrite-cleanup

## Overview

**Feature**: wide-pair-overwrite-cleanup
**SPEC.md**: `feature-docs/wide-pair-overwrite-cleanup/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/wide-pair-overwrite-cleanup/IMPLEMENTATION.md`

## Build Verification

- コマンド（term_core コンポーネント / GUI 込みビルド確認）:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- コマンド（workspace コンポーネント / CLI-only feature gate 確認）:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- 期待結果: いずれも exit code 0、エラーなし

## Test Verification

- コマンド（term_core）:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- コマンド（workspace）:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- カバレッジ数値目標: 設けない（プロジェクトに測定基盤なし）。代わりに TS1〜TS4 / TS6 / TS7 が新規ユニットテストとして存在し全緑であることを完了条件とする。
- 既知の注意: tabs.rs の replay テストは並列実行で非決定的に落ちることがある（`--test-threads=1` で安定）。term_core 側の失敗と区別すること。

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | (FR1) P3 再現: ⏭️（U+23ED + VS16）の wide ペアを書いた後、base 位置に幅1 文字を上書き | col+1 の旧 spacer が空白（`get_cell_char` = 空白 1 文字、`get_cell_width` = 1） | Unit |
| TS2 | (FR2) P4 再現: spacer 位置に幅1 文字を上書き | col-1 の base が空白（幅1）になり幅2 グリフが残らない | Unit |
| TS3 | (FR1-FR4) P5 再現: ⏭️ を含む行を 1 桁ズラして書き直し（フレーム間の列幅変化を模擬） | 旧フレームの残骸（孤児 spacer / 孤児 base）が行内に残らない | Unit |
| TS4 | (FR4, NFR1) チャンク分割耐性: U+23ED と VS16 を別チャンクで流す | 遡及 widen が正常（P1/P2 の正常系維持） | Unit |
| TS5 | (FR1-FR4) 実機確認: ⏭️ を含むテーブルの Claude Code ストリーム描画 | 罫線ズレ・文字重なりが再現せず、Ctrl+L 後の残留もない | Manual |
| TS6 | (FR3) 連鎖掃除エッジケース: 幅2 書き込みの col+1 が別ペアの base | その spacer（col+2）も空白化される | Unit |
| TS7 | (FR5) ICH/DCH/ECH のペア分断境界（消去開始/終了境界、シフト境界、右端押し出し） | 残存する半分が空白化され、孤児 spacer / 孤児 base が行内に残らない | Unit |
| TS8 | (NFR1) 回帰: 既存 term_core スイート + workspace スイート | 全緑（wide ペア非関与の経路の挙動不変） | Automated |
| TS9 | (NFR4) ASCII 高速パスの性能ガード | handle_print_ascii の diff で、wide 非関与時（旧セル幅 1）の追加コストが分岐 1 回に留まる（IMPLEMENTATION.md D4） | Manual (review) |
| TS10 | (NFR3) 修正の局所性 | 統合 diff が crates/term_core 配下に閉じている | Manual (review) |

## Code Quality Verification

- Format: 規約コマンドなし（workflow.yaml format_command 未設定）。crate 全体 fmt は走らせず、変更ファイルのみ既存スタイルに合わせる。
- Static analysis: 規約コマンドなし。

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | 幅2 base への幅1 上書きで col+1 の旧 spacer が空白化（P3 / FR1） | TS1 |
| SC-2 | spacer への上書きで col-1 の base が空白化（P4 / FR2） | TS2 |
| SC-3 | placeholder 作成時、col+1 が別ペア base ならその spacer（col+2）も空白化（FR3） | TS6 |
| SC-4 | widen_after_merge の spacer 作成箇所にも同規則適用（FR4） | TS3, TS4 |
| SC-5 | P3 / P4 / P5 の再現手順がユニットテスト化され回帰ガードになる | TS1, TS2, TS3 の実装確認 |
| SC-6 | ⏭️ を含むテーブルのストリーム描画で乱れが再現しない | TS5（実機） |
| SC-7 | wide ペア非関与の通常書き込み経路の挙動不変（NFR1） | TS8 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS3 |
| FR2 | task0001 | TS2, TS3 |
| FR3 | task0001 | TS3, TS6 |
| FR4 | task0001 | TS3, TS4 |
| FR5 | task0002 | TS7 |
| NFR1 | task0001, task0002 | TS4, TS8 |
| NFR2 | task0001, task0002 | TS1, TS2, TS7（相方空白化の挙動そのものが xterm / Alacritty / WezTerm の慣行との整合点） |
| NFR3 | task0001, task0002 | TS10 |
| NFR4 | task0001 | TS9 |

## E2E Testing

E2E 基盤は存在しない（SPEC.md）。自動 E2E は省略し、下記 Manual Testing で代替する。

## Manual Testing (E2E Not Possible)

- [ ] TS5: リリースビルドの eMterm 上で、⏭️ を含むテーブルを Claude Code にストリーム描画させ、罫線ズレ・文字重なりが再現しないこと、Ctrl+L 後に残骸が残らないことをユーザーが目視確認する。
- [ ] TS9: handle_print_ascii の diff をレビューし、wide ペア非関与時の追加コストが旧セル幅の判定分岐 1 回に留まることを確認する。
- [ ] TS10: 統合 diff（base_commit..parent_branch）が crates/term_core 配下に閉じていることを確認する。

## Performance Verification

- NFR4: TS9 で確認（ASCII 高速パスの通常経路に測定可能な追加コストを入れない設計）。専用ベンチマークは要件にないため実施しない。

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit（新規） | TS1, TS2, TS3, TS4, TS6, TS7 | 6 | 0 | 0 |
| Regression | TS8 | 1 | 0 | 0 |
| Manual / Review | TS5, TS9, TS10 | 0 | 0 | 3 |
| 合計 | 10 | 7 | 0 | 3 |
