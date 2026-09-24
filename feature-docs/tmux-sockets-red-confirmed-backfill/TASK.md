# Task Document: task0001 — Backfill red_confirmed on AC-3..AC-6

## Change

`test-docs/tmux-sockets-discover-flake/task0001.tests.yaml` の `acceptance_tests` のうち、AC-3・AC-4・AC-5・AC-6 の各エントリに `red_confirmed: false` を 1 行ずつ追記する。

- 追記位置は各エントリの `tests` と `red_reason` の間（AC-1 / AC-2 および既存 test-docs と同じキー順）
- インデントは既存の `red_confirmed` 行と同じ 4 スペース
- 値は YAML の真偽値 `false`
- それ以外の内容（`task_id`・`baseline_failures`・`final_failures`・AC-1・AC-2・各 `tests` の値・各 `red_reason` のブロックスカラー）は変更しない
- `src-tauri/` 配下のソース（`src-tauri/src/tmux_sockets.rs` を含む）、他の test-docs・feature-docs は変更しない

## Expected Result

- AC-1〜AC-6 のすべてのエントリが `tests` / `red_confirmed` / `red_reason` をこの順で持つ
- AC-3〜AC-6 の `red_confirmed` は真偽値 `false`、AC-1 は `true`、AC-2 は `false` のまま
- ファイルは YAML として読み込める
- 変更前との差分は、このファイル 1 つに対する 4 行の追加のみ（削除 0 行）
