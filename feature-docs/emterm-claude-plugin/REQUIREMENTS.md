---
title: "emterm-claude-plugin"
created_date: 2026-07-25
status: draft
---

# emterm-claude-plugin - 要件定義書

## 1. 概要

### 1.1 背景
eMterm は AI 時代の terminal として Claude Code との親和性を長期目標に置いている。既に `emterm agent-status` OSC と `emterm mux read|send|wait` API、`emterm markdown|json|yaml|image` のリッチ表示 CLI が揃っており、Claude Code 側の hook / skill から呼び出せば eMterm の機能をそのまま享受できる。

### 1.2 目的
eMterm リポジトリを Claude Code の plugin marketplace として公開し、Claude Code ユーザーが `/plugin marketplace add` + `/plugin install` で eMterm 連携（agent-status hook、リッチ表示 skill、mux API skill）をワンステップで導入できるようにする。

### 1.3 スコープ
本 feature は以下を含む。

- `.claude-plugin/marketplace.json`（リポジトリ root）の追加
- `plugins/emterm/` プラグインの追加（plugin.json、hooks/hooks.json、hook スクリプト、skills 7 本、README）
- hook 発火 → `/dev/tty` 経由で `emterm agent-status` OSC が PTY に届くことのローカル動作確認（POC）

以下は本 feature のスコープ外とする。

- Windows 版 hook スクリプト（notify-status.ps1）とその実測
- mux-agent-status-api の drain wiring 残件（5件 deferred）の解消
- 案 B（`EMTERM_PANE_ID` 経由の `emterm mux send`）による hook fallback
- v0.2.0 以降の拡張（Notification 本文の name 反映等）

## 2. ビジネス要件

### 2.1 ビジネス目標
Claude Code ユーザーが eMterm の AI 連携機能を試す導線を作る。

### 2.2 対象ユーザー
| ユーザータイプ | 説明 |
|----------------|------|
| Claude Code ユーザー | eMterm 上または任意のターミナル上で Claude Code を使う開発者 |

### 2.3 期待される効果
- eMterm の agent-status 表示・リッチ表示・mux API が Claude Code 標準の hook / skill 経路で使えるようになる
- eMterm の存在と機能が Claude Code の plugin marketplace 経由でリーチする

## 3. ユースケース

### 3.1 ユースケース一覧
| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | plugin marketplace 追加とインストール | Claude Code ユーザー | 高 |
| UC02 | Claude Code 応答中に eMterm タブへ working/idle/blocked を反映 | Claude Code ユーザー | 高 |
| UC03 | Claude Code から Markdown/JSON/YAML/画像を eMterm のリッチ表示で見る | Claude Code ユーザー | 中 |
| UC04 | Claude Code から mux 他ペインを read/send/wait する | Claude Code ユーザー | 中 |

### 3.2 ユースケース詳細

#### UC01: plugin marketplace 追加とインストール

**アクター**: Claude Code ユーザー

**事前条件**:
- eMterm リポジトリが GitHub 上に公開されている
- ユーザー環境に `emterm` または `emterm-cli` バイナリが導入済み（未導入でも本 plugin のインストール自体は成功する）

**基本フロー**:
1. ユーザーが Claude Code で `/plugin marketplace add m-m-n/emterm` を実行する
2. ユーザーが `/plugin install emterm@emterm-plugins` を実行する
3. Claude Code がプラグインをキャッシュし、hooks.json と skills を読み込む

**事後条件**:
- Claude Code の hook / skill 一覧に `emterm` プラグインの要素が現れる

#### UC02: Claude Code 応答中に eMterm タブへ状態を反映

**アクター**: Claude Code ユーザー

**事前条件**:
- UC01 が完了している
- ユーザーが eMterm ウィンドウ内のタブで Claude Code を実行している

**基本フロー**:
1. ユーザーがプロンプトを送信すると Claude Code の `UserPromptSubmit` hook が発火し、notify-status.ts が `emterm agent-status working` を `/dev/tty` に書き込む
2. eMterm が OSC を受信し、当該タブの状態を working に更新（バッジ／ステータスバー表示）
3. Claude Code が応答を返すと `Stop` hook が発火し、`emterm agent-status idle` が同経路で送られる
4. Claude Code が待機通知を出すと `Notification` hook が発火し、`emterm agent-status blocked` が同経路で送られる

**代替フロー**:
- `emterm` が PATH に無い場合、hook スクリプトは黙って exit 0 する（no-op）
- `/dev/tty` に書けない環境（PTY が親と共有されていない等）では OSC は届かないが Claude Code 側は正常に動作する

**事後条件**:
- eMterm タブの状態表示が Claude Code のライフサイクルに追従する

#### UC03: リッチ表示 skill

**アクター**: Claude Code ユーザー

**基本フロー**:
1. ユーザーが `/emterm:display-markdown <file>` 等を実行、または Claude 自身が該当 skill を自動発動する
2. skill は `emterm markdown|json|yaml|image` を呼び、eMterm の子 WebView に表示する

#### UC04: mux API skill

**アクター**: Claude Code ユーザー

**基本フロー**:
1. ユーザーが `/emterm:mux-read|mux-send|mux-wait` を実行する
2. skill は `emterm mux <sub>` を呼び、`EMTERM_PANE_ID` の解決は既存 CLI 側に委ねる

## 4. 機能要件

### 4.1 機能一覧
| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| F01 | marketplace.json | リポジトリ root のマーケットプレイスカタログ | 高 |
| F02 | plugin.json | `plugins/emterm/` のプラグインメタデータ | 高 |
| F03 | agent-status hook 配線 | UserPromptSubmit / Stop / Notification を `emterm agent-status` に対応付ける hooks.json | 高 |
| F04 | notify-status.ts | hook から呼ばれる Bun+TypeScript スクリプト。`/dev/tty` へ OSC を送出、失敗時は no-op | 高 |
| F05 | display-* skill | markdown / json / yaml / image の 4 本の CLI ラッパー skill | 中 |
| F06 | mux-* skill | read / send / wait の 3 本の mux API ラッパー skill | 中 |
| F07 | プラグイン README | インストール手順、前提（emterm バイナリを別途入れる）、既知の制限（drain wiring 残件、Linux only、 mux fallback 未実装）を記載 | 中 |
| F08 | POC 動作確認 | ローカルで `claude --plugin-dir` 相当の手段で hook 発火 → eMterm タブへの反映を確認 | 高 |

### 4.2 機能詳細

#### F03/F04: agent-status hook 配線と notify-status.ts

**hook イベントと state のマッピング**:

| Claude Code イベント | eMterm agent-status |
|---|---|
| `UserPromptSubmit` | `working` |
| `Stop` | `idle` |
| `Notification` | `blocked` |
| `SubagentStop` | 発火させない |

**notify-status.ts の要件**:
- 引数 1: state (`idle` / `working` / `blocked` / `done` を受け付けるが v0.1.0 では `done` は未使用)
- `emterm` バイナリが PATH に無ければ黙って exit 0
- `emterm agent-status <state> --name "claude-code"` を実行し、stdout を **`/dev/tty` に書き込む**（Claude Code の stdout ではなく）
- `/dev/tty` オープン失敗、`emterm` プロセスの非 0 終了、その他の例外はすべて **黙って exit 0**（Claude Code の応答を阻害しない）
- hook の timeout は 3 秒（hooks.json 側で指定）
- 実装言語: Bun + TypeScript（`#!/usr/bin/env bun`）

**hook 実装案 B の非採用**:
- `EMTERM_PANE_ID` 環境変数経由で `emterm mux send` にフォールバックする案は v0.1.0 では実装しない。案 A が届かない環境では黙って no-op のみ。

#### F05: display-* skill

- `/emterm:display-markdown` → `emterm markdown <file>`
- `/emterm:display-json` → `emterm json <file>`
- `/emterm:display-yaml` → `emterm yaml <file>`
- `/emterm:display-image` → `emterm image <file> [--protocol kitty|sixel]`

各 SKILL.md の description は英語で、Claude が自動発動できるよう「when to use」を明記する。

#### F06: mux-* skill

- `/emterm:mux-read` → `emterm mux read ...`
- `/emterm:mux-send` → `emterm mux send ...`
- `/emterm:mux-wait` → `emterm mux wait ...`

`--pane current` の解決は既存 `emterm` CLI の `EMTERM_PANE_ID` 参照ロジックに委ねる。

## 5. 非機能要件

### 5.1 パフォーマンス要件
- hook 実行のオーバーヘッドは 3 秒 timeout 以内に収まること（Bun 起動 + `emterm agent-status` 起動 + `/dev/tty` write の合計）
- POC 段階で `UserPromptSubmit` を含む頻繁な hook 発火が Claude Code の対話体感を損なわないことをローカルで確認する

### 5.2 セキュリティ要件
- hook スクリプトは受け取った state 引数を allow-list（`idle` / `working` / `blocked` / `done`）で検証し、それ以外は no-op
- hook スクリプトが外部入力（環境変数・引数）をシェル評価せず、`emterm` バイナリへは配列引数として渡す
- プラグインはバイナリを同梱しない。README で GitHub Releases の直リンクを案内する

### 5.5 互換性要件
- 動作保証プラットフォーム: Linux（tmux 有無いずれも）
- Windows: v0.1.0 では動作保証しない。README に既知制限として明記
- 依存: `emterm`（または `emterm-cli`）バイナリを別途インストール済みであること
- `bun` を Bun runtime として PATH に持つこと（hook スクリプトが `#!/usr/bin/env bun`）

## 9. 制約条件

### 9.1 技術的制約
- Claude Code plugin cache は `../` 外を参照できないため、`plugins/<name>/` 内で完結させる
- hook の stdout は PTY に届かない前提。OSC は `/dev/tty` 直接書き込みで届かせる
- プラグインリポジトリはバイナリを追跡しない

## 10. 想定される課題とリスク

### 10.1 技術的課題
| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| Claude Code の実行環境で `/dev/tty` オープンが常に成功する保証がない（headless / 特殊 SSH 条件など） | 中 | POC で実測。届かない環境では no-op で degrade する設計 |
| `UserPromptSubmit` は毎入力で発火する | 中 | Bun 起動オーバーヘッドを POC で実測。3 秒 timeout。問題があれば実装最適化を別 feature で検討 |
| mux-agent-status-api の drain wiring 残件で state が可視化されないケースが残っている | 中 | README に既知制限として明記。修正は別 feature |

## 11. 成功基準

### 11.1 受け入れ基準
- [ ] `.claude-plugin/marketplace.json` と `plugins/emterm/` 一式がリポジトリに追加されている
- [ ] `plugins/emterm/hooks/hooks.json` が UserPromptSubmit / Stop / Notification を notify-status.ts に配線している
- [ ] notify-status.ts が `emterm` 未インストール時 no-op、インストール時に `/dev/tty` 経由で OSC を送出する（POC 実測）
- [ ] display-* skill 4 本、mux-* skill 3 本が SKILL.md を持ち description が英語で書かれている
- [ ] plugins/emterm/README.md が導入手順・前提・既知制限を明記している
- [ ] `bun test` と `bun run typecheck` が本 feature の変更後もパスする
- [ ] ローカルで `/plugin marketplace add` 相当の手段でインストールし、eMterm タブに state 変化が反映されることを確認できている

## 12. テストシナリオ

### 12.1 テスト観点
- [ ] 正常系: hook 発火 → `emterm agent-status` 呼び出し → OSC が `/dev/tty` に書かれる（POC 手動確認）
- [ ] 異常系: `emterm` バイナリ未インストール → hook スクリプトが exit 0 で終了、Claude Code の応答を阻害しない
- [ ] 異常系: `/dev/tty` オープン失敗 → hook スクリプトが exit 0 で終了
- [ ] 異常系: state 引数が allow-list 外 → no-op
- [ ] 静的: `plugin.json` / `marketplace.json` の schema 妥当性、Claude Code が plugin としてロードできる

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| plugin marketplace | Claude Code のプラグイン配布機構 |
| hook | Claude Code のライフサイクルイベントで実行される外部コマンド |
| skill | Claude Code のスラッシュコマンドとして呼ばれる指示書 |
| agent-status | eMterm 独自の OSC で、タブの状態（idle/working/blocked/done）を eMterm 本体に報告する |
| mux | eMterm 内蔵の tmux 風多重化機構 |

## 14. 確認事項

### 14.1 確認済み事項
- [x] feature 名: `emterm-claude-plugin`
- [x] marketplace 名: `emterm-plugins` / plugin 名: `emterm`（`/plugin install emterm@emterm-plugins`）
- [x] POC（`/dev/tty` 経由の hook → OSC）は本 feature の implement フェーズで実測する
- [x] Windows は v0.1.0 に含めない（Linux 専用としてリリース）
- [x] mux-agent-status-api の drain wiring 残件は本 feature では潰さず README で既知制限として明記
- [x] agent-status の state マッピング: `Stop=idle` に寄せる（`done` は使わない）
- [x] hook スクリプトは案 A（`/dev/tty`）のみ、失敗時は黙って no-op（案 B の fallback は入れない）
- [x] hook スクリプトの実装言語: Bun + TypeScript
- [x] SKILL.md の description は英語で書く

### 14.2 未確認・保留事項
- [ ] POC で `/dev/tty` 書き込みが Claude Code 実行環境で通ることの実測結果
- [ ] Bun 起動オーバーヘッドの実測値
