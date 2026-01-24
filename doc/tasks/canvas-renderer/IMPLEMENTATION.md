# Implementation Plan: Canvas 2D Renderer

## Overview

Canvas 2D APIを使用したターミナルレンダラーを実装し、高速スクロール時のパフォーマンスを改善する。既存のDOMレンダラーと共存させ、フィーチャーフラグで切り替え可能にする。

## Objectives

- Canvas 2D APIによる高速な描画パフォーマンスを実現
- 既存DOMレンダラーと同等の視覚的品質を維持
- 共通インターフェースにより両レンダラーを切り替え可能に
- 移行完了後はCanvas 2Dに完全切り替え

## Prerequisites

### Development Environment
- Bun (package manager and bundler)
- Tauri development environment
- Canvas 2D API対応のWebView

### Dependencies
- 既存モジュール: `attributes.ts`, `colors.ts`, `unicode.ts`, `grid.ts`
- 既存ユーティリティ: `performance.ts`

### Knowledge Requirements
- Canvas 2D API (fillText, fillRect, stroke operations)
- High DPI rendering (devicePixelRatio)
- requestAnimationFrame-based rendering

## Architecture Overview

### Technology Stack
- **Language**: TypeScript
- **Rendering**: Canvas 2D API
- **Scheduling**: requestAnimationFrame

### Design Approach

レンダラー抽象化により、DOMとCanvasを同じインターフェースで扱う:

```
ITerminalRenderer (共通インターフェース)
├── scheduleRender(state)
├── forceRender(state)
├── resize(cols, rows)
├── renderSelection(selection)
├── clearSelectionHighlight()
├── getCharWidth() / getCharHeight()
└── getFontFamily() / getFontSize()
        ▲              ▲
        │              │
TerminalRenderer   CanvasRenderer
(DOM-based)        (Canvas 2D-based)
```

### Component Interaction

```
TerminalState
      │
      ▼
CanvasRenderer.scheduleRender()
      │
      ▼
requestAnimationFrame
      │
      ▼
render() - dirty rowsのみ処理
      │
      ├──→ renderLine() - テキストと属性の描画
      ├──→ updateCursor() - カーソル描画
      └──→ renderSelection() - 選択範囲描画
      │
      ▼
Canvas 2D Context Operations
```

## Implementation Phases

### Phase 1: Core Rendering Infrastructure

**Goal**: Canvas要素の初期化と基本的なテキスト描画を実現

**Files to Create**:
- `src/terminal/canvas-renderer.ts` - Canvas 2Dレンダラー本体

**Files to Modify**:
- なし（この段階では独立して開発）

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| CanvasRenderer | Canvas要素の管理とコンテキスト初期化 | コンテナ要素とフォント設定 | Canvas要素が描画可能な状態 |
| setupCanvas | High DPI対応のCanvas初期化 | コンテナのサイズが確定 | Canvas解像度がDPI対応 |
| measureCharacterSize | 文字サイズの測定 | フォント設定が完了 | charWidth/charHeightが設定済み |
| renderLine | 単一行のテキスト描画 | Lineデータと行番号 | Canvas上に行が描画される |
| groupCellsIntoSpans | 同一属性セルのグループ化とwidth=0セル処理 | Lineデータ | 属性でグループ化されたspan配列（combining marks統合済み） |
| getVisibleLines | scrollOffsetに基づく可視行の取得 | TerminalState, scrollOffset | 画面に表示する行の配列 |
| calculateScrollPosition | スクロール位置から描画開始行を算出 | scrollOffset, scrollbackLength | 描画開始インデックス |

**Processing Flow**:
```
1. コンストラクタ呼び出し
   ├─ コンテナにCanvas要素を作成
   └─ 2D描画コンテキストを取得

2. setupCanvas
   ├─ devicePixelRatioを取得
   ├─ Canvasサイズをコンテナに合わせて設定
   └─ コンテキストをDPI比率でスケール

3. measureCharacterSize
   ├─ 測定用にオフスクリーンでテキストを配置
   └─ 'W'の幅と行高さを取得

4. renderLine (行ごとに呼び出し)
   ├─ 行の背景を既定背景色でクリア（行全体をfillRectで塗りつぶし）
   ├─ groupCellsIntoSpansで属性グループ化
   ├─ 各spanの背景をfillRectで描画（既定背景色以外の場合）
   └─ 各spanのテキストをfillTextで描画

5. スクロールオフセット処理
   ├─ scrollOffsetから表示開始行を算出
   │   └─ 表示開始行 = scrollbackBuffer.length - scrollOffset
   ├─ 描画対象行の決定
   │   └─ スクロールバック領域 + 現在の画面バッファから可視行を取得
   └─ 各行のY座標計算
       └─ y = (画面上の行番号) * charHeight
```

**Implementation Steps**:

1. **Canvas要素の初期化**
   - コンテナ内にCanvas要素を作成
   - 2D描画コンテキストを取得
   - High DPI対応（devicePixelRatioでスケーリング）
   - devicePixelRatio変更の監視:
     - `window.matchMedia(`(resolution: ${dpr}dppx)`).addEventListener('change', ...)`で検出
     - 変更検出時はsetupCanvas()を再呼び出しして再描画

2. **文字サイズ測定**
   - 既存DOMレンダラーと同等のアプローチ
   - measureText APIでcharWidthを計算
   - lineHeightからcharHeightを算出

3. **基本テキスト描画**
   - fillTextでテキスト描画
   - 属性なし（デフォルト色）での描画
   - テキストベースライン:
     - ctx.textBaseline = 'alphabetic'を使用
     - Y座標: `rowIndex * charHeight + (charHeight - fontDescent)`
     - fontDescentはctx.measureText('M').fontBoundingBoxDescentで取得

4. **セルグループ化ロジック**
   - 既存のgroupCellsIntoSpansを流用可能な形式で実装
   - width=0セルの処理:
     - Wide文字の2セル目（プレースホルダー）: スキップ（1セル目で2セル幅を描画済み）
     - Combining marks（結合文字）: 直前のセルの文字と結合して描画（テキストを連結）
   - groupCellsIntoSpansでwidth=0セルを適切に処理し、結合文字は前のspanに追加

5. **リサイズ処理**
   - resize(cols, rows)メソッドの処理フロー:
     1. cols/rowsプロパティを更新
     2. setupCanvas()を再呼び出し（Canvas要素のサイズ再設定とDPRスケーリング）
     3. 全画面を再描画（forceRender呼び出し）
   - コンテナのresizeイベントまたはwindow.resizeで呼び出される

6. **スクロールオフセット処理**
   - scrollOffsetから描画対象行を算出:
     ```typescript
     // scrollOffset: 0 = 最新（画面バッファのみ表示）
     // scrollOffset: N = N行分スクロールバックを表示
     const scrollbackLength = state.scrollbackBuffer.length;
     const visibleRows = state.rows;

     // 表示する行を収集
     const linesToRender: Line[] = [];
     for (let screenRow = 0; screenRow < visibleRows; screenRow++) {
       const bufferIndex = scrollbackLength - scrollOffset + screenRow;
       if (bufferIndex < scrollbackLength) {
         // スクロールバックバッファから取得
         linesToRender.push(state.scrollbackBuffer[bufferIndex]);
       } else {
         // 現在の画面バッファから取得
         const screenIndex = bufferIndex - scrollbackLength;
         linesToRender.push(state.lines[screenIndex]);
       }
     }
     ```
   - 各行のCanvas Y座標: `screenRow * charHeight`
   - スクロール位置変更時は全画面を再描画

**Dependencies**:
- Requires: なし
- Blocks: Phase 2, Phase 3, Phase 4

**Testing Approach**:

*Unit Tests*:
- setupCanvasがDPR対応でCanvasサイズを設定することを検証
- measureCharacterSizeが正の値を返すことを検証
- groupCellsIntoSpansが正しくセルをグループ化することを検証
- groupCellsIntoSpansがWide文字の2セル目（width=0）をスキップすることを検証
- groupCellsIntoSpansがcombining marks（width=0）を前のセルに結合することを検証
- renderLineがfillTextを呼び出すことを検証（モック使用）
- getVisibleLinesがscrollOffset=0で画面バッファのみ返すことを検証
- getVisibleLinesがscrollOffset>0でスクロールバックと画面バッファの混合を返すことを検証
- calculateScrollPositionが正しい開始インデックスを返すことを検証

*Manual Testing*:
- [ ] Canvas要素がコンテナ内に表示される
- [ ] テキストが正しい位置に描画される
- [ ] 高DPIディスプレイでぼやけない

**Acceptance Criteria**:
- [ ] Canvas要素が正しいサイズで初期化される
- [ ] High DPI環境で鮮明に表示される
- [ ] デフォルト色でテキストが描画される
- [ ] Wide文字が2セル分の幅で描画される

**Estimated Effort**: 中 (3-5 days)

**Risks and Mitigation**:
- **Risk**: フォント測定の精度がDOMと異なる
  - **Mitigation**: 既存のmeasureCharacterSizeアプローチを参考にする

---

### Phase 2: Attributes and Styling

**Goal**: 全SGR属性（色、太字、下線など）のサポート

**Files to Modify**:
- `src/terminal/canvas-renderer.ts` - 属性描画の追加

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| applyTextAttributes | フォントスタイルとfillStyleを設定 | CellAttributesオブジェクト | コンテキストにスタイル適用済み |
| drawBackground | 背景色の矩形描画 | 位置、サイズ、色 | 背景矩形が描画される |
| drawUnderline | 下線の描画 | 位置とサイズ | テキスト下部に線が描画される |
| drawStrikethrough | 取り消し線の描画 | 位置とサイズ | テキスト中央に線が描画される |
| buildFontString | フォント文字列の構築 | 属性（bold, italic） | "italic bold 13px fontFamily"形式 |

**Processing Flow**:
```
1. span描画の前処理
   ├─ getEffectiveForeground/Backgroundで実効色を取得
   └─ reverse属性の場合は色を入れ替え

2. 背景描画
   ├─ 背景色が設定されている場合
   └─ fillRectで背景矩形を描画

3. テキストスタイル設定
   ├─ buildFontStringでフォント文字列構築
   ├─ fillStyleで前景色設定
   └─ globalAlphaでdim属性対応

4. テキスト描画
   ├─ hidden属性でない場合
   └─ fillTextでテキスト描画

5. 装飾描画
   ├─ underline → drawUnderline
   └─ strikethrough → drawStrikethrough

6. 状態リセット
   └─ globalAlphaを1.0に戻す
```

**Implementation Steps**:

1. **色の適用**
   - 前景色: fillStyleに設定
   - 背景色: fillRectで先に描画
   - 既存のgetEffectiveForeground/getEffectiveBackgroundを使用
   - rgbToCSS相当のCanvas用色変換

2. **フォントスタイル**
   - bold: font-weightをboldに
   - italic: font-styleをitalicに
   - buildFontStringで組み立て

3. **テキスト装飾**
   - underline: moveToとlineToで線描画
   - strikethrough: 文字中央に線描画

4. **特殊属性**
   - dim: globalAlphaを0.5に設定
   - hidden: fillTextをスキップ
   - blink: タイマーベースの表示/非表示切り替え
     - タイマー間隔: 500ms（setIntervalを使用）
     - 再描画戦略: blink属性を持つセルを追跡し、タイマー発火時にそれらのセルを含む行のみ再描画
     - blinkVisible状態フラグで表示/非表示を制御
   - reverse: getEffective関数で処理済み

**Dependencies**:
- Requires: Phase 1
- Blocks: Phase 4

**Testing Approach**:

*Unit Tests*:
- 各属性フラグに対応するコンテキスト設定を検証
- buildFontStringが正しいフォント文字列を返すことを検証
- drawUnderline/drawStrikethroughが正しい座標で描画することを検証
- 16色/256色/RGB色がすべて正しく変換されることを検証

*Manual Testing*:
- [ ] 太字テキストが太く表示される
- [ ] 斜体テキストが傾いて表示される
- [ ] 下線が正しい位置に表示される
- [ ] 各色が正しく表示される（16色、256色、RGB）

**Acceptance Criteria**:
- [ ] 全SGR属性が正しく描画される（bold, italic, underline, strikethrough, blink, reverse, hidden, dim）
- [ ] 16色、256色、RGB色が正確に表示される
- [ ] reverse属性で前景色と背景色が入れ替わる
- [ ] DOMレンダラーと視覚的に同等

**Estimated Effort**: 中 (3-5 days)

**Risks and Mitigation**:
- **Risk**: blinkアニメーションのパフォーマンス影響
  - **Mitigation**: blink対象セルのみを再描画

---

### Phase 3: Cursor and Selection

**Goal**: カーソル描画・ブリンクと選択範囲ハイライト

**Files to Modify**:
- `src/terminal/canvas-renderer.ts` - カーソルと選択の追加

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| renderCursor | カーソルの描画 | カーソル位置、スタイル、表示状態 | Canvas上にカーソルが描画される |
| startCursorBlink | ブリンクタイマーの開始 | カーソルが表示状態 | 500ms間隔でブリンク |
| stopCursorBlink | ブリンクタイマーの停止 | ブリンク中 | タイマーがクリアされる |
| dispose | レンダラーの破棄とリソース解放 | - | 全タイマー停止、Canvas要素削除 |
| renderSelection | 選択範囲のハイライト | 選択開始/終了位置 | 半透明背景で選択表示 |
| normalizeSelection | 選択範囲の正規化 | 開始/終了位置 | start < endを保証 |

**Processing Flow**:
```
描画レイヤー順序（下から上）:
1. 背景色 - 各セルの背景をfillRectで描画
2. 選択範囲 - 半透明ハイライトを重ねて描画
3. テキスト - fillTextで文字を描画
4. 装飾 - 下線、取り消し線を描画
5. カーソル - 最上位レイヤーとして描画

カーソル描画フロー:
1. カーソル位置を計算（col * charWidth, row * charHeight）
2. 表示状態とブリンク状態を確認
   ├─ 非表示 → 描画スキップ
   └─ ブリンクOFF状態 → 描画スキップ
3. カーソルスタイルに応じて描画
   ├─ block → fillRectで塗りつぶし
   ├─ underline → 下部に細い矩形
   └─ bar → 左端に縦線

ブリンクフロー:
1. setIntervalでタイマー設定（500ms）
2. タイマー発火時
   └─ cursorVisible状態を反転 → カーソル領域のみ再描画

選択範囲描画フロー:
1. normalizeSelectionで開始<終了を保証
2. 各行について
   ├─ 行の選択開始列と終了列を計算
   └─ 半透明fillRectでハイライト描画
```

**Implementation Steps**:

1. **カーソル描画**
   - block: fillRectで1セル塗りつぶし
   - underline: 下部2pxの矩形
   - bar: 左端2pxの縦線
   - カーソル色は緑（#008000）

2. **カーソルブリンク**
   - setIntervalで500ms間隔
   - cursorVisibleフラグで表示/非表示
   - カーソル領域のみを再描画して最適化

3. **選択範囲ハイライト**
   - 選択範囲を正規化（start < end保証）
   - 半透明の背景色（rgba(50, 150, 250, 0.3)）
   - 複数行選択に対応

4. **リソースクリーンアップ（dispose）**
   - dispose()メソッドの実装:
     - カーソルブリンクタイマーを停止（clearInterval）
     - blinkテキストタイマーを停止（clearInterval）
     - devicePixelRatio変更リスナーを削除
     - Canvas要素をDOMから削除
   - ターミナル終了時に呼び出す

**Dependencies**:
- Requires: Phase 1
- Blocks: Phase 4

**Testing Approach**:

*Unit Tests*:
- renderCursorが各スタイルで正しい座標に描画することを検証
- ブリンクタイマーが正しく開始/停止することを検証
- normalizeSelectionが逆方向選択を正しく処理することを検証
- renderSelectionが複数行選択を正しく描画することを検証

*Integration Tests*:
- カーソル移動後に正しい位置に再描画されることを検証
- 選択範囲変更後に正しくハイライトが更新されることを検証

*Manual Testing*:
- [ ] カーソルが正しい位置に表示される
- [ ] カーソルがブリンクする（500ms間隔）
- [ ] 3種類のカーソルスタイルが正しく表示される
- [ ] テキスト選択がハイライトされる
- [ ] 複数行選択が正しく表示される

**Acceptance Criteria**:
- [ ] block/underline/barの3スタイルが正しく描画される
- [ ] カーソルが500ms間隔でブリンクする
- [ ] 選択範囲が半透明ハイライトで表示される
- [ ] 複数行選択が正しく処理される

**Estimated Effort**: 小 (1-2 days)

**Risks and Mitigation**:
- **Risk**: ブリンク時の部分再描画が複雑
  - **Mitigation**: 最初は全画面再描画、後で最適化

---

### Phase 4: Integration and Feature Flag

**Goal**: レンダラーファクトリと環境変数による切り替え

**Files to Create**:
- `src/terminal/renderer-factory.ts` - レンダラーファクトリ
- `src/terminal/renderer-interface.ts` - 共通インターフェース定義

**Files to Modify**:
- `src/terminal/renderer.ts` - インターフェース実装を明示化
- `src/terminal/index.ts` - ファクトリのエクスポート追加

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| ITerminalRenderer | 共通インターフェース定義 | - | 型定義として提供 |
| createRenderer | レンダラーインスタンスの生成 | コンテナ、フォント設定 | 適切なレンダラーインスタンス |
| getRendererType | 環境変数からレンダラー種別を取得 | - | "canvas" or "dom" |

**Processing Flow**:
```
アプリケーション起動フロー:
1. createRenderer呼び出し
2. getRendererTypeで環境変数確認
   ├─ EMTERM_RENDERER="canvas" → CanvasRenderer生成
   └─ その他またはundefined → TerminalRenderer(DOM)生成
3. 生成されたレンダラーを返却
```

**Implementation Steps**:

1. **共通インターフェース定義**
   - ITerminalRendererインターフェースを定義
   - 既存TerminalRendererの公開メソッドをベースに

2. **レンダラーファクトリ**
   - createRenderer関数の実装
   - 環境変数`EMTERM_RENDERER`の読み取り方法:
     - Vite/Bun環境: `import.meta.env.VITE_EMTERM_RENDERER`
     - ビルド時に環境変数をembedする（実行時には変更不可）
   - "canvas"の場合CanvasRenderer、それ以外はTerminalRenderer

3. **既存コードの更新**
   - TerminalRendererがITerminalRendererを実装するよう明示
   - index.tsにファクトリをエクスポート

4. **パフォーマンス最適化**
   - dirty row tracking（変更行のみ再描画）
   - フォント文字列のキャッシュ
   - 色文字列のキャッシュ

**Dependencies**:
- Requires: Phase 1, Phase 2, Phase 3
- Blocks: Phase 5

**Testing Approach**:

*Unit Tests*:
- createRendererが環境変数に応じて正しいインスタンスを返すことを検証
- ITerminalRendererの全メソッドが両レンダラーで実装されていることを検証

*Integration Tests*:
- 環境変数切り替えで正しいレンダラーが使用されることを検証
- 両レンダラーで同じ入力に対して視覚的に同等の出力を生成することを検証

*E2E Tests*:
- 環境変数を設定してアプリケーションを起動し、正しいレンダラーが使用されることを検証

*Manual Testing*:
- [ ] EMTERM_RENDERER=canvasでCanvas版が使用される
- [ ] EMTERM_RENDERER=domでDOM版が使用される
- [ ] 環境変数未設定でDOM版が使用される
- [ ] 両レンダラーで同じ表示になる

**Acceptance Criteria**:
- [ ] 環境変数でレンダラーを切り替え可能
- [ ] 両レンダラーが同じインターフェースを実装
- [ ] パフォーマンスがDOMレンダラーより改善
- [ ] 全既存機能がCanvasレンダラーで動作

**Estimated Effort**: 中 (3-5 days)

**Risks and Mitigation**:
- **Risk**: 環境変数の読み取り方法がTauri環境で異なる
  - **Mitigation**: import.meta.envまたはTauri APIで確認

---

### Phase 5: Migration Complete

**Goal**: Canvas 2Dへの完全移行（DOMレンダラー削除）

**Files to Modify**:
- `src/terminal/renderer-factory.ts` - ファクトリをシンプル化
- `src/terminal/index.ts` - エクスポート更新

**Files to Delete**:
- `src/terminal/renderer.ts` - DOMレンダラー
- `src/terminal/style-cache.ts` - DOM専用キャッシュ
- `src/terminal/renderer-interface.ts` - インターフェース不要に

**Processing Flow**:
```
1. パフォーマンス検証完了を確認
2. DOMレンダラー関連ファイルを削除
3. ファクトリを削除してCanvasRendererを直接使用
4. 環境変数による切り替えを削除
```

**Implementation Steps**:

1. **検証完了確認**
   - Canvasレンダラーが全機能をカバー
   - パフォーマンスが改善していることを確認

2. **DOMレンダラー削除**
   - renderer.ts削除
   - style-cache.ts削除（DOM専用）
   - renderer-interface.ts削除（単一レンダラーのため不要）

3. **ファクトリ簡略化**
   - 環境変数チェックを削除
   - CanvasRendererを直接エクスポート

4. **ドキュメント更新**
   - 環境変数の記述を削除
   - Canvas 2Dが唯一のレンダラーであることを明記

**Dependencies**:
- Requires: Phase 4の検証完了
- Blocks: なし

**Testing Approach**:

*Unit Tests*:
- 削除後のビルドが成功することを検証
- 全既存テストがパスすることを検証

*Manual Testing*:
- [ ] アプリケーションが正常に起動する
- [ ] 全機能が正常に動作する

**Acceptance Criteria**:
- [ ] DOMレンダラー関連コードが削除されている
- [ ] ビルドエラーがない
- [ ] 全テストがパス
- [ ] パフォーマンスが維持されている

**Estimated Effort**: 小 (1-2 days)

**Risks and Mitigation**:
- **Risk**: 予期せぬ依存関係による削除漏れ
  - **Mitigation**: TypeScriptコンパイルエラーで検出

---

## Complete File Structure

```
src/terminal/
├── renderer.ts              # 既存DOMレンダラー（Phase 5で削除）
├── canvas-renderer.ts       # 新規: Canvas 2Dレンダラー
├── renderer-factory.ts      # 新規: レンダラーファクトリ
├── renderer-interface.ts    # 新規: 共通インターフェース（Phase 5で削除）
├── performance.ts           # 既存: パフォーマンスユーティリティ（共有）
├── unicode.ts               # 既存: charWidth関数（共有）
├── attributes.ts            # 既存: CellAttributes型と関数（共有）
├── colors.ts                # 既存: 色パレットと変換（共有）
├── grid.ts                  # 既存: Line, Cell型（共有）
├── style-cache.ts           # 既存: DOMスタイルキャッシュ（Phase 5で削除）
├── state.ts                 # 既存: TerminalState（共有）
└── index.ts                 # モジュールエクスポート（更新）
```

**File Descriptions**:
- `canvas-renderer.ts`: Canvas 2D APIを使用したレンダラー実装
- `renderer-factory.ts`: 環境変数に基づくレンダラー生成（一時的）
- `renderer-interface.ts`: 両レンダラーの共通インターフェース（一時的）

## Testing Strategy

### Unit Testing

**Approach**:
- Bunの組み込みテストランナーを使用
- Canvas 2D APIはモックで検証
- 既存テストパターンに従う

**Test Coverage Goals**:
- Core logic: 90%+
- 描画ロジック: 80%+（Canvas APIのモック制限）

**Key Test Areas**:

1. **Canvas初期化** (`canvas-renderer.test.ts`)
   - High DPI対応の検証
   - 文字サイズ測定の検証
   - コンテキスト設定の検証

2. **テキスト描画**
   - groupCellsIntoSpansの動作検証
   - Wide文字（width=2）の処理
   - 空行の処理

3. **属性描画**
   - 各SGR属性の適用検証
   - 色変換の検証
   - 複合属性の検証

4. **カーソル描画**
   - 各スタイルの描画検証
   - ブリンクタイマーの動作検証
   - 表示/非表示の切り替え

5. **選択範囲**
   - 正規化の検証
   - 複数行選択の検証

### Integration Testing

**Scenarios**:
1. 全画面レンダリングの正常動作
2. dirty row最適化の動作確認
3. リサイズ時のCanvas再初期化
4. 選択範囲のハイライト表示
5. カーソルブリンクの動作

### E2E Tests

- 環境変数によるレンダラー切り替え
- 高速スクロール時のパフォーマンス
- テキスト選択とコピー

### Manual Testing Checklist

**基本機能**:
- [ ] 通常のテキスト描画
- [ ] 各種属性（色、太字、斜体、下線）の描画
- [ ] カーソルの表示とブリンク
- [ ] Wide文字の描画
- [ ] テキスト選択とハイライト
- [ ] スクロールバックバッファの表示
- [ ] 環境変数によるレンダラー切り替え

**異常系**:
- [ ] 不正な文字コードの処理
- [ ] 空行の描画
- [ ] 画面いっぱいのテキスト

**パフォーマンス**:
- [ ] 高速スクロール時の描画

## Dependencies

### External Dependencies

| Package | Version | Purpose | Note |
|---------|---------|---------|------|
| - | - | - | 新規外部依存なし |

### Internal Dependencies

**共有モジュール**:
- `attributes.ts`: CellAttributes型、getEffective関数
- `colors.ts`: RGB型、rgbToCSS関数、パレット
- `unicode.ts`: charWidth関数
- `grid.ts`: Line、Cell型
- `performance.ts`: RenderTimer、PerformanceMonitor

**Implementation Order**:
1. Phase 1 (依存なし)
2. Phase 2 (Phase 1に依存)
3. Phase 3 (Phase 1に依存)
4. Phase 4 (Phase 1, 2, 3に依存)
5. Phase 5 (Phase 4の検証完了に依存)

## Risk Assessment

### Technical Risks

1. **フォント測定の精度**
   - **Risk**: Canvas APIとDOM APIで測定結果が異なる可能性
   - **Likelihood**: 中
   - **Impact**: 中（レイアウトずれ）
   - **Mitigation**: 既存のmeasureCharacterSizeアプローチを参考に、必要に応じて調整係数を適用

2. **High DPI対応**
   - **Risk**: devicePixelRatioの適用漏れでぼやける
   - **Likelihood**: 低
   - **Impact**: 中（視覚品質低下）
   - **Mitigation**: setupCanvas内で一貫してDPR対応を実装

3. **テキスト選択とクリップボード連携**
   - **Risk**: Canvasはネイティブのテキスト選択をサポートしない
   - **Likelihood**: 確実
   - **Impact**: 低（既存の選択ロジックを流用）
   - **Mitigation**: 既存の選択ロジック（座標ベース）をそのまま使用

### Implementation Risks

1. **スコープクリープ**
   - **Risk**: 仕様外の最適化に時間を費やす
   - **Mitigation**: 仕様書に記載された機能のみを実装

2. **パフォーマンス改善が期待以下**
   - **Risk**: Canvas 2DでもDOMと大差ない可能性
   - **Likelihood**: 低
   - **Impact**: 中
   - **Mitigation**: 早期にパフォーマンス計測、改善なければDOMにフォールバック

## Performance Considerations

1. **Dirty Row Tracking**
   - 変更のあった行のみを再描画
   - 既存のTerminalState.getDirtyRows()を使用

2. **属性グループ化**
   - 連続する同一属性セルを1回のfillTextで描画
   - コンテキスト状態変更を最小化

3. **キャッシュ戦略**
   - フォント文字列キャッシュ（属性組み合わせごと）
   - 色文字列キャッシュ（RGB値ごと）

4. **requestAnimationFrame**
   - 描画スケジューリングで重複描画を防止
   - 既存パターンを踏襲

## Security Considerations

1. **XSSリスク**
   - fillTextはHTMLを解釈しないため、XSSリスクなし
   - Canvasは本質的にセキュア

2. **リソース制限**
   - Canvasサイズはターミナルサイズに制限
   - メモリ使用量は制御下

## Open Questions

### From Specification:
- なし（全項目確認済み）

### Implementation-Specific:
- [ ] ブリンクテキストの再描画範囲最適化は必要か？（初期実装では全画面再描画で可）

## Future Enhancements

### Phase 5以降の改善案（仕様外）:
- WebGL 2.0への移行（さらなるパフォーマンス向上）
- フォント文字列キャッシュの最適化
- オフスクリーンCanvasによるダブルバッファリング

## Success Metrics

### Functional Completeness
- [ ] 全機能要件（FR1-FR9）が実装されている
- [ ] 全テストシナリオがパス
- [ ] エラーハンドリングが正常動作

### Quality Metrics
- [ ] テストカバレッジ90%以上（コアロジック）
- [ ] 手動テストで重大バグなし
- [ ] コードがプロジェクト規約に準拠

### Performance Metrics
- [ ] PerformanceMonitorで計測してDOMより改善
- [ ] 高速スクロール時のフレームドロップ減少

### User Experience
- [ ] DOMレンダラーと視覚的に同等
- [ ] カーソルが正しくブリンク
- [ ] 選択範囲が正しくハイライト

## References

- **要件定義書**: `doc/tasks/canvas-renderer/要件定義書.md`
- **技術仕様書**: `doc/tasks/canvas-renderer/SPEC.md`
- **既存DOMレンダラー**: `src/terminal/renderer.ts`
- **パフォーマンスユーティリティ**: `src/terminal/performance.ts`
- **Unicode幅計算**: `src/terminal/unicode.ts`

## Next Steps

1. **レビューと承認**
   - 実装計画の確認
   - 不明点の解消

2. **環境準備**
   - 開発ブランチの作成
   - テスト環境の確認

3. **実装開始**
   - Phase 1から順に実装
   - 各フェーズでテストを先に作成（TDD）

4. **継続的検証**
   - 各フェーズ完了時にパフォーマンス計測
   - DOMレンダラーとの比較検証
