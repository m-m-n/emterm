# Feature: Synchronized Output (DEC Private Mode 2026)

## Overview

Implement DEC Private Mode 2026 (Synchronized Output) to eliminate visual flicker when TUI applications redraw the screen. When enabled (`CSI ?2026h`), terminal output is buffered without rendering; when disabled (`CSI ?2026l`), all accumulated changes are rendered in a single frame. Also implement DECRPM response for mode 2026 so applications can detect support.

## Objectives

- Support `CSI ?2026h` (begin synchronized update) and `CSI ?2026l` (end synchronized update)
- Buffer rendering while synchronized mode is active, flush on deactivation
- Respond to DECRPM queries (`CSI ? 2026 $ p`) to advertise support
- Maintain compatibility with existing frame budget and cursor visibility interrupt logic

## User Stories

### US1: Flicker-free TUI Rendering
As a user running TUI applications (vim, neovim, htop, etc.), I want the terminal to support Synchronized Output so that screen redraws appear as a single atomic update without visible tearing or flicker.

**Acceptance Criteria:**
- [ ] `CSI ?2026h` suppresses rendering until `CSI ?2026l` is received
- [ ] All dirty rows accumulated during suppression are rendered in one frame on `CSI ?2026l`
- [ ] Applications that do not use mode 2026 are unaffected

### US2: Feature Detection
As a TUI application developer, I want to query whether the terminal supports Synchronized Output via DECRPM so that I can enable it only when supported.

**Acceptance Criteria:**
- [ ] `CSI ? 2026 $ p` returns `CSI ? 2026 ; 1 $ y` (set) or `CSI ? 2026 ; 2 $ y` (reset) depending on current state

## Technical Requirements

### Functional Requirements

- **FR1: Mode 2026 Flag in WASM** - Add `MODE_SYNCHRONIZED_OUTPUT` bit constant to the WASM mode bitfield. `handle_set_mode(2026, true/false)` sets/clears it and returns `MODE_ACTION_NONE`.
- **FR2: Render Suppression in WASM** - While `MODE_SYNCHRONIZED_OUTPUT` is set, dirty rows continue to accumulate normally in the WASM dirty bitfield (no clearing between chunks). WASM does not need to change dirty tracking behavior since TS already reads dirty rows only at render time.
- **FR3: Render Suppression in TS** - In `pty-handler.ts`, after `process_pty_data()` and `syncModesFromWasm()`, check the synchronized output mode flag. If set, skip `renderImmediate()`. When the flag transitions from set to cleared, call `renderImmediate()` to flush all accumulated dirty rows.
- **FR4: DECRPM Response for Mode 2026** - Handle `CSI ? Ps $ p` (DECRPM request) in `csi_dispatch.rs`. For mode 2026, respond with `CSI ? 2026 ; Ps $ y` where Ps=1 if currently set, Ps=2 if currently reset. For unknown modes, respond with Ps=0 (not recognized).
- **FR5: Mode Reset on Buffer Switch** - When switching to/from alternate screen buffer (modes 47/1047/1049), synchronized output mode is implicitly reset to prevent orphaned suppression states.

### Non-Functional Requirements

- **NFR1 - Performance:** No measurable overhead when mode 2026 is not active. The check is a single bit test per PTY data chunk.
- **NFR2 - Compatibility:** Frame budget deadline logic in pty-handler.ts continues to work normally. When frame budget expires during synchronized mode, leftover data is deferred to next frame as usual; rendering remains suppressed until `?2026l` arrives.

## Implementation Approach

### Architecture

```
PTY data → WASM process_pty_data()
         → sets MODE_SYNCHRONIZED_OUTPUT on ?2026h
         → dirty rows accumulate normally
         → TS reads mode flag after syncModesFromWasm()
         → if synchronized: skip renderImmediate()
         → on ?2026l: MODE_SYNCHRONIZED_OUTPUT cleared
         → TS detects transition → renderImmediate() flushes all dirty rows
```

### Data Flow

```
?2026h received:
  WASM: set MODE_SYNCHRONIZED_OUTPUT bit
  TS: syncModesFromWasm() → detect flag → skip render

Normal output while synchronized:
  WASM: process data, mark dirty rows (normal behavior)
  TS: skip render (flag still set)

?2026l received:
  WASM: clear MODE_SYNCHRONIZED_OUTPUT bit
  TS: syncModesFromWasm() → detect flag cleared → renderImmediate()
```

### DECRPM Implementation

DECRPM (`CSI ? Ps $ p`) is a new CSI dispatch case:
- Intermediate: `?`
- Final byte: `p` with intermediate `$` (two intermediates: `?` and `$`)
- Actually: intermediate `$`, with `?` prefix on params — needs parser check

Standard DECRPM encoding:
- Request: `CSI ? Ps $ p` (intermediates: `?`, final: `p`, with `$` as second intermediate)
- Response: `CSI ? Ps ; Pm $ y`
  - Pm=0: not recognized
  - Pm=1: set
  - Pm=2: reset
  - Pm=3: permanently set
  - Pm=4: permanently reset

For mode 2026: return Pm=1 (if set) or Pm=2 (if reset).
For other known modes (7, 25, 2004, etc.): return appropriate Pm value.
For unknown modes: return Pm=0.

### Dependencies

**Internal Dependencies:**
- `wasm/src/terminal_core.rs` - MODE constant definition
- `wasm/src/csi_modes.rs` - handle_set_mode() case for 2026
- `wasm/src/csi_dispatch.rs` - DECRPM dispatch
- `src/terminal-app/pty-handler.ts` - render suppression logic
- `src/terminal/state.ts` - syncModesFromWasm() (may need to expose new mode)

### File Structure

```
wasm/src/
  terminal_core.rs      # Add MODE_SYNCHRONIZED_OUTPUT constant
  csi_modes.rs          # Add mode 2026 case in handle_set_mode()
  csi_dispatch.rs       # Add DECRPM handler (CSI ? Ps $ p)
src/
  terminal-app/
    pty-handler.ts      # Render suppression logic
  terminal/
    state.ts            # Expose synchronizedOutput flag from WASM modes
```

## Test Scenarios

### Unit Tests (WASM/Rust)
- [ ] `handle_set_mode(2026, true)` sets MODE_SYNCHRONIZED_OUTPUT and returns MODE_ACTION_NONE
- [ ] `handle_set_mode(2026, false)` clears MODE_SYNCHRONIZED_OUTPUT and returns MODE_ACTION_NONE
- [ ] MODE_SYNCHRONIZED_OUTPUT default is false (not set on init)
- [ ] DECRPM for mode 2026 returns correct response (Pm=1 when set, Pm=2 when reset)
- [ ] DECRPM for unknown mode returns Pm=0
- [ ] Buffer switch (mode 1049) resets MODE_SYNCHRONIZED_OUTPUT

### Integration Tests (TypeScript)
- [ ] Existing E2E tests pass without regression

### Edge Cases
- [ ] Nested `?2026h` calls (second set is no-op, single `?2026l` clears)
- [ ] `?2026l` without prior `?2026h` (no-op, renders normally)
- [ ] Buffer switch during synchronized mode (mode reset, rendering resumes)
- [ ] Frame budget expiry during synchronized mode (data deferred, no render)

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All unit tests pass
- [ ] Existing E2E tests pass without regression
- [ ] TUI applications (neovim, htop) show reduced flicker when using mode 2026

## References

- Synchronized Output specification: https://gist.github.com/christianparpart/d8a62cc1ab659194571ec2c5f3b4ad28
- DECRPM: https://vt100.net/docs/vt510-rm/DECRPM.html
