# Verification Document: windows-skk-ime-hang

## Overview

**Feature**: windows-skk-ime-hang /
**SPEC.md**: `feature-docs/windows-skk-ime-hang/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/windows-skk-ime-hang/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: 変更した IME 経路の全 FR がユニットテストで検証されること

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | notify_focus(true) 記録後 flush まで無コール、flush 後 set_ime_allowed(true) 1 回 | flush 前 mock コール 0 / flush 後 [true] | Unit |
| TS-2 | flush 前の複数 notify_cursor_rect の合体 + 同一 rect dedup | 最後の rect 1 コールのみ、同一 rect 再 flush は無コール | Unit |
| TS-3 | 同一ターンの allow + cursor rect の flush 順序 | allow → cursor の順 | Unit |
| TS-4 | 記録なし flush | mock コール 0 | Unit |
| TS-5 | pending 要求ありの Drop | 要求破棄 + set_ime_allowed(false) 1 回 | Unit |
| TS-6 | 構築時 enable の記録と初回 flush | flush 前 [] / flush 後 [true] | Unit |
| TS-7 | 既存ゲート述語・イベント変換テストの無変更パス | 既存テスト全パス（明示 flush 追随のみ） | Unit |
| TS-8 | 二重 Enabled / 二重 Disabled の warn latch | 初回のみ発火、2 回目は latch | Unit |
| TS-9 | 統合: 既存スイート全体 | `cargo test --lib` exit 0 | Integration |

## Code Quality Verification

- Format: なし（format_command 未設定 — rustfmt 未インストール環境）
- Static analysis: `cargo check`（Build Verification と同一コマンド）

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | window_event 配送から request_ime_update へ同期到達する経路が無い | `BridgeWindow` 副作用メソッドの呼び出し元が flush 実装と Drop のみであることを grep / コードレビューで確認 |
| SC-2 | FR テストシナリオ（TS-1〜TS-8）が Linux ホストでパス | `cargo test --lib` |
| SC-3 | 既存 IME テストが意味論無変更でパス | `cargo test --lib`（diff レビューで assert 列の不変を確認） |
| SC-4 | ビルド green | `cargo check` |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-6 + SC-1（構造検証） |
| FR2 | task0001 | TS-2, TS-3, TS-4 |
| FR3 | task0001 | TS-5 |
| FR4 | task0001 | TS-8 |
| FR5 | task0001 | TS-7, TS-9 |
| NFR1 | task0001 | 設計検証: flush が about_to_wait（同一ループターン内）で呼ばれる配線を diff レビューで確認 |
| NFR2 | task0001 | Cargo.toml に依存追加が無いことを diff で確認 |
| NFR3 | task0001 | TS-1〜TS-8 が Linux ホストの `cargo test --lib` で完結することを確認 |

## Manual Testing (E2E Not Possible)

- [ ] MT-1（実施不可・フィールド確認に委ねる）: Windows + CorvusSKK で
  変換モード中に `l` を押し、応答なしにならないこと。ビルド環境では検証
  不可能（SPEC Assumptions A1）。verify フェーズでは評価対象外とし、
  リリース後のフィールドレポート（FR4 の warn ログ）で確認する。
- [ ] MT-2（任意・Linux）: fcitx5/ibus で日本語入力し、preedit 表示・確定・
  候補ウィンドウ位置が従来どおりであること（Linux 非退行の実機スポット
  チェック）。

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Unit/Integration | 9 (TS-1〜TS-9) | 9 | 0 | 0 |
| Structural (SC-1) | 1 | 0 | 0 | 1 |
| Manual | 2 (MT-1, MT-2) | 0 | 0 | 2（MT-1 はフィールド委任） |
