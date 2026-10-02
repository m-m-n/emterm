# Feature: mux-cut-csi-post-strip-closure

## Overview

mux デーモンの書き込みフィルタで、カット時の閉鎖（DEL）と次の呼び出しへ引き継ぐ CSI 状態を、strip 適用後の出力の状態で決める。あわせて、前身フィーチャー mux-suppressed-output-round4-fixes の review round 1 の medium 指摘 `4c0ad9058a983648` について、判断・理由・回帰テストを記録する。要件の詳細は [REQUIREMENTS.md](REQUIREMENTS.md) を参照する。

## Objectives

- カット時の閉鎖と次の呼び出しへ引き継ぐ CSI 状態を、除去後の出力の状態で決める。strip が開いた CSI を中断する構文を除去したときも、クライアントが開始していない問い合わせ（CPR など）がリプレイで成立しないようにする。
- review round 1 の medium 指摘 `4c0ad9058a983648` について、対応済みか・対応不要かを判断し、理由と回帰テストを記録する。

## User Stories

該当なし。受け入れ基準は「Success Criteria」に記載する。

## Technical Requirements

### Functional Requirements

- **FR1:** カット閉鎖を除去後の出力で判定。カットの直前で出力したバイト列（strip 適用後）が未完の CSI で終わるとき、書き込みフィルタはその後に CSI_CLOSING（DEL）を 1 回書く。strip が除去した構文の ESC は書かれていないので、CSI を中断したとは扱わない。`ESC[6` + `ESC]777;emterm;markdown;…BEL`、`ESC[6` + `ESC_G…ESC\`、`ESC[6` + `ESC[6n` の直後のカットでは、出力は `ESC[6` + DEL になる。除去対象の後に、カットで落とす未完の構文が続く場合（例: `ESC[6` + 完全な起動列 + `ESC]0;t` + カット）も同じ。除去されない ESC 系の構文（`ESC]0;t BEL`、`ESC X` など）が CSI を中断した場合は、従来どおり DEL を書かない。
- **FR2:** 引き継ぐ CSI 状態を除去後の出力で決める。カットの無い呼び出しの後に引き継ぐ CSI 状態は、strip 適用後に書かれたバイト列の末尾の状態にする。除去対象の構文が別の呼び出しで完成し、保持していた run が strip で空になった場合も、前の呼び出しから開いていた CSI は開いたままとして引き継ぐ。後に続くフォールバック閉鎖（空範囲 + fed 0 のカット）や次のカットは、この状態で DEL を書く。
- **FR3:** オーバーフロー経路も同じ規則にする。512 KiB を超えたときの早期フラッシュでも、引き継ぐ状態（最後のセグメント）とカット前の閉鎖（カットが続くセグメント）を、strip 適用後の出力の状態で決める。
- **FR4:** 回帰テストの追加。`src-tauri/src/mux/ipc/pty_spawn/tests/round4_cut_csi.rs` の `CLOSING_CASES` に 3 形式（`ESC[6 ESC]777;emterm;markdown;…BEL` / `ESC[6 ESC_G…ESC\` / `ESC[6 ESC[6n`）を、kept=`ESC[6`、inside=true として追加する。term_core 生ストリーム比較にも、それぞれにカットと後続の `n` を続けたケースを、同一呼び出しのカットとフォールバック閉鎖の両方で追加する。
- **FR5:** 判断の記録。stable_id `4c0ad9058a983648` について、判断（対応済み / 対応不要）、理由、回帰テストを、`feature-docs/mux-cut-csi-post-strip-closure/` 配下の決定記録に書く。前身の `feature-docs/mux-suppressed-output-round4-fixes/reviews/round1.yaml` と `DECISIONS.md` は変更しない。
- **FR6:** カットの無い strip 連結は残件として記録。共有の strip（`strip_rich_content_and_remap_with_designator`。スナップショット経路を含む）の出力は変えない。カットの無い strip 連結は修正せず、決定記録に残件として書いて別タスクで扱う。対象は次の 3 つ: (1) `ESC[6` + 起動列 + `n` を 1 回の呼び出しで渡すと `ESC[6n` として書かれる、(2) リングが除去済み構文の直後の `ESC[6` で終わったままスナップショットが取られ、次のライブの `n` が CPR を成立させうる、(3) 呼び出しをまたいで書かれた照会をスナップショット時の strip だけが除去する。決定記録には、発生条件と、この修正の範囲外である理由を書く。

### Non-Functional Requirements

- **NFR1 - 互換性:** mux_ipc のワイヤ形式、Snapshot / SnapshotRestore のフレーム形状、スナップショットのバイト配置、共有 strip の出力は変えない。リングの内容が変わるのは、FR1-FR3 の場合に DEL が 1 バイト加わることだけとする。
- **NFR2 - リーダー通常経路の負荷:** リーダーの通常経路にバイト走査のパスを追加しない。除去後の CSI 状態は、既存の strip のパスか scan_boundary のパスの中で O(1) の状態として求める。
- **NFR3 - セキュリティ（TM-1）:** カットとその状態引き継ぎの経路で、クライアントが開始していない問い合わせを、リングのリプレイで成立させない。
- **NFR4 - セキュリティ（TM-2）:** 走査と strip は有界で、panic しない。512 KiB の pending 上限と、strip を通すオーバーフローフラッシュは維持する。
- **NFR5 - ビルドとプラットフォーム:** `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が通る。Linux と Windows で挙動は同じ。

## Implementation Approach

### Architecture

変更対象は mux デーモンの書き込みフィルタ。UI には触れない。

判定の規則:

| 判定 | 基準 | 要件 |
|------|------|------|
| カット直前の閉鎖（DEL を 1 回書くか） | strip 適用後に出力したバイト列が未完の CSI で終わるか | FR1 |
| カットの無い呼び出しの後に引き継ぐ CSI 状態 | strip 適用後に書かれたバイト列の末尾の状態 | FR2 |
| オーバーフローフラッシュ時の引き継ぎ状態と閉鎖 | 各セグメントの strip 適用後の出力の状態 | FR3 |

除去後の CSI 状態は、既存の strip のパスか scan_boundary のパスの中で O(1) の状態として求める（NFR2）。

### Data Flow

- 除去対象の構文が別の呼び出しで完成し、保持していた run が strip で空になった場合、前の呼び出しから開いていた CSI は開いたままとして引き継ぐ（FR2、EC-1）。
- 後に続くフォールバック閉鎖（空範囲 + fed 0 のカット）や次のカットは、引き継いだ状態で DEL を書く（FR2）。
- 除去されない ESC 系の構文（`ESC]0;t BEL`、`ESC X` など）が CSI を中断した場合は、従来どおり DEL を書かない（FR1、EC-4）。

### API Design

該当なし。mux_ipc のワイヤ形式、Snapshot / SnapshotRestore のフレーム形状は変えない（NFR1）。

### Database Schema

該当なし。

### Dependencies

**Internal Dependencies:**
- 共有の strip（`strip_rich_content_and_remap_with_designator`）: 出力は変えない（FR6、NFR1）。
- 前身フィーチャー mux-suppressed-output-round4-fixes: ロック順、mux_ipc のワイヤ形式、512 KiB の pending 上限、閉鎖バイトとしての DEL（CSI_CLOSING）、指定子 ESC と閉鎖が同時に書かれないこと、リーダーの通常経路にバイト走査のパスを追加しないこと、の不変条件を引き継ぐ（as-01）。
- term_core: 生ストリーム比較とリプレイ検証の基準に使う（FR4、AC-1、TS-2）。

**External Dependencies:**
- 記載なし。

### File Structure

```
src-tauri/src/mux/ipc/pty_spawn/tests/round4_cut_csi.rs   # CLOSING_CASES と term_core 生ストリーム比較へのケース追加（FR4）
feature-docs/mux-cut-csi-post-strip-closure/               # 決定記録（FR5、FR6）
```

実装の変更ファイルは create-plan で `workflow.yaml` の各タスクの `files` から導出する。

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/mux-cut-csi-post-strip-closure/**`
- `test-docs/mux-cut-csi-post-strip-closure/**`

`feature-docs/mux-cut-csi-post-strip-closure/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/mux-cut-csi-post-strip-closure/**` covers `test-docs/mux-cut-csi-post-strip-closure/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/mux-cut-csi-post-strip-closure/` directory at all; the declared
`test-docs/mux-cut-csi-post-strip-closure/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests
- [ ] TS-1（FR1, FR2, FR4）: `CLOSING_CASES` に 3 形式を追加し、末尾のカット・3 回のカット・1 バイトずつ・フォールバック閉鎖のすべてで、出力が `ESC[6` + DEL になり、状態がクリアされることを確認する。
- [ ] TS-2（FR1, FR2, FR4, NFR3）: term_core 生ストリーム比較で、3 形式 + 画面切り替えの組 + 後続（`n` ほか）の場合を検証する。同一呼び出しのカットとフォールバック閉鎖の両方で、後続の解釈と応答が基準と等しいことを確認する。strip が設計上除去する構文そのものの効果は、比較から切り離す。
- [ ] TS-3（FR2）: 除去対象を含む入力で、すべての分割位置と 1 バイトずつの供給について、出力と状態（pending、指定子待ち、csi）が 1 回で渡した結果と等しいことを確認する。
- [ ] TS-4（FR3）: 上限付近まで保持した OSC がフラッシュされ、その run が `ESC[6` + 完全な除去対象で終わり、カットが続く場合を検証する。DEL が書かれ、後続の `n` で CPR が成立しないことを確認する。
- [ ] TS-6（FR5, FR6）: 決定記録に `4c0ad9058a983648` の行と FR6 の残件があることを確認する。

### Integration Tests
- [ ] TS-5（FR1, FR2, NFR3）: 本番のリーダーと可視性復元のハーネス（`run_visibility_restore_at`）で、`ESC[6` + 除去対象 + 画面切り替え、その後の読み取りで `n`、という順にデータを流す。クライアントの応答・画面・カーソルが生ストリームの基準と等しいことを確認する。

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Existing E2E tests pass without regression

### Edge Cases
- [ ] EC-1: 除去対象の構文が別の呼び出しで完成する場合（`ESC[6` を呼び出し 1、`ESC]777…` を pending に保持、`BEL` を呼び出し 2）。FR2 の状態は開いた CSI のまま。
- [ ] EC-2: 除去対象の後に、カットで落とす未完の構文が続く場合（`ESC[6` + 完全な起動列 + `ESC]0;t` + カット）。DEL が書かれる。
- [ ] EC-3: 除去された CSI 照会に C0 が埋め込まれている場合（strip は C0 を再出力する）。CSI は開いたままと判定する。
- [ ] EC-4: 除去されない ESC 系の構文（`ESC]0;t BEL`、`ESC X` など）が CSI を中断した場合は、従来どおり DEL を書かない。
- [ ] EC-5: 生ストリームの基準との比較では、strip が除去する構文の効果（生ストリームでの `ESC[6n` への正当な CPR、Kitty APC の応答や画像配置）が基準側にだけ現れる。比較は後続バイトの解釈と応答に絞るか、その効果を出さないペイロードを選ぶ。
- [ ] EC-6: 前身フィーチャーの FR3 の、指定子 ESC と DEL が同時に書かれない不変条件を保つ。

### Performance Tests
- [ ] TS-7（NFR2, NFR4）: 除去対象と開いた CSI を交互に並べた長い入力でも、予算内に終わり panic しないことを確認する。

### Build Checks
- [ ] TS-8（NFR5）: `--no-default-features` の cargo check が通ることを確認する。

## Security Considerations

- **Authentication:** 該当なし。
- **Authorization:** 該当なし。
- **Input Validation:** 走査と strip は有界で、panic しない。512 KiB の pending 上限と、strip を通すオーバーフローフラッシュは維持する（NFR4）。
- **Data Protection:** カットとその状態引き継ぎの経路で、クライアントが開始していない問い合わせを、リングのリプレイで成立させない（NFR3）。
- **XSS Prevention:** 該当なし。
- **SQL Injection Prevention:** 該当なし。
- **CSRF Protection:** 該当なし。

## Error Handling

### Error Codes

該当なし。

### Error Flow

該当なし。走査と strip は panic しない（NFR4）。

## Performance Optimization

### Performance Goals
- リーダーの通常経路にバイト走査のパスを追加しない（NFR2）。
- 除去対象と開いた CSI を交互に並べた長い入力でも、予算内に終わる（TS-7）。

### Optimization Strategies
- 除去後の CSI 状態は、既存の strip のパスか scan_boundary のパスの中で O(1) の状態として求める（NFR2）。

### Caching Strategy
- 該当なし。

## Success Criteria

- [ ] AC-1（FR1, FR2, NFR3）: 3 形式それぞれで、カット（同一呼び出しのカット・フォールバック閉鎖）の後に `n` を続けたリングを term_core でリプレイしても CPR 応答は出ず、`n` は文字として表示される。
- [ ] AC-2（FR1, FR2, FR4）: `CLOSING_CASES` に追加した 3 ケースで、既存の経路（末尾のカット、同じ位置への 3 回のカット、1 バイトずつ、フォールバック閉鎖、2 回目の閉鎖が何も書かないこと）がすべて期待どおりになる。
- [ ] AC-3（FR2）: カットの無い呼び出しの後、`csi_phase()` は除去後の出力の状態を返す（`ESC[6` + 完全な起動列の後は `Some(Param)`）。分割位置を問わず、また 1 バイトずつ渡しても、出力バイトと状態は 1 回で渡した結果と等しい。
- [ ] AC-4（FR3）: オーバーフローフラッシュの後のカットでも、除去後の出力が開いた CSI で終われば DEL が 1 回書かれる。
- [ ] AC-5（FR4）: 修正前のコードで、追加した回帰テストが失敗し、修正後に通る。
- [ ] AC-6（FR5, FR6）: 決定記録に `4c0ad9058a983648` の判断・理由・回帰テストと、FR6 の残件（発生条件と範囲外の理由）がある。前身の `reviews/round1.yaml` と `DECISIONS.md` は変更されていない。
- [ ] AC-7（NFR1）: 既存テストは変更なしで通る。意図して期待値を変えたテストは決定記録に列挙する。改名があれば `.claude/rules/test-docs-records.md` に従って前身の test-docs を更新する。
- [ ] AC-8（NFR5）: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` と、`--no-default-features` の cargo check が通る。

## Assumptions

- as-01: 前身の不変条件を引き継ぐ。ロック順、mux_ipc のワイヤ形式、512 KiB の pending 上限、閉鎖バイトとしての DEL（CSI_CLOSING）、指定子 ESC と閉鎖が同時に書かれないこと、リーダーの通常経路にバイト走査のパスを追加しないこと。
- as-02: 決定記録は `feature-docs/mux-cut-csi-post-strip-closure/` 配下に置く。前身の `reviews/round1.yaml` と `DECISIONS.md` は変更しない。
- as-03: 既存テストの改名は想定しない。改名が生じた場合は、`.claude/rules/test-docs-records.md` に従って前身の test-docs を更新する。
- as-04: 修正範囲は cut-and-carry とする。共有 strip の出力は変えず、カットの無い連結は残件として記録する。

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし。

## Implementation Phases (if applicable)

該当なし。

## References

- 要件定義書: `feature-docs/mux-cut-csi-post-strip-closure/REQUIREMENTS.md`
- 前身フィーチャーのレビュー記録: `feature-docs/mux-suppressed-output-round4-fixes/reviews/round1.yaml`
- 前身フィーチャーの決定記録: `feature-docs/mux-suppressed-output-round4-fixes/DECISIONS.md`
- test-docs 記録のルール: `.claude/rules/test-docs-records.md`
- 回帰テストの追加先: `src-tauri/src/mux/ipc/pty_spawn/tests/round4_cut_csi.rs`
