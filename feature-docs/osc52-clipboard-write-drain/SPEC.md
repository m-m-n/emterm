# Feature: osc52-clipboard-write-drain

## Overview

eMterm が受信した OSC 52 の書き込み（`ESC ] 52 ; <target> ; <base64> ST/BEL`）とクリア（空ペイロード）を、pump ごとにキューから排出してシステムクリップボードへ適用する。OSC 52 の読み取りキューは毎 pump 排出して破棄する。要件の詳細は `feature-docs/osc52-clipboard-write-drain/REQUIREMENTS.md` を参照。

## Objectives

- eMterm が受信した OSC 52 の書き込みがシステムクリップボードに届き、SSH 越しの Claude Code のドラッグ選択による自動コピーが実際にコピーされる
- OSC 52 のクリア（空ペイロード）がシステムクリップボードに届く
- OSC 52 コールバックのキュー（`pending_clipboard_writes`、`pending_clipboard_reads`）が際限なく伸びない
- 排出の退行をテストで検出できる

## User Stories

### US1: OSC 52 書き込みをクリップボードへ反映する
SSH 越しに Claude Code を使う開発者として、リモートのプログラムが出力した OSC 52 の書き込みを、ローカルのシステムクリップボードから貼り付けたい。

**Acceptance Criteria:**
- [ ] AC-1: eMterm のタブ（ローカルまたは SSH 越し）で `printf '\e]52;c;%s\a' "$(printf hello | base64)"` を実行した後、CLIPBOARD を貼り付けると `hello` が得られる
- [ ] AC-2: 代替のクリップボード sink で、キュー済みの書き込みが次のとおり振り分けられる。target `c` は CLIPBOARD、`p` は PRIMARY、`cp`（または `pc`）は両方、`''`・`s`・`0`・`7`・`q`・`s0` は CLIPBOARD。Windows では PRIMARY の書き込み先はすべて CLIPBOARD になる
- [ ] AC-5: アクティブペインの mux PtyOutput フレーム内の OSC 52 書き込みと、同じ coalesce 済みバッファの Detached 後の末尾にある OSC 52 書き込みが、パースされたのと同じ pump でクリップボード sink に届く
- [ ] AC-10: 排出を取り除くか、mux フレーム適用ループより前に置くと失敗するテストがある

### US2: OSC 52 クリアをクリップボードへ反映する
OSC 52 を出力するプログラムを eMterm 上で使う開発者として、空ペイロードの OSC 52 で選択領域をクリアしたい。

**Acceptance Criteria:**
- [ ] AC-3: キュー済みのクリア（`c;`、`p;`、`cp;`、`;`）が、対応する書き込み先のクリア経路に届き、空テキストの早期 return で落とされない。選択範囲が空に解決されたコピー操作は、引き続き何も書き込まない

### US3: キューが伸びず、既存の挙動が保たれる
eMterm のユーザーとして、OSC 52 の書き込みや照会が続いてもキューが伸びず、スナップショット再生やクリップボード不在でクリップボードが壊れないようにしたい。

**Acceptance Criteria:**
- [ ] AC-4: OSC 52 の書き込みと照会を受けた pump の後、両方のキューが空で、照会への応答として PTY に書かれたバイトがない
- [ ] AC-6: OSC 52 の書き込みまたはクリアを含む Snapshot / SnapshotRestore を適用しても、クリップボードへの書き込みもクリアも起きない
- [ ] AC-7: クリップボードが使えない状態で、OSC 52 の書き込みとクリアを含む pump がパニックせずに終わり、両方のキューが空になる
- [ ] AC-8: 既存の callbacks テスト `osc_52_write_denied_when_over_quota`、`osc_52_query_denied_when_read_disabled`、`osc_52_write_default_allows_within_quota`、`osc_52_clear_pushes_empty_write`、`parse_osc52_*` が引き続き通る
- [ ] AC-9: `--no-default-features` の cargo check が成功する

## Technical Requirements

### Functional Requirements

- **FR1: キュー済み OSC 52 書き込みのシステムクリップボードへの適用** — `NativeCallbacks::handle_clipboard` が `pending_clipboard_writes` に積んだ、テキストが空でない `(target, text)` の各エントリを、target が選ぶ選択領域（FR3）へ、UI スレッドで `WindowHost` が持つ arboard のクリップボードインスタンスを通して書き込む。エントリは到着順に適用し、1 回の pump の後、各書き込み先はそこへ宛てられた最後の書き込みのテキストを持つ。
- **FR2: キュー済み OSC 52 クリアの適用** — テキストが空のエントリ（`Osc52Action::Clear` の分岐が積む）は、target が選ぶ選択領域（FR3）をクリアする。クリア後、その選択領域はテキストを持たない。クリアは専用経路で行う。`WindowHost::set_clipboard` と `WindowHost::set_primary` は空テキストで早期 return し（`window_host/mod.rs:707`、`:733`）、既存の呼び出し元（コピー操作、マウスアップ時の PRIMARY コピー、`SelectionWriteSink`）向けにこの早期 return を維持する。
- **FR3: target から選択領域への対応付け** — `c` を含む target は CLIPBOARD、`p` を含む target は PRIMARY、両方を含む target は両方を選ぶ。空、または `s`・`0`〜`7`・`q` のみ（`c` も `p` も含まない）の target は CLIPBOARD を選ぶ。Windows では PRIMARY を CLIPBOARD に置き換える（両方を選ぶ target は CLIPBOARD に 1 回書き込み、最終内容は同じ）。クリア（FR2）も同じ対応付けを使う。
- **FR4: OSC 52 読み取りキューの排出と破棄** — `pending_clipboard_reads` を毎 pump 空にし、エントリを破棄する。クリップボードの内容は読まず、PTY へ OSC 52 の応答を書かない。読み取りへの応答は別フィーチャーで扱う。`handle_clipboard` の既存の `clipboard_read_osc52` ゲートと、その `LOG_OSC52_DENIED` ログはそのまま残す。
- **FR5: pump 内のすべてのパースの後に排出する** — 各タブのクリップボードキューは、同じ pump でそのタブの `NativeCallbacks` に届きうるすべてのパースが終わった後で排出する。対象は、mux 前の外側のパース、mux フレーム適用ループ（`apply_active_pane_output` / `flush_coalesced_output` によるアクティブペインの PtyOutput）、`Tab::process_combined` の Detached 後の末尾の再ルーティング、`App::pump_all` が `tab.pump()` の後に `tab.poll_pending_switch()` / `poll_pending_scrollback_restore()` で適用するキュー済みのライブ出力である。これらのどの経路で届いた OSC 52 も同じ pump で適用し、後の pump へ持ち越さない。`process_combined` 内の早い段階の `cb_state` 排出ブロック（`output_pipeline.rs:345-401`）は、mux フレーム適用ループ（`:433-497`）と末尾の再ルーティング（`:543-548`）より前で動くため、この要件を満たさない。`App::pump_all` の既存の `drain_osc` の位置（`app/mod.rs:1217`）は、これらすべての後にある。
- **FR6: すべてのタブを排出する** — `drain_osc` がすでにすべてのタブを対象にしているのと同じく、バックグラウンドタブと mux 接続タブを含むすべてのタブのクリップボードキューを、毎 pump 排出して適用する。
- **FR7: スナップショット再生でクリップボードに書き込まない** — mux スナップショット再生の内容（同期の `reset_frame_for_replay` 経路、または off-thread 再生で適用される Snapshot / SnapshotRestore）に含まれる OSC 52 は、システムクリップボードへの書き込みもクリアも行わない。ウィンドウ切り替えや再接続で、ユーザーの現在のクリップボードを過去の OSC 52 ペイロードで上書きしない。
- **FR8: クリップボードが使えない場合** — arboard のクリップボードを取得できなかった場合（`WindowHost.clipboard` が `None`）、または書き込み・クリアの呼び出しが失敗した場合も、キューは毎 pump 排出し、パニックせず、失敗はクリップボードのテキストを含めずにログへ出す。
- **FR9: 既存の受理ゲートを変えない** — `handle_clipboard` の既存の拒否規則の挙動を維持する。不正な base64、デコード後のサイズが `clipboard_max_size_osc52` を超えるもの（target と長さのみで `LOG_OSC52_DENIED` を出す）、UTF-8 でないペイロードはキューに積まれず、クリップボードに届かない。

### Non-Functional Requirements

- **NFR1: キューの上限** — 各 pump の後、すべてのタブで `pending_clipboard_writes` と `pending_clipboard_reads` が空である。OSC 52 の書き込みや照会が続いても、際限なく伸びない。
- **NFR2: UI スレッドからのクリップボードアクセス** — arboard は UI（イベントループ）スレッドからのみ、単一の `WindowHost` クリップボードインスタンスを通して呼ぶ。パース中に動くコールバックはキューに積むことだけを続ける。
- **NFR3: ログにクリップボード内容を含めない** — このフィーチャーで追加・変更するログ行は、デコード済みの OSC 52 テキストも base64 ペイロードも含まない。出してよいのは target、長さ、エラーの種類のみ。
- **NFR4: フィーチャーゲート** — クリップボード適用のコードは `gui` フィーチャー内に置く。`CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` がコンパイルを通り続ける。
- **NFR5: 既存のコピー・貼り付けの挙動を変えない** — コピー操作（選択範囲での `set_clipboard`）、マウスアップ時の PRIMARY 自動コピー、`SelectionWriteSink::write_selection`、貼り付け（`get_clipboard` / `get_primary`）は現在の挙動を保つ。
- **NFR6: プラットフォーム** — Linux（arboard 経由の X11 と Wayland）と Windows で成り立つ。macOS は対象外。

## Implementation Approach

### Architecture

**System Architecture:**
```
┌──────────────────────────────────────────────────────────┐
│ パース（tab の core、NativeCallbacks を持つ）            │
│   mux 前の外側のパース                                    │
│   mux フレーム適用ループ（アクティブペインの PtyOutput）  │
│   Detached 後の末尾の再ルーティング                       │
│   poll_pending_switch / poll_pending_scrollback_restore   │
├──────────────────────────────────────────────────────────┤
│ NativeCallbacks::handle_clipboard（受理ゲート、キュー積み）│
│   pending_clipboard_writes / pending_clipboard_reads      │
├──────────────────────────────────────────────────────────┤
│ App::pump_all の排出（すべてのタブ、すべてのパースの後）  │
├──────────────────────────────────────────────────────────┤
│ WindowHost の arboard クリップボード（UI スレッド）       │
│   CLIPBOARD / PRIMARY                                     │
└──────────────────────────────────────────────────────────┘
```

**Component Diagram:**
```
NativeCallbacks ──(target, text)──> pending_clipboard_writes ──┐
NativeCallbacks ───────────────────> pending_clipboard_reads ──┤
                                                               v
                                           App::pump_all の排出（全タブ）
                                              │            │
                                 text 非空: 書き込み     text 空: クリア専用経路
                                              v            v
                                 target 対応付け（FR3） → WindowHost（arboard）
                                 reads: 破棄（PTY 応答なし）
```

### Data Flow

```
アクティブペインの mux PtyOutput
  → handle_pty_output → apply_active_pane_output → core.process_pty_data_fully
  → NativeCallbacks::handle_clipboard → pending_clipboard_writes
  → App::pump_all の排出 → target 対応付け → WindowHost の arboard → CLIPBOARD / PRIMARY
```

- アクティブペインの mux PtyOutput 内の OSC 52 は、`handle_pty_output` → `apply_active_pane_output` → `core.process_pty_data_fully`（`mux_link.rs:399`、`output_pipeline.rs:260-283`）を通って `NativeCallbacks` に届く。タブの core が `NativeCallbacks` を持つ（`tabs/mod.rs:756-764`）。
- 非アクティブな mux ペインの出力は既存のペインルーティング（`mux_link.rs:265-281`）で破棄され、その OSC 52 は失われる。この挙動は変えず、対象外。
- off-thread 切り替えの保留中にキューに入った出力は、swap の後に `App::pump_all` から適用される（`mux_link.rs:283-290` のコメント、`app/mod.rs:1177`）。
- スナップショット再生（同期の `reset_frame_for_replay` 経路、off-thread 再生）の内容に含まれる OSC 52 はクリップボードに適用しない（FR7）。

### API Design

該当なし（外部 API の追加・変更はない）。

### Database Schema

該当なし。

### Dependencies

**Internal Dependencies:**
- `NativeCallbacks::handle_clipboard`: `(target, text)` をキューに積む。クリアは空テキスト。この積み方は `callbacks/tests.rs` の `osc_52_*` と ObservedState のパリティテスト（`tests.rs:1000-1083`）で固定されている
- `WindowHost`: arboard のクリップボードインスタンス（`WindowHost.clipboard`）、`set_clipboard` / `set_primary`（空テキストで早期 return）
- `Tab::process_combined`: mux 前の外側のパース、mux フレーム適用ループ、Detached 後の末尾の再ルーティング
- `App::pump_all`: `tab.pump()`、`tab.poll_pending_switch()` / `poll_pending_scrollback_restore()`、既存の `drain_osc`（`app/mod.rs:1217`）

**External Dependencies:**
- arboard: システムクリップボード（Linux の X11 / Wayland、Windows）へのアクセス

### File Structure

変更するファイルは create-plan で決める。要件分析で参照した既存の箇所:

```
window_host/mod.rs        # set_clipboard / set_primary（:707、:733）
output_pipeline.rs        # process_combined（:260-283、:345-401、:433-497、:543-548）
app/mod.rs                # App::pump_all（:1177、:1217）
mux_link.rs               # ペインルーティング（:265-290、:399）
tabs/mod.rs               # タブの core と NativeCallbacks（:756-764）
callbacks/tests.rs        # osc_52_* テスト、ObservedState パリティテスト
```

## Declared Change Set

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

すべての SPEC は、フィーチャー固有のパスに加えて、ワークフローが生成する次の 2 つをデフォルトで宣言する:

- `feature-docs/osc52-clipboard-write-drain/**`
- `test-docs/osc52-clipboard-write-drain/**`

`feature-docs/osc52-clipboard-write-drain/**` は `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物を含む。生成主体は各フェーズドキュメントと `references/phase-state.md` で、この節は引用のみでルールを再掲しない。

`test-docs/osc52-clipboard-write-drain/**` はタスクごとのテスト記録 `test-docs/osc52-clipboard-write-drain/{T}.tests.yaml` を含む。生成主体は `implement-phase.md` で、この節は引用のみでルールを再掲しない。

この 2 つのデフォルトは、SPEC 作成者が明示的に除外しない限り宣言に含まれる。記載がないことを除外とみなさない。除外は意図的で明示的な絞り込みである。

この宣言はスーパーセットの主張であり、検証時に観測される実際の変更集合は宣言に含まれる（CONTAINED IN）必要があり、一致する必要はない。implement タスクを生成しないフィーチャーは `test-docs/osc52-clipboard-write-drain/` を生成しないが、その場合も宣言された `test-docs/osc52-clipboard-write-drain/**` は正しい。宣言したパスが生成されなくても違反ではない。

## Test Scenarios

### Unit Tests

- [ ] TS-1: 対応付け関数のテーブル駆動テスト。AC-2 に挙げた target 文字列（`c`、`p`、`cp`、`pc`、`''`、`s`、`0`、`7`、`q`、`s0`）を扱い、PRIMARY から CLIPBOARD への置き換えはプラットフォームのパラメータか cfg ごとのケースで確認する - 各 target が AC-2 の書き込み先に対応付けられる（AC-2 / FR1、FR3）
- [ ] TS-2: 記録用の代替 sink（書き込み先ごとの write / clear）に対する排出と適用 - 到着順に書き込まれ最後の書き込みが残る、クリアが sink に届く、適用後に両キューが空、読み取りキューが破棄され PTY への書き込みがない、クリップボードのない sink でパニックしない（AC-2、AC-3、AC-4、AC-7 / FR1、FR2、FR3、FR4、FR8、NFR1）

### Integration Tests（tabs 単位のユニットテスト）

- [ ] TS-3: アクティブペインの PtyOutput フレームに OSC 52 書き込みを含む mux 接続バッファを `Tab::process_combined` に与え、pump 単位の排出を走らせる。Detached 後の末尾に OSC 52 がある場合も同様に確認する - どちらの書き込みもその pump で収集される（AC-5、AC-10 / FR1、FR5、FR6）
- [ ] TS-4: mux 前の外側のパース。通常のシェル出力内の OSC 52 書き込み - その pump の排出で収集される（AC-5 / FR5、FR6）
- [ ] TS-5: 内容に OSC 52 を含む同期のスナップショット再生 - クリップボードのエントリが生じない。off-thread 再生の経路がコールバックに届きうる場合はそのケースも加える（AC-6 / FR7）
- [ ] TS-6: 既存の callbacks の OSC 52 テストを変更せずに実行する - すべて通る（AC-8 / FR9）

### Build Check

- [ ] TS-7: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` - 成功する（AC-9 / NFR4）

### E2E Tests

**Existing E2E tests**: なし
**Run command**: 検出なし

### Manual Check

- [ ] MT-1（ユーザー、リリースビルド）: SSH 越しに再現用の printf を実行してから貼り付ける - 結果が `hello` になる。任意で、リモートの Claude Code の全画面表示でドラッグ選択し、ローカルで貼り付ける（AC-1 / FR1、FR3）

## Security Considerations

- **Authentication:** 該当なし
- **Authorization:** OSC 52 読み取りは応答しない（FR4）。既存の `clipboard_read_osc52` ゲートを残す
- **Input Validation:** `handle_clipboard` の既存の受理ゲート（不正な base64、`clipboard_max_size_osc52` 超過、UTF-8 でないペイロードの拒否）を維持する（FR9）
- **Data Protection:** 追加・変更するログ行にデコード済みテキストも base64 ペイロードも含めない。target、長さ、エラーの種類のみ（NFR3）。スナップショット再生の OSC 52 で現在のクリップボードを上書きしない（FR7）
- **XSS Prevention:** 該当なし
- **SQL Injection Prevention:** 該当なし
- **CSRF Protection:** 該当なし

## Error Handling

### Error Cases

| 事象 | 条件 | 対応 |
|------|------|------|
| クリップボード取得不可 | `WindowHost.clipboard` が `None` | キューを排出する。パニックしない。テキストを含まないログを出す（FR8） |
| 書き込み・クリアの失敗 | set / clear の呼び出しが失敗する | キューを排出する。パニックしない。テキストを含まないログを出す（FR8） |
| 不正な base64 | `handle_clipboard` の受理時 | キューに積まない（FR9） |
| サイズ超過 | デコード後のサイズが `clipboard_max_size_osc52` を超える | キューに積まない。target と長さのみで `LOG_OSC52_DENIED` を出す（FR9） |
| UTF-8 でないペイロード | `handle_clipboard` の受理時 | キューに積まない（FR9） |

### Error Flow

```
set / clear の失敗、またはクリップボードなし → target・長さ・エラーの種類だけをログに出す → 次のエントリへ進む → キューを空にして pump を終える
```

## Performance Optimization

### Performance Goals

- 各 pump の後、すべてのタブで `pending_clipboard_writes` と `pending_clipboard_reads` が空（NFR1）

### Optimization Strategies

- 1 回の pump 内の複数の書き込みは到着順に適用する。書き込み先ごとに最後の書き込みへまとめても、観測される結果は同じ（A5）

### Caching Strategy

該当なし。

## Assumptions

- A1: target の対応付けは `c_and_p_split`（バッチ回答 `osc52.target-mapping`）。`c` は CLIPBOARD、`p` は PRIMARY、両方あれば両方。空・`s`・`0`〜`7`・`q` のみは CLIPBOARD。Windows では PRIMARY を CLIPBOARD に置き換える。クリアも同じ対応付けを使う
- A2: 読み取りキューの扱いは `drain_discard`（バッチ回答 `osc52.read-queue-handling`）。読み取りは毎 pump 排出し、応答しない。読み取りへの応答は別フィーチャー
- A3: target のうち、回答で名指しされた文字以外（例: `x`）はどの書き込み先も選ばず無視する。`c` も `p` もない場合、書き込み先は CLIPBOARD
- A4: バックグラウンドタブの OSC 52 書き込みも適用する（すべてのタブを排出する `drain_osc` と同じ）
- A5: 1 回の pump 内の複数の書き込みは到着順（タブ順、その中でキュー順）に適用する。書き込み先ごとに最後の書き込みへまとめても、観測される結果は同じ
- A6: 確認済みの事実。アクティブペインの mux PtyOutput 内の OSC 52 は `handle_pty_output` → `apply_active_pane_output` → `core.process_pty_data_fully`（`mux_link.rs:399`、`output_pipeline.rs:260-283`）を通って `NativeCallbacks` に届く（タブの core が `NativeCallbacks` を持つ、`tabs/mod.rs:756-764`）。非アクティブな mux ペインの出力は既存のペインルーティング（`mux_link.rs:265-281`）で破棄され、その OSC 52 は失われる。この挙動は変えず、対象外。off-thread 切り替えの保留中にキューに入った出力は swap の後に `App::pump_all` から適用される（`mux_link.rs:283-290` のコメント、`app/mod.rs:1177`）
- A7: 未検証。daemon 側のスナップショット strip が、スナップショット内容から OSC 52 をすでに除去しているか（供給された調査対象の外にある）。FR7 がどちらの場合も求める挙動を定め、追加のガードが要るかは planner が判断する
- A8: クリア後、クリアした選択領域からはテキストが貼り付けられない。arboard の具体的な呼び出し（clear か、空テキストの設定か）は実装の選択とする
- A9: 既存テストで固定済み。`handle_clipboard` のキューへの積み方（`(target, text)` のタプル、クリアは空テキスト）は `callbacks/tests.rs` の `osc_52_*` と ObservedState のパリティテスト（`tests.rs:1000-1083`）で固定されている

## Success Criteria

- [ ] すべての機能要件が実装され、テストされている
- [ ] すべてのテストシナリオ（TS-1〜TS-7）が通る
- [ ] NFR1 のキューの上限を満たす
- [ ] NFR3 のログ要件を満たす
- [ ] ドキュメントがそろっている
- [ ] コードレビューが完了している
- [ ] MT-1 の手動確認で `hello` が貼り付けられる

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし（`status: tbd` の要件はない）。

## Implementation Phases (if applicable)

該当なし（タスク分割は create-plan で行う）。

## References

- 要件定義書: `feature-docs/osc52-clipboard-write-drain/REQUIREMENTS.md`
