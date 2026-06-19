# 検証結果レポート: Tab Bar Horizontal Scroll and Active Indicator

**検証日時**: 2026-06-19
**対象機能**: tab-bar-scroll-and-active-indicator
**VERIFICATION.md**: `doc/tasks/tab-bar-scroll-and-active-indicator/VERIFICATION.md`
**SPEC.md**: `doc/tasks/tab-bar-scroll-and-active-indicator/SPEC.md`
**検証方法**: コード読解 + `git diff` によるファイル構造検証
**注**: build / test / format / static analysis は sdd.5-check で検証済みのため再実行していない（コード変更なし＝stale なし）。release ビルドも実行していない。GUI の見た目・操作挙動は自動検証不可のため、ユーザー実機手動確認項目として分離して記載する。

---

## 総合判定

**PASS**（コードレベルで検証可能な全 FR/NFR が準拠。残りは実機手動確認のみ）

| カテゴリ | 結果 |
|----------|------|
| ファイル構造検証 | PASS（変更は期待した 4 ファイルのみ。`src/` 未変更） |
| FR1 スクロールバー非表示 | PASS（コード根拠あり） |
| FR2 ホイール水平スクロール | PASS（コード根拠あり／挙動は実機確認） |
| FR3 Shift+ホイール水平スクロール | PASS（コード変更不要を確認／挙動は実機確認） |
| FR4 アクティブ scroll-into-view | PASS（フラグ・伝播・クリアの全経路をコードで確認） |
| FR5 アクティブインジケーター一意化 | PASS（ゲート条件・read-only をコードで確認） |
| NFR1 パフォーマンス | PASS（コードレビュー） |
| NFR2 互換性 | PASS（コード）／実機スモークは手動 |
| NFR3 スコープ分離 | PASS（`git diff` に `src/` 変更なし） |
| egui API 存在確認 | PASS（vendored egui 0.29.1 に全シンボル存在） |

---

## 1. ファイル構造検証

`git diff --stat HEAD`（作業ツリー）の結果、変更されたソースファイルは以下の **4 ファイルのみ**:

```
 src-tauri/src/app.rs         | 168 +++++++++
 src-tauri/src/render/mod.rs  |   6 +-
 src-tauri/src/ui/tab_bar.rs  | 258 +++++++++++++++++++-
 src-tauri/src/window_host.rs |   5 +
```

- これは IMPLEMENTATION.md / tasks.yaml の `files_modify` と完全一致。
- 新規作成ファイルなし（`files_create: []` 通り。`doc/tasks/...` の untracked ディレクトリのみ）。
- **`git diff --stat HEAD -- src/` は空** → WebView 版タブバー未変更（**NFR3 / SC-4 充足**）。
- **`git diff --stat HEAD -- src-tauri/src/mux/window_group.rs` は空** → FR5 の mux active-window 状態が read-only（コード非変更）であることを構造的に保証。

**判定: PASS**

---

## 2. SPEC 機能要件準拠チェック（コード根拠付き）

### FR1 — スクロールバーなしのオーバーフロー水平スクロール

- **オーバーフロー分岐のゲート**: `tab_bar.rs:219` `if needed_w > scroll_w {` の中でのみ ScrollArea を構成。fit パス（等幅・非スクロール）は未変更。
- **スクロールバー非表示**: `tab_bar.rs:242`
  `.scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)` を `ScrollArea::horizontal()` に付与。
- import: `tab_bar.rs:28` `use egui::scroll_area::ScrollBarVisibility;`
- スクロール自体は維持（`AlwaysHidden` はバー描画のみ抑制）。

**判定: PASS**（バー非表示の見た目は M-7 で実機確認）

### FR2 — 縦ホイール → 水平スクロール

- `tab_bar.rs:235` `ui.style_mut().always_scroll_the_only_direction = true;` を ScrollArea の strip scope（`allocate_ui_with_layout` 内、`.show` の前）に設定。
- 水平専用 ScrollArea のため、このフラグで縦ホイールデルタが単一（水平）軸へ折り込まれる。
- オーバーフロー分岐内に限定されており、fit パスや他の ScrollArea には影響しない。

**判定: PASS**（実際にホイールでスクロールするかは M-1 で実機確認）

### FR3 — Shift+ホイール → 水平スクロール（コード変更不要）

- tab_bar.rs 側に Shift 関連のコード変更は **なし**（diff で確認）。
- egui 0.29.1 の入力層がこれを担保: `input_state/mod.rs:327-331`
  ```rust
  if modifiers.shift {
      // Treat as horizontal scrolling.
      delta = vec2(delta.x + delta.y, 0.0);
  }
  ```
  Shift+縦ホイールが水平軸に折り込まれ、水平 ScrollArea がそのまま消費する。IMPLEMENTATION.md の参照（input_state/mod.rs:327-331）と一致。

**判定: PASS（コード変更不要を確認）**（挙動は M-2 で実機確認）

### FR4 — キーボード切替時のアクティブセル scroll-into-view

経路を全てコードで確認:

1. **フラグ定義**: `app.rs:264` `scroll_active_tab_into_view: bool,` ／ 初期化 `app.rs` コンストラクタ `scroll_active_tab_into_view: false,`。
2. **plain-tab で立てる**: `app.rs:1405` `self.scroll_active_tab_into_view = true;`。
   `switch_to_tab`（`app.rs:1385`）は冒頭 `app.rs:1386` `if idx >= self.tabs.len() || idx == self.active { return; }` で no-op を早期 return。よってフラグはアクティブが**実際に動いた時のみ**立つ。`NextTab`/`PrevTab`/`JumpTab` は全てこの関数を経由（単一サイトで網羅）。
3. **mux 切替で立てる（TS-9 option b 厳密ガード）**: `dispatch_mux_action` で切替前の active index を `app.rs:2193` `let active_before = tab.mux_group.as_ref().map(|g| g.active_index());` に保持し、`MuxActionOutcome::Changed` ブロック内 `app.rs:2276` `if active_before != active_after { self.scroll_active_tab_into_view = true; }` で**実際に active が動いた時のみ**立てる。同一ウィンドウへの digit ジャンプ（`Changed` を返すが active 不変）ではフラグは立たない。
4. **read-only アクセサ**: `app.rs:3003` `pub fn scroll_active_tab_into_view(&self) -> bool`。
5. **render での伝播**: `render/mod.rs:248`
   `crate::ui::tab_bar::draw(ctx, &items, app.active, app.scroll_active_tab_into_view())`。`draw_terminal` は `&App`（immutable）なので read のみ。
6. **strip への伝播**: `draw`（`tab_bar.rs`）と `layout_tab_strip` に `scroll_active_into_view: bool` 引数を追加。
7. **アクティブセル rect 捕捉**:
   - plain: `tab_bar.rs:672` `active_cell_rect = Some(rect);`（`is_active` 時）。
   - mux: `tab_bar.rs:534-537` `if tab == active_idx && is_active_cell { ... active_cell_rect = Some(rect); }`（アクティブ mux タブ内のアクティブウィンドウのサブタブ）。
8. **scroll_to_rect の単一呼び出し**: `tab_bar.rs:735` `ui.scroll_to_rect(rect, None);` を `if scroll_active_into_view { if let Some(rect) = active_cell_rect { ... } }` でガード（フラグ set かつ rect 捕捉済みのときのみ 1 回）。
9. **マウススクロール起因で発火しないこと（毎フレームクリア）**: `window_host.rs:1360` `app.clear_scroll_active_tab_into_view();` が `egui_ctx.run(...)` クロージャ（`window_host.rs:1355` で閉じる）の直後・post-frame に置かれ、`&mut App` で毎フレームクリア。one-shot 寿命が 1 フレームに限定されるため、マウススクロール後の無関係 repaint でアクティブタブが強制的に引き戻されない。

**判定: PASS**

### FR5 — 混在タブでのアクティブインジケーター一意化

- **ゲート条件**: `tab_bar.rs:534` `if tab == active_idx && is_active_cell {`（`is_active_cell = mux_cell.active`）。親 mux タブがアクティブタブの時のみサブタブのインジケーターバーを描画。
- 非アクティブ親 mux タブはバー非描画 → strip 全体でインジケーターは 1 つ。
- **ラベル色は据え置き**: ラベル色は従来の `mux_cell.active` ベースの強調を維持（バーのみゲート）。
- **mux active-window 状態は read-only**: `draw`/`layout_tab_strip` は per-frame の immutable `&[TabBarItem]` スナップショット上で動作。`tab`/`active_idx` は plain index、`mux_cell.active` はコピーされた bool であり、`MuxWindowGroup` には到達不能。`git diff` で `mux/window_group.rs` 未変更も確認済み（構造的に保証）。

**判定: PASS**（見た目の一意性は M-5 で実機確認）

---

## 3. 非機能要件

- **NFR1（毎フレームコスト）**: FR5 は boolean ゲート 1 つ、FR4 はフラグが立った時のみ rect 1 個の `scroll_to_rect` 1 回。layout ループ内に新規アロケーションなし（diff 上、ループ内追加は `active_cell_rect = Some(rect)` の代入のみ）。**PASS（コードレビュー）**
- **NFR2（互換性）**: タブのセル割当・クリックルーティング・ドラッグは未変更（FR5 はペインタ呼び出しの抑制のみ）。既存テストの呼び出しサイトは `false` 引数を追加して維持。**PASS（コード）／実機スモークは M-6**
- **NFR3（スコープ分離）**: `git diff HEAD -- src/` 空 → WebView 未変更。**PASS**

## 4. セキュリティ

- 該当なし（UI 描画変更。既存の wheel/key イベント以外の新規外部入力なし）。

## 5. E2E テスト

- このプロジェクトに E2E インフラなし → **N/A**。

## 6. egui API 存在確認（vendored egui 0.29.1）

実装が使用する 4 シンボルが全て vendored egui 0.29.1 に存在することを確認:

- `ScrollBarVisibility::AlwaysHidden` — `scroll_area.rs:108`（FR1）
- `ScrollArea::scroll_bar_visibility()` builder — `scroll_area.rs:288`（FR1）
- `Spacing::always_scroll_the_only_direction` — `style.rs:289`（FR2）
- `Ui::scroll_to_rect()` — `ui.rs:1465`（FR4）
- Shift+wheel→horizontal の入力層マッピング — `input_state/mod.rs:327-331`（FR3 の「変更不要」根拠）

---

## 7. ユーザー実機手動確認が必要な項目

このアプリは DevTools 不可・GUI 自動操作不可のため、以下はユーザーが実機で確認する必要がある（コードレベルの準拠は上記で確認済み。残るは見た目・操作挙動）。

- [ ] **M-1（FR2）**: タブが幅に収まらない状態でタブバー上にポインタを置き、ホイールを縦に回す → strip が左右にスクロールし、選択タブは変わらない。
- [ ] **M-2（FR3）**: タブバー上で Shift+ホイール → strip が水平スクロールする。
- [ ] **M-3（FR4）**: アクティブタブが画面外の状態で Ctrl+PageUp/PageDown / Ctrl+Tab / Ctrl+1..9 → 新たにアクティブになったタブが画面内にスクロールイン。
- [ ] **M-4（FR4・スナップバック無し）**: マウスで strip をスクロール後、無関係な再描画を起こす → アクティブタブが強制的に画面内へ引き戻されない。
- [ ] **M-5（FR5・インジケーター一意性）**: mux タブ（アクティブなサブタブあり）と plain タブが共存する状態で plain タブをアクティブ化 → mux サブタブのバーが消える（バーは strip 全体で 1 本）。mux タブを再アクティブ化 → 以前アクティブだったウィンドウにバーが戻る。
- [ ] **M-6（NFR2・既存機能スモーク）**: plain タブのクリック切替・ドラッグ並べ替え・mux サブタブのクリック切替・「+」/ギアボタン → いずれも従来通り動作。
- [ ] **M-7（FR1・スクロールバー非表示）**: タブがオーバーフローした状態でスクロールバーが表示されないことを確認。

---

## 8. SPEC.md 成功基準（SC）対応状況

| ID | 基準 | 状況 |
|----|------|------|
| SC-1 | FR1–FR5 全実装 | コードで確認 → PASS |
| SC-2 | 全テストシナリオ合格 | sdd.5-check で検証済み（本検証では再実行せず）。新規/既存テストの存在は diff で確認 |
| SC-3 | クリック切替・ドラッグ・mux クリック・「+」/ギアの非回帰 | コード上非変更を確認 → 実機スモークは M-6 |
| SC-4 | WebView タブバー未変更 | `git diff` に `src/` 変更なし → PASS |

---

**結論**: コードレベルで検証可能な FR1〜FR5・NFR1〜NFR3・SC-1/SC-4 は全て準拠（PASS）。残るは GUI の見た目・操作挙動に関するユーザー実機手動確認（M-1〜M-7）のみ。**総合判定: PASS**。
