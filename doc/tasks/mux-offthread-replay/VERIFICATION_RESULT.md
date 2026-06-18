# 🔍 実装自動検証レポート: mux-offthread-replay

**検証日時**: 2026-06-18 21:50:06 JST
**対象機能**: mux Off-Thread Snapshot Replay (案a)
**VERIFICATION.md**: `doc/tasks/mux-offthread-replay/VERIFICATION.md`
**検証コミット**: `26b38e13ff0231041fe6f492a4023113ec8b3003`

---

## 📊 検証サマリー

| 検証項目 | 結果 | 詳細 |
|---------|------|------|
| ビルド（既定 + CLI-only） | ✅ | sdd.5-check 済み。両 feature gate exit 0・警告なし |
| テスト実行 | ✅ | sdd.5-check 済み。2470 passed / 0 failed（single-thread） |
| コードフォーマット | ✅ | sdd.5-check 済み。変更5ファイル rustfmt clean |
| 静的解析 / 死蔵コード | ✅ | sdd.5-check 済み。警告ゼロ・dead code なし |
| ファイル構造 | ✅ | 変更5ファイル全て存在 |
| SPEC.md 適合性 | ✅ | FR1–FR7 / NFR1–NFR4 すべて実装・検証に追跡可能 |

**総合評価**: ✅ すべての自動検証項目をクリア

> ビルド/テスト/フォーマット/静的解析は sdd.5-check で実行済み。本 sdd.6 では再実行せず、
> ファイル構造・SPEC 適合性・手動項目抽出に集中した（staleness なし: check と同一 HEAD）。

---

## ✅ ファイル構造検証

変更ファイル（計画どおり、新規ファイルなし）:

- ✅ `crates/term_core/src/terminal_core.rs` — 純粋ビルダー `build_from_snapshot` + `SnapshotReplay` + `static_assert_send`（`TerminalCore` / `SnapshotReplay`）+ `scrollback_capacity()` getter
- ✅ `crates/term_core/src/callbacks.rs` — `TerminalCallbacks: Send` スーパートレイト（Send 静的アサート成立に必須）。test `Recorder` を `RefCell`→`Mutex` / `Rc`→`Arc`
- ✅ `src-tauri/src/tabs.rs` — `OFFTHREAD_REPLAY_THRESHOLD_BYTES` / `PendingSwitch` / `SwapOutcome` / `pending_switch` / dispatch / poll / swap / 整合 / supersession
- ✅ `src-tauri/src/app.rs` — `pump_all` の per-tab poll + active-tab full-redraw 統合、`set_grid_size` を `&mut self.tabs` に
- ✅ `src-tauri/src/fold.rs` — `#[cfg(test)] region_count()` アクセサ（fold パリティ検証用）

---

## ✅ SPEC.md 適合性検証（FR/NFR ↔ 実装 ↔ テスト）

| 要件 | 実装（確認した結線） | 検証 | 結果 |
|------|---------------------|------|------|
| **FR1** off-thread 再パース + メインswap | `dispatch_offthread_replay`（tabs.rs:550, worker→`build_from_snapshot`）→ `poll_pending_switch`（tabs.rs:602, 非ブロッキング `try_recv`）→ `apply_offthread_swap`（Arc内 core 差替, tabs.rs:646）。`pump_all`←`window_host.rs:2794` | TS-1,3,4 | ✅ |
| **FR2** pending-switch 表示（旧pane維持） | dispatch 時に displayed core を reset しない（snapshot arm の off-thread 分岐, tabs.rs:765+） | TS-9, TS-11(手動) | ✅ |
| **FR3** ライブ出力の順序保証 | pending 中の対象pane `PtyOutput` を `pending.live_queue` に積む（tabs.rs:823+）→ swap 後 `apply_queued_live_output` で到着順適用 | TS-5, TS-8 | ✅ |
| **FR4** サイズしきい値 fast path | `if payload.len() < OFFTHREAD_REPLAY_THRESHOLD_BYTES`（=64 KiB, tabs.rs:745）→ 同期 `reset_frame_for_replay`、以上で off-thread | TS-4 | ✅ |
| **FR5** supersession（再切替・resize） | dispatch が前回の pending を置換（receiver drop で旧worker結果破棄）。`Tab::resize(&mut self)`（tabs.rs:2035）で resize が再ディスパッチ | TS-6, TS-12 | ✅ |
| **FR6** 整合の分割 | parse=worker（`build_from_snapshot`）、整合=メイン（`apply_replay_reconcile`）。marks/`evicted_total` は **worker構築core**（eviction=0起点）の drained 値から（tabs.rs:657-662） | TS-1, TS-8 | ✅ |
| **FR7** ワーカー失敗時の同期フォールバック | `poll_pending_switch` の `Disconnected`（worker panic）分岐 → `reset_frame_for_replay` + `apply_queued_live_output` + `log::warn`（tabs.rs:615-628） | TS-7 | ✅ |
| **NFR1** 不変条件維持 | swap 後 `pending_frame_reset` ラッチ + active-tab の per-pane scroll 復元 / selection drop / full redraw を既存経路に統合（app.rs:2827） | TS-8, TS-9 | ✅ |
| **NFR2** 決定的・非flakyテスト性 | worker parse は純関数 `build_from_snapshot`。新規テストは pump_all 非同期ループを足さず単体/単発呼び出しで検証 | TS-1,2,5 | ✅ |
| **NFR3** 移植性（Linux/Windows/CLI-only） | off-thread 経路は GUI 限定（tabs.rs/app.rs）、term_core 純ビルダーは常時ビルド。CLI-only check green | TS-3, TS-10 | ✅ |
| **NFR4** メモリ非退行（1タブ1core） | per-pane 常駐 core / LRU 無し。pending は target/handoff/queue のみ（揮発, swap/supersede でクリア） | 設計レビュー | ✅ |

**成功基準 SC-1〜SC-6**: SC-3〜SC-6 は自動テストで充足。SC-1・SC-2（UI 非ブロック・チラつき無し）は TS-11 手動確認に委譲。

---

## 🐳 E2E テスト結果

- Docker環境: 未構築（`sdd.yaml` の `e2e_test_command` 空、ネイティブ E2E フレームワーク無し）
- E2Eテスト: 対象外。ネイティブ端末は WebView を持たないため、手動 + `emterm.log` で確認する方針

---

## 📋 手動確認が必要な項目（E2E不可）

実機（GUI ビルド）での確認が必要。リリースビルドはユーザー明示時のみ。

- [ ] **TS-11**: 大きな scrollback（~2 MiB）の複数 pane を `Ctrl+B n n n` で高速切替 — UI が応答し続け、旧 pane が swap まで表示され、空白チラつきが無いこと
- [ ] swap 完了前に元 pane へ戻す（supersede to original）— 中間 pane のフラッシュが無いこと
- [ ] パースギャップ中に対象 pane へ出力 — swap 後に順序通り反映されること

### パフォーマンス検証（手動）
- [ ] ~2 MiB pane 切替時、swap を処理する `pump_all` イテレーションが scrollback サイズに比例してブロックしないこと（同期ベースライン 256 KiB=30ms / 1 MiB=117ms / 2 MiB=233ms = `mux-snapshot-reparse-offthread/VERIFICATION_RESULT.md` と比較）

### セキュリティ検証
- N/A（新規の外部入力・信頼境界なし。ローカル端末データのみ）

---

## 🎯 検証サマリー

### ✅ 自動検証結果
- ビルド（既定 + CLI-only）: 成功・警告なし
- テスト: 2470 passed / 0 failed（new: TS-1〜TS-9, TS-12 全緑）
- フォーマット / 静的解析 / 死蔵コード: クリーン
- ファイル構造: 完全（5/5）
- SPEC 適合性: FR1–7 / NFR1–4 すべて追跡可能

### 📝 留意事項
- 上記の手動テスト項目（TS-11 + 3件）を実機で実施し、確認後に本ファイルのチェックボックスを更新すること
- NFR4 は設計レビュー判定（自動テスト対象外、計画どおり）
- しきい値 `OFFTHREAD_REPLAY_THRESHOLD_BYTES` は 64 KiB。対象機での実測が異なれば implement で再調整可

**検証完了時刻**: 2026-06-18 21:50 JST

---

## 🔁 multi-review 由来のハードニング（2026-06-18 追記）

`/em-review:multi-review`（Claude 5 + GPT/Codex 4 = 9観点）の Critical/High 一致指摘を受け、ユーザー承認のうえ off-thread worker 経路を堅牢化した。

- **α (HIGH, Claude包括 + GPT sec/perf/arch)**: `dispatch_offthread_replay` の worker spawn を `.expect`（UI スレッドクラッシュ）から spawn 失敗時の同期フォールバックに変更。加えて `PendingSwitch.cancel: Arc<AtomicBool>` を追加し、supersede（新切替・resize・queue cap・sync snapshot）時に旧 worker へ協調キャンセルを通知。`term_core` に `process_pty_data_fully_cancellable` を追加し、`build_from_snapshot` は `Option<SnapshotReplay>` を返す（cancel 時 None・チャンク境界で離脱）。
- **β (HIGH, GPT sec/perf/arch + Claude perf/包括)**: `OFFTHREAD_LIVE_QUEUE_CAP_BYTES = 4 MiB` を追加。pending 中の live 出力が cap 超過で off-thread を中止し同期再パース＋蓄積出力適用へフォールバック（メモリ・swap 時バーストを有界化）。
- **γ (HIGH, GPT arch 単独)**: resize 時の live_queue 持ち越しの diff 提案は **FR3「出力欠落なし」と衝突**（spec レビュアー2名は現状を FR 準拠と評価）ため、ユーザー判断で**不採用・現状維持**（既知エッジとして記録）。
- medium 群（payload の Arc 化・Send supertrait・queue を active_pane_id でキー・dispatch/swap 分離の scroll/prompt-fold ずれ）は報告のみ（未修正）。

追加テスト: `test_build_from_snapshot_cancelled_returns_none`（term_core）、`offthread_live_queue_cap_falls_back_to_sync`（tabs）。

再検証: 全スイート single-thread **emterm 1813 + cli 12 + term_core 647 = 2472 passed / 0 failed**、CLI-only `cargo check` green、変更5ファイル rustfmt clean、スコープ逸脱なし。
