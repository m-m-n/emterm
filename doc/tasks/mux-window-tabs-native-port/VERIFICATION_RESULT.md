# Verification Result: mux Window Tabs (native-poc port)

**検証日**: 2026-06-14（FR1 再実装後に更新）
**対象 commit**: fec8d8da53e0f271f6e98fe2d8c9231c26ff6fe9 + 未コミットの FR1 配線修正
**総合判定**: ✅ **PASS（FR1 再実装で解消）**

## サマリー

| 項目 | 判定 |
|------|------|
| **FR1（egui タブバーでの mux グループ実描画）** | ✅ **PASS（配線済み）** |
| FR2 ウィンドウ状態モデル | ✅ PASS |
| FR3 APC 送信パス | ✅ PASS |
| FR4 受信ハンドリング | ✅ PASS |
| FR5 ウィンドウ切替（キー + サブタブクリック） | ✅ PASS |
| FR6 新規ウィンドウ | ✅ PASS |
| FR7 rename ダイアログ（入力ゲート配線済み） | ✅ PASS |
| FR8 move ダイアログ（入力ゲート配線済み） | ✅ PASS |
| FR9 close 反映 | ✅ PASS |
| FR10 prefix latch 配線 | ✅ PASS |
| FR11 設定ロード＋動的適用 | ✅ PASS |
| Build / Test / Format / Clippy | ✅ PASS（1399 passed / 1 既知 flake が単独 PASS） |
| SC-4 / NFR3（src-tauri・mux_ipc 不変更） | ✅ PASS |
| SC-5 / NFR2（socket 不追加） | ✅ PASS |
| デッドコード | ✅ 解消（下記） |

## FR1 再実装の内容

初回検証で FR1 は「`mux_group_render_model` / `hit_test_mux_group` がテスト専用デッドコードで、実タブバー描画パスに未配線」のため FAIL だった。以下の配線を追加して解消した。

1. **`ui/tab_bar.rs`**
   - `TabBarItem` に `mux_cells: Option<Vec<MuxGroupCell>>` フィールド + `with_mux_cells` ビルダーを追加。
   - `layout_tab_strip` を「visual cell」列（通常タブ / mux セル）方式に拡張。mux-attached タブを compact(`mux (N)`)/expanded(ヘッダ + サブタブ) で実描画し、アクティブサブタブをハイライト。
   - mux セルのクリックを `hit_test_mux_group` 経由で `TabEvent::MuxToggle` / `MuxSwitch` にルーティング。
   - `visual_cell_count` でグループ展開を考慮した等幅レイアウト。通常タブのみの場合は Phase 4-B と同一挙動（既存テスト不変）。
2. **`render/mod.rs`**: 各タブのビューモデル構築時、`mux_group.is_group()`（2窓以上）のとき `mux_group_render_model(group)` を `with_mux_cells` で付与。1窓に縮小すると `is_group()` が false になりグループ解除（FR9 整合）。
3. **`app.rs::apply_tab_event`**: `MuxToggle`→`group.toggle()`、`MuxSwitch`→タブをフォーカスし `Self::switch_to`（local active index + `SwitchWindow` 送信、FR5）。

→ `mux_group_render_model` / `hit_test_mux_group` / `MuxGroupCell` / `MuxGroupClick` がすべて実描画経路から使用される。`cargo check` のデッドコード警告も消滅。

### 追加テスト
- `ui::tab_bar::tests`: compact は1セル / expanded はヘッダ+サブタブ描画、compact クリック→`MuxToggle`、ヘッダクリック→`MuxToggle`、サブタブクリック→`MuxSwitch{window}`、`visual_cell_count` の検証。
- `app::tests`: `apply_tab_event` の `MuxToggle`（展開フリップ）/ `MuxSwitch`（active index 移動）/ 範囲外タブ no-op。

## デッドコード解消

- `app.rs mux_dialog_open()`: 未使用だった。本来 rename/move ダイアログ開放中のキーボード入力ゲート。`window_host.rs` の KeyboardInput / IME ハンドラに配線し、ダイアログ開放中はキーを egui（TextEdit / DragValue / Enter / Escape）へ転送して PTY/keybind/IME へリークさせないようにした（`handle_mux_dialog_key`）。これにより FR7/FR8 のダイアログ入力が実機で機能する。
- `settings.rs DEFAULT_MUX_STATUS_POSITION`: `StatusBarPosition::default()` と冗長だったため削除（FR11 の status_position 適用自体は既に配線済みで不変）。
- `ui/mod.rs TabEvent::Close(usize)`: Phase 4-B 由来の既存事項で今回のスコープ外（未変更）。

## 実機フィードバックによる FR1 再設計（WebView 準拠・トグル廃止）

実機確認で、当初実装した compact `mux (N)` ＋クリックでトグル展開する挙動が WebView と異なることが判明（WebView の `MuxTabGroup` トグルは実 UI 未配線のデッドコードで、実際は `main.ts`/`tab-bar-ui.ts` の `renderMuxSubTabs` が**常に各ウィンドウを `[N] 名前` のサブタブで表示**、クリックで切替のみ）。ユーザー合意のもと WebView 準拠に再設計した。

- compact/expanded トグルを廃止（`MuxWindowGroup` から `expanded`/`toggle`/`expand`/`compact`/`state`/`is_expanded`/`compact_label`/`GroupState` を削除、`TabEvent::MuxToggle`・`MuxGroupCell`・`MuxGroupClick`・`hit_test_mux_group` を削除）。
- mux アタッチ中は常に各ウィンドウを `[N] 名前` のサブタブで描画（`MuxSubTabCell` + `mux_group_render_model`）。`is_group()` を「windowCount>=1」に変更し、1ウィンドウでもサブタブ表示。0 ウィンドウでのみグループ解除（FR9）。
- サブタブクリック → `TabEvent::MuxSwitch` → `switch_to`（ローカル active index + SwitchWindow + RequestPaneSnapshot）。
- `tab_always_expand` 設定は描画に影響しなくなった（ローダーは存続）。`apply_settings` の expand 反映を削除。
- テスト更新（tab_bar/window_group/app）、1392 passed。

## 実機検証で判明した追加修正（画面スナップショット要求）

FR1 配線後の実機検証で「attach・ウィンドウ切替時にターミナル内容が復元されない」問題が判明。原因は **native-poc が `RequestPaneSnapshot` を一切送っていなかった**こと。WebView (`mux-window-manager.ts` の `switchMuxWindow` / 再アタッチ経路) は attach・switch・remote-switch のたびに `requestPaneSnapshot(activePaneId)` を送り、daemon が `PtyOutput` フレーム（画面リセット＋replay）で応答する。native は受信側 (`apply_mux_message` の `PtyOutput`/`Snapshot`) は実装済みだったが、**要求の送信が欠落**していた（SPEC のデータフローは「daemon が switch で自動的に Snapshot を push する」前提だったが、実プロトコルは明示要求が必要）。

修正（`Tab::request_pane_snapshot` を追加し3経路に配線）:
- `tabs.rs` Welcome ingest: seed 後にアクティブウィンドウの pane へ要求（初回 attach の画面復元）
- `app.rs switch_to`: SwitchWindow 送信後に新ウィンドウの pane へ要求（FR5 ローカル切替の画面復元）
- `tabs.rs` 受信 SwitchWindow（リモート切替）: 同期後に要求
- `apc.rs` に `RequestPaneSnapshot` のエンコード往復テストを追加（1400 passed）

## スコープ外（別タスク）: ライブ PTY ストリーム / 単一所有クライアント

実機の診断ログで以下を確定:
- native は `RequestPaneSnapshot` の応答 `PtyOutput`（例: 12626 bytes）を**正しく受信・描画**する（初期画面・ウィンドウ切替時の画面復元は動作）。
- しかし**継続的なライブ PtyOutput ストリームが native に流れてこない**（rename ブロードキャストは流れ続けるが、`top` 等のライブ更新・打鍵エコーが来ない）。

原因は **daemon（src-tauri/src/mux）の単一所有クライアント設計**:
- ウィンドウ rename 等のメタデータは broadcast チャネル（`connection.rs:90 notify_tx().subscribe()`）で全クライアントに配信 → native も受信。
- ライブ PTY 出力は pane の単一 `owned_tx` 経由で**所有クライアント1つにのみ**配信。新規 attach 時に旧クライアントを kick（`connection.rs:207`「another client attaches → evicts us」）。
- WebView が同時 attach のまま＋native の attach がストリーム上で**2重に出ている**ため、native がライブストリームを安定所有できていない。

→ これは「WebView を閉じて native が引き継ぐ（単一所有ハンドオフ/takeover）」という daemon/bridge 側の別タスク。native-poc の受信・描画は正常で、本タスク（native 側タブグループ UI 移植）の範囲外。調査の起点: native attach がストリーム上で2重に出る点（`emterm mux attach` の二重 attach / kick 合戦の疑い）。

## 実機検証で確認できた範囲（本タスクの完成範囲）
- サブタブ描画（`[N] 名前`）・クリック切替・デタッチでタブ解除・初期画面/切替時のスナップショット復元、いずれも実機で動作。

## PASS 詳細（再掲）

- **キーボード経路**: `window_host.rs:2179` で `observe_mux_key` → `handle_mux_outcome`。prefix n/p/0-9/c/,/m 配線済み。
- **SC-4/NFR3**: 差分は `native-poc/` 配下のみ。`src-tauri/`・`crates/mux_ipc/` 変更ゼロ。
- **SC-5/NFR2**: `native-poc/src/` に socket オープンなし。APC inband 維持。
- **テスト**: 1399 passed。`app::tests::pump_all_shifts_pending_anchor_by_eviction_delta` はフル並列負荷下で稀に揺れる既存の real-PTY タイミングテストで、単独実行では PASS（今回の変更とは無関係）。
