# Feature: Native Terminal + WebView Viewer Hybrid PoC

## Overview

A Proof of Concept (PoC) for restructuring eMterm from a Tauri/WebView application into a hybrid architecture: a native (tao + wgpu + egui) terminal main window combined with on-demand Wry WebView windows for viewers (Markdown). The PoC produces evidence to decide Go/No-Go on the full multi-phase rewrite (1.5–2 months). Out of scope: inline images, mux, Windows IME, macOS, settings UI, and migration to `main`.

## Objectives

- Demonstrate that a `tao` + `wgpu` + `egui` window can host a usable terminal connected to a real PTY.
- Verify long-running stability over an 8-hour Claude Code session on Linux.
- Validate `tao` + `wry` coexistence by spawning an actual Markdown viewer window via OSC.
- Confirm Linux fcitx5 IME behaves acceptably.
- Provide a perceived-quality comparison of input latency and a sampled build-time comparison vs. the current Tauri build.

## User Stories

### US1: Use the PoC terminal as a daily-driver-like shell

As an eMterm developer, I want to launch the PoC terminal and run my normal shell workflow, so that I can perceive whether the native rendering path matches or beats the WebView build.

**Acceptance Criteria:**
- [ ] `cargo run` in `native-poc/` opens a single window.
- [ ] Default shell (`$SHELL` or `/bin/sh`) is spawned in a PTY.
- [ ] Typed input reaches the shell; shell output renders in the grid.
- [ ] `Ctrl+C` sends SIGINT; `exit` closes the tab.

### US2: Run Claude Code for 8 hours without disappearance or memory monotonic growth

As an eMterm developer, I want to run Claude Code in the PoC terminal for 8 hours, so that I can verify the long-term-stability fix that motivated the rewrite.

**Acceptance Criteria:**
- [ ] PoC terminal stays interactive after 8 hours.
- [ ] No screen disappearance or crash observed.
- [ ] RSS/GPU memory usage does not grow monotonically (sampled informally during the session).

### US3: Use multiple tabs concurrently

As an eMterm developer, I want to keep multiple PTYs in tabs, so that I can multi-task in the PoC.

**Acceptance Criteria:**
- [ ] `Ctrl+Shift+T` adds a tab with a fresh PTY.
- [ ] Tabs are visually labeled and switchable.
- [ ] `Ctrl+Shift+W` closes the active tab.

### US4: Copy/paste between the PoC terminal and other apps

As an eMterm developer, I want to copy selected text and paste from the system clipboard, so that the PoC is usable.

**Acceptance Criteria:**
- [ ] Mouse-drag selects text.
- [ ] `Ctrl+Shift+C` copies to system clipboard.
- [ ] `Ctrl+Shift+V` pastes from clipboard into the active PTY.

### US5: Trigger a Wry Markdown viewer from inside the PoC terminal

As an eMterm developer, I want the OSC sequence used by `emterm markdown` to spawn a Wry viewer window, so that hybrid coexistence is validated end-to-end.

**Acceptance Criteria:**
- [ ] An OSC payload received over the PTY is parsed and translated into a Wry window spawn request.
- [ ] The Wry window displays the Markdown content using the existing `src/markdown/` HTML assets (or a faithful subset).
- [ ] Closing the Wry window leaves the main terminal unaffected.

### US6: Use fcitx5 IME for Japanese input

As an eMterm developer, I want fcitx5 to drive preedit/commit/candidate display, so that Japanese input works in the PoC.

**Acceptance Criteria:**
- [ ] Preedit text appears at the cursor position.
- [ ] Candidate window appears.
- [ ] Confirmed text is delivered to the PTY.

### US7: Judge PoC outcome via a checklist

As an eMterm developer, I want a single VERIFICATION.md checklist to drive the Go/No-Go decision, so that the criteria are explicit.

**Acceptance Criteria:**
- [ ] Each acceptance criterion is checked manually.
- [ ] Failing items are recorded with notes for the next phase.

## Technical Requirements

### Functional Requirements

- **FR1 – Native window:** A single tao window hosts an egui+wgpu surface that renders the terminal grid.
- **FR2 – PTY bridge:** A `portable-pty`-based session spawns the user's shell, with separate read/write threads. PTY size follows window resize.
- **FR3 – Minimal ANSI parser:** A new, in-PoC parser handles C0 (BS/CR/LF/HT/BEL), CSI cursor (CUU/CUD/CUF/CUB/CUP/CHA), CSI erase (ED/EL), CSI SGR (colors/attributes), DECSTBM scroll regions, alt-screen DEC modes (1049/47/1047/1048), OSC 0/2 (titles), and the emterm OSC extension. Existing `wasm/src/` is NOT used.
- **FR4 – Grid rendering:** Cell-based rendering on egui+wgpu. Full-frame redraw is acceptable; differential rendering not required for PoC.
- **FR5 – Scrollback:** A line-based history buffer (default ~1000 lines, overridable from `settings.json`) supports wheel and key-driven scrolling.
- **FR6 – Selection & copy:** Mouse-drag selection (line-based, no rectangular selection) and `Ctrl+Shift+C` write to the system clipboard via a crate such as `arboard`.
- **FR7 – Paste:** `Ctrl+Shift+V` sends clipboard text to the PTY; bracketed paste mode wrapping is honored when negotiated.
- **FR8 – Tabs:** Multiple PTYs are tracked as tabs; `Ctrl+Shift+T` adds, `Ctrl+Shift+W` closes, click or `Ctrl+Tab`/`Ctrl+Shift+Tab` switches. No mux/split.
- **FR9 – OSC → Wry viewer spawn:** The emterm Markdown OSC sequence is detected and triggers a Wry window spawn in the same process; lifetimes are independent.
- **FR10 – Wry Markdown viewer:** Loads the existing `src/markdown/` HTML/CSS/TS assets (string-load or local URL) and displays the supplied Markdown content.
- **FR11 – settings.json loader:** On startup, read the user's existing `settings.json` (current Tauri-build location) and apply font/color settings where feasible; missing fields fall back to defaults; unsupported fields produce a warning log.
- **FR12 – Linux fcitx5 IME:** Preedit display, candidate window, and commit work via egui's IME integration (preferred) or tao raw-IME (fallback).

### Non-Functional Requirements

- **NFR1 – Long-run stability:** 8 hours of Claude Code usage without screen loss, crash, or monotonic memory growth.
- **NFR2 – Input latency:** Subjectively no worse than the existing WebView build (measured by feel; no scripted P50/P95).
- **NFR3 – Build time:** Sampled `cargo build` clean/incremental times must be shorter than the current Tauri project; no automated measurement script.
- **NFR4 – Logging:** `log` + `env_logger`; `RUST_LOG=info` shows lifecycle and parser warnings.
- **NFR5 – Code organization:** Modules separated by responsibility (window, PTY, parser, render, tabs, OSC→Wry, settings).
- **NFR6 – Platform:** Linux only (Ubuntu 22.04-family dev machine).

## Implementation Approach

### Architecture

**System Architecture:**
```
┌───────────────────────────────────────────────────────────────┐
│  PoC Process (native-poc binary)                              │
│                                                               │
│   tao event loop (main thread)                                │
│      │                                                        │
│      ├── Main window (egui+wgpu)                              │
│      │      ├── TabBar                                        │
│      │      └── Terminal grid                                 │
│      │                                                        │
│      └── Wry viewer windows (spawned on demand)               │
│             └── Markdown viewer (existing HTML assets)        │
│                                                               │
│   IO thread(s) per active PTY                                 │
│      ├── PTY read → ANSI parser → Grid mutation               │
│      └── PTY write ← Input queue                              │
└───────────────────────────────────────────────────────────────┘
```

**Component Diagram:**

- `App` — owns the tao event loop, tabs, viewer windows.
- `WindowHost` — egui+wgpu surface, frame draw, input dispatch.
- `Tab` — pairs a PTY session with a grid+parser state.
- `Pty` — `portable-pty` wrapper with reader/writer threads.
- `AnsiParser` — minimal state machine (C0/CSI/OSC) emitting grid ops.
- `Grid` — cells + scrollback ring buffer + cursor + alt screen.
- `Renderer` — converts grid into egui draw calls.
- `OscDispatcher` — recognizes emterm OSC extension and triggers viewer spawn.
- `ViewerSpawner` — wry-based Markdown viewer window factory.
- `Settings` — `settings.json` loader.

### Data Flow

```
Shell → PTY → reader thread → AnsiParser → Grid (cells/cursor/scrollback)
                                        ↘ OscDispatcher → ViewerSpawner → Wry window
User input → egui → key/clipboard events → Tab → PTY writer thread → Shell
Window resize → tao → PTY size update + Grid resize
```

### API Design

PoC is a single binary; no public HTTP API. The notable internal protocols are below.

#### OSC Extension (Markdown viewer launch)

**Request (from shell to terminal, embedded in PTY stream):**

```
ESC ] {NUM} ; {payload} BEL
```

Where:

- `{NUM}` matches the emterm Markdown OSC number used by the existing `emterm markdown` CLI.
- `{payload}` is the Markdown body (utf-8). Encoding details (e.g., base64) follow the existing emterm CLI behavior.

**Effect:**

- PoC parses the OSC sequence, base64-decodes/utf-8-decodes the payload per the existing format, and submits a viewer spawn request to the main thread.
- The main thread creates a Wry window with the existing Markdown HTML harness and pushes the content via an established initial message.

**Error/Edge:**

- If the payload is malformed, the parser logs a warning and ignores the sequence (no crash).

#### Settings Loader

**Behavior:**

- On startup, locate `settings.json` in the same path the current Tauri build uses.
- Parse with `serde_json` into a best-effort PoC subset.
- Unknown keys are ignored; missing keys fall back to defaults.

### Database Schema

Not applicable. PoC has no persistent storage of its own.

### Dependencies

**Internal Dependencies:**

- Existing emterm OSC extension specification (Markdown CLI format).
- Existing `src/markdown/` HTML/CSS/TS assets, loaded by Wry as-is or with minimal adaptation.

**External Dependencies (Rust crates, indicative):**

- `tao` — windowing + event loop.
- `wry` — WebView windows for the viewer.
- `wgpu` — GPU surface.
- `egui`, `egui-wgpu`, `egui-winit` (or `egui-tao` equivalent) — UI layer. Concrete glue crate to be decided during implementation.
- `portable-pty` — PTY abstraction.
- `arboard` — clipboard.
- `unicode-width` — cell width.
- `log` + `env_logger` — logging.
- `serde`, `serde_json` — settings parsing.

Concrete versions are picked during Phase 4 implementation; pin them via `Cargo.lock`.

### File Structure

```
native-poc/
├── Cargo.toml                # Independent Cargo project (not a workspace member of src-tauri/)
├── README.md                 # Build and run instructions, known limits
└── src/
    ├── main.rs               # Entrypoint, tao event loop bootstrap
    ├── app.rs                # Top-level App state
    ├── window_host.rs        # egui + wgpu surface integration
    ├── tabs.rs               # Tab vector and switching
    ├── pty/
    │   ├── mod.rs            # Pty handle + reader/writer threads
    │   └── input.rs          # Key event → bytes encoding
    ├── parser/
    │   ├── mod.rs            # AnsiParser state machine entrypoint
    │   ├── csi.rs            # CSI handlers (cursor/erase/SGR/DECSTBM/DEC modes)
    │   ├── osc.rs            # OSC 0/2 + emterm extension dispatch
    │   └── c0.rs             # C0 control characters
    ├── grid/
    │   ├── mod.rs            # Cell, Cursor, Grid
    │   ├── scrollback.rs     # Ring buffer
    │   └── altscreen.rs      # Alt screen state
    ├── render/
    │   ├── mod.rs            # Grid → egui draw
    │   └── theme.rs          # Color + font resolution
    ├── selection.rs          # Selection state + clipboard ops
    ├── viewer/
    │   ├── mod.rs            # ViewerSpawner
    │   └── markdown.rs       # Loads existing src/markdown/ HTML assets
    ├── settings.rs           # settings.json reader
    └── ime/
        └── linux_fcitx5.rs   # IME glue (only if egui's built-in is insufficient)
```

## Test Scenarios

### Unit Tests
- [ ] ANSI parser: CSI cursor (CUU/CUD/CUF/CUB/CUP/CHA) updates the cursor coordinates.
- [ ] ANSI parser: CSI erase (ED/EL) blanks the correct ranges.
- [ ] ANSI parser: SGR color and attribute mapping.
- [ ] ANSI parser: DECSTBM scroll region applies on LF/RI.
- [ ] ANSI parser: Alt-screen 1049/47/1047/1048 switching preserves and restores state.
- [ ] ANSI parser: OSC 0/2 sets a title hook.
- [ ] ANSI parser: emterm Markdown OSC payload is forwarded to the viewer dispatcher.
- [ ] Grid: scrollback ring buffer drops oldest lines past capacity.
- [ ] Settings: `settings.json` parses with missing/extra fields.

### Integration Tests
- [ ] Spawn a `bash` PTY, write `echo hello`, assert grid contains `hello` row.
- [ ] PTY write/read round trip with a known sequence.
- [ ] OSC dispatcher fires a viewer-spawn callback with the decoded payload.

(Where the test would require a real GUI/clipboard/GPU, prefer manual verification per the project policy.)

### E2E Tests
**Existing E2E tests:** `e2e-tests/` (WebdriverIO + tauri-driver). They drive the existing Tauri build and are NOT compatible with the native PoC.
**Run command:** `./scripts/run-e2e-docker.sh` (unchanged on `main`).
- [ ] Existing E2E suite continues to pass on `main` (no regressions caused by PoC work in this branch).
- [ ] PoC adds no new GUI-driven E2E specs; manual verification per the checklist replaces them.

### Edge Cases
- [ ] Shell exits abnormally → tab closes and resources are reclaimed.
- [ ] Window minimized for an extended period → does not stall PTY reading.
- [ ] Rapid open/close of viewer windows → main window stays responsive.
- [ ] Large bursty PTY output (e.g., `cat large_file`) → no rendering corruption.
- [ ] Empty/invalid `settings.json` → defaults are used without panic.
- [ ] Unknown OSC payload format → ignored with a warning log.

### Performance Tests
- [ ] Manual 8h Claude Code run with periodic RSS/GPU snapshots.
- [ ] Manual subjective comparison: type latency, scroll smoothness vs. current Tauri build.
- [ ] `cargo build` clean + incremental timing taken at least twice on the dev machine, vs. current `cargo build --manifest-path src-tauri/Cargo.toml`.

## Security Considerations

- **Authentication:** Not applicable (local desktop app).
- **Authorization:** Not applicable.
- **Input Validation:** OSC payloads are length-bounded; the parser must not allocate unbounded buffers from a malformed sequence.
- **Data Protection:** No new persistence; existing `settings.json` is read-only from the PoC.
- **XSS Prevention:** Markdown content rendered in Wry must be sanitized using the existing `src/markdown/` pipeline; the PoC does not weaken existing sanitization.
- **Injection prevention:** PTY input from the user is sent unaltered to the shell (standard terminal behavior); OSC payloads originating from the shell are treated as data, not code.
- **CSRF Protection:** Not applicable.

## Error Handling

### Error Codes

PoC uses log levels rather than user-facing codes.

| Code | Description | Severity | User Message |
|------|-------------|----------|--------------|
| LOG_PARSER_UNKNOWN | Encountered an unhandled control sequence | warn | (log only, ignore) |
| LOG_OSC_BAD_PAYLOAD | OSC payload could not be decoded | warn | (log only, ignore) |
| LOG_VIEWER_SPAWN_FAIL | Wry window creation failed | error | (log; tab continues) |
| LOG_PTY_DEAD | PTY exited unexpectedly | info | Tab closes |
| LOG_SETTINGS_PARTIAL | Some settings keys could not be applied | warn | (log only) |

### Error Flow

```
Anomaly → log via `log` crate → continue (best-effort) → never crash main thread
```

## Performance Optimization

### Performance Goals

- Input latency: indistinguishable from current Tauri build by feel.
- 8h memory growth: no monotonic upward trend.
- Build time: shorter than current Tauri build (informal sampling).

### Optimization Strategies

- Keep PTY read in a dedicated thread; never block the event loop on PTY.
- Single redraw per frame; coalesce dirty events.
- Avoid allocations on the per-byte path of the ANSI parser (reuse buffers).

### Caching Strategy

- No caching layer beyond egui's internal texture atlas (monitored as part of the 8h test).

## Success Criteria

- [ ] All FR1–FR12 functional requirements are demonstrably working.
- [ ] All US1–US7 acceptance criteria are checked off.
- [ ] 8h Claude Code session passes without screen loss, crash, or monotonic memory growth.
- [ ] Wry viewer window spawns via OSC end-to-end without affecting the main terminal.
- [ ] Linux fcitx5 produces correct preedit/commit/candidates.
- [ ] `cargo build` sampling shows shorter clean+incremental times than the current Tauri project.
- [ ] Manual VERIFICATION.md checklist is fully completed (or items with notes for follow-up).

## Open Questions

> **Note**: Unresolved requirements are tracked in `sdd.yaml` with `status: tbd`.
> Resolve before running `/em-sdd:sdd.2-create-plan`.

- [ ] FR3-detail: Exact subset of ANSI sequences required by Claude Code is incrementally discovered; the parser will add coverage as gaps surface during PoC use. (status: tbd)
- [ ] FR10-detail: URL-load vs. string-load strategy for `src/markdown/` assets in Wry. (status: tbd)
- [ ] NFR3-detail: Acceptance threshold for "shorter than current" build time is qualitative for the PoC. (status: tbd)
- [ ] FR12-detail: Whether egui's built-in IME suffices for fcitx5 or a tao raw-IME glue is needed. (status: tbd)

## Implementation Phases (if applicable)

This SPEC covers Phase 1 only. Subsequent phases live in separate SDDs.

### Phase 1: PoC (this SPEC)

**Goals:** Build judgment material for the full rewrite.

**Deliverables:**
- `native-poc/` Rust binary running on Linux.
- `VERIFICATION.md` checklist with per-criterion results.
- `VERIFICATION_RESULT.md` capturing Go/No-Go and notes.

## References

- `tmp/restruct.md` — Restructuring strategy document, the source of this PoC.
- `CLAUDE.md` — Project-wide guidance.
- `src-tauri/src/pty/` — Reference for the current PTY thread layout (not reused).
- `src/markdown/` — Existing Markdown viewer assets reused by the Wry viewer.
- `e2e-tests/` — Current Tauri-bound E2E suite (kept untouched on `main`).
