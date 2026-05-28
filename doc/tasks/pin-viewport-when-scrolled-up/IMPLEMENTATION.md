# Implementation Plan: Pin Viewport When Scrolled Up

## Overview

`scrollOffset > 0` の状態で PTY 由来のスクロールバック増加が発生したとき、表示中の絶対行を保つように `scrollOffset` を補正するロジックを `CanvasRenderer` に追加する。

## Objectives

- スクロールアップ閲覧中の新規 PTY 出力でビューポートを固定する
- scrollback 容量超過時は最上部にクランプする
- 既存の最下部追従挙動（`scrollOffset === 0`）を保つ
- 既存の renderer 公開 API（`scrollUp` / `scrollDown` / `getScrollOffset` / `setScrollOffset`）を変えない

## Prerequisites

### Development Environment
- Bun（既存）
- Docker Compose（テスト・E2E実行）
- 既存の `src/terminal/canvas-renderer.ts` と `TerminalState.getScrollbackLength()` が動く環境

### Dependencies
- 既存内部コンポーネント:
  - `CanvasRenderer`（`src/terminal/canvas-renderer.ts`）
  - `TerminalState.getScrollbackLength()`（`src/terminal/state.ts`）
- WASM 側変更は不要（`scrollbackLength` は既に露出済み）

## Architecture Overview

### Technology Stack
- **Language**: TypeScript（フロントエンドのみ。WASM 変更なし）
- **テスト**: `bun test`（unit）/ `bun run typecheck`（型）/ `./scripts/run-e2e-docker.sh test` (E2E)

### Design Approach

データフロー上、唯一の追加ポイントは「scrollback 長が増えた」ことを次の render パスで検知する観測ポイントを renderer 内部に置くこと。状態は renderer 内に閉じる（`prevScrollbackLength` フィールド1つ）。pure な補正関数を切り出して単体テスト可能にする。

```
PTY chunk
  -> WASM scroll
  -> scrollbackLength が Δ 増える
  -> 次の render 冒頭で adjustScrollOffsetForGrowth() が Δ を観測
  -> scrollOffset > 0 の場合 scrollOffset += Δ（上限 scrollbackLength でクランプ）
  -> render は補正後の scrollOffset で getVisibleLines() を呼ぶ
```

### Component Interaction

| Caller | Renderer 状態への影響 |
|--------|------------------------|
| PTY 由来の scroll-up | `scrollbackLength` が増える -> renderer が次フレームで `scrollOffset` を補正 |
| ユーザー操作（ホイール/キーボード） | `scrollUp` / `scrollDown` / `setScrollOffset` を呼んで `scrollOffset` を直接変更（pin ロジックと無関係） |
| バッファクリア / pane 切替 / resize | `scrollbackLength` が減る or 同じ -> 補正は走らず、ベースラインを再初期化 |
| alt-screen 入退場 | `state.getScrollbackLength()` は常に primary buffer のものを返す（`state.ts:545-547`）ため、入退場時の Δ === 0 で no-op |

## Implementation Phases

### Phase 1: 補正ロジックの純粋関数化と単体テスト追加（TDD）

**Goal**: scrollOffset 補正アルゴリズムを純粋関数として実装し、SPEC.md T1〜T5 に対応する単体テストでカバーする。

**Files to Create**:
- `src/terminal/scroll-pin.ts` - pin offset 補正ロジック（純粋関数）の定義
- `src/terminal/scroll-pin.test.ts` - 補正ロジックの単体テスト

**Files to Modify**:
- なし（このフェーズは純粋関数のみ）

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `computeAdjustedScrollOffset(prevSbLen, currSbLen, scrollOffset)` | 直前と現在の scrollback 長、現在の scrollOffset から「補正後の scrollOffset」と「次回比較に使う prevSbLen」を返す | すべて非負整数 | `scrollOffset === 0` または `currSbLen <= prevSbLen` の場合は scrollOffset を変えず prevSbLen を currSbLen に再ベースライン化する。`scrollOffset > 0 && currSbLen > prevSbLen` の場合は scrollOffset を `Δ = currSbLen - prevSbLen` だけ増やし、上限 currSbLen でクランプする |

**Processing Flow** (diagram-convertible):
1. `Δ = currSbLen - prevSbLen` を計算
2. 分岐:
   - `Δ <= 0`（減少 or 同値） -> scrollOffset 変更なし、prevSbLen を currSbLen に再ベースライン化（FR の reset cases に対応）
   - `Δ > 0 && scrollOffset === 0` -> scrollOffset 変更なし、prevSbLen を currSbLen に更新（FR2）
   - `Δ > 0 && scrollOffset > 0` -> `newOffset = min(scrollOffset + Δ, currSbLen)`、prevSbLen を currSbLen に更新（FR1 + FR3）
3. `{ nextScrollOffset, nextPrevSbLen }` を返却

**Implementation Steps** (TDD):
1. **テストファイル雛形** - `scroll-pin.test.ts` に describe ブロックを起こし、SPEC.md の T1〜T5 を `test` ケースとして並べる（最初は failing）
2. **T1（FR1）対応** - `scrollOffset=5, prev=10, curr=13` -> `nextScrollOffset=8, nextPrevSbLen=13` を検証する pure 関数を最小実装
3. **T2（FR2）対応** - `scrollOffset=0, prev=10, curr=13` -> `nextScrollOffset=0, nextPrevSbLen=13`
4. **T3（FR3）対応** - クランプ条件 `scrollOffset + Δ > currSbLen` で `nextScrollOffset = currSbLen` を返す
5. **T4（reset / clear）対応** - `curr < prev` のとき scrollOffset 変えず prevSbLen を curr に再ベースライン化
6. **T5（alt-screen 等）対応** - reset path が同じ振る舞いになることをテスト

**Dependencies**: なし

**Blocks**: Phase 2

**Testing Approach**:
- Unit: T1〜T5（後述 VERIFICATION.md 参照）
- 型: `bun run typecheck`
- Integration / E2E: 本フェーズでは実施しない

**Acceptance Criteria**:
- [ ] `src/terminal/scroll-pin.ts` が新規作成され、`computeAdjustedScrollOffset` が export されている
- [ ] `src/terminal/scroll-pin.test.ts` の T1〜T5 が全 pass
- [ ] `bun run typecheck` が pass

**Estimated Effort**: small

---

### Phase 2: CanvasRenderer への補正フックの組み込み

**Goal**: Phase 1 の純粋関数を `CanvasRenderer.render()` / `forceRender()` の冒頭で呼び、`prevScrollbackLength` フィールドで前回値を保持する。既存挙動（最下部追従、ユーザー操作、alt-screen、clear、pane 切替、resize）は変えない。

**Files to Create**:
- なし

**Files to Modify**:
- `src/terminal/canvas-renderer.ts`
  - インスタンスフィールド `prevScrollbackLength: number = 0` を追加
  - private ヘルパー `adjustScrollOffsetForGrowth(state)` を追加し、Phase 1 の純粋関数を呼んで `this.scrollOffset` と `this.prevScrollbackLength` を更新する
  - `render()` 冒頭（state ready ガードの直後、`if (this.scrollOffset > 0) → forceRender()` 早期分岐より前、`scrollOffset` 参照より前）で `adjustScrollOffsetForGrowth(state)` を呼ぶ
  - `forceRender(state)` 冒頭（同じく `scrollOffset` 参照より前）でも呼ぶ
    - 注: `render() → forceRender()` 経路で同フレームに2回呼ばれるが、2回目は Δ=0 のため no-op（Risk Assessment 参照）
  - `renderImmediate()` は内部で `this.render()` を呼ぶため追加変更不要
  - 既存の `this.scrollOffset = 0` 直接代入箇所は無いが、初期化パス（renderer 構築直後、pane 切替や clear に伴う rebind がある場合）で `prevScrollbackLength` が次回 `adjustScrollOffsetForGrowth` の reset path（`curr < prev` -> 再ベースライン化）で吸収されることを確認

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `CanvasRenderer.prevScrollbackLength` | 直前の render 時点で観測した `state.getScrollbackLength()` を保持 | render パス開始前は前回 render の終了時値 | render パス終了時には今回の currSbLen に更新される |
| `CanvasRenderer.adjustScrollOffsetForGrowth(state)` | Phase 1 の純粋関数を呼び、`this.scrollOffset` と `this.prevScrollbackLength` を反映する | `state.isReady()` が真 | FR1〜FR3 と reset cases に従って両フィールドが更新される。副作用は両フィールドの代入のみ |

**Processing Flow** (diagram-convertible):
1. `render()` または `forceRender()` 開始
2. 既存の `isReady()` / race-detector ガードを通過
3. `adjustScrollOffsetForGrowth(state)` を呼ぶ
   - 内部で `state.getScrollbackLength()` を取得し、Phase 1 関数で補正値を計算
   - `this.scrollOffset` / `this.prevScrollbackLength` を更新
4. 以降の既存処理（`getVisibleLines(state, this.scrollOffset)` ほか）はそのまま実行

**Implementation Steps** (5-7 max):
1. **フィールド追加** - `CanvasRenderer` に `prevScrollbackLength: number = 0` を追加
2. **ヘルパー追加** - `adjustScrollOffsetForGrowth(state: TerminalState)` を private で追加し、Phase 1 関数の戻り値で両フィールドを更新
3. **`render()` への組み込み** - 既存 `if (!this.pendingState || !this.pendingState.isReady())` ブロック直後に呼び出しを挿入
4. **`forceRender()` への組み込み** - 同様に冒頭（race-detector 直後、`getVisibleLines` 呼び出しより前）に呼び出しを挿入
5. **既存挙動の手動レビュー** - `scrollUp` / `scrollDown` / `setScrollOffset` は `prevScrollbackLength` に触らないことを確認
6. **alt-screen / clear / pane 切替の確認**:
   - alt-screen: `state.getScrollbackLength()` が常に primary を返すため、Δ === 0 で no-op
   - clear / pane 切替: `curr < prev` の reset path で吸収
7. **`renderImmediate()` の確認**: 内部で `this.render()` を呼ぶため、`render()` への組み込みで自動的にカバーされる

**Dependencies**: Phase 1 完了

**Blocks**: Phase 3

**Testing Approach**:
- Unit: Phase 1 のテストは引き続き pass
- Integration: 既存 `canvas-renderer.test.ts` および周辺テストが pass
- E2E: 本フェーズでは Phase 3 まで保留

**Acceptance Criteria**:
- [ ] `bun test` の全 unit テストが pass
- [ ] `bun run typecheck` が pass
- [ ] 既存 `canvas-renderer.test.ts` のすべてのケースが pass

**Estimated Effort**: small

---

### Phase 3: E2E 検証スペックの追加

**Goal**: SPEC.md E2 に対応する E2E spec を新設し、PTY 出力中にスクロール位置が維持されることをスクリーンショット比較で検証する。Docker E2E で動作させる。

**Files to Create**:
- `e2e-tests/specs/scroll-pin.e2e.js` - スクロールアップ中の PTY バースト時に固定行が変わらないことを確認する spec

**Files to Modify**:
- なし

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `scroll-pin.e2e.js` の `it` ブロック | shell を立ち上げ、scrollback に十分な行を出力したのち停止し、スクロールアップしてから新規バーストを起こす。バースト前後の同じ display row のテキストが一致することを assert する | tauri-driver が起動し、`.tab-content` が表示可能になっている | 同じ display row のテキストが scrollback ring 内で生存している限り変わらないことを検証 |

**Processing Flow** (diagram-convertible):
1. ターミナル起動を待機
2. shell に scrollback を埋める出力（例: `seq 5000`）を流す
3. 出力停止後、PageUp 相当のキー操作で `scrollOffset > 0` まで戻す
4. 現在の表示中行のテキストをサンプリングし screenshot 取得
5. もう一度 scrollback を伸ばす出力を流す
6. 出力中・出力後の同じ display row のテキストを再サンプリングし、screenshot 取得
7. 期待: バースト前後で固定 display row のテキストが等しい

**Implementation Steps** (5-7 max):
1. **spec 雛形** - 既存 `terminal.e2e.js` の `before` / log 収集パターンを踏襲してファイルを起こす
2. **scrollback 充填** - `seq` などで scrollback を確実に増やす
3. **スクロールアップ操作** - キー操作（PageUp）で `scrollOffset > 0` まで戻す
4. **バースト発生** - もう一度 scrollback を伸ばすコマンドを実行
5. **アサート** - バースト前後で固定 display row のテキストが等しいことを assert（および screenshot を `e2e-tests/screenshots/scroll-pin-*.png` に保存）
6. **タイムアウト調整** - Docker 環境の 180s 規約に従う

**Dependencies**: Phase 2 完了

**Blocks**: なし（最終 verify で実行）

**Testing Approach**:
- E2E: `./scripts/run-e2e-docker.sh test scroll-pin.e2e.js`（実行は sdd.6 verify でまとめて）
- Manual: 必要時に手動でも目視確認

**Acceptance Criteria**:
- [ ] `scroll-pin.e2e.js` が生成される
- [ ] sdd.6 verify 時に `./scripts/run-e2e-docker.sh test scroll-pin.e2e.js` が pass する

**Estimated Effort**: medium

---

## Complete File Structure

```
src/terminal/
  scroll-pin.ts            # 新規: 補正ロジック（純粋関数）
  scroll-pin.test.ts       # 新規: T1〜T5 の単体テスト
  canvas-renderer.ts       # 変更: prevScrollbackLength + adjustScrollOffsetForGrowth 組込
  canvas-renderer.test.ts  # 変更なし（pin ロジック自体は scroll-pin.test.ts でカバー）
e2e-tests/specs/
  scroll-pin.e2e.js        # 新規: PTY バースト中のビューポート固定 E2E
doc/tasks/pin-viewport-when-scrolled-up/
  IMPLEMENTATION.md
  VERIFICATION.md
  tasks.yaml
```

## Testing Strategy

- **Unit**: Phase 1 の `computeAdjustedScrollOffset` で T1〜T5 を全てカバー（純粋関数なので happy-dom Canvas 制約と無関係）
- **Integration**: 既存 `canvas-renderer.test.ts` の `getVisibleLines` / `calculateScrollPosition` テストが pass し続ける
- **E2E**: 新規 `scroll-pin.e2e.js` で「スクロールアップ中の PTY バーストで固定行が動かない」ことを screenshot 比較で検証
- **Manual**: vim / less などの alt-screen アプリで本機能が影響しないことの目視確認（任意）

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| 既存 `bun` | 既存版 | unit テスト・型チェック |
| 既存 `tauri-driver` / `WebKitWebDriver` | 既存版 | E2E |

新規外部依存なし。

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| ring buffer 境界で `Δ` が圧縮されるケース（evict と push が同時） | 中 | 中 | T3 で `scrollOffset + Δ > currSbLen` のクランプ動作を unit テストで検証。SPEC.md FR3 と完全一致 |
| 既存 `forceRender()` 経由（`scrollOffset > 0` の早期分岐や resize 後）で複数回 `adjustScrollOffsetForGrowth` が呼ばれる可能性 | 中 | 低 | `adjustScrollOffsetForGrowth` は1パスで `prevScrollbackLength` を currSbLen に書き戻すため、同フレーム内の二回目呼び出しは `Δ === 0` で no-op になる |
| pane 切替で renderer が共有される場合に prev 値がリーク | 中 | 中 | reset case として `curr < prev` で再ベースライン化する設計。Phase 2 の手動レビューで pane 切替経路を確認 |
| E2E のテキストサンプリングが Docker 環境でフレーキー | 低 | 中 | screenshot に加え DOM 上の `.tab-content` テキストも取得し両方で確認、180s タイムアウト |

## Open Questions

- [ ] なし（SPEC.md「Open Questions: None」と一致）

## Success Metrics

- [ ] FR1〜FR6 が VERIFICATION.md の test scenarios で全カバーされる
- [ ] `bun test` / `bun run typecheck` が pass
- [ ] sdd.6 verify で E2E 全スイートが pass
- [ ] 既存 `canvas-renderer.test.ts` に regressions が無い
