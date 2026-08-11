# Feature: wide-pair-overwrite-cleanup

> 要件の一次情報は [REQUIREMENTS.md](./REQUIREMENTS.md) を参照。本書は実装観点での仕様。

## Overview

term_core のセル書き込みで、wide ペア（幅2 base + spacer）の半分だけを上書きしたときに相方セルが掃除されず、グリッド不変条件が壊れる。本機能はその相方セルを空白（幅1 blank）にする掃除を、幅1 上書き・spacer 上書き・幅2 書き込みの placeholder 作成・遡及 widen の各経路に入れる。これにより ⏭️（U+23ED + VS16）を含む行のストリーム描画乱れ（罫線ズレ・文字重なり・Ctrl+L まで残留）を解消する。

## Objectives

- wide ペアの半分だけを上書きしたとき、相方セルを空白化してグリッド不変条件を守る
- ⏭️（U+23ED + VS16）を含む行のストリーム描画乱れ（罫線ズレ・文字重なり・Ctrl+L まで残留）を解消する

## User Stories

本機能は term_core のグリッド書き込みロジックのバグ修正であり、確定要件にユーザーストーリーは含まれない。受け入れ観点は「Success Criteria」を参照。

## Technical Requirements

### Functional Requirements

- **FR1 — 幅2 base への幅1 上書きで旧 spacer を空白化:** 幅2 base セルの上に幅1 文字を書いたとき、col+1 の旧 spacer を空白（幅1 blank）にする（レポート P3: 孤児 spacer 残留の解消）。
- **FR2 — spacer への上書きで base を空白化:** spacer（幅0）セルの上に文字を書いたとき、col-1 の幅2 base を空白（幅1 blank）にする（レポート P4: base 残留によるグリフ重なりの解消）。
- **FR3 — 幅2 書き込みの placeholder 作成時の連鎖掃除:** 幅2 書き込みで col+1 に placeholder を作るとき、col+1 が別ペアの base だった場合はその spacer（col+2）も空白化する。
- **FR4 — widen_after_merge の spacer 作成箇所への適用:** 遡及 widen（`widen_after_merge`）が col+1 に spacer を作る箇所にも FR3 と同じ相方掃除規則を適用する。
- **FR5 — csi_edit 系（ECH/DCH/ICH）への同種掃除の適用 [status: tbd]:** `csi_edit` の ECH/DCH/ICH 系にも wide ペア相方掃除が無い。これをスコープに含めるかどうか。
  - **tbd_reason:** task_description の制約・前提でユーザーが「スコープに含めるかは実装時に判断する」と明示的に保留している。未解決の質問ではなくユーザー決定済みの実装時判断事項。

### Non-Functional Requirements

- **NFR1 — 回帰安全:** wide ペアに関与しない通常の書き込み経路の挙動を変えない。
- **NFR2 — 他ターミナル実装との整合:** 相方セル空白化の挙動は xterm / Alacritty / WezTerm の実装慣行（上書き時に相方セルを空白化）と整合させる。
- **NFR3 — 修正の局所性:** 修正は term_core 内の共通コード（`write_grapheme_to_grid` / `handle_print_ascii` / `widen_after_merge`）に閉じ、mux 経路（daemon parse → GUI parse）にも同一コードで効く。
- **NFR4 — ASCII 高速パスの性能維持:** `handle_print_ascii` は ASCII 高速パスであるため、旧セル状態チェックの追加で通常経路（wide ペア非関与時）の性能特性を損なわない。

## Implementation Approach

### Architecture

修正は `crates/term_core` 内のセル書き込み共通コードに閉じる（NFR3）。同一コードが mux 経路（daemon parse → GUI parse）の両側で動くため、mux 側に個別対応は不要。

| 対象関数 | 掃除を入れる箇所 | 対応要件 |
|----------|------------------|----------|
| `write_grapheme_to_grid` | 旧セルが幅2 base だった場合の col+1 空白化 / 旧セルが spacer だった場合の col-1 空白化 / 幅2 書き込みで col+1 placeholder を作る際の col+2 連鎖掃除 | FR1, FR2, FR3 |
| `handle_print_ascii` | ASCII 高速パスでの同等の相方掃除（NFR4 に留意） | FR1, FR2, NFR4 |
| `widen_after_merge` | 遡及 widen が col+1 に spacer を作る箇所での相方掃除 | FR4 |
| `csi_edit`（ECH/DCH/ICH） | スコープに含めるかは実装時判断 | FR5 (tbd) |

行番号アンカーについて: task_description の行番号（print_handler.rs :68-146 / :149-183 / :280-333）は main df054f53 時点であり、integration base 00c06f35 とはズレうる。実装は関数名（`write_grapheme_to_grid` / `handle_print_ascii` / `widen_after_merge`）をアンカーとする。

### Grid Invariant

wide ペアは「幅2 base（col）+ spacer（col+1）」で 1 組。片方だけが上書きされた時点で組は壊れるため、残った側を幅1 の空白セルに置き換えて不変条件を回復する（NFR2 の慣行と同じ）。

### Dependencies

- 内部: `crates/term_core`（ANSI パーサー + グリッド）
- 外部: なし（確定要件に記載なし）

### File Structure

```
crates/term_core/
└── src/
    └── ...print_handler...   # write_grapheme_to_grid / handle_print_ascii / widen_after_merge
```

## Test Scenarios

### Unit Tests

- [ ] **TS1 (FR1) — P3 再現:** 幅2 ペア（⏭️ = U+23ED + U+FE0F）を書いた後、base 位置に幅1 文字を上書きし、`get_cell_char` / `get_cell_width` で col+1 が空白（幅1）であることを検証する。
- [ ] **TS2 (FR2) — P4 再現:** spacer 位置に幅1 文字を上書きし、col-1 の base が空白（幅1）で幅2 グリフが残っていないことを検証する。
- [ ] **TS3 (FR1, FR2, FR3, FR4) — P5 再現:** 行を 1 桁ズラして書き直し（フレーム間の列幅変化を模擬）、旧フレームの残骸が残らないことを検証する。
- [ ] **TS4 (FR4, NFR1) — チャンク分割耐性の既存挙動維持:** U+23ED と VS16 を別チャンクで流しても遡及 widen が正常（レポート P1/P2 の正常系）。

### Test Implementation Conventions

- テストは `crates/term_core` の inline `#[cfg(test)] mod tests` に、命名規約 `<subject>_<scenario>_<expected>` で追加する。
- 駆動は `process_pty_data`、assert は観測可能なグリッド契約（`get_cell_char` / `get_cell_width`）に対して行う。
- 実行コマンド: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`

### Integration Tests

該当なし（確定要件に記載なし）。

### E2E Tests

**Existing E2E tests**: None（E2E 基盤が存在しない）
**Run command**: Not detected

- [ ] **TS5 (FR1, FR2, FR3, FR4) — 実機確認:** ⏭️ を含むテーブルの Claude Code ストリーム描画をユーザーが手動確認し、乱れが再現しないことを確認する。

### Edge Cases

- [ ] 幅2 書き込みの col+1 が別ペアの base だった場合、その spacer（col+2）まで連鎖して空白化される（FR3）
- [ ] U+23ED と VS16 がチャンク境界で分割されても遡及 widen が正常に動く（TS4）

### Performance Tests

- [ ] `handle_print_ascii` の通常経路（wide ペア非関与時）の性能特性が損なわれない（NFR4）

## Security Considerations

該当なし（確定要件に記載なし）。

## Error Handling

該当なし（確定要件に記載なし）。

## Performance Optimization

- `handle_print_ascii` は ASCII 高速パス。相方掃除は旧セル状態チェックで wide ペア関与時のみ発動させ、通常経路の性能特性を維持する（NFR4）。

## Success Criteria

- [ ] 幅2 base の上に幅1 文字を書いたとき、col+1 の旧 spacer が空白化される（レポート P3 / FR1）
- [ ] spacer の上に文字を書いたとき、col-1 の base が空白化される（レポート P4 / FR2）
- [ ] 幅2 書き込みの placeholder 作成時（col+1）、そこが別ペアの base だった場合はその spacer（col+2）も空白化される（FR3）
- [ ] widen_after_merge の spacer 作成箇所（col+1 上書き）にも同じ規則が適用される（FR4）
- [ ] レポートの P3 / P4 / P5 の再現手順がユニットテストとして追加され、回帰ガードになる（TS1 / TS2 / TS3）
- [ ] ⏭️ を含むテーブルのストリーム描画で乱れが再現しないこと（実機確認 / TS5）
- [ ] wide ペアに関与しない通常の書き込み経路の挙動が変わっていない（NFR1）

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- [ ] FR5: csi_edit 系（ECH/DCH/ICH）への同種掃除の適用 - task_description の制約・前提でユーザーが「スコープに含めるかは実装時に判断する」と明示的に保留している。未解決の質問ではなくユーザー決定済みの実装時判断事項

## Out of Scope

- Claude Code（アプリ側）の幅モデル特定・修正
- mux 経路固有の追加要因調査

## References

- 要件定義書: `feature-docs/wide-pair-overwrite-cleanup/REQUIREMENTS.md`
- 調査レポート: `tmp/vs16-wide-pair-overwrite-2026-08-11.md`（gitignored でワークツリーから読めないが、全文が task_description に埋め込まれており一次入力とする）
- 他ターミナル実装の慣行: xterm / Alacritty / WezTerm（上書き時に相方セルを空白化）
