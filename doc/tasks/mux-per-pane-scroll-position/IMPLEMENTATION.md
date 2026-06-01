# Implementation Plan: Per-Pane Scroll Position in mux

## Overview
mux の pane 切替時に、renderer が保持するスクロール位置 (および scroll-pin のベースライン) を pane ごとに保存・復元し、pane 間でスクロール位置が共有される不具合を解消する。

## Objectives
- mux pane 切替でスクロール位置を pane ごとに保存・復元する (FR1)
- scroll-pin のベースラインも pane ごとに保存・復元し、背景 pane のスクロールバック増加に既存 scroll-pin 補正が正しく効くようにする (FR2)
- 通常タブ (mux 非使用) の挙動を変えない (NFR2)

## Prerequisites

### Development Environment
- Bun (パッケージ管理・テスト・型チェック)
- Docker (テスト/E2E 実行、CLAUDE.md 推奨)

### Dependencies
- 既存の mux pane 状態管理 (`MuxPaneGridState`, `saveMuxPaneState` / `restoreMuxPaneState`)
- 既存の `CanvasRenderer` スクロール API (`getScrollOffset` / `setScrollOffset`)
- 既存の scroll-pin ロジック (`computeAdjustedScrollOffset`)

## Architecture Overview

### Technology Stack
- **Language**: TypeScript (frontend)
- **Framework**: Tauri WebView
- **Key Components**: CanvasRenderer (スクロール位置の実体), mux window manager (pane 切替), MuxPaneGridState (pane 状態スナップショット)

### Design Approach

根本原因: 通常タブは各タブが独立した `CanvasRenderer` を持つためスクロール位置が分離されるが、mux では 1 つの `CanvasRenderer` を全 pane で共有する。pane 切替時に WASM グリッドと `TerminalState` は `MuxPaneGridState` を介して pane ごとに save/restore されるのに対し、renderer が保持する `scrollOffset` と scroll-pin ベースライン (`prevScrollbackLength`) は save/restore 対象に含まれていないため、前 pane の値が次 pane に持ち越される。

修正方針: pane スナップショット (`MuxPaneGridState`) に「スクロール位置」と「scroll-pin ベースライン」を追加し、pane 切替の save/restore 経路でこの 2 値も一緒に退避・復元する。新規 pane や保存状態が無い pane は最下部 (オフセット 0・ベースラインはリセット相当) で初期化する。

alternate screen (TUI 全画面) の pane についての扱い: alternate screen はスクロールバックを持たず scroll-pin は no-op のため、退避時のスクロール位置 (通常 0) をそのまま保存・復元すれば破綻しない。primary/alternate を区別せず、退避時点の renderer のスクロール位置をそのまま保存・復元する方針とする。

### Component Interaction
- mux window manager は pane 切替時に renderer からスクロール位置を読み出してスナップショットに含め、復元時にスナップショットの値を renderer に書き戻す。
- 背景 pane が後でアクティブ化されスクロールバックが増えたとき、復元されたベースラインを起点に既存 scroll-pin 補正が働き、スクロールアップ中の pane は位置を維持し、最下部の pane は追従する。

## Implementation Phases

### Phase 1: Per-pane scroll offset save/restore (FR1)

**Goal**: pane 切替でスクロール位置が pane ごとに保持・復元され、別 pane を開いても前 pane のスクロール量が持ち越されない。

**Files to Modify**:
- `src/terminal/state-mux-pane.ts` — `MuxPaneGridState` にスクロール位置フィールドを追加する
- `src/terminal-app/mux/mux-window-manager.ts` — pane の save/restore/新規生成の各経路でスクロール位置を扱う
- `src/terminal/renderer-interface.ts` — (必要なら) scroll-pin ベースライン用 API を宣言する (Phase 2 と共有)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| MuxPaneGridState | pane のスクロール位置を保持する | — | スナップショットがスクロール位置を含む |
| pane save 経路 | 退避時に現在のスクロール位置をスナップショットへ記録 | renderer が存在 | 退避 pane のスクロール位置が保存される |
| pane restore 経路 | 復元時にスナップショットのスクロール位置を renderer へ反映 | 復元対象スナップショットが存在 | アクティブ pane のスクロール位置が復元される |
| fresh/未保存 pane 経路 | 新規・未保存 pane を最下部に初期化 | — | スクロール位置 = 0 |

**Processing Flow** (diagram-convertible):
1. pane 切替開始
   - 退避元 pane あり -> スナップショットに現在のスクロール位置を記録して保存
2. 切替先 pane のグリッドを復元
   - 保存スナップショットあり -> スナップショットのスクロール位置を renderer に反映
   - 保存スナップショットなし (初訪問/新規) -> スクロール位置を 0 に初期化
3. 再描画

**Implementation Steps**:
1. **スナップショット型の拡張** — `MuxPaneGridState` にスクロール位置を表すフィールドを追加する
2. **save 経路の対応** — pane を退避する 3 経路 (通常切替・リモート切替・pane 生成時の前 pane 退避) で renderer の現在スクロール位置をスナップショットへ記録する
3. **restore 経路の対応** — pane を復元する経路でスナップショットのスクロール位置を renderer へ書き戻す
4. **fresh/未保存経路の対応** — 新規グリッド生成および保存状態なし分岐でスクロール位置を 0 に初期化する

**Dependencies**: Blocks Phase 2 (同じスナップショット型・同じ経路を共有)

**Testing Approach**:
- Unit: スナップショットがスクロール位置を保持/復元すること、未保存 pane が 0 になること
- Integration: 切替往復 (A→B→A) で A のスクロール位置が復元されること
- E2E: pane A をスクロールアップ → pane B (B の位置) → pane A (復元)

**Acceptance Criteria**:
- [ ] pane A をスクロールアップ後に pane B へ切替えても、pane B は前 pane のスクロール量を引き継がない
- [ ] pane A に戻ると pane A のスクロール位置が復元される
- [ ] 新規/未保存 pane は最下部で表示される

**Estimated Effort**: small

---

### Phase 2: Per-pane scroll-pin baseline save/restore (FR2)

**Goal**: 背景 pane に出力が届いてスクロールバックが増えても、既存 scroll-pin 挙動 (スクロールアップ中は位置維持、最下部は追従) が pane ごとに正しく適用される。

**Files to Modify**:
- `src/terminal/renderer-interface.ts` — scroll-pin ベースラインの取得/設定 API を宣言する
- `src/terminal/canvas-renderer.ts` — scroll-pin ベースライン (`prevScrollbackLength`) の取得/設定アクセサを公開する
- `src/terminal/state-mux-pane.ts` — `MuxPaneGridState` に scroll-pin ベースラインフィールドを追加する
- `src/terminal-app/mux/mux-window-manager.ts` — save/restore/新規生成の各経路でベースラインを扱う

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| CanvasRenderer ベースライン API | scroll-pin ベースラインを外部から取得/設定可能にする | — | ベースラインが save/restore できる |
| MuxPaneGridState | pane の scroll-pin ベースラインを保持する | — | スナップショットがベースラインを含む |
| save/restore 経路 | スクロール位置と同じ経路でベースラインを退避・復元する | Phase 1 完了 | pane ごとにベースラインが分離される |

**Processing Flow** (diagram-convertible):
1. pane 退避時: 現在の scroll-pin ベースラインをスナップショットへ記録
2. pane 復元時: スナップショットのベースラインを renderer へ反映
3. アクティブ化後にスクロールバックが増加 -> 復元ベースラインを起点に既存 scroll-pin 補正が働く
   - スクロールアップ中 (オフセット > 0) -> 増加分だけオフセットを補正し位置維持
   - 最下部 (オフセット = 0) -> 追従

**Implementation Steps**:
1. **renderer アクセサの追加** — `CanvasRenderer` に scroll-pin ベースラインの取得/設定アクセサを追加し、インターフェースに宣言する
2. **スナップショット型の拡張** — `MuxPaneGridState` に scroll-pin ベースラインフィールドを追加する
3. **save/restore 経路の対応** — Phase 1 と同じ 3 経路でベースラインを退避・復元し、fresh/未保存経路ではベースラインをリセット相当 (0) に初期化する

**Dependencies**: Requires Phase 1

**Testing Approach**:
- Unit: スナップショットがベースラインを保持/復元すること
- Integration: 背景 pane のスクロールバック増加時に既存 scroll-pin 補正が復元ベースラインを起点に動くこと
- Manual: 実機で背景 pane に出力が来てもスクロールアップ中の pane が位置を維持すること

**Acceptance Criteria**:
- [ ] 背景 pane に出力が届いても、その pane の scroll-pin 挙動はアクティブ pane と同じルールに従う
- [ ] スクロールアップ中の pane はアクティブ化後の出力増加でも位置を維持する

**Estimated Effort**: small

---

## Complete File Structure

```
src/terminal/
├── state-mux-pane.ts          # MuxPaneGridState にスクロール位置 + scroll-pin ベースラインを追加
├── canvas-renderer.ts         # scroll-pin ベースラインの get/set アクセサを公開
├── renderer-interface.ts      # ベースライン API をインターフェースに宣言
└── scroll-pin.ts              # (変更なし) 既存補正ロジックを再利用
src/terminal-app/mux/
└── mux-window-manager.ts      # save/restore/fresh 各経路でスクロール位置 + ベースラインを扱う
```

## Testing Strategy
- Unit: スクロール位置・ベースラインの save/restore/初期化ロジック
- Integration: pane 切替往復でのスクロール位置復元
- E2E: WebdriverIO + tauri-driver (Docker) で pane 切替シナリオ
- Manual: 背景 pane への出力時の挙動 (実機判断)

## Dependencies
| Package | Version | Purpose |
|---------|---------|---------|
| (なし) | — | 既存コンポーネントのみで実装可能 |

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| 復元スクロール位置が新グリッドの行数を超える | 中 | 中 | 復元時に scrollback 長へクランプ (既存 scroll-pin と同じ方針) |
| 通常タブ経路への波及 | 低 | 中 | 変更は mux pane の save/restore 経路に限定し、通常タブの独立 renderer は触らない |
| 背景 pane のベースライン基準ずれ | 中 | 中 | scroll-pin ベースラインも pane ごとに退避・復元して基準を一致させる |
| alternate screen pane の復元でスクロール位置が不整合 | 低 | 低 | alternate screen は scrollback なし・scroll-pin no-op。退避時の値 (通常 0) をそのまま保存・復元 |

## Open Questions
- [ ] なし (要件は確定済み)

## Success Metrics
- [ ] FR1 / FR2 の受け入れ基準を満たす
- [ ] 通常タブのスクロール位置分離に回帰がない
- [ ] スクロール位置・ベースラインの単体テストが通る
