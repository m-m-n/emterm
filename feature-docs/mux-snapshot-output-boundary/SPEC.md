# Feature: mux-snapshot-output-boundary

## 概要

mux デーモンで、pane ごとの出力連番と「捕捉の排他」を導入し、shadow parser の更新・リング書き込み・snapshot の取得を同じ出力境界で区切る。snapshot が取り込んだ最後の連番以下のチャンクは、その snapshot を受け取る送信先へ snapshot の後に転送しない。要件の詳細は `feature-docs/mux-snapshot-output-boundary/REQUIREMENTS.md` を参照する。

## 目的

- snapshot に取り込まれた出力チャンクが、接続後にもう一度転送・適用されないようにする（タブ切替・reattach・可視化復帰時の一時的な表示崩れをなくす）。
- shadow parser の更新・リング書き込み・snapshot の取得を、同じ出力境界で区切る。

## 受け入れ基準

- [ ] AC-1（FR1・FR2・FR3）: reader を shadow・リングの更新と転送のあいだで止めたまま snapshot を取り、reader を再開する並行処理テストがある。「snapshot の後に届くチャンク」を適用した結果が、生のストリームを 1 回だけ適用した結果と一致する（例: `ESC[L` を含むチャンクが 1 回だけ適用される）。visible reattach、可視化復帰（`resume_pane_with_permit`）、オンデマンド snapshot の 3 経路それぞれで確認する。修正前のコードではこのテストが失敗する。
- [ ] AC-2（FR2）: snapshot が取得したリングと shadow parser は、同じ集合のチャンクを反映している。reader が shadow を更新してからリングに書くまでのあいだに snapshot を取ろうとしても、shadow にだけ含まれるチャンクが生じない。resize と snapshot が並行しても、取得したリングの寸法マーカーと shadow parser の寸法が食い違わない。
- [ ] AC-3（FR4・FR11）: snapshot の取得時にチャネル満杯で送信待ちだったチャンクが、snapshot より後にクライアントへ届かない。
- [ ] AC-4（FR5）: 境界より後のチャンクと EOF の空チャンクは、欠けずに転送される。サイズ上限で snapshot を送らなかった場合は、チャンクが抑止されない。
- [ ] AC-5（FR6）: 抑止したチャンクに含まれる OSC 9 について、Detached と同じ通知処理が 1 回実行され、通知チャネルに 1 件届く（`try_send` が成功する条件で）。そのチャンクはクライアントに転送されないので、GUI 側では発火しない。抑止したチャンクと次のチャンクにまたがる OSC 9 は二重に発火せず、後の Detached 期間の出力とつながった通知も生じない。
- [ ] AC-6（NFR1）: snapshot のバイト列の形を固定している既存テスト（`reattach/tests.rs`、`pane/tests.rs`、`pty_spawn/tests.rs` の apt / リング一周系）が、変更なしで通る。
- [ ] AC-7（NFR2・NFR5）: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` と `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が通る。
- [ ] AC-8（FR7）: FR7 の対象コメントが、新しい保証と矛盾しない。
- [ ] AC-9（FR8）: 抑止したチャンクに含まれるタイトル変更（OSC 0/2）、agent-status の報告、OSC 133 マーク、OSC 7 の cwd が、抑止しない場合と同じく反映・送信される。
- [ ] AC-10（FR9）: 抑止したチャンクに含まれる端末問い合わせ（例: `ESC[6n`、`ESC[c`）が、snapshot の後、境界後のチャンクより前に、送信先へちょうど 1 回届く。snapshot のバイト列に残る問い合わせは、もう一度は届かない。
- [ ] AC-11（FR10）: 抑止したチャンクの末尾で未完了の CSI・OSC・UTF-8 文字、および `ScrollbackWriteFilter` が pending に保留したリッチコンテンツ候補について、snapshot とその後に届くバイトを `term_core` に流した結果が、生のストリームを 1 回だけ流した参照と一致し、続きのバイトや置換文字が表示されない。
- [ ] AC-12（FR11）: 送信先 A 向けに記録した境界が、送信先 B へのチャンクを抑止しない。境界より大きいチャンクは、どの送信先でも欠けない。
- [ ] AC-13（NFR2）: reader・resize・snapshot の 4 経路を並行して繰り返し走らせるテストが、デッドロックせず時間内に終わる。

## 技術要件

### 機能要件

- **FR1:** pane ごとの出力連番。PTY reader（`pty_reader_loop`）は pane ごとに、読み取ったチャンク 1 つにつき単調増加の出力連番を 1 つ割り当てる。連番はデーモン内部の状態であり、snapshot のバイト列と `mux_ipc` のワイヤ形式には載せない。
- **FR2:** 更新と取得を同じ排他で区切る。チャンクの shadow parser 更新・リング書き込み・連番の確定は、pane ごとの 1 つの排他（捕捉の排他）の中で行う。snapshot は同じ捕捉の排他の中で、リングの内容・shadow parser の状態・取り込んだ最後の連番を読み取る。これで、shadow ダンプには含まれるがリングには含まれないチャンク（現象 b）が生じない。対象は `collect_reattach_data`（visible reattach）、`resume_pane_with_permit`、`evaluate_output_target` の `ResumeWithSnapshot` 分岐、`handle_request_pane_snapshot` の 4 つの snapshot 組み立て経路すべて。`MuxPane::resize` のリングへの寸法マーカー記録と shadow parser の寸法変更も、同じ捕捉の排他の中で行う。
- **FR3:** 取り込み済みチャンクを再転送しない。snapshot が取り込んだ最後の連番以下のチャンクは、その snapshot を受け取るクライアントへ snapshot の後に転送しない（現象 a）。FR2 の 4 経路すべてに適用する。
- **FR4:** バックプレッシャ経路も対象にする。チャネルが満杯で、reader が `output_target` ロックの外で `blocking_send` に入ったチャンクも FR3 の対象にする。snapshot を取得する時点で送信先がすでに決まっていたチャンクが snapshot より後に届き、二重に適用される経路を残さない。
- **FR5:** 取り込んでいないチャンクまで止めない。snapshot が取り込んだ最後の連番より大きいチャンクは、従来どおり転送する。PTY EOF の空チャンク（終了通知）は抑止しない。サイズ上限を超えて snapshot を送らない経路（`handle_request_pane_snapshot` の拒否、`resume_pane_with_permit` の `NoChange`、`evaluate_output_target` の `Unchanged`）では抑止を記録しない。hidden reattach（`visible=false`）は snapshot を作らないので、抑止も記録しない。
- **FR6:** 抑止したチャンクの通知とリッチコンテンツ。reader が転送を抑止したチャンクは、Detached 腕と同じ `capture_passthrough` を 1 回通す。OSC 9 のデスクトップ通知は、Detached と同じ通知処理（デーモンの通知チャネルへの送信）を 1 回実行する。通知チャネルへの `try_send` が失敗したときの既存の欠落までは保証しない。抑止したチャンクで始まり境界後のチャンクで終わる OSC 9 は、二重に発火しない。そのチャンクで `passthrough_scanner` に残った途中状態が、後の Detached 期間の出力とつながって別の通知を生まない。ビューア起動や画像などのリッチコンテンツは、snapshot の既存方針（`strip_replayable_rich_content` で除去し、`raw_passthrough` は再生しない）どおり再生しない。抑止した 1 チャンクの中だけで完結するビューア起動・画像のシーケンスは、クライアントに届かない。これは仕様上の既知の欠落とする。
- **FR7:** コメントを新しい保証に合わせる。この境界が無いことを前提にしたコメントを書き直す。対象は、`handle_request_pane_snapshot` の「A narrow race window remains」の段落、`resume_pane_with_permit` の FR9 の説明、`collect_reattach_data` 内の順序の説明、`capture_passthrough` の doc コメントの「Called ONLY from the Detached arms」と「On the Connected arm the scanner is never run」、`pty_reader_loop` の転送箇所の「The Detached arms also feed passthrough_scanner」。
- **FR8:** 抑止は転送だけに効く。抑止の対象は `output_target` 経由の転送だけとする。reader が転送とは別に行う既存の処理は、抑止したチャンクでも抑止しないチャンクと同じく実行する。対象は shadow parser の更新、リング書き込み、タイトル変更の送信（`title_sender`）、agent-status の報告と OSC 133 マークの送信（`forward_agent_status_items`）、OSC 7 による cwd の記録、osc-probe のログ出力。
- **FR9:** 抑止したチャンクの端末問い合わせ。デーモンは端末問い合わせ（DSR・DA など）に応答せず、応答するのは生の出力を受け取ったクライアントの `term_core` だけである。snapshot のバイト列からは端末問い合わせが除去される。抑止したチャンクに含まれる端末問い合わせのうち snapshot のバイト列に残らないものは、snapshot を受け取った送信先へ、snapshot の後、境界より後のチャンクより前に、元の順序で 1 回だけ届ける。クライアントはそれぞれに 1 回だけ応答する。snapshot のバイト列に残る問い合わせは、もう一度は届けない。
- **FR10:** 境界をまたぐシーケンスの続きを文字として表示しない。抑止したチャンクの末尾で未完了の制御シーケンスや UTF-8 文字があるとき、境界後に転送するチャンクの先頭にある続きのバイトが、クライアントで文字や置換文字として表示されない。そのシーケンスはクライアントで 1 回だけ処理される。`ScrollbackWriteFilter` が pending に保留したバイトもこれに含む（pending のバイトはリングに入っていないので、snapshot にも含まれない）。境界をまたいで完結するリッチコンテンツ（ビューア起動・画像）は、クライアントに 1 回届いてよい。未完了部分の長さが計画で決める上限を超える場合は、続きが表示されうる。これは既知の欠落とする。
- **FR11:** 抑止の境界を送信先と結び付ける。抑止の境界（取り込んだ最後の連番）は、snapshot を受け取る送信先（その接続の `pane_output_tx`）と組にして記録する。reader がチャンクを抑止するのは、転送先がその送信先と同じで、チャンクの連番が境界以下のときだけとする。別の送信先への転送は、境界で抑止しない。抑止するかどうかは、判定の時点ではなく、そのチャンクと snapshot が送信先のチャネルに入る順序で整合させる。境界以下のチャンクは、`blocking_send` で待つあいだに snapshot が先にチャネルに入った場合も含めて、その送信先で snapshot の後に届かない。snapshot が `DeferredOutputQueue` で送信延期され、境界以下のチャンクが snapshot より前にチャネルに入る場合は、転送しても抑止してもよい（snapshot が上書きする）。境界より大きいチャンクは、その送信先で欠けない。

### 非機能要件

- **NFR1:** snapshot のバイト列とワイヤ形式を変えない。snapshot のバイト列の形を変えない（apt 進捗バー残骸の既知課題の制約）。`mux_ipc` のワイヤ形式と、`SnapshotRestore` / `Snapshot` のフレームの形も変えない。FR9・FR10 で snapshot の後に届けるバイトは、既存の `PtyOutput` チャンクとして送る。
- **NFR2:** ロック規律とデッドロック回避。`output_target` ロックを保持したまま `blocking_send` しない（G1 の規律、`pty_spawn/mod.rs` 493-495 行と 548-567 行、EOF 分岐の回帰テスト）。捕捉の排他は、reader・snapshot の 4 経路・`MuxPane::resize` のすべてで、リングと shadow parser のロックより先に取る。reader は捕捉の排他を解放してから `output_target` を取る。`output_target` を持ったまま捕捉の排他を取る経路（`resume_pane_with_permit`、`evaluate_output_target`、境界を読む reader の転送判定）は、順序を `output_target` → 捕捉の排他 → リング・shadow parser に統一する。捕捉の排他を持ったまま `output_target` を取る経路は作らない。現状のコードには、リングと shadow parser を逆順に同時に持つ経路は無い。resize がリングのロックを `master.resize()` をまたいで持つ点（`pane/tests.rs` 1270-1324 行）も、この順序の監査に含める。
- **NFR3:** ロック内処理を増やさない。tokio ワーカー上で `output_target` ロックの中で行う処理を増やさない。今ロックの外で行っているオンデマンド snapshot の組み立てとプローブ再生を、`output_target` ロックの中へ移さない（別タスク「リング一周後の snapshot プローブ再生がロック内で走る」と衝突させない）。snapshot 側が捕捉の排他を持つのは、リング・shadow parser の状態・最後の連番を読み取るあいだだけとする。snapshot の組み立て・エンコード・プローブ再生・送信は、その排他の外で行う。
- **NFR4:** reader の通常経路のコスト。reader の通常経路（抑止しないチャンク）に足すコストは、連番の加算と比較、競合しない排他の取得程度に留める（低遅延の維持）。FR9・FR10 の問い合わせの抜き出しと未完了部分の扱いは、抑止したチャンクだけで行う。
- **NFR5:** ビルド構成とプラットフォーム。`--no-default-features`（CLI と mux のみ）のビルドが通る。Linux と Windows で同じように動く（プラットフォーム固有の API を使わない）。

## 実装方針

### アーキテクチャ

pane ごとの出力連番の方式を採る。更新から転送までを 1 つのクリティカルセクションに収める方式は採らない（ASM-1）。

**ロック順序（NFR2）:**

```
output_target → 捕捉の排他 → リング・shadow parser
```

- reader・snapshot の 4 経路・`MuxPane::resize` は、捕捉の排他をリングと shadow parser のロックより先に取る。
- reader は捕捉の排他を解放してから `output_target` を取る。
- 捕捉の排他を持ったまま `output_target` を取る経路は作らない。
- `output_target` ロックを保持したまま `blocking_send` しない。

### データフロー

```
PTY 読み取り
  └─ 捕捉の排他の中
       ├─ 出力連番の確定（FR1）
       ├─ shadow parser 更新（FR2）
       └─ リング書き込み（FR2）
  └─ 捕捉の排他を解放
  └─ output_target 経由の転送判定（FR3・FR11）
       ├─ 転送先が境界を記録した送信先と同じ、かつ連番 ≤ 境界 → 転送を抑止
       │     ├─ capture_passthrough を 1 回通す（FR6）
       │     ├─ snapshot に残らない端末問い合わせを snapshot の後に届ける（FR9）
       │     └─ 境界をまたぐ未完了部分を扱う（FR10）
       └─ それ以外 → 従来どおり転送（FR5）
  └─ 転送とは別の既存処理は抑止に関係なく実行（FR8）

snapshot 組み立て（4 経路）
  └─ 捕捉の排他の中: リング・shadow parser の状態・最後の連番を読み取る（FR2）
  └─ 境界を送信先（pane_output_tx）と組にして記録（FR11）
  └─ 捕捉の排他の外: 組み立て・エンコード・プローブ再生・送信（NFR3）
```

### デーモン内部の状態

| 項目 | 説明 |
|------|------|
| 出力連番 | pane ごとに、読み取ったチャンク 1 つにつき単調増加で 1 つ割り当てる（FR1）。 |
| 抑止の境界 | snapshot が取り込んだ最後の連番。snapshot を受け取る送信先（`pane_output_tx`）と組にして記録する（FR11）。 |

- 出力連番と抑止の境界は、デーモンの 1 回の起動のあいだだけメモリ上に持つ。hot-upgrade の引き継ぎ形式には加えない。復元した pane（`mux::upgrade` が `pty_reader_loop` を呼び直す経路）では連番を初期値から数え直す（ASM-3）。
- 同じ送信先について記録する最後の連番は単調に増やす（後退させない）。

### API 設計

該当なし。`mux_ipc` のワイヤ形式と `SnapshotRestore` / `Snapshot` のフレームの形は変えない（NFR1）。FR9・FR10 で snapshot の後に届けるバイトは、既存の `PtyOutput` チャンクとして送る。

### データベーススキーマ

該当なし。

### 依存関係

**内部依存（requirements_analysis に記載のある箇所）:**
- `pty_reader_loop`（`pty_spawn/mod.rs`）: 出力連番の割り当て、転送判定、抑止。
- `collect_reattach_data`（`reattach.rs`）: visible reattach の snapshot 経路。
- `resume_pane_with_permit`: 可視化復帰の snapshot 経路。
- `evaluate_output_target` の `ResumeWithSnapshot` 分岐: snapshot 経路。
- `handle_request_pane_snapshot`（`handlers/mod.rs`）: オンデマンド snapshot 経路。
- `MuxPane::resize`（`session/pane/mod.rs`）: 寸法マーカー記録と shadow parser の寸法変更。
- `capture_passthrough` / `passthrough_scanner`: 抑止したチャンクの通知処理。
- `ScrollbackWriteFilter` / `strip_replayable_rich_content`（`src-tauri/src/mux/scrollback_filter.rs`）: pending の保留と snapshot からの除去。
- `DeferredOutputQueue`: snapshot の送信延期。

**外部依存:**
- 該当なし。

### ファイル構成

このフィーチャーで変更するファイルは、create-plan で `workflow.yaml` の各タスクの `files` から導出する。

## 宣言された変更集合

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/mux-snapshot-output-boundary/**`
- `test-docs/mux-snapshot-output-boundary/**`

`feature-docs/mux-snapshot-output-boundary/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/mux-snapshot-output-boundary/**` covers `test-docs/mux-snapshot-output-boundary/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/mux-snapshot-output-boundary/` directory at all; the declared
`test-docs/mux-snapshot-output-boundary/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## テストシナリオ

並行処理テストを決定的にするため、reader の更新と転送のあいだにテスト専用の停止点（`cfg(test)`）を設けてよい。reader は `pty_reader_loop` に `Box<dyn Read>` を渡すかたちで既存テストから駆動できる（`pty_spawn/tests.rs` 2302 行ほか）（ASM-2）。

### 並行処理テスト

- [ ] TS-1（AC-1 / FR1, FR2, FR3）: visible reattach のあいだの二重適用 - Detached の pane で reader が `ESC[L` を含むチャンクの shadow とリングを更新したところで止める → `collect_reattach_data(visible=true)` を実行する → reader を再開する → 新しい送信先に、そのチャンクが `PtyOutput` として届かないことを確かめる。snapshot とその後のチャンクを `term_core` に流した結果が、生のストリームを流した参照と一致することも確かめる。
- [ ] TS-2（AC-1 / FR1, FR2, FR3）: 可視化復帰のあいだの二重適用 - `Detached{HiddenByVisibility}` の pane で TS-1 と同じ止め方をする → `resume_pane_with_permit` → reader を再開する → チャネル上の snapshot より後に、そのチャンクが無いことを確かめる。
- [ ] TS-3（AC-1 / FR1, FR2, FR3）: オンデマンド snapshot のあいだの二重適用 - Connected の pane で TS-1 と同じ止め方をする → `handle_request_pane_snapshot` と同じ組み立てと enqueue を行う → reader を再開する → チャネルの順序が [snapshot] になり、[snapshot, 同じチャンク] にならないことを確かめる。
- [ ] TS-4（AC-2 / FR2）: shadow とリングの食い違い - reader が shadow を更新してからリングに書くまでのあいだで snapshot を取ろうとする → 取得したリングと shadow ダンプが、同じ最後の連番に対応していることを確かめる。resize を snapshot と並行させ、リングの寸法マーカーと shadow parser の寸法が一致することも確かめる。
- [ ] TS-5（AC-3 / FR4, FR11）: バックプレッシャ経路 - 容量の小さいチャネルを満杯にして、reader を `blocking_send` 待ち（および `try_send` が Full を返してから `blocking_send` に入るまでのあいだ）にする → オンデマンド snapshot を取る → 満杯を解消する → 待っていたチャンクが snapshot より後に届かないことを確かめる。
- [ ] TS-12（AC-13 / NFR2）: ロック順序 - reader に出力を流し続けながら、resize と 4 つの snapshot 経路を別スレッド・別タスクから繰り返し呼ぶ → 時間内に終わることを確かめる。

### 抑止の範囲と付随処理のテスト

- [ ] TS-6（AC-4 / FR5）: 過剰抑止が無いこと - snapshot の後に新しいチャンクと EOF を流す → すべて転送されることを確かめる。サイズ上限を超えた snapshot では、in-flight のチャンクが転送されることを確かめる。
- [ ] TS-7（AC-5 / FR6）: 抑止したチャンクの OSC 9 - OSC 9 を含むチャンクを TS-3 と同じ条件で抑止する → 通知チャネルにちょうど 1 件届き、送信先にはそのチャンクが届かないことを確かめる。OSC 9 を抑止したチャンクと次のチャンクに分けて流し、二重に発火しないこと、さらに後で Detached にして出力を流しても、つながった通知が出ないことを確かめる。
- [ ] TS-8（AC-9 / FR8）: 転送以外の処理 - OSC 2 のタイトル、OSC 777 の agent-status 報告、OSC 133 マーク、OSC 7 を含むチャンクを TS-3 と同じ条件で抑止する → `title_sender`・agent-status の送信・pane の cwd に、抑止しない場合と同じ結果が出ることを確かめる。
- [ ] TS-9（AC-10 / FR9）: 抑止したチャンクの端末問い合わせ - `ESC[6n` と `ESC[c` を含むチャンクを TS-1・TS-3 と同じ条件で抑止する → 送信先のチャネルで、snapshot の後、次のチャンクより前に、問い合わせがちょうど 1 回ずつ届くことを確かめる。
- [ ] TS-10（AC-11 / FR10）: 境界をまたぐシーケンス - 抑止するチャンクの末尾を (a) 途中で切れた CSI、(b) 途中で切れた UTF-8 文字、(c) `ScrollbackWriteFilter` が pending に保留するリッチコンテンツ候補の先頭 にして、続きを次のチャンクに置く → TS-1 と同じ条件で抑止する → snapshot とその後に届くバイトを `term_core` に流した結果が参照と一致し、続きのバイトや置換文字が画面に出ないことを確かめる。
- [ ] TS-11（AC-12 / FR11）: 送信先との結び付け - 送信先 A 向けに境界を記録する → 別の送信先 B への reattach による乗っ取りのあと、B への転送が A の境界で抑止されないことを確かめる。

### 既存テストとビルド

- [ ] AC-6（NFR1）: snapshot のバイト列の形を固定している既存テスト（`reattach/tests.rs`、`pane/tests.rs`、`pty_spawn/tests.rs` の apt / リング一周系）が変更なしで通る。
- [ ] AC-7（NFR2・NFR5）: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` と `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が通る。

### E2E テスト

**既存の E2E テスト**: なし（`resolved_input_paths.e2e` は空）
**実行コマンド**: 検出なし

### エッジケース

- [ ] reader は pane ごとに逐次動くので、境界をまたいで in-flight になるチャンクは 1 つまで。抑止の影響も snapshot 1 回につきチャンク 1 つまで。
- [ ] vt100 の panic で shadow parser を作り直したチャンクにも、連番を割り当てる。
- [ ] alt-screen 中のチャンクや alt 切替を含むチャンクは、リングに書かない（または一部だけ書く）が、連番は進める。
- [ ] `ScrollbackWriteFilter` が次の読み取りまで保留した末尾バイト（pending、リッチコンテンツ候補）は、連番 N のチャンクに属していても、リングには N+1 以降と一緒に書かれるため snapshot には含まれない。N を抑止した場合、この保留分は FR10 に従って扱い、N+1 の先頭にある続きが文字として漏れないようにする。上限を超えて生のままリングへ吐き出す場合も含めて FR10 の上限の扱いに従う。
- [ ] オンデマンド snapshot を短い間隔で続けて取る場合や、`DeferredOutputQueue` での同じ pane の合流（新しいほうが勝つ）では、同じ送信先について記録する最後の連番は単調に増やす（後退させない）。
- [ ] `DeferredOutputQueue` が、別 pane の上限超えで古いオンデマンド snapshot を捨てた場合、その snapshot のために抑止したチャンク 1 つは、クライアントが snapshot を要求し直すまで表示に出ない（既存の回復経路は再要求）。
- [ ] `DeferredOutputQueue` に積まれた `VisibilityResume` は、送信時に `resume_pane_with_permit` を通るので、境界はその時点で記録される。
- [ ] 別クライアントからの reattach による乗っ取りでは、境界は新しい送信先と組で記録される（FR11）。古いクライアントには kick が送られる。古い送信先へ送信待ちだったチャンクは、新しい送信先の境界では抑止しない。
- [ ] 送信待ちの Full 経路で送信先が閉じた場合の Detached への切替（`NetworkDetach`）は、従来どおり動く。
- [ ] 抑止したチャンクを `capture_passthrough` に通すと、そこから抜き出した画像・Markdown のシーケンスが `raw_passthrough` に入る。これは次の復帰で読み捨てられ、再生されない（既存の方針どおり）。
- [ ] DSR 6n（カーソル位置の問い合わせ）を snapshot の後に届けた場合、クライアントが返す位置は、snapshot を適用した後のカーソル位置になる。問い合わせはチャンク末尾に置かれることが多く、修正前もチャンクを二重に適用した途中の位置で応答していたので、この差は許容する。
- [ ] 端末問い合わせが抑止したチャンクの末尾で途中までしか無い場合は、FR10 の未完了のシーケンスとして扱い、続きのチャンクとつながって 1 回だけ応答される。
- [ ] Detached の期間の最後に `passthrough_scanner` に途中状態が残ったまま Connected に戻る場合の途中状態の扱い（抑止とは関係の無い既存の経路）は、このタスクで変えなくてよい。FR6 が求めるのは、抑止したチャンクで残った途中状態についてだけ。

### 性能テスト

- 該当する数値目標は無い。reader の通常経路に足すコストは NFR4 の範囲に留める。

## セキュリティ

- **認可:** `handle_request_pane_snapshot` にある、接続中のセッションに属する pane だけに snapshot を返す認可の検査は、変えない。
- **データ保護:** FR9・FR10 で snapshot の後に届けるバイトは、その snapshot を受け取った送信先にだけ送る。

## エラー処理

- 通知チャネルへの `try_send` が失敗したときの既存の欠落（警告ログを出して捨てる）は、そのまま残す（ASM-8）。
- 送信待ちの Full 経路で送信先が閉じた場合の Detached への切替（`NetworkDetach`）は、従来どおり動く。

## 性能

- tokio ワーカー上で `output_target` ロックの中で行う処理を増やさない（NFR3）。
- snapshot 側が捕捉の排他を持つのは、リング・shadow parser の状態・最後の連番を読み取るあいだだけとする。組み立て・エンコード・プローブ再生・送信は排他の外で行う（NFR3）。
- reader の通常経路（抑止しないチャンク）に足すコストは、連番の加算と比較、競合しない排他の取得程度に留める。FR9・FR10 の処理は抑止したチャンクだけで行う（NFR4）。

## 前提事項

- ASM-1: 完了の定義が挙げる 2 つの方式のうち、pane ごとの出力連番の方式を採る。更新から転送までを 1 つのクリティカルセクションに収める方式は採らない。理由は 3 つ。(1) 転送の一部である `blocking_send` は、デッドロックを避けるため `output_target` ロックの外で行う必要がある（`pty_spawn/mod.rs` 493-495 行、548-567 行、G1）。そのため 1 つのクリティカルセクションではバックプレッシャ経路を覆えない。(2) オンデマンド経路（`handlers/mod.rs` 459 行以降）は `output_target` を取らずに組み立てている。クリティカルセクション方式にすると、組み立てを tokio ワーカー上のロック内へ移すことになる（NFR3 に反する）。(3) 連番はデーモン内部だけに持てるので、snapshot のバイト列の制約（NFR1）と両立する。
- ASM-2: AC-1 と AC-3 の並行処理テストを決定的にするため、reader の更新と転送のあいだにテスト専用の停止点（`cfg(test)`）を設けてよい。reader は `pty_reader_loop` に `Box<dyn Read>` を渡すかたちで既存テストから駆動できる（`pty_spawn/tests.rs` 2302 行ほか）。
- ASM-3: 出力連番と「snapshot が取り込んだ最後の連番」は、デーモンの 1 回の起動のあいだだけメモリ上に持つ。hot-upgrade の引き継ぎ形式には加えない。復元した pane（`mux::upgrade` が `pty_reader_loop` を呼び直す経路）では連番を初期値から数え直す。
- ASM-4: snapshot のバイト列の形を変えない（apt 進捗バーの既知課題）。これはタスクが課す守るべき制約。
- ASM-5: チャンクが表示から一時的に消える向きの既知の隙間は、このタスクの対象外とする。このタスクが扱うのは二重適用だけ。対象外の隙間は 2 つある。(1) 境界より後に作られたチャンクが、後回しにされたオンデマンド snapshot より先に届き、snapshot に上書きされる（`DeferredOutputQueue` のコメント 248-262 行）。(2) `collect_reattach_data` が snapshot を読んでから送信先を Connected に切り替えるまで（`reattach.rs` 216-237 行）に reader が処理したチャンクは、Detached として捕捉されるだけで、snapshot にも転送にも入らない。
- ASM-6: リング一周後のプローブ再生を `output_target` ロックの外へ移す件は、別タスクのスコープとする。
- ASM-7: デーモンの mux 実装は端末問い合わせに応答しない（mux 配下に応答処理が無い）。応答は、生の出力を受け取ったクライアント側の `term_core` が返す。snapshot のバイト列から端末問い合わせ（DSR・DA・XTWINOPS・DECRPM）を除去するのは、`strip_replayable_rich_content` の既存の挙動である。FR9 はこの事実を前提にする。
- ASM-8: FR6 の OSC 9 は「Detached と同じ通知処理を 1 回実行する」ことまでを保証する。通知チャネルへの `try_send` が失敗したときの既存の欠落（警告ログを出して捨てる）は、そのまま残す。
- ASM-9: FR6 の方式として、抑止したチャンクを Detached 腕と同じ `capture_passthrough` に通す案（案 A）を採る。抑止したチャンクを snapshot の後に再生する案（案 B）は採らない。案 B は、`PassthroughScanner` が OSC 777 を抜き出さないこと、OSC 9999・DCS の検出が strip の処理と一致しないこと、復元後の再生でカーソルと描画の順序が崩れることから退けた。
- ASM-10: FR10 では、境界をまたぐシーケンスを両側とも捨てるのではなく、クライアントで 1 回だけ処理させる。これは修正前（抑止したチャンクがもう一度転送され、続きのチャンクとつながっていた）と同じ結果になる。

## 成功基準

- [ ] すべての機能要件が実装され、テストされている
- [ ] すべてのテストシナリオが通る
- [ ] 非機能要件（NFR1〜NFR5）を満たす
- [ ] セキュリティ要件を満たす
- [ ] FR7 の対象コメントが新しい保証と矛盾しない
- [ ] コードレビューが完了している

## 未解決事項

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- なし（`status: tbd` の要件は無い）。

**計画で決める事項:**
- OSC 10/11/12/4 の色問い合わせを、snapshot のバイト列から除去する端末問い合わせの対象に含むか（FR9、ASM-7）。
- FR10 の実現方法と、未完了部分の長さの上限。
- FR4・FR11 の順序の整合の方式（`try_send` が Full を返してから `blocking_send` の待ち行列に入るまでのあいだに、snapshot が先にチャネルに入りうる）。

## 実装フェーズ

該当なし（create-plan で決める）。

## 参考資料

- 要件定義書: `feature-docs/mux-snapshot-output-boundary/REQUIREMENTS.md`
