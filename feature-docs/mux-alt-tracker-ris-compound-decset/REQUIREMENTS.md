---
title: "mux-alt-tracker-ris-compound-decset"
created_date: 2026-10-11
status: draft
---

# mux-alt-tracker-ris-compound-decset - 要件定義書

## 1. 概要

### 1.1 背景

review round1 で、medium の指摘 2 件が未解消のまま残っている。

- 4aef749bee51a717（RIS）
- b019b1bb35b07fc6（複合 DECSET）

mux ペインの AltSpanTracker は、client_alt と shadow_alt が食い違った状態（?1047 の使用後、または指示子スロットでの切り替え後）で、RIS と未対応の複合 DECSET による画面切り替えを追えていない。

### 1.2 目的

mux ペインの AltSpanTracker が、client_alt と shadow_alt が食い違った状態でも、RIS と未対応の複合 DECSET による画面切り替えに追従するようにする。これにより、mux ペインの OSC 7501 記録表と OSC 777 状態が、同じバイト列を一括で処理した通常タブと一致する。この一致は、読み込み境界がどこにあっても成り立つ。

### 1.3 スコープ

**対象**:
- AltSpanTracker の走査での RIS の扱い（FR1〜FR3）
- 食い違い状態で受けた未対応の画面切り替え形の検出（FR4、FR5）
- 通常タブとの一致（FR6）と、その回帰テスト（FR7）

**対象外**:
- FR8 に挙げた、変更しない挙動
- 食い違っていない状態で受けた未対応の形（14.3 の A2）
- ESC が指示子スロットにある複合 DECSET（14.3 の A7）
- UI と画面の変更（デザインステップはスキップする）

## 2. ビジネス要件

### 2.1 ビジネス目標

- mux ペインの AltSpanTracker が、client_alt と shadow_alt が食い違った状態でも、RIS と未対応の複合 DECSET による画面切り替えに追従する。mux ペインの OSC 7501 記録表と OSC 777 状態は、同じバイト列を一括で処理した通常タブと一致する。この一致は、読み込み境界がどこにあっても成り立つ。
- review round1 で medium として残った指摘 4aef749bee51a717（RIS）と b019b1bb35b07fc6（複合 DECSET）を解消する。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| mux ペインの利用者 | mux ペインの OSC 7501 記録表と OSC 777 状態を参照する |

### 2.3 期待される効果

- 読み込み境界がどこにあっても、mux ペインの OSC 7501 記録表と OSC 777 状態が通常タブと一致する。
- review round1 の指摘 4aef749bee51a717 と b019b1bb35b07fc6 が解消する。

## 3. ユースケース

該当なし。このフィーチャーは、mux の読み込みループ内部にある状態機械（AltSpanTracker）のバグ修正であり、UI と画面は変わらない。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 状態 |
|----|--------|------|
| FR1 | RIS への追従 | resolved |
| FR2 | 読み込み境界で分割された RIS | resolved |
| FR3 | 指示子スロットにある ESC c | resolved |
| FR4 | 食い違い状態での未対応の画面切り替え形の検出 | resolved |
| FR5 | 読み込み境界で分割された FR4 の形の検出 | resolved |
| FR6 | 通常タブとの一致 | resolved |
| FR7 | 回帰テスト | resolved |
| FR8 | 変わらない挙動 | resolved |

### 4.2 機能詳細

#### FR1: RIS への追従

**説明**:
- AltSpanTracker の走査では、指示子スロットの外にある ESC c を RIS として扱う。
- RIS を受けたら、client_alt と shadow_alt を false にする。
- 同時に、引き継ぎ状態（切り替えの途中状態、指示子待ち、FR5 の CSI 解析状態）を消す。
- c の直後からライブにする。
- メイン画面上の RIS ではライブ区間を途切れさせず、一括抽出したときの区間と一致させる。

**関連する受け入れ基準**: AC1

#### FR2: 読み込み境界で分割された RIS

**説明**:
- 読み込みが単独のライブな ESC で終わり、次の読み込みが c で始まる場合も、FR1 と同じく RIS として扱う。
- 引き継いだ途中状態が 2 バイト以上あるとき（例: ESC [）は、続く c を RIS にしない。この並びは CSI c になる。
- 途中状態を引き継いでいる最中に、次の読み込みが ESC c で始まる場合（ESC [?10 | ESC c）は、RIS として扱う。

**関連する受け入れ基準**: AC3

#### FR3: 指示子スロットにある ESC c

**説明**:
- ESC ( または ESC ) の直後のスロットにある ESC c は、クライアント側の規則（term_core と step_ris の規則）では RIS にしない。
- この扱いは、1 回の読み込みの中でも、境界をまたぐ場合（ESC ( | ESC c、ESC ( ESC | c）でも同じとする。
- シャドウパーサーは、どの ESC を受けても処理中のシーケンスを打ち切る。そのため、シャドウ側の規則ではこの ESC c を RIS として扱い、shadow_alt を false にする。
- その結果、この形では adopt が起きない。

**関連する受け入れ基準**: AC4

#### FR4: 食い違い状態での未対応の画面切り替え形の検出

**説明**:
- 対象は、client_alt と shadow_alt が食い違っている状態で受けた、ライブな ESC で始まる DEC private mode set / reset（ESC [ ? 引数列 h|l）とする。
- 引数列が数値として 47 / 1047 / 1049 を含み、かつ ALT_SCREEN_TOGGLES のどの形にも当たらないとき、次のように扱う。
  - その読み込みには A3 の保守的なフォールバックを返す。読み込みの前後ともメイン画面なら読み込み全体、それ以外は空とする。
  - 読み込み後のパーサー状態を adopt する。
- 例: ESC [?1;1049h、ESC [?1049;1h、ESC [?1;1047h/l、ESC [?01049h
- ESC が指示子スロットにある CSI は、クライアントにとって文字列である。FR4 の検出対象外とし、既存のパーサー照合に任せる。

**関連する受け入れ基準**: AC2

#### FR5: 読み込み境界で分割された FR4 の形の検出

**説明**:
- FR4 の形は、読み込み境界で分割されていても検出する（create-spec-q0001 requirement.compound-decset-split = carry_csi_state）。
- そのために、DEC private CSI を解析している途中の状態を O(1) で次の読み込みに引き継ぐ。引き継ぐのは次の 3 つ。
  - ESC [ ? を見たか
  - いま読んでいる引数の値（上限で飽和させる）
  - それまでの引数に 47 / 1047 / 1049 があったか
- 終端の h / l を受けた読み込みで FR4 のフォールバックを返し、adopt する。
- 切り替えの途中状態（ESC [?104 など）が ALT_SCREEN_TOGGLES の形から外れ、DEC private CSI として続くことがある（ESC [?104 | 9;1h）。この場合は、保持しているバイトから CSI の解析を続ける。
- CSI の解析状態は、次のどれかが来たら検出なしで捨てる。
  - ESC（FR1〜FR3 の判定を含む）
  - CAN
  - SUB
  - 引数でも終端でもないバイト
- adopt、RIS、パーサーの再構築のときも、ほかの引き継ぎ状態と一緒に CSI の解析状態を消す。

**関連する受け入れ基準**: AC5

#### FR6: 通常タブとの一致

**説明**: 対象のストリームは、再現手順 1 と 2 のストリーム、および FR1〜FR5 の形を含むストリームとする。これらについて、mux ペイン（実際の pty_reader_loop とデーモンの agent-status タスク）の OSC 7501 記録表と OSC 777 状態が、同じストリームを一括で処理した通常タブと一致する。この一致は、各行が指定するすべての配送（すべての分割位置と、1 バイトずつの読み込みを含む）で成り立つ。

**関連する受け入れ基準**: AC1、AC2、AC5

#### FR7: 回帰テスト

**説明**: 次の 3 つを追加する。
1. トラッカーの単体テスト（altscreen_split_read.rs）
   - RIS と複合 DECSET を含むストリームを、すべての分割位置と 1 バイトずつの読み込みで流し、期待するライブ区間になることを確かめる。
   - 食い違い状態は両方向を含める（client_alt=true/shadow_alt=false と、client_alt=false/shadow_alt=true）。
2. SC-1x のデーモン行（daemon/tests.rs、drive_reads）
3. 2 に対応する通常タブの行（tabs/tests/output_pipeline.rs、process_combined）

#### FR8: 変わらない挙動

**説明**: 次は変更しない。
- scrollback の書き込み経路（extract_main_buffer、to_write、cuts、フォールバックのときに何も書かないこと）
- AgentStatusFeedScanner（step_ris と cut_live / tail_live を含む）
- term_core
- 食い違っていない状態で受けた未対応の形のフォールバック（AC-6a）

既存のテストは、すべて変更なしで通る。

**関連する受け入れ基準**: AC7

## 5. 非機能要件

| ID | 名前 | 内容 |
|----|------|------|
| NFR1 | 堅牢性 | どの入力でも panic しない。引き継ぐ状態は O(1) とする（切り替えの途中状態は最大 7 バイトで、それに CSI の解析状態を加える）。引数の値は上限で飽和させ、オーバーフローさせない。 |
| NFR2 | フィーチャーゲート | 変更は、--no-default-features でビルドされる mux のコードの中に収める。CLI のみのビルドもコンパイルできる。 |
| NFR3 | ログ | 新しいログ出力を追加しない。 |
| NFR4 | プラットフォーム | Linux と Windows。 |
| NFR5 | term_core 不変 | term_core（crates/term_core）は基準となる挙動として扱い、変更しない。 |

### 5.1 パフォーマンス要件
- 引き継ぐ状態: O(1)。切り替えの途中状態は最大 7 バイトで、それに CSI の解析状態を加える（NFR1）。

### 5.2 セキュリティ要件
- 入力検証: どの入力でも panic しない。引数の値は上限で飽和させ、オーバーフローさせない（NFR1）。

### 5.3 可用性要件
該当なし。

### 5.4 保守性要件
- ログ出力: 新しいログ出力を追加しない（NFR3）。

### 5.5 互換性要件
- プラットフォーム: Linux と Windows（NFR4）。
- ビルド: --no-default-features の CLI のみのビルドもコンパイルできる（NFR2）。

## 6. UI/UX要件

該当なし。UI と画面の変更は無く、デザインステップはスキップする。

## 7. データ要件

### 7.1 データモデル概要

AltSpanTracker が読み込みをまたいで引き継ぐ状態（FR1、FR5、NFR1）。

### 7.2 データ項目

| エンティティ | 項目名 | 型 | 必須 | 説明 |
|--------------|--------|-----|------|------|
| 引き継ぎ状態 | 切り替えの途中状態 | バイト列（最大 7 バイト） | ○ | ALT_SCREEN_TOGGLES の形を照合している途中のバイト |
| 引き継ぎ状態 | 指示子待ち | 状態 | ○ | ESC ( / ESC ) の直後のスロットを待っている状態 |
| CSI の解析状態 | ESC [ ? を見たか | 真偽 | ○ | DEC private CSI の解析中かどうか |
| CSI の解析状態 | いま読んでいる引数の値 | 数値（上限で飽和） | ○ | 飽和した値は 47 / 1047 / 1049 として扱わない |
| CSI の解析状態 | それまでの引数に 47 / 1047 / 1049 があったか | 真偽 | ○ | 終端の h / l で FR4 の検出に使う |

これらの状態は、adopt、RIS、パーサーの再構築のときにすべて消す（FR1、FR5）。

### 7.3 データ保持期間
該当なし。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約
- term_core（crates/term_core）は変更しない（NFR5）。
- 変更は、--no-default-features でビルドされる mux のコードの中に収める（NFR2）。
- scrollback の書き込み経路（extract_main_buffer、to_write、cuts、ScrollbackWriteFilter）と AgentStatusFeedScanner（step_ris を含む）は変更しない（FR8、A4）。

### 9.2 ビジネス上の制約
該当なし。

### 9.3 スケジュール制約
該当なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/mux-alt-tracker-ris-compound-decset/**`
- `test-docs/mux-alt-tracker-ris-compound-decset/**`

`feature-docs/mux-alt-tracker-ris-compound-decset/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/mux-alt-tracker-ris-compound-decset/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/mux-alt-tracker-ris-compound-decset/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/{feature}/` ディレクトリを生成しないが、宣言された `test-docs/{feature}/**` は依然として正しい。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| CSI の解析状態を引き継ぎ状態に加えるため、resume、指示子スロット、adopt 時の消去と整合させる必要がある | 中 | FR1、FR2、FR5 で消去と継続の規則を定める |
| 数値のオーバーフローと、シーケンスの打ち切りを安全に扱う必要がある | 中 | NFR1、A6（上限で飽和させる）。FR5（ESC / CAN / SUB と、引数でも終端でもないバイトで捨てる） |
| 食い違い状態で、すべての分割位置と 1 バイトずつの読み込みを検証する必要がある | 中 | FR7、TS1、TS2、AC5 |
| 検出した複合 DECSET は A3 のフォールバックで扱う。同じ読み込みの中で複合形より前にあるマークは正確に扱えない | 低 | 許容する（A3） |
| 食い違っていない状態で受けた未対応の形（例: ESC [?1;1047h）は検出しない | 低 | 対象外とする（A2）。変更前から残っている問題で、退行ではない |

### 10.2 ビジネスリスク
該当なし。

## 11. 成功基準

### 11.1 受け入れ基準

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

### 11.2 KPI
該当なし。

## 12. テストシナリオ

### 12.1 テスト観点

| ID | 種別 | 対象 | 内容 |
|----|------|------|------|
| TS1 | unit | FR1〜FR5、AC1、AC3、AC4、AC5 | altscreen_split_read.rs で、次の形を含むストリームを、すべての分割位置（1 か所、2 か所）と 1 バイトずつの読み込みで流す。形は、RIS（メイン画面上、一致した代替画面上、食い違い状態）、指示子スロットの ESC c、複合 DECSET（食い違いの両方向）。各読み込みのライブ区間が期待値と一致することを確かめる。extract_main_buffer は RIS を扱わないので、期待値は、RIS を扱うテスト側のオラクルで求めるか、明示する。 |
| TS2 | unit | NFR1、AC6 | AC-5 の網羅テストを拡張する。食い違い状態で、DEC private CSI の途中位置と 256 通りのバイト値、オーバーフローする引数、ESC / CAN / SUB による打ち切りを流す。panic せず、結果が整った形であることを確かめる。 |
| TS3 | unit | FR1、FR3 | 実際のシャドウパーサー（vt100）で次の挙動を固定する。RIS の後は alternate_screen() が false になる。ESC ( ESC c を RIS として扱う。ESC [?1;1049h には従い、ESC [?1;1047h は無視する。 |
| TS4 | integration | FR6、FR7、AC1〜AC5 | daemon/tests.rs に、SC-1x の行（K38 以降）を追加する。既存の K16〜K37 の一覧とは別の一覧にする。各行を、その行が指定するすべての配送で drive_reads（seed 付き）に流し、記録表と OSC 777 状態を比べる。 |
| TS5 | integration | FR6、FR7 | tabs/tests/output_pipeline.rs で、同じストリームを seeded_plain_tab / process_combined で一括処理し、基準となる記録表と OSC 777 状態を得る。既存の K16〜K37 の一覧とは別の一覧にする。 |
| TS6 | regression | FR8、AC7 | src-tauri と term_core の --lib スイート全体と、--no-default-features でのチェックを実行する。 |

### 12.2 エッジケース

- メイン画面上の RIS ではライブ区間を途切れさせない（コーパスに既にある E(P) L(P) A + ESC c の行）。
- 一致した代替画面上の RIS は、これまでフォールバックで扱っていたが、正確な区間で扱うようになる。
- ESC [ c（DA1）と ESC [ | c は RIS ではない。
- 途中状態を引き継いでいる最中の RIS（ESC [?10 | ESC c）。CSI の解析途中の RIS（ESC [?1;10 | ESC c）。
- 指示子スロットの ESC c: ESC ( ESC c、ESC ) ESC c、ESC ( | ESC c、ESC ( ESC | c
- client_alt=true / shadow_alt=false の状態での、複合形の leave（ESC [?1;1049l）
- client_alt=false / shadow_alt=true の状態での ESC [?1;1047h
- 引数の順序の違い（?1049;1h）、先頭のゼロ（?01049h）、空の引数（?;1049h）
- 切り替えの途中状態が CSI の続きに変わる場合（ESC [?104 | 9;1h）
- CSI の解析途中に来る ESC / CAN / SUB による打ち切りと、引数でも終端でもないバイト
- オーバーフローする長い引数（例: ?99999999999999999999h）
- ランダムストリームの差分テストでは、比較できたケースが 2000 件を超える条件を保つ（adopt したケースはスキップされる）。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| RIS | ESC c |
| 指示子スロット | ESC ( / ESC ) の直後のスロット |
| client_alt | クライアント側の規則（term_core と step_ris の規則）で見た代替画面の状態 |
| shadow_alt | シャドウパーサー（vt100）で見た代替画面の状態 |
| 食い違い状態 | client_alt と shadow_alt が異なる状態 |
| 複合 DECSET | DEC private mode set / reset（ESC [ ? 引数列 h または l）のうち、引数列が数値として 47 / 1047 / 1049 を含み、ALT_SCREEN_TOGGLES のどの形にも当たらないもの（FR4） |
| A3 のフォールバック | 読み込みの前後ともメイン画面なら読み込み全体、それ以外は空のライブ区間を返す扱い |
| adopt | 読み込み後のパーサー状態を採用すること |
| 引き継ぎ状態 | 読み込みをまたいで持ち越す状態（切り替えの途中状態、指示子待ち、CSI の解析状態） |

## 14. 確認事項

### 14.1 確認済み事項

- [x] 読み込み境界で分割された複合 DECSET を対象にするか（create-spec-q0001 requirement.compound-decset-split）: 対象にする（carry_csi_state）。single_read_only は採らない。
- [x] デザインステップ（create-spec-q0001 design-step.recommendation）: スキップする。

### 14.2 未確認・保留事項

なし。

### 14.3 前提

- A1: vt100 のシャドウパーサーは、RIS（ESC c）でメイン画面に戻る（alternate_screen() が false になる）。ESC ( ESC c では、ESC が ESC ( を打ち切るため、RIS として扱う。TS3 でこの挙動を固定する。
- A2: FR4 / FR5 の検出は、client_alt と shadow_alt が食い違っている状態に限る。食い違っていない状態で受けた未対応の形は対象外とする（例: ESC [?1;1047h。シャドウパーサーが ?1047 を無視するため検出できない）。これは変更前から残っている問題で、退行ではない。
- A3: 検出した複合 DECSET は A3 の保守的なフォールバックで扱い、複合形の切り替え位置を正確にはモデル化しない。そのため、同じ読み込みの中で複合形より前にあるマークは正確に扱えない。これは許容する。
- A4: scrollback の書き込み経路（extract_main_buffer、to_write、cuts、ScrollbackWriteFilter）、term_core、AgentStatusFeedScanner（step_ris を含む）は変更しない。
- A5: 読み込み境界で分割された複合 DECSET も対象にする（carry_csi_state）。match_switch / resume は ALT_SCREEN_TOGGLES の途中状態しか引き継がないので、ESC [?1;10 | 49h のように分割されると途中状態が失われる。CSI の解析状態を O(1) で引き継ぐことで、フォールバックと adopt が読み込み境界によらず働く。single_read_only は、分割された形で現象が残るため採らない。
- A6: 引数の値は上限で飽和させる。飽和した値は 47 / 1047 / 1049 として扱わない。引数の個数が term_core やシャドウパーサーの上限を超える入力では、検出が保守的に働いてもよい（そのときはフォールバックして adopt する）。
- A7: ESC が指示子スロットにある複合 DECSET（ESC ( ESC [?1;1049h/l）は、FR4 / FR5 で検出しない。シャドウパーサーはこれに従うので、既存のパーサー照合がフォールバックして adopt する。adopt 後のクライアント状態は、食い違いのどちらの方向でも term_core と一致する。
- A8: デザインステップはスキップする（UI と画面の変更が無いため）。

## 15. 参考資料

- review round1 の指摘: 4aef749bee51a717（RIS）、b019b1bb35b07fc6（複合 DECSET）
