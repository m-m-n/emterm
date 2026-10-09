# Feature: osc7501-program-status

## Overview

OSC 7501（Program Status Protocol draft 0.3）を通常タブと mux ペインで受信し、id ごとのレコード表に保持する。OSC 777 emterm;agent-status の状態と OSC 7501 の集約状態を合成し、タブバッジ・mux サイドバー・通知・ステータスバーのテンプレート変数 `{agent_status}`・mux エージェント API に反映する。要件の詳細は [REQUIREMENTS.md](REQUIREMENTS.md) を参照する。

## Objectives

- OSC 7501（Program Status Protocol draft 0.3）を受信し、既存のエージェント状態表示（タブバッジ・mux サイドバー・通知・ステータスバーのテンプレート変数）に反映する。
- eMterm 専用のフックなしで、OSC 7501 対応エージェントの状態を表示できるようにする。
- 既存の OSC 777 emterm;agent-status の挙動を変えない。

## User Stories

### US1: 通常タブで OSC 7501 の状態を表示する
OSC 7501 対応エージェントの利用者として、エージェントを通常タブで動かしたい。eMterm 専用のフックなしで状態をタブバッジと `{agent_status}` で見られるようにするため。

**Acceptance Criteria:**
- [ ] AC1: OSC 7501 ; state=... を BEL 終端と ST 終端の両方で受信し、通常タブのタブバッジと `{agent_status}` に状態が反映される。
- [ ] AC2: state の 6 値を仕様どおり扱う: レコードの完全置換、clear による部分木と全体の削除、error の独立した表示と集約順位。
- [ ] AC3: 解析規則と破棄条件を満たす: 不正ペアの読み飛ばし、未知 key の無視、後勝ち、4096 バイト・title・msg・id の上限、256 件を超えたときの最古更新レコードの追い出し。
- [ ] AC4: msg・title の base64 をデコードし、不正な base64、上限超過、制御文字を含むデコード結果をもつ報告を丸ごと拒否する。
- [ ] AC5: OSC 7501 ; ? に、同じ終端子で body ? の応答を返す。
- [ ] AC6: live・main screen の OSC 133 A で working・blocked・idle のレコードが消え、done・error は残る。タブまたはペインの終了で全レコードが消え、RIS で全レコードが消える。DECSTR と alt screen の切替では残る。

### US2: error への遷移を通知で受け取る
OSC 7501 対応エージェントの利用者として、エージェントが error を報告したときに通知を受け取りたい。done と同じ設定で error を知るため。

**Acceptance Criteria:**
- [ ] AC7: error への遷移は agent_notify_on_done が有効なときに通知され、本文の名前には無害化済みの title（無ければ app）が使われる。

### US3: mux ペインで OSC 7501 の状態を表示する
OSC 7501 対応エージェントの利用者として、エージェントを mux ペインで動かしたい。通常タブと同じように状態を見て、mux エージェント API で待てるようにするため。

**Acceptance Criteria:**
- [ ] AC8: mux ペインでも同じように動作する: daemon 経由で GUI に届き、未接続中の報告も反映され、再アタッチ後に通知なしで再同期され、リプレイで照会に再応答しない。WaitAgentState は合成状態を対象にし、error を待てる。
- [ ] AC9: mux daemon の hot-upgrade の後も、OSC 7501 のレコードと追い出し順が保たれる。

### US4: OSC 777 の挙動を保つ
OSC 777 agent-status の利用者として、これまでと同じ表示・通知・mux API の結果を得たい。

**Acceptance Criteria:**
- [ ] AC10: 既存の OSC 777 agent-status のテストがすべて通り、OSC 7501 を受けていないペインの表示・通知・mux API の結果が変わらない。

## Technical Requirements

### Functional Requirements
- **FR1:** 受信 — `OSC 7501 ; <body>` を BEL 終端と ST（ESC \）終端の両方で受信する。通常タブでは TerminalCore::register_osc_app_param で 7501 を未使用の内部番号に写像する（term_core には 7501 を埋め込まない）。mux ペインでは daemon の AgentStatusFeedScanner が OSC 本文の `7501;` を認識し、OSC 777 報告・OSC 133 mark と同じ 1 本の順序付きフィードで処理する。
- **FR2:** 解析規則 — body は key=value を `:` で連結したもの。key は `[a-z]+`、value は `[A-Za-z0-9_.,+/=-]*`。形式に合わないペアは読み飛ばし、未知の key は無視し、重複する key は後勝ちとする。
- **FR3:** 破棄条件 — 次の報告は丸ごと破棄する: シーケンス全体が 4096 バイトを超える報告、title がエンコード後 256 バイトまたはデコード後 192 バイトを超える報告、msg がエンコード後 2732 バイトまたはデコード後 2048 バイトを超える報告、title・msg の base64 が不正な報告、title・msg のデコード結果が UTF-8 でないか制御文字を含む報告。state が欠落しているか未知値の報告は無視する。id が不正な報告（各セグメントが `[A-Za-z0-9_.+-]{1,32}` でない、8 階層を超える、128 バイトを超える）は無視する。
- **FR4:** state とレコード置換 — state は idle / working / done / blocked / error / clear の 6 値。clear 以外の報告は、その id のレコードを完全に置き換える（省略した key はレコードから消える）。state=clear は対象 id とその子孫のレコードを削除し、id が無指定なら全レコードを削除する。
- **FR5:** 任意 key の解析と保持 — kind（permission / question / auth）、progress（0〜100 の整数）、app（`[A-Za-z0-9_.+-]{1,32}`）、title・msg（base64 の UTF-8）を解析してレコードに保持する。kind は state=blocked のときだけ、progress は state=working / blocked のときだけ保持する。app を持たないレコードは、最も近い祖先レコードの app を継承する。kind・progress・msg は表示にも通知にも使わない。
- **FR6:** レコード上限 — OSC 7501 のレコードはタブまたは mux ペインあたり 256 件まで。上限を超えるときは、最後の更新が最も古いレコードを追い出す。
- **FR7:** 寿命 — live・main screen の OSC 133 A を受けたら、state が working・blocked・idle のレコードを削除する（直前の OSC 133 D は条件にしない）。done・error のレコードは、明示的な clear、RIS、タブまたはペインの終了まで残す。RIS で全レコードを削除する。DECSTR と alt screen の切替ではレコードを削除しない。タブまたは mux ペインが閉じたときは、レコードをエントリーごと破棄する。
- **FR8:** 照会応答 — body がちょうど `?` の `OSC 7501 ; ? ST` に、body `?` の OSC 7501 で応答する。終端子は照会と同じもの（BEL または ST）を使う。照会はレコードを変更しない。
- **FR9:** ストア構成と合成 — 既存の OSC 777 エントリー（AgentStatusModel と daemon の MuxPane の agent-status）はそのまま残す。それとは別に、タブまたはペインごとに OSC 7501 のレコード表を持つ。表示・通知・mux API が使うペインの状態は、OSC 777 の状態と OSC 7501 の集約状態を FR11 の優先順位で合成したもの。OSC 777 だけを受けたペインの合成状態は、現在の OSC 777 の状態と一致する。
- **FR10:** 階層 id の集約 — ペインの OSC 7501 集約状態は、ルートと子孫を含む全レコードのうち、FR11 の優先順位で最も高い状態とする。
- **FR11:** error 状態 — core の AgentState と mux wire の mux_ipc::protocol::AgentState に Error を追加する。既読・未読の扱いは done と同じ。集約順位は blocked > 未読 error > 未読 done > working > 既読 error > 既読 done > idle。タブバッジと mux サイドバーでは、error に専用の絵文字と md3 の error 系の色を使う。
- **FR12:** 通知 — 合成後のペイン状態が blocked・done・error に遷移したときに通知する。error への遷移は agent_notify_on_done のトグルで制御し、設定は追加しない。それ以外のゲート（notification_enabled、agent_status_notifications、agent_notify_visible_pane、ペインごと 30 秒のレート制限）は既存のものを使う。通知本文の名前には、合成状態を決めた OSC 7501 レコードの無害化済み title を使う。title が無ければ継承した app を使い、どちらも無ければ OSC 777 の name、それも無ければ既存の代替名を使う。
- **FR13:** ステータスバーのテンプレート変数 — ステータスバーのテンプレート変数 `{agent_status}` を追加する。値はアクティブタブの合成済み集約状態（タブ自身の状態と、mux 接続中ならウィンドウグループ内の全ペイン）。状態が無いときは空文字列にする。集約状態が変わったら、変数のバージョンを上げて再描画を要求する。既定のテンプレートには追加しない。
- **FR14:** mux 配信 — mux ペインでは daemon が OSC 7501 を解析してレコード表を保持する（GUI の接続の有無を問わない）。合成状態の計算と通知名の決定に必要な情報は、wire に OSC 7501 用の項目またはメッセージを追加して GUI に届ける。どちらのソースが変わっても、ペインの revision を上げる。スナップショット・再アタッチ・ウィンドウ切替の後は replay_derived（通知なし）で再同期する。GUI は、mux の内側コンテンツを解析して得た OSC 7501 を、既存の OSC 777 と同じく捨てる。daemon は RIS を検出し、そのペインのレコードを全削除する。
- **FR15:** mux リプレイ除去 — daemon のスクロールバックリングとスナップショットから、OSC 7501 の報告と照会（`?`）を取り除く。再アタッチ時のリプレイで、レコードの再適用も照会への再応答も起こさない。
- **FR16:** mux エージェント API と CLI — WaitAgentState などの mux エージェント API は、合成後のペイン状態を対象にする。`emterm mux wait` の状態指定に error を追加する。`emterm agent-status` CLI は変更しない（引き続き OSC 777 を送る）。OSC 7501 のレコードを照会する API は追加しない。
- **FR17:** hot-upgrade の引き継ぎ — mux daemon の hot-upgrade では、各ペインの OSC 7501 の全レコードと、追い出しに使う更新順を handoff ファイルに含めて引き継ぐ。
- **FR18:** OSC 777 互換 — OSC 777 emterm;agent-status の解析・推論クリア（D→A latch）・mux 配信・CLI の挙動を変えない。OSC 7501 を受けていないペインの表示と通知は現在と同じ。
- **FR19:** 対象外 — OSC 9;4 の対応付け、terminfo の Pst capability、kind・progress・msg の表示は扱わない。既存の OSC 9 通知処理は変更しない。

### Non-Functional Requirements
- **NFR1 - ビルド構成:** OSC 7501 の解析モジュールとレコード表は gui feature に依存させず、`--no-default-features` のビルドでもコンパイルできるようにする。
- **NFR2 - コード転用禁止:** 外部実装のコードは転用しない。参照するのは仕様書と解説記事だけとする。
- **NFR3 - 対応プラットフォーム:** Linux と Windows の両方で動作する。
- **NFR4 - UI スレッド:** OSC 7501 の処理と通知の送出で UI スレッドをブロックしない。
- **NFR5 - セキュリティ:** title・msg をマークアップとして解釈しない。名前に使う title は、制御文字と、双方向制御文字などの不可視文字を取り除き、80 文字までに切り詰める。`{agent_status}` の値は固定の状態語だけにし、端末から受け取った文字列を含めない。

## Implementation Approach

### Architecture

**System Architecture:**
```
┌──────────────────────────────────────────────────────────────┐
│ 表示・通知: タブバッジ / mux サイドバー / 通知 / {agent_status} │
├──────────────────────────────────────────────────────────────┤
│ 合成: OSC 777 の状態 + OSC 7501 の集約状態（FR11 の優先順位）  │
├──────────────────────────────────────────────────────────────┤
│ ストア: OSC 777 エントリー（既存） / OSC 7501 レコード表（新規）│
├──────────────────────────────────────────────────────────────┤
│ 受信: term_core（通常タブ） / AgentStatusFeedScanner（daemon）│
└──────────────────────────────────────────────────────────────┘
```

**Component Diagram:**
```
OSC 7501 解析モジュール（gui 非依存, NFR1 / A7）
  ├─ 通常タブ: TerminalCore::register_osc_app_param で 7501 → 未使用の内部番号
  │             → callbacks → タブの OSC 7501 レコード表
  └─ mux ペイン: daemon AgentStatusFeedScanner（OSC 777・OSC 133 と同じ順序付きフィード）
                → MuxPane の OSC 7501 レコード表 → wire（OSC 7501 用の項目またはメッセージ）→ GUI

AgentStatusModel（OSC 777, 既存）+ OSC 7501 レコード表 → 合成状態
  → タブバッジ / mux サイドバー / 通知 / {agent_status} / WaitAgentState
```

### Data Flow

```
通常タブ:
PTY 出力 → term_core（7501 を内部番号に写像）→ 解析 → レコード表 → 合成 → 表示・通知

mux ペイン:
PTY 出力 → daemon AgentStatusFeedScanner → 解析 → MuxPane レコード表（revision 更新）
        → wire → GUI → 合成 → 表示・通知
        （スナップショット・再アタッチ・ウィンドウ切替後は replay_derived で通知なし再同期）

照会:
OSC 7501 ; ? → term_core → 同じ終端子で OSC 7501 ; ? を応答
（mux ペインでは接続中の GUI の term_core が PtyInput 経由で返す, A3）
```

### API Design

#### mux wire: AgentState

- mux_ipc::protocol::AgentState に `Error` を追加する（FR11）。

#### mux wire: OSC 7501 用の項目またはメッセージ

- 合成状態の計算と通知名の決定に必要な情報を GUI に届ける項目またはメッセージを追加する（FR14）。
- OSC 777・OSC 7501 のどちらのソースが変わっても、ペインの revision を上げる。

#### mux エージェント API

- WaitAgentState などの mux エージェント API は合成後のペイン状態を対象にする（FR16）。
- `emterm mux wait` の状態指定に `error` を追加する（FR16）。
- OSC 7501 のレコードを照会する API は追加しない（FR16）。

#### ステータスバーのテンプレート変数

- `{agent_status}`: 状態の wire 名（idle / working / blocked / done / error）。状態が無いときは空文字列（FR13, A11）。

### Data Model

#### OSC 7501 レコード表（タブまたは mux ペインごと）

| 項目 | 型 | 説明 |
|------|----|------|
| id | 文字列 | 階層 id。各セグメント `[A-Za-z0-9_.+-]{1,32}`、8 階層まで、128 バイトまで |
| state | 列挙 | idle / working / done / blocked / error |
| kind | 列挙 | permission / question / auth。state=blocked のときだけ保持 |
| progress | 整数 | 0〜100。state=working / blocked のときだけ保持 |
| app | 文字列 | `[A-Za-z0-9_.+-]{1,32}`。無ければ最も近い祖先レコードの app を継承 |
| title | 文字列 | base64 の UTF-8 をデコードしたもの |
| msg | 文字列 | base64 の UTF-8 をデコードしたもの |
| 更新順 | 順序 | 追い出しに使う最後の更新の順序。hot-upgrade の handoff ファイルで引き継ぐ |

- 上限はタブまたはペインあたり 256 件（FR6）。

### Dependencies

**Internal Dependencies:**
- TerminalCore::register_osc_app_param: 通常タブで 7501 を未使用の内部番号に写像する。
- AgentStatusModel: OSC 777 のエントリー。そのまま残し、OSC 7501 の集約状態と合成する。
- daemon AgentStatusFeedScanner: mux ペインで OSC 7501 を OSC 777・OSC 133 と同じ順序付きフィードで認識する。
- daemon MuxPane の agent-status: OSC 777 のエントリー。そのまま残す。
- mux_ipc::protocol::AgentState: Error を追加する。
- replay_derived: スナップショット・再アタッチ・ウィンドウ切替後の通知なし再同期。
- hot-upgrade の handoff ファイル: OSC 7501 のレコードと更新順を引き継ぐ。
- 既存の通知ゲート: notification_enabled、agent_status_notifications、agent_notify_on_done、agent_notify_visible_pane、ペインごと 30 秒のレート制限。

**External Dependencies:**
- なし。

### File Structure

ファイル配置は create-plan で `workflow.yaml` の各タスクの `files` として決める。OSC 7501 の解析モジュールは gui feature に依存させず、agent_status.rs と同じ配置とする（A7）。

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/osc7501-program-status/**`
- `test-docs/osc7501-program-status/**`

`feature-docs/osc7501-program-status/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/osc7501-program-status/**` covers `test-docs/osc7501-program-status/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/osc7501-program-status/` directory at all; the declared
`test-docs/osc7501-program-status/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests
- [ ] TS1（FR2, FR3, FR4, FR5, FR6, FR10, FR11）: 解析器の単体テスト - 6 つの state、不正ペアの読み飛ばし、未知 key、重複 key の後勝ち、state の欠落と未知値、id の各上限、title・msg の base64 不正・上限超過・制御文字、4096 バイト超過。
- [ ] TS2（FR2, FR3, FR4, FR6, FR10, FR11）: レコード表の単体テスト - 完全置換、clear による部分木と全体の削除、app の継承、kind・progress の条件付き保持、256 件超過時の最古更新レコードの追い出し。
- [ ] TS3（FR7）: 寿命の単体テスト - OSC 133 A で working・blocked・idle が消え done・error が残ること、RIS で全削除されること、DECSTR と alt screen では残ること、alt screen 上の A では消えないこと。

### Integration Tests
- [ ] TS4（FR1, FR8, FR9, FR13）: callbacks・term_core 経由のテスト - BEL 終端と ST 終端の受信、`?` への同じ終端子での応答、OSC 777 と OSC 7501 の合成。
- [ ] TS5（FR1, FR4, FR9, FR10, FR11, FR13）: 集約と表示のテスト - error を含む集約順位、error のバッジ表示（未読と既読）、`{agent_status}` の値と空文字列、バージョン更新。
- [ ] TS6（FR11, FR12）: 通知のテスト - error が agent_notify_on_done で制御されること、名前の選択順（title、app、OSC 777 の name、代替名）、title の無害化。
- [ ] TS7（FR14, FR15, FR16）: mux のテスト - daemon スキャナが 7501 を OSC 133・777 とのバイト順どおりに出すこと、Error と 7501 項目の wire serde 往復、replay_derived での再同期、リングとスナップショットからの 7501 報告・照会の除去、WaitAgentState が error と合成状態を扱うこと、daemon での RIS 検出。
- [ ] TS8（FR17）: hot-upgrade の handoff で、7501 のレコードと更新順が往復で保たれること。
- [ ] TS9（FR18）: 回帰 - 既存の agent_status・agent_status_model・exit_latch・notifications・mux agent-status のテストを全件実行する。

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Existing E2E tests pass without regression

### Edge Cases
- [ ] シーケンス全体が 4096 バイトを超える報告は丸ごと破棄する（FR3）。
- [ ] title・msg のエンコード後・デコード後の上限超過、base64 不正、UTF-8 でない・制御文字を含むデコード結果は報告を丸ごと破棄する（FR3）。
- [ ] state の欠落・未知値、不正な id の報告は無視する（FR3）。
- [ ] progress が 0〜100 の整数でなければ不正ペアとして読み飛ばす（A5）。
- [ ] 257 件目のレコードで、最後の更新が最も古いレコードを追い出す（FR6）。
- [ ] alt screen 上の OSC 133 A ではレコードを消さない（FR7, A6）。
- [ ] 再アタッチ時のリプレイで、レコードの再適用も照会への再応答も起こさない（FR15）。

### Performance Tests
- 該当なし。NFR4（UI スレッドをブロックしない）は設計で満たす。

## Security Considerations

- **Authentication:** 該当なし。
- **Authorization:** 該当なし。
- **Input Validation:** FR2 の解析規則と FR3 の破棄条件に従う。
- **Data Protection:** 該当なし。
- **Markup / Injection Prevention:** title・msg をマークアップとして解釈しない。名前に使う title は、制御文字と、双方向制御文字などの不可視文字を取り除き、80 文字までに切り詰める。`{agent_status}` の値は固定の状態語だけにし、端末から受け取った文字列を含めない（NFR5）。通知には既存のゲート（unix ではマークアップのエスケープ）を使う（A9）。

## Error Handling

### Error Codes

| 条件 | 対応 |
|------|------|
| シーケンス全体が 4096 バイト超過 | 報告を丸ごと破棄 |
| title がエンコード後 256 バイトまたはデコード後 192 バイト超過 | 報告を丸ごと破棄 |
| msg がエンコード後 2732 バイトまたはデコード後 2048 バイト超過 | 報告を丸ごと破棄 |
| title・msg の base64 が不正 | 報告を丸ごと破棄 |
| title・msg のデコード結果が UTF-8 でないか制御文字を含む | 報告を丸ごと破棄 |
| state の欠落・未知値 | 報告を無視 |
| id が不正 | 報告を無視 |
| 形式に合わないペア | そのペアを読み飛ばす |
| 未知の key | 無視 |
| wire の未知の変種や項目（バージョン混在時） | 既存の「malformed AgentStatusUpdate payload」警告経路で捨てる（A8） |

### Error Flow

```
受信 → サイズ・base64・UTF-8・制御文字の検査 → 不合格なら報告を破棄
     → state・id の検査 → 不合格なら報告を無視
     → レコード表を更新
```

## Performance Optimization

### Performance Goals
- OSC 7501 の処理と通知の送出で UI スレッドをブロックしない（NFR4）。

### Optimization Strategies
- 該当なし。

### Caching Strategy
- 該当なし。

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Performance meets specified goals
- [ ] Security requirements are satisfied
- [ ] Documentation is complete
- [ ] Code review is completed
- [ ] `--no-default-features` のビルドがコンパイルできる（NFR1）

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- なし。

## Assumptions

- A1: 仕様の「接続プロセス終了時」は、PTY 子プロセスの終了でタブまたは mux ペインが閉じることとし、既存の discard 経路（タブを閉じたとき、または PtyExited）でレコードを破棄する。
- A2: 通常タブでは register_osc_app_param で 7501 を未使用の内部番号に写像し、term_core に 7501 を埋め込まない。
- A3: 照会応答は body `?` で、照会と同じ終端子を使う。mux ペインでは、既存のデバイス照会と同じく、接続中の GUI の term_core が PtyInput 経由で返す。
- A4: レコード上限はタブまたはペインあたり 256 件とし、最後の更新が最も古いレコードから追い出す。
- A5: progress が 0〜100 の整数でなければ不正ペアとして読み飛ばす。kind は blocked 以外、progress は working・blocked 以外では保持しない。
- A6: 寿命判定に使う OSC 133 A は live・main screen の mark だけとする。alt screen の切替と DECSTR ではレコードを消さず、RIS では全消去する。
- A7: OSC 7501 の解析モジュールは gui feature に依存させない（agent_status.rs と同じ配置）。
- A8: GUI と mux daemon は同じバージョンで動かす前提とする。旧バージョンが混在した場合、未知の変種や項目は既存の「malformed AgentStatusUpdate payload」警告経路で捨てられることを許容する。
- A9: OSC 7501 由来の通知にも既存のゲートを使う（設定、可視ペイン、30 秒のレート制限、unix ではマークアップのエスケープ）。
- A10: daemon のリングとスナップショットから OSC 7501 の報告と照会を取り除き、状態の再同期は replay_derived で行う。
- A11: `{agent_status}` の値は状態の wire 名（idle / working / blocked / done / error）とし、状態が無いときは空文字列にする。
- A12: error のバッジは、未読のとき U+274C（CROSS MARK）、既読のとき done と同じく IDLE_BADGE_EMOJI（U+1F4A4）を使う。代替の円は done と同じく未読で塗りつぶし、既読でリング。色は md3 の error ロールを使う。
- A13: 通知名に使う title・app は、OSC 7501 集約状態を決めたレコードから取る。同じ順位のレコードが複数あれば、最後の更新が最も新しいものを使う。
- A14: error 通知の状態語は、英語で "error"、日本語で "エラー" とする。

## References

- 要件定義書: [REQUIREMENTS.md](REQUIREMENTS.md)
- Program Status Protocol draft 0.3（OSC 7501）
