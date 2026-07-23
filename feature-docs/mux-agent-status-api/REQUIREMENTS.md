---
title: "muxエージェント状態表示とエージェント向けAPI"
created_date: 2026-07-23
status: draft
---

# muxエージェント状態表示とエージェント向けAPI - 要件定義書

## 1. 概要

### 1.1 背景

複数の AI エージェント（Claude Code 等）を複数の pane/tab で並行運用すると、どのエージェントが承認待ちで止まっているか・作業完了したかを全 pane を巡回して確認する必要がある。

### 1.2 目的

- 各 pane で動くエージェントの状態（idle / working / blocked / done）を能動報告で受け取り、タブ/ウィンドウ一覧とステータスバーで俯瞰できるようにする
- blocked / done への遷移を OS 通知で知らせる
- エージェント自身が他 pane を読み・入力を送り・状態を待てる API（read / send / wait）を提供する

### 1.3 スコープ

- 対象: 状態検出（OSC 能動報告）+ 可視化（バッジ・ステータスバー）+ 通知、およびエージェント向け API（read / send / wait）
- 対象外: エージェントのセッションレジューム、pane 内容の再起動後復元、git worktree 統合、split 等のレイアウト操作 API、画面パターンマッチによる状態検出、Claude Code hooks との公式統合

## 2. ビジネス要件

### 2.1 ビジネス目標

AI エージェントの並行運用時に「今どのエージェントに人間の対応が必要か」を巡回なしで把握できるターミナルにする。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| エージェント並行運用ユーザー | 複数の pane/tab で AI コーディングエージェントを同時に動かす開発者 |
| エージェントプロセス | 他 pane の出力読み取り・入力送出・状態待ちを行う AI エージェント自身 |

### 2.3 期待される効果

- 承認待ち（blocked）のエージェントを即座に発見でき、待ち時間が減る
- 非アクティブな pane の完了・承認待ちを通知で受け取れる
- エージェント間の協調（完了待ち・出力参照・指示送出）をスクリプト化できる

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | エージェントが状態を報告する | エージェント/ユーザーのシェル | 高 |
| UC02 | 状態をタブ一覧・ステータスバーで俯瞰する | ユーザー | 高 |
| UC03 | blocked/done を OS 通知で受け取る | ユーザー | 高 |
| UC04 | 他 pane の出力を読む | エージェント | 中 |
| UC05 | 他 pane に入力を送る | エージェント | 中 |
| UC06 | 他 pane の状態変化を待つ | エージェント | 中 |

### 3.2 ユースケース詳細

#### UC01: エージェントが状態を報告する

**アクター**: エージェント（またはそのラッパースクリプト）

**事前条件**:
- eMterm の pane 内（直接またはSSH/tmux 越し）でプロセスが動作している

**基本フロー**:
1. プロセスが `emterm agent-status working` 等を実行する
2. CLI が状態報告 OSC シーケンスを stdout に出力する（tmux 内では DCS passthrough でラップ）
3. eMterm（plain tab では GUI、mux pane では daemon）が OSC を解釈し、その pane の状態を更新する

**代替フロー**:
- 不正な state 値や不正な符号化のシーケンスは全体を無視する
- `emterm agent-status clear` で状態なしに戻す

**事後条件**:
- pane の状態が更新され、revision が加算される

#### UC02: 状態をタブ一覧・ステータスバーで俯瞰する

**アクター**: ユーザー

**基本フロー**:
1. タブ/ウィンドウ一覧の各項目に、含まれる pane の状態を集約したバッジが表示される
2. ステータスバーに状態別の件数サマリが表示される
3. ユーザーが該当タブを前面表示すると、そのタブの done/blocked の未読強調が解除される（意味状態は変わらない）

#### UC03: blocked/done を OS 通知で受け取る

**アクター**: ユーザー

**基本フロー**:
1. 前面ウィンドウに表示されていない pane の状態が blocked または done に実遷移する
2. eMterm が OS 通知を発火する（エージェント名を含む場合は制御文字除去済みの表示名）

**代替フロー**:
- snapshot 受信・再接続・リプレイ由来の状態設定では通知しない
- 同一状態の再報告では通知しない
- pane 単位のレート制限を超えた通知は抑制する
- 設定でエージェント通知が無効、またはグローバル通知が無効の場合は通知しない

#### UC04: 他 pane の出力を読む

**アクター**: エージェント

**基本フロー**:
1. `emterm mux read --pane <id> --lines N` を実行する
2. daemon が対象 pane の描画行末尾 N 行を ANSI 除去済み UTF-8 テキストで返す

**代替フロー**:
- 対象 pane が存在しない場合はエラー
- plain tab（mux 外）を指す場合は not_mux_pane エラー
- N・応答バイト数は上限で切り詰める

#### UC05: 他 pane に入力を送る

**アクター**: エージェント

**基本フロー**:
1. `emterm mux send --pane <id> --text "..."`（または `--stdin`）を実行する
2. daemon が対象 pane の PTY へ UTF-8 文字列をそのまま不可分に書き込む（Enter の暗黙追加なし）
3. 応答に書込み成功直前の revision（watermark）を返す

**代替フロー**:
- NUL を含む入力・サイズ上限超過は拒否する

#### UC06: 他 pane の状態変化を待つ

**アクター**: エージェント

**基本フロー**:
1. `emterm mux wait --pane <id> --state done,blocked [--timeout 秒] [--after <revision>]` を実行する
2. 現在状態が指定集合に含まれ、かつ revision が `--after` より大きければ即時成功（level-triggered）
3. そうでなければ状態変化まで（または timeout まで）ブロックする

**代替フロー**:
- timeout 到達時は専用 exit code で終了する
- 対象 pane が破棄されたらエラーで終了する
- クライアント切断時に daemon 側の waiter を破棄する

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| F01 | 状態報告 OSC シーケンス | `OSC 777;emterm;agent-status;…` の set / clear を解釈する | 高 |
| F02 | agent-status CLI | 状態報告 OSC を stdout に出す `emterm agent-status` サブコマンド | 高 |
| F03 | daemon 状態保持 | pane ごとに state / name / revision を保持し、ライフサイクルで破棄する | 高 |
| F04 | リプレイ除去と状態同期 | 状態 OSC を scrollback/snapshot リプレイから除去し、snapshot 後に状態を別途同期する | 高 |
| F05 | AgentStatusUpdate 配信 | daemon → GUI の専用 IPC メッセージで状態を配信する | 高 |
| F06 | GUI 状態モデル | plain tab は GUI 所有、mux pane は daemon 所有の状態を共通モデルに正規化する | 高 |
| F07 | タブ/ウィンドウ状態バッジ | 一覧項目に優先順で集約したバッジと未読強調を表示する | 高 |
| F08 | ステータスバー集約サマリ | 状態別件数をステータスバーに表示する | 高 |
| F09 | OS 通知 | blocked/done 実遷移時に非表示 pane に限り通知する | 高 |
| F10 | mux read API | 対象 pane の描画行末尾 N 行をテキストで返す | 中 |
| F11 | mux send API | 対象 pane の PTY へ入力を書き込み revision watermark を返す | 中 |
| F12 | mux wait API | 状態集合と revision 条件での level-triggered 待機 | 中 |
| F13 | pane ID 体系 | daemon incarnation を含む非再利用 opaque ID・`EMTERM_PANE_ID` 注入・GUI からの ID コピー | 中 |

### 4.2 機能詳細

#### F01: 状態報告 OSC シーケンス

**説明**: 既存の `OSC 777;emterm;<kind>;…` 名前空間に `agent-status` を追加する。

**入力（wire フォーマット）**:
- 設定: `OSC 777;emterm;agent-status;v=1;state=<idle|working|blocked|done>[;name=<表示名>]`
- 解除: `OSC 777;emterm;agent-status;clear`

**ビジネスルール**:
- `name` は percent-encoded UTF-8。復号・正規化後 80 文字上限（超過分は切り詰め）
- 未知キーは無視する。state の欠落・不正値・キー重複はシーケンス全体を無視する
- シーケンスは出力元 pane にのみ作用する（pane ID は含めない）
- 同一 pane への複数報告元は「最後に受信した報告を採用」とする
- clear・同一状態の再報告でも revision は加算する

**エラーケース**:
| エラー | 条件 | 対応 |
|--------|------|------|
| 不正シーケンス | state 不正・キー重複・復号失敗 | シーケンス全体を無視（状態・revision とも変更なし） |

#### F02: agent-status CLI

**説明**: `emterm agent-status <idle|working|blocked|done> [--name <名前>]` と `emterm agent-status clear`。既存 CLI サブコマンドと同様にステートレスで、OSC を stdout に出力し、tmux 内では DCS passthrough でラップする。

#### F03: daemon 状態保持

**説明**: mux daemon は pane ごとに `state`（4値または状態なし）、正規化済み `name`、単調増加 `revision` を保持する。

**ビジネスルール**:
- 受理した報告（set / clear / 同一状態の再報告）ごとに revision を加算する
- PtyExited・pane 破棄で状態を破棄する
- 状態は永続化しない（daemon 再起動で消える）

#### F04: リプレイ除去と状態同期

**説明**: agent-status OSC は scrollback / snapshot リプレイから除去する（既存の除去機構を拡張）。snapshot 適用後の最新状態は F05 のメッセージで別途同期する。

**ビジネスルール**:
- Snapshot payload の既存フォーマットは変更しない
- snapshot・再接続由来の状態設定では通知を発火しない

#### F05: AgentStatusUpdate 配信

**説明**: mux_ipc に daemon → GUI の専用メッセージを追加し、`pane ID / state / name / revision / リプレイ由来フラグ` を配信する。既存 StatusUpdate メッセージは変更しない。

#### F06: GUI 状態モデル

**説明**: GUI は共通の AgentStatus モデルを持つ。plain tab では GUI 自身が OSC を解釈して状態を所有し、mux pane では daemon 配信を受信する。タブを閉じたら状態は破棄する。

#### F07: タブ/ウィンドウ状態バッジ

**説明**: タブ/ウィンドウ一覧の各項目に、配下 pane の状態を集約した単一バッジを表示する。

**ビジネスルール**:
- 集約優先順: blocked > 未読 done > working > 既読 done > idle
- 未読/既読（seen/unseen）は GUI クライアント側のみで管理し、意味状態とは分離する
- 「既読」は pane を含むタブが前面 OS ウィンドウで表示されたときに付く
- 既読化してもバッジの強調・未読印のみ解除し、意味状態は変更しない

#### F08: ステータスバー集約サマリ

**説明**: ステータスバーに状態別の pane 件数（blocked / working / done / idle）を表示する。件数は既読に関係なく意味状態で数える。状態報告のある pane が 0 のときは表示しない。

#### F09: OS 通知

**説明**: blocked / done への実遷移時に OS 通知を発火する。

**ビジネスルール**:
- 通知対象は「前面ウィンドウに表示されていない pane」の実遷移のみ
- 同一状態の再報告・name のみの変更・snapshot/リプレイ由来では通知しない
- pane 単位の短いレート制限を設ける
- エージェント通知設定（デフォルト ON）とグローバル通知設定の両方が有効な場合のみ通知する
- 通知本文に載せる name は制御文字除去済みの表示名を使う

#### F10: mux read API

**説明**: `emterm mux read --pane <id|current> [--lines N]`。daemon が対象 pane の現在画面 + scrollback 末尾から描画行 N 行を ANSI 除去済み UTF-8 プレーンテキストで返す。

**ビジネスルール**:
- N と応答バイト数に上限を設ける
- plain tab を指す場合は not_mux_pane エラー

#### F11: mux send API

**説明**: `emterm mux send --pane <id|current> (--text <文字列> | --stdin)`。UTF-8 文字列をそのまま対象 pane の PTY へ書き込む。

**ビジネスルール**:
- Enter の暗黙追加なし・キー表現としての解釈なし
- NUL 禁止・サイズ上限・1 要求単位の不可分書き込み
- 応答に書込み成功直前の revision watermark を返す

#### F12: mux wait API

**説明**: `emterm mux wait --pane <id|current> --state <集合> [--timeout 秒] [--after <revision>]`。

**ビジネスルール**:
- level-triggered: 現在状態が集合に含まれ revision > `--after`（指定時）なら即時成功
- 状態なしの pane は状態が付くまで待機する
- pane 破棄はエラー終了、timeout は専用 exit code
- クライアント切断時は daemon 側の waiter を破棄する

#### F13: pane ID 体系

**説明**: API のターゲット指定に使う公開 pane ID。

**ビジネスルール**:
- daemon incarnation を含む非再利用の opaque 文字列（daemon 再起動をまたいで衝突しない）
- window/tab 位置や name を ID に埋め込まない
- mux pane 生成時に環境変数 `EMTERM_PANE_ID` を注入し、CLI の `--pane current` で解決する
- GUI に pane ID をコピーする導線を設ける

## 5. 非機能要件

### 5.1 パフォーマンス要件

- 状態更新はイベント駆動とし、ポーリングや描画フレームごとの追加コストを持ち込まない
- read API の応答サイズは上限で抑える

### 5.2 セキュリティ要件

- 入力検証: OSC パラメータ・API 入力の検証（F01/F10-F12 のルール）
- データ保護: name は表示・通知前に制御文字を除去する
- 信頼境界: 任意の PTY 出力・SSH 先が状態 OSC を偽装できることをドキュメントに明記する。偽装の影響は表示・通知に限定し、API 権限・pane 識別には状態を使わない
- API 権限: mux ソケットは同一ユーザー限定を維持し、read/send が端末操作と同等の強い権限であることを明記する

### 5.3 可用性要件

- daemon 再起動後は状態なしから再開する（状態の永続化はしない）

### 5.4 保守性要件

- ログ出力: 状態遷移・API 要求の診断ログは既存 logging 機構に従う（リリースで残るのは warn 以上）
- ドキュメント: 状態報告 OSC / CLI / API の利用方法と信頼境界の説明を含める

### 5.5 互換性要件

- 既存 mux IPC との互換: 新メッセージ追加に伴う PROTOCOL_VERSION の扱いを定義する（バージョン上げ、または後方互換な追加）
- Snapshot payload の既存フォーマットは変更しない
- CLI-only ビルド（--no-default-features）では `emterm agent-status` を利用可能とする。`emterm mux read/send/wait` は GUI ビルドのバイナリで提供する
- tmux 内からの状態報告は既存の DCS passthrough 機構で透過する

## 6. UI/UX要件

### 6.1 画面設計要件

- タブ/ウィンドウ一覧の項目に状態バッジを表示する（集約優先順・未読強調あり）
- ステータスバーに状態別件数サマリを表示する
- GUI に pane ID のコピー導線を設ける
- 配色・形状は MD3 デザイントークン（doc/UI-DESIGN-GUIDELINES.yaml）に整合させる

### 6.2 画面遷移

該当なし（既存画面への表示追加のみ）。

### 6.3 レスポンシブ対応

該当なし。

## 7. データ要件

### 7.1 データモデル概要

pane ごとの AgentStatus: `state`（idle/working/blocked/done/なし）、`name`（正規化済み表示名）、`revision`（単調増加）。GUI 側のみ `seen` フラグを持つ。

### 7.2 データ項目

| エンティティ | 項目名 | 型 | 必須 | 説明 |
|--------------|--------|-----|------|------|
| AgentStatus | state | enum 4値 | × | 状態なしを既定とする |
| AgentStatus | name | String | × | 復号・正規化後 80 文字上限 |
| AgentStatus | revision | u64 | ○ | 受理した報告ごとに加算 |
| AgentStatus (GUI) | seen | bool | ○ | GUI クライアント側のみ |

### 7.3 データ保持期間

| データ種別 | 保持期間 |
|------------|----------|
| AgentStatus | pane 生存中のみ（永続化しない） |

## 8. 外部連携

### 8.1 連携システム

| システム名 | 連携方法 | データ |
|------------|----------|--------|
| エージェント/シェル | `emterm agent-status` CLI（OSC 出力） | 状態報告 |
| エージェント | `emterm mux read/send/wait` CLI（mux ソケット） | pane テキスト・入力・状態待ち |

### 8.2 API仕様要件

第4章 F10-F13 のとおり。

## 9. 制約条件

### 9.1 技術的制約

- 状態検出は OSC 能動報告のみ（画面パターンマッチは実装しない）
- 既存 StatusUpdate メッセージ・Snapshot payload フォーマットは変更しない
- Linux / Windows 両対応（macOS 非対応）

### 9.2 ビジネス上の制約

- なし

### 9.3 スケジュール制約

- なし

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| send 直後の wait が過去状態で即時成功する | 高 | revision watermark と `--after` で待機基準を線形化する |
| daemon 再起動後の pane ID 再利用による誤操作 | 高 | ID に daemon incarnation を含める |
| 状態 OSC が snapshot リプレイで再生され通知が誤発火する | 中 | リプレイ除去 + リプレイ由来フラグで通知抑制 |
| 旧バージョン GUI/daemon の組み合わせ | 中 | PROTOCOL_VERSION の扱いを仕様化する |

### 10.2 ビジネスリスク

| リスク | 発生確率 | 影響度 | 対応策 |
|--------|----------|--------|--------|
| なし | - | - | - |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] mux pane / plain tab の両方で状態報告がバッジ・ステータスバーに反映される
- [ ] 直接出力・tmux passthrough の両方で状態報告が届く
- [ ] 前面/背面の条件どおりに通知が発火・抑制される
- [ ] snapshot 再接続で状態が復元され、通知は発火しない
- [ ] pane 終了・clear で状態なしに戻る
- [ ] 不正 OSC が状態・revision を変更しない
- [ ] read/send/wait が仕様どおりの応答・exit code を返す（wait timeout 含む）

### 11.2 KPI

| 指標 | 目標値 | 測定方法 |
|------|--------|----------|
| なし | - | - |

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] 正常系: 状態報告 → バッジ/サマリ反映 → 既読化、read/send/wait の基本フロー
- [ ] 異常系: 不正 OSC、存在しない pane、not_mux_pane、NUL 入力、wait timeout、pane 破棄中の wait
- [ ] 境界値: name 80 文字境界、read の行数/バイト上限、revision の単調性
- [ ] セキュリティ: name の制御文字除去、状態偽装が API 権限に影響しないこと
- [ ] 互換: snapshot リプレイ除去、旧フォーマット snapshot、tmux passthrough

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| 状態（agent state） | idle / working / blocked / done の4値。blocked は承認等の人間対応待ち |
| 状態なし | 状態報告を受けていない、または clear された pane の既定状態 |
| revision | pane ごとの状態報告受理カウンタ（単調増加） |
| seen/unseen | GUI 側の未読管理フラグ。意味状態とは独立 |
| plain tab | mux を使わない通常のターミナルタブ |
| daemon incarnation | daemon プロセスの起動世代を示す識別子 |

## 14. 確認事項

### 14.1 確認済み事項

（batch モード: タスク記述の決定事項 + Codex 相談で確定）

- [x] 取り込み対象: 状態検出+可視化+通知、エージェント向け API（read/send/wait）
- [x] 見送り: エージェントレジューム、pane 内容の再起動後復元、git worktree 統合
- [x] 状態検出方式: OSC 能動報告のみ（4値: idle/working/blocked/done）
- [x] 可視化 UI: タブ/ウィンドウ一覧バッジ + ステータスバー集約サマリの両方
- [x] API スコープ: read/send/wait のみ（レイアウト操作なし）
- [x] OSC は既存 `OSC 777;emterm;<kind>` 名前空間に `agent-status` として追加
- [x] clear 操作を追加（OSC / CLI とも）
- [x] 意味状態と既読（seen/unseen）を分離し、既読は GUI 側のみで管理
- [x] pane ID は daemon incarnation 込みの非再利用 opaque ID、`EMTERM_PANE_ID` 注入、`--pane current` 対応
- [x] revision を導入し、send は watermark を返し、wait は `--after` を受ける
- [x] 専用 AgentStatusUpdate メッセージを新設し、既存 StatusUpdate / Snapshot payload は変更しない

### 14.2 未確認・保留事項

- [ ] Claude Code hooks からの状態報告導線は対象外（出力先が TTY に届く構成が必要という注記のみドキュメントに残す）

## 15. 参考資料

- SPEC.md: feature-docs/mux-agent-status-api/SPEC.md
