# Feature: mux-alt-tracker-detection-read-boundary

## Overview

mux ペインの AltSpanTracker で、食い違い状態で複合 DECSET を検出した読み取りの扱いを変える。検出した後は同じ読み取りの以降の複合形も検出し、最後に検出した複合形より後で記録した引き継ぎ状態を次の読み取りに引き継ぐ。これにより、検出した読み取りでも、読み込み境界の位置によらず、mux ペインの OSC 7501 記録表と OSC 777 状態が、同じバイト列を一括で処理した通常タブと一致する。

要件の詳細は `feature-docs/mux-alt-tracker-detection-read-boundary/REQUIREMENTS.md` にある。

## Objectives

- mux ペインの AltSpanTracker が、食い違い状態で複合 DECSET を検出した読み取りでも、読み込み境界の位置によらず、同じバイト列を一括で処理した通常タブと同じ OSC 7501 記録表と OSC 777 状態を返す（前身 SPEC FR6）。
- mux-alt-tracker-ris-compound-decset の review round 1 で medium として残った指摘 2139988314e6d6ac（同じ読み取りの 2 つ目以降の複合形の取りこぼし）と 89b935844bd8bc3b（検出時の引き継ぎ状態の消去）を解消する。

## User Stories

該当なし。このフィーチャーの変更対象は、mux の読み込みループ内部にある状態機械（AltSpanTracker）だけであり、UI と画面は変わらない。受け入れ基準は次の節に記す。

## Acceptance Criteria

- [ ] AC1（FR1、FR4）: tracker の Rig で read(`ESC[?1047h`) の後に read(`ESC[?1;1049l ESC[?1;1049h ESC]133;A BEL`) を呼ぶと、空の範囲が返る。続く read(mark()) も空になる。逆向き（M2 の後に h 形、l 形の順の複合形と OSC 133;A）では、読み取り全体の範囲が返り、続く mark() はライブになる。
- [ ] AC2（FR2、FR4）: read(`ESC[?1047h ESC[?1;1047h ESC(`) の後の read(`ESC c ESC]133;A BEL`) は空の範囲を返す。分割せずに送った場合と同じ結果になる。M1 の後の `ESC[?1;1049l ESC[?10` | `47h` では ?1047h を適用し、client 側は代替画面になる。
- [ ] AC3（FR4、FR5）: K58 以降の行をデーモン側の drive_reads（seed S-W）に流す。対象は、M1 + l 形 + h 形、M2 + h 形 + l 形、複合形の後に ESC ( / ESC / ESC [?10 が続いて読み取りが切れるもの。各行が指定するすべての配送（1 か所で分割するすべての位置と、1 バイトずつの読み込み）で、OSC 7501 記録表と OSC 777 状態が、同じ行を通常タブで一括処理した結果と一致する。
- [ ] AC4（FR6）: 名前を変えた既存テストが新しい期待値で通る。task0001.tests.yaml の AC-4 は新しい名前を挙げていて、その名前は `cargo test --lib -- --list` の出力にテストとして現れる。Superseded のコメントが付き、red_reason は変わっていない。
- [ ] AC5（FR8、NFR2）: FR6 の 1 本を除く既存テスト（altsplit_*、SC-1 K1〜K57、altmismatch_*、dblesc_* など）が変更なしで通る。src-tauri と term_core の --lib スイートが通り、cargo check --no-default-features が成功する。

## Technical Requirements

### Functional Requirements

- **FR1:** 検出した読み取りの、後続の複合形 — 1 回の読み取りの中で複合形（ライブな ESC で始まる DEC private set / reset のうち、引数列に 47 / 1047 / 1049 を含み、ALT_SCREEN_TOGGLES のどの形にも当たらないもの）を検出した後は、同じ読み取りの以降の複合形を、client_alt と shadow_alt の食い違いの有無によらず検出し、client 側の画面を終端の向き（h は代替画面、l はメイン画面）にする。読み取りの中でまだ検出していないとき、一致状態で受けた複合形の扱い（前身 A2）は変えない。
- **FR2:** 検出した読み取りの引き継ぎ状態 — 検出した読み取りでは、走査が最後に検出した複合形より後で記録した引き継ぎ状態（切り替えの途中状態、指示子待ち、CSI の解析状態）を消さず、次の読み取りに引き継ぐ。例: `ESC [?1;1047h ESC (` で終わる読み取りの後、次の読み取りの `ESC c` は指示子スロットの ESC c として扱う。`ESC [?1;1049l ESC [?10` | `47h` では ?1047h を適用する。
- **FR3:** 検出した読み取りの、ほかの扱い — 検出した読み取りでは、次の扱いを変えない。読み取りの終わりに client 側の画面で判定する A3 のフォールバック範囲（メイン画面なら読み取り全体、代替画面なら空）を返すこと。shadow_alt に読み取り後のパーサー状態を採ること。adoption を 1 回数えること。読み取り後の照合を行わないこと。
- **FR4:** 通常タブとの一致 — 再現手順 1 と 2 のストリーム、および FR1 と FR2 の形を含むストリームについて、mux ペイン（実際の pty_reader_loop とデーモンの agent-status タスク）の OSC 7501 記録表と OSC 777 状態が、同じストリームを一括で処理した通常タブと一致する。この一致は、各行が指定するすべての配送（1 か所で分割するすべての位置と、1 バイトずつの読み込みを含む）で成り立つ。
- **FR5:** 回帰テスト — 次の 3 つを追加する。(1) トラッカーの単体テスト（altscreen_split_read.rs）: M1 / M2 の後に複合形を 2 つ続ける行と、複合形の後で読み取りを `ESC (`、`ESC`、`ESC [?10` で分割する行を、すべての分割位置と 1 バイトずつの読み込みで流し、期待するライブ区間になることを確かめる。(2) デーモン側の行（daemon/tests.rs、drive_reads、seed S-W）を、K58 以降の別の一覧として追加する。(3) (2) に対応する通常タブ側の行（tabs/tests/output_pipeline.rs、process_combined）を、K58 以降の別の一覧として追加する。(2) と (3) には、それぞれの一覧のすべての行を流すテストを付ける。K38〜K57 の一覧と、それを固定するテストは変えない。
- **FR6:** 既存テストの名前と期待値の更新 — altmismatch_ac4_a_detection_read_applies_later_switches_and_clears_the_carry を、新しい挙動を表す名前（例: altmismatch_ac4_a_detection_read_applies_later_switches_and_keeps_the_later_carry）に変える。期待値は FR2 に合わせて更新する。3 つ目のケースでは、2 回目の読み取りの後の partial_len を 5、3 回目の読み取り `49l`+mark の範囲を [3..11] にする。4 つ目のケースでは partial_len を 1 にする。.claude/rules/test-docs-records.md に従い、test-docs/mux-alt-tracker-ris-compound-decset/task0001.tests.yaml の AC-4 の tests にあるこのテスト名を、新しい名前に更新する。あわせて、AC-4 のブロックに、このフィーチャーの SPEC.md と FR2 / FR6 を挙げる Superseded の YAML コメントを付ける。red_reason は変えない。
- **FR7:** 前身 SPEC FR4/FR5 の上書き — このフィーチャーの SPEC で、次の上書きを宣言する（「前身 SPEC の上書き」の節）。検出した読み取りでは、前身 SPEC（feature-docs/mux-alt-tracker-ris-compound-decset/SPEC.md）の FR4/FR5 と引き継ぎ状態表にある「adopt のときに引き継ぎ状態を消す」を、複合形より前の状態に限る。前身の SPEC.md と REQUIREMENTS.md は書かれたまま残す。write_filter.rs の AltSpanTracker の doc コメント（1615-1616 行、1674-1677 行）と scan() の doc コメントは、FR1 と FR2 の挙動に揃える。
- **FR8:** 変わらない挙動 — 次は変えない。scrollback の書き込み経路（extract_main_buffer、to_write、cuts、ScrollbackWriteFilter）。AgentStatusFeedScanner（step_ris、cut_live / tail_live を含む）。term_core。ShadowParser。AltSpanTracker::advance のシグネチャと mod.rs の呼び出し側。一致状態で受けた未対応の形のフォールバック（AC-6a）。検出しなかった読み取りの扱い。FR6 の 1 本を除く既存テストは、すべて変更なしで通る。

### Non-Functional Requirements

- **NFR1 - 堅牢性:** どの入力でも panic しない。引き継ぐ状態は O(1) のまま（切り替えの途中状態は最大 7 バイトで、それに CSI の解析状態を加える）とする。
- **NFR2 - フィーチャーゲート:** 変更は、--no-default-features でビルドされる mux のコードの中に収める。CLI のみのビルドもコンパイルできる。
- **NFR3 - ログ:** 新しいログ出力を追加しない。
- **NFR4 - プラットフォーム:** Linux と Windows。
- **NFR5 - term_core 不変:** term_core（crates/term_core）は基準となる挙動として扱い、変更しない。

## Implementation Approach

### Architecture

**変更対象:**
- AltSpanTracker の走査（scan()）で、複合形を検出した読み取りの扱い（FR1、FR2）
- write_filter.rs の AltSpanTracker の doc コメント（1615-1616 行、1674-1677 行）と scan() の doc コメント（FR7）
- 既存テスト 1 本の名前と期待値（FR6）
- test-docs/mux-alt-tracker-ris-compound-decset/task0001.tests.yaml の AC-4 の記録（FR6）

**変更しないもの（FR3、FR8、NFR5）:**
- scrollback の書き込み経路（extract_main_buffer、to_write、cuts、ScrollbackWriteFilter）
- AgentStatusFeedScanner（step_ris、cut_live / tail_live を含む）
- term_core（crates/term_core）
- ShadowParser
- AltSpanTracker::advance のシグネチャと mod.rs の呼び出し側
- 一致状態で受けた未対応の形のフォールバック（AC-6a）
- 検出しなかった読み取りの扱い
- 検出した読み取りの、A3 のフォールバック範囲、shadow_alt に読み取り後のパーサー状態を採ること、adoption を 1 回数えること、読み取り後の照合を行わないこと（FR3）

### 検出した読み取りでの扱い

| 入力の形 | 条件 | 扱い | 要件 |
|----------|------|------|------|
| 読み取りの最初の複合形 | 食い違い状態 | 検出する（前身 FR4 / FR5） | — |
| 読み取りの最初の複合形 | 一致状態（読み取りの中でまだ検出していない） | 検出しない（前身 A2 のまま） | FR1、A2 |
| 検出した後の、同じ読み取りの複合形 | 食い違いの有無によらない | 検出し、client 側の画面を終端の向き（h は代替画面、l はメイン画面）にする | FR1 |
| 検出した後の、完全な切り替えパターン（E(1049) / L(1049)） | 同じ読み取り | 挙動は変わらない | FR8 |
| 読み取りの終わり | 検出した読み取り | client 側の画面で判定する A3 のフォールバック範囲（メイン画面なら読み取り全体、代替画面なら空）を返す。shadow_alt に読み取り後のパーサー状態を採る。adoption を 1 回数える。読み取り後の照合を行わない | FR3 |

### 前身 SPEC の上書き

このフィーチャーの SPEC は、次の上書きを宣言する（FR7、A3）。

- 対象: 前身 SPEC（feature-docs/mux-alt-tracker-ris-compound-decset/SPEC.md）の FR4/FR5 と引き継ぎ状態表にある「adopt のときに引き継ぎ状態を消す」
- 上書き後: 検出した読み取りでは、消すのは複合形より前の状態に限る。最後に検出した複合形より後で記録した引き継ぎ状態は、消さずに次の読み取りに引き継ぐ（FR2）。
- 前身の SPEC.md と REQUIREMENTS.md は書かれたまま残す。

### 検出した読み取りの引き継ぎ状態

| 項目 | 複合形より前で記録した状態 | 最後に検出した複合形より後で記録した状態 |
|------|----------------------------|------------------------------------------|
| 切り替えの途中状態 | 消す | 次の読み取りに引き継ぐ |
| 指示子待ち | 消す | 次の読み取りに引き継ぐ |
| CSI の解析状態 | 消す | 次の読み取りに引き継ぐ |

検出した読み取りの終わりに残る carry は、必ず最後に検出した複合形より後で記録されたものである（A1）。

引き継ぎの例（FR2）:
- `ESC [?1;1047h ESC (` で終わる読み取りの後、次の読み取りの `ESC c` は指示子スロットの ESC c として扱う。
- `ESC [?1;1049l ESC [?10` | `47h` では ?1047h を適用する。

引き継ぐ状態の大きさは O(1) のままとする（NFR1）。

### API Design

該当なし。AltSpanTracker::advance のシグネチャは変えない（FR8）。

### Database Schema

該当なし。

### Dependencies

**Internal Dependencies:**
- term_core（crates/term_core）: 基準となる挙動。変更しない（NFR5）。
- ShadowParser: shadow_alt の基準。変更しない（FR8）。
- 前身フィーチャー mux-alt-tracker-ris-compound-decset: 複合形の検出（前身 FR4 / FR5）と引き継ぎ状態。このフィーチャーで一部を上書きする（FR7）。

**External Dependencies:**
- 該当なし。

### File Structure

テストの追加先と更新先（FR5、FR6）:

```
altscreen_split_read.rs          # トラッカーの単体テスト（TS1、TS2）
daemon/tests.rs                  # デーモン側の行（K58 以降の別の一覧、TS3）
tabs/tests/output_pipeline.rs    # 対応する通常タブ側の行（K58 以降の別の一覧、TS4）
test-docs/mux-alt-tracker-ris-compound-decset/task0001.tests.yaml   # AC-4 のテスト名と Superseded のコメント（FR6）
```

doc コメントの更新先（FR7）:

```
write_filter.rs                  # AltSpanTracker の doc コメント（1615-1616 行、1674-1677 行）と scan() の doc コメント
```

## Declared Change Set

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

各 SPEC は、上のフィーチャー固有のパスに加えて、次の 2 つのワークフロー生成エントリーをデフォルトで宣言する。

- `feature-docs/mux-alt-tracker-detection-read-boundary/**`
- `test-docs/mux-alt-tracker-detection-read-boundary/**`

`feature-docs/mux-alt-tracker-detection-read-boundary/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` であり、この節は引用のみでルールは再掲しない。

`test-docs/mux-alt-tracker-detection-read-boundary/**` に含まれるもの: `test-docs/mux-alt-tracker-detection-read-boundary/{T}.tests.yaml`（タスクごとのテスト記録）。生成主体は `implement-phase.md` であり、この節は引用のみでルールは再掲しない。

この 2 つのデフォルトエントリーは、SPEC 作成者が明示的に除外しない限り宣言に含まれる。記載が無いことを除外とは見なさない。除外は意図的で明示的な絞り込みとする。

この宣言はスーパーセット（SUPERSET）の主張である。検証時に観測される実際の変更集合は、宣言された集合と等しい必要はなく、宣言に含まれ（CONTAINED IN）ていればよい。implement タスクを 1 つも生成しないフィーチャーは `test-docs/{feature}/` ディレクトリを生成しないが、宣言された `test-docs/{feature}/**` はその場合も正しい。宣言されたパスが生成されなくても違反ではない。

## Test Scenarios

### Unit Tests

- [ ] TS1（FR1、FR2、FR3、AC1、AC2）: altscreen_split_read.rs で、M1 / M2 の後に複合形を 2 つ続けるストリームと、複合形の後で読み取りが ESC ( / ESC / ESC [?10 / ESC [?1;10 / ESC ( ESC [?10 で切れるストリームを、すべての分割位置と 1 バイトずつの読み込みで流す - 各読み取りのライブ区間が期待値と一致する。オラクルには画面の上書き点を 2 か所以上渡せるようにする（client_walk を拡張するか、別の関数を足す）。adoption の回数は配送ごとに期待値を定める。
- [ ] TS2（FR6、AC4）: 名前を変えた既存テストの 4 つのケース（C1l E(1049)、C1h L(1049)、C1l ESC[?10 | 49l、C1l ESC）を流す - 新しい期待値で通る。

### Integration Tests

- [ ] TS3（FR4、FR5、AC3）: daemon/tests.rs に K58 以降の行を別の一覧として追加し、各行をその行が指定するすべての配送で drive_reads（seed S-W）に流す - 記録表と OSC 777 状態が期待値と一致する。
- [ ] TS4（FR4、FR5）: tabs/tests/output_pipeline.rs で、同じ行を別の一覧として seeded plain tab / process_combined で一括処理する - 基準となる記録表と OSC 777 状態が得られる。

### Regression Tests

- [ ] TS5（FR8、NFR2、AC5）: src-tauri と term_core の --lib スイート全体と、--no-default-features でのチェックを実行する - すべて通る。

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

- [ ] Existing E2E tests pass without regression

### Edge Cases

- [ ] 最初の複合形で 2 つの画面が一致する並び: M1 + l 形（C1l、C7l）+ h 形（C1h、Crh、C7h、C0h、Ceh）と、M2 + h 形 + l 形。
- [ ] 1 回の読み取りに複合形が 3 つ以上交互に続く並び。
- [ ] 複合形の後に、完全な切り替えパターン（E(1049) / L(1049)）が同じ読み取りで続く並び（挙動は変わらない）。
- [ ] 検出した読み取りが、指示子待ち、単独の ESC、切り替えの途中状態、CSI の解析途中、指示子スロットの途中状態のどれかで終わる場合。
- [ ] 検出した読み取りが単独の ESC で終わり、次の読み取りが c で始まる場合は RIS として扱う（前身 FR2）。
- [ ] 複合形が 2 つ続くストリームでは、adoption の回数が配送によって変わる（同じ読み取りなら 1 回、2 つの複合形の間で分割すれば 2 回）。
- [ ] 読み取りの最初の複合形を一致状態で受けた場合は、従来どおり検出しない（前身 A2）。
- [ ] 同じ読み取りで、複合形より前にあるマークは正確に扱えない（前身 A3、許容済み）。

### Performance Tests

該当なし。

## Security Considerations

- **Input Validation:** どの入力でも panic しない（NFR1）。
- **Authentication / Authorization / Data Protection / XSS Prevention / SQL Injection Prevention / CSRF Protection:** 該当なし。

## Error Handling

### Error Codes

該当なし。

### Error Flow

- 新しいログ出力を追加しない（NFR3）。

## Performance Optimization

### Performance Goals

- 引き継ぐ状態は O(1) のままとする（切り替えの途中状態は最大 7 バイトで、それに CSI の解析状態を加える）（NFR1）。

### Optimization Strategies

該当なし。

### Caching Strategy

該当なし。

## Assumptions

- **A1:** 検出した読み取りの終わりに残る carry は、必ず最後に検出した複合形より後で記録されたものである（scan() は持ち越した carry を冒頭で消費し、carry を書き込むのはループを抜ける break の箇所だけである）。
- **A2:** 読み取りの最初の複合形を一致状態で受けた場合は検出しない（前身 A2 を維持する）。既存テスト altmismatch_ac4_a_compound_in_a_matched_state_keeps_todays_behavior が固定している。
- **A3:** 前身 SPEC の FR4/FR5 と引き継ぎ状態表は、このフィーチャーの SPEC で上書きを宣言する。前身の SPEC.md と REQUIREMENTS.md は書かれたまま残す。
- **A4:** 回帰行は K58 以降の別の一覧にする。K38〜K57 の一覧と、それを固定するテストは変えない。
- **A5:** 既存テスト altmismatch_ac4_a_detection_read_applies_later_switches_and_clears_the_carry は名前を変える。task0001.tests.yaml AC-4 の記録は新しい名前に更新し、Superseded のコメントを付ける。red_reason は変えない。
- **A6:** CSI 解析の C0（NUL・CAN・SUB）と `:` の扱い（同じレビューの low の指摘）は対象外とする。
- **A7:** デザインステップはスキップする（UI と画面の変更が無いため）。

## Success Criteria

- [ ] FR1〜FR8 がすべて実装され、テストされている
- [ ] AC1〜AC5 を満たす
- [ ] TS1〜TS5 がすべて通る
- [ ] FR6 の 1 本を除く既存のテストが変更なしで通る
- [ ] cargo check --no-default-features が成功する

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし。

## Implementation Phases (if applicable)

該当なし。タスクの分割は create-plan で決める。

## References

- 要件定義書: `feature-docs/mux-alt-tracker-detection-read-boundary/REQUIREMENTS.md`
- 前身 SPEC: `feature-docs/mux-alt-tracker-ris-compound-decset/SPEC.md`
- テスト記録のルール: `.claude/rules/test-docs-records.md`
- review round 1 の指摘: 2139988314e6d6ac（同じ読み取りの 2 つ目以降の複合形の取りこぼし）、89b935844bd8bc3b（検出時の引き継ぎ状態の消去）
