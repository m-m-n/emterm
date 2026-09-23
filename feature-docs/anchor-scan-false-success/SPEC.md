# Feature: anchor-scan-false-success

## 概要

テスト専用モジュール `anchor_scan`（`src-tauri/src/window_host/tests.rs`）の
`locate_call` が、末尾のリテラル引数、文字リテラル・生文字列、同名の入れ子メソッドに
出会ったときに、誤った範囲を `Ok` で返す経路を塞ぐ。各経路では、対象を正しく処理するか、
明示的に `Err` を返す。

## 目的

- テスト専用モジュール `anchor_scan`（`src-tauri/src/window_host/tests.rs`）が、
  `locate_call` の不変条件「誤った範囲を成功として返さない」を満たすようにする。
  対象を正しく処理するか、明示的に `Err` を返すかのどちらかにする
- 良性編集テストが守っているトークン形状ガード（benign-edit-test-token-rewrite の
  FR4c / AC-6）が、リテラル引数・文字リテラル・生文字列・同名の入れ子メソッドによって
  黙って無効になる経路をなくす

## 受け入れ基準

- [ ] **AC-1**（FR1, FR5）: `fn probe_fn(...) { probe_call(a, b, c, d, 0); }` を
  expected_arg_count 4 で渡すと `Err` が返る。このテストは変更前の実装では失敗する
- [ ] **AC-2**（FR1, FR5）: 4 引数で末尾が素の文字列リテラルの呼び出し
  （例: `probe_call(a, b, c, "s")`）を expected_arg_count 3 で渡すと `Err` が返る。
  このテストは変更前の実装では失敗する
- [ ] **AC-3**（FR2, FR4, FR5）: `probe_call(a, b, c, ')')` を expected_arg_count 4 で
  渡すと、`Err` が返るか、引数テキストがちょうど `a, b, c, ')'` になる `Ok` が返る。
  途中で切れた範囲の `Ok` はテスト失敗になる。このテストは変更前の実装では失敗する
- [ ] **AC-4**（FR2, FR4, FR5）: 引数に中身に `"` を含む生文字列
  （例: `r#"contains a " quote"#`）を含む呼び出しを渡すと、`Err` が返るか、
  引数テキストが真の引数リスト全体と完全に一致する `Ok` が返る
- [ ] **AC-5**（FR2, FR5）: ライフタイム注釈を含むソース
  （例: `fn probe_fn<'a>(a: &'a i32, b: i32, c: i32, d: i32) { probe_call(a, b, c, d); }`）を
  expected_arg_count 4 で渡すと、引数テキストが `a, b, c, d` の `Ok` が返る
- [ ] **AC-6**（FR3, FR4, FR5）: `impl` ブロック内に同名メソッド `fn probe_fn` と
  その中の 4 引数 `probe_call(...)` 呼び出しがあり、その後ろに深さ 0 の `fn probe_fn` と、
  別の引数テキストを持つ 4 引数 `probe_call(...)` 呼び出しがあるソースを渡すと、
  `Err` が返るか、深さ 0 側の呼び出しを指す `Ok` が返る。`impl` 側の呼び出しを指す
  `Ok` はテスト失敗になる。このテストは変更前の実装では失敗する
- [ ] **AC-7**（FR4, NFR1, NFR2）: 既存の良性編集テスト 4 本
  （`button_path_rewrapped_argument_list_edit_is_accepted`、
  `button_path_call_with_inserted_comment_edit_is_accepted`、
  `button_path_trailing_comma_edit_is_accepted`、
  `button_path_independent_statement_reorder_edit_is_accepted`）と、
  既存の anchor 負テスト 3 本（`anchor_locate_call_wrong_argument_count_returns_err`、
  `anchor_locate_call_callee_not_shaped_as_a_call_returns_err`、
  `anchor_locate_call_fn_name_separated_by_a_comment_returns_err`）、
  `anchor_locate_call_unbalanced_paren_inside_a_comment_still_returns_the_correct_range` が
  緑のまま。実ソースの `run_button_decision` 内の `apply_outcome_with_held` 呼び出しは
  引き続き 4 引数で位置特定できる
- [ ] **AC-8**（NFR1, NFR2, NFR3）:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が
  全件成功する。差分は `src-tauri/src/window_host/tests.rs` に限られ、`call_site_scan`
  モジュールと `Cargo.toml` は変わらない

## 技術要件

### 機能要件

- **FR1: 末尾リテラル引数を引数カウントに含める** —
  `anchor_scan::locate_call` の引数カウントで、数値リテラルとダブルクォート文字列リテラル
  （エスケープ入りを含む）を 1 個の引数として数える。末尾の位置でも数える。
  数えられない場合は `locate_call` が明示的に `Err` を返す。リテラル引数を数え落とした
  カウントで `Ok` を返してはならない
- **FR2: 文字リテラルと生文字列を不透明スパンとして扱う** —
  `anchor_scan::scan` は文字リテラル（`')'` や `'\''` などのエスケープ形、`b'x'` を含む）と
  生文字列（`r` / `br` / `cr` 接頭辞、`#` の個数は任意）を不透明スパンとして扱う。
  中身の文字を構造上の区切り（括弧・カンマ）や文字列の区切りとして解釈しない。
  扱えない形に出会った場合は `locate_call` が明示的に `Err` を返す。
  ライフタイム・ラベル（`'a`、`'static`、`'outer:`）は文字リテラルとして巻き込まず、
  他の点で正しく位置特定できるソースを `Err` にしない。生識別子 `r#ident` は
  生文字列と誤認しない
- **FR3: 関数探索に波括弧深さのゲートを入れる** —
  `locate_call` の step 1 は、`call_site_scan::extract_function` と同じく波括弧深さ 0 に
  現れる `fn <fn_name>` だけを照合対象にする。`impl` や入れ子ブロックの中にある
  同名定義は選ばない。これができない場合は、同名定義が深さ 0 以外に現れた時点で
  明示的に `Err` を返す
- **FR4: 誤った成功を返さない不変条件** —
  `locate_call` が `Ok` を返すのは、返す `AnchoredCall` の範囲が、深さ 0 の
  `fn <fn_name>` の本体にある唯一の `<callee>(...)` 呼び出しの真の範囲（callee 名の終端、
  真の開き括弧・閉じ括弧、その間の引数テキスト）と一致し、かつ真の引数個数が
  `expected_arg_count` と一致する場合に限る。それ以外はすべて `Err`。
  `locate_call` のシグネチャと `AnchoredCall` のフィールドは変えない
- **FR5: 負のテストを追加する** —
  3 つの経路それぞれについて、手組みの入力（`probe_fn` / `probe_call`）で
  `anchor_scan::locate_call` を呼ぶ単体テストを `src-tauri/src/window_host/tests.rs` に
  追加する。数値の末尾引数、文字列の末尾引数、`')'` の末尾引数、生文字列の引数、
  対象より前に現れる同名の入れ子メソッド、ライフタイム注釈を含むソースを対象にする
  （テストシナリオ TS-1〜TS-6）

### 非機能要件

- **NFR1 - 変更範囲:** 変更は `src-tauri/src/window_host/tests.rs` の `#[cfg(test)]`
  テストコード内に限る。本番コード（`src-tauri/src/window_host/pointer_routing.rs` を含む）は
  変えない
- **NFR2 - call_site_scan を凍結したまま保つ:** `call_site_scan` モジュール
  （そのトークナイザー、private のヘルパー、可視性を含む）は変えない。`anchor_scan` は
  自前の走査ロジックを使い続ける
- **NFR3 - 依存を追加しない:** 新しい crate 依存を追加しない
- **NFR4 - 汎用パーサーは作らない:** あらゆる Rust 構文を網羅的にサポートすることは
  目的にしない。構文の形ごとに、正しく処理する方式と明示的に `Err` を返す方式の
  どちらを採っても、FR4 を満たせばよい

## 実装方針

### 対象

- 変更対象は `src-tauri/src/window_host/tests.rs` 内のテスト専用モジュール `anchor_scan`
  （`scan`、引数カウント、`locate_call`）と、同ファイルへの負のテスト追加に限る（NFR1）
- `call_site_scan` は参照実装として読むだけで、変えない（NFR2）

### 前提（Assumptions）

- **A1:** `call_site_scan` は凍結したまま保つ（tests.rs:6829-6831 のコメントにある
  前 feature の D3 / NFR1）。`anchor_scan` は `call_site_scan` の private ヘルパー
  （`try_scan_string_like` / `try_scan_char_literal`）の可視性を変えて呼ぶことはせず、
  自前でロジックを持つ
- **A2:** 経路ごとに正しく処理するか明示的に `Err` にするかの選択は、実装計画に委ねる。
  タスクの完了定義がどちらも認めているため
- **A3:** 「深さ 0」の定義は `call_site_scan::extract_function` と同じく、
  波括弧 `{` / `}` だけを数えた深さとする
- **A4:** `src-tauri/src/window_host/pointer_routing.rs` は変えない。コード行に
  文字リテラル・ライフタイム・生文字列は無く、`run_button_decision` は深さ 0 に 1 回だけ
  定義されている（793 行目、呼び出しは 850 行目）ので、既存の実ソースでの位置特定は
  変更後も `Ok` のまま

### 依存関係

**内部依存:**
- `call_site_scan::extract_function`: 深さ 0 の定義（A3）と深さゲートの参照実装（FR3）
- `pointer_routing.rs` の `run_button_decision` / `apply_outcome_with_held` 呼び出し:
  既存の実ソースでの位置特定対象（AC-7、A4）

**外部依存:**
- なし（NFR3）

### デザイン

デザインステップはスキップした。テスト専用（`#[cfg(test)]`）の走査ロジックの修正で、
UI・画面・ビジュアルの変更が無いため。

## 宣言する変更範囲（Declared Change Set）

この節は手書きの一覧ではなく、create-plan での導出方法を定める。機能固有のパスは、
create-plan の時点で `workflow.yaml` の各タスクの `files` エントリから導出する
（`references/phases/create-plan-phase.md`）。

各 SPEC は、機能固有のパスに加えて、ワークフローが生成する次の 2 エントリを既定で宣言する。

- `feature-docs/anchor-scan-false-success/**`
- `test-docs/anchor-scan-false-success/**`

`feature-docs/anchor-scan-false-success/**` は `REQUIREMENTS.md`、`SPEC.md`、
`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、
`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成する成果物を含む。
これらはフェーズ文書と `references/phase-state.md` が生成・所有する。この節はそれらを
引用するだけで、規則は再掲しない。

`test-docs/anchor-scan-false-success/**` はタスクごとのテスト記録
`test-docs/anchor-scan-false-success/{T}.tests.yaml` を含む。`implement-phase.md` が
生成・所有する。この節はそれを引用するだけで、規則は再掲しない。

この 2 つの既定エントリは、SPEC 作成者が明示的に外さない限り宣言に含まれる。
記載が無いことを外したとはみなさない。外すことは意図的で明示的な縮小とする。

この宣言は上位集合の表明である。検証時に観測される実際の変更集合は、宣言した集合に
含まれていればよく、一致する必要はない。implement タスクを生まない機能は
`test-docs/anchor-scan-false-success/` を生成しないが、その場合も宣言した
`test-docs/anchor-scan-false-success/**` エントリは正しい。宣言したパスが実体化しない
ことは違反ではない。

## テストシナリオ

### 単体テスト

- [ ] **TS-1**（AC-1）: 入力
  `fn probe_fn(a: i32, b: i32, c: i32, d: i32) { probe_call(a, b, c, d, 0); }`、
  expected_arg_count 4 — `Err`
- [ ] **TS-2**（AC-2）: 入力
  `fn probe_fn(a: i32, b: i32, c: i32) { probe_call(a, b, c, "s"); }`、
  expected_arg_count 3 — `Err`
- [ ] **TS-3**（AC-3）: 入力
  `fn probe_fn(a: i32, b: i32, c: i32) { probe_call(a, b, c, ')'); }`、
  expected_arg_count 4 — `Err`、または args テキストが `a, b, c, ')'` の `Ok`
- [ ] **TS-4**（AC-4）: 入力 生文字列 `r#"contains a " quote"#` を引数に含む
  `probe_call` 呼び出し — `Err`、または args テキストが真の引数リスト全体と一致する `Ok`
- [ ] **TS-5**（AC-5）: 入力
  `fn probe_fn<'a>(a: &'a i32, b: i32, c: i32, d: i32) { probe_call(a, b, c, d); }`、
  expected_arg_count 4 — args テキストが `a, b, c, d` の `Ok`
- [ ] **TS-6**（AC-6）: 入力
  `impl Probe { fn probe_fn(&self, a: i32, b: i32, c: i32, d: i32) { probe_call(a, b, c, d); } } fn probe_fn(w: i32, x: i32, y: i32, z: i32) { probe_call(w, x, y, z); }`、
  expected_arg_count 4 — `Err`、または args テキストが `w, x, y, z` の `Ok`

### 回帰テスト

- [ ] **TS-7**（AC-7, AC-8）: 既存テストと `--lib` スイート全体 — 全件成功

### E2E テスト

**既存の E2E テスト**: なし
**実行コマンド**: 検出なし

### エッジケース

- [ ] ライフタイム・ラベル（`'a`、`'static`、`'outer:`）を文字リテラルとして巻き込まない（FR2）
- [ ] 生識別子 `r#ident` を生文字列と誤認しない（FR2）
- [ ] エスケープ入りの文字列リテラル・文字リテラル（`'\''`）、`b'x'`、`br` / `cr` 接頭辞の
  生文字列を不透明スパンとして扱うか、明示的に `Err` を返す（FR1, FR2）

## 成功基準

- [ ] FR1〜FR5 が実装され、テストされている
- [ ] TS-1〜TS-7 がすべて成功する
- [ ] 差分が `src-tauri/src/window_host/tests.rs` に限られ、`call_site_scan` モジュールと
  `Cargo.toml` が変わらない

## 未解決事項

なし

## 参考

- 前 feature: `benign-edit-test-token-rewrite`（FR4c / AC-6、`call_site_scan` 凍結の D3 / NFR1）
- 対象ファイル: `src-tauri/src/window_host/tests.rs`
- 実呼び出し側: `src-tauri/src/window_host/pointer_routing.rs`
