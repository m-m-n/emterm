# Verification Result: mux Snapshot Main-Buffer Screen Omission

**Date**: 2026-06-28
**Feature**: mux-snapshot-main-buffer-screen-omit
**SPEC.md**: `doc/tasks/mux-snapshot-main-buffer-screen-omit/SPEC.md`
**VERIFICATION.md**: `doc/tasks/mux-snapshot-main-buffer-screen-omit/VERIFICATION.md`
**Project**: eMterm (Rust desktop terminal emulator)

---

## 📊 Summary

| 検証項目 | 結果 | 詳細 |
|---------|------|------|
| ビルド (default features) | ✅ | sdd.5-check で PASS (0.48s, 警告なし) |
| ビルド (CLI-only) | ✅ | sdd.5-check で PASS (0.34s, 警告なし) |
| テスト (src-tauri --lib) | ✅* | sdd.5-check で 2015 passed / 2 baseline-flake / 3 ignored |
| テスト (term_core --lib) | ✅ | sdd.5-check で 685 passed / 0 failed / 7 ignored |
| フォーマット | ✅ | sdd.5-check で 5 ファイル全て diff なし |
| 静的解析 (dead code) | ✅ | sdd.5-check で orphan 0件・compiler warning 0件 |
| ファイル構造 | ✅ | 修正対象 5 ファイル全て存在 |
| SPEC.md 適合性 | ✅ | SC-1〜SC-5 の自動判定可能項目すべて達成 |
| E2E | n/a | プロジェクトに E2E framework 無し |

\* baseline-flake は実装変更とは無関係 (MEMORY `project_test_execution_notes` に既知記録)

**総合評価**: ✅ 全 automated 検証項目クリア

---

## ✅ Automated Verification

### Build / Test / Format / Static Analysis

sdd.5-check で実行済みのため再実行はスキップ (skill 仕様)。 sdd.5 完了 commit と現在 HEAD が同一 (`1e34f0002d464ba03fe5372e2035524f131de82e`) のため staleness なし。

詳細は `VERIFICATION.md` "Actual Test Results / Actual Format Result" セクション参照。

### File Structure Verification

#### Files to Create
- 該当なし (新規ファイルなし、 計画通り)

#### Files to Modify
- ✅ `src-tauri/src/mux/ipc/reattach.rs`
- ✅ `src-tauri/src/mux/ipc/handlers.rs` (plan deviation: 既存 layout-dependent test 2件を alt-screen mode 駆動に refactor)
- ✅ `src-tauri/src/tabs.rs`
- ✅ `crates/term_core/src/terminal_core.rs`
- ✅ `crates/term_core/src/reflow.rs`

### SPEC.md Compliance

| SC ID | 内容 | 検証方法 | 結果 |
|---|---|---|---|
| SC-1 | FR1 / FR2 implemented and covered by unit tests | `build_snapshot_bytes` の `alt_screen` branch 確認 + 該当テスト pass (TS-1, TS-2, TS-3) | ✅ |
| SC-2 | FR3 doc comments reflect the main/alt split | `build_snapshot_bytes` / `build_shadow_parser_snapshot` / `handle_request_pane_snapshot` / `SNAPSHOT_CLEAR_HOME` の doc 検査 | ✅ |
| SC-3 | FR4 investigation-code removal complete | `grep -R "\[DECSTBM-trace\]" src-tauri/ crates/` → 0 hit; `grep -nE "fn probe_"` → 0 hit | ✅ |
| SC-4 | `--lib` test suites pass with the documented `CARGO_TARGET_DIR` | sdd.5-check で確認済 | ✅ |
| SC-5 | Manual verification scenarios 1–5 pass | 後述 Manual 項目で要ユーザー確認 | ⏳ Manual |

### Functional Requirements Coverage

| Requirement | Verification 結果 |
|---|---|
| FR1 (Main-buffer snapshot omits screen dump) | ✅ TS-1 PASS (`build_snapshot_bytes_main_buffer_omits_screen_part`) |
| FR2 (Alt-screen snapshot keeps screen dump) | ✅ TS-2 PASS (`build_snapshot_bytes_layout_is_clear_scrollback_screen` alt branch) |
| FR3 (Doc comments reflect main/alt split) | ✅ 4箇所 (SNAPSHOT_CLEAR_HOME, build_snapshot_bytes, build_shadow_parser_snapshot, handle_request_pane_snapshot) で doc 更新確認 |
| FR4 (Remove investigation code) | ✅ TS-7 PASS (grep 0 hit) |
| NFR1 (Performance — byte size strictly decreases) | ✅ TS-1 が `screen_to_include = &[]` 経路を実証 |
| NFR2 (IPC wire protocol unchanged) | ✅ `build_snapshot_bytes` 関数 signature 不変・ `mux_throughput.rs` テスト含む `mux::ipc` 84件 PASS |
| NFR3 (Single doc comment captures snapshot layout split) | ✅ `build_snapshot_bytes` の doc に main/alt split が明記 |

---

## 🐳 E2E Tests

該当なし。 プロジェクトに E2E framework は存在しない (`test/README.md` に明記、 `docker-compose.e2e.yml` も `e2e-tests/` も無い)。 E2E は手動シナリオでカバー。

---

## 📋 Manual Testing (E2E Not Possible)

VERIFICATION.md の Manual Testing セクションから以下 4 項目を抽出。 sdd.4-implement のレポートによると **release build (`src-tauri/target-host/release/emterm`) の再ビルドが必要** (sdd.4 では `cargo check` + `cargo test --lib` のみ実行)。 ユーザーが実機で確認すること:

- [ ] **M1 — apt 同一タブクリック**: `sudo apt reinstall <package>` を emterm の mux タブで実行。 apt 動作中および直後に同じタブをクリックし、 progress bar と log line が同一行に collapse しないこと。
- [ ] **M2 — apt 別タブ往復**: apt 動作中・直後に別タブへ切替→戻る。 progress bar と log line の row collapse なきこと。
- [ ] **M3 — alt-screen TUI 往復**: vim / htop / less / man のいずれかを起動した状態で別タブ往復。 alt-screen 内容が崩れなく復元されること。
- [ ] **M4 — ログ確認**: `~/.local/share/net.laser5.app.emterm/logs/emterm.log` を確認し、 `[DECSTBM-trace]` 行が一切ないこと。

### Manual 検証前の準備

`make build` または以下で release バイナリを再ビルド:

```
CARGO_TARGET_DIR=src-tauri/target-host cargo build --release --manifest-path src-tauri/Cargo.toml
```

実行:

```
src-tauri/target-host/release/emterm
```

---

## 🔒 Security Verification

N/A — 本変更は snapshot byte 列の組み立て分岐のみで、 新規 attack surface なし。

## ⚡ Performance Verification

NFR1 で main-buffer snapshot payload size の減少を要件としている。 別途 benchmark は不要 (TS-1 にて `screen_to_include = &[]` 経路を直接 assert)。 実装変更により vt100 dump 分のバイト数が削減されることが論理的に保証される。

---

## 🎯 Conclusion

✅ **Automated 検証はすべて PASS** (sdd.5-check 経由 + 本 sdd.6-verify で確認)
✅ **SPEC.md FR/NFR は SC-1〜SC-4 まで自動検証完了**
⏳ **SC-5 (Manual) は M1〜M4 を実機で確認すること**

### 既知事項 (本タスク無関係)

- `tabs::tests::welcome_without_windows_leaves_group_none` — baseline でも fail する pre-existing 既知の挙動
- `tabs::tests::ts7_offthread_swap_then_restored_scrollback_matches_reference` / `ts13_offthread_swap_installs_pending_scrollback_restore` — 並列実行時の flake、 `--test-threads=1` で再実行すると pass (MEMORY 記録済み)
- `status_bar::runtime::tests::runtime_time_provider_timer_fires_wake` — timer 負荷依存 flake

### Out-of-scope (別タスク)

- daemon vt100 が resize race で `contents_formatted()` を trash する根本原因の特定 (本タスクでは main-buffer snapshot から除外することで user に到達しないようにしただけ)
- scrollback ring 2MiB 上限を超える長期 session の復元戦略
