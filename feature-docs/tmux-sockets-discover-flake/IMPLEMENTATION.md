# Implementation Plan: tmux-sockets-discover-flake

## Overview

`src-tauri/src/tmux_sockets.rs` のテスト `discover_returns_only_the_live_socket` が既定の並列テストスレッドで非決定的に失敗する fork 窓レースを、テストモジュール内の fixture 構築の変更のみで解消する。本番コードは変更しない（NFR2）。

## Technology Stack

- **Language**: Rust（`src-tauri` クレートの `#[cfg(unix)]` テストモジュール）
- **Key libraries**: 既存依存のみを使用する — libc（raw Unix ソケット操作、既存依存）、tempfile（テスト用一時ディレクトリ、既存 dev 依存）、std
- **新規依存**: なし（ライセンスレビュー照合用: 本 feature で追加される依存は 0 件。`project.license: MIT` への影響なし）

## Layer Structure

対象は `src-tauri/src/tmux_sockets.rs` 内の `#[cfg(test)] mod tests` のみ。アプリケーション層・UI 層・本番の `discover_in` / `probe_unix_socket` は関与しない。レイヤー構造・依存方向に変更はない。

## Shared Components

なし。本 feature は単一タスク（task0001）で構成され、タスク間で共有されるコンポーネントは存在しない。

## Conventions

- テストは `test/README.md` の規約に従う: インライン `#[cfg(test)] mod tests`、既存テストの構築スタイルの踏襲、完了前にフルスイート `--lib` を 1 回以上実行する。
- cargo はプロジェクトルートから `--manifest-path src-tauri/Cargo.toml` と `CARGO_TARGET_DIR=src-tauri/target` を明示して実行する（`.claude/rules/build-location.md`）。

## Cross-task Design Decisions

なし。単一タスクのため、stale fixture 構築戦略の設計判断は `tasks/task0001.md` の Design 節に置く。

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| fixture 構築の変更が stale の観測的定義（ディスク上の socket 型ファイル + listener 不在）を変えてしまい、FR2 の検証意図が弱まる | 低 | 中 | task0001 の AC で「socket 型ファイルがディスク上に残ることのアサート維持」「live のみ返るアサーションの維持」を明示する |
| テスト側の安定化のみでは不十分と判明する | 低 | 中 | その場合に限り本番 chooser の一時的 stale 表示の是正をスコープに戻す（REQUIREMENTS 10.2）。本計画では対象外 |
| ストレス反復が偶然レース窓を踏まず、検証が偽陰性になる | 低 | 中 | fixture の決定性を OS 意味論（listen 状態に入らないソケットは接続を常に拒否する）で裏づけ、ストレス反復は確認手段と位置づける（task0001 Design 参照） |

## Open Questions

なし。
