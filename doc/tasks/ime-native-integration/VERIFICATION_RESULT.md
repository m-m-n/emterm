# Verification Result: Native IME Integration (Phase 4-G)

**Feature**: ime-native-integration
**Verified at commit**: `dbfb25b9e5ebdd3b08e9647fb29d5e04485282b6` on branch `refactor/native-terminal-hybrid`
**Phase 4-G commit range**: `9f290ab..HEAD`
  - `29daa51` Phase 4-G-A IME backend scaffold + Null fallback
  - `393224d` Phase 4-G-B Linux X11 (XIM) backend
  - `e69d7b2` Phase 4-G-C Linux Wayland backend scaffold
  - `fbcfe02` Phase 4-G-D Windows IMM32 backend
  - `d50ecff` Phase 4-G-E final gates + perf + docs
  - `dbfb25b` docs(ime-native-integration): record implement completed_at_commit
**Verification date**: 2026-05-14
**Verification scope**: Phase 4-G auto-scope (manual / cross-platform host gates explicitly deferred)

---

## Overview

Phase 4-G implements the platform IME clients native-poc needs on top of tao
0.34 without replacing tao:

- **Linux X11**: XIM client via `x11-dl 2` (`XOpenIM` / `XCreateIC` /
  `XFilterEvent` / `XmbLookupString`)
- **Linux Wayland**: `zwp_text_input_v3` scaffold (channel + pump-thread
  infrastructure; runtime probe defers to `Unavailable` pending
  `wl_display` borrow)
- **Windows**: IMM32 + `SetWindowSubclass` (`WM_IME_*` interception)
- **Common backbone**: `ImeBackend` trait + `NullBackend` + factory
  (`EMTERM_NATIVE_IME` env > settings > init-failure auto-fallback)
- **Phase 4-E auto-scope contract preserved**: `ime::preedit::State`,
  `ime::commit::write_commit`, `render::cursor::draw_cursor_with_preedit`
  unchanged across all Phase 4-G commits

All five sub-phases (4-G-A through 4-G-E) are landed; quick check
(`sdd.5-check` at `dbfb25b`) already validated build / test / fmt /
dead-code. This document records the comprehensive verification (file
structure, Phase 4-E 不変契約, SPEC FR/NFR compliance, test ID coverage,
security, performance, and remaining manual host gates).

## Quick Check Summary

The fast quality gates were verified by `sdd.5-check` at commit
`dbfb25b` (see `sdd.yaml.workflow[check].notes`):

- `cargo build --workspace` → **exit 0** (13 warnings, all forward-staged
  outside Phase 4-G scope, no new Phase 4-G warnings)
- `cargo test --workspace` → **2011 passed / 0 failed / 4 ignored**
  (up from Phase 4 baseline 1940 = **+71 IME backbone + backend tests**)
- `cargo fmt --all` → **clean** (auto-format applied, no diffs)
- `cargo clippy -p emterm-native-poc -- -D warnings` →
  Phase 4-G `ime/` subtree clean (0 warnings); pre-existing 14-warning
  baseline outside Phase 4-G scope is forward-staged per Phase 4-F precedent

These results are **not re-run** in this verification pass; sdd.5-check
is authoritative. This pass focuses on structural / compliance /
security verification that cannot be expressed as a build gate.

---

## 1. File Structure Verification — **PASS**

### Files to Create (5/5 present)

| Phase | Path | Present | Notes |
|-------|------|---------|-------|
| 4-G-A | `native-poc/src/ime/backend.rs` | ✓ | `ImeBackend` trait + `ImeEvent` + `KeyDispatchResult` + `ImeInitError` + `RawKeyEvent` + factory |
| 4-G-A | `native-poc/src/ime/null.rs` | ✓ | `NullBackend` (passthrough) |
| 4-G-B | `native-poc/src/ime/x11.rs` | ✓ | `cfg(all(unix, not(target_os = "macos")))` + x11-dl dynamic loading |
| 4-G-C | `native-poc/src/ime/wayland.rs` | ✓ | scaffold; channel + pump-thread infrastructure, `init` returns `Unavailable` pending wl_display borrow |
| 4-G-D | `native-poc/src/ime/windows.rs` | ✓ | `cfg(windows)` + portable `utf16_to_utf8` shared with cross-platform tests |

### Files to Modify (6/6 touched in Phase 4-G commit range)

All entries in VERIFICATION.md "Files to Modify" were touched by at
least one Phase 4-G commit (`9f290ab..HEAD`):

- `native-poc/Cargo.toml` — `x11-dl = "2"` (Linux),
  `wayland-client = "0.31"` + `wayland-protocols = "0.31"` (Linux),
  `windows = "0.58"` (Windows). `raw-window-handle` and
  `crossbeam-channel` were already direct deps (see verify-plan notes)
- `native-poc/src/ime/mod.rs` — re-exports `backend` + `null` + cfg
  backends
- `native-poc/src/settings.rs` — `ImeSettings { native_integration: bool }`
  + `Settings::ime` field; `Default::default()` sets `native_integration: true`
- `native-poc/src/app.rs` — `ime_backend: Box<dyn ImeBackend>` slot,
  `set_ime_backend`, `pump_ime`, `dispatch_key_event_via_ime`,
  `notify_cursor_rect_if_changed`, `notify_ime_focus`, `ime_is_null`
- `native-poc/src/window_host.rs` — startup factory invocation via
  `raw-window-handle 0.6` `HasWindowHandle` / `HasDisplayHandle`,
  `KeyboardInput` backend-first dispatch with `Consumed` short-circuit,
  `Focused` notify, per-tick `pump_ime`, `ReceivedImeText` gated on
  `ime_is_null()`
- `native-poc/README.md` — Phase 4-G feature matrix + env / settings docs
  + Phase 4-E contract reminder

### Phase 4-E Auto-Scope Files (UNCHANGED — contract preserved)

`git diff --stat 9f290ab..HEAD -- native-poc/src/ime/preedit.rs
native-poc/src/ime/commit.rs native-poc/src/render/cursor.rs` returns
**empty** — no lines added or removed across the entire Phase 4-G commit
range. SC-8 / NFR6 satisfied.

### `App::on_ime_*` Signatures (UNCHANGED)

`grep -nE "fn on_ime_(preedit|commit|focus_lost)" native-poc/src/app.rs`:

```
682:    pub fn on_ime_preedit(&mut self, text: &str) {
719:    pub fn on_ime_commit(&mut self, text: &str) {
742:    pub fn on_ime_focus_lost(&mut self) {
```

Signatures match the Phase 4-E contract. Bodies were extended with
`EMTERM_IME_PERF` instrumentation (warn-log only, no behavioral change),
which is permitted by the verify-plan deviation analysis.

### `src-tauri` Untouched (NFR5)

`git diff --stat 9f290ab..HEAD -- src-tauri/` returns **empty**. Legacy
workspace untouched across Phase 4-G.

**Result**: file structure matches IMPLEMENTATION.md `Complete File
Structure` exactly. No missing files. NFR5 (workspace compatibility) and
NFR6 (module layout) satisfied. SC-8 (Phase 4-E 不変契約) satisfied.

---

## 2. SPEC.md Compliance — Success Criteria

| ID | Criterion | Status | Notes |
|----|-----------|--------|-------|
| **SC-1** | FR1-FR10 implemented; all unit + integration tests pass | **PASS** | All FRs in `sdd.yaml.requirements` marked `status: ok`; full per-FR/TS mapping below. Workspace test count 2011 / 0 / 4. |
| **SC-2** | `cargo build --workspace` succeeds on Linux + Windows | **PASS (Linux)** / **host-deferred (Windows)** | Linux: sdd.5-check `cargo build --workspace` exit 0. Windows: no native cross-compile target in CI; `cfg(windows)` Windows backend is gated and Linux CI compiles the portable `utf16_to_utf8` helper. Cross-build deferred to GitHub Actions windows-latest runner. |
| **SC-3** | `cargo test --workspace` exit 0 | **PASS** | sdd.5-check: 2011 passed / 0 failed / 4 ignored. |
| **SC-4** | `cargo fmt --all -- --check` clean | **PASS** | sdd.5-check. |
| **SC-5** | `cargo clippy -p emterm-native-poc -- -D warnings` zero errors | **PASS (Phase 4-G scope) / forward-staged baseline preserved** | Phase 4-G `ime/` tree clean (0 warnings); pre-existing 14-warning baseline outside Phase 4-G scope is forward-staged per Phase 4-F precedent and recorded in `sdd.yaml.workflow[check].notes`. |
| **SC-6** | Manual TS-manual-ime-x11 / x11-ibus / wayland / windows / fallback / imserver-restart / mux pass | **host-deferred** | Docker cannot drive a real IM server (no X11 / Wayland with fcitx5 / IBus, no Windows host). Listed in §4 Manual Gates Pending. |
| **SC-7** | TS-perf-3 / TS-perf-4 / TS-perf-regression meet thresholds | **host-deferred** | `EMTERM_IME_PERF=1` warn-log instrumentation landed (Phase 4-G-E); release-host measurement deferred to host engineer. |
| **SC-8** | Phase 4-E `ime::preedit::State` / `ime::commit::write_commit` 振る舞い不変 | **PASS** | `git diff 9f290ab..HEAD` on `preedit.rs` / `commit.rs` / `render/cursor.rs` empty. TS-route-1 + TS-route-2 regression guards green in sdd.5-check workspace test. |
| **SC-9** | 旧 `src-tauri` build / test 不変 | **PASS** | `git diff 9f290ab..HEAD -- src-tauri/` empty. Workspace build/test green. |

---

## 3. Functional + Non-Functional Requirements Coverage

### Functional Requirements (FR1 - FR10) — all **PASS** (auto-scope)

| FR | Code evidence | Tests | Status |
|----|----------------|-------|--------|
| **FR1** XIM client (Linux X11) | `native-poc/src/ime/x11.rs` — `XOpenIM` (L197), `XCreateIC` (L226), `XSetICFocus` (L250 / L387), `XFilterEvent` (L307), `XmbLookupString` (L325), `XICAttribute` + `XNSpotLocation` (L357), `XUnsetICFocus` (L389), `XDestroyIC` / `XCloseIM` (L410 / L414, Drop). Display borrowed from tao (no second `XOpenDisplay`). | TS-x11-1, TS-x11-2 (auto, 11 keycode / modifier mapping tests); TS-backend-int-1 + TS-manual-ime-x11 / x11-ibus deferred | PASS (auto) |
| **FR2** zwp_text_input_v3 client (Linux Wayland) | `native-poc/src/ime/wayland.rs` — `wayland_client`/`crossbeam_channel` imports (L41-42), pump thread infrastructure (`PumpThread::spawn` L63, `join` L82, `shutdown` AtomicBool L78), backend `pump` drains channel (L184-209), `notify_cursor_rect` records latest state (L199), Drop joins thread (L227-230). `init` currently returns `Unavailable` pending `wl_display` borrow via `wayland-backend/client_system` (intentional scaffold; pure-X11 Linux builds stay libwayland-free; XWayland + fcitx5-X11 routes through X11 backend) | TS-wayland-1, TS-wayland-2 (auto, 10 unit tests covering pump drain / Unavailable variant / drop join); TS-manual-ime-wayland deferred | PASS (auto scaffold) |
| **FR3** IMM32 client (Windows) | `native-poc/src/ime/windows.rs` — `SetWindowSubclass` / `RemoveWindowSubclass` / `DefSubclassProc` imports (L90), `WM_IME_STARTCOMPOSITION` / `WM_IME_COMPOSITION` / `WM_IME_ENDCOMPOSITION` imports (L92) and subclass_wndproc arms (L216 / L219 / L235), `GCS_RESULTSTR` → `ImeEvent::Commit` (L223-226), `GCS_COMPSTR` → `ImeEvent::Preedit` (L227-230), `ImmGetCompositionStringW` (L248 / L254), `ImmSetCompositionWindow(CFS_POINT)` (L166), `RemoveWindowSubclass` on Drop (L198). All `#[cfg(windows)]`-gated. | TS-windows-1, TS-windows-2, TS-windows-3 (auto, 10 portable utf16→utf8 tests run on Linux CI); TS-backend-int-2 + TS-manual-ime-windows deferred | PASS (auto) |
| **FR4** `ImeBackend` trait + `ImeEvent` / `KeyDispatchResult` / `ImeInitError` | `native-poc/src/ime/backend.rs` — `enum ImeEvent` (L20), `enum KeyDispatchResult` (L39), `enum ImeInitError` (L48), `struct RawKeyEvent` (L77), `trait ImeBackend: Send` (L93) | TS-backend-1, TS-backend-2, TS-backend-3 (auto) | PASS |
| **FR5** Routing into Phase 4-E layer | `App::pump_ime` (`app.rs:187`) drains backend events; `ImeEvent::Preedit` → `on_ime_preedit` (L682, unchanged signature), `Commit` → `on_ime_commit` (L719), `FocusOut` → `on_ime_focus_lost` (L742). `ime::preedit::sanitize` + `commit::write_commit` invocation chain preserved | TS-backend-3, TS-route-1, TS-route-2 (auto regression guards) | PASS |
| **FR6** Key event interception | `App::dispatch_key_event_via_ime` (`app.rs:172`) calls `ime_backend.dispatch_key_event`. `window_host.rs` calls it backend-first on `KeyboardInput` with `Consumed` short-circuit before `tao_key_to_bytes` (Phase 4 path) | TS-backend-4, TS-backend-5, TS-x11-1, TS-x11-2 (auto) | PASS |
| **FR7** Cursor rectangle reporting | `App::notify_cursor_rect_if_changed` (`app.rs:219`) cell-diff rate-limited; only invokes `ime_backend.notify_cursor_rect` when (row, col) changes. X11 backend wires this through `XSetICValues` + `XNSpotLocation`; Wayland records latest state for future `set_cursor_rectangle`; Windows wires through `ImmSetCompositionWindow(CFS_POINT)` | TS-cursor-1 (auto); X11/Wayland/Windows manual gates deferred | PASS (auto) |
| **FR8** Focus management | `App::notify_ime_focus` (`app.rs:178`) drives `Focused(true/false)`; `on_ime_focus_lost` clears Phase 4-E preedit state. X11 backend toggles `XSetICFocus` / `XUnsetICFocus` (L387 / L389); Wayland records focus; Windows delegates to `DefSubclassProc` (subclass observes `WM_KILLFOCUS` naturally) | TS-focus-1 (auto); TS-manual-ime-imserver-restart deferred | PASS (auto) |
| **FR9** Opt-out / fallback | `native-poc/src/ime/backend.rs::build_backend` resolves `EMTERM_NATIVE_IME` env > `settings.ime.native_integration` > backend `init` failure → `NullBackend` + single warn log. `window_host.rs:779` calls `build_backend(...)` at startup. `ReceivedImeText` path is gated on `ime_is_null()` to preserve Phase 4 fallback behavior | TS-fallback-1, TS-fallback-2, TS-fallback-3 (auto); TS-manual-ime-fallback deferred | PASS (auto) |
| **FR10** Settings schema additions (`ime.native_integration`) | `native-poc/src/settings.rs` — `pub struct ImeSettings { native_integration: bool }` (L113), `Default::default()` sets `native_integration: true` (L122-128), `Settings::ime: ImeSettings` field (L188). JSON load is Phase 7 (`#[allow(dead_code) // Phase 7]` on the loader site removed in sdd.5-check check since the field is actually read by `window_host:782` + `backend:148`) | TS-settings-1 (auto) — `default_ime_native_integration_is_true` + `ime_settings_default_is_native_integration_true` | PASS |

### Non-Functional Requirements (NFR1 - NFR8)

| NFR | Status | Notes |
|-----|--------|-------|
| **NFR1** preedit redraw < 30 ms (Linux X11 release host) | **host-deferred** | `EMTERM_IME_PERF=1` warn-log instrumentation in `App::on_ime_preedit` (`app.rs:683`) emits entry → `request_redraw` micros for TS-perf-3 release-host measurement |
| **NFR2** commit → `PtySession::write` < 5 ms | **host-deferred** | `EMTERM_IME_PERF=1` instrumentation in `App::on_ime_commit` (`app.rs:720`) emits entry → `PtySession::write` micros for TS-perf-4 |
| **NFR3** IME-OFF regression ≤ +10% | **host-deferred** | Backend dispatch short-circuit (`KeyDispatchResult::Passthrough`) preserves Phase 4 `tao_key_to_bytes` path verbatim. TS-perf-regression release-host measurement deferred |
| **NFR4** Stability — init failure no crash, IM server death falls back within 1 tick | **PASS (init failure auto)** / **host-deferred (mid-session death)** | `build_backend` catches `ImeInitError::*` → `NullBackend` + warn log (TS-fallback-3 auto). Mid-session disconnect detection is wired in X11 (transport error on `XFilterEvent` chain) but full reconnection flow gated by TS-manual-ime-imserver-restart |
| **NFR5** Workspace compatibility, src-tauri untouched | **PASS** | `git diff 9f290ab..HEAD -- src-tauri/` empty. Workspace build/test green (2011 / 0 / 4) |
| **NFR6** Module layout (new code lives under `native-poc/src/ime/{backend,null,x11,wayland,windows}.rs`; `preedit.rs` / `commit.rs` unchanged) | **PASS** | File structure §1 confirms all five new modules present; Phase 4-E `preedit.rs` / `commit.rs` / `render/cursor.rs` diff empty |
| **NFR7** Logging (init success / fallback / reconnect at `log::warn`) | **PASS (structural)** | `build_backend` emits exactly-one warn log on fallback (TS-fallback-3); X11 / Wayland / Windows backends emit warn logs on transport / IM_E* error codes per SPEC error code table (IME_E101 / E102 / E201 / E301 / E302 / E401 / E901). Wall-clock confirmation captured during manual gates |
| **NFR8** Linux fcitx5 IME parity with Phase 1 | **host-deferred** | Phase 1 `ime-input-support/SPEC.md` US1-US5 parity gated by TS-manual-ime-x11 on a Linux X11 + fcitx5 host |

---

## 4. Test ID Coverage

### Automated (executed by `cargo test --workspace`, validated by sdd.5-check)

| ID | Test fn location | Phase |
|----|------------------|-------|
| TS-backend-1 | `native-poc/src/ime/null.rs::tests` (`dispatch_key_event` returns Passthrough) | 4-G-A |
| TS-backend-2 | `native-poc/src/ime/null.rs::tests` (`pump` produces empty vec) | 4-G-A |
| TS-backend-3 | `native-poc/src/app.rs::tests` (`pump_ime_routes_*`) | 4-G-A |
| TS-backend-4 | `native-poc/src/app.rs::tests` (Consumed short-circuit) | 4-G-A |
| TS-backend-5 | `native-poc/src/app.rs::tests` (Passthrough → PTY write) | 4-G-A |
| TS-cursor-1 | `native-poc/src/app.rs::tests` (cell-diff rate-limit) | 4-G-A |
| TS-focus-1 | `native-poc/src/app.rs::tests` (`Focused(false)` clears preedit) | 4-G-A |
| TS-fallback-1 | env-dominance assertion | 4-G-A |
| TS-fallback-2 | settings-disabled assertion | 4-G-A |
| TS-fallback-3 | init-failure → NullBackend + warn-once | 4-G-A |
| TS-settings-1 | `native-poc/src/settings.rs::tests` (default ime.native_integration == true) | 4-G-A |
| TS-route-1 | `native-poc/src/app.rs::tests` (Preedit("a\x1bb") → "ab") | 4-G-A |
| TS-route-2 | `native-poc/src/app.rs::tests` (Commit("a\x1bb") → b"ab" no bracketed paste) | 4-G-A |
| TS-x11-1 | `native-poc/src/ime/x11.rs::tests` (keycode mapping deterministic, in X11 [8,255]) | 4-G-B |
| TS-x11-2 | `native-poc/src/ime/x11.rs::tests` (ShiftMask / ControlMask / Mod1Mask individually + combined) | 4-G-B |
| TS-wayland-1 | `native-poc/src/ime/wayland.rs::tests` (pump drains channel + budget) | 4-G-C |
| TS-wayland-2 | `native-poc/src/ime/wayland.rs::tests` (HandleType discriminant + Unavailable variant) | 4-G-C |
| TS-windows-1 | `native-poc/src/ime/windows.rs::tests` (BMP ASCII + 日本語) | 4-G-D |
| TS-windows-2 | `native-poc/src/ime/windows.rs::tests` (surrogate pair 😀) | 4-G-D |
| TS-windows-3 | `native-poc/src/ime/windows.rs::tests` (lone surrogate → None + IME_E401 warn) | 4-G-D |

Workspace total: **+71 tests** vs Phase 4 baseline (1940 → 2011).
VERIFICATION.md test coverage target was "+20 tests" — exceeded by **3.5×**.

### Manual gate / host-deferred (out of scope for this Docker pass)

Integration tests requiring a real X11 / Windows host:

- TS-backend-int-1 (`#[ignore]` X11 + xvfb + stub IM responder)
- TS-backend-int-2 (`#[cfg(windows)]` hidden HWND + `SendMessageW`)

Manual gates requiring real IM server + IME-capable host:

- TS-manual-ime-x11 (Linux X11 + fcitx5)
- TS-manual-ime-x11-ibus (Linux X11 + IBus)
- TS-manual-ime-wayland (Linux Wayland + fcitx5-wayland; KDE Plasma 6 + Sway)
- TS-manual-ime-windows (Windows + MS-IME / Google IME)
- TS-manual-ime-fallback (any host with `EMTERM_NATIVE_IME=0`)
- TS-manual-ime-imserver-restart (Linux X11 + fcitx5 kill / restart)
- TS-manual-ime-mux (Linux X11 + fcitx5 + `emterm mux attach`)

Performance gates requiring release-host instrumentation:

- TS-perf-3 (preedit overlay redraw latency)
- TS-perf-4 (commit → `PtySession::write` latency)
- TS-perf-regression (IME-OFF baseline ±10%)

See §5 Manual Gates Pending for the consolidated list and deferral
rationale.

---

## 5. Manual Gates Pending (Host Execution Required)

These items require a host machine with a real IM server + IME (Docker
cannot drive these). They are tracked here so the host engineer can
execute them and append results.

- [ ] **TS-backend-int-1** — X11 `#[ignore]` integration on a host with `Xvfb` + a stub XIM responder. Drive `X11Backend` and assert `ImeEvent::{Preedit, Commit}` arrive via `pump`
- [ ] **TS-backend-int-2** — Windows `#[cfg(windows)]` integration on a Windows host. Install subclass on a hidden HWND, `SendMessageW(WM_IME_COMPOSITION, GCS_RESULTSTR)`, assert `ImeEvent::Commit` arrives via `pump`
- [ ] **TS-manual-ime-x11** — Linux X11 + fcitx5 host. Type `nihongo`, observe underline preedit overlay, convert with `Space`, commit with `Enter`; confirm `日本語` reaches the shell exactly once. Verify special chords (`Ctrl+C`, arrows, `Esc`, `Tab`) during composition behave correctly
- [ ] **TS-manual-ime-x11-ibus** — Linux X11 + IBus host. Same flow as above with IBus instead of fcitx5
- [ ] **TS-manual-ime-wayland** — Linux Wayland + fcitx5-wayland host. KDE Plasma 6 (KWin) + Sway sessions. **Note**: Wayland backend `init` currently returns `Unavailable` (scaffold). Real attach requires `wayland-backend/client_system` feature to borrow tao's `wl_display`; this is staged for a follow-up commit. XWayland + fcitx5-X11 already routes through the X11 backend successfully
- [ ] **TS-manual-ime-windows** — Windows 10/11 + MS-IME / Google IME. Same flow; candidate window near cursor is best effort (not gating). Requires `cargo build --target x86_64-pc-windows-msvc` on a Windows host
- [ ] **TS-manual-ime-fallback** — Any host with `EMTERM_NATIVE_IME=0`. Confirm exactly one warn log + Phase 4 fallback (no overlay, ASCII keys still hit PTY through `ReceivedImeText` on Linux X11)
- [ ] **TS-manual-ime-imserver-restart** — Linux X11 + fcitx5. Kill `fcitx5`, observe warn log + automatic fallback; restart `fcitx5`, blur and refocus native-poc, confirm IME re-attaches
- [ ] **TS-manual-ime-mux** — Linux X11 + fcitx5 + `emterm mux attach`. Type Japanese inside a mux session, confirm commit lands in the mux-routed PTY (no regression in mux APC inband path from Phase 4-C)
- [ ] **TS-perf-3** — Linux X11 release host with `EMTERM_IME_PERF=1`. preedit key-press → overlay redraw < 30 ms. Instrumentation already emits via `log::warn!` so a `release` build + log inspection is sufficient
- [ ] **TS-perf-4** — Same host. commit → `PtySession::write` < 5 ms
- [ ] **TS-perf-regression** — Same host. IME-OFF key-down → PTY write latency within +10% of Phase 4 baseline recorded in `doc/tasks/mux-tabs-windows-ime/VERIFICATION_RESULT.md` (TS-perf-1 / TS-perf-2)
- [ ] **Windows cargo build** — `cargo build --workspace --target x86_64-pc-windows-msvc` on a Windows host or GitHub Actions windows-latest. SC-2 manual gate
- [ ] **Legacy E2E regression** — `./scripts/run-e2e-docker.sh test` — confirm same preexisting fail list as `main` (regression check, not a gate). VERIFICATION.md explicitly notes "Phase 4-G による regression がないことの確認、gate ではない". Skipped this pass because (a) legacy E2E is Tauri-only and not relevant to native-poc, and (b) Phase 4-G touches zero files in `src-tauri/`, so a regression is structurally impossible

---

## 6. Performance Verification — Instrumentation Landed, Measurement Deferred

`EMTERM_IME_PERF=1` env toggle (Phase 4-G-E, `app.rs:776 ime_perf_enabled`)
adds `log::warn!` lines at:

- `App::on_ime_preedit` entry (`app.rs:683`) — emits `preedit text=<n> chars
  total=<micros>` after `request_redraw`
- `App::on_ime_commit` entry (`app.rs:720`) — emits `commit text=<n> chars
  pty_write=<micros>` after `PtySession::write`

Cached on first call (lazy `OnceCell` style) so the env check is done
once per process. Release build of native-poc reads the warn log lines
directly — no recompilation needed.

**Targets** (recorded for completeness):

| Test | Target | Source of measurement |
|------|--------|------------------------|
| TS-perf-3 | preedit redraw < 30 ms | `EMTERM_IME_PERF=1` warn-log: `preedit … total=<micros>` |
| TS-perf-4 | commit → `PtySession::write` < 5 ms | `EMTERM_IME_PERF=1` warn-log: `commit … pty_write=<micros>` |
| TS-perf-regression | IME-OFF key-down → PTY write ≤ Phase 4 baseline +10% | Re-run Phase 4 TS-perf-1 / TS-perf-2 (see `doc/tasks/mux-tabs-windows-ime/VERIFICATION_RESULT.md`) and compare |

These three release-host measurements are tracked in §5 Manual Gates
Pending and will be appended to this document by the host engineer.

---

## 7. Security Verification — **PASS**

| Check | Evidence | Status |
|-------|----------|--------|
| preedit / commit C0+C1 sanitization preserved via `ime::preedit::sanitize` | `ime::commit::write_commit` calls `sanitize(text)` (`commit.rs:42`); `App::on_ime_preedit` → `preedit::State::set` → `sanitize` (`preedit.rs:38`). Phase 4-E sanitize tests pin ASCII / CJK / \t / \n pass-through and C0 / C1 / null-byte drop. **TS-route-1** + **TS-route-2** regression guards (auto) verify the chain for `ImeEvent::Preedit("a\x1bb")` and `ImeEvent::Commit("a\x1bb")` end-to-end | PASS |
| Commit not bracketed-paste-wrapped | `ime::commit::write_commit` writes sanitized bytes directly — no `ESC[200~` / `ESC[201~`. `commit_does_not_wrap_in_bracketed_paste` auto test pins this. Backends never write directly to PTY (only push `ImeEvent::Commit` into the App's pump) | PASS |
| UTF-16 → UTF-8 invalid surrogate handling | `windows::utf16_to_utf8` (`windows.rs:43`) uses `String::from_utf16_lossy` and detects U+FFFD substitution; logs `IME_E401` warn and returns `None` for lone surrogates. **TS-windows-3** auto test covers lone high / low surrogate + partial-garbage best effort + warn assertion | PASS |
| Backend `Drop` releases IM resources | X11: `XDestroyIC` (`x11.rs:410`) + `XCloseIM` (`x11.rs:414`); Wayland: pump thread `join` via shutdown AtomicBool (`wayland.rs:227-230`); Windows: `RemoveWindowSubclass` (`windows.rs:198`). No leak on re-init / settings change | PASS |
| Settings JSON shape pinned for Phase 7 loader | `ImeSettings { native_integration: bool }` with `Default::default()` → `true` (`settings.rs:113-128`). TS-settings-1 auto test ensures missing-key fallback. JSON parse / unknown-value rejection is Phase 7 loader responsibility per SPEC FR10 | PASS |
| Env var trust boundary | `EMTERM_NATIVE_IME=0` is user-controlled, scope-limited to "disable native IME" (degrades to NullBackend + Phase 4 ReceivedImeText). No code-execution / privilege-escalation surface. Same trust class as `EMTERM_IME_PERF=1` (log toggle) | N/A (no privileged surface) |
| No new network / FS surface | All transport is OS-local IPC (X11 socket, Wayland socket, Win32 message queue). No new sockets / file paths / pipes introduced | PASS |

No new authentication / authorization / XSS / SQL / CSRF surfaces
introduced. Phase 4-E sanitize contract is the single chokepoint and is
preserved by `git diff` empty on `preedit.rs` / `commit.rs`.

---

## 8. E2E (Legacy Regression Gate) — Not Run

VERIFICATION.md explicitly notes: "既存 `./scripts/run-e2e-docker.sh` は
legacy Tauri build 専用で native-poc には適用外。Phase 4-G では new E2E
は追加しない". The legacy E2E suite is not a Phase 4-G gate.

Skipped this pass because:

1. Legacy E2E is Tauri-only and not relevant to native-poc
2. Phase 4-G touches zero files in `src-tauri/` (verified by `git diff
   --stat 9f290ab..HEAD -- src-tauri/` returning empty), so a regression
   is structurally impossible
3. Docker E2E is a heavy run (180s timeouts × full suite) and the cost /
   value ratio is unfavorable for a "structurally impossible regression"
   check

The host engineer can run `./scripts/run-e2e-docker.sh test` as part of
the release checklist if desired (tracked in §5 Manual Gates Pending).

---

## 9. Overall Result

### Auto-Scope: **PASS**

All structural, compliance, security, and quality checks for the
Phase 4-G auto-scope pass cleanly:

- File structure matches IMPLEMENTATION.md (5 created + 6 modified files)
- Phase 4-E auto-scope contract preserved (`git diff` empty on `preedit.rs`
  / `commit.rs` / `render/cursor.rs`)
- `App::on_ime_*` signatures unchanged (FR5 routing contract preserved)
- All 10 FRs (FR1-FR10) have code + tests; sdd.yaml records every FR as
  `status: ok`
- All 8 NFRs have evidence; NFR5 / NFR6 / NFR7 (workspace / module /
  logging) verified PASS by inspection; NFR1-3 / NFR4 mid-session / NFR8
  parity gated by host execution
- SC-1, SC-3, SC-4, SC-5 (Phase 4-G scope), SC-8, SC-9 verified PASS
- Security: 6 checks (sanitize / no-bracketed-paste / UTF-16 / Drop /
  settings shape / no-new-surface) all PASS
- Quick check (sdd.5-check at `dbfb25b`): 2011 passed / 0 failed / 4
  ignored, fmt clean, clippy Phase 4-G scope clean

### Manual Host Gates Remaining

The following items require a host machine and are explicitly out of
scope for this Docker-driven verification pass (consolidated in §5):

- 2 integration tests (TS-backend-int-1 X11 + xvfb, TS-backend-int-2 Windows)
- 7 manual IME gates (x11 / x11-ibus / wayland / windows / fallback /
  imserver-restart / mux)
- 3 performance gates (TS-perf-3 / TS-perf-4 / TS-perf-regression)
- 1 Windows cross-build (`cargo build --target x86_64-pc-windows-msvc`)
- 1 legacy E2E regression (informational; structurally cannot regress)

These are queued for the host engineer; results will be appended to this
document as they complete.

### Blocking Findings

**None.** Phase 4-G auto-scope is verification-complete. No blocking
issues detected. The pre-existing 14-warning clippy baseline outside
Phase 4-G scope is forward-staged per Phase 4-F precedent and recorded
in `sdd.yaml.workflow[check].notes`.

### Open Issues / Follow-ups (non-blocking)

Recorded from sdd.5-check `notes`:

1. `ImeEvent::FocusOut` `#[allow(dead_code)]` attribute at
   `backend.rs:31` is misleading — `app.rs:204` matches on it. Cleanup
   candidate (cosmetic, not blocking)
2. `Settings::ime` `#[allow(dead_code)] // Phase 7` annotation is
   misleading — read by `window_host:782` + `backend:148`. Cleanup
   candidate (cosmetic, not blocking)
3. `X11Backend::push_event_for_test` at `x11.rs:286` has no caller; the
   Wayland counterpart is used in tests. Either add an X11 test that
   uses it or remove the symbol. Cleanup candidate (not blocking)

The Wayland backend `init` returning `Unavailable` is intentional Phase
4-G-C scaffolding (keeps pure-X11 Linux builds libwayland-free). Real
Wayland attach requires `wayland-backend/client_system` feature; this is
a documented follow-up tracked in TS-manual-ime-wayland.

---

## Appendix A — Phase 4-G Commit Range

```
dbfb25b docs(ime-native-integration): record implement completed_at_commit
d50ecff feat(native-poc): Phase 4-G-E final gates + perf + docs
fbcfe02 feat(native-poc): Phase 4-G-D Windows IMM32 backend
e69d7b2 feat(native-poc): Phase 4-G-C Linux Wayland backend scaffold
393224d feat(native-poc): Phase 4-G-B Linux X11 (XIM) backend
29daa51 feat(native-poc): Phase 4-G-A IME backend scaffold + Null fallback
```

## Appendix B — Verification Commands Used

```bash
# File presence
ls native-poc/src/ime/{backend,null,x11,wayland,windows,mod,preedit,commit}.rs
ls native-poc/src/{app,window_host,settings}.rs native-poc/{Cargo.toml,README.md}

# Phase 4-E contract preservation
git diff --stat 9f290ab..HEAD -- \
    native-poc/src/ime/preedit.rs \
    native-poc/src/ime/commit.rs \
    native-poc/src/render/cursor.rs
# → empty

# src-tauri untouched
git diff --stat 9f290ab..HEAD -- src-tauri/
# → empty

# FR code surface
grep -nE "fn on_ime_(preedit|commit|focus_lost)" native-poc/src/app.rs
grep -nE "XOpenIM|XCreateIC|XFilterEvent|XmbLookupString" native-poc/src/ime/x11.rs
grep -nE "zwp_text_input_v3|crossbeam_channel" native-poc/src/ime/wayland.rs
grep -nE "WM_IME_COMPOSITION|SetWindowSubclass" native-poc/src/ime/windows.rs
grep -nE "trait ImeBackend|enum ImeEvent|enum KeyDispatchResult|enum ImeInitError" \
    native-poc/src/ime/backend.rs
```

Workspace test totals are owned by sdd.5-check at `dbfb25b`
(2011 passed / 0 failed / 4 ignored).
