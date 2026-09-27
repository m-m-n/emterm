---
title: "mux-snapshot-output-boundary"
created_date: 2026-09-27
status: draft
---

# mux-snapshot-output-boundary - 要件定義書

## 1. 概要

### 1.1 背景

mux デーモンの snapshot と出力転送のあいだに、出力の境界が無い。そのため次の 2 つの現象が起きる。

- 現象 a: snapshot に取り込まれた出力チャンクが、接続後にもう一度転送・適用される。
- 現象 b: shadow ダンプには含まれるが、リングには含まれないチャンクが生じる。

### 1.2 目的

- snapshot に取り込まれた出力チャンクが、接続後にもう一度転送・適用されないようにする（タブ切替・reattach・可視化復帰時の一時的な表示崩れをなくす）。
- shadow parser の更新・リング書き込み・snapshot の取得を、同じ出力境界で区切る。

### 1.3 スコープ

**対象**:

- pane ごとの出力連番（FR1）
- shadow parser の更新・リング書き込み・連番の確定と、snapshot の取得を同じ排他で区切ること（FR2）
- 次の 4 つの snapshot 組み立て経路
    - `collect_reattach_data`（visible reattach）
    - `resume_pane_with_permit`
    - `evaluate_output_target` の `ResumeWithSnapshot` 分岐
    - `handle_request_pane_snapshot`
- 取り込み済みチャンクの再転送の抑止と、それに伴う通知・端末問い合わせ・境界をまたぐシーケンスの扱い（FR3〜FR11）

**対象外**:

- チャンクが表示から一時的に消える向きの既知の隙間（ASM-5）。このタスクが扱うのは二重適用だけ。
- リング一周後のプローブ再生を `output_target` ロックの外へ移す件（ASM-6、別タスク）。
- Detached の期間の最後に `passthrough_scanner` に途中状態が残ったまま Connected に戻る場合の途中状態の扱い（抑止とは関係の無い既存の経路）。

## 2. ビジネス要件

### 2.1 ビジネス目標

- snapshot に取り込まれた出力チャンクが、接続後にもう一度転送・適用されないようにする（タブ切替・reattach・可視化復帰時の一時的な表示崩れをなくす）。
- shadow parser の更新・リング書き込み・snapshot の取得を、同じ出力境界で区切る。

### 2.2 対象ユーザー

指定なし。

### 2.3 期待される効果

- タブ切替・reattach・可視化復帰時の一時的な表示崩れがなくなる。

## 3. ユースケース

該当なし（mux デーモン内部の Rust だけの変更）。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 状態 |
|----|--------|------|
| FR1 | pane ごとの出力連番 | resolved |
| FR2 | 更新と取得を同じ排他で区切る | resolved |
| FR3 | 取り込み済みチャンクを再転送しない | resolved |
| FR4 | バックプレッシャ経路も対象にする | resolved |
| FR5 | 取り込んでいないチャンクまで止めない | resolved |
| FR6 | 抑止したチャンクの通知とリッチコンテンツ | resolved |
| FR7 | コメントを新しい保証に合わせる | resolved |
| FR8 | 抑止は転送だけに効く | resolved |
| FR9 | 抑止したチャンクの端末問い合わせ | resolved |
| FR10 | 境界をまたぐシーケンスの続きを文字として表示しない | resolved |
| FR11 | 抑止の境界を送信先と結び付ける | resolved |

### 4.2 機能詳細

#### FR1: pane ごとの出力連番

PTY reader（`pty_reader_loop`）は pane ごとに、読み取ったチャンク 1 つにつき単調増加の出力連番を 1 つ割り当てる。連番はデーモン内部の状態であり、snapshot のバイト列と `mux_ipc` のワイヤ形式には載せない。

#### FR2: 更新と取得を同じ排他で区切る

チャンクの shadow parser 更新・リング書き込み・連番の確定は、pane ごとの 1 つの排他（以下「捕捉の排他」）の中で行う。snapshot は同じ捕捉の排他の中で、リングの内容・shadow parser の状態・取り込んだ最後の連番を読み取る。これで、shadow ダンプには含まれるがリングには含まれないチャンク（現象 b）が生じない。

対象は次の 4 つの snapshot 組み立て経路すべて。

- `collect_reattach_data`（visible reattach）
- `resume_pane_with_permit`
- `evaluate_output_target` の `ResumeWithSnapshot` 分岐
- `handle_request_pane_snapshot`

`MuxPane::resize` のリングへの寸法マーカー記録と shadow parser の寸法変更も、同じ捕捉の排他の中で行う。

#### FR3: 取り込み済みチャンクを再転送しない

snapshot が取り込んだ最後の連番以下のチャンクは、その snapshot を受け取るクライアントへ snapshot の後に転送しない（現象 a）。FR2 の 4 経路すべてに適用する。

#### FR4: バックプレッシャ経路も対象にする

チャネルが満杯で、reader が `output_target` ロックの外で `blocking_send` に入ったチャンクも FR3 の対象にする。snapshot を取得する時点で送信先がすでに決まっていたチャンクが snapshot より後に届き、二重に適用される経路を残さない。

#### FR5: 取り込んでいないチャンクまで止めない

- snapshot が取り込んだ最後の連番より大きいチャンクは、従来どおり転送する。
- PTY EOF の空チャンク（終了通知）は抑止しない。
- サイズ上限を超えて snapshot を送らない経路では抑止を記録しない。対象は次の 3 つ。
    - `handle_request_pane_snapshot` の拒否
    - `resume_pane_with_permit` の `NoChange`
    - `evaluate_output_target` の `Unchanged`
- hidden reattach（`visible=false`）は snapshot を作らないので、抑止も記録しない。

#### FR6: 抑止したチャンクの通知とリッチコンテンツ

- reader が転送を抑止したチャンクは、Detached 腕と同じ `capture_passthrough` を 1 回通す。
- OSC 9 のデスクトップ通知は、Detached と同じ通知処理（デーモンの通知チャネルへの送信）を 1 回実行する。通知チャネルへの `try_send` が失敗したときの既存の欠落までは保証しない。
- 抑止したチャンクで始まり境界後のチャンクで終わる OSC 9 は、二重に発火しない。
- そのチャンクで `passthrough_scanner` に残った途中状態が、後の Detached 期間の出力とつながって別の通知を生まない。
- ビューア起動や画像などのリッチコンテンツは、snapshot の既存方針（`strip_replayable_rich_content` で除去し、`raw_passthrough` は再生しない）どおり再生しない。
- 抑止した 1 チャンクの中だけで完結するビューア起動・画像のシーケンスは、クライアントに届かない。これは仕様上の既知の欠落とする。

#### FR7: コメントを新しい保証に合わせる

この境界が無いことを前提にしたコメントを、新しい保証に合わせて書き直す。対象は次のとおり。

- `handle_request_pane_snapshot` の「A narrow race window remains」の段落
- `resume_pane_with_permit` の FR9 の説明
- `collect_reattach_data` 内の順序の説明
- `capture_passthrough` の doc コメントの「Called ONLY from the Detached arms」と「On the Connected arm the scanner is never run」
- `pty_reader_loop` の転送箇所の「The Detached arms also feed passthrough_scanner」

#### FR8: 抑止は転送だけに効く

抑止の対象は `output_target` 経由の転送だけとする。reader が転送とは別に行う既存の処理は、抑止したチャンクでも抑止しないチャンクと同じく実行する。対象は次のとおり。

- shadow parser の更新
- リング書き込み
- タイトル変更の送信（`title_sender`）
- agent-status の報告と OSC 133 マークの送信（`forward_agent_status_items`）
- OSC 7 による cwd の記録
- osc-probe のログ出力

#### FR9: 抑止したチャンクの端末問い合わせ

デーモンは端末問い合わせ（DSR・DA など、端末が PTY へ応答を返すシーケンス）に応答しない。応答するのは、生の出力を受け取ったクライアントの `term_core` だけである。snapshot のバイト列からは端末問い合わせが除去される。そのため、抑止したチャンクに含まれる問い合わせは、何もしなければ誰も応答せず、プログラムが応答を待ち続けうる。

これを防ぐため、抑止したチャンクに含まれる端末問い合わせのうち snapshot のバイト列に残らないものは、snapshot を受け取った送信先へ、snapshot の後、境界より後のチャンクより前に、元の順序で 1 回だけ届ける。クライアントはそれぞれに 1 回だけ応答する。snapshot のバイト列に残る問い合わせは、もう一度は届けない。

#### FR10: 境界をまたぐシーケンスの続きを文字として表示しない

- 抑止したチャンクの末尾で未完了の制御シーケンスや UTF-8 文字があるとき、境界後に転送するチャンクの先頭にある続きのバイトが、クライアントで文字や置換文字として表示されない。そのシーケンスはクライアントで 1 回だけ処理される。
- `ScrollbackWriteFilter` が pending に保留したバイトもこれに含む。pending のバイトはリングに入っていないので、snapshot にも含まれない。
- 境界をまたいで完結するリッチコンテンツ（ビューア起動・画像）は、snapshot の再生ではないので、クライアントに 1 回届いてよい。
- 未完了部分の長さが計画で決める上限を超える場合は、続きが表示されうる。これは既知の欠落とする。

#### FR11: 抑止の境界を送信先と結び付ける

- 抑止の境界（取り込んだ最後の連番）は、snapshot を受け取る送信先（その接続の `pane_output_tx`）と組にして記録する。
- reader がチャンクを抑止するのは、転送先がその送信先と同じで、チャンクの連番が境界以下のときだけとする。別の送信先への転送は、境界で抑止しない。
- 抑止するかどうかは、判定の時点ではなく、そのチャンクと snapshot が送信先のチャネルに入る順序で整合させる。境界以下のチャンクは、`blocking_send` で待つあいだに snapshot が先にチャネルに入った場合も含めて、その送信先で snapshot の後に届かない。
- snapshot が `DeferredOutputQueue` で送信延期され、境界以下のチャンクが snapshot より前にチャネルに入る場合は、転送しても抑止してもよい（snapshot が上書きする）。
- 境界より大きいチャンクは、その送信先で欠けない。

### 4.3 エッジケース

- reader は pane ごとに逐次動くので、境界をまたいで in-flight になるチャンクは 1 つまで。抑止の影響も snapshot 1 回につきチャンク 1 つまで。
- vt100 の panic で shadow parser を作り直したチャンクにも、連番を割り当てる。
- alt-screen 中のチャンクや alt 切替を含むチャンクは、リングに書かない（または一部だけ書く）が、連番は進める。
- `ScrollbackWriteFilter` が次の読み取りまで保留した末尾バイト（pending、リッチコンテンツ候補）は、連番 N のチャンクに属していても、リングには N+1 以降と一緒に書かれる。そのため snapshot には含まれない。N を抑止した場合、この保留分は FR10 に従って扱い、N+1 の先頭にある続きが文字として漏れないようにする。上限を超えて生のままリングへ吐き出す場合も含めて FR10 の上限の扱いに従う。
- オンデマンド snapshot を短い間隔で続けて取る場合や、`DeferredOutputQueue` での同じ pane の合流（新しいほうが勝つ）では、同じ送信先について記録する最後の連番は単調に増やす（後退させない）。
- `DeferredOutputQueue` が、別 pane の上限超えで古いオンデマンド snapshot を捨てた場合、その snapshot のために抑止したチャンク 1 つは、クライアントが snapshot を要求し直すまで表示に出ない（既存の回復経路は再要求）。
- `DeferredOutputQueue` に積まれた `VisibilityResume` は、送信時に `resume_pane_with_permit` を通るので、境界はその時点で記録される。
- 別クライアントからの reattach による乗っ取りでは、境界は新しい送信先と組で記録される（FR11）。古いクライアントには kick が送られる。古い送信先へ送信待ちだったチャンクは、新しい送信先の境界では抑止しない。
- 送信待ちの Full 経路で送信先が閉じた場合の Detached への切替（`NetworkDetach`）は、従来どおり動く。
- 抑止したチャンクを `capture_passthrough` に通すと、そこから抜き出した画像・Markdown のシーケンスが `raw_passthrough` に入る。これは次の復帰で読み捨てられ、再生されない（既存の方針どおり）。
- DSR 6n（カーソル位置の問い合わせ）を snapshot の後に届けた場合、クライアントが返す位置は、snapshot を適用した後のカーソル位置になる。問い合わせはチャンク末尾に置かれることが多く、修正前もチャンクを二重に適用した途中の位置で応答していたので、この差は許容する。
- 端末問い合わせが抑止したチャンクの末尾で途中までしか無い場合は、FR10 の未完了のシーケンスとして扱い、続きのチャンクとつながって 1 回だけ応答される。
- Detached の期間の最後に `passthrough_scanner` に途中状態が残ったまま Connected に戻る場合の途中状態の扱い（抑止とは関係の無い既存の経路）は、このタスクで変えなくてよい。FR6 が求めるのは、抑止したチャンクで残った途中状態についてだけ。

## 5. 非機能要件

### 5.1 非機能要件一覧

| ID | 項目 | 状態 |
|----|------|------|
| NFR1 | snapshot のバイト列とワイヤ形式を変えない | resolved |
| NFR2 | ロック規律とデッドロック回避 | resolved |
| NFR3 | ロック内処理を増やさない | resolved |
| NFR4 | reader の通常経路のコスト | resolved |
| NFR5 | ビルド構成とプラットフォーム | resolved |

### 5.2 非機能要件詳細

#### NFR1: snapshot のバイト列とワイヤ形式を変えない

snapshot のバイト列の形を変えない（apt 進捗バー残骸の既知課題の制約）。`mux_ipc` のワイヤ形式と、`SnapshotRestore` / `Snapshot` のフレームの形も変えない。FR9・FR10 で snapshot の後に届けるバイトは、既存の `PtyOutput` チャンクとして送る。

#### NFR2: ロック規律とデッドロック回避

- `output_target` ロックを保持したまま `blocking_send` しない（G1 の規律、`pty_spawn/mod.rs` 493-495 行と 548-567 行、EOF 分岐の回帰テスト）。
- 捕捉の排他（FR2）は、reader・snapshot の 4 経路・`MuxPane::resize` のすべてで、リングと shadow parser のロックより先に取る。
- reader は捕捉の排他を解放してから `output_target` を取る。
- `output_target` を持ったまま捕捉の排他を取る経路（`resume_pane_with_permit`、`evaluate_output_target`、境界を読む reader の転送判定）は、順序を `output_target` → 捕捉の排他 → リング・shadow parser に統一する。
- 捕捉の排他を持ったまま `output_target` を取る経路は作らない。
- 現状のコードには、リングと shadow parser を逆順に同時に持つ経路は無い（reader はそれぞれを順に取って放す。resize はリングを放してから shadow parser を取る。snapshot の各経路も順に読む）。
- resize がリングのロックを `master.resize()` をまたいで持つ点（`pane/tests.rs` 1270-1324 行）も、この順序の監査に含める。

#### NFR3: ロック内処理を増やさない

- tokio ワーカー上で `output_target` ロックの中で行う処理を増やさない。
- 特に、今ロックの外で行っているオンデマンド snapshot の組み立てとプローブ再生を、`output_target` ロックの中へ移さない（別タスク「リング一周後の snapshot プローブ再生がロック内で走る」と衝突させない）。
- snapshot 側が捕捉の排他を持つのは、リング・shadow parser の状態・最後の連番を読み取るあいだだけとする。snapshot の組み立て・エンコード・プローブ再生・送信は、その排他の外で行う。

#### NFR4: reader の通常経路のコスト

reader の通常経路（抑止しないチャンク）に足すコストは、連番の加算と比較、競合しない排他の取得程度に留める（低遅延の維持）。FR9・FR10 の問い合わせの抜き出しと未完了部分の扱いは、抑止したチャンクだけで行う。

#### NFR5: ビルド構成とプラットフォーム

`--no-default-features`（CLI と mux のみ）のビルドが通る。Linux と Windows で同じように動く（プラットフォーム固有の API を使わない）。

### 5.3 セキュリティ要件

- `handle_request_pane_snapshot` にある、接続中のセッションに属する pane だけに snapshot を返す認可の検査は、変えない。
- FR9・FR10 で snapshot の後に届けるバイトは、その snapshot を受け取った送信先にだけ送る。

## 6. UI/UX要件

該当なし（mux デーモン内部の Rust だけの変更で、UI・画面・ビジュアルの変更が無い）。

## 7. データ要件

### 7.1 デーモン内部の状態

| 項目 | 説明 |
|------|------|
| 出力連番 | pane ごとに、読み取ったチャンク 1 つにつき単調増加で 1 つ割り当てる（FR1）。 |
| 抑止の境界 | snapshot が取り込んだ最後の連番。snapshot を受け取る送信先（`pane_output_tx`）と組にして記録する（FR11）。 |

### 7.2 データ保持期間

| データ種別 | 保持期間 |
|------------|----------|
| 出力連番・抑止の境界 | デーモンの 1 回の起動のあいだだけメモリ上に持つ（ASM-3）。 |

## 8. 外部連携

該当なし。`mux_ipc` のワイヤ形式は変えない（NFR1）。

## 9. 制約条件

### 9.1 技術的制約

- snapshot のバイト列の形を変えない（NFR1、ASM-4）。
- `mux_ipc` のワイヤ形式と、`SnapshotRestore` / `Snapshot` のフレームの形を変えない（NFR1）。
- `output_target` ロックを保持したまま `blocking_send` しない（NFR2）。
- `--no-default-features` のビルドが通り、Linux と Windows で同じように動く（NFR5）。

### 9.2 ビジネス上の制約

該当なし。

### 9.3 スケジュール制約

該当なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/mux-snapshot-output-boundary/**`
- `test-docs/mux-snapshot-output-boundary/**`

`feature-docs/mux-snapshot-output-boundary/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/mux-snapshot-output-boundary/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/mux-snapshot-output-boundary/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/mux-snapshot-output-boundary/` ディレクトリを生成しないが、宣言された `test-docs/mux-snapshot-output-boundary/**` は依然として正しい。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| 抑止した 1 チャンクの中だけで完結するビューア起動・画像のシーケンスは、クライアントに届かない（FR6） | — | 仕様上の既知の欠落とする。 |
| 境界をまたぐ未完了部分の長さが上限を超える場合は、続きが表示されうる（FR10） | — | 既知の欠落とする。上限は計画で決める。 |
| 通知チャネルへの `try_send` が失敗したときの既存の欠落（警告ログを出して捨てる）（ASM-8） | — | そのまま残す。 |
| `DeferredOutputQueue` が古いオンデマンド snapshot を捨てた場合、抑止したチャンク 1 つは再要求まで表示に出ない（4.3） | — | 既存の回復経路（再要求）による。 |

### 10.2 ビジネスリスク

該当なし。

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC-1（FR1・FR2・FR3）: 並行処理テストがある。reader を shadow・リングの更新と転送のあいだで止めたまま snapshot を取り、そのあと reader を再開する。クライアント側で「snapshot の後に届くチャンク」を適用した結果が、生のストリームを 1 回だけ適用した結果と一致する（例: `ESC[L` を含むチャンクが 1 回だけ適用される）。visible reattach、可視化復帰（`resume_pane_with_permit`）、オンデマンド snapshot の 3 経路それぞれについて確認する。修正前のコードではこのテストが失敗する。
- [ ] AC-2（FR2）: snapshot が取得したリングと shadow parser は、同じ集合のチャンクを反映している。reader が shadow を更新してからリングに書くまでのあいだに snapshot を取ろうとしても、shadow にだけ含まれるチャンクが生じない。resize と snapshot が並行しても、取得したリングの寸法マーカーと shadow parser の寸法が食い違わない。
- [ ] AC-3（FR4・FR11）: snapshot の取得時にチャネル満杯で送信待ちだったチャンクが、snapshot より後にクライアントへ届かない。
- [ ] AC-4（FR5）: 境界より後のチャンクと EOF の空チャンクは、欠けずに転送される。サイズ上限で snapshot を送らなかった場合は、チャンクが抑止されない。
- [ ] AC-5（FR6）: 抑止したチャンクに含まれる OSC 9 について、Detached と同じ通知処理が 1 回実行され、通知チャネルに 1 件届く（`try_send` が成功する条件で）。そのチャンクはクライアントに転送されないので、GUI 側では発火しない。抑止したチャンクと次のチャンクにまたがる OSC 9 は二重に発火せず、後の Detached 期間の出力とつながった通知も生じない。
- [ ] AC-6（NFR1）: snapshot のバイト列の形を固定している既存テスト（`reattach/tests.rs`、`pane/tests.rs`、`pty_spawn/tests.rs` の apt / リング一周系）が、変更なしで通る。
- [ ] AC-7（NFR2・NFR5）: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` と、`CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が通る。
- [ ] AC-8（FR7）: FR7 の対象コメントが、新しい保証と矛盾しない。
- [ ] AC-9（FR8）: 抑止したチャンクに含まれるタイトル変更（OSC 0/2）、agent-status の報告、OSC 133 マーク、OSC 7 の cwd が、抑止しない場合と同じく反映・送信される。
- [ ] AC-10（FR9）: 抑止したチャンクに含まれる端末問い合わせ（例: `ESC[6n`、`ESC[c`）が、snapshot の後、境界後のチャンクより前に、送信先へちょうど 1 回届く。snapshot のバイト列に残る問い合わせは、もう一度は届かない。
- [ ] AC-11（FR10）: 抑止したチャンクの末尾で未完了の CSI・OSC・UTF-8 文字、および `ScrollbackWriteFilter` が pending に保留したリッチコンテンツ候補について、snapshot とその後に届くバイトを `term_core` に流した結果が、生のストリームを 1 回だけ流した参照と一致し、続きのバイトや置換文字が表示されない。
- [ ] AC-12（FR11）: 送信先 A 向けに記録した境界が、送信先 B へのチャンクを抑止しない。境界より大きいチャンクは、どの送信先でも欠けない。
- [ ] AC-13（NFR2）: reader・resize・snapshot の 4 経路を並行して繰り返し走らせるテストが、デッドロックせず時間内に終わる。

### 11.2 KPI

該当なし。

## 12. テストシナリオ

### 12.1 テスト観点

| ID | シナリオ | 受け入れ基準 | 要件 |
|----|----------|--------------|------|
| TS-1 | visible reattach のあいだの二重適用 | AC-1 | FR1, FR2, FR3 |
| TS-2 | 可視化復帰のあいだの二重適用 | AC-1 | FR1, FR2, FR3 |
| TS-3 | オンデマンド snapshot のあいだの二重適用 | AC-1 | FR1, FR2, FR3 |
| TS-4 | shadow とリングの食い違い | AC-2 | FR2 |
| TS-5 | バックプレッシャ経路 | AC-3 | FR4, FR11 |
| TS-6 | 過剰抑止が無いこと | AC-4 | FR5 |
| TS-7 | 抑止したチャンクの OSC 9 | AC-5 | FR6 |
| TS-8 | 転送以外の処理 | AC-9 | FR8 |
| TS-9 | 抑止したチャンクの端末問い合わせ | AC-10 | FR9 |
| TS-10 | 境界をまたぐシーケンス | AC-11 | FR10 |
| TS-11 | 送信先との結び付け | AC-12 | FR11 |
| TS-12 | ロック順序 | AC-13 | NFR2 |

各シナリオの手順は SPEC.md の「テストシナリオ」に記載する。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| 出力連番 | PTY reader が pane ごとに、読み取ったチャンク 1 つにつき 1 つ割り当てる単調増加の番号。デーモン内部の状態（FR1）。 |
| 捕捉の排他 | チャンクの shadow parser 更新・リング書き込み・連番の確定と、snapshot によるリング・shadow parser・最後の連番の読み取りを区切る、pane ごとの 1 つの排他（FR2）。 |
| 抑止の境界 | snapshot が取り込んだ最後の連番。snapshot を受け取る送信先と組にして記録する（FR11）。 |
| 現象 a | snapshot に取り込まれたチャンクが、snapshot の後にもう一度転送されること（FR3）。 |
| 現象 b | shadow ダンプには含まれるが、リングには含まれないチャンクが生じること（FR2）。 |
| 端末問い合わせ | DSR・DA など、端末が PTY へ応答を返すシーケンス（FR9）。 |

## 14. 確認事項

### 14.1 前提事項

- [x] ASM-1: 完了の定義が挙げる 2 つの方式のうち、pane ごとの出力連番の方式を採る。更新から転送までを 1 つのクリティカルセクションに収める方式は採らない。理由は 3 つ。(1) 転送の一部である `blocking_send` は、デッドロックを避けるため `output_target` ロックの外で行う必要がある（`pty_spawn/mod.rs` 493-495 行、548-567 行、G1）。そのため 1 つのクリティカルセクションではバックプレッシャ経路を覆えない。(2) オンデマンド経路（`handlers/mod.rs` 459 行以降）は `output_target` を取らずに組み立てている。クリティカルセクション方式にすると、組み立てを tokio ワーカー上のロック内へ移すことになる（NFR3 に反する）。(3) 連番はデーモン内部だけに持てるので、snapshot のバイト列の制約（NFR1）と両立する。
- [x] ASM-2: AC-1 と AC-3 の並行処理テストを決定的にするため、reader の更新と転送のあいだにテスト専用の停止点（`cfg(test)`）を設けてよい。reader は `pty_reader_loop` に `Box<dyn Read>` を渡すかたちで既存テストから駆動できる（`pty_spawn/tests.rs` 2302 行ほか）。
- [x] ASM-3: 出力連番と「snapshot が取り込んだ最後の連番」は、デーモンの 1 回の起動のあいだだけメモリ上に持つ。hot-upgrade の引き継ぎ形式には加えない。復元した pane（`mux::upgrade` が `pty_reader_loop` を呼び直す経路）では連番を初期値から数え直す。
- [x] ASM-4: snapshot のバイト列の形を変えない（apt 進捗バーの既知課題）。これはタスクが課す守るべき制約。
- [x] ASM-5: チャンクが表示から一時的に消える向きの既知の隙間は、このタスクの対象外とする。このタスクが扱うのは二重適用だけ。対象外の隙間は 2 つある。(1) 境界より後に作られたチャンクが、後回しにされたオンデマンド snapshot より先に届き、snapshot に上書きされる（`DeferredOutputQueue` のコメント 248-262 行）。(2) `collect_reattach_data` が snapshot を読んでから送信先を Connected に切り替えるまで（`reattach.rs` 216-237 行）に reader が処理したチャンクは、Detached として捕捉されるだけで、snapshot にも転送にも入らない。
- [x] ASM-6: リング一周後のプローブ再生を `output_target` ロックの外へ移す件は、別タスクのスコープとする。
- [x] ASM-7: デーモンの mux 実装は端末問い合わせに応答しない（mux 配下に応答処理が無い）。応答は、生の出力を受け取ったクライアント側の `term_core` が返す。snapshot のバイト列から端末問い合わせ（DSR・DA・XTWINOPS・DECRPM）を除去するのは、`strip_replayable_rich_content` の既存の挙動である。FR9 はこの事実を前提にする。
- [x] ASM-8: FR6 の OSC 9 は「Detached と同じ通知処理を 1 回実行する」ことまでを保証する。通知チャネルへの `try_send` が失敗したときの既存の欠落（警告ログを出して捨てる）は、そのまま残す。
- [x] ASM-9: FR6 の方式として、抑止したチャンクを Detached 腕と同じ `capture_passthrough` に通す案（案 A）を採る。抑止したチャンクを snapshot の後に再生する案（案 B）は採らない。案 B は、`PassthroughScanner` が OSC 777 を抜き出さないこと、OSC 9999・DCS の検出が strip の処理と一致しないこと、復元後の再生でカーソルと描画の順序が崩れることから退けた。
- [x] ASM-10: FR10 では、境界をまたぐシーケンスを両側とも捨てるのではなく、クライアントで 1 回だけ処理させる。これは修正前（抑止したチャンクがもう一度転送され、続きのチャンクとつながっていた）と同じ結果になる。

### 14.2 未確認・保留事項

- [ ] OSC 10/11/12/4 の色問い合わせを、snapshot のバイト列から除去する端末問い合わせの対象に含むか（FR9、ASM-7）。計画で確認する。
- [ ] FR10 の実現方法。snapshot のバイト列の末尾（alt 状態の付与やダンプの追加）とクライアントの snapshot 再生の後に parser の途中状態が残るかどうかによって変わる。未完了部分の長さの上限も計画で決める。
- [ ] FR4・FR11 の順序の整合の方式。reader は `try_send` が Full を返してから `blocking_send` の待ち行列に入るまでのあいだにロックを放すので、そのあいだに snapshot が先にチャネルに入りうる。方式は計画で決める。
- [ ] ASM-3 の根拠は `pty_reader_loop` の doc コメントだけ（`mux::upgrade` は未確認）。

## 15. 参考資料

- `feature-docs/mux-snapshot-output-boundary/SPEC.md`
