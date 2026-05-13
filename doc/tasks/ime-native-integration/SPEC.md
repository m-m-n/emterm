# Feature: Native IME Integration (Phase 4-G)

## Overview

Phase 4-G of the emterm restructuring plan (`tmp/restruct.md`). Phase 4 (`doc/tasks/mux-tabs-windows-ime/`) auto-scope landed the IME routing layer in native-poc — `ime::preedit::State`, `ime::commit::write_commit`, `App::on_ime_{preedit,commit,focus_lost}` and `render::cursor::draw_cursor_with_preedit` — but tao 0.34 does not surface platform IME events to those routes, so the Phase 4 manual gates (`TS-manual-ime-linux`, `TS-manual-ime-windows`) and `NFR3: Linux fcitx5 IME parity with Phase 1` ended deferred / N/A.

Phase 4-G implements the platform IME clients native-poc needs on top of tao 0.34 without replacing tao:

- **Linux X11**: an XIM client that reaches fcitx5 / IBus via `XOpenIM` / `XCreateIC` / `XFilterEvent` / `XmbLookupString`.
- **Linux Wayland**: a `zwp_text_input_v3` client (Wayland protocol extension) that talks to fcitx5-wayland / IBus.
- **Windows**: an IMM32 client that subscribes to `WM_IME_STARTCOMPOSITION` / `WM_IME_COMPOSITION` / `WM_IME_ENDCOMPOSITION` via a window subclass.

The existing routing layer is unchanged. Each backend simply funnels preedit / commit strings into `App::on_ime_{preedit,commit,focus_lost}`. Phase 4-E's sanitization contract (`ime::preedit::sanitize`) and the "commit is not a paste" rule (no bracketed-paste wrap) carry over unchanged.

## Objectives

- Reach Phase 1 WebView IME parity on Linux fcitx5 (X11 + Wayland).
- Reach functional Windows MS-IME / Google IME preedit + commit support; candidate window position is best effort.
- Add an explicit fallback path (env var + setting) that downgrades to the current `WindowEvent::ReceivedImeText`-only behavior so a broken IM server never breaks the terminal.
- Land the work without regressing IME-OFF key input latency or the legacy `src-tauri` workspace.

## User Stories

### US1: Linux X11 Japanese input via fcitx5
As a Linux X11 user, I want to compose Japanese in native-poc with fcitx5 so that I can write commit messages and shell commands in Japanese as smoothly as in the Phase 1 WebView build.

**Acceptance Criteria:**
- [ ] Toggle key (e.g. `Ctrl+Space`) turns fcitx5 on, and the next printable key starts a composition.
- [ ] preedit text appears as an underline overlay anchored to the cursor cell (`render::cursor::draw_cursor_with_preedit`).
- [ ] On commit, the bytes reach the active PTY exactly once and the overlay clears.
- [ ] Special chords (`Ctrl+C`, arrows, `Esc`, `Tab`) keep working during composition (they bypass the IME and go through the existing PTY path).

### US2: Linux Wayland Japanese input via fcitx5
As a Linux Wayland user, I want native-poc to talk `zwp_text_input_v3` so that fcitx5-wayland / IBus deliver preedit + commit events without requiring XWayland.

**Acceptance Criteria:**
- [ ] Same composition / commit / toggle behavior as US1.
- [ ] `zwp_text_input_v3::set_cursor_rectangle` is updated when the cursor moves so the candidate window tracks the cursor.

### US3: Windows MS-IME Japanese input
As a Windows user, I want native-poc to subscribe to IMM32 messages so that MS-IME / Google IME can drive preedit + commit just like any other Win32 app.

**Acceptance Criteria:**
- [ ] `WM_IME_COMPOSITION` (`GCS_COMPSTR`) updates the preedit overlay.
- [ ] `WM_IME_COMPOSITION` (`GCS_RESULTSTR`) commits to the PTY exactly once and clears the overlay.
- [ ] `ImmSetCompositionWindow` is called with the cursor's pixel position so the candidate window appears near the cursor (best effort; not gating).

### US4: Fallback when IME integration is unavailable
As any user, I want native-poc to keep working even when the IM server is dead, the compositor is too old, or I explicitly disable the integration.

**Acceptance Criteria:**
- [ ] `EMTERM_NATIVE_IME=0` (env var) or `settings.json` `ime.native_integration = false` disables the new clients.
- [ ] In the disabled mode the terminal falls back to Phase 4's behavior (`WindowEvent::ReceivedImeText` only, no preedit overlay) and emits exactly one warn log.
- [ ] If `XOpenIM` / Wayland binding / window subclass install fails at startup, native-poc auto-falls back instead of crashing and emits a warn log.

### US5: Focus loss clears stale composition
As any user, I want a stale preedit overlay to disappear when I tab away from native-poc.

**Acceptance Criteria:**
- [ ] `WindowEvent::Focused(false)` clears the active tab's preedit state (already wired in Phase 4-E).
- [ ] Each IME backend forwards the focus-out to the IM server (`XUnsetICFocus` / `zwp_text_input_v3::disable` / `WM_KILLFOCUS` via subclass passthrough).

## Technical Requirements

### Functional Requirements

- **FR1: XIM client (Linux X11)** — `native_poc::ime::x11` implements an XIM client that opens the IM server, creates an IC for the native-poc top-level window, filters key events via `XFilterEvent` before the existing `tao_key_to_bytes` path sees them, reads preedit / commit via the IM callbacks (`Preedit*` / `Status*` callbacks) plus `XmbLookupString` for direct commits, and reports cursor location via `XICAttribute::XNSpotLocation`. tao's X11 display handle and window handle are obtained through `raw-window-handle` 0.6. The client borrows the X11 connection that tao already owns — it never opens a second `XOpenDisplay`, because doing so would race with tao's event sink. Filter results route through a new `ImeBackend` trait so `App` is backend-agnostic.
- **FR2: text-input-v3 client (Linux Wayland)** — `native_poc::ime::wayland` implements a `zwp_text_input_v3` client. The client runs an independent Wayland event pump on a dedicated thread because tao 0.34 does not expose the protocol's `zwp_text_input_manager_v3` global on its main display roundtrip. The Wayland display proxy is obtained through `raw-window-handle::RawDisplayHandle::Wayland`. preedit / commit are delivered via `commit_string` / `preedit_string` events and forwarded to the main thread through `crossbeam_channel`. `set_cursor_rectangle` is called on cursor movement.
- **FR3: IMM32 client (Windows)** — `native_poc::ime::windows` installs a window subclass on the native-poc top-level HWND using `SetWindowSubclass` (`windows-rs`). The subclass intercepts `WM_IME_STARTCOMPOSITION`, `WM_IME_COMPOSITION` (`GCS_COMPSTR` for preedit, `GCS_RESULTSTR` for commit), and `WM_IME_ENDCOMPOSITION`. `ImmGetCompositionStringW` reads the UTF-16 payloads which are then converted to UTF-8 before reaching the routing layer. `ImmSetCompositionWindow` with `CFS_POINT` reports the cursor pixel position. The subclass calls `DefSubclassProc` for everything else so tao's window proc keeps owning the rest of the message stream.
- **FR4: `ImeBackend` trait** — `native_poc::ime::backend::ImeBackend` defines the surface area each platform client implements. It is the only seam between `App` and the platform code. Methods: `init(window: RawWindowHandle, display: RawDisplayHandle) -> Result<Self, ImeInitError>`, `dispatch_key_event(raw: &RawKeyEvent) -> KeyDispatchResult { Consumed, Passthrough }`, `notify_cursor_rect(x_px: i32, y_px: i32, w_px: i32, h_px: i32)`, `notify_focus(focused: bool)`, `pump(events: &mut Vec<ImeEvent>)`. `ImeEvent` is `{ Preedit(String), Commit(String), FocusOut }`. `App` owns a `Box<dyn ImeBackend>` and drains the queue once per event-loop tick.
- **FR5: routing into the existing Phase 4-E layer** — Backend events route as: `ImeEvent::Preedit(text) → App::on_ime_preedit(&text)` / `ImeEvent::Commit(text) → App::on_ime_commit(&text)` / `ImeEvent::FocusOut → App::on_ime_focus_lost()`. These methods exist already and are not modified. The sanitization contract (`ime::preedit::sanitize` strips C0/C1) is preserved by reusing those methods.
- **FR6: key event interception** — When native IME integration is active, `WindowEvent::KeyboardInput` is offered to `ImeBackend::dispatch_key_event` first. If the result is `Consumed`, the existing `tao_key_to_bytes` path is skipped for that key. If `Passthrough`, the current Phase 4 path runs unchanged. On Linux X11 the backend converts the tao key event to a synthetic `XKeyPressedEvent` (filling `xkey.keycode` from tao's `physical_key` / scan code) so `XFilterEvent` can decide. On Wayland the keyboard listener owned by the Wayland backend sees raw keys directly; tao's `KeyboardInput` for printable text becomes a no-op (the IME backend takes over). On Windows the subclass intercepts key messages naturally — `dispatch_key_event` returns `Passthrough` for everything (the IME path is decoupled because IMM32 fires its own `WM_IME_*` messages on key events that start a composition).
- **FR7: cursor rectangle reporting** — Whenever the cursor cell coordinate changes (driven by App-side state diff in `App::pump_all` / on PTY render), the active backend's `notify_cursor_rect` is called with the cursor's pixel rect derived from `App::active_tab().grid().cursor()` + `host.cell_size_px`. Rate-limited to "only when the cell row or column changed" — not every frame.
- **FR8: focus management** — `WindowEvent::Focused(true)` calls `ImeBackend::notify_focus(true)`; `Focused(false)` calls `notify_focus(false)` and triggers the existing `App::on_ime_focus_lost()` route (which clears preedit state). On X11 the backend issues `XSetICFocus` / `XUnsetICFocus`. On Wayland it sends `enable` / `disable` on the text-input object. On Windows the subclass simply lets `WM_SETFOCUS` / `WM_KILLFOCUS` flow to `DefSubclassProc`.
- **FR9: opt-out / fallback** — `EMTERM_NATIVE_IME=0` (env) or `settings.json` `ime.native_integration = false` short-circuits backend creation; the App holds a no-op `NullBackend` instead. Backend init failures (`XOpenIM` returned NULL, Wayland global missing, `SetWindowSubclass` failed) also fall back to `NullBackend`. In the fallback path, Phase 4's behavior is preserved — `WindowEvent::ReceivedImeText` still routes through `on_ime_commit`. `tao_key_to_bytes` retains its current Ctrl/Alt-only gating for `TaoKey::Character` so we don't introduce a double-input regression. Each fallback path logs `[WARN][BACKEND] ime: native integration disabled (<reason>)` exactly once.
- **FR10: settings additions** — `Settings` (`native-poc/src/settings.rs`) に `ime: ImeSettings { native_integration: bool }` を追加し、`Default` で `native_integration: true`。`settings.json` からのロードは Phase 7 のローダ責務なので、Phase 4-G では他 fields 同様 `Settings::default()` 経路でのみ exercise する。Phase 7 が JSON parse を実装した後、`settings.json` のシェイプは以下のとおり想定:
  ```jsonc
  "ime": {
    "native_integration": true   // bool, default true
  }
  ```
  Missing keys fall back to defaults; old `settings.json` files keep parsing (Phase 7 で実装)。

### Non-Functional Requirements

- **NFR1 - Performance (preedit overlay):** key press → preedit overlay redraw < 30 ms (2 frames @ 60 FPS) on Linux X11 release host.
- **NFR2 - Performance (commit latency):** IME `Commit` event → `PtySession::write` < 5 ms.
- **NFR3 - Performance (IME-OFF regression):** key-down → PTY write latency in IME-OFF mode must not regress from Phase 4 baseline (TS-perf-1 / TS-perf-2 re-run; <= +10% allowed).
- **NFR4 - Stability:** IME backend init failure must not crash native-poc. IM server death must drop us to fallback within one event-loop tick without losing the terminal.
- **NFR5 - Workspace compatibility:** `cargo build --workspace` and `cargo test --workspace` keep passing through Phase 4-G. Legacy `src-tauri` is not touched.
- **NFR6 - Module layout:** All new code lives under `native-poc/src/ime/{backend,x11,wayland,windows,null}.rs`. The existing `preedit.rs` / `commit.rs` / `mod.rs` are not modified beyond adding new public re-exports of the `ImeBackend` trait and `ImeEvent` enum.
- **NFR7 - Logging:** init success once per protocol, init failure / fallback once with reason, reconnect attempts with backoff visible at `log::warn`.
- **NFR8 - Linux fcitx5 parity:** Phase 1 fcitx5 acceptance criteria (`doc/tasks/ime-input-support/SPEC.md` US1-US5) pass on native-poc on X11.

## Implementation Approach

### Architecture

**Overall slice (post Phase 4-G):**

```
┌───────────────────────────────────────────────────────────────┐
│  native-poc App                                               │
│   ┌────────────────────────────────────────────────────────┐  │
│   │  tao::EventLoop                                        │  │
│   │   ┌──────────────────┐   ┌──────────────────────────┐  │  │
│   │   │ WindowEvent::    │   │ WindowEvent::Focused/    │  │  │
│   │   │ KeyboardInput    │   │ ReceivedImeText (fallback│  │  │
│   │   └────────┬─────────┘   │  path only)              │  │  │
│   │            │             └────────┬──────────────────┘  │  │
│   │            ▼                      │                     │  │
│   │   ┌──────────────────┐            │                     │  │
│   │   │ ImeBackend       │            │                     │  │
│   │   │ (trait dispatch) │            │                     │  │
│   │   └────────┬─────────┘            │                     │  │
│   │            │ Consumed/Passthrough │                     │  │
│   │            ▼                      │                     │  │
│   │     tao_key_to_bytes → PTY        │                     │  │
│   └──────────────────┬────────────────┴─────────────────────┘  │
│                      │                                         │
│             ┌────────▼─────────┐                               │
│             │   App (existing) │                               │
│             │  on_ime_preedit  │◀── ImeEvent::Preedit          │
│             │  on_ime_commit   │◀── ImeEvent::Commit           │
│             │  on_ime_focus_lost│◀─ ImeEvent::FocusOut         │
│             └───────────────────┘                              │
│                                                                │
│  ┌──────────────────────┐ ┌────────────────────┐ ┌───────────┐ │
│  │ ime::x11             │ │ ime::wayland       │ │ime::      │ │
│  │  XIM client          │ │  zwp_text_input_v3 │ │windows    │ │
│  │  (X11 conn shared    │ │  client (own       │ │ IMM32     │ │
│  │   with tao)          │ │  thread + pump)    │ │ subclass  │ │
│  └──────────────────────┘ └────────────────────┘ └───────────┘ │
│                                                                │
│  ime::null  ← used when integration is disabled / fallback     │
└────────────────────────────────────────────────────────────────┘
```

**Component split:**

- `native_poc::ime::backend` — `ImeBackend` trait, `ImeEvent` enum, `KeyDispatchResult`, `ImeInitError`, `RawKeyEvent` adapter.
- `native_poc::ime::null` — `NullBackend` (passthrough only, used when integration is disabled / fallback).
- `native_poc::ime::x11` — `X11Backend`, XIM glue. `#[cfg(all(unix, not(target_os = "macos")))]` + runtime probe `RawDisplayHandle::Xlib(_)`.
- `native_poc::ime::wayland` — `WaylandBackend`, `zwp_text_input_v3` glue + Wayland event pump thread. Same cfg as X11, runtime probe `RawDisplayHandle::Wayland(_)`.
- `native_poc::ime::windows` — `WindowsBackend`, IMM32 + window subclass. `#[cfg(windows)]`.
- `native_poc::ime::preedit` / `commit` (existing Phase 4-E) — unchanged.

### Data Flow

**Linux X11 preedit + commit:**

```
tao::Event::WindowEvent::KeyboardInput
    └─▶ X11Backend::dispatch_key_event(raw)
            │
            ├─▶ XFilterEvent(synthetic XKeyPressedEvent)
            │       ├─ true  → IM consumed → KeyDispatchResult::Consumed
            │       │           └─▶ later: IC callbacks → ImeEvent::{Preedit, Commit}
            │       └─ false → KeyDispatchResult::Passthrough
            │                   └─▶ existing tao_key_to_bytes path
            ▼
   event-loop tick: ImeBackend::pump(&mut events)
   App drains events:
     ├─ ImeEvent::Preedit(text) → App::on_ime_preedit(&text)
     ├─ ImeEvent::Commit(text)  → App::on_ime_commit(&text)
     └─ ImeEvent::FocusOut      → App::on_ime_focus_lost()
```

**Linux Wayland preedit + commit:**

```
[Wayland thread]
  wayland_client::Connection::dispatch
    └─▶ zwp_text_input_v3 listener
          ├─ preedit_string{text, ...} → push ImeEvent::Preedit(text) → channel
          ├─ commit_string{text}       → push ImeEvent::Commit(text)  → channel
          └─ done                      → flush

[main thread / event-loop tick]
  WaylandBackend::pump(&mut events) — drains channel → App
```

**Windows IMM32 preedit + commit:**

```
[OS] WM_IME_COMPOSITION (GCS_COMPSTR)
   └─▶ Subclass WndProc
         ├─ ImmGetCompositionStringW(GCS_COMPSTR) → UTF-16 → UTF-8
         ├─ push ImeEvent::Preedit(text) to thread-local queue
         └─ DefSubclassProc → tao keeps running

[OS] WM_IME_COMPOSITION (GCS_RESULTSTR)
   └─▶ Subclass WndProc
         ├─ ImmGetCompositionStringW(GCS_RESULTSTR) → UTF-16 → UTF-8
         ├─ push ImeEvent::Commit(text)
         └─ DefSubclassProc

[main thread / event-loop tick]
  WindowsBackend::pump(&mut events) — drains queue → App
```

**Fallback path (NullBackend):**

```
tao::WindowEvent::KeyboardInput → tao_key_to_bytes (Ctrl/Alt-only for Character; same as Phase 4-E)
tao::WindowEvent::ReceivedImeText(text) → App::on_ime_commit(&text)   // unchanged
```

### API Design

No HTTP / IPC API surface change. The new internal trait:

```rust
// native-poc/src/ime/backend.rs
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

pub enum ImeEvent {
    Preedit(String),
    Commit(String),
    FocusOut,
}

pub enum KeyDispatchResult {
    /// IME swallowed the key. Skip the rest of the input pipeline.
    Consumed,
    /// IME did not consume the key. Continue to `tao_key_to_bytes`.
    Passthrough,
}

#[derive(Debug)]
pub enum ImeInitError {
    Unavailable(String),         // protocol not present (no XIM server, no zwp_text_input_v3)
    HandleType(String),          // RawWindowHandle / RawDisplayHandle of wrong variant
    PlatformError(String),       // X11 / Wayland / Win32 call failed
}

pub trait ImeBackend: Send {
    fn init(window: RawWindowHandle, display: RawDisplayHandle) -> Result<Self, ImeInitError>
    where
        Self: Sized;

    fn dispatch_key_event(&mut self, raw: &RawKeyEvent) -> KeyDispatchResult;
    fn notify_cursor_rect(&mut self, x_px: i32, y_px: i32, w_px: i32, h_px: i32);
    fn notify_focus(&mut self, focused: bool);
    fn pump(&mut self, events: &mut Vec<ImeEvent>);
}

/// Captured from tao `WindowEvent::KeyboardInput` and converted into a
/// platform-neutral shape. The X11 backend rehydrates this into an
/// `XKeyPressedEvent` for `XFilterEvent`; the Wayland backend ignores it
/// (its own keyboard listener is the source of truth); the Windows
/// backend ignores it (the subclass sees raw `WM_KEYDOWN` first).
pub struct RawKeyEvent<'a> {
    pub physical_key_code: u32,   // tao `scancode` / `physical_key`
    pub state_pressed: bool,
    pub mods: crate::pty::input::Modifiers,
    pub tao_event: &'a tao::event::KeyEvent,
}
```

### Database Schema

Not applicable.

### Dependencies

**Internal:**

- `term_core` — unchanged. cursor cell coordinate read from `App::active_tab().grid()`.
- `native-poc::ime::{preedit, commit}` — Phase 4-E layer, unchanged.
- `native-poc::pty::input::Modifiers` — reused as-is.
- `native-poc::pty::PtySession` — reused as-is (`PtyWriter` impl for commit).

**External (new):**

- `x11-dl = "2"` — Xlib XIM bindings. Loaded dynamically so a non-X11 host (pure Wayland with no Xlib runtime) doesn't fail at link time. Linux-only (`[target.'cfg(all(unix, not(target_os = "macos")))'.dependencies]`).
- `wayland-client = "0.31"` + `wayland-protocols = "0.31"` (`features = ["unstable"]`) — `zwp_text_input_v3` bindings. Linux-only.
- `windows = "0.58"` — IMM32 + `SetWindowSubclass`. Windows-only (`[target.'cfg(windows)'.dependencies]`).
- `raw-window-handle` — already a workspace dep; no version bump.
- `crossbeam-channel` — already a workspace dep (used for Wayland thread → main-thread event hand-off).

No tao replacement, no egui replacement.

### File Structure

```
native-poc/
├── Cargo.toml                                # add x11-dl / wayland-{client,protocols} / windows
└── src/
    ├── app.rs                                # add ImeBackend storage + per-tick pump + notify_cursor_rect call
    ├── window_host.rs                        # add ImeBackend init + KeyboardInput dispatch_key_event + Focused notify_focus
    ├── settings.rs                           # add ime.native_integration: bool (default true)
    └── ime/
        ├── mod.rs                            # MODIFY: add `pub mod backend; pub mod null;` + cfg backends
        ├── preedit.rs                        # UNCHANGED (Phase 4-E)
        ├── commit.rs                         # UNCHANGED (Phase 4-E)
        ├── backend.rs                        # NEW: ImeBackend trait + ImeEvent + KeyDispatchResult + ImeInitError
        ├── null.rs                           # NEW: NullBackend (passthrough)
        ├── x11.rs                            # NEW: X11Backend (#[cfg(all(unix, not(target_os="macos")))])
        ├── wayland.rs                        # NEW: WaylandBackend (#[cfg(all(unix, not(target_os="macos")))])
        └── windows.rs                        # NEW: WindowsBackend (#[cfg(windows)])
```

`doc/tasks/ime-native-integration/` holds 要件定義書.md, SPEC.md, sdd.yaml at create-spec time; IMPLEMENTATION.md / VERIFICATION.md / tasks.yaml are produced by sdd.2-create-plan.

### Settings Schema

```jsonc
{
  "ime": {
    "native_integration": true   // bool; default true. When false, behave like Phase 4 fallback.
  }
}
```

### Environment Variables

- `EMTERM_NATIVE_IME=0` — equivalent to `settings.ime.native_integration = false`. Env wins over settings so users can disable per-launch without editing settings.json.

## Test Scenarios

### Unit Tests

- [ ] **TS-backend-1**: `NullBackend::dispatch_key_event` returns `Passthrough` for every key.
- [ ] **TS-backend-2**: `NullBackend::pump` produces an empty `ImeEvent` vector.
- [ ] **TS-backend-3**: `App::pump_all` (or new `App::pump_ime`) drains a fake `MockBackend`'s queue and routes events through `on_ime_{preedit,commit,focus_lost}`.
- [ ] **TS-backend-4**: When `MockBackend::dispatch_key_event` returns `Consumed`, the App's per-tick handling skips the existing `tao_key_to_bytes` path (verified via mocked `PtySession` recording no bytes).
- [ ] **TS-backend-5**: When `dispatch_key_event` returns `Passthrough`, the bytes from `tao_key_to_bytes` reach the mocked PTY exactly once (regression guard for IME-OFF).
- [ ] **TS-cursor-1**: `App` calls `ImeBackend::notify_cursor_rect` exactly once when the cursor cell coordinate changes; not called when the cursor stays put.
- [ ] **TS-focus-1**: `Focused(false)` triggers `notify_focus(false)` + `App::on_ime_focus_lost`; the active tab's `preedit_state.active()` becomes false.
- [ ] **TS-fallback-1**: `EMTERM_NATIVE_IME=0` results in `App` holding a `NullBackend` instance regardless of `settings.ime.native_integration` value.
- [ ] **TS-fallback-2**: `settings.ime.native_integration = false` (with no env var) also yields `NullBackend`.
- [ ] **TS-fallback-3**: `ImeBackend::init` returning `ImeInitError::Unavailable(_)` is caught at startup and replaced with `NullBackend`; exactly one warn log emitted.
- [ ] **TS-settings-1**: `Settings::default().ime.native_integration` が `true`。Phase 7 で JSON ローダが実装された時に missing `ime.native_integration` キーが default `true` に解決されることを担保するための shape pin (構造体レベルの test)。
- [ ] **TS-route-1** (regression of Phase 4-E): `ImeEvent::Preedit("a\x1bb")` reaches `App::on_ime_preedit` and `ime::preedit::sanitize` still strips the ESC (overlay text becomes `"ab"`).
- [ ] **TS-route-2** (regression of Phase 4-E): `ImeEvent::Commit("a\x1bb")` reaches `App::on_ime_commit`; the mocked PTY receives only `b"ab"` (no ESC), and the bytes are not wrapped in `ESC[200~ ... ESC[201~`.

### Integration Tests

- [ ] **TS-backend-int-1** (X11, `#[cfg(unix)]` + Docker host with X11): drive an `X11Backend` against a stub IM server (or `xvfb-run` + a minimal XIM responder) and assert preedit / commit events arrive in the App-side `ImeEvent` queue. Marked `#[ignore]` if no X11 display is available, similar to Phase 4 `TS-perf-*` patterns.
- [ ] **TS-backend-int-2** (Windows, `#[cfg(windows)]`): subclass on a hidden HWND, post `WM_IME_COMPOSITION` (`GCS_RESULTSTR`) manually via `SendMessageW`, assert the resulting `ImeEvent::Commit` arrives. Wayland integration test is host-deferred (compositor + fcitx5-wayland required).

### E2E Tests

Existing E2E suite (`e2e-tests/specs/*.e2e.js`) is legacy Tauri only and not relevant. Phase 4-G is verified through manual host gates:

- [ ] **TS-manual-ime-x11** (Linux X11 + fcitx5 host): launch native-poc on X11, toggle fcitx5 with `Ctrl+Space`, type `nihongo`, observe the underline preedit overlay, convert with `Space`, commit with `Enter`; confirm `日本語` reaches the shell exactly once. Also verify special chords (`Ctrl+C`, arrows, `Esc`, `Tab`) during composition behave as expected.
- [ ] **TS-manual-ime-x11-ibus** (Linux X11 + IBus host): same flow as above with IBus instead of fcitx5. Confirms XIM client works with both IM servers.
- [ ] **TS-manual-ime-wayland** (Linux Wayland + fcitx5-wayland host): same flow under a Wayland session.
- [ ] **TS-manual-ime-windows** (Windows host): same flow with MS-IME and Google IME. Candidate window position is observed for "near cursor" (best effort, not gating).
- [ ] **TS-manual-ime-fallback** (any host): launch with `EMTERM_NATIVE_IME=0`, confirm warn log + Phase 4 behavior (no overlay, ASCII keys still hit PTY through `ReceivedImeText` on Linux X11).
- [ ] **TS-manual-ime-imserver-restart** (Linux X11): kill `fcitx5`, observe warn log + automatic fallback; restart `fcitx5`, blur and refocus native-poc, confirm IME reattaches.
- [ ] **TS-manual-ime-mux** (Linux X11 + fcitx5 + emterm mux): inside an `emterm mux attach` session, type Japanese, confirm commit lands in the mux-routed PTY (no regression in mux APC inband path from Phase 4-C).

### Edge Cases

- [ ] **EC1**: `XOpenIM` returns NULL (no IM server) → `ImeInitError::Unavailable`, fallback, warn log, terminal continues.
- [ ] **EC2**: Wayland compositor missing `zwp_text_input_manager_v3` → fallback, warn log.
- [ ] **EC3**: `SetWindowSubclass` fails (rare) → fallback, warn log.
- [ ] **EC4**: 100+ character preedit composition stays in sync; sanitize doesn't drop CJK (Phase 4-E `sanitize_passes_cjk` test already pins this).
- [ ] **EC5**: composition open + window resize → preedit overlay anchor stays at the (row, col) it was anchored to; if that cell scrolls off, overlay is treated as inactive (Phase 4-E `State::set` with empty text idiom).
- [ ] **EC6**: composition open + IM server crash mid-stream → `pump` surfaces no more events, fallback warn log emitted on next failed pump; preedit overlay cleared.
- [ ] **EC7**: ASCII typing during fallback path stays on the existing Ctrl/Alt-only gating in `tao_key_to_bytes` to avoid the Linux X11 double-input regression (`3fcc7ef` behavior preserved).

### Performance Tests

- [ ] **TS-perf-3**: preedit key-press → overlay redraw latency < 30 ms (Linux X11 release host). Measured by instrumenting `App::on_ime_preedit` entry and `WindowHost::request_redraw` (using `Instant::now()` deltas, results recorded in VERIFICATION_RESULT.md).
- [ ] **TS-perf-4**: commit → `PtySession::write` < 5 ms (release host, instrumented around `App::on_ime_commit`).
- [ ] **TS-perf-regression**: Phase 4 `TS-perf-1` / `TS-perf-2` re-run; new measurement must be within +10% of the Phase 4 baseline recorded in `doc/tasks/mux-tabs-windows-ime/VERIFICATION_RESULT.md`.

## Security Considerations

- **Sanitization**: All preedit / commit strings flow through the existing `ime::preedit::sanitize` (C0 + C1 stripping). Backends are forbidden from skipping the routing layer.
- **No bracketed paste wrapping**: `ime::commit::write_commit` already enforces this; backends do not write to the PTY directly.
- **UTF-8 validation**: Backend code converts platform UTF-16 (Windows) / wide-char (X11 `XmbLookupString`) into UTF-8 with `String::from_utf16` / explicit validation, dropping invalid sequences with a warn log.
- **Resource cleanup**: `Drop` on each backend releases the IM resources (`XDestroyIC` / `XCloseIM`, `zwp_text_input_v3::destroy`, `RemoveWindowSubclass`). Required so re-init after settings change or window recreation doesn't leak.
- **No new network / FS surface**: All transport is through OS-local IPC (X11 socket, Wayland socket, Win32 message queue).
- **Settings validation**: `settings.ime.native_integration` is a `bool` with default `true`. JSON validation / unknown-value rejection is Phase 7 loader's responsibility; Phase 4-G only pins the struct shape (`ImeSettings { native_integration: bool }`).

## Error Handling

### Error Codes

| Code | Description | Severity | User-Facing Message |
|------|-------------|----------|---------------------|
| IME_E101 | `XOpenIM` returned NULL / IBus / fcitx5 not running on X11 | warn | (logged once, fall back) |
| IME_E102 | `XCreateIC` failed (unsupported IM styles) | warn | (logged once, fall back) |
| IME_E103 | XIM server disconnect mid-session | warn | (logged once, fall back; reconnect on next focus-in) |
| IME_E201 | Wayland missing `zwp_text_input_manager_v3` global | warn | (logged once, fall back) |
| IME_E202 | Wayland event pump thread panicked | error | (logged, fall back; terminal continues) |
| IME_E301 | `SetWindowSubclass` failed | warn | (logged once, fall back) |
| IME_E302 | `ImmGetCompositionStringW` returned `IMM_ERROR_GENERAL` | warn | (logged, drop this composition event) |
| IME_E401 | UTF-16 → UTF-8 conversion failure (invalid surrogate pair) | warn | (logged, drop the event) |
| IME_E901 | Backend produced more than 1024 queued events between pumps (back-pressure) | warn | (logged, drop overflow events to keep the App responsive) |

### Error Flow

```
Backend::init
    ├─ Ok(backend)   → installed
    └─ Err(reason)
         └─ log::warn!("ime: native integration disabled ({reason}), falling back to ReceivedImeText")
         └─ install NullBackend

Backend::pump (per event-loop tick)
    ├─ events drained → routed to App
    └─ on transport error (X11 disconnect, Wayland EOF):
         └─ log::warn! once
         └─ replace this backend with NullBackend
         └─ App::on_ime_focus_lost() to clear any stale preedit
```

## Performance Optimization

### Performance Goals

- preedit overlay redraw latency: < 30 ms (60 FPS / 2 frames) on Linux X11 release host.
- commit → PTY write: < 5 ms.
- IME-OFF key-down → PTY write: <= Phase 4 baseline + 10%.

### Optimization Strategies

- **Single shared X11 connection**: `X11Backend` does not open its own `XDisplay`. It borrows tao's display via `raw-window-handle` to avoid an extra socket roundtrip per key.
- **Cursor rect updates rate-limited**: `App` calls `notify_cursor_rect` only when the cursor cell (row, col) changes, not every frame.
- **Wayland event pump on a dedicated thread**: keeps the egui render loop free of Wayland blocking calls. The main thread drains a `crossbeam_channel::Receiver` once per tick.
- **Windows subclass passthrough**: every message that isn't `WM_IME_*` is forwarded immediately via `DefSubclassProc`, so non-IME paths see zero extra overhead.
- **Pump rate limiting**: `ImeBackend::pump` is bounded — at most 1024 events drained per tick; overflow is dropped with a warn log (IME_E901) to keep the App responsive if a faulty IM server flood-spams events.

### Caching Strategy

- preedit text is owned by `ime::preedit::State` (Phase 4-E) and replaced on each `set`. No additional caching needed.
- `RawWindowHandle` / `RawDisplayHandle` are captured once at `App` startup and held by the backend for its lifetime (no per-event lookup).

## Success Criteria

- [ ] FR1-FR10 implemented; all unit + integration tests above pass.
- [ ] `cargo build --workspace` succeeds on Linux + Windows.
- [ ] `cargo test --workspace` exits 0.
- [ ] `cargo fmt --all -- --check` clean.
- [ ] `cargo clippy -p emterm-native-poc -- -D warnings` zero errors. Forward-staged warnings only if documented in `sdd.yaml` notes.
- [ ] `TS-manual-ime-x11`, `TS-manual-ime-x11-ibus`, `TS-manual-ime-wayland`, `TS-manual-ime-windows`, `TS-manual-ime-fallback`, `TS-manual-ime-imserver-restart`, `TS-manual-ime-mux` pass on host.
- [ ] `TS-perf-3` / `TS-perf-4` meet targets on release host.
- [ ] `TS-perf-regression` shows IME-OFF latency within +10% of Phase 4 baseline.
- [ ] Phase 4-E `ime::preedit::State` / `ime::commit::write_commit` are unchanged (verified by `git diff` showing no edits to those files beyond doc-comment-level changes).
- [ ] Legacy `src-tauri` build / test unaffected (NFR5).

## Open Questions

The clarifications below are recorded as deferred and resolved during sdd.2-create-plan / implementation:

- [ ] **OQ1**: X11 crate selection — `x11-dl` (dynamic loading, no link-time X dependency) vs `x11rb` (safer Rust API, but stricter link-time). Default proposal: `x11-dl` because XIM specifically requires Xlib (no XCB equivalent) and dynamic loading lets non-X hosts skip the link.
- [ ] **OQ2**: Wayland binding choice — `wayland-client` direct vs `smithay-client-toolkit`. Default proposal: `wayland-client` direct for minimum surface; `zwp_text_input_v3` is small enough that SCTK is overkill.
- [ ] **OQ3**: Wayland compositor support matrix — at minimum verify KDE Plasma 6 (KWin) and a wlroots-based compositor (e.g. Sway) with fcitx5-wayland. GNOME's `gnome-shell` is best effort because of well-known fcitx5-wayland regressions there.
- [ ] **OQ4**: Windows TSF — out of scope for Phase 4-G; revisit only if MS-IME / Google IME show concrete gaps under IMM32 in manual gates.
- [ ] **OQ5**: Whether `notify_cursor_rect` needs sub-cell precision for proportional fonts. Not relevant today (native-poc grids are monospace), so cell-aligned pixel rect is sufficient.

## Implementation Phases

The plan author (sdd.2-create-plan) will subdivide further. Suggested ordering, matching the Phase 4-G staged-rollout requirement in 要件定義書.md §1.4:

### Sub-Phase 4-G-A: backend scaffolding + NullBackend + fallback wiring

**Goals**: Land the `ImeBackend` trait, `NullBackend`, App-side pump, settings + env-var fallback wiring, regression tests. No platform IME yet.

**Deliverables:**
- `native_poc::ime::{backend, null}` modules.
- `App` carries `Box<dyn ImeBackend>` + per-tick pump.
- `EMTERM_NATIVE_IME` env var + `settings.ime.native_integration` field.
- TS-backend-{1,2,3,4,5}, TS-cursor-1, TS-focus-1, TS-fallback-{1,2,3}, TS-settings-1, TS-route-{1,2} pass.
- `cargo test --workspace` green; no platform IME behavior change vs Phase 4.

### Sub-Phase 4-G-B: Linux X11 (XIM) backend

**Goals**: First platform backend. The Go/No-Go decision point for Phase 4-G.

**Deliverables:**
- `native_poc::ime::x11` module.
- `x11-dl` workspace dep gated `target_os = linux`.
- `RawDisplayHandle::Xlib` runtime probe → use `X11Backend`.
- TS-backend-int-1 passes; manual TS-manual-ime-x11 + TS-manual-ime-x11-ibus pass on host.
- Phase 1 fcitx5 acceptance criteria (NFR8) recorded in VERIFICATION_RESULT.md.

### Sub-Phase 4-G-C: Linux Wayland (zwp_text_input_v3) backend

**Goals**: Wayland parity with X11.

**Deliverables:**
- `native_poc::ime::wayland` module + Wayland event pump thread.
- `wayland-client` / `wayland-protocols` workspace deps gated `target_os = linux`.
- `RawDisplayHandle::Wayland` runtime probe → use `WaylandBackend`.
- Manual TS-manual-ime-wayland passes on KDE Plasma 6 + Sway with fcitx5-wayland.

### Sub-Phase 4-G-D: Windows IMM32 backend

**Goals**: Windows parity (best-effort candidate window).

**Deliverables:**
- `native_poc::ime::windows` module + window subclass.
- `windows` crate workspace dep gated `target_os = windows`.
- TS-backend-int-2 passes; manual TS-manual-ime-windows passes with MS-IME + Google IME.

### Sub-Phase 4-G-E: final gates + docs

**Goals**: clippy / fmt / docs / VERIFICATION_RESULT.

**Deliverables:**
- `cargo clippy -p emterm-native-poc -- -D warnings` clean.
- `cargo fmt --all -- --check` clean.
- TS-perf-3 / TS-perf-4 / TS-perf-regression recorded in VERIFICATION_RESULT.md.
- `doc/tasks/mux-tabs-windows-ime/sdd.yaml` `NFR3` and `FR11` / `FR12` manual gates re-evaluated and marked accordingly (cross-task update; left as a notes-only follow-up — the actual gate status flip lives in the mux-tabs-windows-ime sdd, not in this one).
- README updated with Phase 4-G feature matrix.

## References

- restruct.md Phase 4-G section: `tmp/restruct.md` (lines covering `Phase 4-G: IME 連携プロトコル自前実装`).
- Phase 4 SPEC: `doc/tasks/mux-tabs-windows-ime/SPEC.md` (FR11 / FR12 / NFR3 deferred state).
- Phase 4 sdd.yaml: `doc/tasks/mux-tabs-windows-ime/sdd.yaml` (recorded deferral with reason).
- Phase 1 WebView IME SPEC: `doc/tasks/ime-input-support/SPEC.md` (parity reference for fcitx5).
- SKK freeze fix: `doc/tasks/skk-ime-freeze-fix/SPEC.md` (historical IME behavior).
- Existing routing layer: `native-poc/src/ime/{mod.rs, preedit.rs, commit.rs}`.
- Existing event loop seam: `native-poc/src/window_host.rs` (`tao_key_to_bytes`, `WindowEvent::ReceivedImeText`, `WindowEvent::Focused`).
- App-side IME routes: `native-poc/src/app.rs` (`on_ime_preedit`, `on_ime_commit`, `on_ime_focus_lost`).
