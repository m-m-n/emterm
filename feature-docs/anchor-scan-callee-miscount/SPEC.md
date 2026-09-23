# Feature: anchor-scan-callee-miscount

## Overview

テスト専用モジュール `anchor_scan` の `locate_call` には、callee の呼び出しや関数定義を数え落とす経路が残っている。このため、呼び出しが 2 つある本体や深さ 0 の定義が 2 つあるソースでも `Ok` が返る。本機能はこれらの経路を塞ぎ、曖昧な入力に対して必ず `Err` を返すようにする。

## Objectives

- テスト専用の `anchor_scan::locate_call` が callee の呼び出しや関数定義を数え落とす経路を塞ぐ。callee の呼び出しが 2 つある入力、または関数の深さ 0 の定義が 2 つある入力では、`Ok` ではなく必ず `Err` を返す。これにより anchor-scan-false-success の FR4 の不変条件「誤った範囲を成功として返さない」を守る。

## Technical Requirements

### Functional Requirements

- **FR1: 生識別子の callee を step 2 で数える**
  `locate_call` の step 2 は、`r#` 接頭辞を取り除いてから名前を比較する。トークン `r#<name>` の直後に `(` が続く場合、callee `<name>` の呼び出しとして数える。本体に `r#probe_call(w, x, y, z); probe_call(a, b, c, d);` を含む場合、`locate_call(src, "probe_fn", "probe_call", 4)` は `Err` を返す。

- **FR2: 数値リテラルの走査を `..` と `.識別子` の手前で止める**
  数値リテラルの走査は、直後の文字が `.` または識別子開始文字である `.` の手前で終わる。直後が数字の `.` はリテラルの一部として続ける（例: `1.5`、`1.0f32`）。本体に `let r = 0..probe_call(k); probe_call(a, b, c, d);` を含む場合、`locate_call` は `Err` を返す。本体に `self.0.probe_call(x); probe_call(a, b, c, d);` を含む場合も同様に `Err` を返す。

- **FR3: turbofish 形式の callee 呼び出しを step 2 で数える**
  step 2 では、callee 名、`::`、`<`…`>` のジェネリック引数リスト（入れ子の `<` / `>` は対応を取る）、`(` の順に並ぶものを callee の呼び出しとして数える。本体に `probe_call::<u8>(w, x, y, z); probe_call(a, b, c, d);` を含む場合、`locate_call(src, "probe_fn", "probe_call", 4)` は `Err` を返す。

- **FR4: 生識別子の関数名を step 1 で数える**
  step 1 の `fn_name` の比較でも、`r#` 接頭辞を取り除いてから比較する。深さ 0 の `fn r#probe_fn(...) { ... }` と `fn probe_fn(...) { ... }` を両方含むソースでは、`locate_call(src, "probe_fn", "probe_call", 4)` は曖昧として `Err` を返す。`fn` キーワードの判定は引き続き素のトークン `fn` にだけ一致させる。`r#fn` はキーワードとして扱わない。

- **FR5: トークンのバイト範囲は変えない**
  `r#` の除去は名前の比較にだけ適用する。トークンの開始・終了バイトオフセット、`callee_end`、`open_paren_end`、`close_paren_start`、返す引数範囲は現状と同じ方法で計算する。単独の呼び出し `r#probe_call(a, b, c, d)` は `Ok` でアンカーされ、その `callee_end` は `r#probe_call` の直後になる。

### Non-Functional Requirements

- **NFR1:** 変更は `src-tauri/src/window_host/tests.rs` 内の `#[cfg(test)]` の `anchor_scan` モジュールとそのテストに限る。本番ソースは変更しない。別のトークナイザである `call_site_scan` は変更しない。
- **NFR2:** 既存の `anchor_locate_call_*` テストと、実際に埋め込まれた `pointer_routing` ソースに対してアンカーする良性編集テスト `button_path_rewrapped_argument_list_edit_is_accepted` / `button_path_call_with_inserted_comment_edit_is_accepted` はすべて緑のまま保つ。

## Acceptance Criteria

- [ ] **AC-1** (FR1): 本体に `r#probe_call(w, x, y, z)` と `probe_call(a, b, c, d)` を両方含む負のテストが、`locate_call(..., "probe_fn", "probe_call", 4)` に `Err` を期待する。
- [ ] **AC-2** (FR2): 本体に `let r = 0..probe_call(k);` と `probe_call(a, b, c, d);` を両方含む負のテストが `Err` を期待する。
- [ ] **AC-3** (FR2): 本体に `self.0.probe_call(x);` と `probe_call(a, b, c, d);` を両方含む負のテストが `Err` を期待する。
- [ ] **AC-4** (FR3): 本体に `probe_call::<u8>(w, x, y, z);` と `probe_call(a, b, c, d);` を両方含む負のテストが `Err` を期待する。
- [ ] **AC-5** (FR4): 深さ 0 の定義 `fn r#probe_fn` と `fn probe_fn` を両方含む負のテストが `Err` を期待する。
- [ ] **AC-6** (FR1, FR5): 単独の呼び出し `r#probe_call(a, b, c, d)` が `Ok` でアンカーされ、引数テキストが `a, b, c, d`、`callee_end` が `r#probe_call` の直後になる。
- [ ] **AC-7** (FR2): 浮動小数点・接尾辞付きの数値引数を持つ単独の呼び出し `probe_call(1.5, 1.0f32, 1e10, 0x1F)` が `Ok` でアンカーされ、引数が 4 個、引数テキストが `1.5, 1.0f32, 1e10, 0x1F` になる。
- [ ] **AC-8** (FR1, FR2, NFR2): タスクの 2 つの再現手順が `Ok` ではなく `Err` を返し、既存の `anchor_locate_call_*` テストと良性編集テストがすべて緑のまま保たれる。

## Implementation Approach

### 対象

- `src-tauri/src/window_host/tests.rs` の `#[cfg(test)]` 配下にある `anchor_scan` モジュール（NFR1）
  - トークン走査（生識別子の分岐、数値リテラルの走査）
  - `locate_call` の step 1（`fn_name` の比較）と step 2（callee 呼び出しの一意性チェック）

### 前提（Assumptions）

- **as-1:** step 1 の `fn` キーワード判定は、引き続き素のトークン `fn` にだけ一致する。`r#fn` は識別子であり、キーワードとして扱わない。（理由: Rust の生識別子の意味論。影響: low。可逆: はい）
- **as-2:** 本体にある callee の呼び出しが turbofish 形式の 1 つだけ（`probe_call::<u8>(a, b, c, d)` のみ）の場合、`locate_call` は現状どおり `Err` を返し続ける。FR3 が変えるのは一意性のカウントだけで、step 3 がアンカーする呼び出し形式は変えない。（理由: 現状の挙動を保つ。明示的な失敗が誤った範囲を返すことはなく、requirement.turbofish-callee-scope への回答が求めているのは、turbofish 呼び出しが別の呼び出しと並ぶ場合の `Err` だけである。影響: low。可逆: はい）
- **as-3:** `fn_name` が `probe_fn` のとき、深さ 0 に `fn r#probe_fn` の定義が 1 つだけあれば、それを見つける（FR4 の素の名前による比較の帰結）。（理由: Rust では `r#probe_fn` と `probe_fn` は同じ名前として扱われる。影響: low。可逆: はい）

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/{feature}/**`
- `test-docs/{feature}/**`

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

いずれも `locate_call(src, "probe_fn", "probe_call", 4)` を呼ぶ。

- [ ] **TS-1** (AC-1, 負): 入力 `fn probe_fn(w: i32, x: i32, y: i32, z: i32) { r#probe_call(w, x, y, z); probe_call(a, b, c, d); }` - `Err` を返す
- [ ] **TS-2** (AC-2, 負): 入力 `fn probe_fn(a: i32, b: i32, c: i32, d: i32) { let r = 0..probe_call(k); probe_call(a, b, c, d); }` - `Err` を返す
- [ ] **TS-3** (AC-3, 負): 入力 `fn probe_fn(a: i32, b: i32, c: i32, d: i32) { self.0.probe_call(x); probe_call(a, b, c, d); }` - `Err` を返す
- [ ] **TS-4** (AC-4, 負): 入力 `fn probe_fn(w: i32, x: i32, y: i32, z: i32) { probe_call::<u8>(w, x, y, z); probe_call(a, b, c, d); }` - `Err` を返す
- [ ] **TS-5** (AC-5, 負): 入力 `fn r#probe_fn(a: i32, b: i32, c: i32, d: i32) { probe_call(a, b, c, d); } fn probe_fn(w: i32, x: i32, y: i32, z: i32) { probe_call(w, x, y, z); }` - `Err` を返す
- [ ] **TS-6** (AC-6, 正): 入力 `fn probe_fn(a: i32, b: i32, c: i32, d: i32) { r#probe_call(a, b, c, d); }` - `Ok` を返し、`src[args_start..args_end] == "a, b, c, d"`、`src[..callee_end]` が `"r#probe_call"` で終わる
- [ ] **TS-7** (AC-7, 正): 入力 `fn probe_fn(a: i32, b: i32, c: i32, d: i32) { probe_call(1.5, 1.0f32, 1e10, 0x1F); }` - `Ok` を返し、`src[args_start..args_end] == "1.5, 1.0f32, 1e10, 0x1F"`

### Regression Tests

- [ ] **TS-8** (AC-8, 回帰): 既存スイート。`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib anchor_locate_call` に加え、`button_path_*` の良性編集テストを実行する - すべて緑

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases

- [ ] **EC-1:** `0..=probe_call(k)` - 直後の文字が `.` なので、リテラルは最初の `.` で終わる（FR2）
- [ ] **EC-2:** タプルインデックス `x.0.1` - `0.1` が 1 個のリテラルのまま残ってもよい。識別子は失われない
- [ ] **EC-3:** turbofish 内の入れ子のジェネリクス（`probe_call::<Vec<u8>>(...)`）- `>` トークンが `<` トークンと対応を取る。スキャナは各 `>` を個別の句読点トークンとして出力する（FR3）
- [ ] **EC-4:** 呼び出しではない turbofish 参照（`let f = probe_call::<u8>;`）- 呼び出しではないので数えない（FR3 は閉じる `>` の後に `(` を要求する）
- [ ] **EC-5:** callee 名、`::`、ジェネリック引数リスト、`(` の間にコメントがある場合 - スキャンがコメントトークンを捨てるため、カウントは変わらない

## Success Criteria

- [ ] FR1〜FR5 がすべて実装され、テストされている
- [ ] TS-1〜TS-8 がすべて通る
- [ ] NFR1 の変更範囲と NFR2 の既存テストの緑が守られている

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし（FR1〜FR5 はすべて resolved）。

## References

- anchor-scan-false-success の SPEC FR4（不変条件「誤った範囲を成功として返さない」）
