# 🔍 実装自動検証レポート: mux Scroll Isolation

**対象機能**: mux Scroll Isolation
**VERIFICATION.md**: `doc/tasks/mux-scroll-isolation/VERIFICATION.md`
**プロジェクト**: emterm（native build, `refactor/promote-native-poc`）
**検証コミット**: 684fe107191ae9d9185cc103a8806639185ce4cb

---

## 📊 検証サマリー

| 検証項目 | 結果 | 詳細 |
|---------|------|------|
| ビルド | ✅ | `cargo check`(default) + `--no-default-features`(CLI) ともに exit 0・警告ゼロ（sdd.5-check 実施） |
| テスト実行 | ✅ | single-thread で 1791/1791 PASS（+21 が本機能）、統合 12 PASS |
| コードフォーマット | ✅ | 変更5ファイルは rustfmt 整形済み（crate 全体 fmt は方針により非実行） |
| 静的解析 | ✅ | 変更コードに rustc 警告なし（dead code/unused なし） |
| ファイル構造 | ✅ | 変更5ファイル全存在・新規作成ファイル 0 |
| SPEC.md適合性 | ✅ | FR1/FR2/FR3・NFR1/2/3 すべて実装・テスト紐付け済み |

**総合評価**: ✅ すべての自動検証項目をクリア（既存 flaky テスト1件を検出・本機能の回帰ではない）

---

## ✅ 自動検証項目

### ✅ ビルド検証（sdd.5-check）
- `cargo check --manifest-path src-tauri/Cargo.toml`: exit 0
- `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`（CLI-only feature gate）: exit 0
- リリースビルドはプロジェクト方針により非実行。`cargo check` で代替。

### ✅ テスト実行（sdd.5-check）
- lib: **1791 passed / 0 failed / 1 ignored**（single-thread）。本機能の新規テスト 21 件を含む。
- 統合（cli_subcommands）: 12 passed。
- 並列実行時に `app::tests::pump_all_shifts_selection_by_eviction_delta` が稀に失敗するが、**HEAD（本機能適用前）でも 1/5 程度で再現する既存 flaky**。原因は `pump_all` が実 PTY を pump するタイミング依存（合成した eviction baseline と実 core の eviction カウンタの不整合）で、本機能の変更経路（per-tab/per-pane scroll・snapshot）とは無関係。

### ✅ ファイル構造検証
- 新規作成ファイル: なし（計画どおり）
- 変更ファイル (5/5 存在):
  - ✅ `src-tauri/src/app.rs`
  - ✅ `src-tauri/src/tabs.rs`
  - ✅ `src-tauri/src/mux/window_group.rs`
  - ✅ `src-tauri/src/mux/ipc/handlers.rs`
  - ✅ `src-tauri/src/mux/ipc/reattach.rs`
- 計画で「変更不要」と判断された `window_host.rs` / `render/mod.rs` / `crates/term_core` は未変更（既存の `needs_full_redraw`→全行 emit、`reset_and_replay` で要件成立）。

### ✅ SPEC.md適合性検証

| 要件 | 状態 | 実装の所在 |
|------|------|-----------|
| FR1（オンデマンド snapshot に scrollback 同梱） | ✅ | `reattach.rs:38 build_snapshot_bytes`（clear+scrollback+screen 共有レイアウト）、`handlers.rs:445` pane scrollback を clear せず read → `:446` history-bearing snapshot、`:454` ログに scrollback サイズ併記。reattach 経路も共有レイアウトに統一 |
| FR2（切替時の全面再描画で残留行なし） | ✅ | 切替経路（`switch_to_tab` / local pane switch / inbound `SwitchWindow`）で `needs_full_redraw=true`。既存 `dirty_rows_this_frame` が `0..rows` を返し全グリッド再 emit |
| FR3（per-tab + per-pane scroll 位置） | ✅ | `Tab.scroll_position`（tabs.rs:175）、`switch_to_tab` save(1395)/restore(1399)。per-pane は `MuxWindowGroup.pane_scrolls`（window_group.rs:53）＋アクセサ、`pending_pane_switch_from`(183) を SwitchWindow(794) で latch・pump_all で drain |
| NFR1（非mux/単一窓 mux 回帰なし・scroll-pin 維持） | ✅ | `App.scroll_position` を単一アクティブ値として維持しスクロール mutator/scroll-pin 経路に未介入。1791 テスト green（回帰なし） |
| NFR2（O(1) save/restore・snapshot 転送量 reattach 同等） | ✅ | save/restore は数値1個の swap。snapshot は reattach と同構成で scrollback を載せるのみ |
| NFR3（復帰 pane が保存位置と整合） | ✅ | local/inbound 両経路で incoming pane の保存 offset を restore。native の tail 相対 offset モデルと整合 |

---

## 🐳 E2Eテスト結果

- Docker環境: 未構築（native build 用 E2E フレームワーク未検出。`sdd.yaml` の `e2e_test_command` 空）
- E2Eテスト: 対象外。切替時の視覚挙動は下記の手動項目で確認する。

---

## 📋 手動確認が必要な項目（E2E不可）

VERIFICATION.md から以下の手動テスト項目を抽出。実機で確認すること:

- [ ] **TS-8**: 長いユニットを表示 → 短いユニットへ切替 → 下部に前ユニットの残留行が出ない
- [ ] **TS-9**: pane A でスクロールアップ → B（または A をバックグラウンド）で出力を貯める → A へ戻ると A は保存位置と整合（scrolled-up は位置維持、bottom-pinned は新出力に追従）
- [ ] **TS-10**: pane に大量 scrollback を貯め、切替して戻り、snapshot 応答サイズが reattach と同等（snapshot-size warn ログで確認）で異常肥大がない
- [ ] **UC01 walkthrough**: pane A 大量出力 → pane B 切替 → A 復帰 → ホイール/Shift+PageUp で A の過去ログに到達できる（detach/再attach 不要）
- [ ] **回帰**: 単一窓 mux・非 mux タブのスクロール/描画が従来どおり。scroll-pin（スクロールアップ中の新出力で位置固定）が機能する

---

## 🎯 総合評価

✅ 自動検証はすべてクリア。FR1/FR2/FR3・NFR1/2/3 を実装し単体/統合テストで確認。手動項目（TS-8/9/10・UC01・回帰）の実機確認が残る。

### 📝 留意事項
- 並列テストの既存 flaky（`pump_all_shifts_selection_by_eviction_delta`）は本機能と無関係。別途 PTY を使わない決定的テストへ改善する余地がある（本タスク対象外）。
- 実装後の `cargo fmt` がクレート全体を再整形したため、機能と無関係な43ファイルの整形差分は HEAD に戻し、変更を機能5ファイルに限定した。
