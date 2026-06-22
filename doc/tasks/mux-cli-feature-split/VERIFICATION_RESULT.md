# Verification Result: Mux in CLI Build (revised)

**検証日時**: 2026-06-23
**対象機能**: mux-cli-feature-split (revised — `mux` feature 廃止 + CLI ビルドへ mux 同梱)
**VERIFICATION.md**: `doc/tasks/mux-cli-feature-split/VERIFICATION.md`

## サマリ

| カテゴリ | 自動 TS | PASS | FAIL | Manual |
|--|--|--|--|--|
| ビルド (cargo check) | TS-1, TS-2 | 2 | 0 | — |
| テスト (cargo test) | TS-3, TS-4 | 2 | 0 | — |
| 静的 grep | TS-5, TS-6, TS-7, TS-8, TS-9, TS-10 | 6 | 0 | — |
| 実機 (release / deb / SSH) | M1–M4 | — | — | 4 |

総合: 自動・静的検証 10 / 10 PASS。Manual 4 項目は未実施 (ユーザー実機作業)。

## 詳細

### TS-1: GUI `cargo check`
- 結果: PASS (exit 0、warnings 0)

### TS-2: CLI `cargo check --no-default-features`
- 結果: PASS (exit 0、warnings 0)

### TS-3: GUI `cargo test --lib --test-threads=1`
- 結果: PASS (1911 passed / 0 failed / 3 ignored)

### TS-4: CLI `cargo test --no-default-features --lib --test-threads=1`
- 結果: PASS (501 passed / 0 failed / 2 ignored)

### TS-5: `Cargo.toml` から `mux` feature 削除
- 結果: PASS (`grep -nE '^mux = \[|"mux"' src-tauri/Cargo.toml` で 0 件)

### TS-6: `lib.rs` から `cfg(feature = "mux")` 削除
- 結果: PASS (該当 grep 0 件)

### TS-7: `main.rs` から mux feature gate 削除
- 結果: PASS (該当 grep 0 件)

### TS-8: `scripts/build-dpkg.sh` から MUX_ONLY / HEADLESS / emterm-mux 削除
- 結果: PASS (該当 grep 0 件、`bash -n` syntax OK)

### TS-9: `Makefile` から `mux-build` / `mux-dpkg` target 削除
- 結果: PASS (該当 grep 0 件)

### TS-10: `tmux_import` の `feature = "gui"` gate 維持
- 結果: PASS
  - `src-tauri/src/mux/mod.rs:39` `#[cfg(feature = "gui")]`
  - `src-tauri/src/mux/cli.rs:21,265` `#[cfg(feature = "gui")]`

### M1–M4: Manual verification
- 実施: 未実施
- 理由: 実機 release build / deb 生成 / SSH ホスト install が必要で、ユーザー指示なしには走らせない方針
- ユーザー側で次の手順を実機確認: `make build` → `emterm`、`make cli-build` → `./src-tauri/target-host/release/emterm mux --daemon`、`make cli-dpkg` → `dpkg-deb --info build/emterm-cli_<ver>_<arch>.deb` で `Depends: libc6` のみ表示、SSH ホストで install + 起動 + attach

## 結論

revised 仕様の自動・静的検証は完全 PASS。`mux` cargo feature は撤去
され、CLI ビルド (`emterm-cli` deb) が mux を含むようになった。
`make build` / `make cli-build` / `make dpkg` / `make cli-dpkg` の
シグネチャは prior コミット以前の状態に戻っている。
