---
title: "mouse-report-guard-local-arms"
created_date: 2026-09-20
status: draft
---

# mouse-report-guard-local-arms - 要件定義書

> Note: 本書は em-workflow の要件定義テンプレート（章立ては固定）に従う。
> 記載されている要件・受け入れ基準・テストシナリオ・前提はすべて
> requirements-analyst が確定した解決済み要件の描画であり、本書で新たに
> 起草したものは無い。コード識別子・ファイルパス・制御シーケンス名は英語のまま表記する。

## 1. 概要

### 1.1 背景

mouse-reporting フィーチャー（PR #69、main にマージ済み）が導入した SC-8
グリッド所有権ガード `point_belongs_to_grid` は、グリッドが所有しない位置の
イベントを「一律の拒否アーム」で処理する。3 つの SC-10 決定ユニットはいずれも
`if !point_belongs_to_grid(inputs.grid) { return SequenceOutcome::nothing(); }`
で始まり、`Disposition::Nothing` を返す（CF3）。perform 段はこの `Nothing` を
`None => {}` で捨てるため、chrome ガード領域で本来動いていたローカル動作が
そこで消える。

結果として、ホイールのスクロールバック移動と中クリックの PRIMARY ペーストが、
一部の領域で黙って失われている。mouse-reporting フィーチャー自身の AC12
（「レポートを出さず、現在のローカル動作をそのまま保持する」）および
IMPLEMENTATION.md の D11「Boundary」段落が明記した契約（CF7）に反した状態である。

この退行がレビューと CI を通過した機械的な理由も特定されている。
`ac3_ts19_guarded_regions_reject_every_event_kind_and_button`
（`mouse_report.rs:1721-1811`）は 6 領域 × 3 ボタン × 5 イベント種別を
掃くが、`dest.is_empty()` しか検証しない。`Nothing` も `Local(..)` も
バイトを追加しないため、この表明は両者を区別できない（CF5）。

### 1.2 目的

- chrome ガード領域すべてについて、SC-8 ガードが黙って取り除いたローカル
  ポインタ動作を復旧する。ホイールのノッチは再びスクロールバックを動かし、
  中クリックは再び PRIMARY をペーストする（BO1）。
- 退行を通してしまったテストギャップを閉じる。ガード領域テストが領域ごとの
  ローカルアーム同一性を固定するようにする（BO2）。
- 「一律の拒否アーム（上流の早期 return があるため無害）」という現在の設計を、
  領域ごとの明示的な決定に置き換える。将来 chrome ガードの順序が変わっても、
  タブバーの横スクロール・mux サイドバーのリストスクロール・プロファイル
  セレクタのリストスクロールがターミナルのスクロールに化けないようにする（BO3）。

### 1.3 スコープ

**対象範囲（In scope）**

- SC-8 が拒否した位置に対する、ホイール決定のローカルアーム復旧（FR1、FR2）。
- SC-8 が拒否した位置に対する、中ボタン押下のローカルアーム復旧（FR3）。
- 左・右ボタン押下、モーション、ジェスチャー所有者記録、`RecordUpdates` の
  現状維持の明文化（FR4、FR5、FR6、FR8）。
- レポート禁止の維持（FR7）。
- `GridOwnershipInputs.in_top_strip` の 2 つの領域 bool への分解（FR9）。
- 領域ごとのディスパッチと、領域が重なった場合の優先順位規定（FR10）。

**対象外（Out of scope）**

- 新しい `Disposition` バリアントおよび新しい `LocalArm` バリアントの追加（NFR2）。
- `handle_pointer_button` / `handle_mouse_wheel` のガード順序の変更、ガードの
  追加・削除（NFR3）。
- モーション経路のローカルアーム復旧（FR6：`handle_pointer_moved` が決定の前に
  ローカル処理を済ませているため、アームを与えると二重実行になる）。
- E2E ハーネスの導入（本リポジトリに E2E ハーネスは存在せず、導入もしない。
  `test/README.md` "E2E Tests: None at the moment."）。
- デザインステップ（第 6 章を参照）。

## 2. ビジネス要件

### 2.1 ビジネス目標

1. **BO1**: mouse-reporting の SC-8 グリッド所有権ガードが黙って取り除いた
   ローカルポインタ動作を、chrome ガード領域すべてで復旧する。ホイールの
   ノッチは再びスクロールバックを動かし、中ボタン押下は再び PRIMARY を
   ペーストする。これにより mouse-reporting フィーチャー自身の AC12
   （「レポートを出さず、現在のローカル動作をそのまま保持する」）が実際に成立する。
2. **BO2**: 退行を通してしまったテストギャップを閉じる。ガード領域のテストは
   「レポートバイトが生成されなかったこと」しか表明しておらず、
   `Disposition::Nothing` と名前付きローカルアームを区別できない。本変更後は
   テストが領域ごとのローカルアーム同一性を固定する。
3. **BO3**: 現在の「一律の拒否アーム 1 つ、上流の早期 return が到達不能に
   しているので無害」という設計を、領域ごとの明示的な決定に置き換える。
   将来 chrome ガードの順序が変わっても、タブバーの横スクロール・mux
   サイドバーのリストスクロール・プロファイルセレクタのリストスクロールが
   ターミナルのスクロールに化けないようにする。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| eMterm のローカルポインタ動作を使う利用者 | ステータスバー・スクロールバーオーバーレイ・CSD タイトルバー・リサイズ帯の上でホイールを回してスクロールバックを動かす、あるいは中クリックで PRIMARY をペーストする利用者。現在これらの動作が黙って失われている。 |
| egui chrome を操作する利用者 | タブバーの横スクロール、mux サイドバーのウィンドウリストスクロール、プロファイルセレクタのリストスクロールを使う利用者。これらがターミナルのスクロールに化けないことが求められる。 |
| `window_host` のポインタ経路を保守する開発者 | 領域ごとの決定とそれを固定するテストによって、ガード順序の変更が黙って動作を壊さないことを求める開発者。 |

### 2.3 期待される効果

- chrome ガード領域上のホイール・中クリックが退行前と同じローカル動作に戻る。
- ローカルアームの同一性がテストで固定され、どれか 1 つでも `Nothing` に
  戻すとテストスイートが落ちる（AC16）。
- 領域ごとの決定が明示されることで、ガード順序の将来の変更が egui 側の
  スクロール動作を黙ってターミナルスクロールに化けさせられなくなる。

## 3. ユースケース

> ユースケースはビジネス目標と受け入れ基準からの描画であり、解決済み要件に
> 優先度の指定が無いため優先度欄は未設定（`—`）とする。

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | chrome 領域の上でホイールを回してスクロールバックを動かす | ローカルポインタ動作を使う利用者 | — |
| UC02 | chrome 領域の上で中クリックして PRIMARY をペーストする | ローカルポインタ動作を使う利用者 | — |
| UC03 | egui chrome 自身のスクロール操作を行う | egui chrome を操作する利用者 | — |

### 3.2 ユースケース詳細

#### UC01: chrome 領域の上でホイールを回してスクロールバックを動かす

**アクター**: eMterm のローカルポインタ動作を使う利用者

**事前条件**:
- スクロールバックに履歴がある。
- ポインタが CSD タイトルバー帯、下端ステータスストリップ、右端スクロールバー
  オーバーレイ、CSD リサイズホットゾーンのいずれかの上にある。

**基本フロー**:
1. 利用者がホイールを 1 ノッチ回す。
2. `handle_mouse_wheel` の既存の早期 return はこれらの領域を消費しないため、
   イベントは `decide_wheel_event` に到達する（CF1）。
3. `point_belongs_to_grid` が false を返す。
4. `decide_wheel_event` は `wheel_consumer(tracking_active = false, ...)` を
   呼び、その結果を `Local(LocalArm::ScrollScrollback)` または
   `Local(LocalArm::TranslateToArrowBytes)` に写す（FR1）。
5. perform 段がそのアームを実行し、スクロールバックが動く。

**代替フロー**:
- 代替画面かつ DECSET 1007 のモードビットと `alternate_scroll_enabled` が
  ともに有効な場合、アームは `Local(LocalArm::TranslateToArrowBytes)` になる（AC5）。
- トラッキングモードが有効かどうかにかかわらず結果は同一（NFR5）。

**事後条件**:
- スクロールバックが移動し、レポートバイトは 1 バイトも生成されない（AC13）。

#### UC02: chrome 領域の上で中クリックして PRIMARY をペーストする

**アクター**: eMterm のローカルポインタ動作を使う利用者

**事前条件**:
- `middle_click_paste` が有効で、PRIMARY に文字列が入っている。
- ポインタが下端ステータスストリップ、スクロールバーオーバーレイ、mux
  サイドバー、上部ストリップと重ならない CSD リサイズホットゾーンのいずれかの
  上にある。

**基本フロー**:
1. 利用者が中ボタンを押下する。
2. `handle_pointer_button` の該当ガードは `button == Left && state == Pressed`
   条件付きであり中押下を消費しないため、イベントは `run_button_decision` →
   `decide_press` に到達する（CF2）。
3. `point_belongs_to_grid` が false を返す。
4. `decide_press` が `Local(LocalArm::PastePrimary)` を名指しする（FR3）。
5. perform 段がペーストを実行する。

**代替フロー**:
- `middle_click_paste_enabled` が false の場合、どの領域でも `Nothing`（AC10）。
- CSD タイトルバー帯・タブバー帯・プロファイルセレクタ表示中は `Nothing`。
  これら 3 領域では退行前も中押下が `decide_press` に到達していなかったため、
  アームを与えることは修復ではなく新規動作の追加になる（FR3、CF2）。

**事後条件**:
- PRIMARY の内容がペーストされ、ジェスチャー所有者は記録されない（FR5、AC12）。

#### UC03: egui chrome 自身のスクロール操作を行う

**アクター**: egui chrome を操作する利用者

**事前条件**:
- ポインタがタブバー帯、mux サイドバーの上にある、またはプロファイル
  セレクタが表示されている。

**基本フロー**:
1. 利用者がホイールを回す。
2. `handle_mouse_wheel` の既存の早期 return が `egui::Event::MouseWheel` を
   push して return する（CF1）。
3. egui 側のスクロール（タブストリップの横スクロール、サイドバーの
   ウィンドウリスト、モーダルのリスト）が動く。

**代替フロー**:
- 仮にガード順序が変わってイベントが `decide_wheel_event` に到達した場合でも、
  これら 3 領域の決定は `Disposition::Nothing` であり、ターミナルのスクロールは
  起きない（FR2、FR10）。

**事後条件**:
- egui 側のスクロールのみが起き、ターミナルのスクロールバックは動かない（AC6、AC7）。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | 破損したガード領域上のホイールが退行前のホイール消費者を名指しする | 拒否位置のホイール決定を `wheel_consumer(tracking_active = false, ...)` に委ね、その結果をローカルアームに写す。 | — |
| FR2 | 上流の早期分岐が消費する領域はホイール経路で `Nothing` を保つ | タブバー帯・mux サイドバー・プロファイルセレクタ表示中は `Disposition::Nothing`。 | — |
| FR3 | ガード領域上の中押下は、退行前に到達していた 4 領域で PRIMARY をペーストする | 下端ストリップ・スクロールバーオーバーレイ・mux サイドバー・上部ストリップと重ならないリサイズホットゾーンで `Local(LocalArm::PastePrimary)`。 | — |
| FR4 | ガード領域上の左・右押下はローカルアームを名指ししない | 左押下・右押下ともに全領域で `Nothing`。 | — |
| FR5 | 拒否された押下はジェスチャー所有者を記録しない | `updates.gesture == None` を維持する。 | — |
| FR6 | ガード領域上のモーションは `Nothing` のまま | `decide_motion_event` の拒否アームは変更しない。 | — |
| FR7 | 拒否位置は決してレポートしない | 3 経路のいずれでも `Disposition::Report` を生成しない。 | — |
| FR8 | 拒否イベントのレコード更新は現状のまま | `updates == RecordUpdates::default()` を維持する。 | — |
| FR9 | `GridOwnershipInputs.in_top_strip` を 2 つの領域 bool に分解する | CSD タイトルバー帯用と タブバー帯用の独立した bool にする。 | — |
| FR10 | 領域ごとのディスパッチと、重なり領域の優先順位の明示 | 抑制側の領域が勝つ順序を規定する。 | — |

優先度は解決済み要件に指定が無いため、ここでは割り当てず未設定（`—`）とする。

### 4.2 機能詳細

#### FR1: 破損したガード領域上のホイールが退行前のホイール消費者を名指しする

**ステータス**: resolved

**説明**: `point_belongs_to_grid` が false のとき、`decide_wheel_event` は
`wheel_consumer(tracking_active = false, inputs.mods.shift, inputs.on_alt_screen,
inputs.alt_scroll_mode_bit, inputs.alt_scroll_setting)` を呼んで disposition を
決定し、`TranslateToArrows` → `Local(LocalArm::TranslateToArrowBytes)`、
`ScrollScrollback` → `Local(LocalArm::ScrollScrollback)` に写す。
`tracking_active` は無条件に `false` を渡す。当該領域はグリッドが所有しないため、
トラッキング中のアプリケーションはこのノッチに対する権利を持たない。したがって
この分岐では `ReportToApplication` は到達不能である。固定の `ScrollScrollback` は
明示的に却下する。代替画面の矢印変換が壊れるためである（CF6）。

**入力**: `inputs.mods.shift`、`inputs.on_alt_screen`、`inputs.alt_scroll_mode_bit`、
`inputs.alt_scroll_setting`、および固定値 `tracking_active = false`。

**出力**: `Disposition::Local(LocalArm::ScrollScrollback)` または
`Disposition::Local(LocalArm::TranslateToArrowBytes)`。

**ビジネスルール**:
- `tracking_active` は常に `false`。
- `ReportToApplication` はこの分岐から返らない。

**関連受け入れ基準**: AC1、AC2、AC3、AC4、AC5

**関連テストシナリオ**: TS1、TS3、MS1

#### FR2: 上流の早期分岐が消費する領域はホイール経路で `Nothing` を保つ

**ステータス**: resolved

**説明**: FR1 のホイールアームは、ホイールイベントが実際に
`decide_wheel_event` に到達する領域にだけ与える。すなわち CSD タイトルバー帯、
下端ステータスストリップ、右端スクロールバーオーバーレイ、CSD リサイズ
ホットゾーンの 4 領域である。`handle_mouse_wheel` が既に消費して return して
いる領域 — タブバー帯、mux サイドバー、およびプロファイルセレクタ表示中の
任意位置（CF1）— は `Disposition::Nothing` を決定する。

**関連受け入れ基準**: AC6、AC7

**関連テストシナリオ**: TS1、TS10、MS3

#### FR3: ガード領域上の中押下は、退行前に到達していた 4 領域で PRIMARY をペーストする

**ステータス**: resolved

**説明**: `point_belongs_to_grid` が false、ボタンが `Middle`、
`middle_click_paste_enabled` が true のとき、`decide_press` は下端ステータス
ストリップ、スクロールバーオーバーレイ、mux サイドバー、上部ストリップと
重ならない CSD リサイズホットゾーンに対して
`Local(LocalArm::PastePrimary)` を名指しする。CSD タイトルバー帯、タブバー帯、
プロファイルセレクタ表示中は `Nothing` を名指しする。これら 3 領域では
退行前も中押下が `decide_press` に到達していなかったため（CF2）、
`PastePrimary` を与えることは修復ではなく新規動作になる。
`middle_click_paste_enabled` が false のときはどの領域でも `Nothing`。

構造上の注意点として、押下が到達する領域集合とホイールが到達する領域集合は
一致しない（FR2）。mux サイドバーは押下到達・ホイール非到達、
タイトルバー帯はその逆である。

**ビジネスルール**:
- 押下到達領域集合とホイール到達領域集合は別物として扱う。

**関連受け入れ基準**: AC8、AC9、AC10

**関連テストシナリオ**: TS4、TS5、MS2

#### FR4: ガード領域上の左・右押下はローカルアームを名指ししない

**ステータス**: resolved

**説明**: 拒否位置に対して、`Left` 押下は `Nothing` を決定する。
`BeginSelectionDrag` も `OpenHoveredLink` も決して名指ししない。`Right` 押下も
全領域で `Nothing` を決定する。CSD リサイズ帯上の左押下はネイティブの
ウィンドウリサイズであり、既存の早期ガード（`pointer_routing.rs:429-436`）が
扱う。この順序は変更しない。

**関連受け入れ基準**: AC11

**関連テストシナリオ**: TS6、MS4

#### FR5: 拒否された押下はジェスチャー所有者を記録しない

**ステータス**: resolved

**説明**: SC-8 に拒否された押下の outcome は `updates.gesture == None` を
持つ。`Record(_, GestureOwner::Local)` も `Record(_, GestureOwner::Report)` も
記録しない。押下がローカルアームを名指しするようになった後もこれは変わらない。
`Local` を記録すると対応する左リリースが
`CompleteSelectionAndPublishToPrimary` に流れ、開始されていない選択を完了して
しまう。SC-9 の文書化された事後条件「SC-8 が拒否した押下は所有者を記録しない」を
そのまま保持する。

**関連受け入れ基準**: AC12

**関連テストシナリオ**: TS6

#### FR6: ガード領域上のモーションは `Nothing` のまま

**ステータス**: resolved

**説明**: `decide_motion_event` の拒否アームは変更しない。
`handle_pointer_moved` は決定の実行前に、egui への転送、サイドバーのホバー、
リサイズヒント、リンクホバー、選択ドラッグの延長といったローカル処理を
済ませているため、モーションにローカルアームを与えると、既に行われた処理を
二重に実行することになる。

**関連受け入れ基準**: AC14

**関連テストシナリオ**: TS7

#### FR7: 拒否位置は決してレポートしない

**ステータス**: resolved

**説明**: すべての領域、すべてのボタン同一性、両方のエンコーディング、
すべてのトラッキングモードの組み合わせについて、`point_belongs_to_grid` が
拒否する位置は 3 経路のいずれでも `Disposition::Report` を生成しない。
変わるのは答えのローカル側の半分だけであり、mouse-reporting の FR10 / AC12 が
定めるレポート禁止はそのまま保たれる。

**関連受け入れ基準**: AC13

**関連テストシナリオ**: TS2

#### FR8: 拒否イベントのレコード更新は現状のまま

**ステータス**: resolved

**説明**: 拒否位置の outcome は `updates == RecordUpdates::default()`
（`reset: false`、`built_for_tab: None`、`cache_cell: None`、`gesture: None`）を
保つ。ローカルアームの復旧が変えるのは `disposition` フィールドだけである。
これにより「抑制されたモーションはキャッシュセルを進めない」という SC-4 の
性質（親フィーチャーの AC5 / TS-21）が保たれる。

**関連受け入れ基準**: AC14

**関連テストシナリオ**: TS7

#### FR9: `GridOwnershipInputs.in_top_strip` を 2 つの領域 bool に分解する

**ステータス**: resolved

**説明**: `in_top_strip` を、CSD タイトルバー帯用と タブバー帯用の 2 つの
独立した bool に置き換える。`grid_ownership_inputs`
（`pointer_routing.rs:116-123`）は現在使っているのと同じジオメトリから
これらを埋める。タイトルバー帯 = `position.y < TITLE_BAR_HEIGHT`、
タブバー帯 = `TITLE_BAR_HEIGHT <= position.y < TITLE_BAR_HEIGHT +
effective_tab_bar_height(app.show_tab_bar)` であり、これは
`handle_mouse_wheel` のタブバーガードが使う境界と同一（CF1）なので、
両者が食い違うことはない。`point_belongs_to_grid` は意味と真理値表を
そのまま保つ。いずれかの領域 bool が true、またはプロファイルセレクタが
表示中であれば false を返す。

**入力**: `position.y`、`TITLE_BAR_HEIGHT`、`effective_tab_bar_height(app.show_tab_bar)`。

**出力**: `GridOwnershipInputs` の 2 つの領域 bool。

**関連受け入れ基準**: AC3、AC15

**関連テストシナリオ**: TS8、TS11

#### FR10: 領域ごとのディスパッチと、重なり領域の優先順位の明示

**ステータス**: resolved

**説明**: 拒否時の答えは領域ごとに計算する。「上流ガードが到達不能にして
いるので無害な一律の値」という形は採らない。同じ位置で複数の領域 bool が
true になる場合は、抑制側の領域が勝つ。すなわち、プロファイルセレクタ表示中が
最優先、次にタブバー帯と mux サイドバー、最後にアームを持つ領域である。
これにより実際に存在する重なり（リサイズ帯の上端がタイトルバー帯の内側、
下端がステータスストリップの内側、右端がスクロールバーオーバーレイの内側）に
対して答えが決定的になり、egui が既に消費しているイベントが追加で
ターミナルを動かすことがなくなる。

**ビジネスルール**:
- 優先順位: プロファイルセレクタ表示中 > タブバー帯・mux サイドバー >
  アームを持つ領域。

**関連受け入れ基準**: AC7

**関連テストシナリオ**: TS1、TS10

## 5. 非機能要件

### 5.0 非機能要件一覧

| ID | 名称 | 要約 |
|----|------|------|
| NFR1 | 決定ユニットは純粋関数のまま | プレーン値のみを受け取り返す。決定時にレコードを変更しない。 |
| NFR2 | 新しい `Disposition` バリアントを追加しない | 既存の `Local(..)` と `Nothing` で表現する。 |
| NFR3 | ガード順序と SC-8 の意味を変更しない | 早期 return の順序を並べ替えず、ガードを追加・削除しない。 |
| NFR4 | ポインタホットパスに新たなコストやログを追加しない | 割り当て・ロック取得・ログ行をイベントごとに追加しない。 |
| NFR5 | 修復は mouse-reporting の有効・無効に依存しない | トラッキングの有無にかかわらず同一の動作。 |
| NFR6 | 既存の mouse-reporting の動作を退行させない | 既存の緑のアサーションは TS9 による強化を除きすべて通る。 |

### 5.1 パフォーマンス要件

- **NFR4**: 本変更はポインタイベントごとの割り当て、ロック取得、ログ行を
  一切追加しない。`wheel_consumer` は 5 つの bool に対する純粋な分岐であり、
  領域ディスパッチは入力構造体に既に存在する bool に対する match である。

### 5.2 セキュリティ要件

解決済み要件に本フィーチャーのセキュリティ要件は含まれていない。ここでは
何も規定しない。

### 5.3 可用性要件

解決済み要件に本フィーチャーの可用性要件は含まれていない。ここでは
何も規定しない。

### 5.4 保守性要件

- **NFR1**: `decide_wheel_event`、`decide_button_event` とそのヘルパーは
  SC-10 の性質 3 を保つ。すなわち、シグネチャに winit 型・`term_core` 型・
  ウィンドウハンドル・GPU サーフェス・PTY を一切含まず、プレーン値のみを
  受け取り返し、決定時にはレコードを変更しない（レコード変更は引き続き
  `RecordUpdates` に載せて `apply_outcome` が適用する）。新規テストはすべて、
  ウィンドウもサーフェスも PTY も構築しない素の `#[test]` から実行できること。
- **NFR2**: 拒否位置の答えは既存の `Local(..)` と `Nothing` で表現する。
  レビュアーの代替案 (b)（`Disposition::NotGridOwned` の追加）は採用しない。
  decide / perform の境界をぼかし、「1 つの値が 1 つの具体的な動作を名指しする」
  という SC-10 の契約を弱めるためである。`LocalArm` にもバリアントを追加しない。
- **NFR3**: `handle_pointer_button` と `handle_mouse_wheel` の内部にある
  早期 return の順序は並べ替えず、ガードの削除も追加も行わない。
  `point_belongs_to_grid` は「この点をターミナルグリッドが所有するか」という
  意味と真理値表をそのまま保つ。変わるのは呼び出し側が `false` という答えを
  どう扱うかだけである。
- **NFR6**: `mouse_report.rs` のインラインテストモジュールにある現在緑の
  アサーションはすべて通り続ける。唯一の意図的な例外は、ガード領域の
  アサーションを「バイトが無いこと」から「この厳密な disposition であること」へ
  強化する箇所（TS9）である。Rust スイートは
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path
  src-tauri/Cargo.toml --lib` で実行する。

### 5.5 互換性要件

- **NFR5**: SC-8 はトラッキング有効判定より先に参照されるため、本欠陥は
  トラッキングモードが有効かどうかにかかわらず発生する。復旧後の動作は
  両方の状態で同一でなければならず、テストも両方を網羅する。

## 6. UI/UX要件

### 6.1 画面設計要件

UI/UX 要件は無い。本フィーチャーではデザインステップをスキップする。理由は
解決済み要件が記録したとおり、`window_host` 内部の純粋なイベントルーティング
修復であり、UI 要素・レイアウト・デザイントークン・視覚的サーフェスのいずれにも
触れないためである。

### 6.2 画面遷移

該当なし。画面の追加・変更は無い。

### 6.3 レスポンシブ対応

該当なし。

## 7. データ要件

永続データモデルの追加は無い。本変更が触れる状態は `GridOwnershipInputs` の
フィールド構成（FR9）のみであり、これは決定ユニットへの入力を組み立てるための
ランタイム値である。設定項目は追加しない。

## 8. 外部連携

### 8.1 連携システム

外部システムとの連携は無い。

### 8.2 API仕様要件

プログラム的な API は追加しない。変更は `window_host` 内部の
`GridOwnershipInputs` のフィールド構成（FR9）と、3 つの決定ユニットが
拒否位置に対して返す値に閉じる。

## 9. 制約条件

### 9.1 技術的制約

- 決定ユニットは純粋関数のまま保つ。シグネチャに winit 型・`term_core` 型・
  ウィンドウハンドル・GPU サーフェス・PTY を含めない（NFR1）。
- `Disposition` および `LocalArm` にバリアントを追加しない（NFR2）。
- `handle_pointer_button` / `handle_mouse_wheel` のガード順序を変更しない。
  ガードの追加・削除も行わない（NFR3）。
- ポインタホットパスに割り当て・ロック取得・ログ行を追加しない（NFR4）。
- 本リポジトリに E2E ハーネスは存在せず、導入もしない。自動テストは
  `src-tauri/src/window_host/mouse_report.rs` の既存インライン
  `#[cfg(test)] mod tests` に置く。
- `handle_mouse_wheel` / `handle_pointer_button` は `&mut WindowHost` と
  `&mut App` を取りテストから構築できないため、実 winit イベントから早期ガードを
  経て決定呼び出しに至る配線は手作業でしか確認できない（第 12 章の手動シナリオ）。

### 9.2 ビジネス上の制約

- 本変更は既存仕様への適合回復であり、新しいプロダクト動作を追加しない（CF7）。
- 退行前に `decide_press` に到達していなかった 3 領域（CSD タイトルバー帯、
  タブバー帯、プロファイルセレクタ表示中）に `PastePrimary` を与えない（FR3）。

### 9.3 スケジュール制約

解決済み要件に記録は無い。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/mouse-report-guard-local-arms/**`
- `test-docs/mouse-report-guard-local-arms/**`

`feature-docs/mouse-report-guard-local-arms/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/mouse-report-guard-local-arms/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/mouse-report-guard-local-arms/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/mouse-report-guard-local-arms/` ディレクトリを生成しないが、宣言された `test-docs/mouse-report-guard-local-arms/**` は依然として正しい。

## 10. 想定される課題とリスク

### 10.1 技術的課題

解決済み要件は独立したリスク登録簿を持たない。以下は決定に伴うリスクを持つ
前提の描画であり、可逆性はいずれも「可逆」と記録されている。

| 前提 | 内容 | 可逆性 |
|------|------|--------|
| A1 | キャッシュされた `host.current_resize_dir` が `None` の CSD リサイズホットゾーン内の左押下は、早期リサイズガードを迂回して `decide_press` に到達する。ベースコミット 1d5af0d ではこの押下が選択を開始していたが、FR4 はこれを `Nothing` にする。タスク記述が明示的にそう指示しており、親フィーチャーの FR10 / AC12 とも整合するため、第二の退行ではなく意図した結果として扱う。本変更が退行前と厳密に一致しない唯一の箇所であるため明示する。 | 可逆 |
| A2 | `in_mux_sidebar` と `in_bottom_strip` は現状ジオメトリ的に互いに素であるため、FR10 の優先順位規定は今日の観測可能な動作の変更ではなく将来への備えである。 | 可逆 |
| A3 | `GridOwnershipInputs` は `pub(super)` であり、読み取りスコープの 2 ファイル内では `grid_ownership_inputs`（`pointer_routing.rs:116`）と `mouse_report.rs` 自身のテストでのみ構築される。FR9 のフィールド分割は `window_host/` の他の場所に構築箇所を持たないと仮定する。見落としがあればコンパイラが検出するため、リスクはビルドエラーであり黙った動作変更ではない。 | 可逆 |
| A4 | `scroll_by_wheel_notch` と `handle_mouse_wheel` の perform 段にある `alt_scroll_accum` の帳簿付けは変更不要である。ガード領域から復旧した `ScrollScrollback` アームは、グリッド所有のノッチと同じ `tracking.any_active()` の分岐構造に入る。 | 可逆 |

### 10.2 ビジネスリスク

解決済み要件には、上記の前提以外にビジネスリスクの記録は無い。

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC1 — 下端ステータスバーストリップ上のホイールノッチが `Local(LocalArm::ScrollScrollback)` を名指しし、スクロールバックが動く。トラッキング無効時・有効時のいずれも同じ。(FR1, NFR5)
- [ ] AC2 — 右端スクロールバーオーバーレイ上のホイールノッチが `Local(LocalArm::ScrollScrollback)` を名指しし、スクロールバックが動く。(FR1)
- [ ] AC3 — CSD タイトルバー帯（`y < TITLE_BAR_HEIGHT`）上のホイールノッチが `Local(LocalArm::ScrollScrollback)` を名指しし、スクロールバックが動く。(FR1, FR9)
- [ ] AC4 — 4 つの CSD エッジリサイズホットゾーンのいずれの上でも、ホイールノッチが `Local(LocalArm::ScrollScrollback)` を名指しし、スクロールバックが動く。(FR1)
- [ ] AC5 — 代替画面で DECSET 1007 のモードビットと `alternate_scroll_enabled` がともに有効なとき、AC1〜AC4 の各領域上のホイールノッチは `ScrollScrollback` ではなく `Local(LocalArm::TranslateToArrowBytes)` を名指しする。(FR1, CF6)
- [ ] AC6 — タブバー帯上のホイールノッチが `Disposition::Nothing` を名指しし、タブストリップは `handle_mouse_wheel` の既存の egui 転送によって横スクロールし続ける。ターミナルはスクロールしない。(FR2, FR9)
- [ ] AC7 — mux サイドバー上のホイールノッチが `Nothing` を名指しし（サイドバーのウィンドウリストはスクロールし続ける）、プロファイルセレクタ表示中のホイールノッチが `Nothing` を名指しする（モーダルのリストはスクロールし続ける）。(FR2)
- [ ] AC8 — `middle_click_paste` が有効なとき、下端ステータスストリップ上の中押下が `Local(LocalArm::PastePrimary)` を名指しし、PRIMARY がペーストされる。(FR3)
- [ ] AC9 — `middle_click_paste` が有効なとき、スクロールバーオーバーレイ上、mux サイドバー上、上部ストリップと重ならないリサイズホットゾーン上の中押下が、それぞれ `Local(LocalArm::PastePrimary)` を名指しする。(FR3)
- [ ] AC10 — CSD タイトルバー帯上、タブバー帯上、プロファイルセレクタ表示中の中押下は `Nothing` を名指しする。`middle_click_paste` が無効なときの中押下も同様。(FR3)
- [ ] AC11 — ガード領域上の左押下は `Nothing` を名指しし、`BeginSelectionDrag` も `OpenHoveredLink` も名指ししない。ガード領域上の右押下も `Nothing` を名指しする。(FR4)
- [ ] AC12 — ガード領域上の押下はジェスチャー所有者を記録せず、対応する左リリースはアームを名指しせずバイトも追加しない。特に `CompleteSelectionAndPublishToPrimary` には到達しない。(FR5)
- [ ] AC13 — すべてのガード領域 × すべてのボタン同一性 × すべてのイベント種別 × 両方のエンコーディング × すべてのトラッキングモードの組み合わせで、レポートバイトが生成されない。親フィーチャーの AC12 が定めるレポート禁止は変わらない。(FR7)
- [ ] AC14 — ガード領域上のモーションが `updates == RecordUpdates::default()` とともに `Nothing` を名指しし、その後グリッド上の直前キャッシュセルでのモーションは依然としてレポートする。(FR6, FR8)
- [ ] AC15 — タイトルバー bool またはタブバー bool が true のとき `point_belongs_to_grid` が false を返し、その他すべての入力組み合わせに対する答えは分解前から変わらない。(FR9)
- [ ] AC16 — ガード領域テストが領域ごとに決定された `Disposition` 全体を表明し、いずれかのアームを `Nothing` に戻すとスイートが落ちる。バイトの不在のみを表明するテストスイートはこの基準を満たさない。(BO2, CF5)

### 11.2 KPI

解決済み要件に記録は無い。

## 12. テストシナリオ

### 12.0 ハーネス方針

本リポジトリに E2E ハーネスは存在せず、導入もしない（`test/README.md`
"E2E Tests: None at the moment."）。以下の自動シナリオはすべて
`src-tauri/src/window_host/mouse_report.rs` の既存インライン
`#[cfg(test)] mod tests` に置く Rust ユニットテストであり、既存の
`base_wheel_inputs` / `base_button_inputs` / `base_motion_inputs` ヘルパーから
組み立てたプレーンな `*EventInputs` 値に対して `decide_wheel_event` /
`decide_button_event` / `decide_motion_event` を直接呼ぶ。ウィンドウも
GPU サーフェスも PTY も使わない。命名は `<subject>_<scenario>_<expected>` に従う。
実行コマンドは
`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`。

手動シナリオを分ける理由: 決定ユニットは純粋で完全にユニットテスト可能だが、
`handle_mouse_wheel` / `handle_pointer_button` は `&mut WindowHost` と
`&mut App` を取りテスト内で構築できない。実 winit イベントから早期ガードを経て
決定呼び出しに至る配線は、したがって手作業でしか確認できない。

### 12.1 テスト観点

- [ ] 正常系: 拒否位置のホイール・中押下が退行前のローカルアームを名指しする（TS1、TS3、TS4）。
- [ ] 異常系: `middle_click_paste` 無効時、左押下・右押下、モーションはいずれも `Nothing`（TS5、TS6、TS7）。
- [ ] 境界値: タブバー非表示時の `TITLE_BAR_HEIGHT` 近傍（TS11）、領域が重なった場合の優先順位（TS10）。
- [ ] セキュリティ: 解決済み要件にセキュリティシナリオの記録は無い。
- [ ] パフォーマンス: 負荷試験は規定しない。制約は NFR4（ポインタホットパスに新規コストを足さない）であり、純粋関数の分岐として構造的に満たす。
- [ ] 退行防止: ガード領域テストが disposition 同一性を固定する（TS9）。

### 12.2 テストシナリオ一覧

| ID | 種別 | タイトル | 対象 |
|----|------|----------|------|
| TS1 | unit | ホイール × 領域マトリクス（トラッキング無効・主画面） | AC1, AC2, AC3, AC4, AC6, AC7, FR1, FR2, FR10 |
| TS2 | unit | 同マトリクスをトラッキング有効・両エンコーディングで反復 | AC1, AC13, NFR5 |
| TS3 | unit | 代替画面 + DECSET 1007 での矢印変換アーム | AC5, FR1 |
| TS4 | unit | 中押下 × 領域マトリクス（`middle_click_paste_enabled = true`） | AC8, AC9, AC10, FR3 |
| TS5 | unit | 中押下 × 領域マトリクス（`middle_click_paste_enabled = false`） | AC10, FR3 |
| TS6 | unit | 左押下・右押下 × 領域マトリクスとリリース追従 | AC11, AC12, FR4, FR5 |
| TS7 | unit | モーション × 領域マトリクス | AC14, FR6, FR8 |
| TS8 | unit | `point_belongs_to_grid` の真理値表（分解後） | AC15, FR9 |
| TS9 | unit | ガード領域テストの強化（disposition 同一性の固定） | AC16, BO2, CF5 |
| TS10 | unit | 領域が重なった場合の優先順位 | AC7, FR10 |
| TS11 | unit | タブバー非表示時の境界 | AC1, AC3, FR9 |
| MS1 | manual | 7 箇所でのホイールスクロールバック確認 | AC1, AC2, AC3, AC4 |
| MS2 | manual | ステータスバー上の中クリックペースト確認 | AC8 |
| MS3 | manual | egui chrome 自身のスクロール確認 | AC6, AC7 |
| MS4 | manual | 左押下ドラッグとリサイズ帯の確認 | AC11 |

### 12.3 テストシナリオ詳細

- **TS1**: 7 つの領域入力（タイトルバー帯、タブバー帯、下端ストリップ、
  スクロールバーオーバーレイ、mux サイドバー、リサイズホットゾーン、
  プロファイルセレクタ表示中）× {WheelUp, WheelDown} の各セルについて厳密な
  disposition を表明する。タイトルバー帯 / 下端ストリップ / スクロールバー
  オーバーレイ / リサイズホットゾーンは `Local(ScrollScrollback)`、
  タブバー帯 / mux サイドバー / プロファイルセレクタは `Nothing`。
- **TS2**: 同じマトリクスを 1000 / 1002 / 1003 のそれぞれ有効な状態（かつ両方の
  エンコーディング）で回す。disposition は TS1 と同一であり、マトリクスの
  どのセルでも `apply_outcome` がバイトを追加しない。
- **TS3**: アームを持つ 4 領域の上で `on_alt_screen`、`alt_scroll_mode_bit`、
  `alt_scroll_setting` をすべて true、トラッキング無効にすると
  `Local(TranslateToArrowBytes)`。3 つのうちどれか 1 つを false にすると
  `Local(ScrollScrollback)` に戻る。Shift 押下時も答えは変わらない。
- **TS4**: `middle_click_paste_enabled = true` の中押下 × 領域マトリクス。
  下端ストリップ / スクロールバーオーバーレイ / mux サイドバー /
  リサイズホットゾーンは `Local(PastePrimary)`、タイトルバー帯 / タブバー帯 /
  プロファイルセレクタ表示中は `Nothing`。
- **TS5**: `middle_click_paste_enabled = false` の同マトリクス。全領域で `Nothing`。
- **TS6**: 左押下 × 領域マトリクスは全セルが `updates.gesture == None` を伴う
  `Nothing`。続けてその結果のレコードを左 `Release` に流すと `Nothing` となり
  バイトを追加しない。右押下 × 領域マトリクスも全セルが `Nothing`。
- **TS7**: モーション × 領域マトリクスは `updates == RecordUpdates::default()` を
  伴う `Nothing`。続くグリッド所有の同一セルでのモーションは依然としてレポートする。
- **TS8**: 分解後入力に対する `point_belongs_to_grid` の真理値表。タイトルバー
  bool のみ true で false、タブバー bool のみ true で false、その他すべての
  単一領域・複数領域の組み合わせで分解前から不変。
- **TS9**: `mouse_report.rs:1721` の
  `ac3_ts19_guarded_regions_reject_every_event_kind_and_button` を強化（あるいは
  置き換え）し、`dest.is_empty()` に加えて (領域, 種別, ボタン) の各セルについて
  決定された `SequenceOutcome.disposition` を期待表と突き合わせる。いずれかの
  アームを `Nothing` に戻したらテストが落ちること。
- **TS10**: 重なりの優先順位。`resize_hot_zone && bottom_strip` → ローカルアーム、
  `mux_sidebar && bottom_strip` → `Nothing`、`profile_selector_visible` と
  アームを持つ領域の組み合わせ → `Nothing`、`tab_bar_band && resize_hot_zone` →
  `Nothing`。
- **TS11**: タブバー非表示（`effective_tab_bar_height == 0`）のとき、タブバー帯は
  空になるので `TITLE_BAR_HEIGHT` のすぐ下の位置はグリッド所有となり、その上の
  位置はタイトルバー帯として依然として `ScrollScrollback` を名指しする。
- **MS1**: リリースビルドでスクロールバックを満たした状態で、下端ステータスバー、
  右端スクロールバーオーバーレイ、CSD タイトルバー、4 つのリサイズ帯の上で
  ホイールを回す。7 箇所すべてでスクロールバックが動くこと。mouse-reporting
  対応アプリケーションを走らせた状態（トラッキング有効）でも同じ結果になること。
- **MS2**: `middle_click_paste` を有効にし PRIMARY に既知の文字列を入れた状態で
  ステータスバー上を中クリックする。その文字列がペーストされること。
- **MS3**: タブバー上のホイールが依然としてタブストリップを横スクロールし、
  mux サイドバー（常設・オーバーレイの両方）上のホイールが依然としてウィンドウ
  リストをスクロールし、プロファイルセレクタを開いた状態でホイールがモーダルの
  リストのみをスクロールすること。3 つのいずれでもターミナルのスクロールバックは
  動かないこと。
- **MS4**: ステータスバーまたはスクロールバーオーバーレイの上から始める左押下
  ドラッグがターミナルの選択を開始しないこと。リサイズ帯上の左押下は依然として
  ウィンドウをリサイズすること。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| SC-8 | mouse-reporting が導入したグリッド所有権ガード。`point_belongs_to_grid` として実装され、「この点をターミナルグリッドが所有するか」を答える（NFR3）。 |
| SC-10 | 決定ユニットが純粋関数であるという mouse-reporting の性質。プレーン値のみを受け取り返し、決定時にレコードを変更しない（NFR1）。 |
| 決定ユニット | `decide_press` / `decide_release` / `decide_motion_event` / `decide_wheel_event`。イベントに対して 1 つの `Disposition` を名指しする純粋関数。 |
| `Disposition::Nothing` | 「何もしない」を名指しする決定値。perform 段では `None => {}` に落ちる（CF3）。 |
| `Disposition::Local(LocalArm)` | 具体的なローカル動作を名指しする決定値。本フィーチャーが扱うアームは `ScrollScrollback`、`TranslateToArrowBytes`、`PastePrimary`（FR1、FR3）。 |
| ガード領域 | `point_belongs_to_grid` が false を返す 7 領域: CSD タイトルバー帯、タブバー帯、下端ステータスストリップ、右端スクロールバーオーバーレイ、mux サイドバー、CSD リサイズホットゾーン、プロファイルセレクタ表示中。 |
| アームを持つ領域 | ホイール経路でローカルアームを与える 4 領域（FR2）。CSD タイトルバー帯、下端ステータスストリップ、スクロールバーオーバーレイ、CSD リサイズホットゾーン。 |
| `wheel_consumer` | `input_translate.rs:416-434` の純粋関数。`tracking_active` を最初に分岐し、退行前のホイール消費者を決める（CF6）。 |
| `RecordUpdates` | 決定時ではなく `apply_outcome` で適用されるレコード変更の束（FR8、NFR1）。 |
| `GestureOwner` | 押下が記録するジェスチャー所有者。`Local` と `Report` があり、SC-8 に拒否された押下はどちらも記録しない（FR5）。 |

## 14. 確認事項

### 14.1 確認済み事項

requirements-analyst が実ソースを読んで確認した事実。

- [x] CF1: `handle_mouse_wheel`（`pointer_routing.rs`）は SC-10 のホイール決定の
      手前に厳密に 3 つの早期 return を持ち、それぞれ `egui::Event::MouseWheel` を
      push して return する。(1) `app.profile_selector.visible`（728-743 行）、
      (2) タブバー帯 `logical.y >= TITLE_BAR_HEIGHT && logical.y < top_strip_h`
      （756-778 行。「Restricted to the tab-bar band (below the CSD title bar);
      the title bar's existing wheel behaviour is left untouched.」というコメントを
      伴う）、(3) `ui::mux_sidebar::point_in_sidebar`（791-835 行）。CSD タイトル
      バー帯、下端ステータスストリップ、スクロールバーオーバーレイ、CSD リサイズ
      ホットゾーンに対する早期 return は存在しない。したがってこの 4 領域は
      `decide_wheel_event`（898 行）に到達し、ホイール動作が現在死んでいる 4 領域である。
- [x] CF2: `handle_pointer_button`（`pointer_routing.rs`）のガードはボタン依存で
      別のパターンを取る。CSD リサイズ引き継ぎ（429-436 行）と 下端ストリップ /
      スクロールバー / mux サイドバーのガード（535-587 行）は
      `button == Left && state == Pressed` のときだけ、上部ストリップ
      `egui_pos.y < top_strip_h`（521-526 行）と `app.profile_selector.visible`
      （592-594 行）は無条件に return する。したがって中押下は、下端ストリップ、
      スクロールバーオーバーレイ、mux サイドバー、上部ストリップと重ならない
      リサイズホットゾーンの部分という厳密に 4 領域について `run_button_decision`
      → `decide_press`（604-612 行）に到達し、現在は `Nothing` と答えられて
      `PastePrimary` を失っている。
- [x] CF3: SC-10 の 3 ユニットはいずれも同一の一律拒否で始まる。`decide_press`
      `mouse_report.rs:640-642`、`decide_motion_event` 820-822、
      `decide_wheel_event` 863-865 の各所で
      `if !point_belongs_to_grid(inputs.grid) { return SequenceOutcome::nothing(); }`、
      すなわち `Disposition::Nothing` と `RecordUpdates::default()` を返す。
      `decide_release`（698）は意図的に SC-8 を参照しない。perform 段はその後
      `None => {}`（ホイールは `pointer_routing.rs:983`、ボタンは `:690`）に落ち、
      そこでローカルアームが消える。
- [x] CF4: `GridOwnershipInputs`（`mouse_report.rs:260-268`）は単一の
      `in_top_strip` bool を持ち、`pointer_routing.rs:117` で
      `position.y < TITLE_BAR_HEIGHT + effective_tab_bar_height(app.show_tab_bar)`
      として埋められる。CSD タイトルバー帯とタブバー帯が 1 つの bool に潰されて
      いるため、現在の決定ユニットは両者を区別できない。これが FR9 の要求する分解である。
- [x] CF5: 退行を捕まえるはずだったテスト
      `ac3_ts19_guarded_regions_reject_every_event_kind_and_button`
      （`mouse_report.rs:1721-1811`）は 6 領域 × 3 ボタン ×
      {Press, Release, Motion, WheelUp, WheelDown} を掃くが、`apply_outcome` の後に
      `dest.is_empty()` しか表明しない。`Nothing` も `Local(..)` もバイトを追加
      しないため、この表明は両者で同一に成立する。これが退行がレビューと CI を
      通過した直接の機械的理由である。
- [x] CF6: `wheel_consumer`（`input_translate.rs:416-434`）は `tracking_active` で
      最初に分岐する。`tracking_active = false` のとき `shift_held` を完全に無視し、
      `on_alt_screen && alt_scroll_mode_bit && alt_scroll_setting` のときに限り
      `TranslateToArrows` を、そうでなければ `ScrollScrollback` を返す。SC-8 が
      拒否したホイール分岐から `tracking_active = false` を与えれば、フィーチャー
      導入前のホイール動作表が厳密に再現される。固定の `ScrollScrollback` が
      誤りである理由はこれである。
- [x] CF7: mouse-reporting の IMPLEMENTATION.md の D11「Boundary」段落は、この
      退行が破った契約を既に明記している。「The left-press-only gating of the
      existing LOCAL side effects stays exactly as it is; only the reporting
      question becomes identity-independent.」。SC-8 の行（53 行）も同様に
      「A false answer leaves the event's current local behaviour exactly as it is;
      it never changes the left-press-only gating of the existing local side
      effects.」で終わっている。本修正は既存仕様への適合回復であり、新しい
      プロダクト動作を追加しない。
- [x] デザインステップ: スキップ。`window_host` 内部の純粋なイベント
      ルーティング修復であり、UI 要素・レイアウト・デザイントークン・視覚的
      サーフェスのいずれにも触れない。

### 14.2 未確認・保留事項

無し。本書のすべての機能要件（FR1〜FR10）は `status: resolved` であり、
`tbd_reason` を持つ要件は存在しない。

## 15. 参考資料

- SPEC.md: `feature-docs/mouse-report-guard-local-arms/SPEC.md` — 本要件の
  実装向け描画。
- `feature-docs/mouse-reporting/SPEC.md` — 親フィーチャーの仕様。FR10 / AC12 の
  レポート禁止（FR7 が保持する対象）。
- `feature-docs/mouse-reporting/IMPLEMENTATION.md` — D11「Boundary」段落と
  SC-8 の行（CF7 が引用する契約）。
- `.claude/rules/core-commands.md` / `.claude/rules/core-build-location.md` —
  NFR6 が指定する `CARGO_TARGET_DIR` 付きテストコマンド。
- `test/README.md` — E2E ハーネスが存在しないことの根拠（第 12 章）。
