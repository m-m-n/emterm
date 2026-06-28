# Verification Document: SGR Reverse Default-Color Swap

## Overview

**Feature**: SGR Reverse Default-Color Swap
**SPEC.md**: `doc/tasks/sgr-reverse-default-color-swap/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/sgr-reverse-default-color-swap/IMPLEMENTATION.md`

## Build Verification

- Library build (test cycle):
  - Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
  - Expected: exit code 0、警告 0（既存比）
  - **Actual**: `cargo test --lib` 実行時に同一ターゲットでビルド成功（`Finished test profile ... 6.47s`）。test 経路で `check` 相当を通過
- CLI-only feature gate check:
  - Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  - Expected: exit code 0
  - **Actual**: exit 0 (`Finished dev profile [unoptimized + debuginfo] target(s) in 0.42s`)
- Release build はユーザー指示時のみ実施（本タスク範囲外、CLAUDE.md `feedback_no_unsolicited_build.md` 準拠）。今回もスキップ

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0、新規 6 ケース + 既存 `bold_brighten_packed_*` / `packed_to_egui_*` がすべて成功
- Coverage target: 修正関数 `resolve_cell_style_from_packed` の reverse / selection / bold-brighten 分岐を網羅

### Actual Results

- **新規ユニットテスト (TS-1〜TS-6)**: 6/6 成功
  - `render::tests::reverse_with_both_default_swaps_to_theme_bg_and_fg` ... ok (TS-1)
  - `render::tests::reverse_with_indexed_fg_default_bg_swaps` ... ok (TS-2)
  - `render::tests::reverse_with_truecolor_swaps` ... ok (TS-3)
  - `render::tests::reverse_then_selection_cancels` ... ok (TS-4)
  - `render::tests::no_reverse_no_selection_uses_theme_defaults` ... ok (TS-5)
  - `render::tests::reverse_with_bold_brighten_promotes_perceived_fg` ... ok (TS-6)
- **render モジュールリグレッション**: 186/186 成功（`cargo test --lib render::`）
  - 既存 `bold_brighten_packed_*` / `packed_to_egui_*` 全て pass
- **lib 全体テスト**: 2020/2026 成功、6 失敗
  - 失敗は全て `tabs::tests::*`（off-thread replay worker / mux_group の並列依存系）。本タスク修正前の baseline（`git stash` で render 変更を退避後の HEAD）でも同じ 4-6 件が再現するフレーキーな既知問題。MEMORY.md `project_test_execution_notes` および `feedback_tdd_scope` に既出。本タスクスコープ外と確定
- TS-9 (regression baseline): `render::tests::*` 186/186 + 既存 `bold_brighten_packed_*` / `packed_to_egui_*` 全 pass で達成

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `reverse_with_both_default_swaps_to_theme_bg_and_fg` — packed_fg=DEFAULT、packed_bg=DEFAULT、flags=STYLE_REVERSE、selected=false | `fg == rgb_to_egui(theme.bg)` かつ `bg == rgb_to_egui(theme.fg)` | Unit |
| TS-2 | `reverse_with_indexed_fg_default_bg_swaps` — packed_fg=indexed(1) 赤、packed_bg=DEFAULT、flags=STYLE_REVERSE | `fg == rgb_to_egui(theme.bg)`、`bg == indexed(1) の palette16 解決色` | Unit |
| TS-3 | `reverse_with_truecolor_swaps` — packed_fg=truecolor(R1,G1,B1)、packed_bg=truecolor(R2,G2,B2)、flags=STYLE_REVERSE | `fg = Color32(R2,G2,B2)`、`bg = Color32(R1,G1,B1)` | Unit |
| TS-4 | `reverse_then_selection_cancels` — packed_fg=DEFAULT、packed_bg=DEFAULT、flags=STYLE_REVERSE、selected=true | `fg == rgb_to_egui(theme.fg)`、`bg == rgb_to_egui(theme.bg)`（XOR で打ち消し） | Unit |
| TS-5 | `no_reverse_no_selection_uses_theme_defaults` — flags=0、selected=false、両 DEFAULT（コントロール） | `fg == rgb_to_egui(theme.fg)`、`bg == rgb_to_egui(theme.bg)` | Unit |
| TS-6 | `reverse_with_bold_brighten_promotes_perceived_fg` — packed_fg=DEFAULT、packed_bg=indexed(1) 赤、flags=STYLE_REVERSE\|STYLE_BOLD、`theme.bold_brightens_ansi_colors=true` | 最終 `fg`（= 描画される文字色 = reverse 後の perceived foreground）が indexed(9) bright red の解決色、最終 `bg` が `rgb_to_egui(theme.fg)`（DEFAULT が reverse 用 fallback で解決される）になる | Unit |
| TS-7 | Manual: `printf '\e[7mREVERSE\e[0m NORMAL\n'` を eMterm で実行 | `REVERSE` 区間が反転表示、`NORMAL` 区間が通常表示。WezTerm の表示と一致 | Manual |
| TS-8 | Manual: `printf '\e[31;42m\e[7mX\e[0m Y\n'` を eMterm で実行 | `X` が fg=green / bg=red、`Y` は通常スタイル | Manual |
| TS-9 | 既存ユニットテスト全体（特に `bold_brighten_packed_*` / `packed_to_egui_*`）のリグレッション確認 | exit code 0、失敗 0 | Unit (regression) |

## Code Quality Verification

- Format: `cargo fmt` 等のクレート全体実行は行わない（CLAUDE.md `feedback_no_crate_wide_cargo_fmt.md` 準拠）。修正範囲は既存スタイルに合わせて手書きで揃える
  - **Actual**: `cargo fmt --manifest-path src-tauri/Cargo.toml -- src-tauri/src/render/mod.rs --check` exit 0（差分なし。PostToolUse hook 経由の整形済み）
- Static analysis: 本タスク追加なし（`cargo check` の警告監視で十分）
  - **Actual**: `cargo check --no-default-features` exit 0、警告増分なし

## File Structure Verification

### Files to Create

なし

### Files to Modify

- [x] `src-tauri/src/render/mod.rs`
  - [x] `resolve_cell_style_from_packed` 内、`packed_to_egui` 呼び出し直前に `(fg_fallback, bg_fallback)` を reverse に応じて選ぶブロックを追加（L1234-1238）。`packed_to_egui` の第2引数および `unwrap_or_else` の RGB 値をその変数経由に差し替え（L1241-1244）
  - [x] 既存の packed-level reverse / bold-brighten コメントを役割分担（packed swap = bold-brighten 可視化用 / fallback swap = `None` 返却時の救済用）の説明に更新（layer 1 / layer 2 ラベル付け）
  - [x] `#[cfg(test)] mod tests` に TS-1〜TS-6 を追加（L1556-1666）

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1-FR4 が仕様通りに実装されている | コードレビュー + TS-1〜TS-6 通過 |
| SC-2 | 新規および既存の `cargo test --lib` がすべて通る | `cargo test --lib` exit 0 |
| SC-3 | `printf '\e[7mREVERSE\e[0m NORMAL\n'` の手動再現で REVERSE 区間が反転表示される | TS-7 |
| SC-4 | `bold_brighten_packed_*` および既存レンダリングテストにリグレッションがない | TS-9 |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 — Reverse covers both DEFAULT and explicitly-colored cells | Phase 1 | TS-1 / TS-2 / TS-3 |
| FR2 — Bold-brighten ordering preserved | Phase 1 | TS-6 |
| FR3 — Selection swap unchanged | Phase 1 | TS-4 |
| FR4 — Dim / hidden ordering unchanged | Phase 1 | TS-5（コントロール）+ TS-9（既存リグレッション） |
| NFR1 — No rendering performance regression | Phase 1 | 設計上 O(1) の `std::mem::swap` 追加のみ。実測なし（NFR1 合議） |
| NFR2 — WezTerm / xterm / alacritty 互換 | Phase 1 | TS-7 / TS-8（manual side-by-side） |
| NFR3 — Scope: no term_core changes, DECSCNM out of scope | Phase 1 | 変更ファイル一覧（`src-tauri/src/render/mod.rs` のみ）で担保 |

## E2E Testing

本タスクの修正に対する covering E2E は既存ハーネス（`./scripts/run-e2e-docker.sh`）に存在しない。SPEC §"E2E Tests" 方針に従い、新規 E2E は追加しない。

- [ ] 既存 E2E スイートを走らせる場合の確認: 失敗が増えない（任意・本タスクの必須要件ではない）

### Existing E2E Regression (Phase 3.8)

- `sdd.yaml.e2e_test_command` 未設定。本タスクの修正は `resolve_cell_style_from_packed` のピュア関数で、PTY / IPC / Docker 経路に触れないため Phase 3.8 はスキップ

## Manual Testing (E2E Not Possible)

色味の最終確認は人間判断で行う。release build を起動済みの eMterm 内シェルで以下を実行する。

- [ ] **TS-7** `printf '\e[7mREVERSE\e[0m NORMAL\n'` — `REVERSE` 区間が反転表示・`NORMAL` 区間が通常表示。WezTerm との side-by-side で同等の見た目。
- [ ] **TS-8** `printf '\e[31;42m\e[7mX\e[0m Y\n'` — `X` が fg=green / bg=red、`Y` がデフォルトスタイル。
- [ ] reverse セルを範囲選択しても、選択ハイライトが通常セルと同等に見える（FR3 / UC03 の目視確認）。

## Performance Verification

- O(1) のスワップ追加のみ。実測ベンチは取らない（SPEC NFR1 合議）。

## Security Verification

- 該当なし（純粋なレンダリングパスの色解決変更。入力解析・IPC・ファイル I/O への影響はない）

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit (新規) | TS-1〜TS-6 | 6 | 0 | 0 |
| Unit (regression) | TS-9 | 1 | 0 | 0 |
| Manual repro | TS-7 / TS-8 | 0 | 0 | 2 |
| **Total** | **9** | **7** | **0** | **2** |
