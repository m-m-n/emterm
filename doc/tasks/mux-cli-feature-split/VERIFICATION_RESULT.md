# Verification Result: Mux CLI Feature Split

**検証日時**: 2026-06-23
**対象機能**: mux-cli-feature-split
**VERIFICATION.md**: `doc/tasks/mux-cli-feature-split/VERIFICATION.md`
**SPEC.md**: `doc/tasks/mux-cli-feature-split/SPEC.md`
**実行フェーズ**: sdd.6-verify

このレポートは、VERIFICATION.md の検証項目を sdd.5-check 完了後に再点検した結果。`cargo check` / `cargo test` の自動再実行は行わず、sdd.5-check の結果を引用し、静的検証・ファイル構造・SPEC 適合・手動検証項目の整理に注力した。

---

## 自動検証項目（sdd.5-check 由来）

`cargo check` と `cargo test` のマトリクスは sdd.5-check で全 exit 0 確認済み（VERIFICATION.md "Actual results (Phase 3 sdd.4-implement, 2026-06-23)" セクション参照）。本 verify では再実行しない（プロジェクト方針「リリースビルドを勝手に走らせない」、および sdd.6-verify の責務範囲）。

| ID    | 内容                                                                                  | 結果                                  |
| ----- | ------------------------------------------------------------------------------------- | ------------------------------------- |
| TS-1  | `cargo check` (default GUI) succeeds                                                  | PASS (sdd.5-check, exit 0)            |
| TS-2  | `cargo check --no-default-features` (CLI-only)                                        | PASS (sdd.5-check, exit 0)            |
| TS-3  | `cargo check --no-default-features --features mux` (CLI+mux)                          | PASS (sdd.5-check, exit 0)            |
| TS-8  | `cargo test` (default GUI) passes                                                     | PASS (sdd.5-check, 1911/1911 + 12/12) |
| TS-9  | `cargo test --no-default-features` (CLI-only) passes                                  | PASS (sdd.5-check, 12/12)             |
| TS-10 | `cargo test --no-default-features --features mux` (CLI+mux) passes                    | PASS (sdd.5-check, 501/501 + 12/12)   |

### 引用元 (VERIFICATION.md より)

- `cargo check` matrix: default GUI / `--no-default-features` / `--no-default-features --features mux` / `--no-default-features --features gui` の 4 形態すべて exit 0、新規 warning なし。
- `cargo test --lib -- --test-threads=1` (default GUI): 1911 / 1911 (3 ignored)、integration `cli_subcommands` 12 / 12。
- `cargo test --no-default-features --features mux --lib -- --test-threads=1`: 501 / 501 (2 ignored)、integration 12 / 12。
- `cargo test --no-default-features --test cli_subcommands` (CLI-only): integration 12 / 12（lib は到達不能、これは FR3 / FR6 の意図的な結果）。

---

## 静的検証（sdd.6 で再点検）

### TS-12: `lib.rs` の `feature = "gui"` gate 残留チェック

**目的**: `mod mux` / `mod pty` / `mod scroll` / `mod wakeup` / `mod self_exec` の宣言が `feature = "gui"` ではなく `feature = "mux"` で gate されていること（FR3, FR6）。

**実行**: `grep -n 'mod mux\|mod pty\|mod scroll\|mod wakeup\|mod self_exec\|cfg(feature' src-tauri/src/lib.rs`

**結果**: PASS

該当 5 モジュールはすべて `#[cfg(feature = "mux")]` で gate されている:

```
86:#[cfg(feature = "mux")]
87:pub mod scroll;
99:#[cfg(feature = "mux")]
100:pub mod mux;
101:#[cfg(feature = "mux")]
102:pub mod pty;
105:#[cfg(feature = "mux")]
106:pub mod self_exec;
121:#[cfg(feature = "mux")]
122:pub mod wakeup;
```

これら 5 行の直前に `feature = "gui"` を含む cfg は存在しない。

### TS-13: `mux/prefix.rs` 内の `parse_mux_action_chord` 参照チェック

**目的**: テストブロック内で `crate::settings::parse_mux_action_chord` が呼ばれていない（GUI-only 経路を踏まない）こと（FR6.1）。

**実行**: `grep -n 'crate::settings::parse_mux_action_chord' src-tauri/src/mux/prefix.rs`

**結果**: PASS（exit code 1, no match）

12 箇所の呼び出しはすべて `crate::mux::prefix::parse_prefix_key` または local `parse_prefix_key` に書き換え済み。GUI-side の `parse_mux_action_chord` 自体は非テストコードで使用継続。

---

## ファイル構造検証

### 作成ファイル

| ファイル                                  | 期待                                                          | 結果 |
| ----------------------------------------- | ------------------------------------------------------------- | ---- |
| `src-tauri/src/viewer_kinds.rs`           | `REPLAYABLE_VIEWER_KINDS` を `pub const` で宣言                | PASS |

`viewer_kinds.rs` の内容確認:
- `pub const REPLAYABLE_VIEWER_KINDS: &[&str] = &["markdown", "image", "json", "yaml"];` （21 行目）
- cfg gate なし（CLI-shared）

### 変更ファイル

| ファイル                                      | 期待                                                              | 結果 |
| --------------------------------------------- | ----------------------------------------------------------------- | ---- |
| `src-tauri/Cargo.toml`                        | `[features]` に `mux = [...]`、`gui` 先頭に `"mux"`               | PASS |
| `src-tauri/src/lib.rs`                        | `mux/pty/scroll/wakeup/self_exec` を `feature = "mux"` gate      | PASS |
| `src-tauri/src/main.rs`                       | `if sub == "mux"` の cfg を `feature = "mux"` 化、エラー文更新   | PASS |
| `src-tauri/src/mux/mod.rs`                    | `pub mod tmux_import;` のみ `#[cfg(feature = "gui")]` 維持        | PASS |
| `src-tauri/src/mux/prefix.rs`                 | テストの 12 箇所を `parse_prefix_key` に書き換え                 | PASS (TS-13) |
| `src-tauri/src/mux/scrollback_filter.rs`      | `use crate::viewer_kinds::REPLAYABLE_VIEWER_KINDS;`              | PASS |
| `src-tauri/src/viewer/mod.rs`                 | `pub use crate::viewer_kinds::REPLAYABLE_VIEWER_KINDS;`           | PASS |
| `Makefile`                                    | `mux-build` / `mux-dpkg` ターゲット追加、`.PHONY` 更新            | PASS |
| `scripts/build-dpkg.sh`                       | `EMTERM_MUX_ONLY` 分岐追加、CLI_ONLY との競合は mux 優先         | PASS |

### 計画外修正 (VERIFICATION.md 記載済み)

| ファイル                          | 内容                                                                   | 結果 |
| --------------------------------- | ---------------------------------------------------------------------- | ---- |
| `src-tauri/src/mux/cli.rs`        | `tmux_import` の `use` (line 21) と call (line 265) に `cfg(gui)` 追加 | PASS |

`grep` で確認: line 21 に `#[cfg(feature = "gui")]\nuse super::tmux_import::import_tmux_conf_if_needed;`、line 265 に `#[cfg(feature = "gui")]\nimport_tmux_conf_if_needed();`。`tmux_import` が GUI-only に gate されたため必須の修正で、IMPLEMENTATION.md の §"Component Interaction" で明示すべき項目（VERIFICATION.md ノートに従い sdd.6 で記載確認、本ドキュメントに記録）。

### Cargo.toml の features

`mux` 新設、`gui` 先頭エントリ `"mux"`、`default = ["gui"]` 保持を確認:
- `mux`: tokio, tokio-util, futures, chrono, anyhow, hostname, vt100, portable-pty, term_core, mux_ipc（FR2 完全一致）
- `gui`: 先頭 `"mux"` + winit/wgpu/egui/egui-wgpu/wry/swash/zeno/fontdb/ab_glyph/resvg/rodio/arboard/notify-rust/raw-window-handle/pollster/term_images/regex/unicode-width/unicode-segmentation/gtk/opener（SPEC FR2 に整合）

### Makefile の整合性

- `.PHONY` に `mux-build mux-dpkg` を含む（line 14）
- `mux-build`: `cargo build --release --no-default-features --features mux $(MANIFEST)`（line 50–51, FR8 完全一致）
- `mux-dpkg`: `EMTERM_MUX_ONLY=1 bash scripts/build-dpkg.sh`（line 63–64, FR8 完全一致）

### build-dpkg.sh の整合性

- 3 モード排他制御: `EMTERM_CLI_ONLY` / `EMTERM_MUX_ONLY` / 既定 GUI（FR9 a）
- 両方 set 時は mux-only 優先、cli-only は警告付き無視（FR9: line 20–23 で `YELLOW Warning: ...` 出力）
- `DEB_PACKAGE` 分岐: `emterm-mux` / `emterm-cli` / `emterm` （line 70–76, FR9）
- ビルドコマンド: `cargo build --manifest-path src-tauri/Cargo.toml --release --no-default-features --features mux`（line 124, FR9 完全一致）
- DEBIAN/control: SPEC 規定通りの `Package: emterm-mux` / `Section: utils` / `Depends: libc6` / `Maintainer` / Description（line 237–255）
- `HEADLESS=1` 時は icons / .desktop / postinst / postrm をスキップ（FR9）

---

## SPEC.md 適合性検証

### 要件カバレッジ (sdd.yaml 由来)

`sdd.yaml.requirements` の 9 個の FR + 4 個の NFR、すべて `status: ok` を確認。各要件は `tasks` フィールドで `tasks.yaml` のタスク ID とリンクされており、`tasks.yaml` の全 8 タスクがすべて `status: completed`。

| 要件   | タイトル                                                                | TS マッピング                                        | 結果 |
| ------ | ----------------------------------------------------------------------- | ---------------------------------------------------- | ---- |
| FR1    | Add `mux` cargo feature                                                 | TS-1, TS-2, TS-3                                     | PASS |
| FR2    | Reclassify dependencies from gui to mux                                 | TS-1, TS-2, TS-3                                     | PASS |
| FR3    | Module gate rewrites in lib.rs                                          | TS-3, TS-12                                          | PASS |
| FR4    | Keep mux::tmux_import gated on gui feature                              | TS-3, TS-6 (manual)                                  | PASS (static); manual smoke 必要 |
| FR5    | Flip emterm mux dispatch cfg in main.rs                                 | TS-3, TS-8, TS-9                                     | PASS |
| FR6    | Move REPLAYABLE_VIEWER_KINDS / scroll / wakeup / self_exec              | TS-3, TS-6, TS-10, TS-12                             | PASS |
| FR6.1  | Rewrite mux::prefix test refs                                           | TS-10, TS-13                                         | PASS |
| FR7    | Re-gate pty module to feature = mux                                     | TS-3, TS-6 (manual)                                  | PASS (static); manual smoke 必要 |
| FR8    | Add mux-build/mux-dpkg Makefile targets                                 | TS-11 (manual packaging)                             | PASS (static); 実機 dpkg 必要 |
| FR9    | Extend build-dpkg.sh with EMTERM_MUX_ONLY                               | TS-11 (manual packaging)                             | PASS (static); 実機 dpkg 必要 |
| NFR1   | Backward compatibility for GUI / CLI-only debs                          | TS-1, TS-2, TS-4, TS-5, TS-11                        | PASS (static); 実機 dpkg shape 確認必要 |
| NFR2   | CLI+mux build is faster/lighter than GUI                                | TS-3 (qualitative)                                   | Manual-Required（subjective） |
| NFR3   | Feature orthogonality (gui+mux equals gui)                              | TS-1, TS-3                                           | PASS（`--features mux,gui` arm exit 0） |
| NFR4   | CLI+mux deb depends on libc6 only                                       | TS-11                                                | PASS (static, control file); 実機 dpkg-deb --info 必要 |

### tasks.yaml 全タスク状態

| ID                                | Phase | Status     |
| --------------------------------- | ----- | ---------- |
| hoist-replayable-viewer-kinds     | 1     | completed  |
| add-mux-cargo-feature             | 2     | completed  |
| regate-mux-pty-scroll-modules     | 3     | completed  |
| flip-mux-dispatch-in-main         | 4     | completed  |
| add-mux-make-targets              | 5     | completed  |
| extend-build-dpkg-script          | 6     | completed  |
| build-test-matrix-sweep           | 7     | completed  |

全 7 タスクすべて `completed`（依存関係も矛盾なし）。

---

## 手動検証必要項目（Manual-Required）

`cargo build --release` 系の実機ビルドや dpkg ファイル shape 確認は本セッションで実行していない（プロジェクト方針: 「リリースビルドを勝手に走らせない」「make build / make mux-build / make dpkg / make mux-dpkg は run しない」）。ユーザーが時間のあるときに以下を実機で実施してほしい。

### TS-4: GUI release build (regression)

- [ ] `make build` が成功し、`src-tauri/target-host/release/emterm` が windowed terminal を起動
- [ ] Inside that terminal, `emterm mux daemon &` と `emterm mux attach` が動作
- [ ] `emterm markdown <file>` が child Markdown viewer を起動

### TS-5: CLI-only release build (regression)

- [ ] `make cli-build` が成功
- [ ] `./src-tauri/target-host/release/emterm` をサブコマンドなしで起動すると "this build provides only CLI subcommands" usage を出して exit 2
- [ ] `emterm mux daemon` が SPEC FR5 通りの新メッセージ "emterm: \`mux\` is not available in this build. / Install a build that includes the \`mux\` feature (\`emterm\` or \`emterm-mux\`) to use \`emterm mux\`." を出して exit 2
- [ ] `emterm markdown <file>` が動作

### TS-6: CLI+mux release build (new)

- [ ] `make mux-build` が成功し、`src-tauri/target-host/release/emterm` を生成
- [ ] `emterm mux daemon &` が daemon を起動
- [ ] `emterm mux attach`（別 shell から）が daemon に bridge して shell が interactive
- [ ] `emterm markdown <file>` が OSC Markdown sequence を stdout に emit
- [ ] `emterm`（no subcommand）が "this build provides only CLI subcommands" usage を出して exit 2

### TS-7: Windows cross-build (regression)

- [ ] `make win-build` (`cargo xwin build --release --target x86_64-pc-windows-msvc`) が成功
- [ ] `src-tauri/target-win/x86_64-pc-windows-msvc/release/emterm.exe` が生成

### TS-11: Packaging tests

- [ ] `make dpkg` で `build/emterm_<ver>_<arch>.deb` 生成、`dpkg-deb --info` が `Depends: libc6, libwebkit2gtk-4.1-0, libgtk-3-0, libglib2.0-0`
- [ ] `make cli-dpkg` で `build/emterm-cli_<ver>_<arch>.deb` 生成、`Depends: libc6`
- [ ] `make mux-dpkg` で `build/emterm-mux_<ver>_<arch>.deb` 生成、`Depends: libc6`
- [ ] `dpkg-deb --info build/emterm-mux_<ver>_<arch>.deb` が `Package: emterm-mux`, `Section: utils`, `Maintainer: m-m-n <51132276+m-m-n@users.noreply.github.com>` を含む

### M1–M5: Manual smoke tests

VERIFICATION.md §"Manual Testing (E2E Not Possible)" の M1–M5 を実機で実施:
- [ ] M1: GUI tier smoke（unchanged behavior）
- [ ] M2: CLI+mux tier smoke on a host with only libc6（`sudo dpkg -i emterm-mux_<ver>_<arch>.deb`）
- [ ] M3: CLI-only tier smoke（unchanged）
- [ ] M4: Deb file shape（`dpkg-deb --contents` で 3 deb のレイアウト確認）
- [ ] M5: SSH-side deployment dry-run on a clean Ubuntu host（UC1）

### NFR2 qualitative

- [ ] `make mux-build` が `make build` より速い・バイナリも小さい（winit/wgpu/wry/GTK/WebKitGTK/swash/zeno/fontdb/resvg 未リンク）

---

## 総合評価

### Static / Automated 結果（このセッションで確認したもの）

- **TS-1〜TS-3, TS-8〜TS-10** (cargo check / cargo test matrix): 全 PASS（sdd.5-check 由来、再実行せず引用）
- **TS-12** (lib.rs `feature = "gui"` 残留 grep): PASS
- **TS-13** (`mux/prefix.rs` `parse_mux_action_chord` 残留 grep): PASS
- **ファイル構造** (作成 1 / 変更 9): 全 PASS
- **Cargo.toml `[features]`**: FR1/FR2 仕様完全一致
- **Makefile / build-dpkg.sh**: FR8/FR9 仕様完全一致
- **`mux/cli.rs` 計画外修正**: 正しく `cfg(feature = "gui")` 化されている
- **sdd.yaml 全要件 status: ok / tasks.yaml 全タスク completed**

### 結論

自動・静的検証範囲はすべて PASS。SPEC.md の機能要件 (FR1–FR9, FR6.1) および非機能要件 (NFR1, NFR3) は静的に満たされている。

NFR2（性能定性比較）と NFR4（実機 deb の `Depends: libc6` 確認）、TS-4–TS-7（release build matrix）、TS-11（dpkg 実機検証）、および M1–M5 の手動 smoke tests は実機での確認が必要（このセッションでは実行しない方針による）。

ステータス: **PASS（自動・静的検証範囲）／ Manual-Required 12 件**
