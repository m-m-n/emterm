# Verification Document: Pin Viewport When Scrolled Up

## Overview

- **Feature**: pin-viewport-when-scrolled-up
- **SPEC.md**: `doc/tasks/pin-viewport-when-scrolled-up/SPEC.md`
- **IMPLEMENTATION.md**: `doc/tasks/pin-viewport-when-scrolled-up/IMPLEMENTATION.md`

`scrollOffset > 0` のときの PTY scrollback 増加に対する補正ロジックを追加する変更を、ビルド/型/単体/E2E/SPEC 要件のレベルで検証する計画書。

## Build Verification

- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"`
- Expected: exit code 0、型エラーなし

ホスト実行が許可されている場合の代替: `bun run typecheck`

### Implementation Result

- Command executed: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cd /app && bun run typecheck"`
- Result: exit code 0、型エラー 0 件
- 確認日: 2026-05-23（sdd.4-implement 実行時）

## Test Verification

- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"`
- Coverage target: `scroll-pin.ts` は 100%（純粋関数のため）

### Implementation Result

- Command executed: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cd /app && bun test"`
- 結果: 2343 pass / 0 fail / 17 todo / 6180 expect() calls (108 files)
- `scroll-pin.test.ts` 単体: 11 pass / 0 fail (TS-1〜TS-6 を全カバー、TS-3 は 3 ケースに分割、TS-4/5/6 も境界補強ケースを追加)
- 既存テストへの regression: なし

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type | 対応場所 |
|----|----------|-----------------|-----------|----------|
| TS-1 | `scrollOffset=5, prevSbLen=10, currSbLen=13` の補正 | `nextScrollOffset=8`, `nextPrevSbLen=13` | Unit | `src/terminal/scroll-pin.test.ts` |
| TS-2 | `scrollOffset=0, prevSbLen=10, currSbLen=13` の補正 | `nextScrollOffset=0`, `nextPrevSbLen=13`（変更なし） | Unit | `src/terminal/scroll-pin.test.ts` |
| TS-3 | `scrollOffset=95, prevSbLen=100, currSbLen=100`（capacity-cap で増えない場合は変更なし）。続けて `currSbLen=100 のまま scrollOffset+Δ > currSbLen` 相当のシナリオで `nextScrollOffset=currSbLen` にクランプ | クランプが scrollbackLength で停止 | Unit | `src/terminal/scroll-pin.test.ts` |
| TS-4 | `prevSbLen=50, currSbLen=0`（clear: ESC[3J 等）| `nextScrollOffset=scrollOffsetそのまま`、`nextPrevSbLen=0` に再ベースライン化、次回 growth で誤補正なし | Unit | `src/terminal/scroll-pin.test.ts` |
| TS-5 | alt-screen 入退場時に `state.getScrollbackLength()` は primary buffer の値を返し続けるため Δ===0、補正なし。退出後の primary growth で FR1 が機能（`prev===curr` を渡したとき `scrollOffset` が変わらないことを純粋関数レベルで検証） | 入退場とも `nextScrollOffset === scrollOffset`、退出後の growth ケースで FR1 通り増加 | Unit | `src/terminal/scroll-pin.test.ts` |
| TS-6 | Δ===0 ケース（partial DECSTBM scroll region active 時は WASM 側で scrollback 増加が起きない既存挙動、および clear 直前の同値ケースの no-op 動作）| `nextScrollOffset=scrollOffsetそのまま` | Unit | `src/terminal/scroll-pin.test.ts`（境界確認）。partial scroll region 自体が scrollback に push しないことは WASM 側の既存挙動として保証されており、本機能の責務外 |
| TS-7 | E2E: scrollback を伸ばしたあとスクロールアップ、再度 burst を流す | 固定 display row のテキストがバースト前後で等しい | E2E | `e2e-tests/specs/scroll-pin.e2e.js` |

## Code Quality Verification

- Format / typecheck: `bun run typecheck`
- 静的解析: 既存リンタ設定に従う（追加の lint 設定変更は行わない）

### Implementation Result

- typecheck: pass (exit 0)
- 追加ファイルは既存ファイルと同じ tab indent / `bun:test` import スタイルに準拠

## File Structure Verification

### Files to Create
- [x] `src/terminal/scroll-pin.ts` - `computeAdjustedScrollOffset` 純粋関数の定義（47 行）
- [x] `src/terminal/scroll-pin.test.ts` - TS-1〜TS-6 の単体テスト（91 行、11 ケース）
- [x] `e2e-tests/specs/scroll-pin.e2e.js` - TS-7 の E2E spec（145 行）

### Files to Modify
- [x] `src/terminal/canvas-renderer.ts` - `prevScrollbackLength` フィールド追加、`adjustScrollOffsetForGrowth` 追加、`render()` / `forceRender()` 冒頭での呼び出し追加（差分 +38 行、合計 1422 行）

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | スクロールアップ中の PTY 出力で見ている行が動かない | TS-1（unit）+ TS-7（E2E）|
| SC-2 | `scrollOffset === 0` での追従挙動が保たれる | TS-2（unit）+ 既存 `canvas-renderer.test.ts` の `getVisibleLines` テスト |
| SC-3 | scrollback 上限超過時にクランプし最上部固定 | TS-3（unit）|
| SC-4 | 既存公開 API シグネチャ不変 | `bun run typecheck` + IPC/利用箇所の grep |
| SC-5 | alt-screen / clear / pane 切替で誤動作しない | TS-4 + TS-5（unit）|
| SC-6 | partial DECSTBM scroll region で本機能が発火しない | TS-6（unit; Δ===0 ケース）|

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 (Pin offset on PTY scrollback growth) | Phase 1 + Phase 2 | TS-1（unit）+ TS-7（E2E） |
| FR2 (Follow-tail when offset is zero) | Phase 1 + Phase 2 | TS-2（unit）+ 既存テスト |
| FR3 (Clamp at scrollback top) | Phase 1 + Phase 2 | TS-3（unit） |
| FR4 (User-initiated scroll unchanged) | Phase 2 | コードレビュー: `scrollUp` / `scrollDown` / `setScrollOffset` は `prevScrollbackLength` に触らない。既存 keyboard handler テストは regression が無いことを確認 |
| FR5 (Alt-screen unaffected) | Phase 1 + Phase 2 | TS-5（unit） |
| FR6 (Partial scroll region unaffected) | Phase 1 + Phase 2 | TS-6（unit; partial region 時は scrollback が増えないため Δ===0 で no-op） |

### Non-Functional Requirements Coverage

| Requirement | Verification |
|-------------|--------------|
| NFR1 (Performance) | `bun test` で既存ベンチ系テスト（`performance.test.ts`）が pass。E2E で既存スループット系（`visibility-throughput-bench` 等）に regression が無いことを sdd.6 verify で確認 |
| NFR2 (Compatibility) | `bun run typecheck` で renderer-interface.ts のシグネチャ不変を強制的に確認 |

## E2E Testing

- 実行コマンド: `./scripts/run-e2e-docker.sh test scroll-pin.e2e.js`
- 実行時期: sdd.6 verify（TDD ループ中は実行しない）

E2E 検証項目:
- [ ] TS-7: scrollback を伸ばしたあとスクロールアップ -> 再度 burst -> 固定 display row のテキストがバースト前後で等しい
- [ ] 既存 E2E スイート（`./scripts/run-e2e-docker.sh test`）に regression が無い

## Manual Testing (E2E Not Possible)

- [ ] vim / less などの alt-screen アプリ使用中に本機能が誤発火しないことを目視確認（任意。FR5 を補強）
- [ ] スクロールバーやマウスホイールでの体感確認（NFR1 の主観評価補強。任意）

## Performance Verification

- 既存 `performance.test.ts` および E2E の throughput bench で regression が無いこと
- 追加処理は1フレームあたり「`getScrollbackLength()` 1回 + 整数比較数回 + フィールド代入2回」のみ。NFR1 「per-frame work added beyond a comparison and a counter update」と一致

### 合否基準

- `bun test` の既存 `performance.test.ts` がすべて pass（実装前後で差分なし）
- E2E throughput bench（`visibility-throughput-bench` 等の既存スイート）が pass
- 上記のいずれも明示的な ms 閾値判定はテスト側に組み込まれているため、本機能は「既存テストを落とさないこと」を合否基準とする
- 主観評価としては、スクロールアップ中に PTY バーストを流しても可視行が動かないことを目視確認

## Security Verification

- 本機能は外部入力を新たに受け付けない。renderer 内部状態のみを操作するため security 要件追加なし

## Verification Summary

| Category | Items | Automated (Unit) | E2E | Manual |
|----------|-------|------------------|-----|--------|
| Functional | 7 (TS-1〜TS-7) | 6 (TS-1〜TS-6) | 1 (TS-7) | 0 |
| Non-Functional | 2 (NFR1, NFR2) | 2（既存 perf テスト + typecheck）| 1（throughput bench で regression check）| 0 |
| Manual supplementary | 2 | 0 | 0 | 2 |
