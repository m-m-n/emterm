# 🔍 実装自動検証レポート: window-maximize-dock-grouping

**対象機能**: ウィンドウの最大化起動とドックグルーピング統一
**VERIFICATION.md**: `doc/tasks/window-maximize-dock-grouping/VERIFICATION.md`
**プロジェクト**: eMterm (Rust native terminal)
**ブランチ**: refactor/promote-native-poc

---

## 📊 検証サマリー

| 検証項目 | 結果 | 詳細 |
|---------|------|------|
| ビルド (default) | ✅ | `cargo build` Finished（33.37s、エラー0、新規警告0） |
| ビルド (CLI-only) | ✅ | `cargo check --no-default-features` Finished、エラー0 |
| テスト実行 | ✅ | `cargo test --lib`（single-thread）1878 passed / 0 failed / 1 ignored（既存無関係） |
| コードフォーマット | ✅ | PostToolUse hook 管理（crate 全体 fmt は未実行＝方針どおり） |
| 静的解析 / デッドコード | ✅ | 未使用関数・変数・import・到達不能コードなし（5項目すべて結線確認） |
| ファイル構造 | ✅ | 計画どおり9ソースファイル変更、無関係ファイルなし |
| SPEC.md 適合性 | ✅ | FR1〜FR5・NFR1〜NFR4 すべてコード上で確認 |

> ビルド/テスト/フォーマット/静的解析は sdd.5-check で実測済み。本レポートはその結果を引用し、ファイル構造と SPEC 適合性を追加検証している。

**総合評価**: ✅ 自動検証はすべて合格（WM レベル挙動は下記の手動検証項目に委ねる）

---

## ✅ 自動検証項目

### ファイル構造検証（9/9）
`git status` で変更ファイルが計画どおりであることを確認（作成ファイルなし、既存9ファイルの変更のみ）:

```
 M src-tauri/src/lib.rs
 M src-tauri/src/settings_window/mod.rs
 M src-tauri/src/viewer/data_window.rs
 M src-tauri/src/viewer/image_window.rs
 M src-tauri/src/viewer/window.rs
 M src-tauri/src/webview_host/linux.rs
 M src-tauri/src/webview_host/mod.rs
 M src-tauri/src/webview_host/windows.rs
 M src-tauri/src/window_host.rs
```

### SPEC.md 適合性検証（FR1〜FR5・NFR1〜NFR4）

| 要件 | 実装上の根拠 | 結果 |
|------|------------|------|
| FR1 設定ウィンドウ最大化 | `webview_host/mod.rs:90` `maximized: bool` + `settings_window/mod.rs:125` `maximized: true`（initial_size 1080×760 を復元サイズとして保持） | ✅ |
| FR2 Markdown ビューア最大化 | `viewer/window.rs:33` `MAXIMIZED=true` + `:89` `maximized: MAXIMIZED`（960×720 復元） | ✅ |
| FR3 データビューア最大化 | `viewer/data_window.rs:357` `.with_maximized(true)`（960×640 復元） | ✅ |
| FR4 Image は最大化対象外 | `viewer/image_window.rs:428` は `with_app_id` のみ、`with_maximized` 無し（画像フィット維持） | ✅ |
| FR5 ドックグルーピング統一 | `lib.rs:24` `APP_WM_ID="emterm"`、winit 側 `with_app_id`（window_host:337 / data_window:363 / image_window:428）、GTK 側 `linux.rs:29-30` `set_prgname`/`set_program_class` | ✅ |
| NFR1 固定挙動（設定項目なし） | settings.json への toggle 追加なし（maximize はコードで固定 true） | ✅ |
| NFR2 プラットフォームスコープ | maximize はクロスプラットフォーム（`windows.rs:78` `.with_maximized(host.maximized)`）／グルーピングは Linux 限定（`#[cfg(target_os="linux")]` の `linux_wm`） | ✅ |
| NFR3 復元サイズ保持 | 各 initial_size を変更せず最大化を追加（コメントで復元サイズと明記） | ✅ |
| NFR4 識別子の単一ソース | `APP_WM_ID` 1定数を全生成箇所から参照 | ✅ |

### デッドコード / 未使用検査
- `WebViewHost.maximized` は Linux（`linux.rs:38`）・Windows（`windows.rs:78`）両方で消費。
- `APP_WM_ID` / `linux_wm::with_app_id` は要求どおり3 winit ウィンドウ＋GTK 側から参照、デッドなし。
- `settings_window::build_host()` は `run()` から呼ばれ、旧インライン構築の残骸なし。
- 新規追加 import に未使用なし。

### 単体テスト（決定可能な事実のみ、3件すべて合格）
| ID | テスト | 対象 |
|----|--------|------|
| TS-1 | `settings_window::tests::settings_host_opens_maximized_with_restore_size` | FR1（`maximized==true` かつ initial 1080×760） |
| TS-1 | `viewer::window::tests::markdown_viewer_opens_maximized` | FR2（`MAXIMIZED==true`） |
| TS-4 | `lib::tests::app_wm_id_is_emterm` | FR5/NFR4（`APP_WM_ID=="emterm"`、CLI-only でも合格） |

---

## 🐳 E2Eテスト結果

- Docker E2E 環境: **対象外**。本変更はウィンドウ属性（最大化・WM_CLASS/app_id）のみで、WM レベルの挙動は WebDriver/Docker E2E では検証不能。プロジェクトに該当 E2E スイートも無い。

---

## 📋 手動確認が必要な項目（E2E不可）

DevTools は使用不可。WM レベルの状態は目視と `emterm.log` で確認する。

> グルーピング注記: GNOME/Ubuntu は X11 `WM_CLASS` / Wayland `app_id` とインストール済み `*.desktop` の一致でドックアイコンを関連付ける。TS-7/TS-8 はインストール済み deb（`emterm.desktop` 存在）での検証が最も確実。`make dev`/`cargo run` では識別子による grouping はされても アイコン/グルーピングが部分的になりうる。

- [ ] **TS-5**: 設定を開く → 最大化。`emterm markdown <file>` → 最大化。`emterm json/yaml <file>` → 最大化。
- [ ] **TS-6**: 各ウィンドウの最大化解除 → 直前のサイズへ復元（~1080×760 / ~960×720 / ~960×640）。
- [ ] **TS-3**: `emterm image <小さい画像>` → 画像サイズで表示、**最大化されない**。
- [ ] **TS-7**（X11 セッション、deb 推奨）: 全ウィンドウ種を開く → Ubuntu ドックに `emterm` アイコンが1つだけ、すべてグルーピング。
- [ ] **TS-8**（Wayland セッション、deb 推奨）: TS-7 と同様＋正しいアプリアイコン表示（app_id が `emterm.desktop` と一致）。

---

## 🎯 検証サマリー

### ✅ 自動検証結果
- ビルド（default / CLI-only）: 成功
- テスト: 1878 passed / 0 failed / 1 ignored（追加3件すべて合格）
- ファイル構造: 9/9 計画どおり
- SPEC 適合性: FR1〜FR5・NFR1〜NFR4 すべて充足
- デッドコード: なし

### 📝 留意事項
- WM レベルの実挙動（実際の最大化・復元サイズ・単一ドックアイコン）は自動テスト不能のため、上記 TS-3/TS-5〜TS-8 の手動検証を実施すること。
- ドックグルーピングの最終確認はインストール済み deb 環境を推奨。
- リリースビルド（target-host）は方針どおり未実行。ユーザーがリリース確認する際に別途ビルドが必要。
