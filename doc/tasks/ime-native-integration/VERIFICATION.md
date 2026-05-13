# Verification Document: Native IME Integration (Phase 4-G)

## Overview

**Feature**: ime-native-integration
**SPEC.md**: `doc/tasks/ime-native-integration/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/ime-native-integration/IMPLEMENTATION.md`

## Build Verification

- **Command**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo build --workspace"`
- **Expected**: exit 0、エラーゼロ。forward-staged dead-code warning は `sdd.yaml` notes に記録すれば許容 (Phase 3 / 4 precedent)
- **Linux**: 各 sub-phase 完了時に必須
- **Windows**: 4-G-D 完了時に必須 (cross-build `cargo build --workspace --target x86_64-pc-windows-msvc` または Windows host での native build)

### 期待結果記録欄

- 4-G-A 完了時: `cargo build --workspace` exit 0
- 4-G-B 完了時: Linux 上で X11 backend 含むコードが build pass
- 4-G-C 完了時: Wayland deps 追加後も Linux build green
- 4-G-D 完了時: Windows target で build green
- 4-G-E 完了時: 最終 build green
  (実測値は sdd.6-verify が `VERIFICATION_RESULT.md` に記録)

## Test Verification

- **Command**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --workspace"`
- **Coverage target**:
  - `native-poc`: Phase 4 完了時点 (1940 tests) から +20 件以上
  - Phase 4-E の既存 `ime::{preedit, commit}` テストは 100% 維持 (regression ゼロ)

### Test Scenarios

| ID | Scenario | Expected Result | Test Type | Phase |
|----|----------|-----------------|-----------|-------|
| TS-backend-1 | `NullBackend::dispatch_key_event` がすべてのキーで `Passthrough` を返す | 戻り値 `Passthrough` | Unit | 4-G-A |
| TS-backend-2 | `NullBackend::pump` が空 `ImeEvent` vec を返す | events.len() == 0 | Unit | 4-G-A |
| TS-backend-3 | `App::pump_ime` が `MockBackend` の queue を drain し `on_ime_{preedit,commit,focus_lost}` にルーティング | 各メソッドが期待回数だけ呼ばれる | Unit (App integration) | 4-G-A |
| TS-backend-4 | `MockBackend::dispatch_key_event` が `Consumed` を返した時、`tao_key_to_bytes` 経路が skip される | mocked PTY が bytes を受け取らない | Unit | 4-G-A |
| TS-backend-5 | `dispatch_key_event` が `Passthrough` を返した時、`tao_key_to_bytes` の bytes が PTY に 1 回だけ届く | mocked PTY が exactly 1 write | Unit | 4-G-A |
| TS-cursor-1 | cursor cell が変化した時のみ `notify_cursor_rect` が呼ばれる | 変化なしフレームでは呼ばれない | Unit | 4-G-A |
| TS-focus-1 | `Focused(false)` で `notify_focus(false)` + `App::on_ime_focus_lost` が呼ばれ、`preedit_state.active()` が false | preedit クリア確認 | Unit | 4-G-A |
| TS-fallback-1 | `EMTERM_NATIVE_IME=0` → `App` が `NullBackend` を保持 (settings の値に関係なく) | backend type 確認 | Unit | 4-G-A |
| TS-fallback-2 | `settings.ime.native_integration = false` (env なし) → `NullBackend` | backend type 確認 | Unit | 4-G-A |
| TS-fallback-3 | `ImeBackend::init` が `ImeInitError::Unavailable(_)` を返した時、startup で catch → `NullBackend` 化、warn ログ exactly 1 回 | log captured, NullBackend installed | Unit | 4-G-A |
| TS-settings-1 | `Settings::default().ime.native_integration` が `true` (構造体 default 経路、Phase 7 JSON ローダの shape pin) | default == true | Unit | 4-G-A |
| TS-route-1 | `ImeEvent::Preedit("a\x1bb")` → `App::on_ime_preedit` → `sanitize` で ESC 削除 → overlay text が "ab" (Phase 4-E regression guard) | preedit.text() == "ab" | Unit | 4-G-A |
| TS-route-2 | `ImeEvent::Commit("a\x1bb")` → `App::on_ime_commit` → PTY が `b"ab"` だけ受信 (ESC drop、bracketed-paste で wrap されない) | PTY mocked write が `b"ab"` exactly 1 回 | Unit | 4-G-A |
| TS-x11-1 | tao KeyEvent (ASCII letter) → synthetic XKeyPressedEvent の keycode 変換が正しい | 変換結果が期待値 | Unit | 4-G-B |
| TS-x11-2 | tao Modifier mask (Ctrl, Alt, Shift) → XKeyEvent::state 変換が正しい | 変換結果が期待値 | Unit | 4-G-B |
| TS-wayland-1 | `WaylandBackend::pump` が internal channel に push された `ImeEvent` を drain | events vec に push 結果反映 | Unit | 4-G-C |
| TS-wayland-2 | registry に `zwp_text_input_manager_v3` がない時、`init` が `ImeInitError::Unavailable` を返す | error type 確認 | Unit | 4-G-C |
| TS-windows-1 | UTF-16 BMP 文字列 → UTF-8 変換が正しい (例: "日本語") | 変換結果が UTF-8 で "日本語" | Unit | 4-G-D |
| TS-windows-2 | UTF-16 Surrogate pair (例: U+1F600) → UTF-8 4 byte sequence | 変換結果が正しい 4 byte | Unit | 4-G-D |
| TS-windows-3 | 不正な UTF-16 surrogate → drop + warn (IME_E401) | None 返却 + log captured | Unit | 4-G-D |
| TS-backend-int-1 | X11Backend を xvfb + stub IM responder に対して起動、preedit / commit が `pump` 経由で届く (`#[ignore]`) | events queue に Preedit / Commit | Integration (host) | 4-G-B |
| TS-backend-int-2 | Hidden HWND に subclass を install、`WM_IME_COMPOSITION` (`GCS_RESULTSTR`) を `SendMessageW` で疑似発射、`ImeEvent::Commit` が pump 経由で届く (`#[cfg(windows)]`) | events queue に Commit | Integration (host) | 4-G-D |
| TS-perf-3 | preedit key 押下 → overlay redraw < 30 ms (Linux X11 release host) | latency < 30 ms | Performance (manual host) | 4-G-E |
| TS-perf-4 | commit → `PtySession::write` < 5 ms | latency < 5 ms | Performance (manual host) | 4-G-E |
| TS-perf-regression | IME-OFF key-down → PTY write latency が Phase 4 baseline の +10% 以内 | delta ≤ Phase 4 baseline × 1.10 | Performance (manual host) | 4-G-E |

## Code Quality Verification

- **Format**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo fmt --all -- --check"` exit 0
- **Static analysis**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo clippy -p emterm-native-poc -- -D warnings"` exit 0。forward-staged warning は `sdd.yaml` notes に記録すれば許容 (Phase 4-F precedent: 14 warning baseline)

### 期待結果記録欄

- `cargo fmt --all -- --check`: clean
- `cargo clippy -p emterm-native-poc -- -D warnings`: clean (新規 warning ゼロ、または notes 記録済み forward-staged のみ)
- `cargo test --workspace`: +20 件以上、failed ゼロ
  (実測値は sdd.6-verify が `VERIFICATION_RESULT.md` に記録)

## File Structure Verification

### Files to Create

- `native-poc/src/ime/backend.rs` — `ImeBackend` trait + `ImeEvent` enum + `KeyDispatchResult` + `ImeInitError` + `RawKeyEvent` + `ImeBackendFactory` (4-G-A)
- `native-poc/src/ime/null.rs` — `NullBackend` (passthrough) (4-G-A)
- `native-poc/src/ime/x11.rs` — `X11Backend` (`cfg(all(unix, not(target_os = "macos")))`) (4-G-B)
- `native-poc/src/ime/wayland.rs` — `WaylandBackend` + pump thread (`cfg(all(unix, not(target_os = "macos")))`) (4-G-C)
- `native-poc/src/ime/windows.rs` — `WindowsBackend` (`cfg(windows)`) (4-G-D)

### Files to Modify

- `native-poc/Cargo.toml` — `raw-window-handle` を direct dep に昇格、`x11-dl` / `wayland-client` / `wayland-protocols` / `windows` を OS 別 cfg target で追加 (4-G-A / 4-G-B / 4-G-C / 4-G-D で漸進的に追加)
- `native-poc/src/ime/mod.rs` — `pub mod backend; pub mod null;` 追加、OS 別 cfg backend の `pub mod` 追加 (各 phase で漸進)
- `native-poc/src/settings.rs` — `ImeSettings { native_integration: bool }` 構造体 + `Settings` field 追加 + `Default::default()` で `native_integration: true` (4-G-A)。JSON load は Phase 7 のローダ責務、Phase 4-G では default 経路のみ exercise する
- `native-poc/src/app.rs` — `ime_backend: Box<dyn ImeBackend>` フィールド + `pump_ime` + `notify_cursor_rect_if_changed` + `dispatch_key_event_via_ime` ヘルパ追加 (4-G-A)
- `native-poc/src/window_host.rs` — startup factory 呼出、`KeyboardInput` を backend dispatch 優先、`Focused` を backend に通知、`ReceivedImeText` は NullBackend のみ既存パス、tick 終端で `pump_ime` (4-G-A)
- `native-poc/README.md` — Phase 4-G feature matrix 追記 (4-G-E)

### Files NOT Modified (Phase 4-E 契約)

- `native-poc/src/ime/preedit.rs` — Phase 4-E auto-scope、変更しない
- `native-poc/src/ime/commit.rs` — Phase 4-E auto-scope、変更しない
- `native-poc/src/render/cursor.rs` — Phase 4-E の `draw_cursor_with_preedit` は変更しない

`git diff` でこの 3 ファイルに content 変更がないことを確認すること。

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1-FR10 実装 + unit/integration tests pass | `cargo test --workspace` exit 0 + 下表の FR coverage |
| SC-2 | `cargo build --workspace` Linux + Windows で成功 | Build commands (Build Verification セクション) を両プラットフォームで実行 |
| SC-3 | `cargo test --workspace` exit 0 | Test command (Test Verification セクション) |
| SC-4 | `cargo fmt --all -- --check` clean | Format command (Code Quality セクション) |
| SC-5 | `cargo clippy -p emterm-native-poc -- -D warnings` clean | Clippy command (Code Quality セクション) |
| SC-6 | Manual TS-manual-ime-x11 / x11-ibus / wayland / windows / fallback / imserver-restart / mux すべて pass | Manual Testing セクション |
| SC-7 | TS-perf-3 / TS-perf-4 / TS-perf-regression がしきい値達成 | Performance Verification セクション |
| SC-8 | Phase 4-E `ime::preedit::State` / `ime::commit::write_commit` 振る舞い不変 | `git diff` で `preedit.rs` / `commit.rs` の content 変更なし、TS-route-1/2 regression pass |
| SC-9 | 旧 `src-tauri` build / test 不変 | Workspace build/test 全 phase で green |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 (XIM client Linux X11) | 4-G-B | TS-x11-1, TS-x11-2, TS-backend-int-1, TS-manual-ime-x11, TS-manual-ime-x11-ibus |
| FR2 (zwp_text_input_v3 Wayland) | 4-G-C | TS-wayland-1, TS-wayland-2, TS-manual-ime-wayland |
| FR3 (IMM32 Windows) | 4-G-D | TS-windows-1, TS-windows-2, TS-windows-3, TS-backend-int-2, TS-manual-ime-windows |
| FR4 (ImeBackend trait) | 4-G-A | TS-backend-1, TS-backend-2, TS-backend-3 |
| FR5 (Routing into Phase 4-E layer) | 4-G-A | TS-backend-3, TS-route-1, TS-route-2 |
| FR6 (Key event interception) | 4-G-A (基盤) + 各 OS phase (実装) | TS-backend-4, TS-backend-5, TS-x11-1, TS-x11-2 |
| FR7 (Cursor rectangle reporting) | 4-G-A (基盤) + 各 OS phase (送信) | TS-cursor-1 + manual host gates |
| FR8 (Focus management) | 4-G-A (基盤) + 各 OS phase (OS 通知) | TS-focus-1 + manual host gates |
| FR9 (Opt-out / fallback) | 4-G-A | TS-fallback-1, TS-fallback-2, TS-fallback-3, TS-manual-ime-fallback |
| FR10 (Settings additions) | 4-G-A | TS-settings-1 |

### Non-Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| NFR1 (preedit redraw < 30 ms) | 4-G-E | TS-perf-3 |
| NFR2 (commit → PtySession::write < 5 ms) | 4-G-E | TS-perf-4 |
| NFR3 (IME-OFF regression ≤ +10%) | 4-G-E | TS-perf-regression |
| NFR4 (Stability: init failure no crash, IM server death falls back within 1 tick) | 4-G-A (fallback) + 4-G-B/C/D (transport error 検出) | TS-fallback-3, TS-manual-ime-imserver-restart |
| NFR5 (Workspace compatibility, src-tauri untouched) | 全 phase | `cargo build/test --workspace` を各 sub-phase 完了時に確認 |
| NFR6 (Module layout) | 全 phase | File Structure Verification セクション + `preedit.rs` / `commit.rs` 不変確認 |
| NFR7 (Logging: init success / fallback / reconnect) | 全 phase | Manual log inspection during manual gates |
| NFR8 (Linux fcitx5 parity with Phase 1) | 4-G-B | TS-manual-ime-x11 で Phase 1 SPEC (`doc/tasks/ime-input-support/SPEC.md` US1-US5) と差分確認 |

## E2E Testing

既存 `./scripts/run-e2e-docker.sh` は legacy Tauri build 専用で native-poc には適用外。Phase 4-G では new E2E は追加しない。

- [ ] Legacy E2E (`./scripts/run-e2e-docker.sh test`) が `main` と同じ preexisting fail list を示すこと (Phase 4-G による regression がないことの確認、gate ではない)

## Manual Testing (E2E Not Possible)

- [ ] **TS-manual-ime-x11** (Linux X11 + fcitx5 host): native-poc を X11 で起動、`Ctrl+Space` で fcitx5 トグル、"nihongo" 入力 → underline preedit overlay 表示 → `Space` で変換 → `Enter` で確定、シェルに "日本語" が exactly 1 回届く。`Ctrl+C` / 矢印 / `Esc` / `Tab` は composition 中でも従来動作
- [ ] **TS-manual-ime-x11-ibus** (Linux X11 + IBus host): 上記と同じフローを IBus で実施、XIM client が両 IM サーバで動作することを確認
- [ ] **TS-manual-ime-wayland** (Linux Wayland + fcitx5-wayland): KDE Plasma 6 (KWin) + Sway の 2 環境で同じフローを実施
- [ ] **TS-manual-ime-windows** (Windows + MS-IME / Google IME): 同じフロー。候補ウィンドウがカーソル近傍に出る (best effort、gating ではない)
- [ ] **TS-manual-ime-fallback** (任意 host): `EMTERM_NATIVE_IME=0` で起動、warn ログ 1 回 + Phase 4 fallback 動作 (preedit overlay なし、ASCII キー入力は `ReceivedImeText` 経由で PTY 到達)
- [ ] **TS-manual-ime-imserver-restart** (Linux X11): fcitx5 を kill → warn ログ + 自動 fallback、fcitx5 を再起動 → native-poc を blur / refocus → IME 再 attach
- [ ] **TS-manual-ime-mux** (Linux X11 + fcitx5 + emterm mux): `emterm mux attach` 中の session で日本語入力、commit が mux 経由 PTY に届く (Phase 4-C の APC inband path に regression なし)

## Performance Verification

- TS-perf-3: preedit key 押下 → overlay redraw < 30 ms (Linux X11 release host)。`App::on_ime_preedit` 入口 + `WindowHost::request_redraw` を `Instant::now()` で挟み記録
- TS-perf-4: `App::on_ime_commit` 入口 → `PtySession::write` 完了まで < 5 ms (release host)
- TS-perf-regression: IME-OFF 時の key-down → PTY write latency。Phase 4 `TS-perf-1` / `TS-perf-2` の baseline (`doc/tasks/mux-tabs-windows-ime/VERIFICATION_RESULT.md` 記録値) を取得し、本 phase 計測値が +10% 以内であることを確認

## Security Verification

- [ ] preedit / commit テキストは既存 `ime::preedit::sanitize` を経由 (C0/C1 strip)。TS-route-1 / TS-route-2 で regression guard
- [ ] commit は bracketed-paste で wrap しない (`ime::commit::write_commit` 既存契約)。TS-route-2
- [ ] UTF-16 → UTF-8 変換は invalid surrogate を drop + warn (TS-windows-3)
- [ ] backend `Drop` で IM リソース解放 (`XDestroyIC` + `XCloseIM`、`zwp_text_input_v3::destroy`、`RemoveWindowSubclass`)。手動確認: 設定変更で backend 再生成しても leak しない
- [ ] `Settings::default().ime.native_integration` が `true` (TS-settings-1)。Phase 7 で JSON ローダが実装された時に未知値が warn + デフォルト fallback される仕様は Phase 7 SDD のスコープ

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit (4-G-A) | 12 (TS-backend-1..5, TS-cursor-1, TS-focus-1, TS-fallback-1..3, TS-settings-1, TS-route-1..2) | 12 | 0 | 0 |
| Unit (4-G-B) | 2 (TS-x11-1, TS-x11-2) | 2 | 0 | 0 |
| Unit (4-G-C) | 2 (TS-wayland-1, TS-wayland-2) | 2 | 0 | 0 |
| Unit (4-G-D) | 3 (TS-windows-1, TS-windows-2, TS-windows-3) | 3 | 0 | 0 |
| Integration | 2 (TS-backend-int-1, TS-backend-int-2; `#[ignore]` で host gate) | 2 | 0 | 0 |
| Performance | 3 (TS-perf-3, TS-perf-4, TS-perf-regression) | 0 | 0 | 3 |
| Manual | 7 (TS-manual-ime-x11, x11-ibus, wayland, windows, fallback, imserver-restart, mux) | 0 | 0 | 7 |
| Legacy regression | 1 (legacy E2E preexisting fail list) | 0 | 1 | 0 |
| **Total** | **32** | **21** | **1** | **10** |
