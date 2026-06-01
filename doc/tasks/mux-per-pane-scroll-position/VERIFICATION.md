# Verification Document: Per-Pane Scroll Position in mux

## Overview
**Feature**: mux-per-pane-scroll-position
**SPEC.md**: `doc/tasks/mux-per-pane-scroll-position/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/mux-per-pane-scroll-position/IMPLEMENTATION.md`

## Build Verification
- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"`
- Expected: exit code 0, no type errors
- **Actual**: exit code 0 (`tsc --noEmit` 型エラー無し)

## Test Verification
- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"`
- Coverage target: 既存 scroll-pin / mux pane state テストに新規ケースを追加。新規ロジックの分岐を網羅
- **Actual**:
  - 新規 `src/terminal/state-mux-pane-scroll.test.ts` — 10 pass / 0 fail (TS-1〜TS-6 を網羅)
  - 回帰確認 `scroll-pin.test.ts` (11) + `mux-window-manager.test.ts` (10) + 新規 (10) = 31 pass / 0 fail

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | pane を退避するとスナップショットに現在のスクロール位置が記録される | スナップショットのスクロール位置 = 退避時の renderer のスクロール位置 | Unit | ✅ `captureScrollState` テスト pass |
| TS-2 | pane を復元するとスナップショットのスクロール位置が renderer に反映される | renderer のスクロール位置 = 保存値 | Unit | ✅ `applyScrollState` テスト pass |
| TS-3 | 保存状態の無い (新規/初訪問) pane は最下部で初期化される | スクロール位置 = 0 | Unit | ✅ `resetScrollState` テスト pass |
| TS-4 | scroll-pin ベースラインも pane ごとに退避・復元される | 復元後のベースライン = 保存値。背景由来のスクロールバック増加に既存補正が復元ベースライン起点で適用 | Unit / Integration | ✅ capture/apply/reset でベースラインも検証 pass |
| TS-5 | 切替往復 (A→B→A) で A のスクロール位置が復元される | A のスクロール位置 = 手順最初の値 | Integration | ✅ round trip テスト pass |
| TS-6 | 通常タブ (mux 非使用) のスクロール位置分離が壊れていない | 各タブが独立 renderer のスクロール位置を維持 | Regression | ✅ 独立 renderer 波及なしテスト pass (helper は mux 経路限定) |

## Code Quality Verification
- Format: (プロジェクトに自動フォーマッタ設定なし — 既存コードスタイルに合わせる)
- Static analysis: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"`

## File Structure Verification

### Files to Modify
- [x] `src/terminal/state-mux-pane.ts` — `MuxPaneGridState` にスクロール位置 + scroll-pin ベースラインフィールドを追加
- [x] `src/terminal/canvas-renderer.ts` — scroll-pin ベースラインの取得/設定アクセサを公開 (`getScrollPinBaseline` / `setScrollPinBaseline`)
- [x] `src/terminal/renderer-interface.ts` — ベースライン API をインターフェースに宣言
- [x] `src/terminal-app/mux/mux-window-manager.ts` — save/restore/fresh 各経路でスクロール位置 + ベースラインを扱う

### Files Created
- `src/terminal/state-mux-pane-scroll.ts` — capture/apply/reset の純粋 helper (renderer 最小インターフェース依存)
- `src/terminal/state-mux-pane-scroll.test.ts` — TS-1〜TS-6 の単体/結合テスト

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | pane 切替でスクロール位置が pane ごとに保持される | TS-1, TS-2, TS-5, E2E |
| SC-2 | pane に戻ると前回のスクロール位置が復元される | TS-5, E2E |
| SC-3 | 背景 pane の出力は既存 scroll-pin 挙動に従う | TS-4, Manual |
| SC-4 | 通常タブのスクロール位置分離に回帰がない | TS-6, Manual |

### Functional Requirements Coverage
| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 | Phase 1 | TS-1, TS-2, TS-3, TS-5 |
| FR2 | Phase 2 | TS-4 |
| NFR1 | Phase 1 | スクロール位置は単一数値の退避・復元 (性能影響なしを設計で担保) |
| NFR2 | Phase 1/2 | TS-6 (回帰), Manual |

## E2E Testing
- Run command: `./scripts/run-e2e-docker.sh test`
- [ ] 既存 E2E テストが回帰なく通る
- [ ] pane A でスクロールアップ → pane B へ切替 (B 自身の位置で表示) → pane A に戻る (A の位置が復元)

## Manual Testing (E2E Not Possible)
- [ ] 背景 pane に出力が届いたとき、スクロールアップ中の pane が位置を維持し、最下部の pane は追従する (実機での主観確認)

## Performance Verification (if applicable)
- NFR1: スクロール位置・ベースラインの退避/復元は pane 切替経路上の単一数値操作であり、体感可能な遅延を生じないこと

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit/Integration | 6 | 5 (TS-1〜TS-5) | — | — |
| Regression | 1 | 1 (TS-6) | — | 1 |
| E2E | 2 | — | 2 | — |
| Manual | 1 | — | — | 1 |
