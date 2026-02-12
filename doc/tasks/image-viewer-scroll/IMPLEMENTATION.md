# IMPLEMENTATION: 画像ビューアー キーボードショートカット簡素化 & ホイールスクロール

## 実装順序

### Phase 1: キーボードショートカットの簡素化

#### 1.1 handleKeydown() の修正

**ファイル**: `src/image-viewer/display-mode.ts`

`handleKeydown()` メソッドから `case "1"` と `case "0"` のブロックを削除する。
これらのキーは `default` ブロックで `preventDefault()`/`stopPropagation()` されるため、ターミナルへの伝播はブロックされたまま。

```typescript
// Before:
switch (e.key) {
  case "f": ...
  case "1": ... // ← 削除
  case "0": ... // ← 削除
  case "Escape": ...
  default: ...
}

// After:
switch (e.key) {
  case "f": ...
  case "Escape": ...
  default: ...
}
```

#### 1.2 i18n helpText の更新

`0`, `1` キーの記載を削除する。

**ファイル**: `src/i18n/locales/en.json`
```json
// Before:
"helpText": "f:toggle 1:100% 0:fit Esc:close"
// After:
"helpText": "f:toggle Esc:close"
```

**ファイル**: `src/i18n/locales/ja.json`
```json
// Before:
"helpText": "f:切替 1:100% 0:フィット Esc:閉じる"
// After:
"helpText": "f:切替 Esc:閉じる"
```

### Phase 2: ホイールスクロールの実装

**ファイル**: `src/image-viewer/index.ts`

#### 2.1 wheelイベントリスナーの追加

`show()` メソッド内で、PanController 初期化後にoverlayへ `wheel` イベントリスナーを追加する。
`hide()` と `dispose()` でリスナーを解除する。

```typescript
// ImageViewer クラスに追加するフィールド:
private boundHandleWheel: (e: WheelEvent) => void;

// コンストラクタで bind:
this.boundHandleWheel = this.handleWheel.bind(this);

// show() で overlay に登録:
this.overlay.addEventListener("wheel", this.boundHandleWheel, { passive: false });

// hide() / dispose() で解除:
this.overlay.removeEventListener("wheel", this.boundHandleWheel);
```

#### 2.2 handleWheel() メソッドの実装

```typescript
private handleWheel(e: WheelEvent): void {
  e.preventDefault();

  // Ctrl+Wheel はブラウザズームブロックのみ（将来のズーム操作予約）
  if (e.ctrlKey) return;

  // パンできない状態（Fitモードまたは画像がビューポート内）なら無視
  if (!this.panController?.canPan()) return;

  const offset = this.panController.getOffset();

  if (e.shiftKey) {
    // Shift+Wheel: 横スクロール
    // OSによっては shiftKey=true のとき deltaX にホイール値が入る場合がある
    const delta = e.deltaX !== 0 ? e.deltaX : e.deltaY;
    this.panController.setOffset(offset.x - delta, offset.y);
  } else {
    // 通常: 縦スクロール
    this.panController.setOffset(offset.x, offset.y - e.deltaY);
  }
}
```

**パンの方向**: スクロールダウン(deltaY > 0)で画像を上に動かす（コンテンツが下にスクロールする自然な方向）ため、`offset.y - deltaY` とする。横方向も同様。

**バウンド制限**: `PanController.setOffset()` が内部で `bounds` にクランプするため、追加のバウンドチェックは不要。

### Phase 3: テスト

#### 3.1 DisplayModeController テスト更新

**ファイル**: `src/image-viewer/display-mode.test.ts`

既存テストの期待値を変更する（モード変更されないことを検証）:
- `"should switch to pixel mode on '1' key"` (line 482) → モードが変わらないことを検証
- `"should switch to fit mode on '0' key"` (line 509) → モードが変わらないことを検証

変更後のテスト:

```typescript
test("should not change mode on '1' key", () => {
  // Fitモードでコントローラー作成
  // '1' キーを dispatch
  // モードが fit のままであることを検証
});

test("should not change mode on '0' key", () => {
  // Pixelモードでコントローラー作成
  // '0' キーを dispatch
  // モードが pixel のままであることを検証
});
```

#### 3.2 ホイールスクロール テスト

**ファイル**: `src/image-viewer/index.test.ts`

ImageViewer のテストは DOM 環境とモック依存が重いため、PanController 単体のテストを優先する。
`panController.setOffset()` がバウンド内でクランプされることは `pan-controller.test.ts` で既にテスト済み。

ImageViewer の wheel ハンドラーは統合レベルのため、以下を検証:

```typescript
describe("wheel scroll", () => {
  test("vertical scroll updates pan offset Y");
  test("Shift+wheel updates pan offset X");
  test("Ctrl+wheel does not update pan offset");
  test("wheel in fit mode does not update pan offset");
});
```

## 対象ファイル一覧

| ファイル | 変更内容 |
|---------|---------|
| `src/image-viewer/display-mode.ts` | `handleKeydown()` から `0`, `1` ケースを削除 |
| `src/image-viewer/index.ts` | `boundHandleWheel` フィールド追加、`handleWheel()` メソッド追加、wheel リスナー登録/解除 |
| `src/i18n/locales/en.json` | `helpText` から `1:100% 0:fit` を削除 |
| `src/i18n/locales/ja.json` | `helpText` から `1:100% 0:フィット` を削除 |
| `src/image-viewer/display-mode.test.ts` | 既存の `0`, `1` キーテストの期待値を変更 |
| `src/image-viewer/index.test.ts` | ホイールスクロールテスト追加 |

## 検証コマンド

```bash
bun run typecheck
bun test src/image-viewer/
```
