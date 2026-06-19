# Implementation Plan: mux Transport/Content Parser Isolation

## Overview

Isolate the outer mux-transport parse from the inner content parse so a mux tab's
`term_core` parser (`Tab::core`) is driven by inner content only, fixing the inline-image
(Kitty / SIXEL) corruption and base64 leak inside mux sessions.

## Objectives

- Add an independent transport extractor that pulls `emterm-mux;` frames out of the PTS
  byte stream without touching `Tab::core`'s parser state.
- Route the tab's outer parse through that extractor once mux is established, keeping
  `Tab::core` for inner content only.
- Remove the temporary DIAG diagnostics added during investigation.

## Prerequisites

### Development Environment
- Rust toolchain pinned by the project (`rust-toolchain`), `cargo`.

### Dependencies
- Internal: `crates/term_core` (`Parser`, `process_pty_data_fully`), `src-tauri/src/mux`
  (`apc::partition_apc_for_mux`, `apply_mux_message`), `src-tauri/src/callbacks.rs`
  (`pending_apc`).
- External: none new.

## Architecture Overview

### Technology Stack
- **Language**: Rust
- **Framework**: native terminal stack (winit / wgpu) — GUI-feature gated
- **Key Components**: `term_core::Parser` (APC/OSC framing), `Tab::pump` (PTS drain),
  `apply_mux_message` (inner content apply)

### Design Approach

A mux tab receives one PTS byte stream that is intrinsically two layers: an outer transport
(`emterm-mux;` APC frames) and, inside `PtyOutput` messages, the inner content (the remote
pane's raw terminal output). Today both layers drive the same `Tab::core` parser, so a Kitty
chunk split across `PtyOutput` boundaries (the daemon reads the PTY in 64KB units) leaves the
parser mid-sequence and the next outer parse corrupts it.

The fix introduces a dedicated transport extractor with its own parser state. Once mux is
established (`mux_session_name.is_some()`), the outer parse of coalesced PTS bytes goes to the
extractor instead of `Tab::core`; the extractor surfaces only the mux-bearing frames into the
existing decode path. `Tab::core` is then driven solely by the inner content via
`apply_mux_message`, so split inner chunks retain their parser state.

Because `term_core`'s `ParsedAction` is crate-private, the extractor is exposed as a public
narrow API from `term_core` (wrapping an independent `Parser`), returning only APC / OSC
payloads — not the full action enum.

### Component Interaction

```
PTS bytes ──► Tab::pump
                 │  mux_session_name.is_some()?
        no ◄─────┤─────► yes
        │                 │
   Tab::core          Transport extractor (independent Parser)
 (outer + Welcome)        │ APC / OSC 9999 frames
        │                 ▼
        └──────► partition_apc_for_mux ──► apply_mux_message
                                              │ PtyOutput arm
                                              ▼
                                        Tab::core (inner content only)
```

## Implementation Phases

### Phase 1: term_core public transport extractor

**Goal**: A reusable, independent extractor in `term_core` that consumes a byte stream and
yields only the transport-relevant frames (APC payloads and OSC param+data), preserving
parser state across calls so split frames reassemble correctly.

**Files to Create**:
- `crates/term_core/src/mux_apc_extractor.rs` - public extractor type wrapping an independent
  `Parser`; surfaces a unified **mux-APC payload list** and discards Print / CSI / Esc /
  Execute. Normalization mirrors `term_core::handle_osc_internal` (`osc_handler.rs:51`):
  - APC frame -> the raw APC payload bytes.
  - OSC 9999 frame whose data starts with `emterm-mux;` -> that data string as an
    APC-equivalent payload (same as the existing `fire_apc_callback(data.as_bytes())`).
  - Any other OSC -> discarded (the outer mux stream carries no other meaningful OSC).

  This keeps both transports (APC default + OSC 9999 ConPTY fallback) flowing into the same
  `partition_apc_for_mux` sink the current `pending_apc` drain feeds, preserving parity.

**Files to Modify**:
- `crates/term_core/src/lib.rs` - export the new extractor type.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Transport extractor | Drive an independent parser over input; collect a unified mux-APC payload list (APC payloads + normalized OSC 9999 `emterm-mux;` payloads) | Holds its own parser state across calls | Returns the mux-APC payloads found; parser state carries any partial frame to the next call |

**Processing Flow** (diagram-convertible):
1. Receive a byte slice.
2. Drive the independent parser over the slice.
   - On APC frame complete -> append its raw payload to the output list.
   - On OSC frame complete with param 9999 and `emterm-mux;` prefix -> append its data string
     (as bytes) to the output list.
   - On any other OSC / Print / CSI / Esc / Execute / DCS -> discard.
3. Return the collected mux-APC payload list; retain partial-frame parser state for the next call.

**Implementation Steps**:
1. **Define extractor type** - owns an independent parser instance and exposes a "feed bytes,
   get mux-APC payloads" operation at responsibility level.
2. **Filter + normalize** - keep APC payloads and OSC 9999 `emterm-mux;` data (normalized to
   the APC payload form); discard everything else.
3. **Preserve cross-call state** - ensure a frame split across two feeds reassembles (the
   parser already models this; the extractor must not reset between feeds).
4. **Export** - make the type public from the crate.

**Dependencies**: Requires `term_core::Parser`. Blocks Phase 2.

**Testing Approach**:
- Unit: complete APC frame in one feed; APC frame split across two feeds; OSC 9999 fallback
  frame; mixed Print + APC (Print discarded, APC kept).

**Acceptance Criteria**:
- [ ] A complete `emterm-mux;` APC frame is returned intact.
- [ ] An APC frame split across feeds reassembles into one payload.
- [ ] An OSC 9999 frame is surfaced.
- [ ] Non-transport output (Print etc.) is discarded.

**Estimated Effort**: medium

---

### Phase 2: Route the outer parse through the extractor when mux is established

**Goal**: When `mux_session_name.is_some()`, `Tab::pump` feeds coalesced PTS bytes to the
extractor instead of `Tab::core`, and the extractor's frames flow into the existing
`partition_apc_for_mux` → `apply_mux_message` path; `Tab::core` is no longer driven by the
outer stream.

**Files to Modify**:
- `src-tauri/src/tabs.rs` - add a transport-extractor field to `Tab`; branch the outer parse
  in `pump` on `mux_session_name`; in the mux branch, skip the `Tab::core` outer parse and
  the outer-stream-only core operations (grapheme flush / device-response / mark-drain) that
  only apply when `Tab::core` itself parses the outer bytes.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `Tab::pump` (mux branch) | Feed PTS to the extractor; route resulting frames to the mux decode path | `mux_session_name.is_some()` | Mux frames decoded; `Tab::core` untouched by the outer stream |
| `Tab::pump` (pre-mux branch) | Existing `Tab::core` outer parse | `mux_session_name.is_none()` | Unchanged behavior |

**Processing Flow** (diagram-convertible):
1. Coalesce `PtyEvent::Data` into one buffer (unchanged).
2. Branch on mux state:
   - mux established -> feed buffer to extractor; obtain transport frames; route them into the
     same partition / apply path the existing `pending_apc` drain uses.
   - pre-mux -> existing `Tab::core` outer parse (grapheme flush, device-response, mark-drain).
3. Inner content continues to be applied by `apply_mux_message` into `Tab::core` (unchanged).

**Implementation Steps**:
1. **Add extractor to `Tab`** - one extractor instance per tab, created with the tab.
2. **Branch the outer parse** - select extractor vs `Tab::core` by `mux_session_name`.
3. **Wire frames to the mux path** - feed extractor output into the existing partition →
   apply flow (the same sink the `pending_apc` drain feeds).
4. **Scope core-only operations** - keep grapheme flush / device-response / mark-drain on the
   pre-mux branch (they pertain to `Tab::core` parsing the outer bytes).

**Dependencies**: Requires Phase 1. Blocks Phase 3.

**Testing Approach**:
- Integration: a Kitty image delivered as inner content split across multiple `PtyOutput`
  messages, with interleaving outer pumps, assembles into one decodable image (core
  regression test).
- Integration: non-mux Kitty image still decodes (no regression).

**Acceptance Criteria**:
- [ ] Inner Kitty chunks split across `PtyOutput` boundaries assemble correctly.
- [ ] No base64 text leaks to the grid in the mux branch.
- [ ] Non-mux image path unchanged.

**Estimated Effort**: medium

---

### Phase 3: Handshake switch, detach restoration, Welcome duplication

**Goal**: The extractor engages on the first pump after Welcome sets `mux_session_name`,
reverts to `Tab::core` on detach, and stays correct under the known double-Welcome delivery.

**Files to Modify**:
- `src-tauri/src/tabs.rs` - on detach (`mux_session_name` cleared) reset/clear the extractor
  state so the pre-mux branch resumes cleanly; confirm the switch timing relative to Welcome;
  ensure double-Welcome does not desync the extractor.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Handshake switch | Use `Tab::core` until Welcome sets `mux_session_name`, then the extractor | Welcome processed | Subsequent pumps use the extractor |
| Detach restore | Return outer parsing to `Tab::core` and clear extractor state | `mux_session_name` cleared | Pre-mux behavior resumes |

**Processing Flow** (diagram-convertible):
1. Pre-mux: `Tab::core` parses outer bytes (incl. the single Welcome APC).
2. After `apply_mux_message` sets `mux_session_name`: next pump -> extractor branch.
3. On detach: `mux_session_name` cleared -> reset extractor -> pre-mux branch resumes.
4. Double Welcome: the existing `first_welcome` guard prevents a second replay; the extractor
   must not retain stale partial state across this.

**Implementation Steps**:
1. **Detach reset** - clear extractor state when `mux_session_name` is cleared.
2. **Switch-timing check** - verify the first post-Welcome pump uses the extractor.
3. **Double-Welcome tolerance** - ensure extractor state stays consistent under duplicate
   Welcome.

**Dependencies**: Requires Phase 2. Blocks nothing.

**Testing Approach**:
- Unit/Integration: pre-mux PTS routes through `Tab::core`; post-detach PTS routes through
  `Tab::core` again; double-Welcome does not corrupt the stream.

**Acceptance Criteria**:
- [ ] Pre-mux tabs are unaffected.
- [ ] Detach restores `Tab::core` routing.
- [ ] Double-Welcome does not corrupt decoding.

**Estimated Effort**: small

---

### Phase 4: Remove DIAG diagnostics

**Goal**: Remove the temporary investigation logs and the diagnostic accessor; restore the
original simple warn in the APC decoder.

**Files to Modify**:
- `crates/term_core/src/terminal_core.rs` - remove the `parser_mid_sequence()` accessor.
- `src-tauri/src/tabs.rs` - remove `DIAG mux PtyOutput ...` (PtyOutput arm), `DIAG drain ...`
  (`drain_and_decode_images`), `DIAG reset_frame_for_replay ...` (`reset_frame_for_replay`).
- `src-tauri/src/mux/apc.rs` - restore `try_decode_emterm_mux` failure-path to the original
  simple warn (remove the DIAG variant).

**Implementation Steps**:
1. **Remove tabs.rs DIAG logs** - three sites listed above.
2. **Remove the accessor** - `parser_mid_sequence()` in term_core.
3. **Restore apc.rs warn** - revert the DIAG decode-failure log to the simple warn.

**Dependencies**: Requires Phase 2/3 confirmed working (remove diagnostics last). Blocks nothing.

**Testing Approach**:
- Unit: existing apc tests still pass; build succeeds with no references to the removed accessor.

**Acceptance Criteria**:
- [ ] No `DIAG` strings remain in the listed files.
- [ ] `parser_mid_sequence()` is removed and unreferenced.
- [ ] Build passes (default + `--no-default-features`).

**Estimated Effort**: small

---

## Complete File Structure

```
crates/term_core/src/
  mux_apc_extractor.rs   # NEW: public independent transport extractor
  lib.rs                 # MOD: export extractor
  terminal_core.rs       # MOD: remove parser_mid_sequence() accessor
src-tauri/src/
  tabs.rs                # MOD: Tab extractor field; pump branch; detach reset; remove DIAG
  mux/apc.rs             # MOD: restore simple warn (remove DIAG)
doc/tasks/mux-transport-parser-isolation/
  SPEC.md / 要件定義書.md / IMPLEMENTATION.md / VERIFICATION.md / sdd.yaml / tasks.yaml
```

## Testing Strategy

- Unit: extractor framing (complete / split / OSC), pre-mux & detach routing.
- Integration: split-chunk Kitty over mux PtyOutput boundaries (core regression);
  non-mux image (no regression). Run `tabs.rs` replay tests with `--test-threads=1`.
- Manual: mux inline image, large image, SIXEL, Markdown viewer / TUI parity, `emterm.log`
  clean of decode failures.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none new) | - | - |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Extractor mishandles APC/OSC split across feeds | Medium | High | Reuse `term_core::Parser` (already models split framing); split-feed unit tests |
| Extractor drops OSC 9999 mux frames (Windows ConPTY regression) | Medium | High | Extractor normalizes OSC 9999 `emterm-mux;` to APC payload form, matching `handle_osc_internal` (`osc_handler.rs:51`); TS-3 covers it |
| Detach path misses extractor reset | Low | Medium | Detach unit test asserting pre-mux routing resumes |
| Removing core-only ops in mux branch drops a needed drain | Low | Medium | Confirm outer mux stream is non-printing (APC only); inner drains stay in `apply_mux_message` |

## Open Questions

- [x] OSC 9999 fallback parity — RESOLVED. `term_core::handle_osc_internal`
      (`osc_handler.rs:51`) already normalizes OSC 9999 `emterm-mux;` frames into the APC
      callback, so today both transports reach `pending_apc`. The extractor replicates this:
      it normalizes OSC 9999 `emterm-mux;` into the same APC payload form (Phase 1), so the
      OSC (Windows ConPTY) transport does not regress (FR3 / NFR1).
- [x] Extractor location — RESOLVED. The extractor lives in `term_core` as a public narrow
      API, because `ParsedAction` is crate-private and `Parser` already handles APC/OSC
      framing (including split frames).

## Success Metrics

- [ ] All FR1–FR7 implemented.
- [ ] mux inline images (Kitty + SIXEL) render with no base64 leak.
- [ ] Split-chunk integration test passes; non-mux and normal mux content show no regression.
- [ ] DIAG diagnostics removed; `cargo test --lib` and `--no-default-features` build pass.
