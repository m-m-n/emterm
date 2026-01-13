# Implementation Plan: Markdown Fullscreen Display

## Overview
- **Specification**: [SPEC.md](./SPEC.md)
- **Status**: Draft
- **Last Updated**: 2026-01-13

Markdown コンテンツをターミナル全体にオーバーレイ表示するフルスクリーンモードを実装する。既存の OSC 777 Markdown 表示機能を拡張し、新しい `render=fullscreen` モードを追加する。

## Objectives
- OSC 777 プロトコルに `render=fullscreen` モードを追加
- フルスクリーンオーバーレイ UI コンポーネントの実装
- スクロール・キーボードナビゲーション機能
- コードブロックのコピー機能
- 外部リンクの確認ダイアログ付きオープン機能

## Prerequisites

### Development Environment
- Bun (package manager)
- TypeScript 5.0+
- Tauri 2.x

### Dependencies (Existing)
- `marked` - Markdown パース
- `dompurify` - HTML サニタイズ
- `highlight.js` - シンタックスハイライト
- `@tauri-apps/plugin-shell` - 外部リンクオープン
- `@tauri-apps/plugin-clipboard-manager` - クリップボード操作

### Knowledge Requirements
- 既存の `MarkdownSessionManager` と `MarkdownRenderer` の構造
- OSC 777 プロトコルの理解
- Tauri プラグイン API

## Architecture Overview

### Component Interaction

```
MarkdownSessionManager
    │
    │ render=fullscreen を検出
    ▼
FullscreenMarkdownView
    │
    ├── MarkdownRenderer (既存) - HTML 生成
    │
    ├── LinkConfirmDialog - リンク確認
    │
    └── Clipboard API - コードコピー
```

### Design Approach
- 既存の `MarkdownSessionManager` を拡張し、`render=fullscreen` の場合は `FullscreenMarkdownView` に委譲
- フルスクリーンビューはターミナル状態に影響を与えない（オーバーレイ方式）
- 既存の `MarkdownRenderer.render()` を再利用してHTML生成

## Implementation Phases

### Phase 1: Type Extensions and RenderMode Update

**Goal**: 型定義を拡張し、fullscreen モードをサポートする基盤を整備

**Files to Modify**:
- `src/markdown/types.ts`:
  - `RenderMode` 型に `"fullscreen"` を追加
  - `FullscreenConfig` インターフェースを追加
  - `FullscreenState` インターフェースを追加

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `RenderMode` | レンダリングモードの定義 | - | "inline" \| "block" \| "fullscreen" |
| `FullscreenConfig` | フルスクリーン表示設定 | - | showCopyButtons, linkBehavior 等を含む |
| `FullscreenState` | フルスクリーン表示状態 | - | isActive を含む |

**Processing Flow**:
```
1. 既存の RenderMode 型を拡張
2. FullscreenConfig インターフェース定義
   ├─ showCloseButton: boolean
   ├─ alwaysShowScrollbar: boolean
   ├─ showCopyButtons: boolean
   └─ linkBehavior: "confirm" | "direct" | "disabled"
       (注: ビューアー側設定。OSCプロトコルでは制御しない。デフォルト: "confirm")
3. FullscreenState インターフェース定義
```

**Implementation Steps**:

1. **RenderMode 型の拡張**
   - "fullscreen" を union type に追加
   - 既存の型定義を保持

2. **FullscreenConfig インターフェース追加**
   - SPEC.md 3.2.1 に定義された構造を実装
   - デフォルト値のドキュメント化

3. **FullscreenState インターフェース追加**
   - SPEC.md 3.2.1 に定義された構造を実装

**Dependencies**:
- Requires: None
- Blocks: Phase 2, 3, 4, 5

**Testing Approach**:

*Unit Tests*:
- 型定義のコンパイル検証（TypeScript による静的チェック）

**Acceptance Criteria**:
- [ ] `RenderMode` に "fullscreen" が含まれる
- [ ] `FullscreenConfig` が正しく定義されている
- [ ] `FullscreenState` が正しく定義されている
- [ ] 既存のテストが通る

**Estimated Effort**: 小 (0.5 day)

---

### Phase 2: FullscreenMarkdownView Core Implementation

**Goal**: フルスクリーンオーバーレイの基本表示と Esc キーによる閉じる機能を実装

**Files to Create**:
- `src/markdown/fullscreen.ts` - フルスクリーンビュークラス
- `src/markdown/fullscreen.css` - フルスクリーンスタイル
- `src/markdown/fullscreen.test.ts` - ユニットテスト

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `FullscreenMarkdownView` | オーバーレイ管理とライフサイクル | MarkdownBlock が渡される | オーバーレイがDOMに挿入される |
| `show()` | フルスクリーン表示開始 | block.html が有効 | overlay が document.body に追加 |
| `close()` | フルスクリーン表示終了 | isActive() === true | overlay が削除、リソース解放 |
| `isActive()` | 表示状態確認 | - | boolean を返す |
| `dispose()` | リソース完全解放 | - | 全イベントリスナー削除 |

**Processing Flow**:
```
1. show() が呼ばれる
   ├─ 既存のビューがあれば close()
   ├─ 現在のフォーカス要素を保存 (previouslyFocusedElement)
   ├─ overlay 要素を作成
   ├─ content 要素を作成し block.html を挿入
   ├─ keydown イベントリスナーを登録
   ├─ document.body に追加
   └─ state.isActive = true

2. Esc キー押下
   └─ close() を呼び出し

3. close() が呼ばれる
   ├─ イベントリスナー削除
   ├─ overlay を DOM から削除
   ├─ 保存した要素にフォーカスを復元
   └─ state をリセット
```

**Implementation Steps**:

1. **FullscreenMarkdownView クラスの基本構造**
   - コンストラクタでの初期化
   - プライベート状態管理

2. **show() メソッド**
   - overlay/content 要素の作成
   - アクセシビリティ属性の設定（role="dialog", aria-modal="true"）
   - フォーカス管理

3. **close() メソッド**
   - クリーンアップ処理
   - 状態リセット

4. **キーボードイベントハンドラ**
   - Esc キーで close() を呼び出し
   - イベント伝播の制御

5. **CSS スタイル**
   - フルスクリーンオーバーレイのスタイル
   - コンテンツコンテナのスタイル
   - テーマ変数の使用

**Dependencies**:
- Requires: Phase 1
- Blocks: Phase 3, 4, 5

**Testing Approach**:

*Unit Tests*:
- show() でオーバーレイが DOM に追加される
- close() でオーバーレイが削除される
- Esc キーで close() が呼ばれる
- isActive() が正しい状態を返す
- 既存のビューがある場合 show() で閉じてから開く

*Manual Testing*:
- [ ] オーバーレイが画面全体を覆う
- [ ] Esc キーで閉じられる
- [ ] テーマカラーが適用されている

**Acceptance Criteria**:
- [ ] `show()` でフルスクリーンオーバーレイが表示される
- [ ] `close()` でオーバーレイが削除される
- [ ] Esc キーで閉じられる
- [ ] `isActive()` が正しい状態を返す
- [ ] アクセシビリティ属性が設定されている

**Estimated Effort**: 中 (2-3 days)

---

### Phase 3: Scroll and Navigation

**Goal**: マウスとキーボードによるスクロール・ナビゲーション機能を実装

**Files to Modify**:
- `src/markdown/fullscreen.ts` - スクロールメソッド追加
- `src/markdown/fullscreen.css` - スクロールバースタイル追加
- `src/markdown/fullscreen.test.ts` - テスト追加

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `scrollTo()` | 指定位置へスクロール | content が存在 | scrollTop が設定される |
| `scrollBy()` | 相対スクロール | content が存在 | scrollTop が delta 分変化 |
| `handleKeydown()` (拡張) | スクロールキー処理 | isActive() === true | 適切なスクロール処理 |

**Processing Flow**:
```
1. キーボードナビゲーション
   ├─ ArrowUp → scrollBy(-40)    // 1行上
   ├─ ArrowDown → scrollBy(40)   // 1行下
   ├─ PageUp → scrollBy(-viewportHeight)
   ├─ PageDown → scrollBy(viewportHeight)
   ├─ Home → scrollTo("top")
   └─ End → scrollTo("bottom")

2. マウススクロール
   └─ CSS overflow-y: scroll で自動処理

3. スクロールバー
   └─ alwaysShowScrollbar 設定に応じて表示
```

**Implementation Steps**:

1. **scrollTo() メソッド**
   - 数値または "top" / "bottom" を受け付ける
   - content.scrollTop を設定

2. **scrollBy() メソッド**
   - スムーズスクロール動作
   - delta 分の相対移動

3. **handleKeydown() 拡張**
   - ArrowUp/Down, PageUp/Down, Home/End キーの処理
   - イベント preventDefault

4. **スクロールバースタイル**
   - WebKit スクロールバーのカスタマイズ
   - テーマ変数の適用

**Dependencies**:
- Requires: Phase 2
- Blocks: Phase 5

**Testing Approach**:

*Unit Tests*:
- scrollTo("top") で scrollTop が 0 になる
- scrollTo("bottom") で最下部にスクロールする
- scrollBy(delta) で scrollTop が増減する
- ArrowDown キーで scrollBy が呼ばれる
- PageDown キーでページ分スクロールする
- Home キーで先頭にスクロールする

*Manual Testing*:
- [ ] マウスホイールでスクロールできる
- [ ] キーボードでスクロールできる
- [ ] スクロールバーが表示される
- [ ] スムーズスクロールが動作する

**Acceptance Criteria**:
- [ ] マウスホイールでスクロール可能
- [ ] Arrow キーで 1 行スクロール
- [ ] Page Up/Down で 1 ページスクロール
- [ ] Home/End で先頭/末尾へ移動
- [ ] スクロールバーが常に表示される
- [ ] スクロールが 60fps を維持

**Estimated Effort**: 小 (1-2 days)

---

### Phase 4: Code Copy Functionality

**Goal**: コードブロックにコピーボタンを追加し、クリップボードへのコピー機能を実装

**Files to Modify**:
- `src/markdown/fullscreen.ts` - コピーボタン追加・ハンドラ
- `src/markdown/fullscreen.css` - コピーボタンスタイル
- `src/markdown/fullscreen.test.ts` - テスト追加

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `addCopyButtons()` | コードブロックにボタン追加 | content が DOM に存在 | 各 pre > code にボタンが追加 |
| `handleCopyClick()` | コピーボタンクリック処理 | クリックイベント | クリップボードにコピー、フィードバック表示 |
| `showCopyFeedback()` | コピー結果フィードバック | ボタン要素 | "Copied!" または "Failed" 表示 |

**Processing Flow**:
```
1. show() 後に addCopyButtons() を呼び出し
   ├─ pre > code 要素を検索
   ├─ 各 pre に position: relative を設定
   └─ Copy ボタンを追加

2. コピーボタンクリック
   ├─ code 要素のテキストを取得
   ├─ writeText() でクリップボードにコピー
   │   ├─ 成功 → showCopyFeedback(true)
   │   └─ 失敗 → showCopyFeedback(false)
   └─ 2秒後に元の表示に戻す
```

**Implementation Steps**:

1. **addCopyButtons() メソッド**
   - showCopyButtons 設定の確認
   - pre > code 要素の検索
   - ボタン要素の作成とスタイリング

2. **handleCopyClick() メソッド**
   - コードテキストの取得
   - Tauri clipboard API の使用
   - エラーハンドリング

3. **showCopyFeedback() メソッド**
   - 成功/失敗の視覚フィードバック
   - タイムアウト後の復元

4. **コピーボタンスタイル**
   - 右上への配置
   - ホバー/アクティブ状態
   - 成功/失敗の色変化

**Dependencies**:
- Requires: Phase 2
- Blocks: Phase 5

**Testing Approach**:

*Unit Tests*:
- addCopyButtons() で各コードブロックにボタンが追加される
- コピーボタンクリックで writeText が呼ばれる
- showCopyFeedback(true) で "Copied!" が表示される
- showCopyFeedback(false) で "Failed" が表示される
- 2秒後に元の表示に戻る

*Manual Testing*:
- [ ] コピーボタンが表示される
- [ ] クリックでコードがコピーされる
- [ ] "Copied!" フィードバックが表示される
- [ ] 失敗時に "Failed" が表示される

**Acceptance Criteria**:
- [ ] 各コードブロックにコピーボタンが表示される
- [ ] クリックでコードがクリップボードにコピーされる
- [ ] 成功時に "Copied!" フィードバック
- [ ] 失敗時に "Failed" フィードバック
- [ ] 2秒後に元の表示に戻る

**Estimated Effort**: 小 (1 day)

---

### Phase 5: Link Handling with Confirmation

**Goal**: 外部リンククリック時の確認ダイアログと外部ブラウザでのオープン機能を実装

**Files to Create**:
- `src/markdown/link-dialog.ts` - 確認ダイアログクラス
- `src/markdown/link-dialog.css` - ダイアログスタイル
- `src/markdown/link-dialog.test.ts` - テスト

**Files to Modify**:
- `src/markdown/fullscreen.ts` - リンクハンドリング追加
- `src/markdown/fullscreen.css` - リンクスタイル追加
- `src/markdown/fullscreen.test.ts` - テスト追加
- `src/markdown/index.ts` - エクスポート追加

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `LinkConfirmDialog` | 確認ダイアログ管理 | - | ダイアログ表示/非表示 |
| `confirm()` | 確認プロンプト表示 | URL が有効 | Promise<boolean> を返す |
| `handleLinkClick()` | リンククリック処理 | クリックイベント | 確認後に外部ブラウザでオープン |

**Processing Flow**:
```
1. リンククリック
   ├─ href を取得
   ├─ http/https 以外 → 無視
   ├─ Ctrl/Cmd + クリック → 確認なしでオープン
   └─ 通常クリック
       ├─ linkBehavior === "direct" → 直接オープン
       ├─ linkBehavior === "confirm"
       │   └─ LinkConfirmDialog.confirm(url)
       │       ├─ true → オープン
       │       └─ false → 何もしない
       └─ linkBehavior === "disabled" → 何もしない

2. LinkConfirmDialog.confirm()
   ├─ ダイアログ作成
   ├─ URL をエスケープして表示
   ├─ キーボードイベント設定
   │   ├─ Enter → true で resolve
   │   └─ Esc → false で resolve
   ├─ ボタンイベント設定
   │   ├─ "開く" → true で resolve
   │   └─ "キャンセル" → false で resolve
   └─ オーバーレイクリック → false で resolve

3. openLink()
   └─ shell.open(url) で外部ブラウザで開く
```

**Implementation Steps**:

1. **LinkConfirmDialog クラス**
   - confirm() メソッドで Promise ベースの確認
   - URL のエスケープ処理
   - キーボードナビゲーション（Enter/Esc）
   - アクセシビリティ属性

2. **handleLinkClick() メソッド**
   - リンク要素の検出
   - Ctrl/Meta キー修飾子のチェック
   - linkBehavior 設定に基づく分岐

3. **openLink() メソッド**
   - Tauri shell.open() の使用
   - エラーハンドリング

4. **ダイアログスタイル**
   - モーダルオーバーレイ
   - 日本語 UI テキスト
   - フォーカス管理

**Dependencies**:
- Requires: Phase 2
- Blocks: Phase 6

**Testing Approach**:

*Unit Tests*:
- LinkConfirmDialog.confirm() でダイアログが表示される
- "開く" ボタンで true が返る
- "キャンセル" ボタンで false が返る
- Esc キーで false が返る
- Enter キーで true が返る
- URL が正しくエスケープされる
- handleLinkClick() で http/https リンクが処理される
- Ctrl+クリックで確認がバイパスされる

*Manual Testing*:
- [ ] リンククリックで確認ダイアログが表示される
- [ ] "開く" で外部ブラウザが開く
- [ ] "キャンセル" で何も起こらない
- [ ] Ctrl+クリックで直接開く
- [ ] 非 http(s) リンクは無視される

**Acceptance Criteria**:
- [ ] リンククリックで確認ダイアログが表示される
- [ ] "開く" で外部ブラウザでリンクが開く
- [ ] "キャンセル" でダイアログが閉じる
- [ ] Ctrl/Cmd + クリックで確認をバイパス
- [ ] http/https 以外のリンクは無視
- [ ] ダイアログは Esc で閉じられる
- [ ] Enter キーで "開く" が実行される

**Estimated Effort**: 中 (1-2 days)

---

### Phase 6: Session Manager Integration

**Goal**: MarkdownSessionManager を拡張し、render=fullscreen を処理

**Files to Modify**:
- `src/markdown/session.ts` - handleEnd() 拡張、fullscreen ビュー統合
- `src/markdown/session.test.ts` - テスト追加
- `src/markdown/index.ts` - エクスポート更新

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `handleBegin()` (拡張) | render=fullscreen の解析 | params に render がある | session.render が設定される |
| `handleEnd()` (拡張) | fullscreen モードの処理 | session.render === "fullscreen" | FullscreenMarkdownView.show() が呼ばれる |
| `handleFullscreenDisplay()` | フルスクリーン表示委譲 | MarkdownBlock | FullscreenMarkdownView に委譲 |

**Processing Flow**:
```
1. handleBegin() - render パラメータの解析
   └─ "fullscreen" を RenderMode として受け付ける

2. handleEnd() - render モードに応じた分岐
   ├─ render === "fullscreen"
   │   ├─ handleFullscreenDisplay(block) を呼び出し
   │   └─ null を返す（通常の DOM 挿入をスキップ）
   └─ render !== "fullscreen"
       └─ 既存の処理を継続

3. handleFullscreenDisplay()
   ├─ FullscreenMarkdownView インスタンス作成（遅延）
   └─ show(block) を呼び出し
```

**Implementation Steps**:

1. **handleBegin() の拡張**
   - render パラメータに "fullscreen" を追加
   - 既存の "inline" / "block" との互換性維持

2. **handleEnd() の拡張**
   - render モードのチェック
   - fullscreen の場合は専用処理に分岐
   - null を返して通常の DOM 挿入をスキップ

3. **handleFullscreenDisplay() メソッド**
   - FullscreenMarkdownView の遅延初期化
   - show() の呼び出し

4. **dispose() の拡張**
   - FullscreenMarkdownView の dispose 呼び出し

**Dependencies**:
- Requires: Phase 1, 2, 3, 4, 5
- Blocks: None

**Testing Approach**:

*Unit Tests*:
- handleBegin() で render=fullscreen が受け付けられる
- handleEnd() で render=fullscreen の場合 null が返る
- handleFullscreenDisplay() で show() が呼ばれる
- 既存の inline/block モードが影響を受けない

*Integration Tests*:
- 完全な OSC 777 シーケンスでフルスクリーン表示
- チャンク転送後のフルスクリーン表示
- 複数回のフルスクリーン表示リクエスト

**Acceptance Criteria**:
- [ ] `render=fullscreen` パラメータが正しく解析される
- [ ] フルスクリーンモード時に `FullscreenMarkdownView.show()` が呼ばれる
- [ ] フルスクリーンモード時に `handleEnd()` は `null` を返す
- [ ] 既存の inline/block モードは影響を受けない
- [ ] dispose() でフルスクリーンビューもクリーンアップされる

**Estimated Effort**: 小 (1 day)

---

## Complete File Structure

```
src/
├── markdown/
│   ├── index.ts              # Module exports (更新)
│   ├── types.ts              # Type definitions (拡張)
│   ├── session.ts            # Session management (拡張)
│   ├── session.test.ts       # Session tests (拡張)
│   ├── renderer.ts           # Markdown rendering (既存)
│   ├── renderer.test.ts      # Renderer tests (既存)
│   ├── theme.ts              # Theme integration (既存)
│   ├── theme.test.ts         # Theme tests (既存)
│   ├── fullscreen.ts         # NEW: Fullscreen view
│   ├── fullscreen.css        # NEW: Fullscreen styles
│   ├── fullscreen.test.ts    # NEW: Fullscreen tests
│   ├── link-dialog.ts        # NEW: Link confirmation dialog
│   ├── link-dialog.css       # NEW: Dialog styles
│   ├── link-dialog.test.ts   # NEW: Dialog tests
│   └── security.test.ts      # Security tests (既存)
```

**File Descriptions**:

| File | Responsibility |
|------|----------------|
| `types.ts` | RenderMode, FullscreenConfig, FullscreenState の型定義 |
| `session.ts` | OSC 777 セッション管理、fullscreen モード分岐 |
| `fullscreen.ts` | フルスクリーンオーバーレイの表示・操作・ライフサイクル |
| `fullscreen.css` | オーバーレイ、コンテンツ、コピーボタンのスタイル |
| `link-dialog.ts` | 外部リンク確認ダイアログ |
| `link-dialog.css` | ダイアログのスタイル |

## Testing Strategy

### Unit Testing

**Approach**:
- Bun の built-in test runner を使用
- happy-dom で DOM 操作をテスト
- モックを使用して Tauri API をテスト

**Test Coverage Goals**:
- Core logic: 80%+
- UI components: 70%+

**Key Test Areas**:

1. **FullscreenMarkdownView** (`fullscreen.test.ts`)
   - show/close ライフサイクル
   - キーボードナビゲーション
   - スクロール動作
   - コピー機能

2. **LinkConfirmDialog** (`link-dialog.test.ts`)
   - ダイアログ表示/非表示
   - ユーザー操作への応答
   - URL エスケープ

3. **Session Manager 拡張** (`session.test.ts`)
   - render=fullscreen の解析
   - fullscreen モードへの分岐

### Integration Testing

**Scenarios**:
1. OSC 777 シーケンスからフルスクリーン表示まで
2. チャンク転送を含む完全なフロー
3. 複数回の表示リクエスト

### Manual Testing Checklist

- [ ] フルスクリーンが画面全体を覆う
- [ ] Esc キーで閉じられる
- [ ] マウスホイールでスクロール
- [ ] キーボードでスクロール
- [ ] コードブロックのコピーボタンが動作
- [ ] リンクをクリックで確認ダイアログ
- [ ] Ctrl+クリックで直接オープン
- [ ] テーマカラーが適用される

## Dependencies

### External Dependencies (Existing)

| Package | Version | Purpose |
|---------|---------|---------|
| marked | ^17.0.0 | Markdown parsing |
| dompurify | ^3.0.0 | HTML sanitization |
| highlight.js | ^11.0.0 | Syntax highlighting |
| @tauri-apps/plugin-shell | ^2.x | External link opening |
| @tauri-apps/plugin-clipboard-manager | ^2.x | Clipboard operations |

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1: Type Extensions (no dependencies)
2. Phase 2: Core View (depends on Phase 1)
3. Phase 3: Scroll (depends on Phase 2)
4. Phase 4: Copy (depends on Phase 2)
5. Phase 5: Links (depends on Phase 2)
6. Phase 6: Integration (depends on all phases)

## Risk Assessment

### Technical Risks

1. **Tauri Plugin Compatibility**
   - **Risk**: shell.open() や clipboard API の動作差異
   - **Likelihood**: Low
   - **Impact**: Medium
   - **Mitigation**: 各プラットフォームでのテスト、エラーハンドリング

2. **DOM Focus Management**
   - **Risk**: フルスクリーン表示後のフォーカス問題
   - **Likelihood**: Medium
   - **Impact**: Low
   - **Mitigation**: 明示的なフォーカス管理、tabindex の設定

3. **Event Propagation**
   - **Risk**: キーイベントがターミナルに漏れる
   - **Likelihood**: Medium
   - **Impact**: Medium
   - **Mitigation**: stopPropagation の適切な使用

### Implementation Risks

1. **既存コードへの影響**
   - **Risk**: session.ts の変更が既存機能に影響
   - **Mitigation**: 既存テストの維持、分岐ロジックの明確化

## Performance Considerations

1. **レンダリング性能**
   - 既存の MarkdownRenderer を再利用
   - 大きなドキュメントでも 100ms 以内

2. **スクロール性能**
   - CSS scroll-behavior: smooth の使用
   - 60fps 維持

3. **メモリ管理**
   - 単一インスタンス方式（前回を閉じてから開く）
   - dispose() での確実なクリーンアップ

## Security Considerations

1. **XSS Prevention**
   - 既存の DOMPurify 設定を再利用
   - リンク URL のエスケープ

2. **Link Security**
   - 確認ダイアログによるユーザー同意
   - http/https のみ処理
   - shell.open() によるサンドボックス内実行

3. **Clipboard Access**
   - ユーザー操作（クリック）のみ
   - Tauri プラグインによるサンドボックス

## Open Questions

### Implementation-Specific:
- [ ] テキスト選択と Ctrl+C の動作確認（ブラウザ標準動作で対応予定）
- [ ] 非常に長いドキュメント（100KB以上）のパフォーマンス検証

## Success Metrics

### Functional Completeness
- [ ] 全ての Acceptance Criteria をパス
- [ ] 全てのユニットテストがパス
- [ ] 手動テストチェックリスト完了

### Quality Metrics
- [ ] テストカバレッジ 80%+（コアロジック）
- [ ] TypeScript エラーなし
- [ ] 既存テストが全てパス

### Performance Metrics
- [ ] フルスクリーン表示 < 100ms（1KB Markdown）
- [ ] スクロール 60fps 維持
- [ ] メモリリークなし

## References

- **Specification**: `doc/tasks/markdown-fullscreen/SPEC.md`
- **Existing Implementation**: `doc/tasks/markdown-display/IMPLEMENTATION.md`
- **Tauri Shell Plugin**: https://v2.tauri.app/plugin/shell/
- **Tauri Clipboard Plugin**: https://v2.tauri.app/plugin/clipboard-manager/
- **WAI-ARIA Dialog Pattern**: https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/

## Next Steps

1. **Review and Approval**
   - 計画のレビュー
   - オープンクエスチョンの解決

2. **Begin Implementation**
   - Phase 1 から順次実装
   - 各フェーズで TDD アプローチ

3. **Verification**
   - `/sdd.3-verify-plan` で整合性検証
   - `/sdd.6-verify` で SPEC 準拠確認
