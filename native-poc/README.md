# eMterm native-poc

Linux-only Proof-of-Concept terminal that combines a `tao` + `wgpu` + `egui`
native main window with on-demand `wry` Markdown viewer windows. The goal is to
gather Go/No-Go evidence for restructuring eMterm out of the Tauri/WebView
shell.

Since Phase 6 of the `term-core-rust-crate` SDD, this crate is a member of the
repository-root Cargo workspace and depends on `term_core` (the pure-Rust ANSI
parser + terminal grid extracted from `wasm/src/`) via a `path` dependency.
The Phase 1 stand-in modules (`src/parser/`, `src/grid/`) have been removed.

## Build

```sh
cargo build -p emterm-native-poc           # workspace-aware
# or the legacy form:
cargo build --manifest-path native-poc/Cargo.toml
```

## Run

```sh
RUST_LOG=info cargo run -p emterm-native-poc
```

## Test

```sh
cargo test -p emterm-native-poc
```

## Format

```sh
cargo fmt --all
```

## Phase 3 feature matrix (`native-terminal-features` SDD)

| Feature | Status | Notes |
|---------|--------|-------|
| Dirty-row diff render (FR1) | ✅ Phase 2 | `App::dirty_rows_this_frame`, frame-level skip when empty, `EMTERM_FULL_REDRAW=1` to disable |
| SGR full reflection (FR9) | ✅ Phase 3 | bold / dim / italic / underline / reverse / hidden / strikethrough; double / curly underline + SGR 58 deferred until `term_core` expands |
| Cursor render (FR2) | ✅ Phase 3 | DECSCUSR / DECTCEM / OSC 12 honored by renderer; parser route via OSC 22 / DECSCUSR landed in Phase 6 |
| Ambiguous width (FR11) | ✅ Phase 3 | `Settings::ambiguous_width_mode` (`Narrow` / `Wide`) |
| Selection (FR3) | ✅ Phase 4 | character / word / line modes (500 ms click classifier); PRIMARY auto-copy + Ctrl+Shift+C |
| Paste (FR4) | ✅ Phase 4 | Ctrl+Shift+V (CLIPBOARD) + middle-click (PRIMARY); bracketed paste wrap per DECSET 2004; `\e[201~` sanitization |
| Scrollback (FR5) | ✅ Phase 4 | `Settings::scrollback_lines` (default 10000); `ScrollPosition` enum; alt-screen suppression |
| Inline image: Kitty (FR6) | ✅ Phase 5 | `ImageLayer` + wgpu textured-quad pipeline (Rgba8UnormSrgb, source-over); LRU + 320 MB quota |
| Inline image: SIXEL (FR7) | ✅ Phase 5 | Same pipeline; DCS path |
| OSC dispatch matrix (FR8) | ✅ Phase 6 | All action types: 0/1/2/4/7/8/9/10/11/12/22/52/104/110/111/112/133/100/101/255 |
| Resize / reflow / image follow (FR10) | ✅ Phase 4+5 | `ImageLayer::recompute_pixel_dims` on resize |
| OSC 9 notifications (FR12) | ✅ Phase 6 | `notify-rust 4.x`; in-process `(title, body)` dedupe within 1 s |
| OSC 52 clipboard (FR13) | ✅ Phase 6 | `Settings::{clipboard_read_osc52, clipboard_max_size_osc52}` policy gate (defaults: true / 10 MiB) |
| Long-run stability (FR14) | 🟡 Phase 7 manual | 12+ h Claude Code session on host (RSS / GPU samples at 4 h / 8 h / 12 h) |

### Build / test status (workspace gate)

- `cargo build --workspace`: PASS
- `cargo test --workspace`: 1801 passed / 0 failed (816 app_lib + 597 term_core + 182 term_images + 169 native-poc + 37 ancillary)
- `cargo fmt --all -- --check`: PASS
- `cargo clippy -p emterm-native-poc`: 19 dead-code warnings (forward-staged Theme/CursorStyle/renderer wiring); no errors

## Phase 4 feature matrix (`mux-tabs-windows-ime` SDD)

| Feature | Status | Notes |
|---------|--------|-------|
| `mux_ipc` protocol extraction (Phase 4-A) | ✅ | `crates/mux_ipc/` holds wire data types shared by `src-tauri` and `native-poc`; `src-tauri/src/mux/ipc/protocol.rs` becomes a 1-line shim. `codec.rs` / `connection.rs` (tokio_util server framing) intentionally stay in `src-tauri` |
| egui tab bar + central keybinds (Phase 4-B) | ✅ | `native-poc/src/ui/tab_bar.rs` + `keybinds.rs`; `Ctrl+Shift+T/W`, `Ctrl+Tab`, `Ctrl+Shift+Tab`, `Ctrl+1..9` routed via `AppAction` |
| Mux client + OSC 777 + prefix latch (Phase 4-C) | ✅ | Blocking `UnixStream` + sync 4-byte BE length framing (`mux/wire.rs`); OSC 777 socket / session_id validation; tmux-style prefix state machine with 3 s timeout + literal double-press; native PTY pause flag + 256 KiB ring buffer; `cfg(test)` mock daemon |
| egui status bar (Phase 4-D) | ✅ | `ui/status_bar.rs` top / bottom panel honoring `settings.statusbar.{enabled,position}`; idle 1 Hz repaint via `ctx.request_repaint_after` |
| IME preedit + commit (Phase 4-E) | ✅ auto / 🟡 manual | `ime/preedit.rs` + `commit.rs` + cursor-underline overlay; C0 (except `\t`,`\n`) and C1 sanitised. Linux fcitx5 parity + Windows MS-IME long-run deferred to host execution |
| Final gates (Phase 4-F) | ✅ auto / 🟡 manual | `cargo fmt --all --check`, `cargo build --workspace`, `cargo test --workspace`, `cargo clippy -p emterm-native-poc -p mux_ipc -- -D warnings` (style fixes applied; remaining 14 warnings are forward-staged dead code documented per commit). 12 h soak (`TS-manual-soak`) + TS-perf-1/2 host measurements deferred to host execution. `mux::perf_tests` scaffolds the harness with `#[ignore]` markers |

## Phase 4-G feature matrix (`ime-native-integration` SDD)

> **Redesign note (2026-05-14)**: the original Phase 4-G shipped a self-built
> XIM / zwp_text_input_v3 / IMM32 stack on tao 0.34. That stack did not work
> in practice because tao 0.34 does not expose XKB keycodes, so `XmbLookupString`
> always returned 0 chars. The redesign migrates the windowing layer to
> **winit 0.30.9**, which handles X11 / Wayland / Windows IME natively via
> `WindowEvent::Ime`, and replaces the self-built backends with a thin
> `WinitImeBridge`. Phase 4-E (`ime/preedit.rs`, `ime/commit.rs`,
> `App::on_ime_*`, `render/cursor.rs::draw_cursor_with_preedit`) is unchanged.

| Feature | Status | Notes |
|---------|--------|-------|
| Cleanup (Phase 4-G-1) | ✅ auto | Removed `ime/{x11,wayland,windows}.rs` (~1500 lines + tests). Dropped `x11-dl`, `wayland-client`, `wayland-protocols`, `windows` crates from `native-poc/Cargo.toml`. `build_platform_backend` reduced to `Err(Unavailable)` → `NullBackend` until 4-G-3. |
| winit migration (Phase 4-G-2) | ✅ auto / 🟡 manual | tao 0.34 → winit 0.30.9 (`default-features=false`, `features=["rwh_06","x11","wayland","wayland-dlopen"]`). `EventLoop::run` closure → `ApplicationHandler` trait. `WindowEvent::ReceivedImeText` removed (winit replaces it with `KeyEvent::text` + `WindowEvent::Ime`). `winit_key_to_bytes` honors `KeyEvent::text` so plain printable input still reaches the PTY. wgpu surface unchanged via raw-window-handle 0.6. Manual host-deferred gate: 素の打鍵が PTY に届く確認 (`EMTERM_NATIVE_IME=0`). |
| Common backbone (Phase 4-G-A — retained from previous Phase 4-G) | ✅ auto | `ime/backend.rs` (`ImeBackend` trait + `ImeEvent` + `KeyDispatchResult` + `ImeInitError` + `RawKeyEvent`) + `ime/null.rs` (`NullBackend` passthrough) + factory + `App` plumbing. Trait gains a default `on_winit_ime` no-op so NullBackend / MockBackend stay valid; `WinitImeBridge` overrides it. |
| winit IME bridge (Phase 4-G-3) | ✅ auto / 🟡 manual | `ime/winit_bridge.rs`: `WinitImeBridge` with Ghostty-derived state machine (`im_composing` flag + `set_ime_cursor_area` dedup). Translates `WindowEvent::Ime { Enabled, Preedit, Commit, Disabled }` into `ImeEvent`s pumped through `App::pump_ime`. Linux X11, Linux Wayland, and Windows IMM32 are all handled by winit's internal IME stack — no per-platform code in native-poc. Manual `TS-manual-ime-x11 / x11-ibus / wayland / windows / fallback / imserver-restart / mux` deferred to a host with fcitx5 / IBus / MS-IME / Google IME (winit path). |
| Manual + perf gates (Phase 4-G-4) | 🟡 manual | TS-perf-3 (preedit redraw < 30 ms), TS-perf-4 (commit → PtySession::write < 5 ms), TS-perf-regression (IME-OFF latency within Phase 4 baseline +10%, winit migration included). Toggle via `EMTERM_IME_PERF=1`. All host-deferred. |

### Phase 4-G env vars + settings

- `EMTERM_NATIVE_IME=0` — disables native IME integration unconditionally; the App falls back to `NullBackend` and winit's `WindowEvent::Ime` events become no-ops at the bridge boundary. Emits exactly one warn log on startup.
- `settings.ime.native_integration` (`bool`, default `true`) — same effect via `settings.json` (Phase 7 wires JSON parsing; Phase 4-G pins the struct shape).
- `EMTERM_IME_PERF=1` — emits warn-level latency micros for TS-perf-3 (`on_ime_preedit → needs_full_redraw`) and TS-perf-4 (`on_ime_commit → PtySession::write`) so release-host measurements can be collected without recompilation.

### Phase 4-E auto-scope contract

Phase 4-G must not modify `ime/preedit.rs`, `ime/commit.rs`, or `render/cursor.rs::draw_cursor_with_preedit`. Backends push `ImeEvent::Preedit` / `Commit` / `FocusOut` into the App's pump; the App routes them through the unchanged Phase 4-E layer. `git diff` for the three files is empty across every Phase 4-G commit.

## Known limits (Phase 3 follow-up)

- Linux only (Ubuntu 22.04 family). No macOS, no Windows.
- `Theme::apply_osc` mutates the OSC color cache; the renderer still reads `Theme::default()` for palette resolution. Wiring the per-tab live `Theme` into `render/mod.rs` is the next Phase 3 follow-up (no SDD change required).
- OSC 52 read/write buffers (`pending_clipboard_writes`/`pending_clipboard_reads`) are drained on the UI thread in `window_host` follow-up; policy gate is fully in place.
- `NotifyRustSink` requires a D-Bus session (unavailable inside Docker). `TestSink` covers automated coverage; the user smoke-tests on host.
- E2E suite (`e2e-tests/`) targets the legacy Tauri WebView build only (SC-6 rationale).
- 12+ h Claude Code session is the only remaining manual gate (Phase 7).

## Known limits (Phase 4 follow-up)

- Mux client is Unix-only (`#[cfg(unix)]` on `mux/client.rs`). Windows port will land alongside the Phase 4-E IME follow-up using named pipes (or skip mux entirely on Windows).
- TS-manual-soak (12 h Claude Code under mux) requires host execution — not runnable from Docker.
- TS-perf-1 (1 MiB snapshot apply) / TS-perf-2 (prefix → wire round trip) host measurements pending. The harness lives in `native-poc/src/mux/perf_tests.rs` behind `#[ignore]` so it does not gate CI yet.
- TS-manual-ime-linux (fcitx5 parity) / TS-manual-ime-windows (MS-IME preedit + commit) require host execution. The auto agent cannot drive a real IME session inside Docker.
