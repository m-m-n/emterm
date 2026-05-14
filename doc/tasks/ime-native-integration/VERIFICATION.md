# Verification Document: Native IME Integration (Phase 4-G, **redesigned**)

## Overview

**Feature**: ime-native-integration
**SPEC.md**: `doc/tasks/ime-native-integration/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/ime-native-integration/IMPLEMENTATION.md`

**Note on redesign**: 前 Phase 4-G (tao 0.34 + 自前 XIM / Wayland / IMM32 backend) は実機で動作しないことが判明し、**winit 0.30.9 への戦略 A 移行** に redesign された。本ドキュメントは redesign 後の検証要件のみ記載する。前 Phase 4-G の検証結果は `VERIFICATION_RESULT.md` に historical record として残るが、本 SDD では superseded 扱い。

## Build Verification

- **Command**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo build --workspace"`
- **Expected**: exit 0、エラーゼロ。forward-staged dead-code warning は `sdd.yaml` notes に記録すれば許容
- **Linux**: 各 sub-phase 完了時に必須
- **Windows**: 4-G-3 完了時に必須 (cross-build `cargo build --workspace --target x86_64-pc-windows-msvc` または Windows host での native build)

### 期待結果記録欄

- 4-G-1 完了時: 自前 XIM 削除後の `cargo build --workspace` exit 0、依存削除でクリーン
- 4-G-2 完了時: winit 0.30.9 への移行後 Linux build green、wgpu surface 整合性 OK
- 4-G-3 完了時: WinitImeBridge 実装後も Linux build green
- 4-G-4 完了時: 最終 build green (Linux + Windows)
  (実測値は sdd.6-verify が `VERIFICATION_RESULT.md` に記録)

## Test Verification

- **Command**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --workspace"`
- **Coverage target**:
  - 4-G-1 完了後: 旧 Phase 4-G test 数 2011 から 削除 33 件 → 約 1978 件 (NullBackend / route / settings / fallback / cursor / focus / backbone 12 件は維持)
  - 4-G-3 完了後: 1978 + 7 (TS-winit-1..7) = 約 1985 件以上
  - Phase 4-E の既存 `ime::{preedit, commit}` テストは 100% 維持 (regression ゼロ)

### Test Scenarios

| ID | Scenario | Expected Result | Test Type | Phase |
|----|----------|-----------------|-----------|-------|
| TS-backend-1 | `NullBackend::dispatch_key_event` がすべてのキーで `Passthrough` を返す | 戻り値 `Passthrough` | Unit | 4-G-1 維持 |
| TS-backend-2 | `NullBackend::pump` が空 `ImeEvent` vec を返す | events.len() == 0 | Unit | 4-G-1 維持 |
| TS-backend-3 | `App::pump_ime` が `MockBackend` の queue を drain し `on_ime_{preedit,commit,focus_lost}` にルーティング | 各メソッドが期待回数だけ呼ばれる | Unit (App integration) | 4-G-1 維持 |
| TS-backend-4 | `MockBackend::dispatch_key_event` が `Consumed` を返した時、`winit_key_to_bytes` 経路が skip される (4-G-2 で名称変更) | mocked PTY が bytes を受け取らない | Unit | 4-G-1 維持 (4-G-2 で fn 名 update) |
| TS-backend-5 | `dispatch_key_event` が `Passthrough` を返した時、`winit_key_to_bytes` の bytes が PTY に 1 回だけ届く | mocked PTY が exactly 1 write | Unit | 4-G-1 維持 (4-G-2 で fn 名 update) |
| TS-cursor-1 | cursor cell が変化した時のみ `notify_cursor_rect` が呼ばれる | 変化なしフレームでは呼ばれない | Unit | 4-G-1 維持 |
| TS-focus-1 | `Focused(false)` で `notify_focus(false)` + `App::on_ime_focus_lost` が呼ばれ、`preedit_state.active()` が false | preedit クリア確認 | Unit | 4-G-1 維持 |
| TS-fallback-1 | `EMTERM_NATIVE_IME=0` → `App` が `NullBackend` を保持 (settings の値に関係なく) | backend type 確認 | Unit | 4-G-1 維持 |
| TS-fallback-2 | `settings.ime.native_integration = false` (env なし) → `NullBackend` | backend type 確認 | Unit | 4-G-1 維持 |
| TS-fallback-3 | `WinitImeBridge::init` が `ImeInitError::Unavailable(_)` を返した時、startup で catch → `NullBackend` 化、warn ログ exactly 1 回 | log captured, NullBackend installed | Unit | 4-G-1 維持 (4-G-3 で実 backend が WinitImeBridge に変わる) |
| TS-settings-1 | `Settings::default().ime.native_integration` が `true` | default == true | Unit | 4-G-1 維持 |
| TS-route-1 | `ImeEvent::Preedit("a\x1bb")` → `App::on_ime_preedit` → `sanitize` で ESC 削除 → overlay text が "ab" (Phase 4-E regression guard) | preedit.text() == "ab" | Unit | 4-G-1 維持 |
| TS-route-2 | `ImeEvent::Commit("a\x1bb")` → `App::on_ime_commit` → PTY が `b"ab"` だけ受信 (ESC drop、bracketed-paste で wrap されない) | PTY mocked write が `b"ab"` exactly 1 回 | Unit | 4-G-1 維持 |
| TS-winit-1 | `WinitImeBridge::on_winit_ime(Ime::Enabled)` で `im_composing = true`、続く `dispatch_key_event` が `Consumed` を返す | state 確認 + Consumed | Unit | 4-G-3 |
| TS-winit-2 | `Ime::Preedit("foo", None)` で `ImeEvent::Preedit("foo")` queue 化、pump で drain | queue / pump 確認 | Unit | 4-G-3 |
| TS-winit-3 | `Ime::Commit("日本")` で `ImeEvent::Commit("日本")` queue 化 + `im_composing = false`、続く `dispatch_key_event` が `Passthrough` | queue / state 両方 | Unit | 4-G-3 |
| TS-winit-4 | `Ime::Disabled` で `ImeEvent::FocusOut` queue 化 + `im_composing = false` | queue / state 確認 | Unit | 4-G-3 |
| TS-winit-5 | `Ime::Commit` → `Ime::Disabled` 順序で idempotent (im_composing false 維持、FocusOut 1 件) | 状態整合 | Unit | 4-G-3 |
| TS-winit-6 | Shift 単独 release (`KeyEvent { state: Released, logical_key: Key::Named(NamedKey::Shift) }`) が `dispatch_key_event` を経由 | dispatch 呼出回数 | Unit | 4-G-3 |
| TS-winit-7 | `notify_cursor_rect(x, y, w, h)` が同一 rect で重複呼出しない | mock window の `set_ime_cursor_area` 呼出回数 | Unit | 4-G-3 |
| TS-winit-int-1 | winit `EventLoop` で window 生成 → `set_ime_allowed(true)` → 実 IM 無しでも `Ime::Disabled` 通知が来る (`#[ignore]`、Xvfb) | events queue に Disabled | Integration (host) | 4-G-3 |
| TS-winit-int-2 | winit が IMM32 経由で `Ime::Commit` を発火 (`#[cfg(windows)]`、host-deferred) | events queue に Commit | Integration (host) | 4-G-3 |
| TS-perf-3 | preedit key 押下 → overlay redraw < 30 ms (Linux X11 release host、`EMTERM_IME_PERF=1`) | latency < 30 ms | Performance (manual host) | 4-G-4 |
| TS-perf-4 | commit → `PtySession::write` < 5 ms | latency < 5 ms | Performance (manual host) | 4-G-4 |
| TS-perf-regression | IME-OFF key-down → PTY write latency が Phase 4 baseline の +10% 以内 (winit 移行込み) | delta ≤ Phase 4 baseline × 1.10 | Performance (manual host) | 4-G-4 |

### Tests Removed (前 Phase 4-G から削除)

| Removed ID | Reason |
|------------|--------|
| TS-x11-1, TS-x11-2 (11 cases) | `ime::x11.rs` 削除に伴う |
| TS-wayland-1, TS-wayland-2 (10 cases) | `ime::wayland.rs` 削除に伴う |
| TS-windows-1, TS-windows-2, TS-windows-3 (10 cases) | `ime::windows.rs` 削除に伴う |
| TS-backend-int-1 | X11 + xvfb stub IM responder integration、自前 XIM 削除で意味喪失 |
| TS-backend-int-2 | Windows hidden HWND + SendMessageW integration、自前 IMM32 削除で意味喪失 |

## Code Quality Verification

- **Format**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo fmt --all -- --check"` exit 0
- **Static analysis**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo clippy -p emterm-native-poc -- -D warnings"` exit 0

### 期待結果記録欄

- `cargo fmt --all -- --check`: clean
- `cargo clippy -p emterm-native-poc -- -D warnings`: clean
- `cargo test --workspace`: 約 1985 件以上、failed ゼロ
  (実測値は sdd.6-verify が `VERIFICATION_RESULT.md` に記録)

## File Structure Verification

### Files to Create

- `native-poc/src/ime/winit_bridge.rs` — `WinitImeBridge` + state machine (4-G-3)

### Files to Modify

- `native-poc/Cargo.toml` — winit 0.30.9 追加、tao / x11-dl / wayland-client / wayland-protocols / windows 削除 (4-G-1 + 4-G-2)
- `native-poc/src/main.rs` — `winit::EventLoop` (4-G-2)
- `native-poc/src/window_host.rs` — winit API、`WindowEvent::Ime` ルーティング (4-G-2 + 4-G-3)
- `native-poc/src/app.rs` — winit 型参照に置換、release dispatch 通し (4-G-2 + 4-G-3)
- `native-poc/src/ime/mod.rs` — x11/wayland/windows mod 削除 + winit_bridge mod 追加 (4-G-1 + 4-G-3)
- `native-poc/src/ime/backend.rs` — factory が `WinitImeBridge::init(window)` を呼ぶ (4-G-1 + 4-G-3)
- `native-poc/README.md` — Phase 4-G feature matrix を winit 採択方針に更新 (4-G-4)

### Files to Delete

- `native-poc/src/ime/x11.rs` (4-G-1)
- `native-poc/src/ime/wayland.rs` (4-G-1)
- `native-poc/src/ime/windows.rs` (4-G-1)

### Files NOT Modified (Phase 4-E 契約)

- `native-poc/src/ime/preedit.rs` — Phase 4-E auto-scope、変更しない
- `native-poc/src/ime/commit.rs` — Phase 4-E auto-scope、変更しない
- `native-poc/src/ime/null.rs` — Phase 4-G-A から不変
- `native-poc/src/settings.rs` — `ImeSettings { native_integration: bool }` 不変
- `native-poc/src/render/cursor.rs` — Phase 4-E の `draw_cursor_with_preedit` は変更しない

`git diff` でこの 5 ファイルに content 変更がないことを確認すること (settings.rs と null.rs は Phase 4-G-A 以降の状態を base に diff empty)。

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR4-FR13 実装 + unit/integration tests pass | `cargo test --workspace` exit 0 + 下表の FR coverage |
| SC-2 | `cargo build --workspace` Linux + Windows で成功 | Build commands を両プラットフォームで実行 |
| SC-3 | `cargo test --workspace` exit 0 | Test command |
| SC-4 | `cargo fmt --all -- --check` clean | Format command |
| SC-5 | `cargo clippy -p emterm-native-poc -- -D warnings` clean | Clippy command |
| SC-6 | Manual TS-manual-ime-x11 / x11-ibus / wayland / windows / fallback / imserver-restart / mux すべて pass | Manual Testing セクション |
| SC-7 | TS-perf-3 / TS-perf-4 / TS-perf-regression がしきい値達成 | Performance Verification セクション |
| SC-8 | Phase 4-E `ime::preedit::State` / `ime::commit::write_commit` 振る舞い不変 | `git diff` で `preedit.rs` / `commit.rs` / `render/cursor.rs` の content 変更なし、TS-route-1/2 regression pass |
| SC-9 | 旧 `src-tauri` build / test 不変 | Workspace build/test 全 phase で green |
| SC-10 (新規) | 旧 `ime::{x11, wayland, windows}` が削除されている | `git ls-files native-poc/src/ime/` 確認 |
| SC-11 (新規) | `Cargo.toml` から tao / x11-dl / wayland-client / wayland-protocols / windows が消えている | `grep` 確認 |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR4 (ImeBackend trait surface) | 4-G-1 維持 | TS-backend-1, TS-backend-2, TS-backend-3 |
| FR5 (Routing into Phase 4-E layer) | 4-G-1 維持 | TS-backend-3, TS-route-1, TS-route-2 |
| FR6 (Key event interception with state machine) | 4-G-3 | TS-backend-4, TS-backend-5, TS-winit-1, TS-winit-3, TS-winit-6 |
| FR7 (Cursor rectangle reporting via set_ime_cursor_area) | 4-G-3 | TS-cursor-1, TS-winit-7 + manual host gates |
| FR8 (Focus management) | 4-G-3 | TS-focus-1, TS-winit-4 + manual host gates |
| FR9 (Opt-out / fallback) | 4-G-1 維持 + 4-G-3 (実 backend が WinitImeBridge) | TS-fallback-1, TS-fallback-2, TS-fallback-3, TS-manual-ime-fallback |
| FR10 (Settings additions) | 4-G-1 維持 | TS-settings-1 |
| FR11 (winit Ime → ImeEvent 変換) | 4-G-3 | TS-winit-2, TS-winit-3, TS-winit-4, TS-winit-5 |
| FR12 (Ghostty state machine `im_composing` + `in_keyevent`) | 4-G-3 | TS-winit-1, TS-winit-3, TS-winit-5, TS-winit-6 |
| FR13 (tao → winit migration) | 4-G-2 | `cargo build --workspace` exit 0 (tao が dep にない、winit が dep にある)、TS-backend-4 / TS-backend-5 (winit_key_to_bytes 動作) |

**Removed**:
- FR1 (XIM client Linux X11): 削除、winit に統合
- FR2 (zwp_text_input_v3 Wayland): 削除、winit に統合
- FR3 (IMM32 Windows): 削除、winit に統合

### Non-Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| NFR1 (preedit redraw < 30 ms) | 4-G-4 | TS-perf-3 |
| NFR2 (commit → PtySession::write < 5 ms) | 4-G-4 | TS-perf-4 |
| NFR3 (IME-OFF regression ≤ +10%) | 4-G-4 | TS-perf-regression |
| NFR4 (Stability: init failure no crash, IM server death falls back within 1 tick) | 4-G-3 (fallback) + 4-G-4 (mid-session disconnect) | TS-fallback-3, TS-manual-ime-imserver-restart |
| NFR5 (Workspace compatibility, src-tauri untouched, winit/wgpu integration verified) | 全 phase | `cargo build/test --workspace` を各 sub-phase 完了時に確認 |
| NFR6 (Module layout) | 全 phase | File Structure Verification + 削除 3 ファイルの不在確認 + `preedit.rs` / `commit.rs` 不変確認 |
| NFR7 (Logging: init success / fallback / Ime::Disabled detection) | 全 phase | Manual log inspection during manual gates |
| NFR8 (Linux fcitx5 parity with Phase 1 via winit) | 4-G-4 | TS-manual-ime-x11 で Phase 1 SPEC US1-US5 と差分確認 |

## E2E Testing

既存 `./scripts/run-e2e-docker.sh` は legacy Tauri build 専用で native-poc には適用外。Phase 4-G redesign では new E2E は追加しない。

- [ ] Legacy E2E (`./scripts/run-e2e-docker.sh test`) が `main` と同じ preexisting fail list を示すこと (regression check, gate ではない)

## Manual Testing (E2E Not Possible)

- [ ] **TS-manual-ime-x11** (Linux X11 + fcitx5 host): native-poc を winit X11 経路で起動、`Ctrl+Space` で fcitx5 トグル、"nihongo" 入力 → underline preedit overlay 表示 → `Space` で変換 → `Enter` で確定、シェルに "日本語" が exactly 1 回届く。`Ctrl+C` / 矢印 / `Esc` / `Tab` は composition 中でも従来動作
- [ ] **TS-manual-ime-x11-ibus** (Linux X11 + IBus host): 上記と同じフローを IBus で実施
- [ ] **TS-manual-ime-wayland** (Linux Wayland + fcitx5-wayland): KDE Plasma 6 (KWin) + Sway の 2 環境で同じフローを実施。**winit がネイティブに zwp_text_input_v3 を扱う**ので前 Phase 4-G の自前 scaffold より動作する見込み
- [ ] **TS-manual-ime-windows** (Windows + MS-IME / Google IME): 同じフロー。候補ウィンドウがカーソル近傍に出る (best effort、gating ではない)
- [ ] **TS-manual-ime-fallback** (任意 host): `EMTERM_NATIVE_IME=0` で起動、warn ログ 1 回 + fallback 動作 (`Window::set_ime_allowed(false)` 相当で IME 関連 event は来ない、ASCII キーは直接 PTY へ)
- [ ] **TS-manual-ime-imserver-restart** (Linux X11): fcitx5 を kill → winit `Ime::Disabled` 検出 → warn ログ + preedit クリア、fcitx5 を再起動 → native-poc を blur / refocus → winit `Ime::Enabled` 再発火、IME 再 attach
- [ ] **TS-manual-ime-mux** (Linux X11 + fcitx5 + emterm mux): `emterm mux attach` 中の session で日本語入力、commit が mux 経由 PTY に届く (Phase 4-C の APC inband path に regression なし)

## Performance Verification

- TS-perf-3: preedit key 押下 → overlay redraw < 30 ms (Linux X11 release host)。`App::on_ime_preedit` 入口 + `WindowHost::request_redraw` を `Instant::now()` で挟み記録 (`EMTERM_IME_PERF=1` env)
- TS-perf-4: `App::on_ime_commit` 入口 → `PtySession::write` 完了まで < 5 ms (release host)
- TS-perf-regression: IME-OFF 時の key-down → PTY write latency。Phase 4 `TS-perf-1` / `TS-perf-2` の baseline (`doc/tasks/mux-tabs-windows-ime/VERIFICATION_RESULT.md` 記録値) を取得し、本 phase 計測値が +10% 以内であることを確認。**winit 移行を含めて担保**

## Security Verification

- [ ] preedit / commit テキストは既存 `ime::preedit::sanitize` を経由 (C0/C1 strip)。TS-route-1 / TS-route-2 で regression guard
- [ ] commit は bracketed-paste で wrap しない (`ime::commit::write_commit` 既存契約)。TS-route-2
- [ ] winit が提供する UTF-8 `String` は valid (winit が内部で X11 wide char / Win32 UTF-16 → UTF-8 変換済み)。invalid byte は winit 側で drop される。`WinitImeBridge` 側では追加 validation 不要
- [ ] `WinitImeBridge::drop` で `Window::set_ime_allowed(false)` を呼ぶ。winit が IC / subclass を自動解放するので手動 cleanup 不要
- [ ] `Settings::default().ime.native_integration` が `true` (TS-settings-1)

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit (4-G-1 retained) | 12 (TS-backend-1..5, TS-cursor-1, TS-focus-1, TS-fallback-1..3, TS-settings-1, TS-route-1..2) | 12 | 0 | 0 |
| Unit (4-G-3 new) | 7 (TS-winit-1..7) | 7 | 0 | 0 |
| Integration (4-G-3 new) | 2 (TS-winit-int-1 Xvfb / TS-winit-int-2 Windows) | 2 | 0 | 0 |
| Performance | 3 (TS-perf-3, TS-perf-4, TS-perf-regression) | 0 | 0 | 3 |
| Manual | 7 (TS-manual-ime-x11, x11-ibus, wayland, windows, fallback, imserver-restart, mux) | 0 | 0 | 7 |
| Legacy regression | 1 (legacy E2E preexisting fail list) | 0 | 1 | 0 |
| **Total** | **32** | **21** | **1** | **10** |

**Note (前 Phase 4-G からの差分)**: 削除 31 件 (TS-x11/wayland/windows + TS-backend-int) - 新規 9 件 (TS-winit-1..7 + TS-winit-int-1..2) = net -22 件。テスト総数は 32 件 → 32 件で同数だが、内訳は backbone 維持 + winit 系新規。
