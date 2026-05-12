# Verification Document: Native Terminal + WebView Viewer Hybrid PoC

## Overview
**Feature**: native-terminal-poc
**SPEC.md**: `doc/tasks/native-terminal-poc/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/native-terminal-poc/IMPLEMENTATION.md`

## Build Verification

- Command: `cargo build --manifest-path native-poc/Cargo.toml`
- Expected: exit code 0, no errors. `cargo build --release` also succeeds.
- Note: existing `src-tauri/` build on `main` must remain unaffected.

## Test Verification

- Command: `cargo test --manifest-path native-poc/Cargo.toml`
- Coverage target: minimum 60% across `parser/`, `grid/`, `selection.rs`, `pty/input.rs`; target 80%. UI and event-loop code are exempt.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Parser: cursor movement (CUU/CUD/CUF/CUB/CUP/CHA) | Cursor lands on documented coordinates, clamped to grid | Unit |
| TS-2 | Parser: erase (ED/EL) | Specified ranges cleared without affecting others | Unit |
| TS-3 | Parser: SGR mapping | Colors and attributes applied to subsequent cells | Unit |
| TS-4 | Parser: DECSTBM scroll region | LF/RI scroll only within the defined region | Unit |
| TS-5 | Parser: alt-screen DEC modes 1049/47/1047/1048 | Primary screen state preserved on enter, restored on exit | Unit |
| TS-6 | Parser: OSC 0/2 title capture | Title field updated; no crash on malformed OSC | Unit |
| TS-7 | Parser: emterm Markdown OSC payload decoded | ViewerRequest enqueued with the expected Markdown body | Unit / Integration |
| TS-8 | Grid: scrollback eviction | Oldest line dropped at capacity; default ~1000 lines | Unit |
| TS-9 | Key encoder: representative keys | Printable ASCII, Enter, Tab, Backspace, arrows, Esc produce the expected bytes | Unit |
| TS-10 | Selection: line-based resolve | Selection over a known grid yields the expected text | Unit |
| TS-11 | Settings: parse with missing / extra / malformed keys | Defaults applied; warnings logged | Unit |
| TS-12 | PTY round-trip: echo hello | "hello" appears in the grid after the shell runs `echo hello` | Integration |
| TS-13 | OSC dispatcher → ViewerRequest queue | An emitted OSC event results in a queued request observable from the main thread | Integration |
| TS-14 | Viewer spawn does not block PTY | While a viewer is spawning/closing, the active tab still receives PTY output | Manual |
| TS-15 | Tabs: open 3, switch, close | Each PTY is isolated; window closes after last tab close | Manual |
| TS-16 | Selection / copy / paste round trip | Copy from PoC → external app paste shows the same text; external copy → PoC paste reaches the shell | Manual |
| TS-17 | Bracketed paste mode honored | Multi-line paste under `set -o vi` etc. arrives as a single bracketed paste when the shell enabled the mode | Manual |
| TS-18 | fcitx5 IME preedit / candidate / commit | Preedit overlays at cursor, candidates appear, commit reaches the shell | Manual |
| TS-19 | OSC → Markdown viewer end-to-end | Printf-ing the emterm Markdown OSC opens a Wry viewer showing the content | Manual |
| TS-20 | Resize | Resizing the window updates rows/cols in the shell (`stty size`) | Manual |
| TS-21 | Shell exits | `exit` closes the tab cleanly; threads joined | Manual |
| TS-22 | Window close on last tab | Closing the last tab closes the window | Manual |
| TS-23 | Large bursty output | `cat` of a large file does not corrupt rendering | Manual |
| TS-24 | Settings.json applied | font / palette overrides visible at startup | Manual |
| TS-25 | 8h Claude Code session | No screen loss, no crash; RSS/GPU not monotonically growing | Manual / Long-run |
| TS-26 | Rapid viewer open/close stress | Repeatedly triggering the OSC keeps the main terminal responsive | Manual |
| TS-27 | Window minimized for extended period | PTY reader does not stall; output buffered and re-rendered correctly on restore | Manual |
| TS-28 | wgpu surface lost / device loss recovery | Surface re-created without crash; terminal continues to render | Manual |
| TS-29 | PTY EOF / abnormal child termination mid-stream | Tab closes gracefully; no thread leak; remaining tabs unaffected | Integration / Manual |

## Code Quality Verification

- Format: `cargo fmt --manifest-path native-poc/Cargo.toml -- --check`
- Static analysis: `cargo clippy --manifest-path native-poc/Cargo.toml -- -D warnings` (warnings tolerated but reviewed; deny on must-fix lints).

## File Structure Verification

### Files to Create
- `native-poc/Cargo.toml` - Independent Cargo project.
- `native-poc/Cargo.lock` - Pinned versions.
- `native-poc/README.md` - Build/run/limits.
- `native-poc/src/main.rs` - Entrypoint.
- `native-poc/src/app.rs` - Top-level state container.
- `native-poc/src/window_host.rs` - tao window + wgpu/egui glue.
- `native-poc/src/logging.rs` - env_logger init.
- `native-poc/src/tabs.rs` - Tab type.
- `native-poc/src/selection.rs` - Selection state + clipboard ops.
- `native-poc/src/settings.rs` - settings.json loader.
- `native-poc/src/pty/mod.rs` - PTY session.
- `native-poc/src/pty/input.rs` - Key encoder.
- `native-poc/src/parser/mod.rs` - Parser state machine.
- `native-poc/src/parser/c0.rs` - C0 handlers.
- `native-poc/src/parser/csi.rs` - CSI handlers.
- `native-poc/src/parser/osc.rs` - OSC handlers.
- `native-poc/src/grid/mod.rs` - Grid state.
- `native-poc/src/grid/scrollback.rs` - Ring buffer.
- `native-poc/src/grid/altscreen.rs` - Alt screen.
- `native-poc/src/render/mod.rs` - Grid → egui render.
- `native-poc/src/render/theme.rs` - Color / font resolution.
- `native-poc/src/ui/tab_bar.rs` - Tab bar widget.
- `native-poc/src/ui/keybinds.rs` - Keybinding map.
- `native-poc/src/viewer/mod.rs` - ViewerSpawner.
- `native-poc/src/viewer/markdown.rs` - Markdown viewer launcher.
- `native-poc/src/ime/linux_fcitx5.rs` - Conditional IME fallback.
- `doc/tasks/native-terminal-poc/VERIFICATION_RESULT.md` - Phase 8 output.

### Files to Modify
- `.gitignore` - Append `native-poc/target/`.

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All FR1–FR12 functional requirements work | Walk TS-1..TS-26 |
| SC-2 | All US1–US7 acceptance criteria pass | Walk per-US criteria during manual phase |
| SC-3 | 8h Claude Code session succeeds | TS-25 |
| SC-4 | Wry viewer spawns via OSC end-to-end | TS-19 |
| SC-5 | fcitx5 produces correct preedit/commit/candidates | TS-18 |
| SC-6 | `cargo build` sampling shows shorter times than current Tauri build | Build-time sampling section below |
| SC-7 | VERIFICATION.md checklist fully completed | Phase 8 walkthrough; VERIFICATION_RESULT.md authored |

### Functional Requirements Coverage
| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 — Native window | Phase 1 | TS during Phase 1 acceptance + manual launch |
| FR2 — PTY bridge | Phase 2 | TS-12, TS-9, TS-20, TS-21 |
| FR3 — Minimal ANSI parser | Phase 3 | TS-1..TS-6, TS-23 |
| FR4 — Grid rendering | Phase 4 | TS-23 + manual visual check |
| FR5 — Scrollback | Phase 3 / 4 | TS-8 + manual scroll |
| FR6 — Selection + copy | Phase 4 | TS-10, TS-16 |
| FR7 — Paste + bracketed paste | Phase 4 | TS-16, TS-17 |
| FR8 — Tabs | Phase 5 | TS-15, TS-22 |
| FR9 — OSC → viewer spawn | Phase 6 | TS-7, TS-13, TS-19 |
| FR10 — Wry Markdown viewer | Phase 6 | TS-19, TS-26 |
| FR11 — settings.json loader | Phase 7 | TS-11, TS-24 |
| FR12 — fcitx5 IME | Phase 7 | TS-18 |
| NFR1 — 8h stability | Phase 8 | TS-25 |
| NFR2 — Input latency by feel | Phase 8 | Manual feel comparison |
| NFR3 — Build-time sampling | Phase 8 | Build-time sampling section |
| NFR4 — Logging | Phases 1+ | `RUST_LOG=info` run; logs visible |
| NFR5 — Module separation | All | File structure check |
| NFR6 — Linux only | All | Build/run on Linux; no Windows/macOS target |

## E2E Testing

The existing WebdriverIO + tauri-driver suite targets the Tauri build and is **incompatible** with the PoC. It must remain green on `main`. PoC adds no new GUI-driven E2E specs.

- [ ] Existing E2E (`./scripts/run-e2e-docker.sh`) continues to pass on `main`.

## Manual Testing (E2E Not Possible)

- [ ] TS-14: Viewer spawn during active output keeps PTY flowing.
- [ ] TS-15: Three tabs open / switch / close.
- [ ] TS-16: Copy ↔ external app paste round-trip.
- [ ] TS-17: Bracketed paste activated by shell honored.
- [ ] TS-18: fcitx5 preedit / candidate / commit.
- [ ] TS-19: Markdown OSC end-to-end.
- [ ] TS-20: Resize updates `stty size`.
- [ ] TS-21: `exit` closes tab cleanly.
- [ ] TS-22: Closing last tab closes window.
- [ ] TS-23: Large bursty output renders without corruption.
- [ ] TS-24: settings.json overrides applied at startup.
- [ ] TS-25: 8h Claude Code session — record RSS at start, mid, end; record any screen loss or crash.
- [ ] TS-26: Rapid viewer open/close stress.
- [ ] TS-27: Minimize window for ≥ 30 minutes; restore and verify shell history and PTY responsiveness.
- [ ] TS-28: Trigger or observe wgpu surface recreation (e.g., GPU driver reload); confirm no crash.
- [ ] TS-29: Send `kill -9` to a tab's shell PID; confirm tab closes cleanly and others continue.

## Performance Verification

### Long-run stability (NFR1)
- 8h Claude Code session with periodic RSS and GPU memory snapshots; no monotonic growth, no screen loss, no crash.

### Input latency (NFR2)
- Subjective comparison vs. current Tauri build on the same machine. Record impression.

### Build-time sampling (NFR3)
- Each measured at least twice on the same dev machine, restarted between samples for cold timing.
  - `cargo build --manifest-path native-poc/Cargo.toml` (clean + incremental)
  - `cargo build --manifest-path src-tauri/Cargo.toml` (clean + incremental)
- Record wall-clock and `cargo build --timings` summary if available.

## Security Verification

- [ ] OSC payload size is bounded; malformed sequences logged and ignored without crash.
- [ ] Markdown rendered in Wry uses the existing sanitization pipeline from `src/markdown/`.
- [ ] PoC does not introduce new persistence of sensitive data.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Parser | TS-1..TS-7 | 7 | 0 | 0 |
| Grid / Scrollback / Selection | TS-8, TS-10 | 2 | 0 | 0 |
| Key encoder | TS-9 | 1 | 0 | 0 |
| Settings | TS-11, TS-24 | 1 | 0 | 1 |
| PTY round-trip / lifecycle | TS-12, TS-20, TS-21 | 1 | 0 | 2 |
| Viewer / OSC | TS-13, TS-14, TS-19, TS-26 | 1 | 0 | 3 |
| Tabs | TS-15, TS-22 | 0 | 0 | 2 |
| Clipboard / Paste | TS-16, TS-17 | 0 | 0 | 2 |
| IME | TS-18 | 0 | 0 | 1 |
| Rendering / output | TS-23 | 0 | 0 | 1 |
| Long-run | TS-25 | 0 | 0 | 1 |
| Resilience (minimize/device-loss/PTY-EOF) | TS-27, TS-28, TS-29 | 1 | 0 | 3 |
| **Total** | **29** | **14** | **0** | **15** |
