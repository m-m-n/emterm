# IMPLEMENTATION: Dynamic Tab Title

## Summary

シェルのOSCシーケンスによるタイトル変更をタブタイトルに反映する。
既存のイベント基盤（`tab:titleChanged` 型定義、`TabBarUI` の購読）は整っており、未接続の通知パスを繋ぐ実装。

## Implementation Steps

### Step 1: TerminalApp にタイトル変更コールバックを追加

**ファイル:** `src/terminal-app/index.ts`

**変更内容:**
1. `onTitleChange` コールバックフィールドを追加（`onSessionExit` と同じパターン）
2. `setupPtyHandlers()` 内のタイトル変更検知ロジック（行286-295）で、ウィンドウタイトル更新に加えてコールバックを呼び出す
3. `onTitleChange()` メソッドを公開

```typescript
// フィールド追加
private titleChangeCallback: ((title: string) => void) | null = null;

// setupPtyHandlers() 内、行287のif文内に追加
if (this.titleChangeCallback) {
  this.titleChangeCallback(newTitle || "Terminal");
}

// 公開メソッド追加
onTitleChange(callback: (title: string) => void): void {
  this.titleChangeCallback = callback;
}
```

### Step 2: TabManager に updateTabTitle() メソッドを追加

**ファイル:** `src/tab-bar/tab-manager.ts`

**変更内容:**
`Tab.title` を更新し、`tab:titleChanged` イベントを発行するメソッドを追加。

```typescript
updateTabTitle(tabId: string, title: string): void {
  const tab = this.tabs.find((t) => t.id === tabId);
  if (tab && tab.title !== title) {
    tab.title = title;
    this.eventEmitter.emit("tab:titleChanged", { tabId, title });
  }
}
```

### Step 3: TabBarUI の updateTabTitle() にツールチップを追加

**ファイル:** `src/tab-bar/tab-bar-ui.ts`

**変更内容:**
1. `updateTabTitle()` で `title` 属性（ツールチップ）も設定する
2. `addTabElement()` で初期タイトルにもツールチップを設定する

```typescript
// updateTabTitle() の変更
updateTabTitle(tabId: string, title: string): void {
  const element = this.tabElements.get(tabId);
  if (element) {
    const titleElement = element.querySelector(".tab-title");
    if (titleElement) {
      titleElement.textContent = title;
    }
    element.setAttribute("title", title);
    element.setAttribute("aria-label", title);
  }
}

// addTabElement() でタブ作成時にもツールチップ設定
tabElement.setAttribute("title", tab.title);
```

### Step 4: main.ts でコールバックを接続

**ファイル:** `src/main.ts`

**変更内容:**
`createTerminalApp` ファクトリ内で、`onTitleChange` コールバックを接続する。
タブIDはクロージャで `createTerminalTabInternal` から渡す必要があるため、`TabManager` 側で接続する。

**再設計:** `main.ts` の `createTerminalApp` コールバックでは `tabId` を知らないため、`TabManager.createTerminalTabInternal()` 内で接続する。

```typescript
// tab-manager.ts の createTerminalTabInternal() 内、行 156-157 の後に追加:
const currentTabId = tabId;
terminalApp.onTitleChange((title: string) => {
  this.updateTabTitle(currentTabId, title);
});
```

これにより `main.ts` の変更は不要になる。

## File Change Summary

| ファイル | 変更内容 | 変更量 |
|---------|---------|-------|
| `src/terminal-app/index.ts` | コールバックフィールド + メソッド + 呼び出し | +10行 |
| `src/tab-bar/tab-manager.ts` | `updateTabTitle()` メソッド + コールバック接続 | +12行 |
| `src/tab-bar/tab-bar-ui.ts` | ツールチップ設定追加 | +3行 |

## Implementation Order

1. **Step 1** (TerminalApp) → Step 2 (TabManager) → Step 3 (TabBarUI)
   - Step 4 は Step 2 に統合（`createTerminalTabInternal` 内で接続）
   - 依存関係: Step 1 のコールバック API が必要で Step 2 で利用する

## Testing Strategy

### TypeScript 型チェック
```bash
bun run typecheck
```

### 手動テスト
1. `bun tauri dev` で起動
2. タブタイトルが "Terminal" で始まることを確認
3. シェルで `cd /tmp` → タブタイトルが変わることを確認（シェル設定による）
4. `echo -ne "\033]0;Custom Title\007"` → タブタイトルが "Custom Title" に変わることを確認
5. `echo -ne "\033]2;Window Title\007"` → タブタイトルが "Window Title" に変わることを確認
6. 長いタイトルが `...` で省略されることを確認
7. ホバーでツールチップにフルタイトルが表示されることを確認
8. `echo -ne "\033]0;\007"` → タブタイトルが "Terminal" に戻ることを確認
9. 複数タブを開き、各タブのタイトルが独立に更新されることを確認
