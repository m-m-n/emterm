# VERIFICATION: Dynamic Tab Title

## Build Verification

### V-1: TypeScript 型チェック
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```
- [x] 型エラーなくパスすること

### V-2: TypeScript テスト
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"
```
- [x] 既存テストが全てパスすること (1516 pass, 0 fail)

### V-3: Rust テスト（影響なし確認）
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"
```
- [x] 既存テストが全てパスすること (8 passed, 0 failed)

## Code Verification

### V-4: TerminalApp コールバック
- [x] `titleChangeCallback` フィールドが追加されている (index.ts:50)
- [x] `onTitleChange()` 公開メソッドが追加されている (index.ts:703-705)
- [x] `setupPtyHandlers()` 内でタイトル変更時にコールバックが呼ばれる (index.ts:297-298)
- [x] 空タイトルの場合 "Terminal" がコールバックに渡される (index.ts:298 `newTitle || "Terminal"`)

### V-5: TabManager 更新メソッド
- [x] `updateTabTitle()` メソッドが追加されている (tab-manager.ts:598-604)
- [x] `Tab.title` が更新される (tab-manager.ts:601)
- [x] `tab:titleChanged` イベントが発行される (tab-manager.ts:602)
- [x] タイトルが同じ場合はイベントが発行されない（不要な更新防止）(tab-manager.ts:600 `tab.title !== title`)
- [x] `createTerminalTabInternal()` 内で `onTitleChange` コールバックが接続されている (tab-manager.ts:161-162)

### V-6: TabBarUI ツールチップ
- [x] `updateTabTitle()` で `title` 属性が設定される (tab-bar-ui.ts:288)
- [x] `updateTabTitle()` で `aria-label` 属性が更新される (tab-bar-ui.ts:289)
- [x] `addTabElement()` で初期タイトルの `title` 属性が設定される (tab-bar-ui.ts:193)

## Functional Verification (手動)

### V-7: OSC 0 タイトル更新
```bash
echo -ne "\033]0;Hello World\007"
```
- [ ] タブタイトルが "Hello World" に変わる

### V-8: OSC 2 タイトル更新
```bash
echo -ne "\033]2;Custom Title\007"
```
- [ ] タブタイトルが "Custom Title" に変わる

### V-9: デフォルトタイトル復帰
```bash
echo -ne "\033]0;\007"
```
- [ ] タブタイトルが "Terminal" に戻る

### V-10: ツールチップ表示
- [ ] タブにホバーするとフルタイトルがツールチップで表示される

### V-11: 長いタイトルの省略
```bash
echo -ne "\033]0;This is a very long title that should be truncated in the tab bar\007"
```
- [ ] タブ内のテキストが `...` で省略表示される
- [ ] ホバーでフルタイトルが表示される

### V-12: 複数タブの独立更新
- [ ] タブ1とタブ2で異なるタイトルが独立に表示される
- [ ] 一方のタイトル変更が他方に影響しない
