# Implementation Plan: wide-pair-overwrite-cleanup

## Overview

term_core のセル書き込み・編集経路で wide ペア（幅2 base + 幅0 spacer）の半分だけが上書き・分断されたとき、残存する相方セルを幅1 の空白に置換してグリッド不変条件を回復する。print 経路（task0001）と CSI 編集・消去経路（task0002）の 2 タスク構成。

## Technology Stack

- **Rust / crates/term_core**: 修正はこのクレート内に閉じる（NFR3）。mux 経路（daemon parse → GUI parse）は同一コードが両側で動くため個別対応不要。
- **新規依存**: なし。ライセンス影響なし（project.license: MIT のまま。新規依存のライセンス記録は「なし」）。

## Layer Structure

- 変更対象は term_core のセル書き込み層（print_handler）と CSI 編集・消去層（csi_edit / csi_screen）のみ。新しいレイヤー・コンポーネントは作らない。依存方向の変更なし。

## Grid Invariant（両タスク共通のゴール）

両タスクは操作後の行に対して次の不変条件を回復する:

1. 幅0 セル（spacer）は、直前セルが幅2（base）のときに限り存在する。
2. 幅2 セル（base）の直後セルは幅0（spacer）である。
   - **既存の例外（変更しない）**: auto-wrap off で最終列に置かれた幅2 base は spacer を持たない（print 経路の遡及 widen が意図的に作る既存 quirk）。

## Shared Components

共有されるのは**挙動契約**であり、共有コード（共通ヘルパー関数）ではない（設計判断 D3 参照）。

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| 相方空白化規則（partner-blanking rule） | wide ペアの片割れが単独で残る状況で、その残存セルを幅1 空白に置換し不変条件を回復する | **事前条件**: 空白化対象セルが現に幅0（spacer）または幅2（base）であることを確認してから行う（幅1 の通常セルを誤って壊さない）。列境界外は対象にしない。**事後条件**: (1) 対象セルの文字は空白 1 文字・幅 1、(2) fg / bg / flags / hyperlink は対象セルが元々持っていた値を保持、(3) 対象セルの内容が overflow テーブルに退避されている場合はエントリと逆引き索引の両方を除去、(4) 該当行に dirty がマークされる | task0001, task0002 |

## Conventions

- テストは crates/term_core の inline `#[cfg(test)] mod tests` に、命名規約 `<subject>_<scenario>_<expected>` で追加する。TerminalCore を明示構築し `process_pty_data` で駆動、`get_cell_char` / `get_cell_width` で assert する。
- テスト実行: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- ビルド確認: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- crate 全体の fmt は走らせない（プロジェクト方針）。変更ファイルのみ既存スタイルに合わせる。

## Cross-task Design Decisions

### D1: FR5（ECH/DCH/ICH への同種掃除）をスコープに含める

- **判断**: 含める。workflow.yaml の FR5 は `status: assumed` とする（ユーザーが「実装時に判断」と保留していた事項を、計画時に planner が判断した仮定）。
- **根拠**: (1) ICH/DCH はシェルの行編集（コマンドライン中の絵文字・CJK の挿入削除）で日常的に wide ペアをまたぐ、(2) 該当ハンドラ（handle_insert_characters / handle_delete_characters / handle_erase_characters）はセル幅を考慮せずシフト・消去しており、print 経路と同一クラスの不変条件破壊が現に存在する、(3) NFR2 が参照する xterm / Alacritty / WezTerm はいずれもこれらの操作で相方セルを空白化する。
- **影響タスク**: task0002 の存在そのもの。task0001 と完全にファイル分離しているため、問題が出た場合は task0002 単独で切り戻せる。

### D2: 空白化セルの定義

- 空白化 = 文字を空白 1 文字・幅 1 に置換。fg / bg / flags / hyperlink は**対象セル自身が元々持っていた値を保持**する。
- BCE（カーソル背景での消去）とは別物として扱う: BCE は「操作が消去対象と定義する範囲」への既存挙動であり、相方空白化は「操作対象外だが不変条件回復のために合成する空白」。相方にカーソル背景を適用すると行内に無関係な背景が混入するため、保持とする。既存の BCE テストの挙動は変えない。
- 対象セルの内容が overflow テーブルに退避されている場合、エントリと逆引き索引を除去する（怠ると空白化後も旧内容が読める）。
- 影響タスク: task0001, task0002。

### D3: 規則は各タスクが自ファイル内に局所実装する（共有ヘルパーを作らない）

- 全タスクは並列 worktree で実装されるため、一方のタスクが作る共通関数を他方から参照できない。相方空白化規則は本書の挙動契約として固定し、各タスクは自分の files に列挙されたファイル内に局所実装する。
- 小規模規則の重複実装は、並列実装を成立させるための意図的なトレードオフとして許容する。統合後の共通化はレビューが提案してよいが、本 feature の必須要件ではない。
- TerminalCore の固有メソッド名は crate 全域で衝突する（統合時にコンパイルエラーになる）ため、補助関数名を予約する:
  - **task0001 予約名**: `blank_wide_pair_partner`（print_handler.rs 内に定義。task0002 はこの名前を定義しない）
  - **task0002 予約名**: `blank_wide_pair_split`（csi_edit.rs 内に定義。task0001 はこの名前を定義しない）
  - 予約名以外の補助名を追加する場合も、相手タスクの files に列挙されたファイルには定義しない。
- 影響タスク: task0001, task0002。

### D4: ASCII 高速パスの性能ガード（NFR4）

- handle_print_ascii では、書き込み先の旧セル幅が 1 のとき（wide ペア非関与の通常経路）は追加処理を一切行わない。相方掃除の要否判定は「旧セル幅が 1 でないか」という分岐 1 回でゲートし、成立時のみ掃除処理へ進む。旧セルは書き込みのために既に参照しているため、追加のメモリアクセスは発生させない。
- 影響タスク: task0001（規則の適用姿勢として task0002 も同様に、通常セルのみの操作では追加コストを判定分岐に留める）。

### D5: 行番号ではなく関数名をアンカーとする

- 調査レポートの行番号は main df054f53 時点のもの。実装は関数名（write_grapheme_to_grid / handle_print_ascii / widen_after_merge / handle_insert_characters / handle_delete_characters / handle_erase_characters）を基準に該当箇所を特定する。
- 影響タスク: task0001, task0002。

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| 相方判定の誤りで正当な隣接セルを空白化し回帰 | 低 | 高 | 空白化前に相方セルの現在幅を必ず確認（幅0 / 幅2 のときのみ実施）。既存スイート全緑を両タスクの AC に含める |
| ASCII 高速パスへの分岐追加による性能劣化 | 低 | 中 | D4 の 1 分岐ゲート。VERIFICATION.md TS9 でレビュー確認 |
| 統合時の固有メソッド名衝突 | 低 | 中 | D3 の名前予約 |
| CSI 系掃除が BCE 挙動を変えて既存 TUI 表示に回帰 | 低 | 中 | D2 で BCE と相方空白化を区別。既存 BCE テスト全緑を維持 |

## Open Questions

- [ ] EL / ED（行・画面単位の消去）にも理論上は同種のペア分断がありうるが、FR5 の対象は ECH/DCH/ICH のみ（要件どおり）。必要になれば別 feature として起票する。
