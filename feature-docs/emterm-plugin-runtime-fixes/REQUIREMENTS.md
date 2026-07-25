---
title: "emterm-plugin-runtime-fixes"
created_date: 2026-07-25
status: draft
---

# emterm-plugin-runtime-fixes - 要件定義書

## 1. 概要

### 1.1 背景

`emterm-claude-plugin` feature (v0.1.0) を em-workflow で実装し、7 体のレビュアー（Claude 4 観点 + Codex 3 観点）× 2 ラウンドを clean 判定で通過した。しかし実際に `/plugin install` を試した時点で 2 件の実行時バグが発覚した。

いずれも「静的にコードを読むだけでは分からない、Claude Code ランタイムの実仕様に反する」種類のバグで、既存レビューが全て静的読解だったため検出できなかった。

1. **marketplace.json の source 形式** — 修正済み（コミット 172740d）。`"source": "emterm"` + `metadata.pluginRoot` は Claude Code 2.1.219 が「未対応の source type」として拒否する。`"source": "./plugins/emterm"` に変更した。
2. **`/dev/tty` 書き込みが hook から不可能** — 未修正。本 feature の主題。

### 1.2 目的

プラグインの agent-status 連携を実際に動作する状態にする。加えて、同じ Codex レビューで検出された High / Medium の findings と、前 feature の round 2 で deferred にした findings をまとめて解消する。

### 1.3 スコープ

含める。

- `/dev/tty` → `terminalSequence` への伝送経路変更（Critical）
- hook 実装を Bun+TypeScript から POSIX sh へ変更
- `emterm` バイナリへの依存を hook から除去
- Notification hook の matcher 追加（High）
- hooks.json の exec form 化（Medium）
- display-* skill の shell injection ハードニング（Medium・前 feature で deferred）
- SPEC / README の記述更新

含めない。

- バージョン番号の変更（v0.1.0 据え置き）
- Windows 対応（v0.2.0 のまま）
- mux-agent-status-api の drain wiring 残件

### 1.4 バージョン方針

**v0.1.0 を維持する。** プラグインは未公開（GitHub にも push していない）ため、リリース済みバージョンの修正ではなく v0.1.0 の完成前修正として扱う。`marketplace.json` / `plugin.json` の `version` は変更しない。

## 2. ビジネス要件

### 2.1 ビジネス目標

プラグインの筆頭機能である agent-status 連携を実際に機能させ、公開可能な状態にする。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| Claude Code ユーザー | eMterm 上で Claude Code を使う開発者 |

### 2.3 期待される効果

- Claude Code のライフサイクルが eMterm のタブ状態に実際に反映される
- `bun` 前提条件が消え、導入のハードルが下がる
- 既知の High / Medium findings が解消され、公開に耐える品質になる

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | Claude Code 応答中に eMterm タブへ状態を反映 | Claude Code ユーザー | 高 |
| UC02 | 人間の入力待ちのときだけ blocked を表示 | Claude Code ユーザー | 高 |

### 3.2 ユースケース詳細

#### UC01: Claude Code 応答中に eMterm タブへ状態を反映

**アクター**: Claude Code ユーザー

**事前条件**:
- プラグインがインストールされている
- ユーザーが eMterm のタブで Claude Code を実行している

**基本フロー**:
1. ユーザーがプロンプトを送信すると `UserPromptSubmit` hook が発火する
2. hook スクリプトが OSC 777 の agent-status シーケンスを組み立て、`{"terminalSequence": "<seq>"}` を stdout に出力する
3. Claude Code が自前のターミナル書き込み経路でシーケンスを発行する
4. eMterm が OSC を受信し、タブのバッジを working に更新する
5. Claude Code が応答を返すと `Stop` hook が発火し、同経路で idle が送られる

**代替フロー**:
- eMterm 以外のターミナルで実行された場合、OSC 777 は解釈されず無視される（無害）
- Claude Code が v2.1.141 未満の場合、`terminalSequence` フィールドが無視され状態は反映されない

**事後条件**:
- eMterm タブのバッジが Claude Code のライフサイクルに追従する

#### UC02: 人間の入力待ちのときだけ blocked を表示

**アクター**: Claude Code ユーザー

**基本フロー**:
1. Claude がツール使用の承認を求める（`permission_prompt`）等、人間の入力を待つ通知が発生する
2. `Notification` hook が matcher に合致して発火し、blocked が送られる
3. eMterm タブが blocked 表示になる

**代替フロー**:
- `idle_prompt`（応答完了して次の入力待ち）や `auth_success` では発火しない。これらは matcher で除外する

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| F01 | terminalSequence 伝送 | hook が OSC 777 を組み立てて JSON で返す | 高 |
| F02 | POSIX sh 化 | hook 実装を Bun+TypeScript から `#!/bin/sh` へ | 高 |
| F03 | Notification matcher | 人間の入力待ちを表す notification_type にのみ絞る | 高 |
| F04 | exec form 化 | hooks.json を `args` を使う exec form に | 中 |
| F05 | display-* ハードニング | 4 つの display skill に argv ベースの文言を入れる | 中 |
| F06 | ドキュメント更新 | README の前提条件・既知制限を実態に合わせる | 中 |

### 4.2 機能詳細

#### F01/F02: terminalSequence 伝送と POSIX sh 化

**採用する伝送経路**:

Claude Code 公式ドキュメント（docs/en/hooks）に明記されている通り、v2.1.139 以降 command hook は controlling terminal を持たない session で実行され、`/dev/tty` を開けない。公式のリプレースメントが `terminalSequence` JSON 出力である。

- allowlist: OSC `0` / `1` / `2` / `9` / `99` / `777` および裸の BEL
- 終端は BEL または ST のどちらでもよい
- allowlist 外を含むとフィールドごと無視される
- universal field なので UserPromptSubmit / Stop / Notification 全てで使える
- v2.1.141 以降が必要

**hook が OSC を自前で組み立てる**:

`emterm agent-status` を spawn せず、hook スクリプトが直接 OSC 文字列を構築する。これにより以下がまとめて解消される。

- tmux 下の DCS ラップ問題（`emterm` を呼ばないので発生しない）
- 2-process chain によるプロンプト毎のレイテンシ
- SIGKILL エスカレーションの不備
- `writeSync` のブロッキング
- `bun` cold start と `bun` 不在時の exit 127

**ワイヤ形式**（`src-tauri/src/cli/agent_status.rs` のテストで確認済み）:

```
ESC ] 777 ; emterm ; agent-status ; v=1 ; state=<state> ; name=<name> ESC \
```

**トレードオフ**: ワイヤ形式が Rust（`src-tauri/src/agent_status.rs`）と hook スクリプトで二重管理になる。形式は 1 行の安定した文字列であり、変更頻度は低いと判断して受容する。

#### F03: Notification matcher

`blocked` を送る notification_type を以下に限定する。

| notification_type | 採用 | 理由 |
|---|---|---|
| `permission_prompt` | ○ | ツール使用の承認待ち。最も典型的な人間の入力待ち |
| `elicitation_dialog` | ○ | MCP サーバーが入力フォームを開いた状態。人間待ち |
| `agent_needs_input` | ○ | バックグラウンドセッションの入力待ち（v2.1.198+） |
| `idle_prompt` | × | 応答完了して次の入力待ち。`Stop` → `idle` と重複し、直後に上書きしてしまう |
| `auth_success` | × | 認証完了。人間の入力待ちではない |
| `elicitation_complete` / `elicitation_response` | × | フォーム送信済み・応答返却済み。待ち状態ではない |

#### F04: exec form 化

`hooks.json` の各 hook を `command` + `args` の exec form にする。公式ドキュメントはパスプレースホルダを含む場合 exec form を推奨しており、`${CLAUDE_PLUGIN_ROOT}` はプレースホルダとして要素ごとに置換され、シェルパーサを通らない。plugin cache のパスに空白や特殊文字が含まれても安全になる。

#### F05: display-* skill ハードニング

`display-markdown` / `display-json` / `display-yaml` / `display-image` の 4 つの SKILL.md が `<file>` をシェル文字列に直接埋める形で書かれている。前 feature の round 1 指摘を受けて `mux-send` には argv / `--stdin` ベースのハードニング文言を入れたが、display 系には同じ対応が入っていない。同じ文言体系を適用する。

## 5. 非機能要件

### 5.1 パフォーマンス要件

- hook 実行は subprocess を持たないため、`sh` の起動と 1 行の `printf` のみになる。3 秒の hook timeout に対して十分な余裕を持つ
- 内部タイムアウト機構は不要になる（待つ対象が無い）

### 5.2 セキュリティ要件

- state 引数を allow-list（`idle` / `working` / `blocked` / `done`）で検証し、それ以外は no-op
- 位置引数がちょうど 1 個であることを検証する
- 検証済みの state のみを出力文字列に埋め込む。ユーザー入力由来の値をシェル評価しない
- display-* skill にファイルパスの argv 渡しを要求する文言を入れる

### 5.5 互換性要件

- 動作保証プラットフォーム: Linux
- 必要な Claude Code バージョン: **v2.1.141 以降**（`terminalSequence` の要件）
- `bun` 前提条件は**撤廃**する
- `emterm` バイナリは agent-status hook には不要になる。display-* / mux-* skill には引き続き必要

## 9. 制約条件

### 9.1 技術的制約

- Claude Code の hook は controlling terminal を持たないため `/dev/tty` を使えない
- `terminalSequence` の allowlist は OSC `0/1/2/9/99/777` と BEL に限られる
- Claude Code plugin cache は `../` 外を参照できない

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| ワイヤ形式が Rust と hook スクリプトで二重管理になる | 中 | 形式は 1 行で安定。SPEC に正準形を明記し、Rust 側テストと hook 側テストの双方で同じ文字列を assert する |
| Claude Code v2.1.141 未満では動作しない | 低 | README に必要バージョンを明記。フィールドが無視されるだけで害はない |
| mux-agent-status-api の drain wiring 残件で state が可視化されないケースが残る | 中 | README に既知制限として継続明記。修正は別 feature |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] hook が `/dev/tty` を一切参照しない
- [ ] hook が `{"terminalSequence": "<OSC 777 sequence>"}` を stdout に出力する
- [ ] hook が POSIX sh で実装され、`bun` にも `emterm` にも依存しない
- [ ] state 引数の allow-list 検証と位置引数 1 個の検証がある
- [ ] `Notification` hook が `permission_prompt` / `elicitation_dialog` / `agent_needs_input` にのみ発火する
- [ ] `hooks.json` の 3 つの hook が全て exec form（`command` + `args`）
- [ ] display-* skill 4 本に argv ベースのハードニング文言が入っている
- [ ] README の前提条件から `bun` が消え、必要な Claude Code バージョンが明記されている
- [ ] `marketplace.json` / `plugin.json` の `version` が `0.1.0` のまま
- [ ] `bun test` と `bun run typecheck` が通る
- [ ] 実機で Claude Code のプロンプト送信 → eMterm タブが working、応答完了 → idle に変化する

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] 正常系: 各 state で正しい OSC 777 シーケンスを含む JSON が出力される
- [ ] 異常系: allow-list 外の state → 何も出力せず exit 0
- [ ] 異常系: 位置引数が 0 個または 2 個以上 → 何も出力せず exit 0
- [ ] 静的: 出力が妥当な JSON としてパースできる
- [ ] 静的: `hooks.json` の 3 つの hook が exec form かつ matcher が仕様通り
- [ ] 静的: hook スクリプトが `/dev/tty` / `emterm` / `bun` を参照しない
- [ ] 静的: hook スクリプトのワイヤ形式が Rust 側 (`agent_status.rs`) の正準形と一致する

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| terminalSequence | Claude Code の hook JSON 出力フィールド。allowlist されたエスケープシーケンスを Claude Code 自身の書き込み経路で発行させる |
| allowlist | `terminalSequence` が受け付ける OSC 番号の集合（0/1/2/9/99/777）と裸の BEL |
| exec form | hooks.json で `command` + `args` を分けて指定する形式。シェルを介さず直接 spawn される |
| agent-status | eMterm 独自の OSC 777 で、タブの状態を eMterm 本体に報告する |

## 14. 確認事項

### 14.1 確認済み事項

- [x] feature 名: `emterm-plugin-runtime-fixes`
- [x] バージョンは v0.1.0 据え置き（未公開のため完成前修正扱い）
- [x] OSC シーケンスは hook 自身が組み立てる（`emterm` CLI を spawn しない）
- [x] hook の実装言語は POSIX sh（`#!/bin/sh`）。subprocess が無くなり bun を使う理由が消えたため
- [x] `terminalSequence` が eMterm 独自 payload を通すかは **SPEC 確定前に実測して確認済み**（下記 14.3）
- [x] Notification matcher は `permission_prompt` / `elicitation_dialog` / `agent_needs_input` の 3 つ
- [x] display-* skill の shell injection ハードニングを本 feature に含める

### 14.3 POC 実測結果（2026-07-25・SPEC 確定前に実施・成立）

**動機**: 前 feature の失敗は「元の plan に『実測必須』と書いてあったのに実測せず仕様に固めた」ことが根本原因だった。同じ轍を踏まないため、設計の成否を決める前提を SPEC 確定前に実測した。

**検証対象**: `terminalSequence` の allowlist が OSC 番号だけを見るのか、payload の中身まで検証するのか。eMterm は `777;emterm;agent-status;v=1;...` という独自 payload を使うため、後者なら設計が根本から変わる。

**方法**: `.claude/settings.local.json` に一時 `UserPromptSubmit` hook を登録し、`{"terminalSequence": "<seq>"}` を stdout に返して eMterm タブのバッジを観察した。判定には `src-tauri/src/ui/tab_bar.rs:229` の `agent_badge_filled()` の性質を利用した。

- Blocked / Done → seen ならリング、unseen なら塗り
- Working / Idle → **常に塗り**

Working は絶対にリングにならないので、blocked → working の A/B でバッジ形状が変われば伝送が成立していると確定できる。

| ラウンド | 送った state | 観察されたバッジ |
| --- | --- | --- |
| 1 | `blocked` | リング |
| 2 | `working` | 塗りつぶし |

**結論**: バッジ形状が state に追従して変化した。以下 3 点が同時に確定した。

1. `terminalSequence` の allowlist は **OSC 番号だけ**を検査しており、`777` の payload が `notify;title;body` 形式かどうかは見ていない。eMterm 独自 payload はそのまま通る
2. Claude Code が自前のターミナル書き込み経路で実際に発行している
3. eMterm が受信して agent-status として反応している

**POC 設計上の教訓**: 当初 OSC 2（ウィンドウタイトル設定）を「allowlist 通過の対照」として同梱したが、Claude Code 自身がターミナルタイトルを常時書き換えるため即座に上書きされ、対照として機能しなかった。バッジ形状の A/B に切り替えて決着した。**POC を設計する際は、観測対象が被験系自身によって書き換えられないかを先に確認すること。**

証跡は `tmp/poc-terminal-sequence.sh` と `tmp/poc-terminal-sequence.log`（tmp/ は gitignored）。一時 hook は撤収済み。

### 14.2 未確認・保留事項

- [ ] 実機での最終確認（プロンプト送信 → working、応答完了 → idle、承認待ち → blocked の 3 遷移）は verify フェーズで実施する

## 15. 参考資料

- Claude Code hooks ドキュメント: `terminalSequence` の allowlist と `/dev/tty` 不可の記述
- Claude Code plugin-marketplaces ドキュメント: plugin source の形式
- Codex レビュー結果（2026-07-25）: 本 feature の findings の出典
- 前 feature: `feature-docs/emterm-claude-plugin/`（SPEC.md / reviews/round1.yaml / reviews/round2.yaml / retrospect.yaml）
