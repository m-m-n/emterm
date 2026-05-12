# Implementation Plan: Native Terminal + WebView Viewer Hybrid PoC

## Overview
Build a Linux-only Proof-of-Concept terminal (`native-poc/` binary) that combines a tao + wgpu + egui native main window with on-demand Wry Markdown viewer windows, then judge Go/No-Go on the full eMterm rewrite using a manual checklist.

## Objectives
- Stand up a single Rust binary that opens a tao window with an egui+wgpu surface and connects to a real PTY.
- Implement the minimum ANSI / CSI / OSC subset needed for Claude Code to run interactively.
- Demonstrate selection / copy / paste, tabs, and an OSC-triggered Wry Markdown viewer end-to-end.
- Run an 8-hour Claude Code session and a sampled `cargo build` comparison, recorded in the verification result.

## Prerequisites

### Development Environment
- Linux desktop (Ubuntu 22.04 family) with a working GPU stack (Vulkan or OpenGL).
- Rust toolchain (stable) — version aligned with `rust-toolchain` if present, otherwise current stable.
- `cargo`, `rustfmt`, `clippy`.
- `fcitx5` with a Japanese input method (e.g., mozc) for the IME acceptance check.
- The existing eMterm repository checked out on branch `refactor/native-terminal-hybrid`.

### Dependencies
- The current `src-tauri/` build must remain compilable on `main`; PoC work does not modify it.
- The existing `src/markdown/` assets (HTML/CSS/TS) are reused by the Wry viewer; they remain untouched.

## Architecture Overview

### Technology Stack
- **Language**: Rust (PoC binary).
- **Framework**: `tao` (windowing/event loop) + `wgpu` (GPU surface) + `egui` (immediate-mode UI) + `wry` (viewer WebView).
- **Key Libraries** (indicative, finalized during Phase 1):
  - `tao` — window and event loop.
  - `wgpu` — GPU surface.
  - `egui`, `egui-wgpu`, plus a tao integration glue crate — UI rendering.
  - `wry` — WebView windows for the Markdown viewer.
  - `portable-pty` — PTY abstraction.
  - `arboard` — system clipboard access.
  - `unicode-width` — cell width calculation.
  - `log` + `env_logger` — logging.
  - `serde` + `serde_json` — settings parsing.

### Design Approach
- Single-process, multi-window. The tao event loop owns the main window plus any Wry viewer windows spawned on demand.
- Threading mirrors the existing Tauri backend at a high level: tao event loop on the main thread, per-tab PTY reader/writer threads, an ANSI parser invoked from the reader path, and a renderer driven by egui's per-frame redraw.
- Grid state and scrollback are owned by the tab and mutated by the ANSI parser; egui reads a snapshot per frame.
- OSC extension payloads cross from the parser to the main thread via a queue and trigger Wry viewer spawn there.
- No code or assets are pulled from `wasm/src/`; the minimal ANSI parser is new.

### Component Interaction
```
                    ┌────────────────────────────┐
                    │     tao event loop          │ (main thread)
                    │                            │
                    │  ┌── Main Window ──────┐   │
                    │  │  egui + wgpu       │   │
                    │  │   TabBar / Grid    │   │
                    │  └────────────────────┘   │
                    │                            │
                    │  ┌── Wry Viewer(s) ────┐   │
                    │  │  Markdown HTML      │   │
                    │  └────────────────────┘   │
                    │                            │
                    │  OSC spawn queue ◀────┐    │
                    └────────────┬───────────┼───┘
                                 │           │
              ┌──────────────────▼──┐        │
              │ Per-Tab PTY Reader  │  emits OSC events
              │  thread             │
              └─────────────────────┘
              ┌─────────────────────┐
              │ Per-Tab PTY Writer  │  consumes input queue
              │  thread             │
              └─────────────────────┘
```

## Implementation Phases

### Phase 1: Project scaffolding and window host

**Goal**: A `native-poc/` Cargo binary that opens a single tao window with an egui+wgpu surface and renders a placeholder UI.

**Files to Create**:
- `native-poc/Cargo.toml` - Independent Cargo project (not a member of the existing src-tauri workspace).
- `native-poc/README.md` - Build, run, and known-limits notes.
- `native-poc/src/main.rs` - Binary entry; bootstraps logging, builds App, runs event loop.
- `native-poc/src/app.rs` - Top-level App state container (tabs, viewer registry, settings).
- `native-poc/src/window_host.rs` - tao window + wgpu surface lifecycle + egui integration.
- `native-poc/src/logging.rs` - `env_logger` initialization wrapper.

**Files to Modify**:
- `.gitignore` - Exclude `native-poc/target/`.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| App | Owns top-level state and lifetime | Process started | Holds an empty tab list and an empty viewer registry |
| WindowHost | Manages the tao main window and the wgpu surface used by egui; recreates the surface on device-loss / surface-lost events | Display server available | Main window is visible and rendering at the configured refresh rate; transient device loss is recovered without crashing |
| Logging | Initializes env_logger from RUST_LOG | Process started | Logs are routed to stderr with origin tagging |

**Processing Flow** (diagram-convertible):
1. Process start
   - Initialize logging
   - Construct App with empty state
2. Create main window
   - tao event loop builder produces the window
   - wgpu surface attaches to the window
3. Per frame
   - tao dispatches events to App
   - App requests egui to lay out a placeholder UI
   - wgpu paints the egui draw buffer

**Implementation Steps**:
1. **Scaffold Cargo project** - Create the directory and minimal `Cargo.toml` with the indicative dependency list; pin exact versions in `Cargo.lock`.
2. **Boot tao event loop** - Build a single window and exit on close.
3. **Attach wgpu surface** - Acquire an adapter/device suited for the chosen presentation mode; handle `SurfaceError::Lost` / `OutOfMemory` by recreating the surface on the next frame.
4. **Wire egui integration** - Use a published tao-egui-wgpu glue crate or a minimal in-tree glue if no crate fits.
5. **Render placeholder UI** - Draw an "eMterm PoC" label and an empty central region.
6. **Initialize logging** - Tag origin and respect `RUST_LOG`.

**Dependencies**: Requires nothing. Blocks every later phase.

**Testing Approach**:
- Unit: none beyond compile-time checks.
- Integration: `cargo run` opens a window and exits cleanly on close.
- E2E: not applicable.
- Manual: confirm window appears and survives focus/blur/resize.

**Acceptance Criteria**:
- [ ] `cargo build --manifest-path native-poc/Cargo.toml` succeeds.
- [ ] `cargo run --manifest-path native-poc/Cargo.toml` opens a window and exits cleanly when closed.

**Estimated Effort**: small.

---

### Phase 2: PTY bridge

**Goal**: Connect the active tab to a real PTY (`$SHELL` or `/bin/sh`), echo raw output into a scrollable text buffer, and route key input back to the PTY.

**Files to Create**:
- `native-poc/src/pty/mod.rs` - PTY session handle + reader/writer thread orchestration.
- `native-poc/src/pty/input.rs` - Key event encoding to PTY bytes (ASCII, control codes, basic CSI for arrows/function keys).
- `native-poc/src/tabs.rs` - Tab type owning one PTY plus its byte buffer.

**Files to Modify**:
- `native-poc/src/app.rs` - Replace empty tab list with a single tab created at startup.
- `native-poc/src/window_host.rs` - Forward key events to the active tab; redraw on PTY data arrival.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| PtySession | Owns the PTY pair and child process, plus reader/writer thread handles | Shell binary resolvable | PTY runs; reader thread emits raw bytes; writer thread drains a bounded input queue |
| InputEncoder | Maps egui/tao key events to PTY bytes | Tab has an active PTY | A byte slice is enqueued for the writer |
| Tab | Holds a PtySession and a raw byte sink for now | App constructed | Tab is interactive: typed bytes round-trip |

**Processing Flow**:
1. Tab creation
   - Resolve `$SHELL`, fall back to `/bin/sh`
   - Spawn PTY pair with current window size
   - Start reader thread; start writer thread
2. Reader thread
   - Block on PTY read
   - Push received bytes into the tab's buffer (still raw bytes)
   - Wake the main thread to request a redraw
3. Writer thread
   - Wait on input queue
   - Write bytes to the PTY
4. Resize event
   - App receives window resize
   - Recompute cell grid dimensions (placeholder formula in this phase)
   - Update PTY size
5. PTY EOF / abnormal child termination mid-stream
   - Reader thread observes a closed PTY or non-recoverable read error
   - Emit a TabExited event with the child's wait status (or "killed")
   - Main thread closes the tab; if it was the last tab, closes the window
   - Writer thread drains any pending input, logs an info entry, and exits
   - Other tabs are unaffected

**Implementation Steps**:
1. **Spawn PTY** - portable-pty pair + child shell, capture shell pid.
2. **Reader thread** - Push bytes into a thread-safe buffer; signal a redraw.
3. **Writer thread** - Drain a bounded queue; back-pressure logged at warn level if it fills.
4. **Key encoding** - Cover printable ASCII, Enter/Tab/Backspace, arrows, Esc; reject other modifiers in this phase.
5. **Resize plumbing** - Translate window size into rows/cols using a fixed placeholder cell size.
6. **Lifecycle teardown** - On window close, send SIGHUP to the child and join threads.
7. **EOF / abnormal termination** - Reader maps EOF and unrecoverable errors to a TabExited event surfaced to the main thread; writer exits after draining; remaining tabs untouched.

**Dependencies**: Requires Phase 1. Blocks Phase 3+.

**Testing Approach**:
- Unit: key encoding produces the correct byte for representative keys.
- Integration: spawn `/bin/sh -c 'echo hello'`, assert "hello" appears in the buffer.
- Manual: type `ls`, observe shell output; verify resize keeps the shell happy.

**Acceptance Criteria**:
- [ ] Typing and shell output round-trip in a single tab.
- [ ] `exit` terminates the PTY and closes the tab.
- [ ] Resize is reflected in the PTY (TIOCSWINSZ effect visible to shell).
- [ ] EOF / `kill -9` of the shell closes the tab gracefully without leaking threads.

**Estimated Effort**: medium.

---

### Phase 3: Minimal ANSI parser and grid

**Goal**: Replace the raw byte buffer with a structured grid driven by a new in-PoC parser covering the C0/CSI/OSC subset enumerated in SPEC.md.

**Files to Create**:
- `native-poc/src/parser/mod.rs` - Parser entry point + state machine driver.
- `native-poc/src/parser/c0.rs` - Handlers for BS, HT, LF, CR, BEL.
- `native-poc/src/parser/csi.rs` - Cursor (CUU/CUD/CUF/CUB/CUP/CHA), erase (ED/EL), SGR, DECSTBM, DEC modes (1049/47/1047/1048).
- `native-poc/src/parser/osc.rs` - OSC 0/2 + emterm extension dispatch hook (handler wired in Phase 6).
- `native-poc/src/grid/mod.rs` - Cell, Cursor, Grid public surface.
- `native-poc/src/grid/scrollback.rs` - Ring buffer storing detached lines.
- `native-poc/src/grid/altscreen.rs` - Alternate screen save/restore.

**Files to Modify**:
- `native-poc/src/tabs.rs` - Replace the raw byte buffer with the Grid; route PTY bytes through the parser.
- `native-poc/src/pty/mod.rs` - Reader thread emits parsed events instead of raw bytes (parser owned on the reader thread).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| AnsiParser | State machine that consumes bytes and emits grid mutations | Reader thread alive | Stream of bytes is converted to a stream of grid operations |
| Grid | Cell matrix + cursor + scroll region + alt-screen flag | Parser emits an operation | Cell content/cursor updated; dirty signal raised |
| Scrollback | Ring buffer for lines pushed off the top | Grid scrolls | Oldest line dropped past capacity, default ~1000 lines |
| AltScreen | Holds saved primary screen state | DECSET 1049 received | Active grid swaps; on DECRST primary state restored |

**Processing Flow**:
1. Reader thread
   - Receives a byte slice
   - Drives the parser state machine byte by byte
2. Parser emits an operation
   - Mutate Grid (cell write, cursor move, SGR state, erase, scroll, mode toggle)
   - For OSC payloads, push into the App's OSC queue (consumed in Phase 6)
3. Redraw signal
   - On any state change the reader thread requests a frame
4. Width handling
   - Each printable codepoint queried via `unicode-width`
   - Zero-width code points combine with the previous cell

**Implementation Steps**:
1. **State machine skeleton** - Ground, Escape, CSI Entry/Param/Intermediate, OSC String terminators including BEL and ST.
2. **C0 handlers** - BS/HT/LF/CR/BEL semantics.
3. **CSI cursor + erase** - Coordinate clamping to grid bounds; respect DECSTBM region.
4. **SGR** - Foreground / background / 8-color / 16-color / 256-color / true color; bold, dim, italic, underline, reverse, strike.
5. **Scroll and alt-screen** - DECSTBM + DEC modes 1049/47/1047/1048.
6. **OSC 0/2 + emterm-ext** - Title sink (stored only; no chrome update until later); emterm payload enqueued.
7. **Scrollback eviction** - Drop oldest lines past capacity.

**Dependencies**: Requires Phase 2. Blocks Phases 4, 6, 7.

**Testing Approach**:
- Unit: each handler verified on hand-crafted byte sequences with assertions on cursor and cell state.
- Integration: feed a small captured Claude Code transcript and assert grid invariants.
- Manual: run `vim`-style alt-screen toggles, watch state restore.

**Acceptance Criteria**:
- [ ] CUU/CUD/CUF/CUB/CUP/CHA move the cursor with clamping.
- [ ] ED/EL clear the documented ranges.
- [ ] SGR mapping covers the SPEC.md subset.
- [ ] DECSTBM scroll regions affect LF and RI.
- [ ] DECSET 1049 / 47 / 1047 / 1048 preserve and restore primary state.
- [ ] OSC 0/2 captures titles; emterm OSC payloads are queued for the main thread.

**Estimated Effort**: large.

---

### Phase 4: Grid rendering, selection, clipboard, paste

**Goal**: Render the Grid through egui on the wgpu surface, support mouse selection, system clipboard copy/paste, and bracketed paste mode.

**Files to Create**:
- `native-poc/src/render/mod.rs` - Per-frame Grid-to-egui draw routine.
- `native-poc/src/render/theme.rs` - Color and font resolution (defaults + later settings.json overrides).
- `native-poc/src/selection.rs` - Selection state, hit testing, and clipboard ops.

**Files to Modify**:
- `native-poc/src/window_host.rs` - Mouse drag handling, clipboard shortcuts, focus rules.
- `native-poc/src/pty/input.rs` - Honor bracketed paste mode flag when enabled by the shell.
- `native-poc/src/tabs.rs` - Expose a snapshot accessor that the renderer consumes per frame.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Renderer | Convert Grid + cursor + selection into egui draw calls | A Grid snapshot is available | The visible viewport reflects current Grid state |
| Theme | Resolve a color from SGR state and the active palette | Render call needs to color a cell | A concrete RGB is returned |
| Selection | Track anchor/extent; produce a text region for copy | Mouse drag occurs | Selection bounds and resolved text are accessible |
| ClipboardBridge | Read/write system clipboard | Copy or paste shortcut received | OS clipboard reflects requested operation |

**Processing Flow**:
1. Per frame
   - Borrow Grid snapshot
   - For each visible row, draw cells using Theme to color glyphs and backgrounds
   - Overlay selection highlight if active
2. Mouse drag
   - Compute cell under pointer; update Selection anchor/extent
3. Copy shortcut
   - Resolve Selection to a string; push into ClipboardBridge
4. Paste shortcut
   - Read ClipboardBridge text
   - If bracketed paste mode is enabled, wrap with the paste sentinels
   - Forward bytes to the writer queue

**Implementation Steps**:
1. **Per-frame grid render** - Iterate visible rows; draw cell glyph and background; advance by Unicode width.
2. **Theme palette** - Hardcoded default palette; settings.json wires in Phase 7.
3. **Selection state** - Anchor / extent / mode (line-based only, no rectangular).
4. **Copy via arboard** - Pull selection text; write to clipboard.
5. **Paste via arboard** - Read clipboard text; respect bracketed paste mode flag from the parser.
6. **Cursor blink** - Optional simple blink; off by default if it complicates timing.

**Dependencies**: Requires Phase 3. Blocks Phase 7 (settings.json integration) and verification.

**Testing Approach**:
- Unit: selection resolution given a known Grid produces the expected text.
- Integration: roundtrip copy text → external clipboard read.
- Manual: drag-select, copy, paste in shell; verify rendering legibility.

**Acceptance Criteria**:
- [ ] Visible viewport renders Grid correctly.
- [ ] Mouse drag selects, Ctrl+Shift+C copies, Ctrl+Shift+V pastes.
- [ ] Bracketed paste wrapping is applied when the shell enabled the mode.

**Estimated Effort**: medium.

---

### Phase 5: Tabs

**Goal**: Allow multiple PTYs as tabs, with a header bar, keybinds, and isolated state per tab.

**Files to Create**:
- `native-poc/src/ui/tab_bar.rs` - egui tab bar widget.
- `native-poc/src/ui/keybinds.rs` - Central keybinding map.

**Files to Modify**:
- `native-poc/src/app.rs` - Hold a Vec<Tab>, active index, focus rules.
- `native-poc/src/window_host.rs` - Route input to the active tab; dispatch keybindings before the tab.
- `native-poc/src/tabs.rs` - Add per-tab title hook (consumed from OSC 0/2).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| TabBar | Renders tab strip and emits switch/close intents | Tab vector exists | UI reflects current tabs and active index |
| Keybinds | Translate (modifier, key) to an Action enum | Key event received | App receives a structured Action |
| Action::OpenTab / CloseTab / SwitchTab | High-level intents | Keybinds emitted them | Tab vector mutated, focus updated |

**Processing Flow**:
1. Key event
   - Try matching against Keybinds first
   - If matched, dispatch the Action and consume the event
   - Otherwise forward to active tab
2. New tab Action
   - Spawn new PtySession
   - Append to tabs; switch focus
3. Close tab Action
   - Teardown PtySession; remove from tabs
   - If empty, close the window

**Implementation Steps**:
1. **Tab vector** - Owned by App; active index tracked.
2. **Tab bar widget** - Show titles (from OSC 0/2 or shell name fallback); click to switch.
3. **Keybinds** - Ctrl+Shift+T, Ctrl+Shift+W, Ctrl+Tab, Ctrl+Shift+Tab; configurable later.
4. **Focus and routing** - Window receives events; route by active index.
5. **Empty window policy** - Close window on last-tab close.

**Dependencies**: Requires Phase 4. Independent of Phase 6.

**Testing Approach**:
- Unit: keybind map resolves expected actions.
- Integration: spawning two PTYs round-trips input/output to each.
- Manual: open three tabs, switch via keys and clicks, close.

**Acceptance Criteria**:
- [ ] At least three tabs can be opened, switched, and closed.
- [ ] PTYs are isolated; input goes to the active tab only.
- [ ] Window closes when the last tab closes.

**Estimated Effort**: small-to-medium.

---

### Phase 6: OSC extension → Wry Markdown viewer

**Goal**: Recognize the emterm Markdown OSC sequence in the parser, spawn a Wry viewer window on the main thread, and reuse the existing Markdown HTML harness.

**Files to Create**:
- `native-poc/src/viewer/mod.rs` - ViewerSpawner: drains the OSC queue and creates Wry windows.
- `native-poc/src/viewer/markdown.rs` - Markdown viewer launcher; embeds the existing HTML harness.

**Files to Modify**:
- `native-poc/src/parser/osc.rs` - Decode the emterm OSC payload and push a structured event onto the OSC queue.
- `native-poc/src/app.rs` - Hold a registry of viewer windows; drain the OSC queue per frame.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| OscDispatcher | Decodes emterm OSC payload into a ViewerRequest | OSC payload received | Event queued for main thread |
| ViewerSpawner | Creates a Wry window with the Markdown harness | tao event loop running | New WebView window is visible |
| MarkdownViewer | Owns the WebView; injects payload via initialization script | Wry window created | Markdown is rendered using existing CSS/JS |
| ViewerRegistry | Tracks live viewers for cleanup and policy decisions | App initialized | Closed viewers are reclaimed |

**Processing Flow**:
1. Reader thread parses OSC
   - Validate the emterm header
   - Decode payload following the existing emterm CLI format (e.g., base64 → utf-8)
   - Enqueue a ViewerRequest with the decoded Markdown
2. Main thread drains queue
   - For each request, spawn a Wry window
   - Inject the payload through the initialization script of the harness
3. Window close
   - Wry callback removes the entry from ViewerRegistry
4. Main terminal unaffected
   - No blocking calls on the main thread during spawn beyond Wry's own setup

**Implementation Steps**:
1. **OSC decode** - Mirror the existing emterm Markdown CLI format; tolerate malformed payloads with a warn log.
2. **Main-thread queue drain** - Per frame, drain pending ViewerRequests.
3. **HTML harness embed** - Include the existing Markdown harness as bundled string assets (string-load assumption per FR10).
4. **Markdown injection** - Use a window-init script to hand the payload to the existing JS.
5. **Lifecycle** - On Wry window close, remove from registry; ensure tao-side accounting is consistent.
6. **Independence from terminal** - Verify that viewer spawn does not block PTY reads.

**Dependencies**: Requires Phase 3 (OSC events) and Phase 1 (event loop / window creation).

**Testing Approach**:
- Unit: OSC decoder accepts valid payloads and rejects malformed ones with a warning.
- Integration: simulate an OSC event in-process and assert a ViewerRequest is queued.
- Manual: `printf` the emterm Markdown OSC sequence into the PoC terminal and confirm the viewer opens.

**Acceptance Criteria**:
- [ ] OSC payload triggers a Wry viewer with the expected Markdown.
- [ ] Closing the viewer leaves the main terminal responsive.
- [ ] Rapid open/close stress does not crash the terminal.

**Estimated Effort**: medium.

---

### Phase 7: settings.json loader and Linux fcitx5 IME

**Goal**: Read the user's existing settings.json on startup and wire fcitx5 IME through egui's built-in support (raw-IME fallback only if necessary).

**Files to Create**:
- `native-poc/src/settings.rs` - settings.json loader and PoC subset model.
- `native-poc/src/ime/linux_fcitx5.rs` - IME glue, present only if egui's built-in IME proves insufficient.

**Files to Modify**:
- `native-poc/src/render/theme.rs` - Apply font/color overrides from settings.
- `native-poc/src/window_host.rs` - Wire IME hooks to the active tab (preedit display, commit dispatch).
- `native-poc/src/tabs.rs` - Reset preedit on PTY focus changes.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Settings | Best-effort parse of existing settings.json | File present at known path | A PoC-subset settings struct is available |
| Theme overrides | Apply font family / size / palette overrides | Settings loaded | Renderer uses overridden values when possible |
| IME glue | Receive preedit and commit events from the framework | Window has focus and IME is active | Preedit overlays the cursor; commit goes to the input queue |

**Processing Flow**:
1. Startup
   - Locate settings.json at the existing Tauri-build path
   - Parse with `serde_json` allowing missing fields
   - Warn for unsupported keys; ignore them
2. IME preedit
   - Receive preedit text from egui
   - Render overlay at the cursor position
3. IME commit
   - Receive commit string
   - Encode bytes and forward to the writer queue

**Implementation Steps**:
1. **Settings model** - PoC subset: font_family, font_size, color palette, scrollback_lines.
2. **File location** - Reuse the existing Tauri-build directory; warn if not found and fall back to defaults.
3. **Apply on init** - Override Theme and Grid defaults.
4. **IME path - built-in** - Hook egui IME events.
5. **IME fallback (conditional)** - If acceptance fails, add a minimal tao raw-IME bridge limited to the terminal grid widget.

**Dependencies**: Requires Phases 4 and 5. Blocks final verification.

**Testing Approach**:
- Unit: settings parsing handles missing, extra, and malformed keys.
- Integration: starting with a known settings.json applies the expected theme.
- Manual: type Japanese with fcitx5 + mozc; verify preedit, candidates, commit.

**Acceptance Criteria**:
- [ ] settings.json overrides font family/size and the palette where supported.
- [ ] fcitx5 preedit overlay tracks the cursor.
- [ ] fcitx5 commit string reaches the PTY.

**Estimated Effort**: medium.

---

### Phase 8: Verification and PoC measurements

**Goal**: Execute the verification checklist, run the 8-hour Claude Code session, and produce `VERIFICATION_RESULT.md` capturing the Go/No-Go decision.

**Files to Create**:
- `doc/tasks/native-terminal-poc/VERIFICATION_RESULT.md` - Manual results with notes.

**Files to Modify**:
- `doc/tasks/native-terminal-poc/sdd.yaml` - Updated by the SDD orchestrator on step completion.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Verification checklist | Manual walkthrough of VERIFICATION.md | Phases 1-7 acceptance criteria met | Each item is marked pass / fail / N/A with notes |
| 8h stability run | Real Claude Code session on PoC terminal | PoC is otherwise feature-complete | Memory and behavior observations recorded |
| Build time sampling | Compare `cargo build` between native-poc and src-tauri | Both projects buildable on the same machine | Sampled times recorded as evidence |

**Processing Flow**:
1. Pre-flight
   - Confirm Phases 1-7 acceptance criteria are checked.
2. Functional walkthrough
   - For each VERIFICATION.md scenario, record outcome.
3. Long-run
   - Launch Claude Code in PoC; use the machine for 8h or leave running.
   - Sample RSS and GPU memory at start, mid, end.
4. Build-time comparison
   - Clean + incremental build of `native-poc/` vs. `src-tauri/`; record at least two samples each.
5. Decision
   - Aggregate pass/fail; write Go/No-Go with rationale to VERIFICATION_RESULT.md.

**Implementation Steps**:
1. **Walk the checklist** - One pass through VERIFICATION.md.
2. **Run the 8h session** - Use Claude Code as the workload.
3. **Sample build times** - Record numbers; the goal is qualitative ranking, not a tight target.
4. **Record viewer stress** - Open/close Markdown viewer rapidly to detect leaks.
5. **Resilience checks** - Walk TS-27 (minimize for 30+ minutes), TS-28 (wgpu surface lost), TS-29 (PTY abnormal termination) and capture outcomes.
5. **Write VERIFICATION_RESULT.md** - Per-item result, overall Go/No-Go, follow-up items for Phase 2+ if Go.

**Dependencies**: Requires all earlier phases.

**Testing Approach**:
- Automated: re-run `cargo test` and `cargo fmt --check` immediately before starting.
- Manual: everything above.

**Acceptance Criteria**:
- [ ] VERIFICATION.md checklist completed.
- [ ] 8h session results recorded.
- [ ] Build-time sampling recorded.
- [ ] VERIFICATION_RESULT.md authored with Go/No-Go and notes.

**Estimated Effort**: medium (mostly wall-clock time).

---

## Complete File Structure

```
emterm/
├── native-poc/                           # NEW
│   ├── Cargo.toml
│   ├── Cargo.lock
│   ├── README.md
│   └── src/
│       ├── main.rs
│       ├── app.rs
│       ├── window_host.rs
│       ├── logging.rs
│       ├── tabs.rs
│       ├── selection.rs
│       ├── settings.rs
│       ├── pty/
│       │   ├── mod.rs
│       │   └── input.rs
│       ├── parser/
│       │   ├── mod.rs
│       │   ├── c0.rs
│       │   ├── csi.rs
│       │   └── osc.rs
│       ├── grid/
│       │   ├── mod.rs
│       │   ├── scrollback.rs
│       │   └── altscreen.rs
│       ├── render/
│       │   ├── mod.rs
│       │   └── theme.rs
│       ├── ui/
│       │   ├── tab_bar.rs
│       │   └── keybinds.rs
│       ├── viewer/
│       │   ├── mod.rs
│       │   └── markdown.rs
│       └── ime/
│           └── linux_fcitx5.rs            # only if egui IME proves insufficient
├── src/                                   # unchanged
├── src-tauri/                             # unchanged
├── wasm/                                  # unchanged
├── doc/tasks/native-terminal-poc/         # NEW (this SDD)
│   ├── 要件定義書.md
│   ├── SPEC.md
│   ├── IMPLEMENTATION.md
│   ├── VERIFICATION.md
│   ├── sdd.yaml
│   ├── tasks.yaml
│   └── VERIFICATION_RESULT.md             # written in Phase 8
└── .gitignore                             # +native-poc/target/
```

## Testing Strategy

- **Unit tests** (`cargo test`): focused on parser handlers, scrollback eviction, key encoding, OSC decoding, and selection resolution. Target meaningful coverage of pure logic; UI and PTY are covered by integration tests.
- **Integration tests**: in-process scenarios that drive the parser with captured byte streams and assert grid state. Where a real PTY is involved, gate behind a feature flag so CI can opt out.
- **E2E**: existing WebdriverIO suite remains for the Tauri build on `main`. PoC has no GUI-driven E2E; manual verification replaces it per the project's testing policy.
- **Manual**: 8h Claude Code session, fcitx5 IME, clipboard with external apps, viewer open/close, resize, build-time sampling.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| tao | latest stable compatible with wry | window/event loop |
| wgpu | latest stable | GPU surface |
| egui | latest stable | UI framework |
| egui-wgpu | latest stable matching egui | wgpu backend for egui |
| (egui + tao glue) | latest stable | tao integration; concrete crate selected in Phase 1 |
| wry | latest stable | WebView windows |
| portable-pty | latest stable | PTY abstraction |
| arboard | latest stable | system clipboard |
| unicode-width | latest stable | cell width |
| log | 0.4 series | logging facade |
| env_logger | latest stable | log backend |
| serde | latest stable | settings parsing |
| serde_json | latest stable | settings parsing |

Concrete versions are pinned during Phase 1; `Cargo.lock` is committed.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| egui IME is insufficient for fcitx5 | medium | high | Add tao raw-IME fallback limited to the terminal widget; if still NG, mark FR12 No-Go and document |
| tao + wry interaction defects | medium | high | Track the wry-with-tao integration recipe; if blocked, surface in VERIFICATION_RESULT.md as No-Go for the hybrid premise |
| wgpu / egui long-run GPU resource leak | medium | high | Snapshot RSS and GPU memory during the 8h run; if growth is monotonic, investigate egui texture atlas and viewer cleanup |
| ANSI parser gaps cause Claude Code malfunction | medium | medium | Iterate on the parser during the session; log unknown sequences; consider reusing wasm/ in Phase 2 of the full project if gaps are unbounded |
| Wry Markdown harness needs more than string-load | low-to-medium | medium | If string-load fails, switch to a temporary local URL serving the bundled assets |
| Build-time gain is marginal | medium | low | The PoC measures direction, not a threshold; surface honestly in VERIFICATION_RESULT.md |
| Clipboard or selection rendering glitches | medium | low | Keep selection line-based; lean on arboard semantics |

## Open Questions

- [ ] FR3 assumption may need extension if Claude Code uses sequences outside the baseline subset; recorded as warnings during PoC.
- [ ] FR10 assumes string-load; revisit during Phase 6 if it fails on the existing Markdown harness.
- [ ] FR12 assumes egui built-in IME; raw-IME fallback decision deferred to Phase 7.
- [ ] NFR3 acceptance is qualitative; "shorter than current" is judged from the recorded samples in VERIFICATION_RESULT.md.

## Success Metrics

- [ ] All Phase 1-7 acceptance criteria are met.
- [ ] VERIFICATION.md checklist is fully walked.
- [ ] 8h Claude Code session is recorded with memory observations.
- [ ] Build-time samples comparing native-poc to src-tauri are recorded.
- [ ] VERIFICATION_RESULT.md captures the Go/No-Go decision and outstanding items.
