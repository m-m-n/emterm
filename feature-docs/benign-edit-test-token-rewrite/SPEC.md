# Feature: benign-edit-test-token-rewrite

要件定義書: `feature-docs/benign-edit-test-token-rewrite/REQUIREMENTS.md`

## Overview

良性編集の受け入れテスト `button_path_benign_edits_to_the_real_source_are_still_accepted`
（`src-tauri/src/window_host/tests.rs:6825-6888`）から、閉じた集合の外側の名前への結合を
取り除く。現在このテストはプロダクションのソーステキストをそのまま書き写したリテラルを
3 箇所に持ち、その識別子が前 feature の NFR8 の閉じた集合の外側にあるため、無関係な
リファクタでテストが赤になる。本 feature は良性編集のカバレッジを保ったまま構築方式を
書き換え、あわせて vacuity と mislocation という 2 つのサイレント失敗モードに対する
ガードを追加する。変更は `src-tauri/src/window_host/tests.rs` に閉じ、プロダクション挙動を
一切変更しない。

## Objectives

- 良性編集の受け入れテスト（`tests.rs:6825-6888`）から、閉じた集合の外側の名前への結合を
  取り除く。現在ハードコードされている 3 つのソースリテラルの識別子（`outcome`、`records`、
  `dest`、`hovered_link`、`host.hover.link_cells`、`middle_click_paste_enabled`、
  `app.settings.middle_click_paste`）は、前 feature の NFR8 が宣言した閉じた集合
  （`tests.rs:5992-5999`）の外側にあり、`run_button_decision` への無関係なリファクタが、
  このテストが守っている held の受け渡しとは無関係な理由でテストを赤にしている。
- 良性編集のカバレッジ自体は保つ。4 種の良性編集（実引数リストの再折り返し、コメント挿入、
  末尾カンマ、独立した 2 文の並べ替え）は、手書きの代用物ではなく実際に埋め込んだ
  プロダクション本体に対して引き続き行使する。
- 2 つのサイレント失敗モード — 編集が何も変えない（vacuity）、編集が意図した呼び出し箇所以外に
  当たる（mislocation）— に対してテストを強化する。

## User Stories

### US1: 無関係なリファクタで良性編集テストが赤にならない

eMterm の開発者として、`run_button_decision` の内部識別子をリネームしたり無関係な文を
作り替えたりしたときに、良性編集テストが赤にならないでほしい。そうすれば、そのテストが
実際に守っている held の受け渡しの破壊だけがシグナルになる。

**Acceptance Criteria:**
- [ ] AC-2: `src-tauri/src/window_host/tests.rs` に `pointer_routing.rs` のプロダクション
      ソーステキストを再現する文字列リテラルが存在しない。とくに現在の `tests.rs:6832`、
      `:6859`、`:6860` の 3 リテラルが消えており、置き換えのリテラルも FR1 の閉じた集合の
      外側の識別子を名指さない。（FR1）
- [ ] AC-8: 本 feature の `git diff --name-only` が、`src-tauri/` 配下では
      `src-tauri/src/window_host/tests.rs` のみを示し、プロダクションファイルに一切
      触れていない。（NFR2）
- [ ] AC-9: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
      が引き続きコンパイルできる。（NFR4）

### US2: 良性編集のカバレッジが実本体に対して保たれる

eMterm の開発者として、4 種の良性編集が手書きの代用物ではなく実際に埋め込んだ
プロダクション本体に対して行使され、その後も判定が `Ok` のままであることを確認したい。
そうすれば書き換えによってカバレッジが失われない。

**Acceptance Criteria:**
- [ ] AC-1: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
      が、書き換え後の良性編集テストを含めてパスする。
- [ ] AC-3: 末尾カンマ編集と文の並べ替え編集が、`include_str!("pointer_routing.rs")` から
      抽出した本体に対して `find_calls` / `split_arg_spans` / `splice_tokens` で構築されており、
      スプライス後のトークン列に対する判定が `Ok` である。（FR2、FR5）
- [ ] AC-4: 再折り返し編集とコメント挿入編集が、FR4(a) の二段アンカーで箇所を特定し、
      FR4(b) に従って元のバイトをスライスして編集後ソースを構築している。編集後ソース中の
      実引数テキストは元の実引数テキストとバイト単位で同一である。（FR4）
- [ ] AC-7: 4 種すべての良性編集の後も、`judge_call` が無変更の閉じた集合定数と
      アリティ引数で `Ok` を返し続ける。（FR5）

### US3: サイレントに空虚化した編集・誤位置に当たった編集が赤になる

eMterm の開発者として、良性編集が実際には何も変えていなかった場合や、意図した呼び出し箇所
以外に当たった場合にテストが赤になってほしい。そうすればテストが空虚にパスし続ける事態を
避けられる。

**Acceptance Criteria:**
- [ ] AC-5: 各良性編集が、受け入れをアサートする前に自身の適用（編集後の成果物 != 元の成果物）を
      アサートしており、何も変えない編集はテストを赤にする。（FR3）
- [ ] AC-6: Class A のルーチンが、特定した範囲のトークン形状 — ボタン経路では呼び出し先パスに
      続くトップレベル実引数ちょうど 4 個 — をアサートしており、誤った呼び出し箇所に一致した
      アンカーは、テキストが変わりトークン列が変わらず判定が依然 `Ok` であってもテストを
      赤にする。（FR4c）

## Technical Requirements

### Functional Requirements

- **FR1 — 良性編集テストの結合を閉じた集合のみに限定する:** 書き換え後の良性編集テストは、
  既存の閉じた集合 — `run_button_decision`、`handle_mouse_wheel`、
  `mouse_report::apply_outcome_with_held`、`mouse_report::apply_wheel_report_step`、
  `mouse_report::apply_outcome`、`mouse_report_held` — の外側にあるプロダクション識別子を
  一切名指さない。この集合は `tests.rs:5992-5999` の定数群が既に宣言している。
  `tests.rs:6832`、`:6859`、`:6860` の 3 つのソースリテラルは、それらのリテラルを守るためだけに
  存在する `src.contains(...)` の sanity アサーションとともに取り除く。レシーバの識別子
  （`host`）は引き続き、抽出した関数自身の引数集合（`FunctionBody.params`）から取得し、
  ハードコードした名前からは取得しない。
- **FR2 — Class B の良性編集をトークンレベルで再現する:** 字句走査器から見える 2 つの
  良性編集 — held 対応呼び出しの実引数リストへの末尾カンマ追加と、同じ本体内の前方にある
  独立した 2 文の並べ替え — は、実際に抽出した本体からトークンレベルで生成する。生成には
  既存のヘルパー `call_site_scan::find_calls` / `split_arg_spans` / `splice_tokens`
  （`tests.rs:5676`、`:5740`、`:5978`）を用いる。並べ替えは、対象 2 文をソーステキストの照合
  ではなく構造的に選ぶ（たとえば抽出した本体内でトークン走査により見つけた、隣接する
  トップレベルの `let` 文 2 つ）。編集後のトークン列はそのまま `judge_call` に渡し、
  ソースへ戻して描画することはしない。
- **FR3 — 全良性編集に vacuity 防止ガードを置く:** すべての良性編集は、受け入れアサーションを
  実行する前に、自分の入力を実際に変更したことをアサートする。Class B ではスプライス後の
  トークン列が未編集のものと異なること、Class A では編集後のソースバイトが元のバイトと
  異なることをアサートする。編集が静かに適用されなかった場合は必ずテストが赤になり、
  空虚にパスさせない。これは既存の
  `assert_ne!(edited_src, src, "sanity: the edit must actually apply")` ガード
  （`tests.rs:6841`、`:6870`）の意図を、それらを現在支えているリテラル結合なしに保つものである。
- **FR4 — Class A の良性編集をアンカー＋バイトスライスで行う:** 字句走査器から見えない
  2 つの編集 — 実引数リストの再折り返しとコメント挿入 — は引き続きソーステキストに対する
  編集とし、次のとおり実装する。
  - **(a) 位置決定は構造的に、二段アンカーで行う。** まず外側の関数名
    （`run_button_decision`）を見つけ、次にその後ろにある呼び出し先の名前を見つけ、
    開き括弧から括弧深度で走査して呼び出しのバイト範囲を決める。行番号にも、呼び出し自身の
    テキストの部分文字列照合にも依存しない。
  - **(b) 構築は元のソースバイトのスライスで行う。** 編集後のソースは
    `original[..span_start] + prefix_edit + original[arg_span] + suffix_edit + original[span_end..]`
    という形にし、実引数のテキストはそのまま持ち回して、テスト側で書き直さない。これが
    ルーチンを閉じた集合外の結合から解放する仕組みであり、実引数の個数も、既存のインデント幅も、
    レシーバの識別子も必要としない。
  - **(c) mislocation 防止ガード。** 特定したバイト範囲について、期待するトークン形状 —
    呼び出し先パスに続くちょうど N 個のトップレベル実引数（ボタン経路では N = 4）— を
    併せてアサートする。既存の 3 つのアサーション（テキストが変わった／トークン列が変わらない／
    判定が依然 `Ok`）だけでは、編集が意図した呼び出し箇所に当たったことを証明できない。
    それを証明するのはこの形状アサーションであり、FR3 の vacuity 防止ガードとは別物である。
  - **(d) アンカーの精緻化。** 実務上可能な範囲で、完全修飾パスではなく呼び出し先関数名の
    素のセグメント（`apply_outcome_with_held`）にアンカーし、モジュールセグメントへの追加結合を
    避ける。既存の閉じた集合の定数（`HELD_AWARE_BUTTON_CALLEE`、`tests.rs:5996`）が既に
    `mouse_report` プレフィックスを持つため、これは選好であって必須要件ではなく、それらの定数を
    上書きすることはない。
- **FR5 — 全良性編集後も判定結果が変わらない:** 4 種の良性編集のいずれの後も、ボタン経路の
  判定は依然 `Ok` である。編集後の成果物は引き続き `run_button_decision` の本体を特定でき、
  `call_site_scan::judge_call` が、未編集の本体を判定するときと同じ閉じた集合の定数と同じ
  アリティ引数（`&HELD_AWARE_BUTTON_CALLEE, 4, 4, HELD_FIELD, &HELD_UNAWARE_CALLEE`、
  `tests.rs:6028-6036`）でそれを受理する。トークンレベルのクラスについては、編集が主張する
  とおりに意味的に等価であることも併せてアサートする（たとえば末尾カンマが実引数の個数を
  変えないこと）。

### Non-Functional Requirements

- **NFR1 — 走査モジュールの新機能なし、新規依存なし:** `call_site_scan` モジュールに
  範囲情報を持つ字句走査器を追加しない。Class A の編集は独立したアンカールーチンで
  ソースバイトに対して行い、Class B の編集は既存のトークンヘルパーを無改変で再利用する。
  新しいテストフレームワーク依存（proptest / criterion）を導入しない。変更は既存のインライン
  `#[cfg(test)]` モジュールファイル `src-tauri/src/window_host/tests.rs` の内側に留める。
- **NFR2 — テスト専用の変更集合:** プロダクションコードは無改変。`pointer_routing.rs`、
  `mouse_report.rs`、`event_loop.rs` はベースリビジョンのまま据え置く。変更集合は
  `src-tauri/src/window_host/tests.rs` に閉じ、さらにそのファイル内では良性編集テストと
  それが必要とするプライベートなテストヘルパーに閉じる。前 feature のその他のテスト
  （TS-1/2/3/4/5/6 および `pointer_routing_handlers_hold_no_decision_only_delegate_to_the_seam`
  を含む既存の構造テスト）は緑かつ無改変のまま維持する。
- **NFR3 — リポジトリのテスト規約:** テスト名は `<subject>_<scenario>_<expected>` に従う。
  テストは決定的かつ並列安全とし、共有のグローバルフィクスチャを持たず、`include_str!` に
  よるコンパイル時埋め込みとローカルな走査器状態のみを使い、`-- --test-threads=1` を
  必要としない。
- **NFR4 — フィーチャーゲートおよび実行コストへの影響なし:** テストは GUI 専用の
  `window_host` モジュール配下にあるため、
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  は影響を受けない。実行コストは引き続き無視できる程度であり、`#[ignore]` ゲートは不要。

## Implementation Approach

### Architecture

本 feature はランタイムのアーキテクチャを持たない。変更は `src-tauri/src/window_host/tests.rs`
のインライン `#[cfg(test)]` モジュール内に閉じる（NFR1 / NFR2）。良性編集テストは 2 つの
構築経路に分かれる。

```
include_str!("pointer_routing.rs")  ← プロダクションソース（無改変、A5 / A6）
        │
        ├─ Class B 経路（FR2）: 既存トークンヘルパー
        │     find_calls / split_arg_spans / splice_tokens
        │        → 編集後トークン列 → judge_call
        │
        └─ Class A 経路（FR4）: 独立アンカールーチン（走査モジュールには追加しない）
              二段アンカー → バイト範囲 → トークン形状アサート（FR4c）
                 → 元バイトのスライスで編集後ソース構築（FR4b）
                 → 再トークン化・再抽出 → judge_call
```

### Data Flow

```
Class B: 実本体 → トークン列 → splice → (差分アサート FR3) → judge_call = Ok (FR5)
Class A: 実ソース → アンカー → 範囲 → (形状アサート FR4c)
                 → スライス構築 → (差分アサート FR3) → 再抽出 → judge_call = Ok (FR5)
```

### API Design

該当なし（公開 API を追加も変更もしないため）。

### Database Schema

該当なし（永続化するデータも新しいデータモデルも導入しないため）。

### Dependencies

**Internal Dependencies:**
- `call_site_scan` のトークンヘルパー: `find_calls`（`tests.rs:5676`）、
  `contains_token_sequence`（`:5725`）、`split_arg_spans`（`:5740-5761`）、
  `splice_tokens`（`:5978`）— いずれも無改変で再利用する（NFR1）。
- 閉じた集合の定数: `tests.rs:5992-5999`（`HELD_AWARE_BUTTON_CALLEE`、`HELD_FIELD`、
  `HELD_UNAWARE_CALLEE` などを含む）。
- `include_str!("pointer_routing.rs")`: `tests.rs` と同ディレクトリのソース埋め込み（A6）。

**External Dependencies:**
- なし。新しいテストフレームワーク依存（proptest / criterion）は追加しない（NFR1）。

### File Structure

```
src-tauri/src/window_host/
├── tests.rs              # 本 feature の唯一の変更先（NFR2）
└── pointer_routing.rs    # include_str! の対象、無改変（NFR2 / A5）
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/benign-edit-test-token-rewrite/**`
- `test-docs/benign-edit-test-token-rewrite/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/{feature}/` directory at all; the declared
`test-docs/{feature}/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests

- [ ] TS-1: `button_path_token_level_trailing_comma_edit_is_still_accepted`（FR2 / FR3 / FR5）—
      実際の `run_button_decision` の本体を抽出し、held 対応呼び出しを特定して、
      `splice_tokens` でその実引数リストに末尾カンマトークンを追加する。トークン列が変わった
      こと、実引数の個数が依然 4 個であること、`judge_call` が `Ok` を返すことをアサートする。
- [ ] TS-2: `button_path_token_level_statement_reorder_is_still_accepted`（FR2 / FR3 / FR5）—
      抽出した本体の内側で、隣接する独立したトップレベル文 2 つを（ソーステキスト照合ではなく）
      構造的に特定し、`splice_tokens` で入れ替える。トークン列が変わったことと、`judge_call` が
      `Ok` を返すことをアサートする。
- [ ] TS-3: `button_path_anchored_rewrap_edit_is_still_accepted`（FR4 / FR3 / FR5）—
      二段アンカーで呼び出しのバイト範囲を求め、その範囲のトークン形状が
      「呼び出し先パス + 実引数 4 個」であることをアサートし、スライスによってソースを
      再構築して実引数バイトをそのまま持ち回しつつ周囲の空白を再折り返しする。ソーステキストが
      変わったことをアサートし、再トークン化・再抽出したうえで `judge_call` が `Ok` を返すことを
      アサートする。
- [ ] TS-4: `button_path_anchored_comment_insertion_edit_is_still_accepted`（FR4 / FR3 / FR5）—
      TS-3 と同じアンカーおよびスライス経路を用い、呼び出し先と開き括弧の間にブロックコメントを
      挿入する。
- [ ] TS-5: `anchor_rejects_a_span_whose_token_shape_does_not_match_the_expected_call`（FR4）—
      メモリ内の否定テスト。実引数の個数が異なる／呼び出し先が異なる、呼び出し形状の箇所に
      対してアンカールーチンを動かし、静かに範囲を生成せず明示的な失敗を報告することを
      アサートする。AC-6 のガードが空虚でないことを示す。
- [ ] TS-6: `anchor_reports_an_explicit_failure_on_a_comment_broken_function_name_anchor`（FR4）—
      `fn` キーワードと関数名の間にブロックコメントが挟まるメモリ内入力に対し、ルーチンが
      誤った関数にアンカーせず、パニックもせず、明示的な `Err` を返すことをアサートする。
      EC-1 を固定する。
- [ ] TS-7: `anchor_reports_an_explicit_failure_on_an_unbalanced_paren_inside_a_comment`（FR4）—
      実引数リスト内のブロックコメントに不均衡な閉じ括弧を含むメモリ内入力に対し、括弧深度
      走査の結果が明示的な失敗である（あるいは走査がコメント対応である）ことをアサートし、
      静かに誤った範囲を返さないことを示す。EC-2 を固定する。

### Integration Tests

該当なし（新しい結合テストのコンパイル単位を追加しないため、NFR1）。

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

- [ ] TS-M1: `manual: none required`（NFR2）— プロダクション挙動は無変更であり、E2E ハーネスも
      存在しないため手動確認は不要。AC-8 は `git diff --name-only` / `git diff --stat` で満たす。

### Edge Cases

- [ ] EC-1: `fn` キーワードと関数名の間に挿入されたブロックコメントが関数名アンカーを破る。
      受容したリスク（Codex 相談）。要求される扱い: ルーチンは誤った関数にアンカーせず、
      明示的な失敗を返す。静かに誤った範囲を返すことはしない。TS-6 で固定する。
- [ ] EC-2: 実引数リスト内のブロックコメントに含まれる不均衡な閉じ括弧が、素朴な括弧深度走査を
      破る。受容したリスク（Codex 相談）。要求される扱い: 明示的な失敗、またはコメント対応の
      走査。静かに誤った範囲を返すことはしない。TS-7 で固定する。
- [ ] EC-3: 「テキストが変わった／トークン列が変わらない／判定が依然 `Ok`」の 3 アサーションは、
      編集が意図した呼び出し箇所に当たったことを証明しない。受容したリスク（Codex 相談）。
      FR4(c) の、特定した範囲に対するトークン形状アサーションで閉じる。AC-6 / TS-5 で固定する。
- [ ] EC-4: 末尾カンマが `split_arg_spans` の見る実引数の個数を変えてはならない。当該ヘルパー
      （`tests.rs:5740-5761`）が既に保証しており、TS-1 で再アサートする。
- [ ] EC-5: `apply_outcome_with_held` が禁止対象の `apply_outcome` と取り違えられてはならない。
      トークン単位の全一致照合（`contains_token_sequence`、`tests.rs:5725`）が両者を区別する。
      本 feature では無変更だが、書き換え後も成り立ち続ける必要がある。
- [ ] EC-6: ホイール経路の良性編集カバレッジ — 現状は良性編集テストがボタン経路にのみ存在する
      （`tests.rs:6830`）。本 feature ではホイール経路版を追加しない。後日追加する場合は
      N = 4 ではなく N = 5 として FR2 / FR4 に従う。

### Performance Tests

該当なし（実行コストが無視できるため、NFR4）。

## Security Considerations

該当なし。本変更はテスト専用であり、認証・認可・入力検証・XSS・インジェクション・
データ保護のいずれの表面も持たず、ランタイムのコード経路を追加しない。

## Error Handling

アンカールーチンの失敗はテストのアサーション失敗、または明示的な `Err` として表面化する
（FR4、EC-1、EC-2）。静かに誤った範囲を返す経路は許容しない。

## Success Criteria

- [ ] AC-1: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
      が、書き換え後の良性編集テストを含めてパスする。
- [ ] AC-2: `src-tauri/src/window_host/tests.rs` に `pointer_routing.rs` のプロダクション
      ソーステキストを再現する文字列リテラルが存在しない。とくに現在の `tests.rs:6832`、
      `:6859`、`:6860` の 3 リテラルが消えており、置き換えのリテラルも FR1 の閉じた集合の
      外側の識別子を名指さない。（FR1）
- [ ] AC-3: 末尾カンマ編集と文の並べ替え編集が、`include_str!("pointer_routing.rs")` から
      抽出した本体に対して `find_calls` / `split_arg_spans` / `splice_tokens` で構築されており、
      スプライス後のトークン列に対する判定が `Ok` である。（FR2、FR5）
- [ ] AC-4: 再折り返し編集とコメント挿入編集が、FR4(a) の二段アンカーで箇所を特定し、
      FR4(b) に従って元のバイトをスライスして編集後ソースを構築している。編集後ソース中の
      実引数テキストは元の実引数テキストとバイト単位で同一である。（FR4）
- [ ] AC-5: 各良性編集が、受け入れをアサートする前に自身の適用（編集後の成果物 != 元の成果物）を
      アサートしており、何も変えない編集はテストを赤にする。（FR3）
- [ ] AC-6: Class A のルーチンが、特定した範囲のトークン形状 — ボタン経路では呼び出し先パスに
      続くトップレベル実引数ちょうど 4 個 — をアサートしており、誤った呼び出し箇所に一致した
      アンカーは、テキストが変わりトークン列が変わらず判定が依然 `Ok` であってもテストを
      赤にする。（FR4c）
- [ ] AC-7: 4 種すべての良性編集の後も、`judge_call` が無変更の閉じた集合定数とアリティ引数で
      `Ok` を返し続ける。（FR5）
- [ ] AC-8: 本 feature の `git diff --name-only` が、`src-tauri/` 配下では
      `src-tauri/src/window_host/tests.rs` のみを示し、プロダクションファイルに一切
      触れていない。（NFR2）
- [ ] AC-9: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
      が引き続きコンパイルできる。（NFR4）

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし（FR1〜FR5 はすべて `status: resolved` であり、`status: tbd` の要件は存在しない）。

## Assumptions

- **A1**: 良性編集テストの Class A 編集（実引数リストの再折り返し、コメント挿入）は
  ソーステキストに対する編集のままとし、二段アンカー（外側の関数名、その後ろの呼び出し先名、
  続いて呼び出しのバイト範囲を求める括弧深度走査）で箇所を特定する。編集後ソースは元のバイトを
  スライスして構築し、実引数テキストはそのまま持ち回してテスト側で書き直さない。Class B 編集
  （末尾カンマ、文の並べ替え）は `find_calls` / `split_arg_spans` / `splice_tokens` により
  トークンレベルで再現する。走査モジュールに範囲情報を持つ字句走査器は追加しない。
  （可逆。create-spec の Codex 相談で決定。answer `requirement.class-n-benign-edit-coverage`、
  option `closed_set_text_anchor`、packet `benign-edit-test-token-rewrite-q0001`。
  batch ポリシーの `record_as_assumption` による）
- **A2**: 完全修飾パスではなく呼び出し先関数名の素のセグメント（`apply_outcome_with_held`）に
  アンカーすることで `mouse_report` モジュールセグメントへの追加結合を避けられるため、
  実務上可能な範囲でそちらを選好する。必須要件ではない。既存の閉じた集合の定数
  `HELD_AWARE_BUTTON_CALLEE` は既にモジュールプレフィックスを持っており、この精緻化が
  それらの定数と矛盾することはない。（可逆。同じ Codex 相談による精緻化。
  batch ポリシーの `record_as_assumption` による）
- **A3**: 3 つの残存リスクは除去せず受容し、EC-1 / EC-2 / EC-3 として要件に取り込む。対応は
  FR4 が示すとおり — コメントで壊れた関数名アンカーには明示的な失敗、実引数リスト内の
  コメントに含まれる不均衡な閉じ括弧には明示的な失敗（またはコメント対応の走査）、そして
  FR3 の vacuity 防止ガードとは別物の mislocation 防止ガードとして、特定した範囲に対する
  トークン形状アサーション。（可逆。同じ Codex 相談で明示的に受容。
  batch ポリシーの `record_as_assumption` により、黙って落とさず記録）
- **A4**: 本 feature では design ステップを省略する。（可逆。gate `create-spec.design-step` にて
  batch ポリシーで解決。option `skip_design`、source `batch-policy`。既存のインライン
  テストモジュール 1 つに閉じたテスト専用の変更であり、UI 表面も視覚的出力も新しい
  モジュール境界も持たないため）
- **A5**: プロダクションの呼び出し箇所はベースリビジョンで正しく、無改変で据え置かれるため、
  書き換え後のテストは緑から始まる。`pointer_routing.rs:850` が
  `mouse_report::apply_outcome_with_held` の第 4 実引数として `host.mouse_report_held` を渡し、
  `pointer_routing.rs:1129-1135` が `mouse_report::apply_wheel_report_step` の第 5 実引数として
  同じ値を渡す。（可逆。既存のプロダクションソースおよび前 feature のテストで固定された事実）
- **A6**: `include_str!("pointer_routing.rs")` は `tests.rs` から解決できる。両ファイルが
  `src-tauri/src/window_host/` に置かれているため。これは同ファイル内の複数テストで既に
  使われている確立した手法である。（可逆。既存テストで固定された事実）
- **A7**: 前 feature の NFR5 の例外 — ソーステキストに対する構造アサーションは許可するが、
  ボタン経路とホイール経路の held 実引数の受け渡し検査に限る — は引き続き有効であり、
  本 feature はその例外の内側に留まる。本 feature が変えるのは良性編集を「どう構築するか」で
  あって「何をアサートするか」ではなく、source-scan の対象接続を 1 つも広げない。
  （可逆。前 feature の SPEC から引き継いだ制約）

## References

- 要件定義書: `feature-docs/benign-edit-test-token-rewrite/REQUIREMENTS.md`
- 前 feature の SPEC: `feature-docs/mouse-report-held-callsite-test/SPEC.md`
- 前 feature の要件定義書: `feature-docs/mouse-report-held-callsite-test/REQUIREMENTS.md`
- 書き換え対象のテスト: `src-tauri/src/window_host/tests.rs:6825-6888`
- 取り除くソースリテラル: `src-tauri/src/window_host/tests.rs:6832, 6859, 6860`
- 既存の sanity ガード: `src-tauri/src/window_host/tests.rs:6841, 6870`
- 閉じた集合の定数: `src-tauri/src/window_host/tests.rs:5992-5999`（`HELD_AWARE_BUTTON_CALLEE` は `:5996`）
- 既存のトークンヘルパー: `src-tauri/src/window_host/tests.rs:5676`（`find_calls`）、
  `:5725`（`contains_token_sequence`）、`:5740-5761`（`split_arg_spans`）、`:5978`（`splice_tokens`）
- 未編集本体の判定呼び出し: `src-tauri/src/window_host/tests.rs:6028-6036`
- 対象プロダクションコード: `src-tauri/src/window_host/pointer_routing.rs:850, 1129-1135`
