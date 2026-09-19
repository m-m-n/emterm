# Feature: wheel-report-notch-clamp

## Overview

マウスレポート経路のホイールレポート複製に上限を導入する。現状、1 回のホイールイベントが
要求できる複製回数に上限が無く、数ギガバイト規模のアロケーションと PTY への大量書き込みを
引き起こせる。本機能は `src-tauri/src/window_host/input_translate.rs` にレポート経路専用の
定数 `MAX_WHEEL_REPORT_NOTCHES = 100` を導入し、変換側（`as i32` キャスト前）と消費側
（`Vec::with_capacity` とループ）の 2 層でクランプする。要件の全文は
`feature-docs/wheel-report-notch-clamp/REQUIREMENTS.md` を参照。

## Objectives

- 1 回のホイールイベントで数ギガバイト規模のアロケーションを要求し PTY を溢れさせられる、
  上限の無いホイールレポート複製を取り除き、PR #69 で導入されたマウスレポート経路の
  ローカル DoS 耐性を回復する。
- 1 回のホイールイベントが子プロセスへ書き込めるバイト数の最悪値を、呼び出し側の規律ではなく
  構造として上限づけ、慣性スクロールや合成されたピクセルバーストが winit イベントループを
  停止させられないようにする。
- レビュー round 3 で持ち越しとなったセキュリティ所見 7063d53b08e160d8（round 2 の
  ef02afd34f1da70f から未解決で継続）を解消する。3 名のレビュアーが独立に報告した所見で、
  修正が 2 ファイルにまたがるため単一ファイルの自動修正経路では対応できなかった。

## User Stories

### US1: 病的なホイールデルタでも上限内に収まる
eMterm 利用者として、マウストラッキング対応 TUI 上で慣性スクロールや合成されたピクセル
バーストが発生しても、1 回のホイールイベントが上限を超えるアロケーションや PTY 書き込みを
引き起こさないようにしたい。ターミナルと子プロセスが応答を保つため。

**Acceptance Criteria:**
- [ ] AC1: `f32::MAX`、`f32::MIN`、±INFINITY、NaN を含む任意の f32 入力に対し、
      `wheel_report_notches` は絶対値が `MAX_WHEEL_REPORT_NOTCHES` 以下の値を返し、
      所見に書かれた攻撃シナリオ（ノッチ数が `i32::MAX` に飽和する）が到達不能になっている。
- [ ] AC2: 複製ヘルパーは、`u32::MAX` までの任意の要求複製回数に対して
      `payload_len * MAX_WHEEL_REPORT_NOTCHES` バイトを超えるアロケーションも返却も行わない。
- [ ] AC5: `f32::INFINITY`、`f32::NAN`、非常に大きい有限値をクランプに対して実行する
      ユニットテストが存在し、タスクの定める完了条件を満たしている。

### US2: 通常のスクロール体験が変わらない
eMterm 利用者として、通常のホイール操作では今までとまったく同じ挙動であってほしい。
セキュリティ修正が操作感を劣化させないため。

**Acceptance Criteria:**
- [ ] AC4: 既存の 3 テスト `wheel_report_notches_signed_whole_counts`、
      `wheel_report_notches_sub_notch_delta_is_zero`、`wheel_report_notches_non_finite_is_zero`
      が、アサーションを編集せずに引き続き通る。
- [ ] AC7: ノッチ数が 0 のホイールイベントは引き続き PTY へ何も書き込まず、非ゼロの
      ノッチ数では引き続きホイールイベント 1 回につき `tab.write_input` 呼び出しが
      ちょうど 1 回発生する。

### US3: 安全上の上限が将来の調整で緩まない
eMterm のメンテナとして、レポート経路の上限を独立した定数として持ちたい。alternate scroll
経路の操作感の再調整がレポート経路の安全上の上限を暗黙に変えないようにするため。

**Acceptance Criteria:**
- [ ] AC3: `MAX_WHEEL_REPORT_NOTCHES` が独立した定数として存在して 100 に等しく、
      `MAX_ALT_SCROLL_NOTCHES` が現在値のまま残っている。
- [ ] AC6: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
      が通り、`CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
      が引き続き成功する。

## Technical Requirements

### Functional Requirements

- **FR1:** レポート経路専用のノッチ上限定数。`src-tauri/src/window_host/input_translate.rs` に
  レポート経路のみで使う定数 `MAX_WHEEL_REPORT_NOTCHES = 100` を、既存の
  `MAX_ALT_SCROLL_NOTCHES = 100`（input_translate.rs:332）と併存しつつ独立して導入する。
  既存の alternate scroll 用定数は再利用せず、変更もしない。2 つの経路は異なる理由で
  チューニングされており、1 つの定数を共有すると alternate scroll 側の操作感の調整が
  レポート経路の安全上の上限を暗黙に変えてしまう。
- **FR2:** `as i32` キャスト前の大きさ飽和。`wheel_report_notches`（input_translate.rs:452）は
  `as i32` キャストを行う**前**にホイール差分の大きさを `MAX_WHEEL_REPORT_NOTCHES` へ
  飽和させ、大きい有限の f32 がキャストの `i32::MAX` への飽和挙動に到達できないようにする。
  関数の文書化された契約は「任意の f32 入力に対し、戻り値の絶対値は
  `MAX_WHEEL_REPORT_NOTCHES` 以下である」となる。
- **FR3:** 上限未満での `wheel_report_notches` 既存セマンティクス保持。クランプ後の大きさが
  変化しない入力では現行挙動を厳密に維持する。非有限入力（NaN、±INFINITY）は 0 を返す。
  サブノッチの大きさ（`|lines| < 1.0`）はゼロ方向へ切り捨てて 0 を返す。非ゼロの結果の符号は
  `lines` の符号に従う（`lines >= 0.0` なら正、それ以外は負）。大きさは絶対値の floor。
  公開シグネチャは `pub(super) fn wheel_report_notches(lines: f32) -> i32` のまま。
- **FR4:** 複製処理の純粋関数への切り出し。現在
  `src-tauri/src/window_host/pointer_routing.rs:917-921` にインライン展開されている
  レポートバイトの複製を、シングルノッチのペイロードバイトと要求複製回数を受け取って
  複製済みバッファを返す純粋関数へ抽出する。この関数は `app` にも `tabs` にもその他の
  ホスト状態にも依存しないため、単体でユニットテスト可能である。
- **FR5:** 呼び出し側での第 2 のクランプ層（多層防御）。FR4 の抽出関数は要求複製回数を
  FR2 と同じ定数 `MAX_WHEEL_REPORT_NOTCHES` で再度クランプし、クランプ後の値を
  `Vec::with_capacity` の引数と複製ループの上限の**双方**に用いる。クランプは独立した 2 層で
  適用され、変換側が契約を保証し、消費側は呼び出し元を信頼せずに同じ上限を再適用する。
- **FR6:** 呼び出し側のヘルパー経由への付け替え。pointer_routing.rs のホイールレポート分岐
  （lines 913-924 の `if let Some((tab_id, bytes)) = dest.into_iter().next()` ブロック）は、
  インラインのアロケーションとループをやめて FR4 のヘルパーを呼ぶ。それ以外の観測可能な
  挙動は変えない。ノッチ数 0 では引き続き何も書き込まず、非ゼロのノッチ数では引き続き
  複製済みバッファ全体を運ぶ `tab.write_input(buf)` 呼び出しがちょうど 1 回発生する。
- **FR7:** 変換関数の境界値・病的入力ユニットテスト。`wheel_report_notches` について
  境界値 99.999、100.0、100.999、101.0 とその負の対応値、NaN、+INFINITY、-INFINITY、
  `f32::MAX`、`f32::MIN`、+0.0、-0.0、±0.999、±1.999 を網羅し、加えて任意の有限入力に対して
  結果の絶対値が `MAX_WHEEL_REPORT_NOTCHES` 以下であることを主張する不変条件テストを置く。
- **FR8:** 複製ヘルパーのユニットテスト。FR4 のヘルパーについて、要求回数 0 で空バッファ、
  要求回数 1 で入力ペイロードとバイト単位で同一のバッファ、要求回数 100 でちょうど 100 個の
  連結、要求回数 101 と `u32::MAX` がいずれも 100 個に制限されること、そして出力長が
  「ペイロード長 × `MAX_WHEEL_REPORT_NOTCHES`」を超えないという不変条件を網羅する。

### Non-Functional Requirements

- **NFR1 - Performance:** 最悪アロケーションの上限化。レポート経路で 1 回のホイール
  イベントが要求しうるアロケーションは「シングルノッチの SGR ペイロード長 ×
  `MAX_WHEEL_REPORT_NOTCHES`」を上限とする。これはキロバイト規模であり、現状で可能な
  約 21 GB の要求に対する上限となる。クランプされていないノッチ数から容量を計算する
  コード経路があってはならない。
- **NFR2 - Performance:** イベントあたりの PTY 書き込み量の上限化。1 回のホイール
  イベントが PTY へ書き込むレポートバイトは最大で `MAX_WHEEL_REPORT_NOTCHES` ノッチ相当で
  あり、慣性スクロールや合成されたピクセルバーストが子プロセスを溢れさせたり winit
  イベントループを停止させたりできない。
- **NFR3 - Usability:** 通常スクロールへの体感変化なし。通常のホイール入力（100 ノッチを
  はるかに下回る大きさ）は現行実装とバイト単位で同一の PTY 出力を生成する。クランプに
  到達できるのは病的または合成されたデルタのみである。
- **NFR4 - Compatibility:** 新規依存なし・可視性の拡大なし。本修正はクレート依存を追加せず、
  `window_host` モジュールツリー内で `pub(super)` を超える可視性の項目を導入しない。
- **NFR5 - Compatibility:** alternate scroll 経路は不変。`MAX_ALT_SCROLL_NOTCHES` と
  `alternate_scroll_wheel_bytes`（input_translate.rs:332、361-384）はそのまま残す。
  alternate scroll 経路は既に同等の上限を自前で持っており、対象外である。
- **NFR6 - Maintainability:** テストスタイルを既存スイートに合わせる。新規テストは
  test/README.md に従い、対象関数の既存の置き場所（`src-tauri/src/window_host/tests.rs`）での
  インライン `#[cfg(test)]` スタイル、`<subject>_<scenario>_<expected>` の命名、標準
  ライブラリのアサーションのみを用い、新たなテストフレームワーククレートを追加しない
  （proptest なし、criterion なし）。

## Implementation Approach

### Architecture

**System Architecture:**
```
┌─────────────────────────────────────────────────────────┐
│ winit wheel event (f32 line delta)                      │
├─────────────────────────────────────────────────────────┤
│ input_translate.rs                                      │
│   MAX_WHEEL_REPORT_NOTCHES = 100   (FR1)                │
│   wheel_report_notches(lines: f32) -> i32               │
│     clamp magnitude BEFORE `as i32`  (FR2, layer 1)     │
├─────────────────────────────────────────────────────────┤
│ pointer_routing.rs                                      │
│   wheel report branch (:913-924)      (FR6)             │
│     -> duplication helper             (FR4)             │
│          re-clamp requested count     (FR5, layer 2)    │
│          with_capacity + loop bound from clamped value  │
├─────────────────────────────────────────────────────────┤
│ tab.write_input(buf)  — exactly one call per event      │
├─────────────────────────────────────────────────────────┤
│ PTY -> child process                                    │
└─────────────────────────────────────────────────────────┘
```

**Component Diagram:**
```
input_translate.rs
  MAX_WHEEL_REPORT_NOTCHES  (report path only; independent of MAX_ALT_SCROLL_NOTCHES)
  wheel_report_notches      (pub(super); signature unchanged)
  MAX_ALT_SCROLL_NOTCHES    (untouched — NFR5)
  alternate_scroll_wheel_bytes (untouched — NFR5)

pointer_routing.rs
  wheel report branch       (calls the helper instead of inlining — FR6)
  duplication helper        (pure: payload bytes + requested count -> buffer — FR4/FR5)

tests.rs
  existing conversion tests (:1498-1513, unmodified — AC4)
  existing alt-scroll clamp test (:2691-2697, unmodified — NFR5)
  new conversion tests      (FR7)
  new duplication helper tests (FR8)
```

### Data Flow

```
wheel event (f32 lines)
  → wheel_report_notches: 非有限なら 0 / floor(abs) → MAX_WHEEL_REPORT_NOTCHES へ飽和
      → as i32 → lines の符号を適用
  → notch count (|n| <= MAX_WHEEL_REPORT_NOTCHES)
  → duplication helper: requested count を MAX_WHEEL_REPORT_NOTCHES で再クランプ
      → with_capacity(clamped * payload.len()) → clamped 回のループ
  → buffer (len <= payload.len() * MAX_WHEEL_REPORT_NOTCHES)
  → tab.write_input(buf)  (ノッチ数 0 のときは呼ばない)
```

### API Design

該当なし。ネットワーク API を持たない。変更されるのはモジュール内部の Rust 関数のみで、
`wheel_report_notches` の公開シグネチャ
（`pub(super) fn wheel_report_notches(lines: f32) -> i32`）は変わらない。

### Database Schema

該当なし。永続データを扱わない。

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/window_host/input_translate.rs`: 上限定数と変換関数の所在（FR1、FR2、FR3）。
- `src-tauri/src/window_host/pointer_routing.rs`: 複製ヘルパーと呼び出し側（FR4、FR5、FR6）。
  `wheel_report_notches` の import は line 17、呼び出しは line 914。
- `src-tauri/src/window_host/tests.rs`: ユニットテストの所在（FR7、FR8）。import は line 14、
  既存アサーションは lines 1498-1513。

**External Dependencies:**
- なし。NFR4 によりクレート依存を追加しない。

### File Structure

```
src-tauri/src/window_host/
├── input_translate.rs      # MAX_WHEEL_REPORT_NOTCHES (FR1), wheel_report_notches (FR2/FR3)
│                           # MAX_ALT_SCROLL_NOTCHES / alternate_scroll_wheel_bytes は不変 (NFR5)
├── pointer_routing.rs      # 複製ヘルパー (FR4/FR5), ホイールレポート分岐の付け替え (FR6)
└── tests.rs                # 既存テスト維持 (AC4) + 新規テスト (FR7/FR8)
```

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

- [ ] TS1 (FR2/FR3/FR7): 変換の境界値 — `wheel_report_notches` は 99.999 で 99、100.0 で 100、
      100.999 で 100、101.0 で 100 を返し、対応する各負入力に対してその符号反転値を返す
      （-99.999 → -99、-100.0 → -100、-100.999 → -100、-101.0 → -100）。
- [ ] TS2 (FR2/FR3/FR7): 変換の病的入力 — `wheel_report_notches` は NaN で 0、+INFINITY で 0、
      -INFINITY で 0 を返し、`f32::MAX` で `MAX_WHEEL_REPORT_NOTCHES`、`f32::MIN` で
      `-MAX_WHEEL_REPORT_NOTCHES` を返す。
- [ ] TS3 (FR3/FR7): 変換のゼロ近傍・サブノッチ入力 — `wheel_report_notches` は +0.0、-0.0、
      +0.999、-0.999 で 0 を返し、1.999 で 1、-1.999 で -1 を返す。
- [ ] TS4 (FR2/FR7): 変換のクランプ不変条件 — 小さい値・境界値・大きい値・極端な値を両符号で
      掃引した有限入力の集合に対し、すべての結果が
      `result.unsigned_abs() <= MAX_WHEEL_REPORT_NOTCHES` を満たす。
- [ ] TS5 (FR4/FR5/FR8): 複製ヘルパーの退化ケース — 要求回数 0 で空バッファを返し、
      要求回数 1 でペイロードとバイト単位で同一のバッファを返す。
- [ ] TS6 (FR5/FR8): 複製ヘルパーの上限前後 — 要求回数 100 でペイロードがちょうど 100 個
      連結され、要求回数 101 と `u32::MAX` はいずれもちょうど 100 個になる。
- [ ] TS7 (FR5/FR8/NFR1): 複製ヘルパーの長さ不変条件 — 代表的なペイロード（空ペイロードと
      現実的な SGR ホイールレポートを含む）と要求回数（0、1、99、100、101、`u32::MAX`）の
      組み合わせで、返されるバッファ長が `payload.len() * MAX_WHEEL_REPORT_NOTCHES` を
      超えない。
- [ ] TS8 (FR3): 既存の変換回帰ガード — `src-tauri/src/window_host/tests.rs:1498`、`:1505`、
      `:1511` の既存 3 テストが無修正で通り続ける。
- [ ] TS9 (NFR5): alternate scroll 経路が影響を受けない — 既存の alternate scroll クランプ
      テスト（tests.rs:2691-2697、`MAX_ALT_SCROLL_NOTCHES` に対してバイト長を主張する）が
      通り続け、alternate scroll の上限が再調整も共有もされていないことを確認する。

### Integration Tests

該当なし。本変更はモジュール内部の純粋関数とその呼び出し側に閉じており、検証は上記の
ユニットテストとビルドシナリオ TS10 で足りる。

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

- [ ] TS10 (NFR4): フィーチャーゲートのコンパイル — 変更後もデフォルトフィーチャのテスト
      ビルド（`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`）と
      `--no-default-features` の check ビルド
      （`CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`）の
      双方が成功する。

### Edge Cases

- [ ] 非有限入力（NaN、+INFINITY、-INFINITY）: 0 を返し、PTY へ何も書き込まない（TS2）。
- [ ] 極値の有限入力（`f32::MAX`、`f32::MIN`）: `±MAX_WHEEL_REPORT_NOTCHES` にクランプされる（TS2）。
- [ ] サブノッチ入力（±0.999）とゼロ（+0.0、-0.0）: 0 を返す（TS3）。
- [ ] 要求複製回数 `u32::MAX`: 100 個に制限される（TS6）。
- [ ] 空ペイロード: 長さ不変条件を満たす（TS7）。

### Performance Tests

- [ ] TS7 を最悪アロケーションの検証として用いる。1 ホイールイベントあたりのバッファ長が
      `payload.len() * MAX_WHEEL_REPORT_NOTCHES` を超えないこと（NFR1）。
- [ ] TS4 を PTY 書き込み量の上限の裏付けとして用いる。ノッチ数の絶対値が
      `MAX_WHEEL_REPORT_NOTCHES` を超えないこと（NFR2）。

## Security Considerations

- **Authentication:** 該当なし。認証を伴う経路ではない。
- **Authorization:** 該当なし。
- **Input Validation (SEC1):** クランプは f32 の大きさに対して `as i32` キャストの**前**に
  適用する。Rust の浮動小数点から整数への `as` キャストは飽和するため、大きい有限の f32 は
  `i32::MAX` になる。キャスト後にのみクランプすると `i32::MAX` という値が将来の他の
  呼び出し元から観測可能なまま残り、本修正が掲げる契約が成立しなくなる。
- **Input Validation (SEC2):** ホイールレポート経路の `Vec::with_capacity` の引数を、
  検証されていないノッチ数から計算してはならない。容量の式とループ上限はいずれも
  クランプ後の値から導出し、両者が乖離しないようにする。
- **Data Protection (SEC3):** この上限は UX のチューニングつまみではなくセキュリティ特性
  である。将来 alternate scroll 経路の操作感が再調整されても暗黙に緩まないよう、独立した
  定数として表現する。
- **Attack surface:** ローカルからのみ到達可能であり、マウストラッキング対応 TUI がすでに
  動作しているホスト上でのホイール入力を必要とする。リモートまたはクロスユーザーの攻撃経路は
  対象外である。
- **XSS Prevention:** 該当なし。WebView 経路を含まない。
- **SQL Injection Prevention:** 該当なし。データベースを扱わない。
- **CSRF Protection:** 該当なし。

## Error Handling

### Error Codes

該当なし。本機能はエラーコードを追加しない。範囲外の入力はエラーではなく、クランプまたは
0 として正常に処理される。

| 入力条件 | 処理 | 結果 |
|----------|------|------|
| 非有限（NaN、±INFINITY） | `wheel_report_notches` が 0 を返す | PTY への書き込みなし |
| `\|lines\| < 1.0` | ゼロ方向へ切り捨て | PTY への書き込みなし |
| 大きさが `MAX_WHEEL_REPORT_NOTCHES` 超 | キャスト前に飽和 | 100 ノッチ相当を書き込み |
| 要求複製回数が `MAX_WHEEL_REPORT_NOTCHES` 超 | ヘルパーが再クランプ | 100 個の複製に制限 |

### Error Flow

```
範囲外の入力 → クランプまたは 0 化 → 正常経路を継続（例外もエラー返却も発生しない）
```

## Performance Optimization

### Performance Goals

- 1 ホイールイベントあたりのアロケーション: `payload.len() * MAX_WHEEL_REPORT_NOTCHES` 以下
  （キロバイト規模、現状の約 21 GB に対して）。
- 1 ホイールイベントあたりの PTY 書き込み: `MAX_WHEEL_REPORT_NOTCHES` ノッチ相当以下。
- 通常のホイール入力: 現行実装とバイト単位で同一の出力（追加コストは上限比較のみ）。

### Optimization Strategies

- 上限クランプ: 容量計算とループ回数の双方をクランプ後の値から導出し、過大な
  `Vec::with_capacity` を構造的に不可能にする。
- 書き込み粒度の維持: ホイールイベント 1 回につき `tab.write_input` は 1 回のままとし、
  ノッチごとの書き込みには分割しない。

### Caching Strategy

該当なし。

## Success Criteria

- [ ] AC1: 任意の f32 入力に対し `wheel_report_notches` の戻り値の絶対値が
      `MAX_WHEEL_REPORT_NOTCHES` 以下で、`i32::MAX` への飽和が到達不能。
- [ ] AC2: 複製ヘルパーが `payload_len * MAX_WHEEL_REPORT_NOTCHES` バイトを超えない。
- [ ] AC3: `MAX_WHEEL_REPORT_NOTCHES` が独立定数として 100 で存在し、
      `MAX_ALT_SCROLL_NOTCHES` は現在値のまま。
- [ ] AC4: 既存の 3 テストがアサーション無編集で通る。
- [ ] AC5: `f32::INFINITY` / `f32::NAN` / 巨大有限値のクランプテストが存在する。
- [ ] AC6: `--lib` テストと `--no-default-features` check の双方が成功する。
- [ ] AC7: ノッチ数 0 で書き込みなし、非ゼロでイベントあたり `tab.write_input` 1 回。
- [ ] FR1〜FR8 がすべて実装され、テストされている。
- [ ] TS1〜TS10 がすべて通る。
- [ ] NFR1〜NFR6 が満たされている。
- [ ] コードレビューが完了している。

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

未解決要件なし。FR1〜FR8 および NFR1〜NFR6 はすべて resolved であり、`status: tbd` の
要件は存在しない。

## Implementation Phases (if applicable)

### Phase 1: 変換側のクランプ
**Goals:** レポート経路専用の上限定数を導入し、キャスト前にクランプする。
**Deliverables:**
- FR1: `MAX_WHEEL_REPORT_NOTCHES = 100`（input_translate.rs）
- FR2 / FR3: クランプ済み `wheel_report_notches`（既存セマンティクス保持）
- FR7: 変換の境界値・病的入力・不変条件テスト（TS1〜TS4、TS8）

### Phase 2: 消費側のクランプ
**Goals:** 複製処理を純粋関数へ切り出し、第 2 のクランプ層を置いて呼び出し側を付け替える。
**Deliverables:**
- FR4: 複製ヘルパーの抽出（pointer_routing.rs）
- FR5: ヘルパー内での再クランプ（`with_capacity` とループ上限の双方）
- FR6: ホイールレポート分岐のヘルパー経由化
- FR8: 複製ヘルパーのユニットテスト（TS5〜TS7）

## References

- 要件定義書: `feature-docs/wheel-report-notch-clamp/REQUIREMENTS.md`
- `src-tauri/src/window_host/input_translate.rs`: `MAX_ALT_SCROLL_NOTCHES`（:332）、
  `alternate_scroll_wheel_bytes`（:361-384、上限は :373）、`wheel_report_notches`（:452）
- `src-tauri/src/window_host/pointer_routing.rs`: import（:17）、呼び出し（:914）、
  ホイールレポート分岐（:913-924）、インラインの複製処理（:917-921）
- `src-tauri/src/window_host/tests.rs`: import（:14）、既存の変換テスト（:1498-1513）、
  alternate scroll クランプテスト（:2691-2697）
- test/README.md: テストスタイル・命名規約（NFR6）
- レビュー所見 7063d53b08e160d8（round 3、round 2 の ef02afd34f1da70f から持ち越し）
- PR #69: マウスレポート経路の導入元
