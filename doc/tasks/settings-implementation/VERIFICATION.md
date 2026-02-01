# Verification Document: Settings Implementation (All 15 Phases)

## Overview

**Feature**: 設定項目の実装完了 (全 15 フェーズ)
**SPEC.md**: `doc/tasks/settings-implementation/SPEC.md`
**要件定義書**: `doc/tasks/settings-implementation/要件定義書.md`
**IMPLEMENTATION**: `doc/tasks/settings-implementation/IMPLEMENTATION-{1..15}.md`

---

## Build Verification

### Build Commands

```bash
# TypeScript type check
bun run typecheck

# TypeScript tests
bun test

# Rust tests
cargo test --manifest-path src-tauri/Cargo.toml
```

### Expected Result

- 全コマンドが exit code 0 で完了する
- エラーメッセージなし

---

## Phase-by-Phase Verification

### Phase 1: Font Family

**Automated Tests:**

| ID | Test Case | Expected Result | Test Type | File |
|----|-----------|-----------------|-----------|------|
| P1-T1 | `applySetting("fontFamily", "Fira Code")` updates font | fontFamily プロパティが更新される | Unit | `src/terminal/canvas-renderer.test.ts` |
| P1-T2 | Empty string falls back to default monospace | デフォルトモノスペースが使用される | Unit | `src/terminal/canvas-renderer.test.ts` |

**Manual Tests:**

- [ ] 設定でフォントファミリーを変更すると、ターミナルの表示フォントが変わる
- [ ] フォント変更後、文字幅・高さが再計測される（文字間隔が正しい）

---

### Phase 2: Line Height

**Automated Tests:**

| ID | Test Case | Expected Result | Test Type | File |
|----|-----------|-----------------|-----------|------|
| P2-T1 | `applySetting("lineHeight", 1.5)` updates character height | charHeight が更新される | Unit | `src/terminal/canvas-renderer.test.ts` |
| P2-T2 | Line height affects `getCharHeight()` | getCharHeight() の戻り値が変わる | Unit | `src/terminal/canvas-renderer.test.ts` |

**Manual Tests:**

- [ ] 設定で行の高さを変更すると、ターミナルの行間が変わる

---

### Phase 3: UI Theme

**Automated Tests:**

| ID | Test Case | Expected Result | Test Type | File |
|----|-----------|-----------------|-----------|------|
| P3-T1 | `applyUiTheme("dark")` sets `data-theme="dark"` | 属性が正しく設定される | Unit | `src/settings/settings-applier.test.ts` |
| P3-T2 | `applyUiTheme("light")` sets `data-theme="light"` | 属性が正しく設定される | Unit | `src/settings/settings-applier.test.ts` |
| P3-T3 | `applyUiTheme("system")` respects OS preference | OS 設定に追従する | Unit | `src/settings/settings-applier.test.ts` |

**Manual Tests:**

- [ ] "dark" テーマでダーク配色が適用される
- [ ] "light" テーマでライト配色が適用される
- [ ] "system" テーマで OS 設定に追従する
- [ ] テーマ切替時にタブバー、設定パネルの色が変わる

---

### Phase 4: Opacity

**Automated Tests:**

| ID | Test Case | Expected Result | Test Type | File |
|----|-----------|-----------------|-----------|------|
| P4-T1 | `applyOpacity(0.5)` sets CSS variable | `--terminal-opacity` が `0.5` に設定される | Unit | `src/settings/settings-applier.test.ts` |
| P4-T2 | `applyOpacity(0.5)` notifies renderers | `notifyRenderers("opacity", 0.5)` が呼ばれる | Unit | `src/settings/settings-applier.test.ts` |
| P4-T3 | `applySetting("opacity", 0.5)` updates opacity | CanvasRenderer の opacity プロパティが更新される | Unit | `src/terminal/canvas-renderer.test.ts` |
| P4-T4 | `setOpacity()` triggers forceRender | forceRender が呼ばれること | Unit | `src/terminal/canvas-renderer.test.ts` |

**Manual Tests:**

- [ ] 設定で透明度を変更すると、ターミナル背景の透明度が反映される
- [ ] テキストは完全に不透明を維持する
- [ ] 最小値 0.3 でも内容が視認可能

---

### Phase 5: Padding

**Automated Tests:**

| ID | Test Case | Expected Result | Test Type | File |
|----|-----------|-----------------|-----------|------|
| P5-T1 | `applyPadding(8)` sets CSS variable | `--terminal-padding` が `8px` に設定される | Unit | `src/settings/settings-applier.test.ts` |

**Manual Tests:**

- [ ] 設定でパディングを変更すると、ターミナルの周囲に余白が表示される
- [ ] パディング変更後、ターミナルのカラム数・行数が再計算される

---

### Phase 6: Show Scrollbar

**Dependencies:** Phase 9 (Scrollback Lines) が完了していること

**Automated Tests:**

| ID | Test Case | Expected Result | Test Type | File |
|----|-----------|-----------------|-----------|------|
| P6-T1 | `applyScrollbar("always")` maps to "scroll" | `--terminal-scrollbar-overflow` が `scroll` に設定される | Unit | `src/settings/settings-applier.test.ts` |
| P6-T2 | `applyScrollbar("never")` maps to "hidden" | `--terminal-scrollbar-overflow` が `hidden` に設定される | Unit | `src/settings/settings-applier.test.ts` |
| P6-T3 | `applyScrollbar("auto")` maps to "auto" | `--terminal-scrollbar-overflow` が `auto` に設定される | Unit | `src/settings/settings-applier.test.ts` |

**Manual Tests:**

- [ ] "always" でスクロールバーが常時表示される
- [ ] "never" でスクロールバーが非表示になる
- [ ] "auto" でスクロール可能時のみスクロールバーが表示される

---

### Phase 7: Cursor Style / Cursor Blink

**Automated Tests:**

| ID | Test Case | Expected Result | Test Type | File |
|----|-----------|-----------------|-----------|------|
| P7-T1 | `applySetting("cursorStyle", "bar")` changes cursor | カーソルスタイルが変更される | Unit | `src/terminal/canvas-renderer.test.ts` |
| P7-T2 | `applySetting("cursorStyle", "underline")` changes cursor | underline スタイルへの変更 | Unit | `src/terminal/canvas-renderer.test.ts` |
| P7-T3 | `applySetting("cursorBlink", false)` stops blink | 点滅タイマーが停止される | Unit | `src/terminal/canvas-renderer.test.ts` |
| P7-T4 | `applySetting("cursorBlink", true)` starts blink | 点滅タイマーが開始される | Unit | `src/terminal/canvas-renderer.test.ts` |

**Manual Tests:**

- [ ] 設定でカーソルスタイルを変更すると、カーソル形状がリアルタイムに変わる
- [ ] 設定でカーソル点滅を OFF にすると、カーソルが点滅しなくなる
- [ ] 設定でカーソル点滅を ON にすると、カーソルが点滅する

---

### Phase 8: Terminal Color Scheme

**Automated Tests:**

| ID | Test Case | Expected Result | Test Type | File |
|----|-----------|-----------------|-----------|------|
| P8-T1 | All 6 presets exist | 6 つのプリセットが定義されている | Unit | `src/terminal/colors.test.ts` |
| P8-T2 | Each preset has required fields | fg, bg, 16 ANSI colors が存在する | Unit | `src/terminal/colors.test.ts` |
| P8-T3 | "emterm" preset matches DEFAULT values | デフォルトプリセットが既存定数と一致 | Unit | `src/terminal/colors.test.ts` |
| P8-T4 | Selecting scheme updates colors | スキーム選択で色が更新される | Unit | `src/settings/settings-applier.test.ts` |
| P8-T5 | "default" clears overrides | デフォルトでカスタム色がクリアされる | Unit | `src/settings/settings-applier.test.ts` |

**Manual Tests:**

- [ ] 6 種のカラースキームプリセットが選択可能
- [ ] "eMterm" がドロップダウンの先頭に表示される
- [ ] スキーム変更時にターミナルの色が変わる
- [ ] "eMterm" でデフォルトカラーに戻る

---

### Phase 9: Scrollback Lines

**Automated Tests:**

| ID | Test Case | Expected Result | Test Type | File |
|----|-----------|-----------------|-----------|------|
| P9-T1 | Lines pushed off screen saved to scrollback | スクロールアウトした行がバッファに保存 | Unit | `src/terminal/state.test.ts` |
| P9-T2 | Buffer respects size limit | バッファサイズが設定値以内 | Unit | `src/terminal/state.test.ts` |
| P9-T3 | Buffer overflow drops oldest lines | 古い行が削除される | Unit | `src/terminal/state.test.ts` |
| P9-T4 | `getVisibleLines()` with offset 0 | 画面バッファを返す | Unit | `src/terminal/canvas-renderer.test.ts` |
| P9-T5 | `getVisibleLines()` with offset > 0 | スクロールバック行を返す | Unit | `src/terminal/canvas-renderer.test.ts` |

**Manual Tests:**

- [ ] 設定したスクロールバック行数分の履歴が保持される
- [ ] マウスホイールで過去の出力にスクロールできる
- [ ] スクロール中に新しい出力が来てもスクロール位置が維持される

---

### Phase 10: Shell Path / Shell Args

**Automated Tests:**

| ID | Test Case | Expected Result | Test Type | File |
|----|-----------|-----------------|-----------|------|
| P10-T1 | `PtyClient.spawn()` passes shell and args | shell と args が invoke に渡される | Unit (TS) | `src/pty/client.test.ts` |
| P10-T2 | `spawn()` without args omits args | args 省略時はパラメータなし | Unit (TS) | `src/pty/client.test.ts` |
| P10-T3 | Session creation with custom args (Rust) | カスタム引数でセッション作成 | Unit (Rust) | `src-tauri/src/pty/session.rs` |

**Manual Tests:**

- [ ] シェルパスを設定すると、新しいタブで指定シェルが起動する
- [ ] シェル引数を設定すると、起動時に引数が渡される
- [ ] 空のシェルパスではデフォルトシェルが使用される
- [ ] 設定変更は新しいタブから適用される（既存タブには影響しない）

---

### Phase 11: Scroll Speed

**Dependencies:** Phase 9 (Scrollback Lines) が完了していること

**Automated Tests:**

| ID | Test Case | Expected Result | Test Type | File |
|----|-----------|-----------------|-----------|------|
| P11-T1 | Scroll speed multiplier affects scroll amount | スクロール量が速度で乗算される | Unit | Phase 9 テストファイル内 |
| P11-T2 | Speed 1 scrolls minimum | 最小速度で最小量 | Unit | Phase 9 テストファイル内 |

**Manual Tests:**

- [ ] スクロール速度の設定値がスクロール量に反映される
- [ ] 値が大きいほどスクロール量が多い

---

### Phase 12: Bell Action

**Automated Tests:**

| ID | Test Case | Expected Result | Test Type | File |
|----|-----------|-----------------|-----------|------|
| P12-T1 | `handleBel()` calls onBell callback | `state.onBell` コールバックが呼ばれる | Unit | `src/terminal/handlers/c0_handlers.test.ts` |
| P12-T2 | `handleBel()` without callback does nothing | コールバック未登録時に例外が発生しない | Unit | `src/terminal/handlers/c0_handlers.test.ts` |
| P12-T3 | BEL with "visual" triggers flash | フラッシュイベントが発生 | Unit | `src/terminal-app/` 配下 |
| P12-T4 | BEL with "sound" triggers beep | ビープ再生メソッドが呼ばれる | Unit | `src/terminal-app/` 配下 |
| P12-T5 | BEL with "none" does nothing | 何もしない | Unit | `src/terminal-app/` 配下 |

**Manual Tests:**

- [ ] "visual" で BEL 文字受信時に画面がフラッシュする
- [ ] "sound" で BEL 文字受信時にビープ音が鳴る
- [ ] "none" で BEL 文字受信時に何もしない

---

### Phase 13: URL Detection

**Automated Tests:**

| ID | Test Case | Expected Result | Test Type | File |
|----|-----------|-----------------|-----------|------|
| P13-T1 | Detects `https://example.com` | URL が検出される | Unit | `src/terminal/url-detector.test.ts` |
| P13-T2 | Detects URL with path and query | パス・クエリ付き URL | Unit | `src/terminal/url-detector.test.ts` |
| P13-T3 | Detects `ftp://` URLs | FTP URL の検出 | Unit | `src/terminal/url-detector.test.ts` |
| P13-T4 | Returns empty for no URLs | URL なしで空リスト | Unit | `src/terminal/url-detector.test.ts` |
| P13-T5 | Multiple URLs on one line | 1 行に複数 URL | Unit | `src/terminal/url-detector.test.ts` |
| P13-T6 | Disabled detection returns empty | 無効化時は空 | Unit | `src/terminal/url-detector.test.ts` |

**Manual Tests:**

- [ ] ターミナル出力内の URL が検出・ハイライトされる
- [ ] Ctrl+クリックで外部ブラウザに遷移する
- [ ] 設定を OFF にすると URL 検出が無効になる

---

### Phase 14: Copy on Select

**Automated Tests:**

| ID | Test Case | Expected Result | Test Type | File |
|----|-----------|-----------------|-----------|------|
| P14-T1 | Selection completion triggers copy when ON | ON 時にコピーされる | Unit | `src/selection-v2/` 配下 |
| P14-T2 | Selection completion does not copy when OFF | OFF 時にコピーされない | Unit | `src/selection-v2/` 配下 |

**Manual Tests:**

- [ ] 設定 ON 時、テキスト選択完了でクリップボードにコピーされる
- [ ] 設定 OFF 時、選択だけではコピーされない

---

### Phase 15: Keybinds

**Automated Tests:**

| ID | Test Case | Expected Result | Test Type | File |
|----|-----------|-----------------|-----------|------|
| P15-T1 | Parses "Ctrl+T" correctly | 修飾キー+キーが正しくパースされる | Unit | `src/keybind/matcher.test.ts` |
| P15-T2 | Parses "Ctrl+Shift+T" correctly | 複数修飾キーのパース | Unit | `src/keybind/matcher.test.ts` |
| P15-T3 | Parses single key "F11" | 単独キーのパース | Unit | `src/keybind/matcher.test.ts` |
| P15-T4 | Matches KeyboardEvent against keybind | 一致判定が正しい | Unit | `src/keybind/matcher.test.ts` |
| P15-T5 | Non-matching event returns false | 不一致で false | Unit | `src/keybind/matcher.test.ts` |
| P15-T6 | Custom keybind triggers correct action | カスタムキーバインドが機能する | Unit | `src/tab-bar/keyboard-handler.test.ts` |
| P15-T7 | Default keybinds work without custom settings | デフォルトが機能する | Unit | `src/tab-bar/keyboard-handler.test.ts` |

**Manual Tests:**

- [ ] 設定で変更したキーバインドが実際のショートカットとして動作する
- [ ] デフォルトのキーバインドが初期値として機能する

---

## Code Quality Verification

### Format Check

```bash
# TypeScript (Biome or equivalent if configured)
# Note: Check project's actual formatter configuration
```

### Static Analysis

```bash
# TypeScript
bun run typecheck

# Rust
cargo clippy --manifest-path src-tauri/Cargo.toml
```

---

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | 全 15 フェーズの設定項目が実際に動作する | 各フェーズの手動テストチェックリストを全てパス |
| SC-2 | 既存のテストがすべてパスする | `bun test` と `cargo test` が全パス |
| SC-3 | 新規テストが各フェーズに追加されている | 各フェーズのテストファイルに新規テストが存在 |
| SC-4 | 設定変更が即座に反映される (< 100ms) | 手動操作で遅延が感じられないこと |
| SC-5 | 既存の設定ファイルとの後方互換性 | 旧設定ファイルでアプリが正常起動すること |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| Font Family | 1 | P1-T1, P1-T2 + 手動テスト |
| Line Height | 2 | P2-T1, P2-T2 + 手動テスト |
| UI Theme | 3 | P3-T1 ~ P3-T3 + 手動テスト |
| Opacity | 4 | P4-T1 ~ P4-T4 + 手動テスト |
| Padding | 5 | P5-T1 + 手動テスト |
| Show Scrollbar | 6 | P6-T1 ~ P6-T3 + 手動テスト |
| Cursor Style/Blink | 7 | P7-T1 ~ P7-T4 + 手動テスト |
| Color Scheme | 8 | P8-T1 ~ P8-T5 + 手動テスト |
| Scrollback Lines | 9 | P9-T1 ~ P9-T5 + 手動テスト |
| Shell Path/Args | 10 | P10-T1 ~ P10-T3 + 手動テスト |
| Scroll Speed | 11 | P11-T1, P11-T2 + 手動テスト |
| Bell Action | 12 | P12-T1 ~ P12-T5 + 手動テスト |
| URL Detection | 13 | P13-T1 ~ P13-T6 + 手動テスト |
| Copy on Select | 14 | P14-T1, P14-T2 + 手動テスト |
| Keybinds | 15 | P15-T1 ~ P15-T7 + 手動テスト |

---

## Edge Case Verification

| ID | Edge Case | Phase | How to Verify |
|----|-----------|-------|---------------|
| EC-1 | Invalid font family falls back to monospace | 1 | ユニットテスト |
| EC-2 | Unknown scheme name treated as "default" | 8 | ユニットテスト |
| EC-3 | Invalid shell path shows error | 10 | 手動テスト: 存在しないパスを設定 |
| EC-4 | Empty shell_args passes no arguments | 10 | ユニットテスト |
| EC-5 | Keybind string parsing handles edge cases | 15 | ユニットテスト |
| EC-6 | Scrollback buffer overflow | 9 | ユニットテスト |
| EC-7 | Opacity at minimum (0.3) | 4 | 手動テスト |

---

## Performance Verification

| Requirement | Threshold | How to Verify |
|-------------|-----------|---------------|
| 設定変更の反映速度 | < 100ms | 手動操作で遅延が感じられないこと |
| Canvas 再描画フレーム時間 | < 16ms | 開発者ツールの Performance パネルで確認 |

---

## Phase Dependencies Verification

```
Phase 9 (Scrollback) が Phase 6 (Scrollbar) より先に完了していること
Phase 9 (Scrollback) が Phase 11 (Scroll Speed) より先に完了していること
他の全フェーズは独立して実装可能
```

### Dependency Checklist

- [ ] Phase 9 完了後に Phase 6 を実装
- [ ] Phase 9 完了後に Phase 11 を実装

---

## Verification Summary

| Category | Items | Automated | Manual |
|----------|-------|-----------|--------|
| Build | 3 | Yes | - |
| Phase 1 Tests | 2 | Yes | 2 |
| Phase 2 Tests | 2 | Yes | 1 |
| Phase 3 Tests | 3 | Yes | 4 |
| Phase 4 Tests | 4 | Yes | 3 |
| Phase 5 Tests | 1 | Yes | 2 |
| Phase 6 Tests | 3 | Yes | 3 |
| Phase 7 Tests | 4 | Yes | 3 |
| Phase 8 Tests | 5 | Yes | 4 |
| Phase 9 Tests | 5 | Yes | 3 |
| Phase 10 Tests | 3 | Yes | 4 |
| Phase 11 Tests | 2 | Yes | 2 |
| Phase 12 Tests | 5 | Yes | 3 |
| Phase 13 Tests | 6 | Yes | 3 |
| Phase 14 Tests | 2 | Yes | 2 |
| Phase 15 Tests | 7 | Yes | 2 |
| Code Quality | 2 | Yes | - |
| SPEC Compliance | 5 | Partial | Yes |
| Edge Cases | 7 | Partial | Partial |
| Performance | 2 | - | Yes |

**Total**: 54 automated test cases, 41 manual test items, 5 success criteria
