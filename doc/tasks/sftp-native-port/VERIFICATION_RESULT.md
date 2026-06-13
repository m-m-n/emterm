# 🔍 実装自動検証レポート: sftp-native-port

**検証日時**: 2026-06-13 16:51 JST
**対象機能**: SFTP Upload — native-poc Port
**VERIFICATION.md**: `doc/tasks/sftp-native-port/VERIFICATION.md`
**プロジェクト**: emterm (native-poc / emterm-native-poc)
**HEAD**: c7a3403c139bc334eff8b57948098d2292aee2a4（未コミット・作業ツリー）

---

## 📊 検証サマリー

| 検証項目 | 結果 | 詳細 |
|---------|------|------|
| ビルド | ✅ | `cargo check` 成功（warning は既存・非sftp） |
| テスト実行 | ✅ | 1289 passed / 0 failed / 1 ignored（sftp 88件） |
| コードフォーマット | ✅ | `cargo fmt --check` 差分なし |
| 静的解析 | ✅ | clippy sftp モジュール警告 0 |
| ファイル構造 | ✅ | 作成 9/9・変更 7/7 すべて存在 |
| SPEC.md適合性 | ✅ | FR1-12 / NFR1-5 実装・TS-1..TS-16 で検証 |
| セキュリティレビュー | ✅ | 自動レビュー検出の2件（argv 密輸 / ペーストインジェクション）を修正・テスト追加 |
| 変更スコープ | ✅ | native-poc + SDD docs のみ（WebView/src-tauri 無変更） |

**総合評価**: ✅ すべての自動検証項目をクリア

> build/test/format/static は sdd.5-check で実測済み。本 sdd.6 では再実行せず、ファイル構造・SPEC適合・変更スコープ・手動項目抽出を実施。

---

## ✅ ファイル構造検証（16/16）

### 作成ファイル（9個）
- ✅ `native-poc/src/sftp/mod.rs`
- ✅ `native-poc/src/sftp/args.rs`
- ✅ `native-poc/src/sftp/check.rs`
- ✅ `native-poc/src/sftp/pool.rs`
- ✅ `native-poc/src/sftp/progress.rs`
- ✅ `native-poc/src/sftp/process.rs`
- ✅ `native-poc/src/sftp/service.rs`
- ✅ `native-poc/src/sftp/remote_path.rs`
- ✅ `native-poc/src/sftp/ui.rs`

### 変更ファイル（7個）
- ✅ `native-poc/src/main.rs`（`mod sftp` 登録）
- ✅ `native-poc/src/profiles.rs`（spawn-overrides が SSH 接続名を保持）
- ✅ `native-poc/src/tabs.rs`（Tab が接続名 + lookup）
- ✅ `native-poc/src/app.rs`（SFTP UI 状態 + progress/result pump + service + close guard）
- ✅ `native-poc/src/window_host.rs`（winit ドロップ + settings reload cap）
- ✅ `native-poc/src/render/mod.rs`（オーバーレイ/ダイアログ/トースト描画）
- ✅ `native-poc/src/settings.rs`（`sftp_max_concurrent_uploads` の dead_code allow 除去）

> 注: 計画上の `i18n.rs` 変更は未実施。native-poc は文言テーブルを持たず各 UI が
> `match app.locale` でインライン分岐する設計（既存 `profile_selector` 方式）。SFTP も
> `render/mod.rs` でインライン en/ja を提供し FR12 を満たす（VERIFICATION.md に記録済み）。

---

## ✅ SPEC.md 適合性検証（FR 17 / NFR 5）

| 要件 | Phase | 検証 | 結果 |
|------|-------|------|------|
| FR1 コア移植 | A | TS-1..4 + isolation grep | ✅ |
| FR2 service | B | TS-6, TS-10 | ✅ |
| FR3 per-tab SSH | C | TS-13 | ✅ |
| FR4 drop dispatch | D | TS-9 | ✅ |
| FR5 remote path | D | TS-7 | ✅ |
| FR6 重複+上書き | E | TS-2, TS-12 | ✅ |
| FR7 並行制限 | A/B/F | TS-4, TS-5 | ✅ |
| FR8 進捗トースト | E | TS-11 | ✅ |
| FR9 キャンセル | E | 手動 + pool slot 解放 | ✅(自動)/手動 |
| FR10 クローズガード | F | TS-14 | ✅ |
| FR11 settings 反映 | F | TS-5 | ✅ |
| FR12 i18n | E/F | 手動（en/ja インライン） | 手動 |
| NFR1 セキュリティ | A/B/D | TS-6, TS-15, TS-16 | ✅ |
| NFR2 アーキ（Tauri非依存） | A | isolation grep（コードレベル依存なし） | ✅ |
| NFR3 応答性 | B/E | 手動 | 手動 |
| NFR4 wall-clock不使用 | B/E | TS-10 + レビュー | ✅ |
| NFR5 クロスプラットフォーム | A/B | レビュー（Unix/Windows 検出） | ✅ |

### 成功基準（SC）
- ✅ SC-1: 全 FR 実装 + 純粋ロジックをユニットテスト（88 sftp テスト green）
- ✅ SC-2: `grep tauri native-poc/src/sftp/` はコードレベル依存なし（doc コメント言及のみ）
- ✅ SC-3: crate ビルド + テスト合格（1289）
- ✅ SC-4: 既存 WebView E2E は影響なし（**変更スコープが native-poc 限定で WebView/src-tauri を一切触っていない**ことで構造的に担保）
- ⏳ SC-5: 手動 US1/US2 確認（下記チェックリスト）

---

## 🐳 E2E テスト結果

- Docker 環境: 存在する（`scripts/run-e2e-docker.sh` / `docker-compose.e2e.yml` / `e2e-tests/`）
- 実行: **未実行（スコープ外）**
- 理由: 既存 E2E は WebView 版（tauri-driver/WebdriverIO）専用で native-poc バイナリを
  カバーしない。本機能の変更は native-poc 限定で WebView/src-tauri ソースを一切変更して
  いないため、WebView E2E への回帰リスクは構造的にゼロ（SC-4）。native-poc の挙動は
  手動確認による。

---

## 📋 手動確認が必要な項目（E2E不可・native-poc 実機）

ビルド済み `native-poc/target-host/release/emterm-native-poc` で確認:

- [ ] SSH タブ: ファイルをドロップ → アップロードダイアログ → 確定 → トースト完了
- [ ] ディレクトリのドロップ → 再帰アップロード
- [ ] 同名ファイル → 上書き確認ダイアログが出る
- [ ] 非 SSH タブ: ファイルをドロップ → 整形パスが端末に貼り付く
- [ ] 進行中アップロードをトーストの Cancel で中止
- [ ] アップロード中のタブを閉じる → 確認ダイアログ → 確定でキャンセル後クローズ
- [ ] `sftp_max_concurrent_uploads` を変更 → リロード → 並行数が変わる
- [ ] sftp バイナリ不在時 → トーストに明確な失敗メッセージ

### セキュリティ（自動検証済み）
- [x] hostname のシェルメタ文字拒否（TS-6）
- [x] remote path の null/危険文字拒否（TS-6）
- [x] local path の危険文字/非存在拒否（TS-6）
- [x] sftp 引数が argv 配列で渡される + `--` 終端マーカー + `-` 始まり host/user 拒否（TS-15）
- [x] 非SSHタブへのペーストはシングルクォートエスケープ + 制御文字パス除外（TS-16）

### 🔒 セキュリティレビュー対応（2026-06-13 / 自動レビュー指摘）
バックグラウンドのセキュリティレビューが検出した MEDIUM 2件を修正:

| # | 指摘 | 対象 | 対応 |
|---|------|------|------|
| 1 | Argument Injection（argv フラグ密輸） | service.rs / args.rs | `validate_connection` で hostname/username の `-` 始まりを拒否。`build_sftp_args` が positional host の前に `--` を挿入（多層防御）。TS-15 追加 |
| 2 | Command Injection via Terminal Paste | remote_path.rs | `format_paths_for_paste` をシングルクォートエスケープに変更（`$()`・backtick・`*` 等を literal 化）。改行/CR/NUL を含むパスは貼り付けず除外（貼り付け時 Enter による実行を遮断）。TS-16 追加 |

いずれも移植元 WebView 実装が持っていた弱い引用を踏襲していた箇所。移植元より強化した形（移植元側は本件のスコープ外なので未変更）。

---

## 🎯 検証サマリー

### ✅ 自動検証結果
- ビルド: ✅ 成功
- テスト: ✅ 1289/1289（sftp 88）
- フォーマット: ✅ 差分なし
- 静的解析: ✅ sftp clippy 0
- ファイル構造: ✅ 16/16
- SPEC 適合: ✅ FR/NFR 実装・TS-1..16 で検証
- セキュリティ: ✅ 自動レビュー指摘2件を修正（TS-15/16）

### 📝 留意事項
- 残りは native-poc 実機での手動確認（上記チェックリスト 8 項目 + 引数渡しレビュー1件）。
- 手動確認が済んだら本ファイルのチェックボックスを更新すること。
- 実 sftp サブプロセス起動を伴う転送はユニットテスト対象外（CI 不可）のため手動で担保。

---

**検証完了時刻**: 2026-06-13 16:51 JST
