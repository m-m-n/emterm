# Verification Document: tmux-sockets-discover-flake

## Overview

**Feature**: tmux-sockets-discover-flake
**SPEC.md**: `feature-docs/tmux-sockets-discover-flake/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/tmux-sockets-discover-flake/IMPLEMENTATION.md`

前提: 対象モジュールは `#[cfg(unix)]` 限定であり、検証は Unix（Linux）環境で行う。Windows のテスト経路は影響を受けない。

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: 該当なし（テストモジュールのみの変更で、新規本番コードはない）

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | ストレス: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib tmux_sockets` を、既定の並列テストスレッドのまま 60 回以上繰り返す（同モジュール内の spawn 系テストが fork の多い兄弟として並走する） | 全反復で失敗 0 件 | Stress（反復実行） |
| TS2 | stale なソケットファイル（socket 型でディスク上に残存、listener 不在）が `discover_in` の結果から除外され、live なソケットのみが返る | `discover_returns_only_the_live_socket` の既存アサーションが維持されたまま成功する | Unit |
| TS3 | フルスイート: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` | exit code 0（全テスト成功） | Integration |

実行上の注意:

- TS1 の反復は既定の並列スレッド構成のまま行う。`--test-threads=1` を付けると NFR1 の検証にならない。
- `tabs.rs` の replay テストは本 feature と無関係に、並列実行時に非決定的に落ちることが既知（`test/README.md`）。TS3 で tabs 系テストのみが落ちた場合は本 feature の失敗と混同せず、`test/README.md` の手順に従い再実行して判断する。

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Static analysis: プロジェクト標準の追加コマンドなし

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | `discover_returns_only_the_live_socket` が並列ストレス 60 回以上で失敗 0 件 | TS1 |
| SC-2 | 既存の discover / enumerate テストの stale 除外の検証意図が弱められていない | TS2 + MT-1（差分確認） |
| SC-3 | `src-tauri` の `cargo test --lib` フルスイートが引き続き成功する | TS3 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS3 |
| FR2 | task0001 | TS2, TS3 |
| NFR1 | task0001 | TS1（既定並列のまま、スレッド数指定なしで実施すること自体が検証） |
| NFR2 | task0001 | TS3 + MT-1（差分が `#[cfg(test)]` 内に限られること） |

## E2E Testing

該当なし（プロジェクトに E2E フレームワークはなく、`workflow.yaml` の `e2e_test_command` も空）。

## Manual Testing (E2E Not Possible)

- [ ] MT-1: 差分レビュー — 変更が `src-tauri/src/tmux_sockets.rs` の `#[cfg(test)] mod tests` 内に限られ、本番の `discover_in` / `probe_unix_socket`（および `#[cfg(test)]` 外の一切）に変更がないことを確認する（NFR2）。あわせて stale fixture 構築部に fork 窓レースの機序と旧パターンへ戻してはいけない理由のコメントがあることを確認する（task0001 AC-6）。

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Test scenarios | 3 (TS1–TS3) | 3 | 0 | 0 |
| Code quality (format) | 1 | 1 | 0 | 0 |
| Manual | 1 (MT-1) | 0 | 0 | 1 |
