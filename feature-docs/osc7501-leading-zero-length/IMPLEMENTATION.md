# Implementation Plan: osc7501-leading-zero-length

## Overview

The OSC 7501 whole-sequence limit (4096 bytes) is measured on the bytes as
received, and the plain-tab parser, the mux daemon ingestion and the mux
history strip recognize the OSC number and data by one rule. Two tasks: the
plain-tab path (term_core, program_status, the plain-tab responder) and the
mux path (osc_identify, the daemon feed scanner, the daemon and pane, the
mux query gates, the FR9 test rename).

## Technology Stack

- **Language**: Rust (edition and toolchain as already pinned by the project)
- **Crates touched**: `term_core` (crates/term_core), `emterm` (src-tauri)
- **New dependencies**: none. `project.license` (MIT) is unaffected.

## Layer Structure

| Layer | Location | Responsibility in this feature |
|---|---|---|
| Terminal parser | `crates/term_core` | Counts the received OSC-string length, accumulates the OSC number with saturation, hands both to the host. Knows no application protocol number (NFR2). |
| Protocol core | `src-tauri/src/program_status.rs` | Owns the 4096-byte rule and the body grammar. Builds without `gui` (NFR1). |
| Plain-tab host | `src-tauri/src/callbacks.rs`, `src-tauri/src/tabs/` | Passes term_core's count to the protocol core. |
| Shared OSC rule | `src-tauri/src/mux/osc_identify.rs` | Reference implementation of SC-1 for the mux side. Builds without `gui` (NFR1). |
| Mux ingestion | `src-tauri/src/mux/ipc/pty_spawn/`, `src-tauri/src/mux/daemon/`, `src-tauri/src/mux/session/pane/` | Recognizes OSC 7501 / OSC 133 through SC-1 and applies results to the pane table. |

Dependency direction is unchanged: the plain-tab host and the mux layers
depend on the protocol core; the protocol core depends on neither; term_core
depends on nothing in src-tauri.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|---|---|---|---|
| SC-1: shared recognition rule | Decides an OSC string's number and data | Input: the OSC string (bytes strictly between `ESC ]` and the terminator). Digits before the first `;` accumulate in base 10 with saturation, so leading zeros do not change the value. Non-digit bytes before the first `;` are data, in their original order (digits interleaved with them still accumulate into the number). The first `;` is removed; every byte after it is data. A string without `;` has its non-digit bytes as its whole data. Data handed to text consumers is decoded lossily (invalid UTF-8 becomes U+FFFD). A value above the u16 range is not a number: term_core holds it at 65535, which matches no OSC code (A5); osc_identify reports no number. Both mean "neither OSC 7501 nor OSC 133". The existing `osc_identify::recover_osc` is the reference for the mux side; term_core's OSC string state must produce the same number class and data. | task0001, task0002 |
| SC-2: received OSC-string length | The byte count the 4096-byte rule is applied to | The number of bytes of the OSC string as received: leading zeros, every digit, non-digit bytes before the first `;`, the first `;`, every data byte before U+FFFD replacement, and bytes term_core drops past `MAX_OSC_LEN`. Excludes `ESC ]` and the terminator (BEL, or the `ESC \` of ST). Saturates instead of overflowing. On the mux side it equals the scanner's raw body length for that sequence. | task0001, task0002 |
| SC-3: `program_status::parse_received` | The single place the whole-sequence limit is computed | Signature: `parse_received(body: raw bytes, received_osc_len: count per SC-2, terminator: Terminator) -> Parsed`. Pre: `body` is the SC-1 data as bytes (the lossily decoded text's bytes); the string ended with BEL or ST (an unterminated string never reaches it). Post: `Query` when `body` is exactly `?`, for any `received_osc_len` (FR8). Otherwise `Ignored` when 2 + `received_osc_len` + terminator length (BEL 1, ST 2), computed with saturation, exceeds `MAX_SEQUENCE_BYTES` (4096). Otherwise exactly the result the existing body grammar gives for `body`. Pure: no logging, no panic, no allocation beyond what the existing grammar does. The existing `parse` / `parse_bytes` keep their current results (FR5): each equals `parse_received` with `received_osc_len` = 5 + body byte length (the canonical `7501;` introducer). | task0001 (owner), task0002 (consumer) |
| SC-4: parity corpus | The common expectations both paths are tested against (FR4) | See "Parity corpus" below. Each path's test feeds the seed and then one row and asserts the row's final table; the mux side also asserts the history-strip verdict. | task0001, task0002 |

### Parity corpus (SC-4)

Notation: `Z(n)` is n ASCII `0` bytes, `FF(n)` is n bytes of value 0xFF. The
"OSC string" column is the bytes between `ESC ]` and the terminator. The
length is the whole-sequence length per SC-3. Every row starts from a table
seeded by the canonical `ESC ] 7501;state=working BEL` (one root `working`
record). "Strip verdict" is `osc_body_identity` on the OSC string
(`ProgramStatus` = yes). "Answer" is the plain-tab reply; the mux daemon
never answers. Rows P23 and P24 are fed as live main-screen output.

| Row | OSC string | Term. | Length | Strip verdict | Body given to SC-3 | Final table | Answer |
|---|---|---|---|---|---|---|---|
| P1 | `07501;state=clear` | BEL | 20 | yes | `state=clear` | empty | none |
| P2 | `07501;state=clear` | ST | 21 | yes | `state=clear` | empty | none |
| P3 | Z(4096) `7501;state=error` | BEL | 4115 | yes | `state=error` | root `working` | none |
| P4 | Z(4077) `7501;state=error` | BEL | 4096 | yes | `state=error` | root `error` | none |
| P5 | Z(4078) `7501;state=error` | BEL | 4097 | yes | `state=error` | root `working` | none |
| P6 | Z(4076) `7501;state=error` | ST | 4096 | yes | `state=error` | root `error` | none |
| P7 | Z(4077) `7501;state=error` | ST | 4097 | yes | `state=error` | root `working` | none |
| P8 | `7501;state=error:x=` FF(4074) | BEL | 4096 | yes | `state=error:x=` then 4074 U+FFFD | root `error` | none |
| P9 | `7501;state=error:x=` FF(4075) | BEL | 4097 | yes | `state=error:x=` then 4075 U+FFFD | root `working` | none |
| P10 | `x7501;:state=clear` | BEL | 21 | yes | `x:state=clear` | empty | none |
| P11 | `7501x;state=clear` | BEL | 20 | yes | `xstate=clear` | root `working` | none |
| P12 | `7501` | BEL | 7 | yes | empty | root `working` | none |
| P13 | `7500;state=clear` | BEL | — | no | — | root `working` | none |
| P14 | `17501;state=clear` | BEL | — | no | — | root `working` | none |
| P15 | `75010;state=clear` | BEL | — | no | — | root `working` | none |
| P16 | `65536;state=clear` | BEL | — | no | — | root `working` | none |
| P17 | `655367501;state=clear` | BEL | — | no | — | root `working` | none |
| P18 | `07501;?` | BEL | 10 | yes | `?` | root `working` | one `ESC ] 7501;?` BEL |
| P19 | Z(5000) `7501;?` | ST | 5010 | yes | `?` | root `working` | one `ESC ] 7501;?` ST |
| P20 | `7501;state=clear` | BEL | 19 | yes | `state=clear` | empty | none |
| P21 | Z(9000) `7501;state=clear` | BEL | 9019 | yes | `state=clear` | root `working` | none |
| P22 | FF(1) `7501;:state=clear` | BEL | 21 | yes | U+FFFD `:state=clear` | empty | none |
| P23 | `0133;A` | BEL | — | no | — | empty (prompt start) | none |
| P24 | `133;A` | BEL | — | no | — | empty (prompt start) | none |

On the mux side P21 is dropped by the scanner's 8 KiB carry bound before it
reaches SC-3; the final table is the same.

## Conventions

- **Arithmetic on PTY-derived values**: every count and every OSC number
  derived from PTY bytes saturates; nothing on these paths can panic in a
  debug build or wrap in a release build (NFR3).
- **Logging**: rejected, ignored or unrecognized input produces no log line on
  either path (NFR4). The mux scanner's existing one-shot carry-overflow
  warning is the only log on that path and stays as it is.
- **Feature gate**: `program_status` and `osc_identify` gain no dependency on
  a `gui`-gated item (NFR1).
- **Platform**: no platform-gated code is added (NFR5).
- **Test names**: existing tests keep their names and are updated in place
  when a helper or an item shape changes, so no other test-docs record
  changes. The only rename is the FR9 test, owned by task0002.

## Cross-task Design Decisions

### D1: Measure at the receiving edge, decide in one place

term_core (plain tab) and the daemon feed scanner (mux) each count SC-2 where
the bytes arrive, because only they see leading zeros, the first `;` and the
bytes before replacement. Neither applies the 4096-byte rule itself: both
hand the count to SC-3, so the plain tab and the mux cannot disagree on the
arithmetic. Affects: task0001, task0002.

### D2: The canonical entries stay

`parse` / `parse_bytes` keep their results for the canonical spelling (FR5)
and remain available to existing tests and callers. Production ingestion on
both paths calls `parse_received`. Affects: task0001, task0002.

### D3: One body text on both paths

Both paths hand SC-3 the SC-1 data decoded the same lossy way term_core
decodes it, so for the same bytes the plain tab and the mux parse the same
body (FR4). The mux scanner's former one-byte substitution of non-ASCII bytes
existed only to keep the length rule right; with SC-2 carried separately it
is no longer needed. Affects: task0001, task0002.

### D4: Missing-foundation rule

Tasks run in parallel in separate worktrees. When task0002's worktree lacks
SC-3, task0002 adds `parse_received` at the owner's path with the owner's
name and the SC-3 contract, enough to compile and pass its own Acceptance
Criteria, and writes no tests for owner-only behavior. task0001's version
supersedes it through parent-side adoption at merge. Affects: task0002.

### D5: File-overlap map

| File | Tasks |
|---|---|
| `src-tauri/src/program_status.rs` | task0001 (owner of SC-3), task0002 (D4 seam only) |
| `src-tauri/src/tabs/tests/output_pipeline.rs` | task0001 (plain-tab tests), task0002 (coalesce-gate tests) |

Each task changes only the parts of a shared file its own plan names.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| task0002's D4 copy of SC-3 differs from task0001's | Low | High | SC-3 pins the contract; parent-side adoption keeps the owner's version; both halves of SC-4 run again in verify |
| Adding a field to term_core's dispatched OSC action breaks every exact-match parser test | High | Low | task0001 owns those tests and updates their expectations in place |
| A responder that only implements the existing method changes behavior | Low | High | The new responder method defaults to the existing one; existing responder tests stay unchanged (task0001 AC-3) |
| Merge conflicts in the D5 files | Medium | Low | D5 map, parent-side adoption |

## Open Questions

- [ ] `payload_has_device_query` does not treat a 7501 query whose `?` sits
  before the first `;` (for example `ESC ] 7501? BEL`) as a query, while
  term_core answers it. The gate only decides coalescing, so the answer is
  still produced; an existing test pins the current gate behavior and the
  SPEC lists no change for it. Left unchanged.
- [ ] An OSC string ended by `ESC ESC \` is cut short (unterminated) by
  term_core but committed as ST by the daemon feed scanner. This predates the
  feature and lies outside the SC-4 corpus. Left unchanged.
