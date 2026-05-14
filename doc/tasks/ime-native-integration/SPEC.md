# Feature: Native IME Integration (Phase 4-G, **redesigned**)

## Overview

Phase 4-G is being **redesigned** after the original tao 0.34 + 自前 XIM / Wayland / IMM32 実装が実機で動作しないことが判明したため。

**判明した実測ブロッカー (HEAD `dbfb25b`):**

- tao 0.34 が XKB keycode を公開しないため、自前 X11 backend が synthetic `XKeyEvent` を組み立てて `XmbLookupString` に渡しても **0 chars** が返り、通常の英数字すら PTY に届かない。
- tao 0.34 は IME を意図的にサポートしない (Tauri が WebView 側で IME を処理する設計のため、winit 0.27 系から fork した時点で `platform_impl/linux/x11/ime/` を削除済み、`set_ime_position` は `//TODO` の空実装)。
- 3 つの参考実装 (WezTerm / Alacritty / Ghostty、`tmp/research/{wezterm-ime,alacritty-winit-ime,ghostty-ime}.md`) は揃って「自前 XIM をやめて toolkit に任せろ」で結論一致。

**採択方針: 戦略 A (winit 0.30 移行)**

tao 0.34 を捨てて `winit 0.30.9` に乗り換える。winit は `WindowEvent::Ime { Enabled, Preedit, Commit, Disabled }` で X11 / Wayland / Windows をまとめて統合カバーする。Phase 4-E で配線済の `App::on_ime_{preedit, commit, focus_lost}` ルーティング層と `ime::preedit::State` / `ime::commit::write_commit` の不変契約はそのまま保持する。

**前 Phase 4-G コミット範囲 (`9f290ab..dbfb25b`、6 コミット) のうち、自前 XIM 関連は本 redesign で完全削除する。** 保持するのは backbone (trait / Null / preedit / commit / App ルーティング / settings) のみ。

## Objectives

- tao 0.34 → winit 0.30.9 移行 (event loop / window / wgpu surface / raw-window-handle 0.6 経路維持)
- winit の `WindowEvent::Ime` を Phase 4-E ルーティング層に薄く接続するブリッジ実装
- Ghostty 由来のステートマシン (`im_composing` + `in_keyevent`) で commit と key event の二重消費を防ぐ
- 自前 X11 / Wayland / Windows backend を完全削除し、依存 crate (`x11-dl`, `wayland-client`, `wayland-protocols`, `windows`) も除去
- Linux X11 + fcitx5 / IBus、Linux Wayland + fcitx5-wayland、Windows + MS-IME / Google IME の実機 manual gate を winit 経路で達成
- Phase 4-E auto-scope (`ime::preedit::State` / `ime::commit::write_commit` / `App::on_ime_*` / `render::cursor::draw_cursor_with_preedit`) を変更しない
- 旧 `src-tauri` の build / test を一切触らない

## User Stories

### US1: Linux X11 Japanese input via fcitx5
As a Linux X11 user, I want to compose Japanese in native-poc with fcitx5 so that I can write commit messages and shell commands in Japanese as smoothly as in the Phase 1 WebView build.

**Acceptance Criteria:**
- [ ] Toggle key (e.g. `Ctrl+Space`) turns fcitx5 on, and the next printable key starts a composition (winit `WindowEvent::Ime::Enabled` / `Preedit`).
- [ ] preedit text appears as an underline overlay anchored to the cursor cell (`render::cursor::draw_cursor_with_preedit`).
- [ ] On commit (winit `WindowEvent::Ime::Commit`), the bytes reach the active PTY exactly once and the overlay clears.
- [ ] Special chords (`Ctrl+C`, arrows, `Esc`, `Tab`) keep working during composition.

### US2: Linux Wayland Japanese input via fcitx5
As a Linux Wayland user, I want native-poc to receive IME events through winit's Wayland integration (`zwp_text_input_v3`) so that fcitx5-wayland / IBus deliver preedit + commit events without an extra protocol layer.

**Acceptance Criteria:**
- [ ] Same composition / commit / toggle behavior as US1, driven by winit's Wayland IME path.
- [ ] Cursor rectangle updates via `Window::set_ime_cursor_area` so the candidate window tracks the cursor.

### US3: Windows MS-IME Japanese input
As a Windows user, I want native-poc to receive IMM32 events through winit so that MS-IME / Google IME drive preedit + commit just like any other Win32 app.

**Acceptance Criteria:**
- [ ] winit `WindowEvent::Ime::Preedit` updates the preedit overlay.
- [ ] winit `WindowEvent::Ime::Commit` commits to the PTY exactly once and clears the overlay.
- [ ] `Window::set_ime_cursor_area` reports the cursor's pixel rect so the candidate window appears near the cursor (best effort; not gating).

### US4: Fallback when IME integration is unavailable
As any user, I want native-poc to keep working when I explicitly disable IME or when winit's IME path fails to enable.

**Acceptance Criteria:**
- [ ] `EMTERM_NATIVE_IME=0` (env) or `settings.json` `ime.native_integration = false` calls `Window::set_ime_allowed(false)` and falls back to `NullBackend`.
- [ ] In the disabled mode the terminal falls back to non-IME key dispatch and emits exactly one warn log.
- [ ] If winit fails to enable IME (e.g. compositor missing protocol), native-poc auto-falls back instead of crashing.

### US5: Focus loss clears stale composition
As any user, I want a stale preedit overlay to disappear when I tab away from native-poc.

**Acceptance Criteria:**
- [ ] `WindowEvent::Focused(false)` clears the active tab's preedit state (already wired in Phase 4-E).
- [ ] winit emits `WindowEvent::Ime::Disabled` on focus-out which routes through `WinitImeBridge` → `App::on_ime_focus_lost`.

## Technical Requirements

### Functional Requirements

#### Retained from previous Phase 4-G

- **FR4: `ImeBackend` trait surface** — `native_poc::ime::backend::ImeBackend` の trait surface (`ImeEvent`, `KeyDispatchResult`, `ImeInitError`, `RawKeyEvent`, factory パターン) は保持する。新ブリッジ `WinitImeBridge` が同 trait を実装する。
- **FR5: Routing into Phase 4-E layer** — Backend events route as: `ImeEvent::Preedit(text) → App::on_ime_preedit(&text)` / `ImeEvent::Commit(text) → App::on_ime_commit(&text)` / `ImeEvent::FocusOut → App::on_ime_focus_lost()`. これらのメソッドは Phase 4-E から不変。
- **FR6 (変更): Key event interception** — winit の `WindowEvent::KeyboardInput` と `WindowEvent::Ime` の整理。Ghostty 由来のステートマシンを適用し、press / release **両方** を `filterKeypress` 相当に通す (fcitx5 の modifier 単独 release で IM 切替を判定するため必須)。`im_composing` 中は KeyboardInput を PTY に流さず、commit / preedit イベントの到着を待つ。
- **FR7: Cursor rectangle reporting** — Whenever the cursor cell coordinate changes, the active backend's `notify_cursor_rect` is called with the cursor's pixel rect. `WinitImeBridge` 内では `Window::set_ime_cursor_area` を呼ぶ。Rate-limited to cell-change.
- **FR8: Focus management** — `WindowEvent::Focused(true)` calls `WinitImeBridge::notify_focus(true)` (内部で `Window::set_ime_allowed(true)`); `Focused(false)` calls `notify_focus(false)` + triggers `App::on_ime_focus_lost()`. winit が `WindowEvent::Ime::Disabled` を自動発火するのでそれも `FocusOut` として吸い上げる。
- **FR9: Opt-out / fallback** — `EMTERM_NATIVE_IME=0` (env) or `settings.json` `ime.native_integration = false` short-circuits bridge creation; the App holds a `NullBackend`. `WinitImeBridge::init` 失敗時もフォールバック。各フォールバック経路は warn 一発。
- **FR10: Settings additions** — `Settings::ime: ImeSettings { native_integration: bool }`、`Default::default()` で `native_integration: true`。Phase 7 JSON loader 委譲は不変。

#### New requirements (Phase 4-G redesign)

- **FR11: winit `WindowEvent::Ime` → `ImeEvent` 変換** — `WinitImeBridge` は winit の IME event 4 種を `ImeEvent` にマップする:
  - `WindowEvent::Ime(Ime::Enabled)` → 内部状態 `im_composing = true`、`ImeEvent` は emit せず (Phase 4-E State は preedit text 受信時に初期化)。
  - `WindowEvent::Ime(Ime::Preedit { preedit_text, cursor: Option<(usize, usize)> })` → `ImeEvent::Preedit(text)`。`preedit_text` が空文字列の場合は preedit クリアとして `Preedit("")` を発火する (Phase 4-E `State::set("")` で active=false になる契約と整合)。
  - `WindowEvent::Ime(Ime::Commit(text))` → `ImeEvent::Commit(text)`、`im_composing = false`。
  - `WindowEvent::Ime(Ime::Disabled)` → `ImeEvent::FocusOut`、`im_composing = false`。
- **FR12: Ghostty 由来のステートマシン (`im_composing` + `in_keyevent`)** — commit と key event の二重消費を防ぐため、`WinitImeBridge` 内で 3 値ステートマシン (idle / in_keyevent / im_composing) を持つ。
  - `WindowEvent::KeyboardInput` 到達時に `in_keyevent = true` を立て、winit が同期的に `Ime::Preedit` / `Ime::Commit` を発火する場合はそれを優先 (= `Consumed`)。
  - `Ime::Commit` 受信時に `im_composing = false`、続く KeyboardInput release は通常通り処理 (PTY には流さない、composition 由来として一回限り suppress)。
  - `Ime::Preedit` 中 (`im_composing == true`) の KeyboardInput press / release は `dispatch_key_event` で `Consumed` を返す。
  - fcitx5 で modifier 単独 release が IM 切替に使われるケースに対応するため、release event も同様に `dispatch_key_event` を呼ぶ。
  - IBus / GTK simple input の commit / preedit-end 順序差異への耐性のため、`im_composing` フラグは `Ime::Commit` 到達で確実に倒すが、`Ime::Disabled` 到達でも倒す (二重に倒しても idempotent)。
- **FR13: tao 0.34 → winit 0.30.9 windowing migration** — event loop, window 作成, KeyboardInput / Focused / Resized / ReceivedCharacter (winit では `Ime::Commit` に統合) のハンドラ、wgpu surface 連携、raw-window-handle 0.6 経路を winit に移行する。`window_host.rs` / `main.rs` 約 200 行の書き換え。winit features は `default-features = false, features = ["rwh_06", "x11", "wayland"]` (Windows は workspace 自動 enable)。

### Non-Functional Requirements

- **NFR1 - Performance (preedit overlay):** key press → preedit overlay redraw < 30 ms (2 frames @ 60 FPS) on Linux X11 release host. winit 経由でも同条件。
- **NFR2 - Performance (commit latency):** IME `Commit` event → `PtySession::write` < 5 ms。
- **NFR3 - Performance (IME-OFF regression):** key-down → PTY write latency in IME-OFF mode must not regress from Phase 4 baseline (TS-perf-1 / TS-perf-2 再 run; <= +10% allowed). tao → winit 移行による影響を含めて担保。
- **NFR4 - Stability:** winit IME 初期化失敗 (`Window::set_ime_allowed(true)` 後に `Ime::Enabled` が来ない compositor 等) で native-poc がクラッシュしない。`Ime::Disabled` を受け取ったら 1 tick 以内に `on_ime_focus_lost` でクリア。
- **NFR5 - Workspace compatibility (変更):** `cargo build --workspace` and `cargo test --workspace` keep passing through the redesign. Legacy `src-tauri` は触らない。**ただし winit 追加で wgpu integration の整合 (raw-window-handle 0.6 経由の surface 作成) を再確認する必要がある。**
- **NFR6 - Module layout (変更):** New code lives under `native-poc/src/ime/{backend, null, preedit, commit, winit_bridge}.rs`. **削除**: `native-poc/src/ime/{x11, wayland, windows}.rs`。`preedit.rs` / `commit.rs` は不変 (Phase 4-E 契約)。
- **NFR7 - Logging:** winit IME enable 成功 / fallback / `Ime::Disabled` 検出を `log::warn` で記録。
- **NFR8 - Linux fcitx5 parity:** Phase 1 fcitx5 acceptance criteria (`doc/tasks/ime-input-support/SPEC.md` US1-US5) pass on native-poc on X11 (winit 経路)。

## Implementation Approach

### Architecture

**Overall slice (post-redesign):**

```
┌───────────────────────────────────────────────────────────────┐
│  native-poc App                                               │
│   ┌────────────────────────────────────────────────────────┐  │
│   │  winit::EventLoop (replaces tao::EventLoop)            │  │
│   │   ┌──────────────────┐   ┌──────────────────────────┐  │  │
│   │   │ WindowEvent::    │   │ WindowEvent::Ime         │  │  │
│   │   │ KeyboardInput    │   │  Enabled/Preedit/        │  │  │
│   │   │ (press+release   │   │  Commit/Disabled         │  │  │
│   │   │  両方)            │   │                          │  │  │
│   │   └────────┬─────────┘   └────────┬─────────────────┘  │  │
│   │            │                      │                     │  │
│   │            ▼                      ▼                     │  │
│   │   ┌──────────────────┐   ┌──────────────────────────┐  │  │
│   │   │ WinitImeBridge   │   │ WinitImeBridge           │  │  │
│   │   │  dispatch_key    │   │  on_winit_ime_event      │  │  │
│   │   │  _event          │   │  → ImeEvent              │  │  │
│   │   │  (state machine) │   │                          │  │  │
│   │   └────────┬─────────┘   └────────┬─────────────────┘  │  │
│   │            │ Consumed/Passthrough │                     │  │
│   │            ▼                      ▼                     │  │
│   │     winit_key_to_bytes → PTY    pump queue              │  │
│   └──────────────────┬─────────────────────────────────────┘  │
│                      │                                         │
│             ┌────────▼─────────┐                               │
│             │   App (existing) │                               │
│             │  on_ime_preedit  │◀── ImeEvent::Preedit          │
│             │  on_ime_commit   │◀── ImeEvent::Commit           │
│             │  on_ime_focus_lost│◀─ ImeEvent::FocusOut         │
│             └───────────────────┘                              │
│                                                                │
│  ime::backend (unchanged trait)                                │
│  ime::null (unchanged NullBackend)                             │
│  ime::winit_bridge (NEW: WinitImeBridge implements ImeBackend) │
│  ime::preedit / ime::commit (Phase 4-E、UNCHANGED)             │
└────────────────────────────────────────────────────────────────┘
```

**Removed (vs. previous Phase 4-G):**
- `native-poc/src/ime/x11.rs` (XOpenIM/XCreateIC/XFilterEvent/XmbLookupString のラッパ + synthetic XKeyEvent)
- `native-poc/src/ime/wayland.rs` (zwp_text_input_v3 scaffold + pump thread)
- `native-poc/src/ime/windows.rs` (SetWindowSubclass + WM_IME_* + IMM32)
- `native-poc/Cargo.toml` の `x11-dl`, `wayland-client`, `wayland-protocols`, `windows` crate 依存

**Component split:**

- `native_poc::ime::backend` — `ImeBackend` trait, `ImeEvent` enum, `KeyDispatchResult`, `ImeInitError`, `RawKeyEvent` adapter, factory。**保持 (内容微調整: winit factory に変更)**
- `native_poc::ime::null` — `NullBackend` (passthrough only)。**保持**
- `native_poc::ime::winit_bridge` — `WinitImeBridge` (winit `WindowEvent::Ime` を `ImeEvent` に変換 + Ghostty state machine + `set_ime_cursor_area` 呼出)。**新規**
- `native_poc::ime::preedit` / `commit` (Phase 4-E) — **不変**

### Data Flow

**winit 統合 (X11 / Wayland / Windows 共通):**

```
winit::Event::WindowEvent::KeyboardInput { state, ... }
    └─▶ WinitImeBridge::dispatch_key_event(raw)
            │ (state machine: idle / in_keyevent / im_composing)
            ├─ im_composing == true → KeyDispatchResult::Consumed
            │     └─▶ skip winit_key_to_bytes
            └─ im_composing == false → KeyDispatchResult::Passthrough
                  └─▶ existing winit_key_to_bytes (移植版) → PTY

winit::Event::WindowEvent::Ime(Ime::Enabled)
    └─▶ WinitImeBridge: im_composing = true

winit::Event::WindowEvent::Ime(Ime::Preedit { text, cursor })
    └─▶ WinitImeBridge: ImeEvent::Preedit(text) を queue へ

winit::Event::WindowEvent::Ime(Ime::Commit(text))
    └─▶ WinitImeBridge: im_composing = false
        ImeEvent::Commit(text) を queue へ

winit::Event::WindowEvent::Ime(Ime::Disabled)
    └─▶ WinitImeBridge: im_composing = false
        ImeEvent::FocusOut を queue へ

[event-loop tick]
  ImeBackend::pump(&mut events)
    └─▶ App drains:
          Preedit(s) → on_ime_preedit(&s)
          Commit(s)  → on_ime_commit(&s)
          FocusOut   → on_ime_focus_lost()

[cursor cell change]
  App::notify_cursor_rect_if_changed
    └─▶ WinitImeBridge::notify_cursor_rect(x, y, w, h)
         └─▶ Window::set_ime_cursor_area(...)
```

**Fallback path (NullBackend):**

```
winit::WindowEvent::KeyboardInput → winit_key_to_bytes (IME 統合 OFF 時)
winit::WindowEvent::Ime は無視 (Window::set_ime_allowed(false) のため発火しない)
```

### API Design

trait surface は **不変** (Phase 4-G オリジナルから継承):

```rust
// native-poc/src/ime/backend.rs (unchanged)
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

pub enum ImeEvent {
    Preedit(String),
    Commit(String),
    FocusOut,
}

pub enum KeyDispatchResult {
    Consumed,
    Passthrough,
}

#[derive(Debug)]
pub enum ImeInitError {
    Unavailable(String),
    HandleType(String),
    PlatformError(String),
}

pub trait ImeBackend: Send {
    fn dispatch_key_event(&mut self, raw: &RawKeyEvent) -> KeyDispatchResult;
    fn notify_cursor_rect(&mut self, x_px: i32, y_px: i32, w_px: i32, h_px: i32);
    fn notify_focus(&mut self, focused: bool);
    fn pump(&mut self, events: &mut Vec<ImeEvent>);
}

pub struct RawKeyEvent<'a> {
    pub physical_key_code: u32,
    pub state_pressed: bool,
    pub mods: crate::pty::input::Modifiers,
    pub winit_event: &'a winit::event::KeyEvent,  // (was tao::event::KeyEvent)
}
```

**新規追加 (WinitImeBridge 内部):**

```rust
// native-poc/src/ime/winit_bridge.rs (NEW)
use std::collections::VecDeque;
use std::sync::Arc;
use winit::window::Window;

pub struct WinitImeBridge {
    window: Arc<Window>,
    im_composing: bool,
    in_keyevent: bool,
    queue: VecDeque<ImeEvent>,
    last_cursor_rect: Option<(i32, i32, i32, i32)>,
}

impl WinitImeBridge {
    pub fn init(window: Arc<Window>) -> Result<Self, ImeInitError> {
        window.set_ime_allowed(true);
        Ok(Self { window, im_composing: false, in_keyevent: false,
                  queue: VecDeque::new(), last_cursor_rect: None })
    }

    /// Called from window_host on winit::WindowEvent::Ime(_).
    pub fn on_winit_ime(&mut self, ime: &winit::event::Ime) {
        match ime {
            winit::event::Ime::Enabled => self.im_composing = true,
            winit::event::Ime::Preedit(text, _cursor) => {
                self.queue.push_back(ImeEvent::Preedit(text.clone()));
            }
            winit::event::Ime::Commit(text) => {
                self.queue.push_back(ImeEvent::Commit(text.clone()));
                self.im_composing = false;
            }
            winit::event::Ime::Disabled => {
                self.queue.push_back(ImeEvent::FocusOut);
                self.im_composing = false;
            }
        }
    }
}

impl ImeBackend for WinitImeBridge { /* ... */ }
```

### Database Schema

Not applicable.

### Dependencies

**Internal:**

- `term_core` — unchanged.
- `native-poc::ime::{preedit, commit}` — Phase 4-E layer, unchanged.
- `native-poc::pty::input::Modifiers` — reused as-is.
- `native-poc::pty::PtySession` — reused as-is.

**External (new — replaces tao + 自前 IME deps):**

- `winit = "0.30.9"` (`default-features = false, features = ["rwh_06", "x11", "wayland"]`) — event loop, window, IME 統合
- `raw-window-handle` — 既存 direct dep (0.6)、不変
- `crossbeam-channel` — 既存 (Wayland thread からの掃出しは不要になるが他用途で残る)

**External (removed — Phase 4-G 自前 XIM 関連):**

- ~~`x11-dl = "2"`~~ — XIM bindings 削除
- ~~`wayland-client = "0.31"`~~ — zwp_text_input_v3 削除
- ~~`wayland-protocols = "0.31"`~~ — 同上
- ~~`windows = "0.58"`~~ — IMM32 削除
- ~~`tao = "0.34"`~~ — winit に置換

### File Structure

```
native-poc/
├── Cargo.toml                                # MODIFY: winit 追加, tao / x11-dl / wayland-* / windows 削除
└── src/
    ├── app.rs                                # MODIFY: tao → winit 型参照のみ (本体 ImeBackend 連携部は不変)
    ├── main.rs                               # MODIFY: tao::EventLoop → winit::EventLoop
    ├── window_host.rs                        # MODIFY: tao API → winit API (約 200 行)
    ├── settings.rs                           # UNCHANGED: ImeSettings は Phase 4-G オリジナルから不変
    └── ime/
        ├── mod.rs                            # MODIFY: x11/wayland/windows mod を削除、winit_bridge を追加
        ├── preedit.rs                        # UNCHANGED (Phase 4-E)
        ├── commit.rs                         # UNCHANGED (Phase 4-E)
        ├── backend.rs                        # MODIFY: factory が WinitImeBridge を probe するように変更
        ├── null.rs                           # UNCHANGED
        ├── winit_bridge.rs                   # NEW: WinitImeBridge (winit Ime → ImeEvent + state machine)
        ├── x11.rs                            # DELETED
        ├── wayland.rs                        # DELETED
        └── windows.rs                        # DELETED
```

### Settings Schema

Phase 4-G オリジナルから不変:

```jsonc
{
  "ime": {
    "native_integration": true   // bool; default true. When false, behave like Phase 4 fallback.
  }
}
```

### Environment Variables

- `EMTERM_NATIVE_IME=0` — Phase 4-G オリジナルから不変。env wins over settings。

## Test Scenarios

### Unit Tests (retained)

- [ ] **TS-backend-1**: `NullBackend::dispatch_key_event` returns `Passthrough` for every key.
- [ ] **TS-backend-2**: `NullBackend::pump` produces an empty `ImeEvent` vector.
- [ ] **TS-backend-3**: `App::pump_ime` drains a fake `MockBackend`'s queue and routes events through `on_ime_{preedit,commit,focus_lost}`.
- [ ] **TS-backend-4**: When `MockBackend::dispatch_key_event` returns `Consumed`, the App's per-tick handling skips the existing `winit_key_to_bytes` path.
- [ ] **TS-backend-5**: When `dispatch_key_event` returns `Passthrough`, the bytes from `winit_key_to_bytes` reach the mocked PTY exactly once.
- [ ] **TS-cursor-1**: `App` calls `ImeBackend::notify_cursor_rect` exactly once when the cursor cell coordinate changes; not called when the cursor stays put.
- [ ] **TS-focus-1**: `Focused(false)` triggers `notify_focus(false)` + `App::on_ime_focus_lost`; the active tab's `preedit_state.active()` becomes false.
- [ ] **TS-fallback-1**: `EMTERM_NATIVE_IME=0` results in `App` holding a `NullBackend` regardless of `settings.ime.native_integration`.
- [ ] **TS-fallback-2**: `settings.ime.native_integration = false` (with no env) also yields `NullBackend`.
- [ ] **TS-fallback-3**: `WinitImeBridge::init` returning `ImeInitError::*` is caught at startup and replaced with `NullBackend`; exactly one warn log.
- [ ] **TS-settings-1**: `Settings::default().ime.native_integration` is `true`.
- [ ] **TS-route-1** (Phase 4-E regression): `ImeEvent::Preedit("a\x1bb")` → `App::on_ime_preedit` → sanitize strips ESC.
- [ ] **TS-route-2** (Phase 4-E regression): `ImeEvent::Commit("a\x1bb")` → PTY receives only `b"ab"`, not wrapped in `ESC[200~ ... ESC[201~`.

### Unit Tests (new — winit bridge)

- [ ] **TS-winit-1**: `WinitImeBridge::on_winit_ime(Ime::Enabled)` sets `im_composing = true`; subsequent `dispatch_key_event` returns `Consumed`.
- [ ] **TS-winit-2**: `WinitImeBridge::on_winit_ime(Ime::Preedit("foo", None))` queues `ImeEvent::Preedit("foo")`; pump drains it.
- [ ] **TS-winit-3**: `WinitImeBridge::on_winit_ime(Ime::Commit("日本"))` queues `ImeEvent::Commit("日本")` and resets `im_composing = false`; subsequent `dispatch_key_event` returns `Passthrough`.
- [ ] **TS-winit-4**: `WinitImeBridge::on_winit_ime(Ime::Disabled)` queues `ImeEvent::FocusOut` and resets `im_composing = false`.
- [ ] **TS-winit-5**: State machine resilience — `Ime::Commit` followed by `Ime::Disabled` is idempotent (im_composing remains false; FocusOut still emitted; no double-commit).
- [ ] **TS-winit-6**: Modifier-only release (Shift release with no other event) hits `dispatch_key_event` (fcitx5 IM switch criterion).
- [ ] **TS-winit-7**: `notify_cursor_rect(x, y, w, h)` calls `Window::set_ime_cursor_area` only when the rect changes; mock window records call count.

### Integration Tests

- [ ] **TS-winit-int-1** (Linux X11 + Xvfb, `#[ignore]`): winit `EventLoop` で window 生成 → `set_ime_allowed(true)` → 実 IM サーバ無しでも `Ime::Disabled` 通知が来ることを確認。
- [ ] **TS-winit-int-2** (Windows, `#[cfg(windows)]`, host-deferred): winit が IMM32 経由で `Ime::Commit` を発火することを確認。

### E2E Tests

Existing E2E suite is legacy Tauri only and not relevant. Phase 4-G redesign は manual host gates で検証する:

- [ ] **TS-manual-ime-x11** (Linux X11 + fcitx5 host): launch native-poc on X11, toggle fcitx5 with `Ctrl+Space`, type `nihongo`, observe underline preedit overlay, convert with `Space`, commit with `Enter`; confirm `日本語` reaches the shell exactly once.
- [ ] **TS-manual-ime-x11-ibus** (Linux X11 + IBus host): same flow with IBus.
- [ ] **TS-manual-ime-wayland** (Linux Wayland + fcitx5-wayland host): same flow under Wayland session. winit が `zwp_text_input_v3` を扱う。
- [ ] **TS-manual-ime-windows** (Windows host): same flow with MS-IME and Google IME.
- [ ] **TS-manual-ime-fallback** (any host): launch with `EMTERM_NATIVE_IME=0`, confirm warn log + fallback behavior.
- [ ] **TS-manual-ime-imserver-restart** (Linux X11): kill `fcitx5`, observe `Ime::Disabled` → warn log; restart `fcitx5`, refocus, confirm IME reattaches.
- [ ] **TS-manual-ime-mux** (Linux X11 + fcitx5 + emterm mux): inside `emterm mux attach` session, type Japanese, confirm commit lands in the mux-routed PTY.

### Edge Cases

- [ ] **EC1**: winit IME enable に対して compositor が `Ime::Enabled` を返さない → `Ime::Disabled` のみ届く / 一切届かない場合の挙動。`set_ime_allowed(true)` 後 N tick で何も来ない場合は NullBackend にフォールバックは **しない** (winit に任せる) が、preedit overlay は出ない動作のままターミナルは動く。
- [ ] **EC2**: 100+ character preedit composition stays in sync; sanitize doesn't drop CJK.
- [ ] **EC3**: composition open + window resize → preedit overlay anchor stays at the (row, col) it was anchored to; if that cell scrolls off, overlay is treated as inactive.
- [ ] **EC4**: composition open + IM server crash mid-stream → winit `Ime::Disabled` 受信 → `on_ime_focus_lost` で preedit クリア。
- [ ] **EC5**: ASCII typing during fallback path stays on the existing PTY path (no double-input).
- [ ] **EC6**: `Ime::Commit` 直後の同期 `KeyboardInput` release は二重入力にならない (state machine が press / release 両方 `Consumed` を返す)。

### Performance Tests

- [ ] **TS-perf-3**: preedit key-press → overlay redraw latency < 30 ms (Linux X11 release host, `EMTERM_IME_PERF=1`).
- [ ] **TS-perf-4**: commit → `PtySession::write` < 5 ms.
- [ ] **TS-perf-regression**: Phase 4 `TS-perf-1` / `TS-perf-2` 再 run; new measurement must be within +10% of the Phase 4 baseline. **winit 移行込みで担保**。

## Security Considerations

- **Sanitization**: All preedit / commit strings flow through the existing `ime::preedit::sanitize` (C0 + C1 stripping). `WinitImeBridge` は backend なので PTY を直接触らない。
- **No bracketed paste wrapping**: `ime::commit::write_commit` already enforces this.
- **UTF-8 validation**: winit が UTF-8 `String` で提供する (winit 内部で X11 wide char / Win32 UTF-16 → UTF-8 変換済み)。`WinitImeBridge` は追加 validation 不要。invalid byte が含まれた場合は winit が drop 済み。
- **Resource cleanup**: `Drop` on `WinitImeBridge` calls `Window::set_ime_allowed(false)`. winit が IC / subclass を自動解放する。
- **No new network / FS surface**: winit は OS-local IPC のみ。
- **Settings validation**: 不変。

## Error Handling

### Error Codes

| Code | Description | Severity | User-Facing Message |
|------|-------------|----------|---------------------|
| IME_E001 | winit `set_ime_allowed(true)` 後 N tick で `Ime::Enabled` が来ない compositor | warn (informational) | (logged once, terminal continues without IME) |
| IME_E002 | winit Ime event の text が空でない `Commit` で `String::is_empty()` false なのに UTF-8 validity warning が log に出る (winit 経由なので通常起きない) | warn | (logged, drop) |
| IME_E901 | Backend produced more than 1024 queued events between pumps | warn | (logged, drop overflow events) |

Legacy 自前 XIM 用の `IME_E101/102/201/301/302/401` は本 redesign で削除する。

### Error Flow

```
WinitImeBridge::init
    ├─ Ok(bridge)   → installed (window.set_ime_allowed(true))
    └─ Err(reason)
         └─ log::warn!("ime: native integration disabled ({reason}), falling back")
         └─ install NullBackend

WinitImeBridge::on_winit_ime(Ime::Disabled)
    └─ ImeEvent::FocusOut → App::on_ime_focus_lost
       (preedit がクリアされる; im_composing = false)
       次の Ime::Enabled でリカバリ
```

## Performance Optimization

### Performance Goals

- preedit overlay redraw latency: < 30 ms (60 FPS / 2 frames) on Linux X11 release host.
- commit → PTY write: < 5 ms.
- IME-OFF key-down → PTY write: <= Phase 4 baseline + 10%.

### Optimization Strategies

- **winit ネイティブ IME 経路**: 自前 XIM の synthetic XKeyEvent / XmbLookupString のオーバヘッドが消える。winit が直接 IM server と通信する経路の方が短い。
- **Cursor rect updates rate-limited**: `App` calls `notify_cursor_rect` only when the cursor cell (row, col) changes.
- **State machine cached on `WinitImeBridge`**: `im_composing` / `in_keyevent` のチェックは bool 比較のみ。
- **Pump rate limiting**: at most 1024 events drained per tick (IME_E901)。

### Caching Strategy

- preedit text は `ime::preedit::State` (Phase 4-E) が保持。追加のキャッシュ不要。
- `last_cursor_rect` を `WinitImeBridge` 内で保持し、同一 rect への `set_ime_cursor_area` 呼出を抑制。

## Success Criteria

- [ ] FR4-FR13 implemented; all unit + integration tests above pass.
- [ ] 旧 `ime::{x11, wayland, windows}` モジュールが削除されている (`git ls-files` で確認)。
- [ ] `Cargo.toml` から `x11-dl`, `wayland-client`, `wayland-protocols`, `windows`, `tao` が消えている。
- [ ] `cargo build --workspace` succeeds on Linux + Windows.
- [ ] `cargo test --workspace` exits 0.
- [ ] `cargo fmt --all -- --check` clean.
- [ ] `cargo clippy -p emterm-native-poc -- -D warnings` zero errors.
- [ ] `TS-manual-ime-x11`, `TS-manual-ime-x11-ibus`, `TS-manual-ime-wayland`, `TS-manual-ime-windows`, `TS-manual-ime-fallback`, `TS-manual-ime-imserver-restart`, `TS-manual-ime-mux` pass on host (winit 経路で).
- [ ] `TS-perf-3` / `TS-perf-4` meet targets.
- [ ] `TS-perf-regression`: IME-OFF latency within +10% of Phase 4 baseline (winit 移行込み).
- [ ] Phase 4-E `ime::preedit::State` / `ime::commit::write_commit` / `render::cursor::draw_cursor_with_preedit` files unchanged (`git diff 9f290ab..HEAD` empty on these paths).
- [ ] Legacy `src-tauri` build / test unaffected (NFR5).

## Open Questions

- [ ] **OQ1 (新規)**: winit features 構成 — `x11` / `wayland` を default-features = false で明示 enable、`rwh_06` は raw-window-handle 0.6 経路維持に必須。Windows は自動 enable。`serde` features は不要。
- [ ] **OQ2 (新規)**: wgpu surface 作成は `wgpu::Instance::create_surface(&window)` で raw-window-handle 0.6 経由になる。winit 0.30 は `Arc<Window>` を返すので surface は `Arc::clone(&window)` で共有する。Phase 4 で `tao::Window` を使う既存箇所をすべて `winit::window::Window` に差し替える。
- [ ] **OQ3 (新規)**: winit 0.30 の `ApplicationHandler` trait 採択 vs 旧 `EventLoop::run` クロージャ式どちらを使うか。Phase 4-G-2 移行時に決定。`ApplicationHandler` のほうが clean だが、既存 `window_host.rs` の構造との fit を見て判断。
- [ ] **OQ4 (新規)**: winit の `KeyEvent` から `Modifiers` への変換 (Phase 4 の `tao_key_to_bytes` 相当を `winit_key_to_bytes` に rewrite する責務範囲)。winit `Modifiers` は struct で持つので mapping は素直。
- [ ] **OQ5 (継承)**: `notify_cursor_rect` のサブセル精度は不要 (cell-aligned で十分).

## Implementation Phases

### Sub-Phase 4-G-1: Cleanup (自前 XIM 関連削除)

**Goals**: 自前 XIM / Wayland / IMM32 ファイルと依存を削除し、cargo test --workspace green を維持。

**Deliverables:**
- `native-poc/src/ime/{x11, wayland, windows}.rs` 削除
- `native-poc/src/ime/mod.rs` から該当 mod 削除
- `native-poc/Cargo.toml` から `x11-dl`, `wayland-client`, `wayland-protocols`, `windows` 依存削除
- `native-poc/src/ime/backend.rs` の factory probe コードを WinitImeBridge 用に簡略化 (まだ実体はないので `Err(Unavailable)` で NullBackend 化)
- 該当ユニットテスト (`TS-x11-*`, `TS-wayland-*`, `TS-windows-*`, `TS-backend-int-*`, `TS-manual-ime-*` の自前 XIM 起源分) 削除
- `cargo test --workspace` green (NullBackend のみで動作)

### Sub-Phase 4-G-2: winit 移行 (windowing only)

**Goals**: tao 0.34 → winit 0.30.9 に置換。IME は NullBackend のままで、素の打鍵と既存 `WindowEvent::ReceivedCharacter` (winit では `Ime::Commit` で来る) が動くことを確認。

**Deliverables:**
- `native-poc/Cargo.toml`: `tao = "0.34"` 削除、`winit = "0.30.9"` 追加 (`default-features = false, features = ["rwh_06", "x11", "wayland"]`)
- `native-poc/src/main.rs`: `tao::EventLoop` → `winit::EventLoop`
- `native-poc/src/window_host.rs`: tao API → winit API (event loop, window creation, KeyboardInput, Focused, Resized, ReceivedCharacter → Ime::Commit, wgpu surface 連携、raw-window-handle 0.6 経路維持)
- `native-poc/src/app.rs`: `tao::event::*` 参照を winit に置換 (本体ロジックは不変)
- `winit_key_to_bytes` 関数: tao 版から winit 版に rewrite (modifier mapping を winit の `Modifiers` に合わせる)
- `cargo build --workspace` / `cargo test --workspace` green
- TS-perf-regression を含む既存 perf 計測で +10% 以内を維持 (host-deferred)

### Sub-Phase 4-G-3: winit IME bridge

**Goals**: `WinitImeBridge` を新規実装し、`ImeBackend` trait 経由で App::on_ime_* に接続。Ghostty 由来のステートマシンを実装。

**Deliverables:**
- `native-poc/src/ime/winit_bridge.rs` 新規
- `native-poc/src/ime/mod.rs`: `pub mod winit_bridge`
- `native-poc/src/ime/backend.rs`: factory が `WinitImeBridge::init(window)` を呼ぶ
- `native-poc/src/window_host.rs`: `WindowEvent::Ime(_)` を `WinitImeBridge::on_winit_ime` に転送、`KeyboardInput` の press / release 両方を `dispatch_key_event` に通す、`Window::set_ime_cursor_area` を `notify_cursor_rect` 経由で呼ぶ
- `TS-winit-1..7` ユニットテスト
- `cargo test --workspace` green、+7 件以上

### Sub-Phase 4-G-4: manual gate 再実施

**Goals**: TS-manual-ime-* を winit 経路で再実施し、Phase 1 WebView IME parity (NFR8) を達成。

**Deliverables:**
- TS-manual-ime-x11 / x11-ibus / wayland / windows / fallback / imserver-restart / mux のホスト実施結果を `VERIFICATION_RESULT.md` に追記
- TS-perf-3 / TS-perf-4 / TS-perf-regression のホスト計測結果を追記
- README.md の Phase 4-G feature matrix を winit 採択方針に更新

## References

- `tmp/research/wezterm-ime.md` — WezTerm が winit から自前実装に切り替えた経緯
- `tmp/research/alacritty-winit-ime.md` — Alacritty の winit + IME 設計
- `tmp/research/ghostty-ime.md` — Ghostty の `im_composing` + `in_keyevent` ステートマシン (本 redesign の直接の参照源)
- restruct.md Phase 4-G section: `tmp/restruct.md`
- Phase 4 SPEC: `doc/tasks/mux-tabs-windows-ime/SPEC.md` (FR11 / FR12 / NFR3 deferred state)
- Phase 1 WebView IME SPEC: `doc/tasks/ime-input-support/SPEC.md` (parity reference for fcitx5)
- Existing routing layer: `native-poc/src/ime/{mod.rs, preedit.rs, commit.rs}` (Phase 4-E 不変)
- Existing App-side IME routes: `native-poc/src/app.rs` (`on_ime_preedit`, `on_ime_commit`, `on_ime_focus_lost`)
- 旧 Phase 4-G VERIFICATION_RESULT (本 redesign で superseded): `doc/tasks/ime-native-integration/VERIFICATION_RESULT.md`
