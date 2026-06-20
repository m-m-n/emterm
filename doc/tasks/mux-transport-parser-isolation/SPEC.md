# Feature: mux Transport/Content Parser Isolation

## Overview

Inside a mux session, `emterm image <file>` fails to render the inline image and leaks
base64-like text to the screen. The root cause is that a mux tab multiplexes a single
`term_core` parser (`Tab::core`) across two independent streams — the outer transport
stream (PTS bytes carrying `emterm-mux;` APC frames) and the inner content stream
(remote pane raw terminal output) — so parser state is corrupted at stream boundaries.
This feature isolates the two parses so `Tab::core` becomes inner-content-only.

## Objectives

- Render inline images (Kitty / SIXEL) correctly inside a mux session, with no base64 leak.
- Fully separate outer-transport parsing from inner-content parsing of parser state.
- Purify `Tab::core` into an inner-content-only parser, eliminating outer-transport
  parser pollution.

## User Stories

### US1: Display an image inside mux
As a mux user, I want to run `emterm image <file>` inside a mux tab and see the inline
image, so that rich content works the same as in a non-mux tab.

**Acceptance Criteria:**
- [ ] The inline image renders with no base64 text leak.
- [ ] A large image (several MB) whose Kitty chunks span PtyOutput boundaries is assembled correctly.

### US2: Keep normal mux content working
As a mux user, I want plain text, TUIs (e.g. vim), and the Markdown viewer to keep
working inside mux, so that isolating the outer transport has no side effects.

**Acceptance Criteria:**
- [ ] Plain text, TUI, and the Markdown viewer behave as before inside mux.
- [ ] Non-mux tabs are unaffected (no regression).

## Technical Requirements

### Functional Requirements

- **FR1 - Dedicated transport extractor:** After mux is established, route the tab's
  coalesced PTS bytes through a dedicated APC/OSC extractor with independent parser state
  (not `self.core`) and emit the `emterm-mux;` frames into the existing
  `partition_apc_for_mux` → `apply_mux_message` path. The extractor keeps only
  `ApcDispatch` / `OscDispatch` payloads and discards Print etc.
- **FR2 - Inner-content-only `self.core`:** After mux is established, `self.core` is driven
  ONLY by the inner content stream (the `apply_mux_message` PtyOutput arm,
  `tabs.rs:945`), so an inner Kitty chunk that spans PtyOutput boundaries keeps its
  parser state.
- **FR3 - APC and OSC fallback:** The extractor handles both the APC transport
  (`ESC _ emterm-mux; ... ESC \`) and the OSC 9999 fallback (`ESC ] 9999 ; emterm-mux; ...`),
  matching the bridge transport detection in `mux/bridge.rs`. It normalizes an OSC 9999
  `emterm-mux;` frame into the same APC payload form the APC transport produces — mirroring
  `term_core::handle_osc_internal` (`osc_handler.rs:51`), which today routes OSC 9999
  `emterm-mux;` to the APC callback so both transports reach `pending_apc`. This preserves
  parity with the current behavior (no Windows-ConPTY regression).
- **FR4 - Pre-mux routing unchanged:** Before mux is established (normal tab / before the
  Welcome handshake), PTS bytes are processed by `self.core` as today. The Welcome APC
  itself is parsed once by `self.core` (pre-mux); the switch to the extractor happens on
  the first pump AFTER `mux_session_name` is set.
- **FR5 - Detach restores `self.core` routing:** On detach (`mux_session_name` cleared,
  `tabs.rs:1364`), PTS processing returns to `self.core`. The routing decision must hold
  across the detach boundary WITHIN a single coalesced pump buffer: when the same pump's
  PTS buffer is `[... Detached frame][post-detach shell bytes]`, the bytes AFTER the
  `Detached` frame are routed to `self.core` in that same pump, not silently discarded by
  the extractor. (The known failure: the extractor consumes the whole coalesced buffer and
  discards non-APC bytes before `apply_mux_message` clears `mux_session_name`, so a shell
  prompt printed right after detach is dropped and the grid stays blank until the next key.)
- **FR6 - Welcome duplication tolerance:** The switch to the extractor must remain correct
  under the known double-Welcome delivery (guarded today by `first_welcome`).
- **FR7 - Remove DIAG diagnostics:** Remove the temporary investigation logs added during
  root-cause analysis (see "Diagnostics removal" below).

### Non-Functional Requirements

- **NFR1 - No regression (non-mux):** The non-mux path is unchanged.
- **NFR2 - Protocol stability:** mux daemon, mux_ipc protocol, and the bridge are unchanged.
- **NFR3 - Out of scope (WebView):** The WebView side (`src/`) is out of scope
  (slated for removal; one-directional merge policy).
- **NFR4 - Performance:** The existing `pump` coalescing / frame budget
  (`FRAME_BUDGET_MS = 12ms`, `COALESCE_CAP = 1MB`) behavior is preserved; the extractor
  adds minimal overhead.
- **NFR5 - `term_core` holds no mux application-protocol constants:** `term_core` (a
  low-level terminal-emulator crate) must not embed the mux inband-protocol constants
  (OSC param `9999`, frame prefix `emterm-mux;`):
  - `MuxApcExtractor` takes the OSC param and frame prefix as constructor parameters; the
    caller (`tabs.rs`) passes `mux_ipc::protocol::{MUX_OSC_PARAM, APC_PREFIX}` (the
    cross-crate SSOT), so the extractor carries no copy of the values.
  - OSC 9999 `emterm-mux;` recognition is moved OUT of `term_core::handle_osc_internal`
    (`osc_handler.rs`) to the application layer (the OSC callback in
    `src-tauri/src/callbacks.rs`). A pre-mux OSC 9999 `emterm-mux;` Welcome (Windows
    ConPTY transport, which strips APC but passes OSC) still reaches the mux APC path,
    preserving the handshake (FR4 / NFR1) without `term_core` knowing the mux protocol.
  - The duplicated `MUX_OSC_PARAM` / `MUX_PREFIX` constants in `term_core` and their
    `drift_*` tests are removed (no in-crate copy left to drift).

## Implementation Approach

### Architecture

Two-layer parse is intrinsic to the mux inband protocol (NFR2 in mux-inband-protocol —
SSH transparency). The bridge (`emterm mux attach`) holds a local Unix socket to the
daemon and converts each `MuxMessage` into an `emterm-mux;<base64>` APC escape on the PTY;
the GUI reads the PTY as if the bridge were a plain shell.

```
                 PTS bytes (one byte stream)
                          |
          ┌───────────────┴────────────────┐
          │  mux_session_name.is_some() ?   │
          └───────────────┬────────────────┘
            no (pre-mux)   │   yes (mux established)
          ┌────────────────┘                └────────────────┐
          ▼                                                   ▼
  self.core.process_pty_data_fully            MuxApcExtractor (independent parser state)
  (outer + Welcome, as today)                  emits emterm-mux; APC / OSC 9999 frames only
                                                          │
                                                          ▼
                                          partition_apc_for_mux → apply_mux_message
                                                          │ (PtyOutput arm)
                                                          ▼
                                  self.core.process_pty_data_fully(inner content)
                                  (INNER STREAM ONLY — Kitty/SIXEL assembled here)
```

After the switch, `self.core` is driven exclusively by the inner content stream, so a
Kitty chunk split across PtyOutput boundaries no longer has its parser state clobbered by
an interleaving outer parse.

### Data Flow

```
Before mux:  PTS → self.core (parser) → cb_state.pending_apc → partition_apc_for_mux
After mux:   PTS → MuxApcExtractor (parser') → mux APC payloads → partition_apc_for_mux
                                                                   → apply_mux_message
                                                                   → (PtyOutput) self.core
```

### Component Diagram

- `Tab::pump` (`tabs.rs:1417`): currently runs `self.core.process_pty_data_fully(&combined)`
  at `tabs.rs:1457` on the coalesced PTS buffer. Gate this on `mux_session_name`: when set,
  feed `combined` to the extractor instead and skip the core-side grapheme flush /
  device-response / mark-drain that only apply to the outer stream.
- `MuxApcExtractor` (new, transport extraction): independent APC/OSC scanner. May reuse a
  standalone `term_core::Parser` instance, or be a small purpose-built scanner. If
  `term_core` exposes no public standalone `Parser` API, add the minimal public surface
  (e.g. a `MuxApcExtractor` that returns only APC/OSC payloads).
- `apply_mux_message` PtyOutput arm (`tabs.rs:935-975`): unchanged inner parse into
  `self.core`; the existing device-response / `drain_marks` / `backfill_marks` drains keep
  driving the real render.

### Dependencies

**Internal Dependencies:**
- `crates/term_core` (`Parser`, `process_pty_data`, `process_pty_data_fully`): outer/inner
  parse engine; may gain a standalone extractor entry point.
- `src-tauri/src/mux/apc.rs` (`try_decode_emterm_mux`, `partition_apc_for_mux`): mux APC
  decode path the extractor output feeds into.
- `src-tauri/src/mux/bridge.rs`: transport detection (APC vs OSC 9999) the extractor mirrors.

**External Dependencies:**
- None new.

### File Structure

```
crates/term_core/src/
  terminal_core.rs        # remove parser_mid_sequence() diagnostic accessor;
                          # possibly add minimal standalone extractor entry point
  terminal_dispatch.rs    # process_pty_data (reference)
  mux_apc_extractor.rs    # NFR5: new(osc_param, prefix) constructor injection;
                          # remove MUX_OSC_PARAM/MUX_PREFIX consts + drift_* tests
  osc_handler.rs          # NFR5: remove OSC 9999 emterm-mux special-casing
                          # (recognition moves to the app-layer OSC callback)
src-tauri/src/
  tabs.rs                 # pump(): gate outer parse on mux_session_name → extractor;
                          # FR5: re-route post-Detached tail in the same pump to self.core;
                          # construct MuxApcExtractor with mux_ipc::protocol constants;
                          # apply_mux_message PtyOutput arm: remove DIAG logs;
                          # drain_and_decode_images / reset_frame_for_replay: remove DIAG logs
  callbacks.rs            # NFR5: OSC callback recognizes OSC 9999 emterm-mux and routes it
                          # to the mux APC path (moved out of term_core)
  mux/apc.rs              # try_decode_emterm_mux: restore original simple warn (remove DIAG)
  mux/                    # MuxApcExtractor home (new module or within an existing mux file)
```

## Test Scenarios

### Unit Tests
- [ ] `MuxApcExtractor` extracts a complete `emterm-mux;` APC frame from a single buffer.
- [ ] `MuxApcExtractor` reassembles an `emterm-mux;` APC frame split across multiple buffers
      (no corruption / no leak).
- [ ] `MuxApcExtractor` handles the OSC 9999 fallback transport.
- [ ] Pre-mux PTS bytes still route through `self.core` (extractor not engaged before Welcome).
- [ ] Detach (`mux_session_name` cleared) restores `self.core` routing.
- [ ] (NFR5) `MuxApcExtractor` constructed with an injected OSC param + prefix extracts
      frames using the injected values, and discards an OSC frame whose param differs from
      the injected one (proves the values are not hard-coded in `term_core`).

### Integration Tests
- [ ] A Kitty image sequence delivered as inner content split across multiple mux PtyOutput
      messages — with interleaving outer-transport pumps — is assembled into a single
      decodable image (the core regression test for this fix). Run with `--test-threads=1`
      for the `tabs.rs` replay tests.
- [ ] Non-mux Kitty image still decodes (no regression).
- [ ] (FR5) `process_combined` fed one coalesced buffer
      `[inner PtyOutput frame][Detached frame][plain shell prompt bytes]` renders the plain
      prompt bytes via `self.core` (they are NOT discarded by the extractor across the
      detach transition).
- [ ] (NFR5) A pre-mux OSC 9999 `emterm-mux;` Welcome frame parsed by `self.core` still
      reaches the mux APC path via the application-layer OSC callback (Windows ConPTY
      handshake parity), now that `term_core::handle_osc_internal` no longer special-cases
      it.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] (Manual) See "Manual Verification" below.

### Edge Cases
- [ ] Double-Welcome delivery: the switch to the extractor stays correct (no double replay
      corruption) under the `first_welcome` guard.
- [ ] Kitty chunk boundary lands mid-APC introducer (`ESC _`) across PtyOutput messages.
- [ ] Large image (several MB) spanning many PtyOutput boundaries.
- [ ] `Detached` frame arriving mid-coalesced-buffer immediately followed by post-detach
      shell output (FR5 — tail must reach `self.core`, not be dropped).

### Manual Verification
1. In a mux tab, run `emterm image <file>` → inline image renders, no base64 leak.
2. A large image (several MB) assembles correctly across chunk boundaries.
3. Image decode succeeds (no `Kitty image decode failed` / `mux APC decode failed` in
   `emterm.log`).
4. Non-mux tabs still render images as before (no regression).
5. SIXEL (`emterm image --protocol sixel`) renders the same way.
6. Markdown viewer, plain text, and TUIs (e.g. vim) behave as before inside mux (no
   side effects from the outer isolation).
7. Tests: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
   (the `tabs.rs` replay tests need `--test-threads=1`).

## Diagnostics Removal

Remove the temporary investigation logs added during root-cause analysis (FR7):

- `crates/term_core/src/terminal_core.rs`: `parser_mid_sequence()` accessor.
- `src-tauri/src/tabs.rs`: `apply_mux_message` PtyOutput arm `DIAG mux PtyOutput ...`.
- `src-tauri/src/tabs.rs`: `drain_and_decode_images` `DIAG drain ...`.
- `src-tauri/src/tabs.rs`: `reset_frame_for_replay` `DIAG reset_frame_for_replay ...`.
- `src-tauri/src/mux/apc.rs`: `try_decode_emterm_mux` failure-path `DIAG mux APC decode failed ...`
  (restore the original simple warn).

## Error Handling

- A malformed outer `emterm-mux;` frame must not corrupt subsequent frames (extractor state
  is independent of `self.core`). On decode failure, log at the original simple `warn` level
  in `apc.rs` (DIAG variant removed).
- Inner-content parse failures remain handled by the existing image pipeline
  (`drain_and_decode_images`).

## Success Criteria

- [ ] All functional requirements (FR1–FR7) are implemented (FR5 covers the mid-coalesced-
      buffer detach transition: post-detach shell bytes reach `self.core`).
- [ ] mux inline images (Kitty + SIXEL) render with no base64 leak.
- [ ] The split-chunk integration test passes.
- [ ] Non-mux path and normal mux content (text / TUI / Markdown) show no regression.
- [ ] DIAG diagnostics are removed; `apc.rs` failure log restored to the simple warn.
- [ ] (NFR5) `term_core` embeds no mux protocol constants: `MuxApcExtractor` takes them by
      constructor injection, OSC 9999 `emterm-mux;` recognition lives in the app layer, and
      the `term_core` `MUX_OSC_PARAM`/`MUX_PREFIX` constants + `drift_*` tests are removed.
- [ ] `cargo test --lib` passes (`tabs.rs` replay with `--test-threads=1`).
- [ ] CLI-only build still compiles (`--no-default-features`).

## References

- Design document: `tmp/mux-kitty-image-leak-fix-design.md`
- Requirements: `doc/tasks/mux-transport-parser-isolation/要件定義書.md`
- Related: `doc/tasks/mux-inband-protocol/`, `doc/tasks/kitty-protocol-compat/`,
  `doc/tasks/image-display/`
