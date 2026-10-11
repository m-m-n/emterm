# Feature: mux-alt-tracker-ris-compound-decset

## Overview

mux ペインの AltSpanTracker を、client_alt と shadow_alt が食い違った状態（?1047 の使用後、または指示子スロットでの切り替え後）でも、RIS と未対応の複合 DECSET による画面切り替えに追従させる。これにより、mux ペインの OSC 7501 記録表と OSC 777 状態が、同じバイト列を一括で処理した通常タブと一致する。この一致は、読み込み境界がどこにあっても成り立つ。

要件の詳細は `feature-docs/mux-alt-tracker-ris-compound-decset/REQUIREMENTS.md` にある。

## Objectives

- mux ペインの AltSpanTracker が、食い違い状態でも RIS と未対応の複合 DECSET による画面切り替えに追従する。mux ペインの OSC 7501 記録表と OSC 777 状態は、読み込み境界がどこにあっても、同じバイト列を一括で処理した通常タブと一致する。
- review round1 で medium として残った指摘 4aef749bee51a717（RIS）と b019b1bb35b07fc6（複合 DECSET）を解消する。

## User Stories

該当なし。このフィーチャーは、mux の読み込みループ内部にある状態機械（AltSpanTracker）のバグ修正であり、UI と画面は変わらない。受け入れ基準は次の節に記す。

## Acceptance Criteria

- [ ] AC1（FR1、FR6）: mux ペインに `\x1b[?1047h`、`\x1bc`、OSC 7501 の working 報告、`\x1b]133;A\x07` を順に与える。一括で与えた場合も、各位置で分割した場合も、記録表は通常タブと同じく空で終わる。食い違い状態を 1049 で作った場合（`\x1b[?1049h\x1b(\x1b[?1049l` の後に RIS）も同じ。
- [ ] AC2（FR4、FR6）: seed_root_working から始め、drive_reads で `\x1b(\x1b[?1049h` と `\x1b[?1;1049h\x1b]133;A\x07` を別々の読み込みとして与える。mux も通常タブと同じく、root の working を保つ。
- [ ] AC3（FR2）: RIS の ESC と c が読み込み境界で分割されても、AC1 と同じ記録表で終わる。ESC [ | c は RIS として扱われない。
- [ ] AC4（FR3）: 代替画面上で ESC ( ESC c を受けた後（一括の場合と、境界で分割した場合）、続く OSC 133;A はライブにならず、通常タブと一致する。この形では adopt が起きない。
- [ ] AC5（FR5、FR6）: 食い違い状態の両方向で、複合 DECSET（?1;1049h/l、?1049;1h、?1;1047h/l）とその後の OSC 133;A を流す。複合形の内部のすべての位置で分割した場合と、1 バイトずつ読み込んだ場合のどちらでも、mux の記録表が通常タブと一致する。
- [ ] AC6（NFR1）: 次の入力を、食い違い状態と一致状態の両方で流しても panic しない。返るライブ区間は整った形（昇順で重ならず、読み込みの範囲内）になる。
  - DEC private CSI のすべての途中位置の後に、256 通りのバイト値を続けた入力
  - 非常に長い引数（オーバーフローする数値）
  - 引数の個数が多い入力
- [ ] AC7（FR8、NFR2）: 既存のテストが変更なしで通る（altsplit_*、SC-1 K1〜K37、dblesc_* など）。src-tauri と term_core の --lib スイートが通り、cargo check --no-default-features が成功する。

## Technical Requirements

### Functional Requirements

- **FR1:** RIS への追従 — AltSpanTracker の走査では、指示子スロットの外にある ESC c を RIS として扱う。RIS を受けたら client_alt と shadow_alt を false にし、引き継ぎ状態（切り替えの途中状態、指示子待ち、FR5 の CSI 解析状態）を消して、c の直後からライブにする。メイン画面上の RIS ではライブ区間を途切れさせず、一括抽出したときの区間と一致させる。
- **FR2:** 読み込み境界で分割された RIS — 読み込みが単独のライブな ESC で終わり、次の読み込みが c で始まる場合も、FR1 と同じく RIS として扱う。引き継いだ途中状態が 2 バイト以上あるとき（例: ESC [）は、続く c を RIS にしない（CSI c になる）。途中状態を引き継いでいる最中に、次の読み込みが ESC c で始まる場合（ESC [?10 | ESC c）は、RIS として扱う。
- **FR3:** 指示子スロットにある ESC c — ESC ( または ESC ) の直後のスロットにある ESC c は、クライアント側の規則（term_core と step_ris の規則）では RIS にしない。1 回の読み込みの中でも、境界をまたぐ場合（ESC ( | ESC c、ESC ( ESC | c）でも同じとする。シャドウパーサーはどの ESC を受けても処理中のシーケンスを打ち切るため、シャドウ側の規則ではこの ESC c を RIS として扱い、shadow_alt を false にする。その結果、この形では adopt が起きない。
- **FR4:** 食い違い状態での未対応の画面切り替え形の検出 — client_alt と shadow_alt が食い違っている状態で、ライブな ESC で始まる DEC private mode set / reset（ESC [ ? 引数列 h または l）を受けたときの扱いを定める。引数列が数値として 47 / 1047 / 1049 を含み、ALT_SCREEN_TOGGLES のどの形にも当たらないとき、その読み込みには A3 の保守的なフォールバック（読み込みの前後ともメイン画面なら読み込み全体、それ以外は空）を返し、読み込み後のパーサー状態を adopt する。例: ESC [?1;1049h、ESC [?1049;1h、ESC [?1;1047h/l、ESC [?01049h。ESC が指示子スロットにある CSI はクライアントにとって文字列なので、FR4 の検出対象外とし、既存のパーサー照合に任せる。
- **FR5:** 読み込み境界で分割された FR4 の形の検出 — FR4 の形は、読み込み境界で分割されていても検出する（create-spec-q0001 requirement.compound-decset-split = carry_csi_state）。そのために、DEC private CSI を解析している途中の状態を O(1) で次の読み込みに引き継ぐ。引き継ぐのは、ESC [ ? を見たか、いま読んでいる引数の値（上限で飽和させる）、それまでの引数に 47 / 1047 / 1049 があったか、の 3 つ。終端の h / l を受けた読み込みで FR4 のフォールバックを返し、adopt する。切り替えの途中状態（ESC [?104 など）が ALT_SCREEN_TOGGLES の形から外れ、DEC private CSI として続く場合（ESC [?104 | 9;1h）は、保持しているバイトから CSI の解析を続ける。ESC（FR1〜FR3 の判定を含む）、CAN、SUB、引数でも終端でもないバイトが来たら、CSI の解析状態を検出なしで捨てる。adopt、RIS、パーサーの再構築のときも、ほかの引き継ぎ状態と一緒に消す。
- **FR6:** 通常タブとの一致 — 再現手順 1 と 2 のストリーム、および FR1〜FR5 の形を含むストリームについて、mux ペイン（実際の pty_reader_loop とデーモンの agent-status タスク）の OSC 7501 記録表と OSC 777 状態が、同じストリームを一括で処理した通常タブと一致する。この一致は、各行が指定するすべての配送（すべての分割位置と、1 バイトずつの読み込みを含む）で成り立つ。
- **FR7:** 回帰テスト — 次の 3 つを追加する。(1) トラッカーの単体テスト（altscreen_split_read.rs）。RIS と複合 DECSET を含むストリームを、すべての分割位置と 1 バイトずつの読み込みで流し、期待するライブ区間になることを確かめる。食い違い状態の両方向（client_alt=true/shadow_alt=false と client_alt=false/shadow_alt=true）を含める。(2) SC-1x のデーモン行（daemon/tests.rs、drive_reads）。(3) (2) に対応する通常タブの行（tabs/tests/output_pipeline.rs、process_combined）。
- **FR8:** 変わらない挙動 — 次は変更しない。scrollback の書き込み経路（extract_main_buffer、to_write、cuts、フォールバックのときに何も書かないこと）。AgentStatusFeedScanner（step_ris と cut_live / tail_live を含む）。term_core。食い違っていない状態で受けた未対応の形のフォールバック（AC-6a）。既存のテストはすべて変更なしで通る。

### Non-Functional Requirements

- **NFR1 - 堅牢性:** どの入力でも panic しない。引き継ぐ状態は O(1) とする（切り替えの途中状態は最大 7 バイトで、それに CSI の解析状態を加える）。引数の値は上限で飽和させ、オーバーフローさせない。
- **NFR2 - フィーチャーゲート:** 変更は、--no-default-features でビルドされる mux のコードの中に収める。CLI のみのビルドもコンパイルできる。
- **NFR3 - ログ:** 新しいログ出力を追加しない。
- **NFR4 - プラットフォーム:** Linux と Windows。
- **NFR5 - term_core 不変:** term_core（crates/term_core）は基準となる挙動として扱い、変更しない。

## Implementation Approach

### Architecture

**変更対象:**
- AltSpanTracker の走査（FR1〜FR5）

**変更しないもの（FR8、NFR5、A4）:**
- scrollback の書き込み経路（extract_main_buffer、to_write、cuts、ScrollbackWriteFilter）
- AgentStatusFeedScanner（step_ris と cut_live / tail_live を含む）
- term_core（crates/term_core）

### 入力の形ごとの扱い

| 入力の形 | 条件 | 扱い | 要件 |
|----------|------|------|------|
| ESC c（指示子スロットの外） | 状態を問わない | RIS。client_alt と shadow_alt を false にし、引き継ぎ状態をすべて消し、c の直後からライブにする | FR1 |
| 読み込み末尾の単独のライブな ESC と、次の読み込み先頭の c | 状態を問わない | RIS（FR1 と同じ） | FR2 |
| 2 バイト以上の途中状態（例: ESC [）と、次の読み込み先頭の c | 状態を問わない | RIS にしない（CSI c） | FR2 |
| 途中状態の引き継ぎ中に、次の読み込みが ESC c で始まる（ESC [?10 と ESC c） | 状態を問わない | RIS | FR2 |
| 指示子スロットにある ESC c（ESC ( ESC c、ESC ) ESC c、境界をまたぐ形を含む） | 状態を問わない | クライアント側の規則では RIS にしない。シャドウ側の規則では RIS として shadow_alt を false にする。adopt は起きない | FR3 |
| ライブな ESC で始まる DEC private mode set / reset で、引数列が 47 / 1047 / 1049 を含み、ALT_SCREEN_TOGGLES のどの形にも当たらないもの | 食い違い状態 | その読み込みに A3 のフォールバックを返し、読み込み後のパーサー状態を adopt する | FR4 |
| 上の形が読み込み境界で分割されたもの | 食い違い状態 | CSI の解析状態を引き継ぎ、終端の h / l を受けた読み込みで FR4 と同じく扱う | FR5 |
| 切り替えの途中状態が ALT_SCREEN_TOGGLES の形から外れて DEC private CSI として続くもの（ESC [?104 と 9;1h） | 食い違い状態 | 保持しているバイトから CSI の解析を続ける | FR5 |
| ESC が指示子スロットにある CSI（ESC ( ESC [?1;1049h/l など） | 状態を問わない | FR4 / FR5 の検出対象外。既存のパーサー照合に任せる | FR4、A7 |
| 未対応の形 | 一致状態 | 既存のフォールバック（AC-6a）のまま変えない | FR8、A2 |

### 引き継ぎ状態

| 項目 | 内容 | 消すとき |
|------|------|----------|
| 切り替えの途中状態 | ALT_SCREEN_TOGGLES の形を照合している途中のバイト（最大 7 バイト） | adopt、RIS、パーサーの再構築 |
| 指示子待ち | ESC ( / ESC ) の直後のスロットを待っている状態 | adopt、RIS、パーサーの再構築 |
| CSI の解析状態: ESC [ ? を見たか | DEC private CSI の解析中かどうか | adopt、RIS、パーサーの再構築。ESC、CAN、SUB、引数でも終端でもないバイトを受けたとき（検出なしで捨てる） |
| CSI の解析状態: いま読んでいる引数の値 | 上限で飽和させる。飽和した値は 47 / 1047 / 1049 として扱わない（A6） | 同上 |
| CSI の解析状態: それまでの引数に 47 / 1047 / 1049 があったか | 終端の h / l で FR4 の検出に使う | 同上 |

引き継ぐ状態の大きさは O(1) とする（NFR1）。

### API Design

該当なし。

### Database Schema

該当なし。

### Dependencies

**Internal Dependencies:**
- term_core（crates/term_core）: 基準となる挙動。変更しない（NFR5）。
- シャドウパーサー（vt100）: shadow_alt の基準。RIS でメイン画面に戻り、ESC ( ESC c を RIS として扱う（A1、TS3）。
- ALT_SCREEN_TOGGLES、match_switch / resume: 既存の切り替え形の照合と、その途中状態の引き継ぎ。

**External Dependencies:**
- vt100（シャドウパーサー、既存）

### File Structure

テストの追加先（FR7）:

```
altscreen_split_read.rs          # トラッカーの単体テスト（TS1、TS2、TS3）
daemon/tests.rs                  # SC-1x のデーモン行（K38 以降、TS4）
tabs/tests/output_pipeline.rs    # 対応する通常タブの行（TS5）
```

## Declared Change Set

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

各 SPEC は、上のフィーチャー固有のパスに加えて、次の 2 つのワークフロー生成エントリーをデフォルトで宣言する。

- `feature-docs/mux-alt-tracker-ris-compound-decset/**`
- `test-docs/mux-alt-tracker-ris-compound-decset/**`

`feature-docs/mux-alt-tracker-ris-compound-decset/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` であり、この節は引用のみでルールは再掲しない。

`test-docs/mux-alt-tracker-ris-compound-decset/**` に含まれるもの: `test-docs/mux-alt-tracker-ris-compound-decset/{T}.tests.yaml`（タスクごとのテスト記録）。生成主体は `implement-phase.md` であり、この節は引用のみでルールは再掲しない。

この 2 つのデフォルトエントリーは、SPEC 作成者が明示的に除外しない限り宣言に含まれる。記載が無いことを除外とは見なさない。除外は意図的で明示的な絞り込みとする。

この宣言はスーパーセット（SUPERSET）の主張である。検証時に観測される実際の変更集合は、宣言された集合と等しい必要はなく、宣言に含まれ（CONTAINED IN）ていればよい。implement タスクを 1 つも生成しないフィーチャーは `test-docs/{feature}/` ディレクトリを生成しないが、宣言された `test-docs/{feature}/**` はその場合も正しい。宣言されたパスが生成されなくても違反ではない。

## Test Scenarios

### Unit Tests

- [ ] TS1（FR1〜FR5、AC1、AC3、AC4、AC5）: altscreen_split_read.rs で、RIS（メイン画面上、一致した代替画面上、食い違い状態）、指示子スロットの ESC c、複合 DECSET（食い違いの両方向）を含むストリームを、すべての分割位置（1 か所、2 か所）と 1 バイトずつの読み込みで流す - 各読み込みのライブ区間が期待値と一致する。extract_main_buffer は RIS を扱わないので、期待値は、RIS を扱うテスト側のオラクルで求めるか、明示する。
- [ ] TS2（NFR1、AC6）: AC-5 の網羅テストを拡張し、食い違い状態で、DEC private CSI の途中位置と 256 通りのバイト値、オーバーフローする引数、ESC / CAN / SUB による打ち切りを流す - panic せず、結果が整った形になる。
- [ ] TS3（FR1、FR3）: 実際のシャドウパーサー（vt100）で挙動を固定する - RIS の後は alternate_screen() が false になる。ESC ( ESC c を RIS として扱う。ESC [?1;1049h には従い、ESC [?1;1047h は無視する。

### Integration Tests

- [ ] TS4（FR6、FR7、AC1〜AC5）: daemon/tests.rs に SC-1x の行（K38 以降）を、既存の K16〜K37 とは別の一覧として追加する。各行を、その行が指定するすべての配送で drive_reads（seed 付き）に流す - 記録表と OSC 777 状態が通常タブと一致する。
- [ ] TS5（FR6、FR7）: tabs/tests/output_pipeline.rs で、同じストリームを seeded_plain_tab / process_combined で一括処理する。既存の K16〜K37 とは別の一覧にする - 基準となる記録表と OSC 777 状態が得られる。

### Regression Tests

- [ ] TS6（FR8、AC7）: src-tauri と term_core の --lib スイート全体と、--no-default-features でのチェックを実行する - すべて通る。

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

- [ ] Existing E2E tests pass without regression

### Edge Cases

- [ ] メイン画面上の RIS ではライブ区間を途切れさせない（コーパスに既にある E(P) L(P) A + ESC c の行）。
- [ ] 一致した代替画面上の RIS は、これまでフォールバックで扱っていたが、正確な区間で扱うようになる。
- [ ] ESC [ c（DA1）と ESC [ | c は RIS ではない。
- [ ] 途中状態を引き継いでいる最中の RIS（ESC [?10 | ESC c）。CSI の解析途中の RIS（ESC [?1;10 | ESC c）。
- [ ] 指示子スロットの ESC c: ESC ( ESC c、ESC ) ESC c、ESC ( | ESC c、ESC ( ESC | c
- [ ] client_alt=true / shadow_alt=false の状態での、複合形の leave（ESC [?1;1049l）
- [ ] client_alt=false / shadow_alt=true の状態での ESC [?1;1047h
- [ ] 引数の順序の違い（?1049;1h）、先頭のゼロ（?01049h）、空の引数（?;1049h）
- [ ] 切り替えの途中状態が CSI の続きに変わる場合（ESC [?104 | 9;1h）
- [ ] CSI の解析途中に来る ESC / CAN / SUB による打ち切りと、引数でも終端でもないバイト
- [ ] オーバーフローする長い引数（例: ?99999999999999999999h）
- [ ] ランダムストリームの差分テストでは、比較できたケースが 2000 件を超える条件を保つ（adopt したケースはスキップされる）。

### Performance Tests

該当なし。

## Security Considerations

- **Input Validation:** どの入力でも panic しない。引数の値は上限で飽和させ、オーバーフローさせない（NFR1）。
- **Authentication / Authorization / Data Protection / XSS Prevention / SQL Injection Prevention / CSRF Protection:** 該当なし。

## Error Handling

### Error Codes

該当なし。

### Error Flow

- 新しいログ出力を追加しない（NFR3）。
- CSI の解析途中に ESC、CAN、SUB、引数でも終端でもないバイトを受けたら、CSI の解析状態を検出なしで捨てる（FR5）。

## Performance Optimization

### Performance Goals

- 引き継ぐ状態は O(1) とする（切り替えの途中状態は最大 7 バイトで、それに CSI の解析状態を加える）（NFR1）。

### Optimization Strategies

該当なし。

### Caching Strategy

該当なし。

## Assumptions

- **A1:** vt100 のシャドウパーサーは、RIS（ESC c）でメイン画面に戻る（alternate_screen() が false になる）。ESC ( ESC c では、ESC が ESC ( を打ち切るため、RIS として扱う。TS3 でこの挙動を固定する。
- **A2:** FR4 / FR5 の検出は、client_alt と shadow_alt が食い違っている状態に限る。食い違っていない状態で受けた未対応の形は対象外とする（例: ESC [?1;1047h。シャドウパーサーが ?1047 を無視するため検出できない）。これは変更前から残っている問題で、退行ではない。
- **A3:** 検出した複合 DECSET は A3 の保守的なフォールバックで扱い、複合形の切り替え位置を正確にはモデル化しない。そのため、同じ読み込みの中で複合形より前にあるマークは正確に扱えない。これは許容する。
- **A4:** scrollback の書き込み経路（extract_main_buffer、to_write、cuts、ScrollbackWriteFilter）、term_core、AgentStatusFeedScanner（step_ris を含む）は変更しない。
- **A5:** 読み込み境界で分割された複合 DECSET も対象にする（carry_csi_state）。match_switch / resume は ALT_SCREEN_TOGGLES の途中状態しか引き継がないので、ESC [?1;10 | 49h のように分割されると途中状態が失われる。CSI の解析状態を O(1) で引き継ぐことで、フォールバックと adopt が読み込み境界によらず働く。リスクは次の 3 点。引き継ぎ状態を増やすので、resume、指示子スロット、adopt 時の消去と整合させる必要がある。数値のオーバーフローとシーケンスの打ち切りを安全に扱う必要がある。食い違い状態で、すべての分割位置と 1 バイトずつの読み込みを検証する必要がある。single_read_only は、分割された形で現象が残るため採らない。
- **A6:** 引数の値は上限で飽和させる。飽和した値は 47 / 1047 / 1049 として扱わない。引数の個数が term_core やシャドウパーサーの上限を超える入力では、検出が保守的に働いてもよい（そのときはフォールバックして adopt する）。
- **A7:** ESC が指示子スロットにある複合 DECSET（ESC ( ESC [?1;1049h/l）は、FR4 / FR5 で検出しない。シャドウパーサーはこれに従うので、既存のパーサー照合がフォールバックして adopt する。adopt 後のクライアント状態は、食い違いのどちらの方向でも term_core と一致する。
- **A8:** デザインステップはスキップする（UI と画面の変更が無いため）。

## Success Criteria

- [ ] FR1〜FR8 がすべて実装され、テストされている
- [ ] AC1〜AC7 を満たす
- [ ] TS1〜TS6 がすべて通る
- [ ] 既存のテストが変更なしで通る
- [ ] cargo check --no-default-features が成功する

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし。

## Implementation Phases (if applicable)

該当なし。タスクの分割は create-plan で決める。

## References

- 要件定義書: `feature-docs/mux-alt-tracker-ris-compound-decset/REQUIREMENTS.md`
- review round1 の指摘: 4aef749bee51a717（RIS）、b019b1bb35b07fc6（複合 DECSET）
