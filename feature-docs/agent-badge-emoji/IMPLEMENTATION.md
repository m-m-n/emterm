# Implementation Plan: agent-badge-emoji

## Overview

エージェント状態バッジの表現を 4 状態すべて絵文字に統一する（blocked=❓/❔、done=✅/💤）。変更は `ui::tab_bar` の判定関数・定数・フォールバック解決に閉じるため、単一タスク（task0001）で実装する。

## Technology Stack

- **Rust / egui**: 既存スタックのみ。絵文字描画は既存のバンドル Noto Color Emoji と既存のラスタライズ・キャッシュ経路をそのまま使う
- **新規依存**: なし（`project.license: MIT` に対するライセンス確認対象の新規依存は 0 件）

## Layer Structure

既存構造を維持する。バッジの表示判定（`badge_presentation()` / `resolve_badge_render_mode()`）は `ui::tab_bar` に置き、`ui::mux_sidebar` は共有 painter（`paint_agent_badge`）経由で消費する（FR5）。依存方向は既存どおり mux_sidebar → tab_bar の一方向。

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| （なし） | 単一タスク構成のためタスク間で共有する新規契約はない | — | — |

既存の共有点（`badge_presentation()` を tab_bar / mux_sidebar が共有）は task0001 内で完結して維持される。

## Conventions

- 絵文字クラスタ定数は既存の `WORKING_BADGE_EMOJI` / `IDLE_BADGE_EMOJI` と同形式（`tab_bar.rs` 内の公開文字列定数・単一コードポイント・VS-16 なし）とする（FR6）
- ユニットテストは対象モジュール内のインラインテストモジュールに置き、`--lib` で実行される形にする（NFR3、`test/README.md` の規約）

## Cross-task Design Decisions

### D1: 単一タスク構成

変更が `tab_bar.rs` の 1 つの判定関数の分岐・定数・インラインテストに集中しており、分割すると同一ファイルへの並行編集で必然的なマージ競合を生む。task0001 のみとする。

### D2: 新規依存を追加しない

新規 3 コードポイント（U+2753 / U+2754 / U+2705）は既存定数（U+26A1 / U+1F4A4)と同じくバンドルの Noto Color Emoji で描画する（SPEC A3）。ライブラリ追加・フォント追加は行わない。

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| 新規 3 コードポイントがバンドル絵文字フォントのビットマップに無く描画できない | 低 | 中 | FR4 の円フォールバックにより空白にはならない。VERIFICATION.md の手動確認（MT-1）で実表示を照合する |
| フォールバック形の伝搬漏れ（blocked/done の seen がリングにならず塗りになる） | 中 | 低 | task0001 の AC-4 とユニットテスト（TS-2）で固定する |

## Open Questions

なし
