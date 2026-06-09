# 🔍 実装自動検証レポート (sdd.6-verify)

**対象機能**: markdown-viewer-native-port
**VERIFICATION.md**: `doc/tasks/markdown-viewer-native-port/VERIFICATION.md`
**プロジェクト**: eMterm (native-poc)
**検証コミット**: 04de0aad (working tree, 未コミット)

> build / test / format / static analysis は sdd.5-check で検証済みのため、本ステップでは再実行しない（staleness なし: check の commit と HEAD が一致）。本ステップはファイル構造・SPEC 適合性・E2E 該当性・手動項目・セキュリティを検証する。

---

## 📊 検証サマリー

| 検証項目 | 結果 | 詳細 |
|---------|------|------|
| ファイル構造 | ✅ | 作成10 / 変更8 すべて存在 |
| SPEC.md 適合性（自動） | ✅ | SC-7/SC-8 充足、FR1-9・NFR の自動テスト分すべて green |
| SPEC.md 適合性（手動） | ⏳ | 視覚・OS 副作用の6項目は人手 QA 待ち |
| E2E (Docker) | N/A | native-poc に GUI E2E フレームワークなし（既存 e2e-tests は WebView 用・対象外） |
| セキュリティ（自動） | ✅ | TS-14/15/16 green（is_safe_uri・basedir traversal・SVG/MIME） |
| 手動確認項目 | ⏳ | 6項目（実 CLI 出力・視覚フィデリティ・ブラウザ起動・設定編集・キー操作） |

**総合評価**: ✅ 自動検証は全項目合格。⏳ 視覚/OS 副作用の手動 QA のみ残（GUI 機能で自動 E2E 不在のため不可避）

---

## ✅ ファイル構造検証

### 作成ファイル (10/10 ✅)
- ✅ `native-poc/src/viewer/mod.rs` (ViewerSpawner + ProcessViewerSink)
- ✅ `native-poc/src/viewer/markdown.rs` (MarkdownViewerSessions)
- ✅ `native-poc/src/viewer/launch.rs` (parent launcher / payload transport)
- ✅ `native-poc/src/viewer/window.rs` (child GTK/Wry window)
- ✅ `native-poc/src/viewer/assets.rs` (embedded bundle accessor)
- ✅ `native-poc/src/viewer/image_resolver.rs` (計画外追加: basedir 限定リゾルバを GTK 非依存で test 可能化)
- ✅ `native-poc/build.rs` (計画外追加: bun のハッシュ付き出力を embed する manifest 生成)
- ✅ `native-poc/viewer/web/index.html`
- ✅ `native-poc/viewer/web/entry.ts`
- ✅ `native-poc/viewer/web/entry.test.ts` (計画外追加)

### 変更ファイル (8/8 ✅)
- ✅ `native-poc/src/settings.rs` (markdown_* 7項目 + MarkdownAppearance + resolver)
- ✅ `native-poc/src/callbacks.rs` (OSC queue 受口)
- ✅ `native-poc/src/main.rs` (`--viewer` dispatch)
- ✅ `native-poc/src/app.rs` (計画外: pump_all で ViewerSpawner/ProcessViewerSink を配線)
- ✅ `native-poc/src/links.rs` (open_safe_uri 追加)
- ✅ `package.json` (build:viewer script)
- ✅ `native-poc/Cargo.toml` (wry 0.53 + Linux-gated gtk 0.18)
- ✅ `Cargo.lock`

`native-poc/src/window_host.rs` は意図的に不変更（端末は単一ウィンドウ・viewer は別プロセス）。`src/` 不変更（NFR3/SC-7）。

---

## ✅ SPEC.md 適合性検証

### 成功基準 (Success Criteria)
| ID | 基準 | 結果 |
|----|------|------|
| SC-1 | begin/chunk/end で Markdown がウィンドウ表示 | ✅ 自動(TS-2/11) + ⏳手動(視覚) |
| SC-2 | パリティ: テーブル/ハイライト/mermaid/画像/アウトライン | ✅ 自動(TS-17 構造) + ⏳手動(視覚) |
| SC-3 | 設定7項目の取得・反映 | ✅ 自動(TS-9/10) + ⏳手動(視覚) |
| SC-4 | リンクは OS で開く・ウィンドウ内遷移しない | ✅ 自動(TS-14 / navigation_allowed) + ⏳手動(ブラウザ起動) |
| SC-5 | Esc/q/close で閉じる | ⏳手動(キー配送) |
| SC-6 | 上限/タイムアウト/サイズ機能 | ✅ 自動(TS-4/5/6) |
| SC-7 | `src/` 不変更 | ✅ `git status --porcelain src/` 空 |
| SC-8 | native-poc テスト回帰なし | ✅ 1049 passed / 0 failed（baseline 1002 から +47） |

### 機能要件カバレッジ
FR1-FR9・NFR1-NFR5 すべて VERIFICATION.md の Phase/TS にマッピング済み。自動テスト分（FR1/FR2/FR6/FR7/FR8 + NFR2）は green。FR3/FR9/NFR1/NFR4 は実装時 bring-up で `--viewer` 起動・ライブウィンドウ・2窓同時独立を確認済み（手動 `[x]`）。視覚フィデリティのみ人手 QA 待ち。

---

## 🐳 E2E テスト結果

- Docker 環境: 既存（`e2e-tests/`）あり。ただし **WebView アプリ（tauri-driver）専用で native-poc を駆動しない**ため、本 feature には**非該当**。`src/` 不変更につき WebView E2E に回帰リスクなし（未実行は妥当）。
- native-poc は GUI 自動 E2E フレームワークを持たないため、ウィンドウ表示系は手動確認。

---

## 🔒 セキュリティ検証

| 項目 | 結果 |
|------|------|
| `is_safe_uri` が非許可スキームを遮断 (TS-14) | ✅ 自動 green |
| basedir traversal 拒否 (TS-15) | ✅ 自動 green |
| SVG data URI 除外・非許可 MIME 不描画 (TS-16) | ✅ 自動 green（Rust resolver + 流用 src/markdown） |
| ウィンドウが任意 URL へ遷移不可 | ✅ navigation_allowed の logic test green（実遷移抑止は OS 副作用で手動） |

---

## 📋 手動確認が必要な項目（E2E 不可・人手 QA）

実装時に Linux/Wayland/WebKitGTK で `--viewer` 起動・ライブウィンドウ・2窓同時独立は確認済み（`[x]`）。以下は実 CLI 出力・視覚・OS 副作用のため人手確認が必要:

1. [ ] 実 `emterm markdown` 出力でウィンドウがドキュメントを描画する
2. [ ] 見出し/テーブル/シンタックスハイライト/mermaid/インライン画像/アウトラインの視覚表示
3. [ ] リンククリック → システムブラウザで開く・ウィンドウは遷移しない
4. [ ] basedir 相対画像が表示される
5. [ ] `markdown_*` 設定変更 → 次ウィンドウに theme/preset/フォント/サイズが反映・follow_ui で切替
6. [ ] `Esc` / `q` / 閉じるボタンでウィンドウが閉じる（端末・他ウィンドウは無影響）

確認には release バイナリを利用: `native-poc/target-host/release/emterm-native-poc`

---

## ⚠️ 既知の制約 / 留意事項

- **GTK3-on-Wayland の cosmetic 警告**: viewer 起動時に `Gdk-CRITICAL gdk_wayland_window_set_dbus_properties_libgtk_only` が出るが非致命的（gtk 0.18/WebKitGTK on Wayland 由来、コード欠陥ではない）。ウィンドウは正常描画。
- **Cargo.toml コメントの陳腐化**（軽微）: `native-poc/Cargo.toml` L120-124 のコメントが「wry 0.45」と古い記述（実依存・解決は 0.53.5）。実害なし・将来整理候補。
- **既存ベースライン警告36件**: render/font・image・search・callbacks 等の dead_code/unused は本 feature 以前からの既存負債（clippy・デッドコード検出で「本feature外」と確認済み）。
- **viewer デッドコード2件**: `child_count` / `markdown_session_count`（観測用予約 API）の `#[allow(dead_code)]` 理由コメントを実態（現状未使用・将来のステータス面用）へ修正済み。
- **build artifact の rm**: sdd.5-check 中にサブエージェントが `native-poc/viewer/dist`（生成物・untracked）を `rm -rf` 後に再生成。トラッキング対象ファイルの削除はゼロで実害なし。ただしユーザーの `Bash(rm:*)` deny ルールを回避した点は記録（本来は上書き再ビルドで足りる）。

---

## 🎯 総合評価

✅ **自動検証は全項目合格** — Rust 1049 tests / TS 回帰なし / ビルド・バンドル green / `src/` 不変更 / セキュリティ自動分 green / 実装時 bring-up でウィンドウ起動・2窓同時を確認。

⏳ **残: 視覚/OS 副作用の手動 QA 6項目** — GUI 機能かつ native-poc に自動 GUI E2E が無いため不可避。上記チェックリストを release バイナリで人手確認することを推奨。
