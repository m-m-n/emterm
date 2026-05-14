# Implementation Plan: Native IME Integration (Phase 4-G, **redesigned**)

## Overview

Phase 4-G の **redesign** 後の実装計画。tao 0.34 + 自前 XIM / Wayland / IMM32 backend を捨て、`winit 0.30.9` に移行する。Phase 4-E の auto-scope (`ime::preedit::State` / `ime::commit::write_commit` / `App::on_ime_*` / `render::cursor::draw_cursor_with_preedit`) は不変、`ImeBackend` trait + `NullBackend` + settings (`ImeSettings`) は保持。新規 `WinitImeBridge` が winit の `WindowEvent::Ime` を Phase 4-E ルーティングに流す。

## Objectives

- 自前 XIM 関連ファイル / 依存をすべて削除し、`cargo test --workspace` green を維持
- tao 0.34 → winit 0.30.9 へ event loop / window / wgpu surface / raw-window-handle 0.6 経路を移行
- winit `WindowEvent::Ime` を Phase 4-E ルーティング層に薄く接続するブリッジ実装
- Ghostty 由来のステートマシン (`im_composing` + `in_keyevent`) で commit と key event の二重消費を防ぐ
- Linux X11 + fcitx5 / IBus、Linux Wayland + fcitx5-wayland、Windows + MS-IME / Google IME の manual gate を winit 経路で達成
- Phase 4-E auto-scope ファイル diff empty を維持
- 旧 `src-tauri` build / test を一切触らない

## Prerequisites

### Development Environment

- 既存 Rust workspace + Docker E2E イメージ (`docker compose -f docker-compose.e2e.yml`)
- 実機 manual gate 用:
  - Linux X11 + fcitx5 / IBus
  - Linux Wayland + fcitx5-wayland (KDE Plasma 6 または Sway)
  - Windows 10/11 + MS-IME / Google 日本語入力

### Dependencies

- 前 Phase 4-G コミット (6 コミット, `9f290ab..dbfb25b`) が landed 済みであること。`native-poc/src/ime/{backend,null,x11,wayland,windows}.rs` が存在し `cargo test --workspace` が 2011 件 green。本 redesign はそこから減算 + winit 追加で再構築する
- 以下は変更しない:
  - `native-poc/src/ime/{preedit.rs, commit.rs, mod.rs(preedit/commit re-export 部分)}`
  - `App::on_ime_preedit / on_ime_commit / on_ime_focus_lost` のシグネチャ
  - `render::cursor::draw_cursor_with_preedit`
  - `native-poc/src/settings.rs` の `ImeSettings { native_integration: bool }`
- `raw-window-handle` 0.6 が workspace 依存にあること

## Architecture Overview

### Technology Stack

- **Language**: Rust (workspace pinned)
- **Window / Event loop**: **winit 0.30.9** (`default-features = false, features = ["rwh_06", "x11", "wayland"]`) — tao 0.34 から置換
- **IME 統合**: winit `WindowEvent::Ime { Enabled, Preedit, Commit, Disabled }` (X11 / Wayland / Windows 統合カバー)
- **Key libraries (既存)**: `raw-window-handle`, `crossbeam-channel`, `log`

### Design Approach

トップダウン + 段階リリース。

1. まず自前 XIM 関連を削除し (cleanup)、NullBackend のみで動作する状態に戻す
2. 次に tao → winit に windowing を移行 (IME 接続なし、素の打鍵のみ確認)
3. 続いて `WinitImeBridge` を新規実装し、Ghostty 由来のステートマシンで winit IME を Phase 4-E に接続
4. 最後に manual gate を winit 経路で再実施

各段階の Go / No-Go は `cargo test --workspace` green + (3) 以降は手動 IME gate で判定する。Phase 4-E sanitize / write_commit / focus_lost ルーティングはすべて不変。

### Component Interaction

```
[winit::EventLoop]
  WindowEvent::KeyboardInput { state: Pressed | Released } ─▶ ImeBackend::dispatch_key_event
                                    ├─ im_composing == true → Consumed (skip winit_key_to_bytes)
                                    └─ im_composing == false → Passthrough → winit_key_to_bytes
  WindowEvent::Ime(Enabled)        ─▶ WinitImeBridge::on_winit_ime → im_composing = true
  WindowEvent::Ime(Preedit(text))  ─▶ queue.push(ImeEvent::Preedit(text))
  WindowEvent::Ime(Commit(text))   ─▶ queue.push(ImeEvent::Commit(text)) + im_composing = false
  WindowEvent::Ime(Disabled)       ─▶ queue.push(ImeEvent::FocusOut) + im_composing = false
  WindowEvent::Focused(b)          ─▶ ImeBackend::notify_focus(b) + (b == false) App::on_ime_focus_lost

[event-loop tick]
  ImeBackend::pump(&mut events) → App drains:
    ImeEvent::Preedit(s)   → App::on_ime_preedit(&s)
    ImeEvent::Commit(s)    → App::on_ime_commit(&s)
    ImeEvent::FocusOut     → App::on_ime_focus_lost()

[App per-frame cursor diff]
  cursor cell (row, col) 変化時 → ImeBackend::notify_cursor_rect(x, y, w, h)
                                  → Window::set_ime_cursor_area(...)
```

## Implementation Phases

### Sub-Phase 4-G-1: Cleanup (自前 XIM 関連削除)

**Goal**: 自前 XIM / Wayland / IMM32 のコードと依存を削除し、`cargo test --workspace` green を維持する。

**Files to Delete**:
- `native-poc/src/ime/x11.rs` (約 470 行 + テスト)
- `native-poc/src/ime/wayland.rs` (約 280 行 + テスト)
- `native-poc/src/ime/windows.rs` (約 320 行 + テスト)

**Files to Modify**:
- `native-poc/src/ime/mod.rs` — `pub mod x11; pub mod wayland; pub mod windows;` 削除 (`backend` / `null` / `preedit` / `commit` 再エクスポートは保持)
- `native-poc/src/ime/backend.rs` — `build_backend` factory の `RawDisplayHandle::Xlib` / `Wayland` probe + `#[cfg(windows)]` 経路を削除。当面は無条件で `Err(ImeInitError::Unavailable("no platform backend; pending winit bridge"))` → NullBackend に倒す
- `native-poc/Cargo.toml` — 以下の deps を削除:
  - `[target.'cfg(all(unix, not(target_os = "macos")))'.dependencies]` 配下の `x11-dl`, `wayland-client`, `wayland-protocols`
  - `[target.'cfg(windows)'.dependencies]` 配下の `windows`
- `native-poc/src/window_host.rs` — `RawDisplayHandle` / `RawWindowHandle` を backend factory に渡している箇所は **保持** (winit 移行後に再利用)、ただし backend factory 呼出引数は temporary に dummy または保留扱い

**Key Components Removed**:

| Component | Reason |
|-----------|--------|
| `X11Backend` (XOpenIM/XCreateIC/XFilterEvent/XmbLookupString + synthetic XKeyEvent + XICAttribute) | tao が XKB keycode を公開しないので XmbLookupString が 0 chars を返す |
| `WaylandBackend` (zwp_text_input_v3 scaffold + pump thread + crossbeam_channel) | winit が zwp_text_input_v3 をネイティブ処理する |
| `WindowsBackend` (SetWindowSubclass + WM_IME_* + ImmGetCompositionStringW + utf16_to_utf8) | winit が IMM32 をネイティブ処理する |

**Tests Removed**:

- `TS-x11-1`, `TS-x11-2` (11 cases) — keycode / modifier mapping
- `TS-wayland-1`, `TS-wayland-2` (10 cases) — pump drain / HandleType
- `TS-windows-1`, `TS-windows-2`, `TS-windows-3` (10 cases) — utf16_to_utf8
- `TS-backend-int-1`, `TS-backend-int-2` (`#[ignore]` integration)
- `TS-manual-ime-*` のうち自前 XIM 起源項目は **Phase 4-G-4 で winit 経路で再定義**

合計 削除: 約 31 unit tests + 2 integration tests。

**Tests Retained**:

- `TS-backend-1..5`, `TS-cursor-1`, `TS-focus-1`, `TS-fallback-1..3`, `TS-settings-1`, `TS-route-1..2` (Phase 4-G-A 由来の backbone tests、全 12 件)

**Implementation Steps**:

1. **ファイル削除** — `git rm native-poc/src/ime/{x11,wayland,windows}.rs`
2. **mod.rs から該当 `pub mod` を削除** — `backend` / `null` / `preedit` / `commit` の再エクスポートのみ残す
3. **`backend.rs` の factory を簡略化** — `build_backend(_window, _display, settings, env)` は引き続き受け取るが、本体は env / settings check のあと最終的に `Err(ImeInitError::Unavailable("winit bridge not yet wired (Phase 4-G-3)"))` → `NullBackend` で warn 一発
4. **`Cargo.toml` 依存削除** — `x11-dl`, `wayland-client`, `wayland-protocols`, `windows` を削除
5. **`window_host.rs` の backend factory 呼出引数は保留** — winit 移行で再利用するので削除しない、ただし `RawDisplayHandle::Xlib` / `Wayland` の compile-time 依存が消えるので import を整理
6. **`cargo build --workspace` / `cargo test --workspace` green を確認** — workspace test 数は 2011 - (31 + 2) ≒ 1978 程度に減る (NullBackend / route / settings / fallback / cursor / focus / backbone は維持)

**Dependencies**: 前 Phase 4-G が landed 済みであること

**Testing Approach**:
- Unit: 保持テスト 12 件すべて green
- Integration: 削除済
- E2E: 該当なし
- Manual: 該当なし (Phase 4-G-4 でまとめて再実施)

**Acceptance Criteria**:
- [ ] `git ls-files native-poc/src/ime/` に `x11.rs`, `wayland.rs`, `windows.rs` が存在しない
- [ ] `grep -E 'x11-dl|wayland-client|wayland-protocols|"windows"' native-poc/Cargo.toml` が一致なし
- [ ] `cargo test --workspace` exit 0、新規 ±0 件 (保持テストのみ)
- [ ] `cargo fmt --all -- --check` clean
- [ ] Phase 4-E の `ime::{preedit, commit}` ファイル content 不変

**Estimated Effort**: small (削除中心 + factory 微調整)

---

### Sub-Phase 4-G-2: winit 移行 (windowing only)

**Goal**: tao 0.34 → winit 0.30.9 に windowing を移行。IME は NullBackend のままで、素の打鍵 + winit `WindowEvent::ReceivedCharacter` 相当 (winit 0.30 では `Ime::Commit` 経路) が動くことを確認する。**IME backend には接続しない**。

**Files to Modify**:
- `native-poc/Cargo.toml` — `tao = "0.34"` 削除、`winit = "0.30.9"` 追加 (`default-features = false, features = ["rwh_06", "x11", "wayland"]`)
- `native-poc/src/main.rs` — `tao::EventLoop` → `winit::EventLoop`、`EventLoop::run` クロージャまたは `ApplicationHandler` trait に書き換え
- `native-poc/src/window_host.rs` — 約 200 行の書き換え:
  - `tao::window::WindowBuilder` → `winit::window::WindowAttributes` + `EventLoop::create_window`
  - `tao::event::Event` → `winit::event::Event`
  - `tao::event::WindowEvent` → `winit::event::WindowEvent`
  - `tao::event::KeyEvent` → `winit::event::KeyEvent`
  - `tao::keyboard::Key` → `winit::keyboard::Key`
  - `tao::event::ElementState` → `winit::event::ElementState`
  - `tao::event::WindowEvent::ReceivedImeText(text)` → 当面無視 (winit 0.30 ではこの variant が存在しない、`Ime::Commit` が代替で Phase 4-G-3 で接続)
  - `tao::dpi::PhysicalSize` → `winit::dpi::PhysicalSize`
  - `raw-window-handle 0.6` 経由の wgpu surface 作成は維持 (`HasWindowHandle` / `HasDisplayHandle` trait の trait bound は両方互換)
- `native-poc/src/app.rs` — `tao::event::KeyEvent` 参照を `winit::event::KeyEvent` に置換、`tao_key_to_bytes` を `winit_key_to_bytes` に rename + 内部の modifier mapping を winit 版に rewrite
- `native-poc/src/ime/backend.rs` — `RawKeyEvent::tao_event: &'a tao::event::KeyEvent` → `winit_event: &'a winit::event::KeyEvent`
- `native-poc/Cargo.toml` — `tao` 削除 (workspace dep でない場合)、egui-tao 等 tao を transitive に引きずる依存があるか確認 (本 Phase で wgpu に直接 surface を作る形に変更されている前提)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `winit::EventLoop` | event loop の root | n/a | tao::EventLoop と同等の挙動 |
| `winit::window::Window` | window handle (`Arc<Window>` で wgpu surface と共有) | event loop 作成済み | tao::Window と同等の挙動 |
| `winit_key_to_bytes` | winit::KeyEvent → PTY bytes 変換 | KeyboardInput 到達 | tao_key_to_bytes と同等の出力 (modifier ガード `!ctrl && !alt` の早期 None は維持) |
| wgpu surface 作成 | `wgpu::Instance::create_surface(&window)` 経由で raw-window-handle 0.6 を使う | window 作成済み | tao 版と同等の surface |

**Processing Flow** (素の打鍵):

1. winit `EventLoop::run` がイベントを poll
2. `WindowEvent::KeyboardInput { state: Pressed, event, .. }` → `ImeBackend::dispatch_key_event` (現状 NullBackend なので常に `Passthrough`)
3. `Passthrough` → `winit_key_to_bytes(event, modifiers)` → PTY write
4. `WindowEvent::Ime(_)` は当面無視 (NullBackend が `set_ime_allowed(false)` 相当の挙動で発火しない、または発火しても backend が受け取らない)

**Implementation Steps**:

1. **Cargo.toml の置換** — `tao = "0.34"` 削除 + `winit = "0.30.9"` 追加
2. **main.rs の event loop 立ち上げ書き換え** — `winit::EventLoop::new()` (Result) + `event_loop.run_app(app)` または `event_loop.run(|event, target| { ... })`
3. **window_host.rs の event 種別マッピング表に従う書き換え** — `tao::event::WindowEvent` の各 variant を winit 対応 variant に対応付け
4. **`winit_key_to_bytes` rewrite** — winit `KeyEvent::logical_key` (`Key::Character(_)` / `Key::Named(_)`) + `Modifiers` の組み合わせから PTY bytes を生成。tao 版の `!mods.ctrl && !mods.alt && physical == Character` 早期 None ガードを保持
5. **wgpu surface 作成検証** — `Arc<Window>` を共有して `Instance::create_surface(window.clone())` で surface 作成が成功すること
6. **build / test** — `cargo build --workspace` + `cargo test --workspace` exit 0、既存テストは backbone 12 件のみ green でよい

**Dependencies**: 4-G-1 完了

**Testing Approach**:
- Unit: 既存 12 件が green を維持。新規追加なし (winit_key_to_bytes 単体テストは tao_key_to_bytes 由来の既存テストを rename + 期待値見直しで対応、件数±0)
- Integration: 該当なし
- E2E: 該当なし
- Manual: host-deferred (Phase 4-G-4 で実施)

**Acceptance Criteria**:
- [ ] `cargo build --workspace` exit 0
- [ ] `cargo test --workspace` exit 0
- [ ] `cargo fmt --all -- --check` clean
- [ ] `native-poc/Cargo.toml` に `tao` 依存がない、`winit = "0.30.9"` がある
- [ ] 起動時に native-poc が winit 経由でウィンドウを表示し、素の英数字打鍵が PTY に届く (host-deferred 確認、ただしユニットレベルで `winit_key_to_bytes` の出力が一致する)
- [ ] Phase 4-E ファイル diff empty を維持

**Estimated Effort**: large (約 200 行の書き換え + wgpu surface 整合性確認)

---

### Sub-Phase 4-G-3: winit IME bridge 実装

**Goal**: `WinitImeBridge` を新規実装し、winit `WindowEvent::Ime` を `ImeEvent` に変換して App::on_ime_* に接続する。Ghostty 由来のステートマシン (`im_composing` + `in_keyevent`) で commit と key event の二重消費を防ぐ。

**Files to Create**:
- `native-poc/src/ime/winit_bridge.rs` — `WinitImeBridge` 本体 (winit Ime → ImeEvent 変換 + ステートマシン + `set_ime_cursor_area` 呼出)

**Files to Modify**:
- `native-poc/src/ime/mod.rs` — `pub mod winit_bridge;` 追加
- `native-poc/src/ime/backend.rs` — `build_backend` factory が `Arc<winit::window::Window>` を引数に取り、`WinitImeBridge::init(window)` を呼ぶ。`ImeInitError::Unavailable` 時は NullBackend にフォールバック
- `native-poc/src/window_host.rs` — `WindowEvent::Ime(ime)` を `WinitImeBridge::on_winit_ime(&ime)` に転送、`KeyboardInput` の press / release 両方を `dispatch_key_event_via_ime` に通す、`Window::set_ime_cursor_area` を `notify_cursor_rect` 経由で呼ぶ
- `native-poc/src/app.rs` — `dispatch_key_event_via_ime` がリリースキーも通すよう確認 (既存実装は press のみだったかもしれないため見直し)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `WinitImeBridge::init(window: Arc<Window>)` | `Window::set_ime_allowed(true)` を呼んで IME を enable | window 作成済み | bridge instance ready、winit IME path 有効 |
| `WinitImeBridge::on_winit_ime(&Ime)` | winit `Ime` variant を `ImeEvent` にマップ、`im_composing` フラグ更新 | bridge active | queue に `ImeEvent` が積まれる |
| `WinitImeBridge::dispatch_key_event(raw)` | `im_composing` が true なら `Consumed`、false なら `Passthrough` を返す | dispatch_key_event_via_ime から呼ばれる | state machine による振り分け |
| `WinitImeBridge::pump(events)` | queue を drain | n/a | events vec に push (最大 1024 件/tick、超過は warn IME_E901) |
| `WinitImeBridge::notify_cursor_rect` | `last_cursor_rect` と異なる時のみ `Window::set_ime_cursor_area` 呼出 | bridge active | 候補ウィンドウがカーソル近傍に出る |
| `WinitImeBridge::notify_focus(b)` | `Window::set_ime_allowed(b)` で IME を on/off | bridge active | focus 取得時に IME が attach、focus 喪失時に detach |
| `WinitImeBridge::drop` | `Window::set_ime_allowed(false)` | bridge active | winit が IC / subclass を自動解放 |

**Processing Flow** (preedit + commit):

1. ユーザーがトグルキー (Ctrl+Space / 半角/全角) を押す
2. winit が `WindowEvent::Ime(Ime::Enabled)` を発火
3. `WinitImeBridge::on_winit_ime` が `im_composing = true` をセット
4. ユーザーが "nihongo" と打鍵 → winit が `WindowEvent::KeyboardInput` + `WindowEvent::Ime(Ime::Preedit("にほんご", _))` を発火
5. `dispatch_key_event` は `im_composing == true` なので `Consumed` を返し、`winit_key_to_bytes` は呼ばれない
6. `on_winit_ime(Ime::Preedit(_))` が `ImeEvent::Preedit("にほんご")` を queue に積む
7. tick 終端で `pump` が drain → `App::on_ime_preedit("にほんご")` → Phase 4-E `sanitize` → `preedit::State::set`
8. ユーザーが Space で変換、Enter で確定 → winit が `WindowEvent::Ime(Ime::Commit("日本語"))` を発火
9. `on_winit_ime` が `ImeEvent::Commit("日本語")` を queue に積み、`im_composing = false`
10. tick 終端で `pump` → `App::on_ime_commit("日本語")` → `commit::write_commit` → PTY

**Processing Flow** (modifier 単独 release):

1. ユーザーが Shift キー単独を押して離す (fcitx5 では IM 切替に使われる場合あり)
2. winit が `WindowEvent::KeyboardInput { state: Released, logical_key: Key::Named(NamedKey::Shift), .. }` を発火
3. `dispatch_key_event` がこの release event も処理 (Ghostty 流: press / release 両方通す)
4. `im_composing == false` の場合は `Passthrough` を返し、`winit_key_to_bytes` は modifier 単独 release では何も出さない (既存ガードと整合)

**Processing Flow** (IM サーバ突然死):

1. fcitx5 が kill された / クラッシュした
2. winit / X11 / Wayland 経由で `WindowEvent::Ime(Ime::Disabled)` が発火
3. `on_winit_ime(Ime::Disabled)` が `ImeEvent::FocusOut` を queue に積み、`im_composing = false`
4. tick 終端で `pump` → `App::on_ime_focus_lost` → preedit クリア
5. fcitx5 を再起動 → ユーザーが native-poc を blur / refocus → winit が `Ime::Enabled` を再発火、復帰

**Implementation Steps**:

1. **`winit_bridge.rs` 新規作成** — `WinitImeBridge` 構造体 + `ImeBackend` trait 実装 + state machine 内部関数
2. **`backend.rs::build_backend` 改修** — env / settings check のあと `WinitImeBridge::init(window)` を呼ぶ。失敗時は NullBackend + warn 一発
3. **`window_host.rs` で IME event を bridge にルーティング** — `WindowEvent::Ime(ime)` → `bridge.on_winit_ime(&ime)`
4. **`KeyboardInput` の release も dispatch_key_event_via_ime に通す** — Ghostty 流のステートマシンを成立させるため
5. **`Window::set_ime_cursor_area` の呼出経路を確認** — `App::notify_cursor_rect_if_changed` → `bridge.notify_cursor_rect` → `window.set_ime_cursor_area(Position, Size)`
6. **`TS-winit-1..7` ユニットテスト** — bridge の state machine 単体テスト

**Dependencies**: 4-G-2 完了

**Testing Approach**:
- Unit: TS-winit-1..7 (7 件)、既存 12 件は green を維持
- Integration: TS-winit-int-1 (`#[ignore]`、Linux X11 Xvfb 上で `Ime::Disabled` 通知が来ることを確認)、TS-winit-int-2 (`#[cfg(windows)]`、host-deferred)
- E2E: 該当なし
- Manual: Phase 4-G-4 でまとめて実施

**Acceptance Criteria**:
- [ ] `native-poc/src/ime/winit_bridge.rs` が存在し、`ImeBackend` trait を実装
- [ ] `cargo test --workspace` exit 0、新規 +7 件以上
- [ ] `cargo clippy -p emterm-native-poc -- -D warnings` clean
- [ ] `EMTERM_NATIVE_IME=0` で起動 → NullBackend + warn 一発 (回帰なし)
- [ ] `Settings::default()` ime.native_integration = false → NullBackend
- [ ] Phase 4-E ファイル diff empty を維持

**Estimated Effort**: medium (実装は小さいが state machine の検証 + winit_bridge 単体テストで時間が必要)

---

### Sub-Phase 4-G-4: Manual gate 再実施

**Goal**: TS-manual-ime-* を winit 経路で再実施し、Phase 1 WebView IME parity (NFR8) を達成。VERIFICATION_RESULT.md に結果を追記。

**Files to Modify**:
- `doc/tasks/ime-native-integration/VERIFICATION_RESULT.md` — winit 経路での manual gate 結果を追記 (sdd.6-verify が記録)
- `native-poc/README.md` — Phase 4-G feature matrix を winit 採択方針に更新

**Manual Gates**:

1. **TS-manual-ime-x11** (Linux X11 + fcitx5 host)
   - `EMTERM_NATIVE_IME=1` (default) で起動
   - Ctrl+Space で fcitx5 トグル、"nihongo" → "日本語" 変換
   - 期待: preedit overlay 表示、`Enter` で commit が PTY に exactly once 届く
   - Ctrl+C / 矢印 / Esc / Tab がパススルー
2. **TS-manual-ime-x11-ibus** (Linux X11 + IBus host) — 同上を IBus で
3. **TS-manual-ime-wayland** (Linux Wayland + fcitx5-wayland) — KDE Plasma 6 + Sway の 2 環境
4. **TS-manual-ime-windows** (Windows + MS-IME / Google IME)
5. **TS-manual-ime-fallback** (任意 host with `EMTERM_NATIVE_IME=0`) — warn 一発 + fallback 動作
6. **TS-manual-ime-imserver-restart** (Linux X11) — fcitx5 kill → `Ime::Disabled` warn log → 再起動 + refocus で復帰
7. **TS-manual-ime-mux** (Linux X11 + fcitx5 + `emterm mux attach`) — mux session 内での IME 動作

**Performance Gates**:

- **TS-perf-3**: preedit redraw < 30 ms (Linux X11 release host, `EMTERM_IME_PERF=1`)
- **TS-perf-4**: commit → `PtySession::write` < 5 ms
- **TS-perf-regression**: IME-OFF key-down → PTY write が Phase 4 baseline +10% 以内 (winit 移行込み)

**Implementation Steps**:

1. **release build を host で実行** — `cargo build --release -p emterm-native-poc` (Linux / Windows)
2. **Linux X11 + fcitx5 host で gate 1, 2, 6, 7 を実施** — 結果を VERIFICATION_RESULT.md に追記
3. **Linux Wayland host で gate 3 を実施**
4. **Windows host で gate 4 を実施** (cross-build または windows-latest CI)
5. **任意 host で gate 5 を実施**
6. **TS-perf-3 / TS-perf-4 / TS-perf-regression を `EMTERM_IME_PERF=1` で計測** — warn-log から μs 値を抽出
7. **README.md 更新** — Phase 4-G feature matrix を winit 採択方針に書き換え

**Dependencies**: 4-G-3 完了

**Testing Approach**:
- Unit / Integration: 再 run (regression 確認)
- E2E: 該当なし
- Manual: 全 7 manual gate + 3 perf gate

**Acceptance Criteria**:
- [ ] TS-manual-ime-x11 / x11-ibus / wayland / windows / fallback / imserver-restart / mux すべて pass
- [ ] TS-perf-3 (< 30 ms), TS-perf-4 (< 5 ms), TS-perf-regression (+10% 以内) 達成
- [ ] Phase 1 fcitx5 acceptance criteria (`doc/tasks/ime-input-support/SPEC.md` US1-US5) パス (NFR8)
- [ ] README.md Phase 4-G matrix が winit 採択方針に更新

**Estimated Effort**: medium (host 実機作業)

---

## Complete File Structure

```
native-poc/
├── Cargo.toml                           # MODIFY:
│                                        #   ADD:    winit = "0.30.9" (default-features=false, features=["rwh_06","x11","wayland"])
│                                        #   REMOVE: tao = "0.34"
│                                        #   REMOVE: x11-dl, wayland-client, wayland-protocols (Linux)
│                                        #   REMOVE: windows (Windows)
│                                        #   KEEP:   raw-window-handle, crossbeam-channel
├── README.md                            # MODIFY (4-G-4): Phase 4-G winit feature matrix
└── src/
    ├── main.rs                          # MODIFY (4-G-2): tao::EventLoop → winit::EventLoop
    ├── app.rs                           # MODIFY (4-G-2): tao 型参照を winit に置換 (本体は不変)
    ├── window_host.rs                   # MODIFY (4-G-2 + 4-G-3): tao API → winit API + WindowEvent::Ime ルーティング
    ├── settings.rs                      # UNCHANGED: ImeSettings は Phase 4-G オリジナル維持
    └── ime/
        ├── mod.rs                       # MODIFY (4-G-1 + 4-G-3): x11/wayland/windows mod 削除、winit_bridge 追加
        ├── preedit.rs                   # UNCHANGED (Phase 4-E auto-scope)
        ├── commit.rs                    # UNCHANGED (Phase 4-E auto-scope)
        ├── backend.rs                   # MODIFY (4-G-1 + 4-G-3): factory probe を winit に集約
        ├── null.rs                      # UNCHANGED
        ├── winit_bridge.rs              # NEW (4-G-3): WinitImeBridge + state machine
        ├── x11.rs                       # DELETED (4-G-1)
        ├── wayland.rs                   # DELETED (4-G-1)
        └── windows.rs                   # DELETED (4-G-1)
```

## Testing Strategy

- **Unit**: backend 抽象 / NullBackend / settings / fallback / route guard (Phase 4-G-A 由来 12 件保持) + winit_bridge state machine (新規 7 件)
  - 保持: TS-backend-1..5, TS-cursor-1, TS-focus-1, TS-fallback-1..3, TS-settings-1, TS-route-1..2 (4-G-A 起源、全 phase で green)
  - 新規: TS-winit-1..7 (4-G-3)
  - 削除: TS-x11-1..2, TS-wayland-1..2, TS-windows-1..3 (4-G-1 で削除)
- **Integration**: 実 OS / IM サーバ依存があるものは `#[ignore]` 付き harness
  - 新規: TS-winit-int-1 (Linux X11 Xvfb, `#[ignore]`) — 4-G-3
  - 新規: TS-winit-int-2 (`#[cfg(windows)]`, host-deferred) — 4-G-3
  - 削除: TS-backend-int-1, TS-backend-int-2 (4-G-1 で削除)
- **E2E**: 既存 `./scripts/run-e2e-docker.sh` は legacy Tauri 向けで native-poc には適用外
- **Manual**: host 実機 gate (Phase 4-G-4 でまとめて再実施)
  - TS-manual-ime-x11, TS-manual-ime-x11-ibus, TS-manual-ime-wayland, TS-manual-ime-windows, TS-manual-ime-fallback, TS-manual-ime-imserver-restart, TS-manual-ime-mux (winit 経路で再定義)
  - TS-perf-3, TS-perf-4, TS-perf-regression

## Dependencies

| Package | Version | Purpose | Target |
|---------|---------|---------|--------|
| `winit` | 0.30.9 | event loop, window, IME 統合 | all |
| `raw-window-handle` | 0.6 (既存 direct dep) | window / display handle | all |
| `crossbeam-channel` | (既存) | 既存用途のみ (Wayland pump thread は不要) | all |

**削除**:
- `tao` 0.34 (winit に置換)
- `x11-dl` 2 (winit が X11 IME を内部実装)
- `wayland-client` 0.31 (同上 Wayland)
- `wayland-protocols` 0.31 (同上)
- `windows` 0.58 (winit が IMM32 を内部実装)

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| winit 0.30 への移行で wgpu surface 作成手順が変わる | 中 | 高 | raw-window-handle 0.6 経由なので `Arc<Window>` を直接 surface に渡せる。winit / wgpu 公式 example で確認 |
| winit `KeyEvent` → PTY bytes 変換で tao 版と微妙に異なる挙動 | 中 | 中 | 既存 `tao_key_to_bytes` のユニットテストを winit 版に rewrite し、同じ期待値で green を確認 |
| winit が compositor 都合で `Ime::Enabled` を発火しない (古い GNOME 等) | 中 | 中 | `set_ime_allowed(true)` を呼んだあと `Ime::Disabled` のみ来るケースを想定。bridge は state machine で空回りするだけ、ターミナルは動く。manual gate で確認 |
| winit 0.30 の API breaking changes (event loop の `ControlFlow` 廃止等) | 高 | 中 | winit 0.30 changelog を確認、`EventLoop::run_app` (ApplicationHandler trait) または `EventLoop::run` クロージャを採択 |
| Ghostty 由来 state machine の `Ime::Commit` 直後の release suppress が壊れる | 中 | 中 | `TS-winit-5` (idempotency) + `TS-winit-6` (modifier release) を pin。manual gate で二重入力なしを確認 |
| winit の `set_ime_cursor_area` シグネチャ (Position + Size) が tao の XICAttribute / IMM32 と異なる | 低 | 低 | winit doc を確認、`PhysicalPosition` + `PhysicalSize` をピクセル座標で渡す |
| Windows での winit IME が IMM32 か TSF か | 低 | 低 | winit 0.30 は IMM32 採択 (TSF は別途 PR で議論中)。MS-IME / Google IME はどちらでも動く |

## Open Questions

- [ ] **OQ1**: winit 0.30 の `ApplicationHandler` trait vs `EventLoop::run` クロージャ式 — どちらを採択するか。4-G-2 移行時に決定。`ApplicationHandler` のほうが clean だが、既存 `window_host.rs` の構造との fit を見て判断
- [ ] **OQ2**: tao を transitive に引きずる依存があるか (例: egui-tao 経由)。ある場合は 4-G-2 で同時に対処 (egui-wgpu 等に置換)
- [ ] **OQ3**: winit features 構成 — `x11` / `wayland` を明示 enable、`rwh_06` は必須、`serde` 不要。`mint` も不要
- [ ] **OQ4**: Wayland compositor 最低保証 — KDE Plasma 6 + Sway。GNOME は best effort (winit 経路でも同じ)
- [ ] **OQ5**: Windows TSF は本 redesign スコープ外。IMM32 (winit 経由) で不足が判明したら別 SDD
- [ ] **OQ6**: `notify_cursor_rect` のサブセル精度は不要 (cell-aligned で十分)

## Success Metrics

- [ ] FR4-FR13 すべて実装 + tests / manual gate でカバー (旧 FR1-FR3 は削除済)
- [ ] `cargo build --workspace` (Linux + Windows) exit 0
- [ ] `cargo test --workspace` exit 0
- [ ] `cargo fmt --all -- --check` clean
- [ ] `cargo clippy -p emterm-native-poc -- -D warnings` clean
- [ ] TS-manual-ime-x11 / TS-manual-ime-x11-ibus / TS-manual-ime-wayland / TS-manual-ime-windows / TS-manual-ime-fallback / TS-manual-ime-imserver-restart / TS-manual-ime-mux すべて pass (winit 経路)
- [ ] TS-perf-3 (< 30 ms), TS-perf-4 (< 5 ms), TS-perf-regression (+10% 以内) 達成
- [ ] Phase 4-E `ime::{preedit, commit}` / `render/cursor.rs` のファイル content が unchanged
- [ ] 旧 `src-tauri` build / test affected なし (NFR5)
- [ ] 旧 `ime::{x11, wayland, windows}` が `git ls-files` に存在しない
