# Follow-up: snapshot-replay-scrollback-restore

## 背景

`doc/tasks/snapshot-replay-perf/` で `build_from_snapshot` に `scrollback_bypass` を導入。
`doc/tasks/snapshot-replay-daemon-routing/` で実環境タブ切替経路を新 path に乗せた結果、
2 MiB タブ切替が ~3000 ms → STRETCH < 100 ms 相当に到達。

副作用として、off-thread bypass 経路を通った場合 (snapshot payload ≥ 64 KiB)
は `scrollback_slim` が空のままになり、ユーザーはスクロールアップで履歴を辿れない
(live PTY 出力で再蓄積するまで)。

64 KiB 未満の synchronous `reset_frame_for_replay` 経路は従来通り scrollback を復元するため、
threshold で挙動が分岐する contract drift がある (codex-architecture multi-review 指摘)。

## 目標

off-thread bypass 経路でも、ユーザーが履歴を見える状態に復元する。

候補アプローチ:

1. **2nd-pass restore**: swap 完了直後に別 worker で scrollback だけ build (SlimCell 圧縮込み)、
   live drain が間に合えば差分を埋める
2. **lazy materialize**: scroll up 操作が発生したときだけ scrollback を build
3. **lightweight representation**: build_from_snapshot 中に SlimCell ではない
   軽量な scrollback 表現を埋めておく (post-bypass で SlimCell 化)

## 入口

- 関連ファイル:
  - `crates/term_core/src/terminal_core.rs::build_from_snapshot`
  - `crates/term_core/src/ring_buffer.rs::ring_push_blank` (bypass branch)
  - `src-tauri/src/tabs.rs::apply_offthread_swap`
  - `src-tauri/src/render/mod.rs::build_cell_grid` (`get_scrollback_length` 参照)
- 前段:
  - `doc/tasks/snapshot-replay-perf/` (bypass 機構)
  - `doc/tasks/snapshot-replay-daemon-routing/` (routing 修正、SPEC §Out of Scope 末尾の Known Limitation 参照)
- 関連 multi-review finding:
  - codex-architecture (high): threshold-dependent replay contract
  - claude-comprehensive (medium): empty scrollback after off-thread snapshot replay
