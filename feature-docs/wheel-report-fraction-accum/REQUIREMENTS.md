---
title: "wheel-report-fraction-accum"
created_date: 2026-09-20
status: draft
---

# wheel-report-fraction-accum - 要件定義書

## 1. 概要

### 1.1 背景

マウスレポート経路では `wheel_report_notches(lines)`
(`src-tauri/src/window_host/input_translate.rs:472-478`) が `lines.abs()` を切り捨てるため、
`|lines| < 1.0` は常に 0 になる。その結果、ホイールハンドラの `if notches != 0` ガード
(`src-tauri/src/window_host/pointer_routing.rs:943-949`) が何も書き込まず、セル行の端数へ
正規化されるピクセルデルタ機器 (`pointer_routing.rs:867-870`) はマウスレポートを一切生成できない。

### 1.2 目的

レポート経路に端数アキュムレータを設け、1ノッチ未満の高精度ホイール入力が破棄されずに
マウスレポートへ到達するようにする。同時に、既存の二層ノッチ上限
(`MAX_WHEEL_REPORT_NOTCHES`) と、非有限デルタに対する堅牢性を保つ。

### 1.3 スコープ

対象は `src-tauri/src/window_host/` 配下のホイール入力ルーティング
(`input_translate.rs` / `pointer_routing.rs` / `mouse_report.rs` / `mod.rs`) である。
alternate-scroll 経路 (`WindowHost::alt_scroll_accum`、`MAX_ALT_SCROLL_NOTCHES`) は
変更対象外であり、現状の挙動をそのまま維持する (FR7)。

## 2. ビジネス要件

### 2.1 ビジネス目標

- 高精度ホイール入力 (トラックパッド / ピクセルデルタ) が、1ノッチ閾値未満で破棄されずに
  マウスレポート対象アプリケーションへ到達すること。1ノッチ未満のスクロールがレポート経路上で
  無言のまま死んでいる状態を解消する。
- 保持された端数は、それを生んだジェスチャー・タブ・トラッキングセッションの内部に限定されること。
  タブ切り替えやトラッキングモード解除をまたいで漏れ出すことは決してない。
- レポート経路はセキュリティ特性としての既存の二層ノッチ上限 (`MAX_WHEEL_REPORT_NOTCHES`) を
  維持し、非有限デルタによって恒久的に汚染された状態へ追い込まれないこと。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| - | requirements_analysis に対象ユーザーの区分は含まれない |

### 2.3 期待される効果

- 2.1 のビジネス目標の達成そのもの。個別の効果指標は requirements_analysis に含まれない。

## 3. ユースケース

### 3.1 ユースケース一覧

requirements_analysis にユースケースの定義は含まれない。機能の期待挙動は
4章の機能要件、11章の受け入れ基準、12章のテストシナリオが規定する。

### 3.2 ユースケース詳細

該当なし。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | ステータス |
|----|--------|------|-----------|
| FR1 | Accumulate sub-notch wheel deltas on the report path | レポート経路で1ノッチ未満のホイールデルタをイベント間で累積する | resolved |
| FR2 | Consume whole notches, retain the signed remainder | 整数部をノッチとして消費し、符号付きの端数を保持する | resolved |
| FR3 | Reject a non-finite delta before mutating the accumulator | 非有限デルタはアキュムレータを変更する前に棄却する | resolved |
| FR4 | Saturate the accumulated notch magnitude while still a float | ノッチ絶対値を浮動小数点のまま上限で飽和させる | resolved |
| FR5 | Reset the accumulator on tab change and on tracking release | タブ変更時とトラッキング解除時にアキュムレータをゼロにする | resolved |
| FR6 | A rejected notch neither advances nor resets the accumulator | 棄却されたノッチはアキュムレータを進めも戻しもしない | resolved |
| FR7 | Keep the alternate-scroll accumulator separate and untouched | alternate-scroll 側のアキュムレータは分離したまま変更しない | resolved |

### 4.2 機能詳細

#### FR1: Accumulate sub-notch wheel deltas on the report path

**説明**:
レポート経路は端数のホイールデルタをイベント間で累積し、1ノッチ未満のデルタが連続しても
合計が1ノッチに達するまで積み上がるようにする。現状 `wheel_report_notches(lines)`
(`src-tauri/src/window_host/input_translate.rs:472-478`) は `lines.abs()` を切り捨てるため、
`|lines| < 1.0` はすべて 0 となり、ホイールハンドラの `if notches != 0` ガード
(`src-tauri/src/window_host/pointer_routing.rs:943-949`) が何も書き込まない。したがって
セル行の端数へ正規化されたピクセルデルタ機器 (`pointer_routing.rs:867-870`) はマウスレポートを
生成できない。累積処理は alternate-scroll 経路が `accumulate_alt_scroll_lines`
(`input_translate.rs:340-349`) で既に用いている端数管理に倣うが、レポート経路専用のストアを
用いる (FR7 参照)。

**ステータス**: resolved

#### FR2: Consume whole notches, retain the signed remainder

**説明**:
各ホイールイベントは自身のデルタをアキュムレータへ畳み込み、新しい合計の整数部を PTY 書き込みを
駆動するノッチ数として消費し (`bounded_wheel_report_duplicate`、`pointer_routing.rs:946`)、
残った端数を書き戻す。符号は `accumulate_alt_scroll_lines` が保つのと同一の方法で両方向とも
保存される (合計が非負なら floor、負なら ceil、`input_translate.rs:342-347`)。したがって
方向反転はゼロからのやり直しではなく、保持された端数と相殺される。ホイールイベントのレポート方向
(`MouseEventKind::WheelUp` / `MouseEventKind::WheelDown`、`pointer_routing.rs:905-909` で決定)
は、実際に消費されたノッチ数の符号と一致していなければならない。

**ステータス**: resolved

#### FR3: Reject a non-finite delta before mutating the accumulator

**説明**:
非有限の `lines` 値はアキュムレータを変更する前に棄却する (早期 return、書き込みなし、レポートなし)。
これによりアキュムレータが NaN に至ることはない (そうでなければ inf に続く -inf が NaN を生み、
以降すべての比較が false となってレポート経路が恒久的に沈黙する)。質問
`report-accum.clamp-non-finite` (選択肢 `guard_before_accumulate`) の回答による決定。
`wheel_report_notches` 内 (`input_translate.rs:473-475`) および
`alternate_scroll_wheel_bytes` 内 (`input_translate.rs:367-369`) の既存の非有限早期 return は
そのまま維持する。本ガードはそれらの置き換えではなく、累積ステップの手前に置かれる。

**ステータス**: resolved

#### FR4: Saturate the accumulated notch magnitude while still a float

**説明**:
アキュムレータから消費される絶対値は、浮動小数点値のまま、浮動小数点から整数へのキャストより前に
`MAX_WHEEL_REPORT_NOTCHES` (`input_translate.rs:457`) で飽和させる。これは
`wheel_report_notches` が既に文書化し適用している順序と同一である (`input_translate.rs:464-477`)。
クランプしていない巨大な絶対値を `i32` へキャストすると `i32::MAX` で飽和し、キャストから後続の
クランプまでの区間で上限の事後条件が偽になるためである。`bounded_wheel_report_duplicate`
(`pointer_routing.rs:737-744`) は同じ定数を参照する独立した第二のクランプ層として残り、
どちらか一方の層を個別に取り除いても全体の上限は保たれる (task0001 D2)。質問
`report-accum.clamp-non-finite` (選択肢 `guard_before_accumulate`) の回答による決定。

**ステータス**: resolved

#### FR5: Reset the accumulator on tab change and on tracking release

**説明**:
レポート用アキュムレータはアクティブタブが変化したとき、およびマウストラッキングが解除されたときに
ゼロにする。これにより保持中の端数がジェスチャー境界やタブ境界を越えることはない。リセットの判断は
`decide_wheel_event` が既に算出している観測値から取る。すなわち
`tracking_active = mode_1000 || mode_1002 || mode_1003`、
`tab_changed = records.built_for_tab != Some(active_tab)`、
`reset = !tracking_active || tab_changed` (`src-tauri/src/window_host/mouse_report.rs:981-983`)
であり、これは outcome の `RecordUpdates.reset` (`mouse_report.rs:1007-1014`) に載って伝わる。
ホスト側で再導出してはならない。推奨されるシーム: 既存の `RecordUpdates` シームに合流し、
アキュムレータを `MouseReportRecords` (`mouse_report.rs:421-426`、
`WindowHost::mouse_report_records` / `set_mouse_report_records` により往復、`mod.rs:494-507`)
の内部に持たせる。そうすれば `apply_outcome` の既存の `if outcome.updates.reset` 分岐が
`cell_cache.reset()` および `gesture_owner.clear_all()` と並べてゼロ化する
(`mouse_report.rs:1027-1030`)。ホスト側の別リセットは `outcome.updates.reset` で駆動される場合に
限り許容される。`apply_outcome` は `mouse_report.rs:1031-1033` で `records.built_for_tab` を
上書きするため、その実行後に `tab_changed` を再計算することはできない。順序は本質的である。
リセットは新しいデルタを畳み込む前に適用しなければならず、ホイールハンドラの既存の呼び出し順
(`apply_outcome` が `pointer_routing.rs:940`、ノッチ導出が `:943`) がそれを満たしている。
FR6 との関係: 棄却ノッチ分岐は `RecordUpdates::default()` (reset は false、built_for_tab は None、
`mouse_report.rs:976-979`) を返すため、このシームに乗ることで追加コードなしに棄却ノッチ不変性が
保たれる。なお観測はポインタイベント時にサンプリングされるものであってラッチされない。モードは
イベントごとにアクティブタブの core から読まれ (decision D2、`pointer_routing.rs:898-901`)、
ホスト側にミラーされないため、「トラッキングを解除して、間にホイールイベントを挟まずに再度有効化する」
ケースはリセットだけではカバーされない。AC-7 の「再有効化はゼロから始まる」半分を満たすには、
非アクティブからアクティブへの遷移を観測する必要がある (たとえば `built_for_tab` の隣に直近の
トラッキング有効状態を `MouseReportRecords` に持たせる)。

**ステータス**: resolved

#### FR6: A rejected notch neither advances nor resets the accumulator

**説明**:
グリッド所有権ゲートが棄却したホイールイベント (`point_belongs_to_grid` が false、
`mouse_report.rs:971-980`) は、アキュムレータをそのままの値で残す。デルタによって進むことも、
リセットされることもない。これにより当該分岐が `RecordUpdates::default()` を返すことで文書化して
いるレコード更新の不変性が保たれる。棄却分岐の処理 (`rejected_wheel_disposition`、
`mouse_report.rs:684-`) は変更しない。

**ステータス**: resolved

#### FR7: Keep the alternate-scroll accumulator separate and untouched

**説明**:
レポート用アキュムレータは `WindowHost::alt_scroll_accum` (`mod.rs:237`、初期化は `mod.rs:473`)
とは別のストアとする。`alt_scroll_accum` の挙動は変更しない。これには両方の分岐における画面切り替え
リセット `if !app.alt_screen { host.alt_scroll_accum = 0.0; }`
(`pointer_routing.rs:971-973` および `:980-982`)、トラッキング有効時の Shift+ホイール分岐における
意図的な非変更 (`pointer_routing.rs:954-962`)、およびそれが調整されている定数
`MAX_ALT_SCROLL_NOTCHES` (`input_translate.rs:332`) が含まれる。この定数が
`MAX_WHEEL_REPORT_NOTCHES` の別名とされることは決してない (task0001 D3)。

**ステータス**: resolved

## 5. 非機能要件

### 5.1 非機能要件一覧

| ID | 名称 | ステータス |
|----|------|-----------|
| NFR1 | Decision units stay pure | resolved |
| NFR2 | Bounded work and allocation per wheel event | resolved |
| NFR3 | Two clamp layers, one constant | resolved |
| NFR4 | Testable without a window or event loop | resolved |

### 5.2 パフォーマンス要件

**NFR2 - Bounded work and allocation per wheel event**:
1回のホイールイベントが行うアキュムレータ処理は O(1) であり、ペイロードの繰り返しは
多くとも `MAX_WHEEL_REPORT_NOTCHES` 回である。事前確保サイズと繰り返し上限はいずれも
単一のクランプ済み値から導出され、両者が乖離し得ないようにする
(task0001 D5、`pointer_routing.rs:738-742`)。

### 5.3 セキュリティ要件

**NFR3 - Two clamp layers, one constant**:
2つのクランプ層はいずれも同一の `MAX_WHEEL_REPORT_NOTCHES` 定数を参照し、どちらも
`MAX_ALT_SCROLL_NOTCHES` の別名でも、そこから導出された値でもない。

入力検証は FR3 (非有限デルタの棄却) と FR4 (浮動小数点のままでの飽和) が規定する。

### 5.4 可用性要件

requirements_analysis に稼働率・障害復旧時間の要件は含まれない。関連する堅牢性要件は
FR3 (非有限デルタによる恒久的沈黙の防止) が規定する。

### 5.5 保守性要件

**NFR1 - Decision units stay pure**:
`decide_wheel_event` は平坦な入力に対する純関数であり続ける (SC-10 property 3)。レコードを変更せず、
レコードの変更はすべて返り値の `SequenceOutcome.updates` に載って伝わる
(`mouse_report.rs:507-528`)。デルタの絶対値はこの関数からは見えない (`WheelEventInputs`
(`mouse_report.rs:592-`) は `lines` ではなく `kind` のみを持つ) ため、累積と消費のステップは
シームのホスト側に留まり、リセットは outcome に載る。

**NFR4 - Testable without a window or event loop**:
累積 / クランプ / リセットのロジックは、`WindowHost` も winit イベントループも GPU サーフェスも
使わずに、素の値として検証できる。これは既存の `src-tauri/src/window_host/tests.rs` および
`mouse_report.rs` / `input_translate.rs` のインライン `mod tests` ブロックのスタイルに一致する。

### 5.6 互換性要件

既存の全ノッチ挙動は不変である (AC-9)。alternate-scroll 経路も不変である (AC-10、FR7)。

## 6. UI/UX要件

デザインステップはスキップされた。理由: 変更は `src-tauri/src/window_host/` 配下のホイール入力
ルーティングに限定され、UI サーフェスを追加せず、デザイントークンにも触れず、既存のスクロール
ジェスチャーの応答性以外にユーザーが目にするものを変えない
(ゲート `create-spec.design-step`、選択肢 `decide_autonomously`、推奨 "skip" の受理)。

## 7. データ要件

永続化データの追加は requirements_analysis に含まれない。アキュムレータの保持先については
FR5 (推奨シームは `MouseReportRecords`) および FR7 (`alt_scroll_accum` とは別ストア) を参照。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約

- トラッキングモードのビットはポインタイベントごとにアクティブタブの core から読まれ
  (decision D2、`pointer_routing.rs:898-901`)、ホスト側にミラーされない。したがって
  トラッキングモードの遷移は次のポインタイベントでしか観測できない (A-4)。
- `apply_outcome` は `mouse_report.rs:1031-1033` で `records.built_for_tab` を上書きするため、
  その実行後に `tab_changed` を再計算することはできない (FR5)。
- `WheelEventInputs` は `kind` のみを持ち `lines` を持たないため、デルタの絶対値は
  `decide_wheel_event` からは見えない (NFR1)。

### 9.2 ビジネス上の制約

requirements_analysis に該当情報は含まれない。

### 9.3 スケジュール制約

requirements_analysis に該当情報は含まれない。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/wheel-report-fraction-accum/**`
- `test-docs/wheel-report-fraction-accum/**`

`feature-docs/wheel-report-fraction-accum/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/wheel-report-fraction-accum/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/wheel-report-fraction-accum/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/wheel-report-fraction-accum/` ディレクトリを生成しないが、宣言された `test-docs/wheel-report-fraction-accum/**` は依然として正しい。

### 9.5 前提条件

| ID | 前提 | 可逆 |
|----|------|------|
| A-1 | `wheel_report_notches` の全ノッチ意味論は維持される。既存のユニットテスト (`src-tauri/src/window_host/tests.rs:1499-1580`) が固定している。 | true |
| A-2 | 二層クランプ (変換側 + 複製側、共有定数1つ) は1層に統合せず維持する。`report-accum.clamp-non-finite` の回答が `bounded_wheel_report_duplicate` を独立した第二層として残すと述べている。 | true |
| A-3 | alternate-scroll 経路は自前のアキュムレータと自前の定数を保持する。レポート用アキュムレータは `alt_scroll_accum` とストアを共有せず、`MAX_ALT_SCROLL_NOTCHES` の別名も用いない。 | true |
| A-4 | トラッキングモードのビットはポインタイベントごとにアクティブタブの core から読まれ (decision D2、`pointer_routing.rs:898-901`)、ホスト側にミラーされないため、トラッキングモードの遷移は次のポインタイベントでのみ観測可能である。FR5 はこの制約を前提に記述されている。 | true |
| A-5 | `alt_scroll_accum` が行う画面切り替えリセット — `pointer_routing.rs:971-973` および `:980-982` の `if !app.alt_screen { host.alt_scroll_accum = 0.0; }` によるゼロ化 — はレポート用アキュムレータへ意図的にミラーしない。alternate screen の出入りだけでレポート経路の端数が破棄されることはなく、その挙動は本フィーチャーのスコープ外である。これは FR5 のタブ変更・トラッキング解除リセット (スコープ内かつ必須) とは別の関心事であり、そちらには何ら影響しない。 | true |
| A-6 | レポートは `app.active` ではなく outcome が指し示すタブへ書き込まれる。したがって端数は特定のタブに帰属し、FR5 のリセットがタブ間の寄与を防ぐ唯一の機構である。 | true |

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| ガードなしにアキュムレータへ値を畳み込むと、inf に続く -inf が NaN を生み、以降すべての比較が false になってレポート経路が恒久的に沈黙する | 高 | FR3: 累積の手前で非有限デルタを棄却する (早期 return、書き込みなし) |
| クランプしていない巨大な絶対値を `i32` へキャストすると `i32::MAX` で飽和し、上限の事後条件が偽になる | 高 | FR4: 浮動小数点のまま、キャストの手前で `MAX_WHEEL_REPORT_NOTCHES` により飽和させる |
| `apply_outcome` が `records.built_for_tab` を上書きするため、その後では `tab_changed` を再計算できない | 中 | FR5: リセットは `outcome.updates.reset` に載せ、ホスト側で再導出しない |
| トラッキングモードがホスト側にミラーされないため、「解除→再有効化」の遷移がリセットだけでは捕捉されない | 中 | FR5: 直近のトラッキング有効状態を `MouseReportRecords` に持たせるなど、非アクティブ→アクティブ遷移を観測する |
| `bounded_wheel_report_duplicate` は繰り返し回数を制限できるが、汚染されたアキュムレータを浄化できない | 中 | FR4 / NFR3: 2つのクランプ層をいずれも同一定数参照のまま維持する |

### 10.2 ビジネスリスク

requirements_analysis に該当情報は含まれない。

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC-1: 同一方向の1ノッチ未満デルタの連続で合計が 1.0 に達したとき、その到達イベントで
  ちょうど1ノッチ分のレポートバイトが生成される。その連続の中でそれ以前のイベントは PTY 書き込みを
  一切生じない。(FR1, FR2)
- [ ] AC-2: 端数はイベントをまたいで保持され、符号が保存される。方向反転はゼロからのやり直しではなく
  保持された端数と相殺され、送出されるレポート方向は消費されたノッチ数の符号と一致する。(FR2)
- [ ] AC-3: 非有限デルタ (NaN、+inf、-inf) はレポートを生成せず、アキュムレータをバイト単位で
  同一のまま残す。後続の有限デルタは、その非有限イベントが到着しなかった場合とまったく同じに
  振る舞う。特に inf に続く -inf がレポート経路を沈黙させることはない。(FR3)
- [ ] AC-4: あらゆる f32 デルタと、到達可能なあらゆるアキュムレータ状態に対して、複製ステップへ
  渡されるノッチ数の絶対値は高々 `MAX_WHEEL_REPORT_NOTCHES` であり、浮動小数点から整数への
  キャストの手前で、浮動小数点のまま飽和されている。(FR4)
- [ ] AC-5: `bounded_wheel_report_duplicate` は独立して上限を課し続ける。上限を超える
  `requested_count` を与えても、ちょうど `MAX_WHEEL_REPORT_NOTCHES` 回の繰り返しを出力し、
  それ以上の確保を行わない。(FR4, NFR3)
- [ ] AC-6: タブ A がアクティブな間に蓄積された端数は、アクティブタブが B になった後に送出される
  いかなるレポートにも寄与しない。B 上の最初のホイールイベントは、アキュムレータがゼロだった場合と
  同じバイトをレポートする。(FR5)
- [ ] AC-7: トラッキングが有効な間に蓄積された端数はトラッキング解除とともに破棄され、トラッキングを
  再度有効化すると累積はゼロから始まる。新しいトラッキングセッションの最初のホイールイベントは、
  アキュムレータがゼロだった場合と同じバイトをレポートする。(FR5)
- [ ] AC-8: グリッド所有権ゲートが棄却したホイールイベントは、何も進めず何もリセットしない。
  アキュムレータと `MouseReportRecords` 値全体はイベントをまたいで不変であり、outcome の updates は
  `RecordUpdates::default()` と等しいままである。(FR6)
- [ ] AC-9: 全ノッチのレポート挙動は不変である。既存の `wheel_report_notches` の期待
  (符号付き整数カウント、1ノッチ未満はゼロ、非有限はゼロ、上限境界、上限スイープ) が引き続き成立する。
  (FR1, FR4)
- [ ] AC-10: alternate-scroll 経路は不変である。`alt_scroll_accum` は画面切り替えリセット、
  トラッキング有効時の Shift+ホイール分岐での非変更、自前の `MAX_ALT_SCROLL_NOTCHES` 上限を保つ。
  (FR7)

### 11.2 KPI

requirements_analysis に KPI は含まれない。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] TS-1: 1ノッチ未満の累積による到達 — 0.4、0.4、0.4 を与え、最初の2回はレポートなし、
  3回目でちょうど1ノッチとなることを確認する。(AC-1)
- [ ] TS-2: 端数の保持と反転 — 1.5 の後 (1ノッチ消費、0.5 保持)、-0.7 を与えて合計が -0.2 となり
  ノッチが出ないことを確認し、続けて -0.9 を与えてちょうど1ノッチ下方向が出ることを確認する。(AC-2)
- [ ] TS-3: 非有限ガード — 既知の非ゼロアキュムレータから NaN、+inf、-inf を順に与え、各回でレポートなし・
  アキュムレータ不変であること、および続く有限の 1.0 が依然としてノッチを出すことを確認する。(AC-3)
- [ ] TS-4: 上限飽和スイープ — `f32::MAX`、`f32::MIN`、100.0、100.999、101.0 とそれらの負値、および
  上限近傍に事前ロードしたアキュムレータを与え、すべての場合で
  `|notches| <= MAX_WHEEL_REPORT_NOTCHES` を確認する。(AC-4)
- [ ] TS-5: 独立した複製上限 — `bounded_wheel_report_duplicate(payload, u32::MAX)` がちょうど
  `MAX_WHEEL_REPORT_NOTCHES` 回の繰り返しを返すことを確認する。(AC-5)
- [ ] TS-6: タブ変更リセット — `built_for_tab = Some(A)` で 0.9 を蓄積し、`active_tab = B` で
  ホイールイベントを判定する。outcome が reset true と `built_for_tab` `Some(B)` を運ぶこと、
  適用によりアキュムレータがゼロになること、B のイベントで書かれるバイトがアキュムレータゼロでの
  0.9 デルタと同じ (すなわち書き込みなし) であることを確認する。(AC-6)
- [ ] TS-7: トラッキング解除リセットと再有効化 — トラッキングモード有効の状態で 0.9 を蓄積し、
  全トラッキングモードが off のホイールイベントを観測して outcome が reset true を運びアキュムレータが
  ゼロになることを確認する。続いて同じタブでトラッキングを再有効化し、新セッション最初のホイール
  イベントがゼロから累積すること (「間にホイールイベントを挟まない解除→再有効化」の順序を含む) を
  確認する。(AC-7)
- [ ] TS-8: 棄却ノッチ不変性 — `point_belongs_to_grid` が false のとき、`decide_wheel_event` が
  `updates == RecordUpdates::default()` を返すこと、および本来ノッチ境界を越えるはずのデルタに対して
  outcome の適用がアキュムレータと `MouseReportRecords` の残り全体をビット単位で不変に保つことを
  確認する。(AC-8)
- [ ] TS-9: 全ノッチ回帰ガード — 既存の `wheel_report_notches` テスト
  (`src-tauri/src/window_host/tests.rs:1499-1580`) が変更なしで通ることを確認する。(AC-9)
- [ ] TS-10: alternate-scroll 経路の不変性 — `accumulate_alt_scroll_lines` が `(whole, frac)` の
  契約を保ち、トラッキング有効時の Shift+ホイール分岐が依然として `alt_scroll_accum` に触れないことを
  確認する。(AC-10)

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| notch | レポート経路が PTY へ書き出す1単位のホイール刻み。`wheel_report_notches` が算出し、`bounded_wheel_report_duplicate` が複製する |
| `MAX_WHEEL_REPORT_NOTCHES` | レポート経路のノッチ上限定数 (`input_translate.rs:457`)。2つのクランプ層が共に参照する |
| `MAX_ALT_SCROLL_NOTCHES` | alternate-scroll 経路が調整されている上限定数 (`input_translate.rs:332`)。`MAX_WHEEL_REPORT_NOTCHES` の別名ではない |
| レポート用アキュムレータ | 本フィーチャーが追加する、レポート経路専用の端数保持ストア |
| `alt_scroll_accum` | alternate-scroll 経路の既存アキュムレータ (`mod.rs:237`) |
| tracking_active | `mode_1000 \|\| mode_1002 \|\| mode_1003` (`mouse_report.rs:981-983`) |
| tab_changed | `records.built_for_tab != Some(active_tab)` (`mouse_report.rs:981-983`) |

## 14. 確認事項

### 14.1 確認済み事項

- [x] `report-accum.clamp-non-finite` (選択: `guard_before_accumulate`):
  `wheel_report_accum` を変更する前に非有限の `lines` を棄却し (早期 return、書き込みなし)、
  累積されたノッチ絶対値は浮動小数点から整数へのキャストの手前で、浮動小数点のまま
  `MAX_WHEEL_REPORT_NOTCHES` で飽和させる。`bounded_wheel_report_duplicate` は独立した
  第二のクランプ層として残す。
- [x] `design-step.recommendation` (選択: `decide_autonomously`):
  requirements-analyst の design_step_recommendation "skip" を受理する。

### 14.2 未確認・保留事項

- なし (`status: tbd` の要件は存在しない)。

## 15. 参考資料

- `src-tauri/src/window_host/input_translate.rs`: `wheel_report_notches`、
  `accumulate_alt_scroll_lines`、`alternate_scroll_wheel_bytes`、
  `MAX_WHEEL_REPORT_NOTCHES`、`MAX_ALT_SCROLL_NOTCHES`
- `src-tauri/src/window_host/pointer_routing.rs`: ホイールハンドラ、
  `bounded_wheel_report_duplicate`、alternate-scroll 分岐
- `src-tauri/src/window_host/mouse_report.rs`: `decide_wheel_event`、`SequenceOutcome`、
  `RecordUpdates`、`MouseReportRecords`、`apply_outcome`、`rejected_wheel_disposition`
- `src-tauri/src/window_host/mod.rs`: `alt_scroll_accum`、`mouse_report_records` /
  `set_mouse_report_records`
- `src-tauri/src/window_host/tests.rs`: 既存の `wheel_report_notches` テスト (1499-1580)
- `feature-docs/wheel-report-fraction-accum/SPEC.md`
