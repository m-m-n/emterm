# 🔍 実装自動検証レポート: binary-mismatch-restart-toast

**検証日時**: 2026-06-20 16:49:30 JST (+09:00)
**対象機能**: binary-mismatch-restart-toast
**VERIFICATION.md**: `doc/tasks/binary-mismatch-restart-toast/VERIFICATION.md`
**プロジェクト**: eMterm (native Rust + child WebView)

---

## 📊 検証サマリー

| 検証項目 | 結果 | 詳細 |
|---------|------|------|
| ビルド (default) | ✅ | sdd.5 で検証済み (cargo check PASS) |
| ビルド (CLI-only) | ✅ | sdd.5 で検証済み (--no-default-features PASS) |
| テスト実行 | ✅ | sdd.5 で検証済み (1871 passed / 0 failed / 1 ignored) |
| コードフォーマット | ✅ | sdd.5 で検証済み (cargo fmt --check PASS) |
| 静的解析 | ✅ | sdd.5 で検証済み (警告なし) / デッドコードなし |
| ファイル構造 | ✅ | 作成1 + 変更8、全て存在 (9/9) |
| SPEC.md 適合性 | ✅ | FR1–6 / NFR1–5 すべて実装・配線確認 |

**総合評価: ✅ すべての自動検証項目をクリア（手動項目 M-1..M-3 のみ残）**

> 注: ビルド/テスト/フォーマット/静的解析は sdd.5-check で実施済みのため再実行していない。`check` の `completed_at_commit` と現在 HEAD が一致（コード変更なし）で staleness なし。

---

## ✅ ファイル構造検証 (9/9)

### 作成ファイル (1)
- ✅ `src-tauri/src/self_exec.rs`

### 変更ファイル (8)
- ✅ `src-tauri/src/lib.rs` — `self_exec` を gui gate 下で宣言 (line 68)
- ✅ `src-tauri/src/settings_launcher.rs` — `spawn_self` 経由 (line 74)
- ✅ `src-tauri/src/viewer/mod.rs` — `spawn_self` 経由 (line 254)
- ✅ `src-tauri/src/viewer/image.rs` — `self_exe_path` + `note_spawn_failure` (line 159, 185)
- ✅ `src-tauri/src/mux/daemon.rs` — `self_exe_path` + `note_spawn_failure` (line 154, 210)
- ✅ `src-tauri/src/app.rs` — `RestartToast` 状態 + frame pump (line 395, 648, 1995–2001)
- ✅ `src-tauri/src/render/mod.rs` — トースト描画 + ja/en (line 550–559)
- ✅ `src-tauri/src/main.rs` — startup で `self_exec::init()` (line 166)

---

## ✅ SPEC.md 適合性検証

| 要件 | 実装箇所 | 検証 | 結果 |
|------|----------|------|------|
| FR1 起動時 baseline | main.rs:166 `self_exec::init()` / self_exec.rs | TS-4 | ✅ |
| FR2 inode 検出 | self_exec.rs `is_missing`/`self_binary_missing` | TS-1,2,3,4 | ✅ |
| FR3 4 サイト + シグナル | settings/viewer/image/daemon → self_exec | TS-8,9 + 配線 grep | ✅ |
| FR4 トースト描画 | render/mod.rs:550–559 | M-1 (手動) | ✅ 実装確認 |
| FR5 4秒自動消滅 | app.rs:1995–2001 + RestartToast | TS-5,6,7 | ✅ |
| FR6 i18n ja/en | render/mod.rs:558–559 `t()` | M-1 (手動) | ✅ 実装確認 |
| NFR1 reactive perf | spawn 失敗時のみ検出 (per-site Err 経路) | コードレビュー | ✅ |
| NFR2 terminal 不阻害 | 各サイト既存 Err 処理を維持 | M-2 (手動) | ✅ 実装確認 |
| NFR3 Linux のみ no-op | self_exec.rs unix gate / 非 unix false | TS-8,9 | ✅ |
| NFR4 gui gate / CLI build | lib.rs gui gate | TS-8 | ✅ |
| NFR5 testability | 純粋関数 `is_missing` / RestartToast | TS-1..7 | ✅ |

### 設計の肝 (verify-plan の High 修正) — 確認済み
- ✅ **spawn は `current_exe()` フレッシュ解決**（self_exec.rs:124）。baseline path は spawn に使わない → バイナリ置換後も spawn は失敗し、案A reactive が発火する。
- ✅ **検出は baseline path の dev/inode 比較**（self_exec.rs `metadata_dev_ino`）。spawn とは別の実行ファイル参照を意図的に使い分け。
- ✅ off-thread (image worker) シグナルは `note_spawn_failure` が `crate::wakeup::wake()` でイベントループを起こす。

---

## 🐳 E2E テスト結果

- Docker 環境: **未構築**（このネイティブ Rust バイナリ用の E2E フレームワークはプロジェクトに無い）
- E2E テスト: 対象外（SPEC.md「E2E Tests: None」）。ユニット + 手動で担保。

---

## 📋 手動確認が必要な項目（E2E 不可）

VERIFICATION.md から3項目を抽出。Linux 実機での確認が必要（DevTools 不可・リリースビルドはユーザー実行）。

- [ ] **M-1**: Linux でリリースバイナリを起動 → ディスク上のバイナリを差し替え（パッケージ更新 or `install`/`cp` でパスへ上書き）→ 設定画面（およびビューア/mux）を開く。右上にアクティブ言語のトーストが出て約4秒で自動消滅。連打しても1枚に保たれる。
- [ ] **M-2**: トースト表示前後で通常のターミナル描画・キー入力に影響がない。
- [ ] **M-3**: バイナリ未変更時は、設定/ビューア/mux の通常利用でトーストが出ない。

> 既知の制限（reactive 設計上許容）: image viewer worker は exe を起動時キャッシュするため、置換前に worker 起動済みだとその経路はトーストを出さない。per-use の settings/viewer/daemon は発火する。

---

## 🎯 総合評価

✅ **自動検証はすべて合格**。FR1–6 / NFR1–5 すべて実装・配線を確認。残るは Linux 実機での手動確認 M-1..M-3 のみ。

### 次のアクション
- 手動テスト M-1..M-3 を Linux 実機で実施（リリースビルドはユーザーの明示指示で）。
- 完了後、本ファイルのチェックボックスを更新。
