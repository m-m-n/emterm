# Verification Document: agent-badge-emoji

## Overview

**Feature**: agent-badge-emoji / **SPEC.md**: `feature-docs/agent-badge-emoji/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/agent-badge-emoji/IMPLEMENTATION.md`

## Build Verification

- main（GUI ビルド）:

```
CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml
```

- cli（feature gate 確認）:

```
CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features
```

- Expected: いずれも exit code 0、エラーなし

## Test Verification

- Command:

```
CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib
```

- カバレッジ目標: 数値計測ツールは未導入。変更対象の判定ロジック（`badge_presentation()` / `resolve_badge_render_mode()`）について、4 状態 × unseen/seen の 8 組み合わせとフォールバック分岐（テクスチャ有/無 × 塗り/リング）を分岐網羅する
- 既知の基線: `tabs.rs` の replay テストは並列実行で非決定的に落ちることがある（main 基線の既知事象）。失敗が出た場合は main 基線との差分で合否を判定する

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | （SPEC TS1）`badge_presentation()` のテーブルテスト — 4 状態 × unseen/seen の 8 組み合わせを網羅 | working=⚡/⚡、idle=💤/💤、blocked=❓/❔、done=✅/💤 の期待クラスタに解決される | Unit |
| TS-2 | （SPEC TS2）`resolve_badge_render_mode()` — blocked/done の Emoji 表示解決でテクスチャ不在のとき | 円フォールバックになり空白にならない。フォールバック円が unseen=塗り / seen=リングを保つ | Unit |
| TS-3 | （SPEC TS3）done+seen の Emoji クラスタの同一性 | `IDLE_BADGE_EMOJI` と同一文字列である | Unit |
| TS-4 | （SPEC TS4）既存の tab_bar / mux_sidebar テストを含む `--lib` スイート全体 | グリーン（main 基線比） | Regression |

## Code Quality Verification

- Format: 未設定（workflow.yaml project.components の format_command は空）
- Static analysis: 未設定

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | blocked が unseen で ❓ (U+2753)、seen で ❔ (U+2754) を表示する | TS-1 + MT-1 |
| SC-2 | done が unseen で ✅ (U+2705)、seen で 💤 (U+1F4A4、idle と同一クラスタ) を表示する | TS-1, TS-3 + MT-1 |
| SC-3 | working / idle の表示は変更されていない | TS-1, TS-4 + MT-1 |
| SC-4 | 絵文字テクスチャが取得できないときは既存の円フォールバックが働き、空白バッジにならない | TS-2 |
| SC-5 | タブバーと mux サイドバーで同じ表示になる（同じ判定関数の共有を維持） | TS-4 + MT-2 + レビューで判定点の単一性を確認 |
| SC-6 | 4 状態 × unseen/seen の全組み合わせを網羅するユニットテストがある | TS-1 の存在確認 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1 |
| FR2 | task0001 | TS-1, TS-3 |
| FR3 | task0001 | TS-1, TS-4 |
| FR4 | task0001 | TS-2 |
| FR5 | task0001 | TS-4（+ レビューで `badge_presentation()` の単一判定点を確認） |
| FR6 | task0001 | TS-1, TS-3 |
| NFR1 | task0001 | TS-4（集約・unseen の既存テストが無変更でグリーン） |
| NFR2 | task0001 | TS-4（done 内部状態に触れる既存テストが無変更でグリーン） |
| NFR3 | task0001 | TS-4（新規テストが `--lib` で実行されること自体が配置規約の証明） |

## Manual Testing (E2E Not Possible)

E2E 基盤は本プロジェクトに存在しないため、実表示の確認は手動で行う。

- [ ] MT-1: 実機でタブバーのバッジを 4 状態 × unseen/seen で目視し、表示テーブルどおりの絵文字（⚡ / 💤 / ❓ / ❔ / ✅）が表示される。done+seen は idle と同一表示になる（SPEC A4: 意図どおり）
- [ ] MT-2: mux サイドバーの同じ状態のバッジがタブバーと同一の表示になる

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test Scenarios (TS-1〜TS-4) | 4 | 4 | 0 | 0 |
| Manual (MT-1〜MT-2) | 2 | 0 | 0 | 2 |
