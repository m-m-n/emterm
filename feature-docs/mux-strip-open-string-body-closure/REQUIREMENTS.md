---
title: "mux-strip-open-string-body-closure"
created_date: 2026-10-04
status: draft
---

# mux-strip-open-string-body-closure - 要件定義書

## 1. 概要

### 1.1 背景

共有 strip（`strip_pass`）が、保持した未終了の OSC/DCS/APC 本文の中で構成要素を除去すると、その後ろのバイトが本文に結合する。生のストリームでは、term_core が除去対象の `ESC` で文字列を打ち切っていた。この結合によって、生のストリームが完成させなかった OSC 色照会（OSC 4/10/11/12 `?`）がリング上で成立する。

### 1.2 目的

- 保持した未終了の OSC/DCS/APC 本文の中で構成要素を除去したとき、後続バイトが本文に結合しないようにする。リングの再生は、生のストリームに対する term_core と同じ位置（除去対象の `ESC` で文字列を打ち切った位置）に立つ。
- 生のストリームが完成させなかった OSC 色照会（OSC 4/10/11/12 `?`）が成立しないようにする。スナップショット再生中も、OSC の先頭 + 除去対象で終わるリングに後から続くライブ入力に対しても成立しない。
- mux-strip-join-escape-closure の Residual 1 を解消し、変更した固定済み期待値をすべてこのフィーチャーの DECISIONS.md に記録する。

### 1.3 スコープ

- 対象: `src-tauri/src/mux/scrollback_filter.rs` の共有 strip（全エントリーポイント）、`src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` の `STRING_BODY_CLOSING` 再エクスポートとドキュメントコメント、remap、既存テストの期待値・改名、`test-docs/mux-write-filter-overflow-open-string-cut/task0001.tests.yaml` の AC-11、このフィーチャーの DECISIONS.md。
- 対象外: `crates/term_core` の変更。DCS/APC の ST 探索が入力中の次の ST まで及ぶ既存の挙動（A4）。先行フィーチャーの DECISIONS.md と `feature-docs/` の文章。

## 2. ビジネス要件

### 2.1 ビジネス目標

- 保持した未終了の文字列本文の中で除去が起きても、後続バイトが本文に結合しない。
- 生のストリームが完成させなかった OSC 色照会が、再生でもライブ継続でも成立しない。
- mux-strip-join-escape-closure の Residual 1 を解消し、変更した期待値を記録する。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| mux クライアント | リングのスナップショットを term_core で再生し、その後ライブ出力を受け取る |

### 2.3 期待される効果

- 再生側の端末が、生のストリームでは成立しなかった OSC 色照会に応答しない。
- strip 後のリングの再生結果（行・カーソル・応答）が、除去対象自身の効果を除いた生のストリームと一致する。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | 本文内の除去を含むリングのスナップショット再生 | mux クライアント | 高 |
| UC02 | 本文の先頭 + 除去対象で終わるリングに続くライブ入力 | mux クライアント | 高 |

### 3.2 ユースケース詳細

#### UC01: 本文内の除去を含むリングのスナップショット再生

**アクター**: mux クライアント

**事前条件**:
- PTY 出力が `OSC 色照会の先頭` + 除去対象 + `?BEL` で、cut 無しの 1 回の feed で `ScrollbackWriteFilter` に入る。

**基本フロー**:
1. 書き込み経路の strip が除去対象を除去し、その位置に `ESC CAN` を書く。
2. リングは 先頭 + `ESC CAN` + 再送出した C0 バイト + `?BEL` になる。
3. `build_snapshot_bytes` がリングをそのまま保持する。
4. クライアントがスナップショットを term_core で再生する。

**事後条件**:
- 応答が無い。
- 行とカーソルが、除去対象自身の効果を除いた生のストリームの参照（`view_after_a_cut`）と一致する。

#### UC02: 本文の先頭 + 除去対象で終わるリングに続くライブ入力

**アクター**: mux クライアント

**事前条件**:
- リングが OSC 色照会の先頭 + 除去対象で終わる。

**基本フロー**:
1. 主画面の復帰スナップショット（`build_resume_snapshot_bytes`、画面無し）がスクロールバックの後ろに何も足さずにスナップショットを作る。
2. クライアントがスナップショットを再生する。
3. ライブの `?BEL` が届く。

**事後条件**:
- 応答が無い。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | 除去位置での文字列本文の閉鎖 | 書き込み済み状態が OscBody / StBody のときの除去で `STRING_BODY_CLOSING` を先に書く | 高 |
| FR2 | `STRING_BODY_CLOSING` の単一定義 | `scrollback_filter.rs` に 1 回だけ定義し、`write_filter.rs` は再エクスポートする | 高 |
| FR3 | 全エントリーポイントと全開始状態 | FR1 を全 strip エントリーポイントと、同一パスで書いた本文・`state_in` で持ち込んだ本文に適用する | 高 |
| FR4 | remap が閉鎖を含む | 本文内で除去した構成要素の周辺の監視オフセットの対応付け | 高 |
| FR5 | cut の閉鎖は不変、二重閉鎖なし | 閉鎖後の cut とフォールバック閉鎖は何も書かない | 高 |
| FR6 | 閉鎖は両 strip と vt100 複写に対して不活性 | 閉鎖を strip 対象や ST として読まず、そのまま残す | 高 |
| FR7 | 完了の定義の回帰テスト | 色照会の不成立と生のストリームとの一致を検出するテスト | 高 |
| FR8 | 固定済み期待値と改名 | 名前が不正確になるテストだけを改名し、期待値を反転・移動する | 高 |
| FR9 | 決定記録 | このフィーチャーの DECISIONS.md に記録する | 高 |
| FR10 | ドキュメントコメント | 本文が開いたままになる旨の記述を本文の閉鎖の記述に改める | 中 |

### 4.2 機能詳細

#### FR1: 除去位置での文字列本文の閉鎖

**説明**: `strip_pass` が、書き込み済みストリームが `WrittenState::OscBody` または `WrittenState::StBody` にある間に構成要素をその開始 `ESC` ごと除去する場合、`Written::close_before_removal` を通して先に `STRING_BODY_CLOSING`（ESC + CAN、0x1B 0x18）を書く。

**ビジネスルール**:
- 対象は除去される構成要素 8 種すべて: BEL 終端の OSC 777 起動、ST 終端の OSC 777 起動、OSC 9999 emterm-md、agent-status 報告、Kitty APC、SIXEL DCS、応答済み CSI 照会、C0 バイトを含む CSI 照会。
- 閉鎖後の書き込み済み状態は Ground になる。同じ実行内の後続の除去は何も書かない。
- 閉鎖は、除去した CSI 照会から再送出する C0 バイトより前に書く。再送出した BEL は OSC を終端しない。
- 他の状態は現行どおり: CSI の中と書き込み済み `ESC` の直後では `CSI_CLOSING` を 1 つ書く。Ground と Designator では何も書かない。

#### FR2: `STRING_BODY_CLOSING` の単一定義

**説明**:
- `STRING_BODY_CLOSING` は `src-tauri/src/mux/scrollback_filter.rs` に `pub(in crate::mux)` で 1 回だけ定義する。値は変えない（`[0x1b, 0x18]`）。
- `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` は、`CSI_CLOSING` と同じく同名で再エクスポートする。
- `closure_for` と、この名前を使う既存テストはすべて同じ名前を使い続ける。
- 共有 strip モジュールは IPC 層に依存しない。そのため、そのドキュメントコメントはバイト値を記述し、`ESCAPE_CLOSING` へリンクしない。

#### FR3: 全エントリーポイントと全開始状態

**説明**: FR1 は全 strip エントリーポイントで成り立つ。

- `strip_replayable_rich_content`
- `strip_pty_output_for_scrollback_write`
- `strip_pty_output_for_scrollback_write_with_designator`
- `strip_rich_content_and_remap(_with_designator)`
- `strip_pty_output_for_scrollback_write_with_written_state`

本文は、同じパスの中で先に書かれたものでも、書き込みフィルターが `state_in` で持ち込んだものでもよい。持ち込みには、オーバーフローのフラッシュが残した開いた本文と、オーバーフロー後に保持したライブの単独 `ESC` に次の呼び出しで strip 対象が続く場合を含む。

#### FR4: remap が閉鎖を含む

**説明**: `strip_rich_content_and_remap` と `build_snapshot_bytes` のセグメント remap に適用する。本文内で除去した構成要素について:

- 構成要素の先頭バイトの監視オフセットは、閉鎖の前の出力位置に対応する。
- 構成要素の内側（先頭を除く）または直後のオフセットは、閉鎖と再送出した C0 バイトの後ろに対応する。
- remap 後のオフセットは非減少で、出力の範囲内に収まる。

#### FR5: cut の閉鎖は不変、二重閉鎖なし

**説明**: `closure_for` と cut の経路は変えない。strip が本文の閉鎖を書いた後の書き込み済み状態は Ground なので、次の 3 つはいずれも何も書かない。

- 同じ呼び出しの後の cut
- 次の呼び出しの fed 0 での cut
- 読み手のフォールバック閉鎖（fed 範囲が空で、0 に cut がある場合）

#### FR6: 閉鎖は両 strip と vt100 複写に対して不活性

**説明**:
- 書き込み経路の strip もスナップショット時の strip も、strip が書いた閉鎖を strip 対象の開始や ST として読まず、そのまま残す。
- 閉鎖の直後に書かれた strip 対象は引き続き除去される。そのとき状態は Ground なので閉鎖を追加しない。
- `vt100_replay_copy` は閉鎖をそのまま複写する。閉鎖は DEL を含まない。

#### FR7: 完了の定義の回帰テスト

**説明**:
- (a) OSC 色照会の先頭 `ESC]11;`、`ESC]10;`、`ESC]12;`、`ESC]4;1;` のそれぞれに、除去される構成要素の全種類と `?BEL` を続け、cut 無しの 1 回の呼び出しで `ScrollbackWriteFilter` に与える。リングは 先頭 + `ESC CAN` + 再送出した C0 バイト + `?BEL` になる。`build_snapshot_bytes` はそれをそのまま保持し、スナップショットのペイロードを term_core で再生しても応答は無い。行とカーソルは、構成要素自身の効果を除いた生のストリームの参照（`view_after_a_cut`）と一致する。
- (b) ライブ継続: 先頭 + 除去される構成要素からリングを作り、主画面の復帰スナップショット（`build_resume_snapshot_bytes`、画面無し、スクロールバックの後ろに何も足さない）を通して再生し、その後ライブの `?BEL` を与える。応答は無い。
- (c) 対照テストで、再生用の core が閉鎖の無い結合 `ESC]11;?BEL` に応答することを示す。
- (d) 打ち切りが何も応答しない DCS 本文と APC 本文（例: `ESC Px`、`ESC _x`）について、同等のケースで行・カーソル・応答を生のストリームの参照と比較する。

#### FR8: 固定済み期待値と改名

**説明**: 名前が不正確になるテストだけを改名する。Ground の行と完了した文字列の行は、閉鎖無しの期待値を保つ。本文の行は、新しい閉鎖テストへ移すか、期待値を反転する。

1. `mux::ipc::pty_spawn::tests::overflow_open_string_cut::overflow_open_string_cut_a_non_overflow_strip_then_cut_closes_the_open_osc_body` を改名する。後継名の案は `overflow_open_string_cut_a_non_overflow_strip_closes_the_osc_body_at_the_removal_and_the_cut_adds_nothing`。期待値は次のとおり反転する。

    | 項目 | 変更前 | 変更後 |
    |------|--------|--------|
    | 1 回目の呼び出しの書き込み | `ESC]0;x` | `ESC]0;x ESC CAN` |
    | 書き込み済み状態 | OscBody | Ground |
    | fed 0 での cut の書き込み | `ESC CAN` + テキスト | テキストのみ |
    | フォールバック閉鎖の書き込み | `ESC CAN` | なし |

2. `test-docs/mux-write-filter-overflow-open-string-cut/task0001.tests.yaml` の AC-11 の項目を後継名に更新する。AC-11 には `feature-docs/mux-strip-open-string-body-closure/SPEC.md` とその FR ID（FR1、FR5）を示す supersede の YAML コメントを付け、`red_reason` は変えない（`.claude/rules/test-docs-records.md`）。先行フィーチャーの他の test-docs 記録は変えない。そこに挙がる他のテストはすべて名前を保つ。
3. `scrollback_filter::tests::strip_concat_a_construct_removed_in_ground_adds_no_closing` は名前を保つ。その `ESC]0;t` 行は新しい本文閉鎖テストへ移し、期待値は `ESC]0;t ESC CAN` + c0 + `n`（変更前は `ESC]0;t` + c0 + `n`）。ドキュメントコメントから保持した文字列本文に関する文を除く。
4. `strip_concat_query::strip_concat_a_construct_removed_in_ground_or_a_kept_string_adds_no_closing` は名前を保つ。その `body_head` ループ（`ESC]0;t`、`ESC_Xnot-kitty`）は新しい閉鎖テストへ移し、期待値は body_head + `ESC CAN` + c0 + `n`（変更前は body_head + c0 + `n`）。ドキュメントコメントとそのループのインラインコメントを更新する。
5. `scrollback_filter::tests::post_strip_state_form_reports_the_csi_state_of_the_written_bytes` は名前を保つ。本文の行は次のとおり反転する（出力/状態）。

    | 開始状態 + 入力 | 変更前 | 変更後 |
    |-----------------|--------|--------|
    | OscBody + `ESC[6 BEL n` | `BEL` / Ground | `ESC CAN BEL` / Ground |
    | OscBody + `ESC[6 CR n` | `CR` / OscBody | `ESC CAN CR` / Ground |
    | StBody + `ESC[6 BEL n` | `BEL` / StBody | `ESC CAN BEL` / Ground |
    | Ground + `ESC]0;t ESC[6 BEL n` | `ESC]0;t BEL` / Ground | `ESC]0;t ESC CAN BEL` / Ground |
    | Ground + `ESC]0;t ESC[6n` | `ESC]0;t` / OscBody | `ESC]0;t ESC CAN` / Ground |
    | Ground + `ESC Px ESC[6n` | `ESC Px` / StBody | `ESC Px ESC CAN` / Ground |
    | Ground + `ESC Px ESC[6n ab` | `ESC Px ab` / StBody | `ESC Px ESC CAN ab` / Ground |
    | OscBody または StBody + 構成要素（各構成要素） | 空 / 本文 | `ESC CAN` / Ground |
    | `ESC]0;t` + 構成要素（各構成要素） | `ESC]0;t` / OscBody | `ESC]0;t ESC CAN` / Ground |
    | `ESC Px` + 構成要素（各構成要素） | `ESC Px` / StBody | `ESC Px ESC CAN` / Ground |

6. `scrollback_filter::tests::post_strip_state_form_output_equals_the_write_path_strip` は名前を保つ。OscBody/StBody の恒等式を「designator 形式と等しい」から「`bytes_entering_state(body)` + 入力 を Ground から strip した結果から、その先頭部分を除いたものと等しい」（または plan で検証する同等の恒等式）に変える。ドキュメントコメント（'From a carried string body neither closing is written'）を更新する。
7. FR1 によって期待値が変わる他の既存テストも同じ方法で扱う。

#### FR9: 決定記録

**説明**: このフィーチャーの DECISIONS.md に次の 4 点を記録する。

- 閉鎖バイトの決定（ESC + CAN、共有の `STRING_BODY_CLOSING`）
- FR8 で変わった各期待値の変更前と変更後の値
- 改名と test-docs 記録の更新
- mux-strip-join-escape-closure の Residual 1 の解消

先行フィーチャーの DECISIONS.md と `feature-docs/` の文章は編集しない。

#### FR10: ドキュメントコメント

**説明**: 除去が書き込み済みの本文を開いたままにする、または完了した文字列や本文の後では何も書かない、と述べるドキュメントコメントを、本文の閉鎖を述べる内容に更新する。

- `scrollback_filter.rs`:
    - `WrittenState` のドキュメント
    - `Written::close_before_removal`（'In ground, or after a complete string, nothing is written'）
    - `vt100_replay_copy` の箇条（'ground for [`WrittenState`]'）
    - `strip_replayable_rich_content` の段落（'In ground nothing is added'）
    - written-state 形式のドキュメント（'D1 writes nothing there and the body stays open'）
    - remap のドキュメント（'the closing byte D1 inserts'）
- `write_filter.rs`:
    - `written` フィールドのドキュメント（'such a construct does not end a written string body'、'by a strip that removes the construct whose `ESC` aborted a written string'）
    - `STRING_BODY_CLOSING` のドキュメント（'The write a cut makes'）
    - 'Closure at a cut' 段落（'so can the strip when it removes the construct whose `ESC` aborted a written string'）
    - 'Post-strip state' 段落
    - cut 分岐のコメント（'or one a removal left open'）

**制約**:
- `overflow_open_string_cut_the_doc_comments_state_the_open_body_model` が引き続き通る。必須の語句は残す: `scrollback_filter.rs` の 'OSC body' と 'DCS / APC body'、`write_filter.rs` の 'STRING_BODY_CLOSING'、'open string body'、'`ESC` + CAN'。古い語句は含めないままにする。

## 5. 非機能要件

### 5.1 パフォーマンス要件

- NFR2: strip は O(n) の単一パスで追加状態は O(1) のままとする。strip にも書き込みフィルターにも 2 つ目のパスを足さない。
- NFR3: 新しいテストは既存の 10 秒の予算内に終わる。

### 5.2 セキュリティ要件

- 入力検証: 生のストリームが完成させなかった OSC 色照会（OSC 4/10/11/12 `?`）が、スナップショット再生でもライブ継続でも成立しない（FR1、FR7）。

### 5.3 可用性要件

該当なし

### 5.4 保守性要件

- NFR1: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が通り、`CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が成功する。改名したテストが `cargo test ... --lib -- --list` で解決する。
- NFR3: `crates/term_core` は変えない。新しいテストは既存のオラクル規約に従う。term_core に除去対象自身の効果を除いた生のストリームを与え、行・カーソル・応答を比較する。適合する場合は `strip_concat_query`、`strip_join_escape_closure`、`round4_cut_csi`、`scrollback_filter::tests` のヘルパーを再利用する。
- ドキュメント: FR9、FR10。

### 5.5 互換性要件

- Csi / Escape 状態での除去（`CSI_CLOSING` を 1 つ）と Ground / Designator 状態での除去（何も書かない）は現行の挙動と期待値を保つ（A5）。

## 6. UI/UX要件

### 6.1 画面設計要件

該当なし（バイト列処理の修正で、UI や表示の変更は無い）

### 6.2 画面遷移

該当なし

### 6.3 レスポンシブ対応

該当なし

## 7. データ要件

### 7.1 データモデル概要

| 名前 | 内容 |
|------|------|
| `STRING_BODY_CLOSING` | `[0x1b, 0x18]`（ESC + CAN） |
| `WrittenState` | 書き込み済みストリームの状態。本文内の除去後は Ground |

### 7.2 データ項目

該当なし

### 7.3 データ保持期間

該当なし

## 8. 外部連携

### 8.1 連携システム

該当なし

### 8.2 API仕様要件

該当なし

## 9. 制約条件

### 9.1 技術的制約

- `crates/term_core` は変えない（NFR3）。
- strip は O(n) の単一パスで追加状態は O(1)（NFR2）。
- 前提:
    - A1: 本文内の除去位置で書く閉鎖は ESC + CAN で、既存の `STRING_BODY_CLOSING` の値とする。共有 strip に定義し、書き込みフィルターが再エクスポートする。
    - A2: 名前が不正確になるテストだけを改名する。先行フィーチャーの test-docs 項目は、改名したテストについてだけ supersede 注記付きで更新する。期待値の変更だけでは記録の更新義務は生じない（`.claude/rules/test-docs-records.md`）。
    - A3: term_core は OSC/DCS/APC 本文を、`\` が続かない任意の `ESC` で打ち切る。部分文字列を送出し（OSC は Unterminated として。`osc.rs` osc_escape、`dcs.rs` dcs_escape、`apc.rs` apc_escape）、次のバイトを Escape 状態から処理する。ESC + CAN はその送出を再現して ground に戻る。部分文字列の送出が引き起こすものは、生のストリームと再生とで同一になる。テストは打ち切りが何も応答しない本文の先頭を使う。
    - A4: 範囲外の既存挙動。共有の DCS/APC の ST 探索は入力中の次の ST まで及ぶため、strip 対象の形をした本文の先頭（開いた `ESC P q`、または `ESC _G`）は後の ST までをまたぐ。この修正はそれを変えない。
    - A5: Csi / Escape 状態での除去（`CSI_CLOSING` を 1 つ）と Ground / Designator 状態での除去（何も書かない）は、現行の挙動と期待値を保つ。
    - A6: strip の出力は入力より長くならない。除去される最小の構成要素（`ESC[c`）は 3 バイトで、閉鎖は 2 バイト + 構成要素自身の再送出 C0 バイトである。
    - A7: 再接続のスナップショットはスクロールバックの後ろに `ESC[?1049l` を足し、折り返したリングの経路はダンプブロックを足す（`snapshot_bytes.rs` `append_wrapped_dump_block_if_applicable`）。そのため、ライブ継続のシナリオは何も足さない主画面の復帰スナップショットを通してテストする。

### 9.2 ビジネス上の制約

- 先行フィーチャーの DECISIONS.md と `feature-docs/` の文章は編集しない（FR9）。

### 9.3 スケジュール制約

該当なし

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/mux-strip-open-string-body-closure/**`
- `test-docs/mux-strip-open-string-body-closure/**`

`feature-docs/mux-strip-open-string-body-closure/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/mux-strip-open-string-body-closure/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/mux-strip-open-string-body-closure/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/{feature}/` ディレクトリを生成しないが、宣言された `test-docs/{feature}/**` は依然として正しい。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| FR1 によって既存の固定済み期待値が変わる | 中 | FR8 のとおり改名・移動・反転し、FR9 で記録する |
| 閉鎖が後続の strip や vt100 複写に誤読される | 中 | FR6 のとおり不活性とし、TS-8 で確認する |
| cut や フォールバック閉鎖との二重閉鎖 | 中 | FR5 のとおり閉鎖後は Ground とし、TS-8 で確認する |

### 10.2 ビジネスリスク

該当なし

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC-1（FR1、FR7）: すべての OSC 色照会の先頭とすべての除去される構成要素の種類について、cut 無しの 1 回の呼び出しで与えた `先頭 + 構成要素 + ?BEL` は `先頭 + ESC CAN + c0 + ?BEL` を書く。スナップショット strip はそれを保持し、再生で応答は無く、行とカーソルは生のストリームの参照と一致する。
- [ ] AC-2（FR7）: OSC 色照会の先頭 + 除去される構成要素で終わるリングを主画面の復帰スナップショットを通して再生し、その後ライブの `?BEL` を与える。応答は無い。対照テストで、再生用の core が `ESC]11;?BEL` に応答することを示す。
- [ ] AC-3（FR1、FR3）: すべての strip エントリーポイントが、構成要素の位置で、再送出する C0 バイトより前に、ちょうど 1 つの閉鎖を書く。対象は OSC・DCS・APC 本文、1 つの本文の中の複数の構成要素、同じパスで書いた本文、`state_in` で持ち込んだ本文、オーバーフローのフラッシュが残した本文（保持した単独 `ESC` を含む）。報告される状態は Ground で、`client_written_state` と一致する。DCS/APC の再生は生のストリームの参照と一致する。
- [ ] AC-4（FR4）: 本文内で除去した構成要素の周辺の remap オフセットと `build_snapshot_bytes` のセグメントが FR4 のとおり対応する。
- [ ] AC-5（FR5、FR6）: strip が本文の閉鎖を書いた後、同じ呼び出しの cut、fed 0 での cut、フォールバック閉鎖はいずれも何も書かない。両 strip は閉鎖をそのまま残し、その後ろの対象は引き続き除去する。`vt100_replay_copy` は閉鎖を保持する。
- [ ] AC-6（FR2）: `STRING_BODY_CLOSING` は `scrollback_filter.rs` にだけ定義され、値は `[0x1b, 0x18]`。`write_filter.rs` は再エクスポートでこの名前を使い、`closure_for` もこれを使う。
- [ ] AC-7（FR8、FR9）: 改名したテストと更新した行が新しい期待値を保持する。`test-docs/mux-write-filter-overflow-open-string-cut/task0001.tests.yaml` の AC-11 は後継名を挙げ、supersede コメントを持ち、`red_reason` は変わらない。DECISIONS.md は変わった期待値すべてを変更前と変更後の値とともに挙げる。
- [ ] AC-8（FR10）: ドキュメントコメントの契約テストが通り、FR10 が挙げる古い語句がコメントに無い。
- [ ] AC-9（NFR1〜NFR3）: `--lib` テストと `--no-default-features` チェックが通り、`crates/term_core` は変わらず、新しいテストは 10 秒の予算内に収まる。

### 11.2 KPI

該当なし

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] TS-1（AC-1）: OSC 色照会の先頭 × 除去される構成要素 8 種の表。cut 無しで書き込みフィルターに 1 回 feed し、`build_snapshot_bytes`、ペイロードの term_core 再生と続ける。リングのバイト列、strip 後のスクロールバックがリングと等しいこと、応答が空であること、行とカーソルが `view_after_a_cut(head + construct, ?BEL)` と等しいことを確認する。
- [ ] TS-2（AC-2）: 先頭 + 構成要素のリング → `build_resume_snapshot_bytes(ring, &[], b"", false, DIMS)` → 再生 → ライブの `?BEL`: 応答なし。対照: `view_of(ESC]11;?BEL)` は応答する。
- [ ] TS-3（AC-3）: strip 単位の表。本文の先頭（`ESC]0;t`、`ESC]11;`、`ESC Px`、`ESC_Xnot-kitty`）× 対象 × 継続を、全エントリーポイントで確認する。閉鎖は 1 つで C0 バイトより前。状態は `client_written_state` で Ground。再生は参照と一致する。
- [ ] TS-4（AC-3）: 1 つの本文の中に全対象を連結すると閉鎖は 1 つで、C0 バイトがその後ろに順に続く。
- [ ] TS-5（AC-3）: 持ち込み状態。OscBody/StBody からの状態形式を各構成要素で確認する。書き込みフィルター: 開いた本文を残すオーバーフローのフラッシュの後、次の呼び出しで strip 対象を与える。保持した単独 `ESC` + 対象の継続を含む。
- [ ] TS-6（AC-3）: 先頭 + 文字列対象（保持される構成要素）を全位置で分割して読んでも、1 回の呼び出しと同じリングになる。CSI 照会の対象では、再生で応答が無く、参照と一致する。
- [ ] TS-7（AC-4）: 本文内で除去した構成要素の先頭バイト・内側・直後の監視オフセットを、`strip_rich_content_and_remap` と `build_snapshot_bytes` のセグメントで確認する。
- [ ] TS-8（AC-5）: strip が本文の閉鎖を書いた後の、呼び出し末尾の cut、fed 0 での cut、フォールバック閉鎖はいずれも何も書かない。両 strip とも `body + ESC CAN + target` から `body + ESC CAN` を得る。
- [ ] TS-9（AC-6、AC-7）: 改名した AC-11 のテストと FR8 の更新行。改名したテストを `--lib -- --list` で解決確認する。
- [ ] TS-10（AC-8）: ドキュメントコメントの契約テストと、FR10 の古い語句が無いことの確認。
- [ ] TS-11（AC-9）: `--lib` スイートと `--no-default-features` の cargo check。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| 共有 strip | `strip_pass`。書き込み経路とスナップショット時の両方で使う除去処理 |
| 本文 | 保持した未終了の OSC/DCS/APC 文字列の本文（`WrittenState::OscBody` / `WrittenState::StBody`） |
| 閉鎖 | 除去の前に書く `STRING_BODY_CLOSING`（ESC + CAN） |
| 除去される構成要素 | OSC 777 起動（BEL 終端・ST 終端）、OSC 9999 emterm-md、agent-status 報告、Kitty APC、SIXEL DCS、応答済み CSI 照会、C0 バイトを含む CSI 照会 |
| 生のストリームの参照 | 除去対象自身の効果を除いた生のストリームを term_core に与えた結果 |

## 14. 確認事項

### 14.1 確認済み事項

- [x] 閉鎖バイト: ESC + CAN（既存の `STRING_BODY_CLOSING`）
- [x] 固定済み期待値の扱い: 名前が不正確になるテストだけを改名し、先行フィーチャーの test-docs 記録は改名分だけ supersede 注記付きで更新する
- [x] デザインステップ: 省略する（UI や表示の変更が無いバイト列処理の修正）

### 14.2 未確認・保留事項

なし

## 15. 参考資料

- `.claude/rules/test-docs-records.md`
- `test-docs/mux-write-filter-overflow-open-string-cut/task0001.tests.yaml`
