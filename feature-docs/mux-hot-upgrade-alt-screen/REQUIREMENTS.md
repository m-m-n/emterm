---
title: "mux-hot-upgrade-alt-screen"
created_date: 2026-08-04
status: draft
---

# mux-hot-upgrade-alt-screen - 要件定義書

## 1. 概要

### 1.1 背景

mux デーモンのホットアップグレード（バイナリ更新検出による self-exec）を行うと、alt-screen 上で動作している TUI（Claude Code, glances, nethogs, その他 ncurses TUI）が、アップグレード後の再アタッチで alt-screen 移行前のスクロールバック断片を表示し、画面が破損する。

あわせて、umask 002（Debian/Ubuntu 既定）環境の開発ビルドではホットアップグレードがそもそも発火しない。原因は NFR3 の group-write チェック（`src-tauri/src/mux/identity.rs:455`、deferred high finding `sid-nfr3-group-write-blocks-dev-builds`）が private per-user group の 0o775 パスを拒否することにある。この状態では上記の alt-screen 修正を開発ビルドで検証できない。

### 1.2 目的

- ホットアップグレードとその後の再アタッチを経ても alt-screen TUI が破損せず正しく描画されるようにする。
- umask 002 の開発ビルドでホットアップグレードを発火可能にし、上記修正を検証できるようにする。

### 1.3 スコープ

**対象**:

- NFR3 パス書き込み可能性チェックにおける private per-user group の例外扱い（FR1）と world-write 拒否の現行維持（FR2）
- `HandoffPane`（`crates/mux_ipc/src/handoff.rs`）への alt-screen 状態の追加と `HANDOFF_SCHEMA_VERSION` の 3 への引き上げ（FR3）
- V2 から V3 への移行と既存挙動の保持（FR4）
- `mux::upgrade::snapshot_pane` による既存 main/alt 分岐に沿った取得（FR5）
- `restore_pane` / `MuxPane::from_restored` による shadow_parser への alt-screen 状態リプレイ（FR6）
- client-ack 待機後の再取得によるティアリング窓の縮小（FR7）
- alt-screen ダンプのサイズ上限ポリシーの明文化（FR8）

**対象外**（タスク記述による）:

- reptyr その他の外部プロセス移行依存
- 残り 4 件の deferred high findings（`sid-probe-exec-not-pinned-same-file`, `sid-legacy-daemon-shutdown-on-upgrade-refusal`, `sid-cli-upgrade-coordinator-duplication-false-success`, `sid-probe-stdout-drain-unbounded`）
- 隣接する stale-scrollback-before-exec の論点

**design / plan ステップに委ねる事項**:

- alt-screen ダンプの具体的なサイズ上限値と超過時の挙動（FR8）

## 2. ビジネス要件

### 2.1 ビジネス目標

- mux デーモンのホットアップグレード（バイナリ更新検出による self-exec）が alt-screen TUI を破損させないこと。アップグレードとその後の再アタッチを経ても、alt-screen 上のアプリケーション（Claude Code, glances, nethogs, その他 ncurses TUI）が alt-screen 移行前のスクロールバック断片ではなく正しい画面を描画すること。
- umask 002（Debian/Ubuntu 既定）環境の開発ビルドがホットアップグレードを発火できること。これにより上記修正が開発ビルドで検証可能になる。そのためには現状 private per-user group の 0o775 パスを拒否している NFR3 group-write チェック（`src-tauri/src/mux/identity.rs:455`、deferred high finding `sid-nfr3-group-write-blocks-dev-builds`）の是正が必要。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| mux 上で alt-screen TUI を使う eMterm 利用者 | Claude Code / glances / nethogs 等を mux ペインで実行し、ホットアップグレードとその後の再アタッチを跨いで使う |
| 開発ビルドで検証する開発者 | umask 002 の環境で `src-tauri/target-host/release/emterm` からデーモンを起動し、ホットアップグレードの動作を確認する |

### 2.3 期待される効果

- ホットアップグレード + 再アタッチ後も alt-screen TUI の画面が破損せず、alt-screen 移行前のシェル断片が透けない。
- umask 002 の開発ビルドでホットアップグレードが発火し、修正内容を実機で検証できる。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | alt-screen TUI を開いたままホットアップグレードして再アタッチする | mux 上で alt-screen TUI を使う eMterm 利用者 | 高 |
| UC02 | umask 002 の開発ビルドでホットアップグレードを発火させる | 開発ビルドで検証する開発者 | 高 |

### 3.2 ユースケース詳細

#### UC01: alt-screen TUI を開いたままホットアップグレードして再アタッチする

**アクター**: mux 上で alt-screen TUI を使う eMterm 利用者

**事前条件**:

- ペインで alt-screen TUI（Claude Code / glances 等）が動作している

**基本フロー**:

1. 利用者がホットアップグレードをトリガする
2. デーモンが client-ack 待機後に alt-screen フラグとダンプを再取得する（FR7）
3. `snapshot_pane` が既存の main/alt 分岐に沿ってペイン状態を取得する（FR5）
4. デーモンが self-exec する
5. `restore_pane` / `MuxPane::from_restored` が復元済みスクロールバックを先にリプレイし、alt-screen フラグが true のとき `ESC[?1049h` と取得済み画面ダンプを shadow_parser に流し込む（FR6）
6. 利用者が再アタッチする
7. alt-screen TUI の画面が破損せず表示される

**代替フロー**:

- alt-screen フラグが false（main buffer のペイン）の場合は現行どおりの挙動を維持する（FR5）
- V1 / V2 由来の handoff ドキュメントの場合は alt フラグ false・空ダンプで補われ、既存の復元挙動が維持される（FR4 / NFR3）

**事後条件**:

- 復元後のパーサーが `alternate_screen() == true` を報告し、次回再アタッチの `build_snapshot_bytes` が alt 分岐を通る
- alt-screen 移行前のシェル断片が画面に透けない

#### UC02: umask 002 の開発ビルドでホットアップグレードを発火させる

**アクター**: 開発ビルドで検証する開発者

**事前条件**:

- umask 002 のマシンであること
- 開発ビルド（`src-tauri/target-host/release/emterm`）からデーモンが起動していること

**基本フロー**:

1. デーモンがバイナリ／親ディレクトリの書き込み可能性チェックを実行する
2. group-write（S_IWGRP）が立っている 0o775 のパスについて、FR1 の 3 条件をすべて満たすか判定する
3. 3 条件をすべて満たすためパスが受理される
4. ホットアップグレードが発火する

**代替フロー**:

- 3 条件のいずれかが不成立、または確認不能な場合は現行どおりパスを拒否する（FR1 / NFR2）
- S_IWOTH が立っているパスは無条件に拒否する（FR2）

**事後条件**:

- 開発ビルドのデーモンでホットアップグレードが発火している

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | NFR3 パス書き込み可能性チェックにおける private per-user group 例外 | 3 条件すべてを満たす場合に限り group-write を許容し、それ以外は現行どおり拒否する | 高 |
| FR2 | world-write 拒否の現行維持 | S_IWOTH が立つパスは現行実装どおり無条件で拒否する | 高 |
| FR3 | HandoffPane への alt-screen 状態追加とスキーマ version 3 | alt-screen フラグと画面ダンプを追加し、`HANDOFF_SCHEMA_VERSION` を 2 から 3 に引き上げる | 高 |
| FR4 | V2 から V3 への移行と既存挙動の保持 | 対応バージョン範囲を 1..=3 に広げ、既存パターンで V2→V3 移行を追加する | 高 |
| FR5 | 既存の main/alt 分岐に沿った snapshot_pane の取得 | `build_snapshot_bytes` と同じ main/alt 分岐契約でペイン状態を取得する | 高 |
| FR6 | 復元時の shadow_parser への alt-screen 状態リプレイ | スクロールバックのリプレイ後、alt フラグが true なら `ESC[?1049h` + ダンプを流し込む | 高 |
| FR7 | ティアリング窓を縮小する alt 状態の再取得 | 既存 `refresh_live_agent_state` パスと同じ地点で alt フラグ・ダンプを再取得する | 高 |
| FR8 | alt-screen ダンプのサイズ上限ポリシー | 上限ポリシーを明文化する（具体値と超過時の挙動は design / plan ステップで決定） | 高 |

### 4.2 機能詳細

#### FR1: NFR3 パス書き込み可能性チェックにおける private per-user group 例外

**説明**: `src-tauri/src/mux/identity.rs` のバイナリ／親ディレクトリ書き込み可能性チェックは、以下の 3 条件がすべて成立する場合に限り group-write（S_IWGRP）を許容する。(a) そのグループの `gr_mem` に所有者以外の名前が含まれないこと。(b) 所有者のプライマリ gid がそのグループの gid と一致すること。(c) グループ名が所有者のユーザー名と一致すること。いずれかの条件が不成立、または確認できない場合は現行どおりパスを拒否する（fail-closed）。

**入力**:

- 対象パス: バイナリまたはその親ディレクトリ
- パスのモードビットおよび所有者・グループ情報

**出力**:

- 判定結果: 受理 / 拒否

**ビジネスルール**:

- 3 条件は AND 条件であり、すべて成立する場合のみ受理する
- 条件が確認できない場合は拒否する（fail-closed）
- 親ディレクトリもチェック対象に含む

**バリデーション**:

| 項目 | ルール | エラーメッセージ |
|------|--------|------------------|
| `gr_mem` | 所有者以外の名前を含まないこと | 現行の拒否経路に従う |
| プライマリ gid | 所有者のプライマリ gid がグループの gid と一致すること | 現行の拒否経路に従う |
| グループ名 | 所有者のユーザー名と一致すること | 現行の拒否経路に従う |

**エラーケース**:

| エラー | 条件 | 対応 |
|--------|------|------|
| 条件不成立 | 3 条件のいずれかが不成立 | パスを拒否 |
| 確認不能 | ルックアップ失敗など条件を確認できない | パスを拒否（fail-closed、NFR2） |

#### FR2: world-write 拒否の現行維持

**説明**: S_IWOTH が立っているパスは、現行実装どおり無条件に拒否する。

**入力**:

- 対象パスのモードビット

**出力**:

- 判定結果: 拒否

**ビジネスルール**:

- S_IWOTH が立っている場合、FR1 の判定結果によらず拒否する

**バリデーション**: 該当なし

**エラーケース**:

| エラー | 条件 | 対応 |
|--------|------|------|
| world-write | S_IWOTH が立っている | 無条件に拒否 |

#### FR3: HandoffPane への alt-screen 状態追加とスキーマ version 3

**説明**: `HandoffPane`（`crates/mux_ipc/src/handoff.rs`）に alt-screen フラグと alt-screen 画面ダンプを追加し、`HANDOFF_SCHEMA_VERSION` を 2 から 3 に引き上げる。

**入力**:

- alt-screen フラグ
- alt-screen 画面ダンプ

**出力**:

- version 3 の handoff ドキュメント

**ビジネスルール**:

- `HANDOFF_SCHEMA_VERSION` は 3 とする

**バリデーション**: 該当なし

**エラーケース**: 該当なし

#### FR4: V2 から V3 への移行と既存挙動の保持

**説明**: `SUPPORTED_HANDOFF_SCHEMA_VERSIONS` を 1..=3 に広げ、V1→V2 と同じ確立済みパターン（バージョンごとの struct + `From` 実装）で V2→V3 移行を追加する。V2 由来（および既存チェーン経由の V1 由来）のドキュメントは alt フラグ = false・空ダンプで補われ、既存の復元挙動が保持される。

**入力**:

- V1 / V2 / V3 の handoff ドキュメント

**出力**:

- V3 に移行された handoff ドキュメント

**ビジネスルール**:

- V2 由来のドキュメントは alt フラグ false・空ダンプで補う
- V1 は既存チェーン経由で同様に扱う
- 対応バージョン範囲として 1..=3 を提示する

**バリデーション**: 該当なし

**エラーケース**: 該当なし

#### FR5: 既存の main/alt 分岐に沿った snapshot_pane の取得

**説明**: `mux::upgrade::snapshot_pane`（`src-tauri/src/mux/upgrade.rs:547`）は、`build_snapshot_bytes`（`src-tauri/src/mux/session/pane.rs:1073` および `1229`）と同じ main/alt 分岐契約でペイン状態を取得する。alt-screen のペインは alt フラグと `contents_formatted()` 相当のダンプを提供し、main buffer のペインは現行どおりとする。

**入力**:

- ペイン状態

**出力**:

- alt フラグ + alt-screen ダンプ（alt-screen ペインの場合）
- 現行どおりのスナップショット（main buffer ペインの場合）

**ビジネスルール**:

- `build_snapshot_bytes` と同じ main/alt 分岐契約を用いる
- main buffer ペインの挙動は変更しない

**バリデーション**: 該当なし

**エラーケース**: 該当なし

#### FR6: 復元時の shadow_parser への alt-screen 状態リプレイ

**説明**: `restore_pane`（`src-tauri/src/mux/upgrade.rs:687`）／`MuxPane::from_restored`（`src-tauri/src/mux/session/pane.rs:1971`）は、まず復元されたスクロールバックをリプレイし、次に alt-screen フラグが true の場合に `ESC[?1049h` と取得済み画面ダンプを shadow_parser へ流し込む。これにより復元後のパーサーが `alternate_screen() == true` を報告する。

**入力**:

- 復元されたスクロールバック
- alt-screen フラグ
- alt-screen 画面ダンプ

**出力**:

- `alternate_screen() == true` を報告する shadow_parser（alt フラグが true の場合）

**ビジネスルール**:

- スクロールバックのリプレイを先に行い、その後で `ESC[?1049h` + ダンプを流す
- alt フラグが false の場合はリプレイのみを行う

**バリデーション**: 該当なし

**エラーケース**: 該当なし

#### FR7: ティアリング窓を縮小する alt 状態の再取得

**説明**: alt-screen フラグとダンプを、既存の `refresh_live_agent_state` パス（`src-tauri/src/mux/upgrade.rs:384`、client-ack 待機後に呼ばれる）と同じ地点で再取得する。これによりスナップショットから exec までのバッファ切り替えによるズレは、エージェント状態の陳腐化と同じ窓幅に縮小される。

**入力**:

- client-ack 待機後のライブなペイン状態

**出力**:

- 再取得された alt-screen フラグとダンプ

**ビジネスルール**:

- 再取得地点は既存の `refresh_live_agent_state` パスと同じ地点とする

**バリデーション**: 該当なし

**エラーケース**: 該当なし

#### FR8: alt-screen ダンプのサイズ上限ポリシー

**説明**: handoff ドキュメントの alt-screen ダンプには、明文化された上限ポリシーを設ける。`contents_formatted()` は極端な画面寸法で肥大しうる（`src-tauri/src/mux/session/pane.rs:1282`）。handoff はファイルであるため、スナップショットフレームの制限は適用されない。具体的な上限値と超過時の挙動は design / plan ステップで決定する。

**入力**:

- alt-screen 画面ダンプ

**出力**:

- 上限ポリシーが適用されたダンプ

**ビジネスルール**:

- 上限ポリシーがドキュメント上で明示されていること
- 具体値と超過時の挙動は design / plan ステップで決定する

**バリデーション**: 該当なし（具体値は design / plan ステップで決定）

**エラーケース**: 該当なし（超過時の挙動は design / plan ステップで決定）

## 5. 非機能要件

### 5.0 非機能要件一覧

| ID | 機能名 | 説明 |
|----|--------|------|
| NFR1 | ID ルックアップの有界化 | private group 判定は `getgrgid` 1 回と `getpwuid` 1 回のみを用い、passwd データベースを列挙しない（`getpwent` を使わない） |
| NFR2 | fail-closed のセキュリティ姿勢 | group-write チェックにおけるルックアップ失敗・確認不能はすべて拒否とする |
| NFR3 | handoff デコードの後方互換 | V1 / V2 の handoff ドキュメントは現行と同一の挙動（alt フラグ false・空ダンプ）でデコード・復元できる |
| NFR4 | 新概念・外部依存の不追加 | 既存の main/alt 分岐契約と既存のバージョン別 handoff 移行パターンを再利用し、外部のプロセス移行ツールを導入しない |

### 5.1 パフォーマンス要件

- NFR1: private group 判定は `getgrgid` 1 回・`getpwuid` 1 回のみ。`getpwent` による passwd データベースの全走査は行わない（LDAP/SSSD 環境で全走査は低速・不安定なため）。

### 5.2 セキュリティ要件

- NFR2: group-write チェックにおいて、ルックアップ失敗や条件の確認不能はすべて拒否とする（fail-closed）。
- 脅威モデル: デーモンは当該パスを `execve()` するため、所有者以外（root を除く）による書き込み権限はデーモン権限での任意コード実行を意味する。親ディレクトリもチェック対象に含める。
- FR2: S_IWOTH が立つパスは無条件に拒否する。

### 5.3 可用性要件

該当なし

### 5.4 保守性要件

- NFR4: 既存の main/alt 分岐契約と、既存のバージョン別 handoff 移行パターン（バージョンごとの struct + `From` 実装）を再利用する。新しい概念を導入しない。

### 5.5 互換性要件

- NFR3: V1 および V2 の handoff ドキュメントは、現行と同一の挙動（alt フラグ false・空ダンプ）でデコード・復元される。
- FR4: `SUPPORTED_HANDOFF_SCHEMA_VERSIONS` は 1..=3 を提示する。

## 6. UI/UX要件

### 6.1 画面設計要件

該当なし。本機能はデーモン内部のバグ修正（execve を跨いだパーサー状態のシリアライズと、ファイルパーミッション判定）であり、UI サーフェスを持たない。design ステップはスキップされている（理由: UI サーフェスが無く、視覚・デザイントークンの関与も無いため design ステップが何も追加しない）。

### 6.2 画面遷移

該当なし

### 6.3 レスポンシブ対応

該当なし

## 7. データ要件

### 7.1 データモデル概要

handoff ドキュメント（`crates/mux_ipc/src/handoff.rs`）が、ホットアップグレードの execve を跨いでペイン状態を運ぶ。本要件では `HandoffPane` に alt-screen 状態が加わり、スキーマバージョンが 3 になる。

### 7.2 データ項目

| エンティティ | 項目名 | 型 | 必須 | 説明 |
|--------------|--------|-----|------|------|
| HandoffPane | alt-screen フラグ | 真偽値 | ○ | ペインが alt-screen 上にあるか（FR3）。V1/V2 由来は false（FR4 / NFR3） |
| HandoffPane | alt-screen 画面ダンプ | 画面ダンプ | ○ | `contents_formatted()` 相当のダンプ（FR3 / FR5）。V1/V2 由来は空（FR4 / NFR3） |
| handoff ドキュメント | `HANDOFF_SCHEMA_VERSION` | バージョン番号 | ○ | 2 から 3 へ引き上げ（FR3） |
| handoff ドキュメント | `SUPPORTED_HANDOFF_SCHEMA_VERSIONS` | バージョン範囲 | ○ | 1..=3 に拡大（FR4） |

### 7.3 データ保持期間

| データ種別 | 保持期間 |
|------------|----------|
| handoff ドキュメント | 該当なし（ホットアップグレードの引き継ぎに用いるファイル。保持期間の要件は挙がっていない） |

## 8. 外部連携

### 8.1 連携システム

該当なし

### 8.2 API仕様要件

該当なし。NFR4 のとおり、外部のプロセス移行ツールは導入しない（reptyr は不採用。本件の欠陥はメモリ上の shadow_parser 状態にあり、この種のツールでは運べないため）。

## 9. 制約条件

### 9.1 技術的制約

- private group 判定で使用できる ID ルックアップは `getgrgid` 1 回・`getpwuid` 1 回のみ。`getpwent` による全走査は不可（NFR1）
- group-write チェックは fail-closed であること。親ディレクトリもチェック対象（NFR2）
- V1 / V2 の handoff ドキュメントの挙動を変えられない（NFR3）
- 既存の main/alt 分岐契約と既存のバージョン別 handoff 移行パターンを再利用し、外部依存を追加しない（NFR4）
- `contents_formatted()` は極端な画面寸法で肥大しうる（`src-tauri/src/mux/session/pane.rs:1282`）。handoff はファイルのためスナップショットフレームの制限は適用されない（FR8）
- 本機能は Unix 限定のサーフェス（mux ホットアップグレード、identity チェックは libc を使用）であり、Windows の挙動には影響しない
- 統合テスト `src-tauri/tests/mux_hot_upgrade.rs` は `--test-threads=1` で実行する

### 9.2 ビジネス上の制約

- 実装順は (1) NFR3 修正 → (2) alt-screen handoff。これは (2) を開発ビルドで検証可能にするためのプロセス上の制約であり、ランタイム要件ではない。

### 9.3 スケジュール制約

該当なし

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| スナップショットから exec までの間にバッファが切り替わるとティアリングが起きる | 中 | 既存 `refresh_live_agent_state` パスと同じ地点で alt フラグ・ダンプを再取得し、窓幅をエージェント状態の陳腐化と同等に縮小する（FR7） |
| `contents_formatted()` が極端な画面寸法で肥大しうる | 中 | 明文化された上限ポリシーを設ける。具体値と超過時の挙動は design / plan ステップで決定（FR8） |
| 統合テストハーネス（`src-tauri/tests/mux_hot_upgrade.rs`）で alt-screen ペインのシナリオを組めるか不明 | 中 | 可能なら統合テストを拡張し、不可なら単体レベルの固定 + 手動確認（AC-2 / AC-5）で代替する |
| デーモンが対象パスを `execve()` するため、所有者以外の書き込み権限はデーモン権限での任意コード実行を意味する | 高 | 3 条件の AND 判定と fail-closed（FR1 / NFR2）、S_IWOTH の無条件拒否（FR2） |

### 10.2 ビジネスリスク

該当なし

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC-1（FR1/FR2/NFR1/NFR2、自動）: group-write 判定が、所有者の private per-user group を持つ 0o775 パス（FR1 の 3 条件すべて成立）を受理し、次を拒否する — `gr_mem` に追加メンバーがいるグループ、gid が所有者のプライマリ gid でないグループ、名前が所有者のユーザー名と異なるグループ（例: gid=100 の "users" がプライマリ）、ルックアップ失敗、S_IWOTH のパス。
- [ ] AC-2（FR1、手動）: umask 002 のマシンで、開発ビルド（`src-tauri/target-host/release/emterm`）から起動したデーモンがホットアップグレードを発火する。
- [ ] AC-3（FR3/FR5/FR6、自動）: handoff レコードの alt フラグが true の復元済みペインは、`from_restored` 後に `shadow_parser.screen().alternate_screen() == true` となり、次回再アタッチの `build_snapshot_bytes` が alt 分岐を通る。テストで固定する。
- [ ] AC-4（FR4/NFR3、自動）: V2 ドキュメントが alt = false・空ダンプで V3 に移行し、従来どおり復元される。バージョン範囲 1..=3 が提示される。
- [ ] AC-5（FR6/FR7、手動）: alt-screen TUI（Claude Code / glances）を開いた状態でホットアップグレードをトリガし、再アタッチ後に画面が破損せず、alt-screen 移行前のシェル断片が透けない。

### 11.2 KPI

該当なし

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] 正常系・異常系（TS1）: `src-tauri/src/mux/identity.rs` の private group 判定について、FR1 / FR2 の全分岐（受理および各拒否理由）の単体テスト。既存の inline `#[cfg(test)]` 規約に従う。
- [ ] 互換性（TS2）: `crates/mux_ipc/src/handoff.rs` の V2→V3 `From` 移行と、拡大した `SUPPORTED_HANDOFF_SCHEMA_VERSIONS` 範囲の単体テスト。既存の V1→V2 テストに倣う。
- [ ] 正常系（TS3）: `src-tauri/src/mux/{upgrade.rs,session/pane.rs}` の `snapshot_pane` / `restore_pane` / `from_restored` 周辺の単体テスト。alt ペインが `alternate_screen() == true` へラウンドトリップすること、main ペインの挙動が不変であること、スナップショット後にバッファを切り替えたペインが再取得地点で更新されること。
- [ ] 統合（TS4）: `src-tauri/tests/mux_hot_upgrade.rs`（`--test-threads=1` で実行）を alt-screen ペインのシナリオで拡張する（当該ハーネスで実現可能な場合）。不可なら単体レベルの固定と手動確認 AC-2 / AC-5 で代替する。
- [ ] 手動（TS5）: AC-2（umask 002 下で開発ビルドのホットアップグレードが発火する）および AC-5（アップグレード + 再アタッチ後も alt-screen TUI が無傷）を、ユーザーが実機で確認する。
- [ ] 境界値: 該当なし
- [ ] セキュリティ: TS1 に含む（fail-closed と S_IWOTH 無条件拒否の分岐）
- [ ] パフォーマンス: 該当なし

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| ホットアップグレード | バイナリ更新検出による mux デーモンの self-exec |
| alt-screen | 代替画面バッファ。ncurses TUI（Claude Code, glances, nethogs 等）が使用する |
| handoff ドキュメント | `crates/mux_ipc/src/handoff.rs` が定義する、ホットアップグレードを跨いでペイン状態を運ぶファイル |
| shadow_parser | 復元されたペインが保持するパーサー。`alternate_screen()` を報告する |
| private per-user group | 所有者ひとりだけを含み、gid が所有者のプライマリ gid と一致し、グループ名が所有者のユーザー名と一致するグループ（FR1 の 3 条件） |
| `sid-nfr3-group-write-blocks-dev-builds` | 本要件で是正する deferred high finding。NFR3 group-write チェックが private per-user group の 0o775 パスを拒否する問題 |

## 14. 確認事項

### 14.1 確認済み事項

- [x] 要件の出典: タスク記述の受け入れ基準リストを権威ある要件ソースとする。構造化と現行 worktree での行参照の裏取り以外に要件を再導出していない。
- [x] 行参照の裏取り（すべて確認済み）: `identity.rs:455` は S_IWGRP|S_IWOTH をマスクしている。`HANDOFF_SCHEMA_VERSION` は現在 2 で範囲は 1..=2。`from_restored` は `pane.rs:1971`。alt 分岐は `pane.rs:1075` / `1231`。`snapshot_pane` / `restore_pane` / `refresh_live_agent_state` は `upgrade.rs:547` / `687` / `384`。
- [x] 実装順: (1) NFR3 修正 → (2) alt-screen handoff。(2) を開発ビルドで検証可能にするためのプロセス制約であり、ランタイム要件ではない。
- [x] スコープ外（タスク記述による）: reptyr その他の外部プロセス移行依存、残り 4 件の deferred high findings（`sid-probe-exec-not-pinned-same-file`, `sid-legacy-daemon-shutdown-on-upgrade-refusal`, `sid-cli-upgrade-coordinator-duplication-false-success`, `sid-probe-stdout-drain-unbounded`）、隣接する stale-scrollback-before-exec の論点。
- [x] 対象プラットフォーム: Unix 限定のサーフェス（mux ホットアップグレード、identity チェックは libc を使用）。Windows の挙動は影響を受けない。

### 14.2 未確認・保留事項

- [ ] alt-screen ダンプの具体的なサイズ上限（FR8）は design / plan ステップで決定する。本要件で求めるのは「明文化されたポリシーが存在すること」のみ。
- [ ] 統合テストハーネス（`src-tauri/tests/mux_hot_upgrade.rs`）で alt-screen ペインのシナリオが実現可能かは未確定。不可の場合は単体レベルの固定と手動確認 AC-2 / AC-5 で代替する。

## 15. 参考資料

- `src-tauri/src/mux/identity.rs`（`:455`）: NFR3 パス書き込み可能性チェック（FR1 / FR2 / NFR1 / NFR2）
- `crates/mux_ipc/src/handoff.rs`: `HandoffPane` / `HANDOFF_SCHEMA_VERSION` / `SUPPORTED_HANDOFF_SCHEMA_VERSIONS`（FR3 / FR4 / NFR3）
- `src-tauri/src/mux/upgrade.rs`（`:384` / `:547` / `:687`）: `refresh_live_agent_state` / `snapshot_pane` / `restore_pane`（FR5 / FR6 / FR7）
- `src-tauri/src/mux/session/pane.rs`（`:1073` / `:1229` / `:1971` / `:1282`）: `build_snapshot_bytes` の main/alt 分岐、`MuxPane::from_restored`、`contents_formatted()`（FR5 / FR6 / FR8）
- `src-tauri/tests/mux_hot_upgrade.rs`: ホットアップグレードの統合テスト（`--test-threads=1` で実行）
- deferred high finding `sid-nfr3-group-write-blocks-dev-builds`: 本要件で是正する対象
