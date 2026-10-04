# Implementation Plan: mux-vt100-del-closing

## Overview
Two consumers hand scrollback-ring bytes to a vt100 parser: ReadPane
rendering and the restored shadow-parser replay. After this change, both
receive a copy in which every DEL that `term_core` reads as a CSI cancel
or an escape end is replaced by CAN. The ring, the strip and write-filter
output, and the bytes the client's `term_core` receives stay
byte-identical (NFR1).

## Technology Stack
- **Language**: Rust (the crate under `src-tauri/`)
- **Key libraries**: vt100 (existing), used by the ReadPane scratch parser and the pane shadow parser
- **New dependencies**: none, so there is no license entry to record (project license: MIT)

## Layer Structure
| Layer | Location | Role in this feature |
|---|---|---|
| Shared byte-stream module | `src-tauri/src/mux/scrollback_filter.rs` | Owns the written-stream state (`WrittenState`) and the new replay-copy conversion |
| IPC handlers | `src-tauri/src/mux/ipc/handlers/agent_api.rs` | ReadPane rendering consumes the copy |
| Session | `src-tauri/src/mux/session/pane/mod.rs` | The restored shadow replay consumes the copy |

Allowed dependency directions: IPC to the shared module, and session to the
shared module. The shared module depends on neither the IPC layer nor the
session layer, as the module's own documentation already requires. None of
the three is behind the `gui` feature, and the new function must not be
either (NFR3).

## Shared Components
| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|---|---|---|---|
| vt100 replay copy: function `vt100_replay_copy` in `src-tauri/src/mux/scrollback_filter.rs`. It is visible to the `mux` module tree (the same visibility as the module's other mux-shared items) and has no feature gate | Builds the copy of a byte run that is handed to a vt100 parser | Pre: a byte slice with any content, possibly empty. Post: (1) it returns a new owned byte vector whose length equals the input length. (2) Output byte i equals input byte i, with one exception: an input DEL (0x7F) becomes CAN (0x18) when the scan state just before it is "inside a CSI" (entry or parameter sub-state) or "right after an ESC". (3) The scan starts in ground and advances by each original input byte through the existing `WrittenState` transition, so a replacing CAN never reaches the scan. (4) It does not modify the input. (5) It never panics, makes one pass and keeps O(1) state besides the output. | task0001 |

## Conventions
- The ring is never rewritten for vt100. The conversion applies only to the bytes handed to a vt100 parser, at the call site, right before that parser processes them.
- The scan reuses `WrittenState`'s existing per-byte transition, which itself uses `csi_step`. No second transition table is written. The scan does not go through the strip's write step (the `Written` writer, which applies the D2 rewrite of an answered query's final byte), because the copy may differ from the input only by DEL becoming CAN.
- Doc comments on new and changed items name this feature and the FR IDs they implement, as the existing doc comments in `scrollback_filter.rs` do.
- Error handling: the conversion has no error case. The restore path keeps its panic guard, parser reset and error log.

## Cross-task Design Decisions

### D1: Convert at the vt100 hand-off and keep DEL in the ring
- Decision: CSI_CLOSING stays one DEL byte in the ring and in everything the client receives. Only the copies handed to vt100 carry CAN.
- Rationale: `term_core` cancels a CSI on DEL but not on CAN. vt100 cancels a CSI on CAN but not on DEL. The ring feeds both, so a single closing byte can satisfy only one of them. Converting at the vt100 hand-off satisfies both and keeps NFR1.
- Affected tasks: task0001

### D2: The written-stream state decides which DELs are replaced
- Decision: a DEL is replaced exactly when the written stream is inside a CSI (entry or parameter) or right after an ESC. It is kept in ground, in OSC, DCS and APC bodies (which `WrittenState` treats as ground), and while a charset designator is pending.
- Rationale: `term_core` ends the sequence on DEL in exactly these states (FR3, FR4). A DEL that a program writes inside a CSI is replaced as well. `term_core` cancels on that DEL too, so vt100 still matches `term_core`.
- Affected tasks: task0001

### D3: Every copy is scanned from ground
- Decision: no state is rebuilt for bytes before the ReadPane tail cut or before the ring's evicted head (SPEC a4). A DEL before the first ESC of the copy is in ground and is kept. vt100 also starts in ground and ignores a DEL there.
- Affected tasks: task0001

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| The scan goes through the strip's write step, so the copy rewrites a device query's final byte even when the input has no DEL | Low | Medium | Conventions above. task0001 AC-4 requires a DEL-free input that contains a complete device query to come back unchanged |
| The conversion is applied to the ring or to the client snapshot path, which changes what `term_core` sees | Low | High | D1. task0001 AC-3 checks that the restored ring keeps the DEL, and AC-7 limits the non-test call sites to the two vt100 hand-offs |
| A crafted byte sequence makes the conversion panic on the ReadPane path, which has no panic guard | Low | Medium | THREAT-MODEL.md TM-1, task0001 AC-6 |
| State before the tail cut or the evicted head is lost, so a closing DEL whose CSI started before the copy is kept | Medium | Low | Accepted known constraint (SPEC a4 and its edge cases) |
| `term_core` and vt100 handle a raw CAN inside a CSI differently | Low | Low | Existing difference, out of scope (SPEC.md edge cases) |

## Open Questions
- None.
