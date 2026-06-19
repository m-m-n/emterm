# 🔍 実装自動検証レポート (sdd.6-verify)

**対象機能**: mux-transport-parser-isolation
**VERIFICATION.md**: `doc/tasks/mux-transport-parser-isolation/VERIFICATION.md`
**プロジェクト**: eMterm
**検証コミット**: f6432f483c66ef7968744e58bd0d99f872813e71（ワーキングツリー）

---

## 📊 検証サマリー

| 検証項目 | 結果 | 詳細 |
|---------|------|------|
| ビルド | ✅ | sdd.5 で検証済（default + CLI-only、exit 0） |
| テスト実行 | ✅ | sdd.5 で検証済（lib 1847 passed / term_core 658 passed） |
| コードフォーマット | ✅ | sdd.5 で検証済（変更ファイル rustfmt --check 差分なし） |
| 静的解析 | ✅ | sdd.5 で検証済（clippy 変更ファイルに新規警告なし） |
| ファイル構造 | ✅ | 作成1 / 変更4 すべて存在 |
| SPEC.md 適合性 | ✅ | FR1-7・NFR1-4 すべて充足（NFR3 はスコープ外） |
| デッドコード | ✅ | sdd.5 で検証済（新規 API 全参照あり・DIAG/parser_mid_sequence 全消去） |

**総合評価**: ✅ すべての自動検証項目をクリア（手動 GUI 確認 M1-M6 のみ残）

> ビルド/テスト/フォーマット/静的解析は sdd.5-check で検証済みのため再実行しない（staleness 無し: check の commit と現 HEAD が一致）。

---

## ✅ ファイル構造検証

### 作成ファイル (1)
- ✅ `crates/term_core/src/mux_apc_extractor.rs` — 独立トランスポート抽出器（公開 API）

### 変更ファイル (4)
- ✅ `crates/term_core/src/lib.rs` — `pub mod mux_apc_extractor;` + `pub use ...MuxApcExtractor`（lib.rs:34, :59）
- ✅ `crates/term_core/src/terminal_core.rs` — `parser_mid_sequence()` アクセサ削除
- ✅ `src-tauri/src/tabs.rs` — `Tab.mux_apc_extractor` フィールド（:282 / init :394）、`pump`→`process_combined` 分岐、detach reset（:1357）、DIAG 撤去
- ✅ `src-tauri/src/mux/apc.rs` — `try_decode_emterm_mux` 失敗ログをシンプル warn に復元

---

## ✅ SPEC.md 適合性検証（FR/NFR）

| 要件 | 充足 | 根拠 |
|------|------|------|
| FR1 専用トランスポート抽出器 | ✅ | `mux_apc_extractor.feed(&combined)`（tabs.rs:1491）が独立 `Parser` で APC/OSC 抽出。TS-1/2/3 PASS |
| FR2 self.core 内側コンテンツ専用化 | ✅ | mux 確立後は extractor が外側を担い、`self.core` は `apply_mux_message` の PtyOutput アームのみが駆動。TS-4 PASS |
| FR3 APC + OSC 9999 フォールバック | ✅ | 抽出器が OSC 9999 `emterm-mux;` を APC 相当に正規化（`handle_osc_internal` とパリティ）。TS-3 PASS |
| FR4 mux 確立前は self.core | ✅ | `mux_session_name.is_none()` 分岐で従来経路（tabs.rs:1494-1529）。TS-5 PASS |
| FR5 detach 時に self.core へ復帰 | ✅ | detach で `mux_apc_extractor.reset()`（tabs.rs:1357）。TS-6 PASS |
| FR6 Welcome 二重送信耐性 | ✅ | `first_welcome` ガード + extractor 状態整合。TS-7 PASS |
| FR7 DIAG ログ撤去 | ✅ | `grep DIAG` / `grep parser_mid_sequence` 共にマッチ無し（"DIAGONAL" の Unicode コメントを除く）。TS-8 PASS |
| NFR1 非mux 回帰なし | ✅ | TS-9 PASS（非mux Kitty デコード） |
| NFR2 daemon/mux_ipc/bridge 不変 | ✅ | `git status` で daemon/bridge/mux_ipc に変更なし。TS-10 PASS |
| NFR3 WebView 対象外 | ✅ | `src/` に変更なし（スコープ外） |
| NFR4 pump coalesce/予算保持 | ✅ | `pump` の coalesce ループ・`FRAME_BUDGET_MS`/`COALESCE_CAP` は不変。本体を `process_combined` に分離したのみ |

### Success Criteria
| ID | 基準 | 状態 |
|----|------|------|
| SC-1 | mux インライン Kitty 画像・base64 漏れ無し | ✅ TS-4（自動）/ M1（手動 GUI 残） |
| SC-2 | 大画像のチャンク境界またぎ組み上げ | ⏳ M2（手動 GUI 残） |
| SC-3 | 非mux 経路に影響なし | ✅ TS-9 / M4（手動 GUI 残） |
| SC-4 | mux で SIXEL 表示 | ⏳ M5（手動 GUI 残） |
| SC-5 | mux で Markdown/テキスト/TUI パリティ | ⏳ M6（手動 GUI 残） |
| SC-6 | DIAG 撤去 | ✅ TS-8 |
| SC-7 | 分割チャンク回帰テスト追加・合格 | ✅ TS-4 |

---

## 🐳 E2Eテスト結果

- Docker / E2E 環境: **未構築**（このフィーチャーパスに E2E フレームワーク無し）
- 判定: スコープ外。パーサーレベルの修正のため、回帰検出は Rust ユニット/結合テスト（TS-1〜TS-9）でカバー済み。

---

## 📋 手動確認が必要な項目（E2E不可・GUI 実行が必要）

実機（GUI）でのみ確認可能な項目。実装・自動テストは完了しているが、最終的な目視確認として以下を実施することを推奨:

- [ ] **M1**: mux タブで `emterm image <file>` → インライン画像表示・base64 漏れ無し
- [ ] **M2**: 大きい画像（数MB）がチャンク境界をまたいで正しく組み上がる
- [ ] **M3**: `emterm.log` に `Kitty image decode failed` / `mux APC decode failed` が出ない
- [ ] **M4**: 非mux タブでも従来どおり画像表示
- [ ] **M5**: SIXEL（`emterm image --protocol sixel`）が mux で表示
- [ ] **M6**: Markdown ビューア・通常テキスト・TUI（vim 等）が mux で従来どおり動く

---

## 🎯 総合評価

✅ **自動検証はすべて合格**。FR1-7・NFR1-4 を充足し、回帰防止の分割チャンク結合テスト（TS-4）を含む全テストが PASS。NFR2（プロトコル不変）・DIAG 撤去も確認済み。

残るは GUI 実機での目視確認（M1-M6）のみ。これらは実装の正しさの最終確認であり、コード・自動テスト上は完了している。
