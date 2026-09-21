# Task Document: tests-ac-reference-alignment — tests.rs の AC 参照の出典を明示する

## Change

`src-tauri/src/window_host/tests.rs` の mouse-report-held-callsite-test 由来の
テスト群（およそ 5207〜6871 行）にある `AC-N` ラベルは、
`feature-docs/mouse-report-held-callsite-test/tasks/task0001.md` のタスクローカル
番号であり、`feature-docs/mouse-report-held-callsite-test/SPEC.md` の AC 番号では
ない。この出典をコード側で明示する。番号そのものは付け替えない。

適用規則（全参照箇所に機械的に適用する）:

1. グループ冒頭 — 現在 `AC-1..AC-7` の範囲コメントがある行（`tests.rs:5212`）の
   直後、続く区切り線の前に、既存と同じ `//` 形式で次の宣言を置く。

   ```rust
   // Every AC-N label in this group, including comments and assertion
   // messages, uses the task-local numbering of
   // mouse-report-held-callsite-test/task0001, not SPEC.md's AC numbering.
   ```

2. 同グループ内の通常コメント・doc コメントにある AC 参照（およそ 18 行）には、
   それぞれ出典 `mouse-report-held-callsite-test/task0001` を明記する。番号・
   `(a)(b)(c)` の小項目・TS 番号・範囲表記はいずれも変更しない。範囲表記も
   タスクローカル番号の範囲である旨がわかる形にする。

3. アサーションの診断文字列（`AC-N` ラベルを含むおよそ 31 箇所）と、その直前の
   行には手を加えない。

対象ファイルは `src-tauri/src/window_host/tests.rs` の 1 ファイルのみ。
`feature-docs/mouse-report-held-callsite-test/SPEC.md` と
`feature-docs/mouse-report-held-callsite-test/tasks/task0001.md` は変更しない。

## Expected Result

- `tests.rs` の任意の `AC-N` ラベルについて、それが
  `mouse-report-held-callsite-test/task0001` のタスクローカル番号であり
  SPEC.md の AC 番号ではないことが、コードを読んだだけで判別できる。
- `git diff` の変更行がコメント行のみである。テスト関数名・`#[test]` 属性・
  入力データ・診断文字列・アサーション式・走査処理はいずれも変わらない。
- `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  が、変更前と同じテスト件数・同じ結果（全緑）で完了する。
- `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
  および同 `--no-default-features` が通る。
- 変更ファイルは `src-tauri/src/window_host/tests.rs` のみで、
  `feature-docs/mouse-report-held-callsite-test/` 配下に変更が無い。
