# Implementation Plan: OSC Color Query Response

Cross-task decisions only. Per-task detail lives in `tasks/taskNNNN.md`.
Requirement IDs (FR1-FR10 / NFR1-NFR6) are the ones in
`feature-docs/osc-color-query-response/SPEC.md` and `workflow.yaml`.

## Overview

Answer OSC 4 / 10 / 11 / 12 color queries out of the live GUI theme, deliver
every answer through the existing single-slot device-response route, and
correct the two reset paths (OSC 110 / 111 and OSC 104) so that a value a
query reports is always the value the renderer is using at that moment.

## Technology Stack

- **Language**: Rust (existing crates only). No TypeScript / WebView surface
  is involved.
- **Existing modules in scope**: `crates/term_core` (OSC parse and dispatch,
  the single-slot response buffer, the existing color-response formatter),
  `src-tauri/src/render/theme.rs` (the live theme), `src-tauri/src/tabs/`
  (output pipeline, replay, frame classification).
- **New dependencies**: none. No third-party crate is added, so the project's
  `MIT` license acquires no new obligation and no license check is triggered
  (`references/license-compat.md`: zero new dependencies to classify).

## Layer Structure

| Layer | Responsibility | May depend on |
|---|---|---|
| `crates/term_core` | OSC parsing and dispatch, the single-slot response buffer, the one color-response formatter | nothing in `src-tauri/` |
| `src-tauri/src/render/theme.rs` (GUI) | the live theme: the values a query reports and the values a reset restores | `term_core` |
| `src-tauri/src/tabs/` (GUI) | drains pending responses to the PTY, decides frame coalescing, discards pending responses on replay | `term_core`, theme |

Dependency direction is one-way and stays that way: `term_core` never learns
about the GUI theme (NFR4). The query is therefore answered in the GUI layer
and the answer is handed **down** to `term_core`, which owns the buffer.

## Shared Components

| Component | Responsibility | Contract (pre / post) | Used by tasks |
|---|---|---|---|
| **SC-1 Terminator kind** | Carry which string terminator ended an OSC string (BEL form vs ST form) from the parser to the OSC dispatch boundary and on to any response producer | **Pre**: every OSC dispatch carries exactly one terminator-kind value, derived from the bytes actually received; a string ended by anything other than a terminator (buffer flush, truncation) is classified deterministically and that choice is documented at the definition site. **Post**: a response produced for that dispatch ends with the byte form the value names; no producer hardcodes a terminator | task0001 (defines/produces), task0002 (consumes), task0004 (asserts end to end) |
| **SC-2 Host color-responder seam** | The registration point through which the GUI layer supplies a responder that `term_core` consults on OSC dispatch | **Pre**: registration is optional; an unregistered core behaves exactly as today (no response, no panic, no behavior change). The responder receives the OSC numeric code, the raw payload, and the SC-1 terminator kind. **Post**: the responder returns zero or more complete response byte sequences; `term_core` appends them in returned order to its pending response content, never inspecting or rewriting them; they leave the core only through the existing `take_response` drain and through no other route | task0001 (defines), task0002 (registers + implements), task0004 (integration-tests the route) |
| **SC-3 Pending-response accumulation** | The single-slot response buffer must keep every response produced before the next drain | **Pre**: the buffer is empty or holds undrained bytes. **Post**: every response produced within a parse pass survives until the next drain, in production order (a chained payload produces several); a drain returns all pending bytes and empties the slot. Existing DA1 / DSR / CPR response behavior is unchanged | task0001 (owns), task0002, task0004 |
| **SC-4 Theme OSC entry contract** | The single entry point the GUI theme exposes for OSC codes it owns | **Inputs**: OSC code, raw payload, SC-1 terminator kind. **Outputs**: a visible-change flag, plus ordered response byte sequences. **Invariants**: answering a query never sets the visible-change flag and never mutates theme state; existing set semantics are unchanged; the existing "no visible change → false" contract is preserved, so answering a query never causes a full-grid dirty mark | task0002 (owns the shape change and the query side), task0003 (reset side, same entry) |
| **SC-5 Scheme mirror fields** | Hold the active color scheme's values so a reset can restore them in place | **Pre**: seeded at theme construction from the same constants that seed the corresponding live fields. **Post**: updated only where a color scheme is applied — unconditionally in the preset branch, and in the user-scheme branch only under the same parse guard that gates the live field's update; never written by any OSC set sequence; read only by the reset paths. New fields mirror the existing cursor-color mirror exactly | task0003 (owns), task0002 (its post-reset query values follow from them; never writes them) |
| **SC-6 Palette value resolution rule** | The one rule for "what color is palette index `i` right now" | For index `i`: the sparse overlay's entry when it holds a value; otherwise the 16-color array when `i < 16`; otherwise the xterm 256-color cube / grayscale formula for `16..=255`. An unset overlay slot is never reported as a missing value | task0002 (implements), task0003 (asserts post-reset values against it) |
| **SC-7 Device-query frame classification** | The predicate that keeps a PTY output frame from being coalesced | **Pre**: the predicate sees a frame's raw payload bytes. **Post**: a frame carrying any OSC 4 / 10 / 11 / 12 query token is classified as a device query and parsed alone; frames without one keep current behavior; classification never consumes or rewrites bytes | task0004 (owns), task0002 (its answered query forms define the set that must be recognized) |

## Conventions

- **One formatter**: the existing color-response formatter in `term_core`
  (8-bit components expanded to 16-bit, `rgb:rrrr/gggg/bbbb`) is the only
  producer of response color text. No second formatter is introduced (NFR1).
- **One delivery route**: no new PTY write path, and no second live consumer
  of the response drain. Everything reaches the PTY through the three
  existing drain sites (FR6).
- **Inert invalid input**: malformed, out-of-range or unsupported input
  produces neither response bytes nor state change. It is not an error type
  and it is not logged — the bytes come from an untrusted PTY stream and
  logging them invites spam.
- **Naming**: scheme mirror fields keep the existing `scheme_` prefix and
  mirror their live counterpart's name.
- **Logging**: no new logging on the OSC hot path. Anything genuinely
  unexpected is logged at warn or above (release builds drop lower levels —
  `.claude/rules/debugging-constraints.md`).
- **Tests**: inline unit tests in the module under test, matching the
  project's existing pattern. The app crate's test command already pins
  single-threaded execution; keep new tests independent of execution order.
- **Portability**: nothing on the response path may use a Unix-only API
  (NFR6), and nothing added to `term_core` may require the GUI feature
  (NFR5).

## Cross-task Design Decisions

### D1 — The query is answered in the GUI layer, never in `term_core`

`term_core` owns the response buffer but must not see the theme (NFR4). The
value resolution therefore lives beside the theme, and the resulting bytes
are handed down through SC-2. This is why SC-2 exists at all: it is the only
sanctioned inversion point between the two layers.
Affected: task0001, task0002.

### D2 — Responses travel by return value into the existing buffer

A responder returns bytes; it never writes to a PTY and never reaches into
the pipeline. The buffer and its existing drain remain the single delivery
route (FR6, and the invariant that the drain has exactly one live consumer).
Affected: task0001, task0002, task0004.

### D3 — The terminator is a carried value, never a hardcoded choice

FR5 requires the response terminator to match the request's. The OSC
dispatch boundary today carries only code and payload, and whether the parser
retains the terminator is unverified (SPEC assumption A6). **task0001 owns
resolving A6**: if the parser already retains it, task0001 carries it
forward; if not, task0001 adds the retention. Either way the value reaches
the responder as SC-1.
Affected: task0001, task0002.

### D4 — Resets restore from mirror fields, because nothing else is available

The theme holds no handle to settings, so a reset cannot re-read the active
scheme; restoring from a mirror field held on the theme is the only in-place
mechanism (SPEC assumption A3). FR8 and FR9 therefore add mirrors rather than
re-deriving values, following the existing cursor-color mirror exactly (SC-5).
Affected: task0002 (reads the resulting values), task0003 (owns).

### D5 — Coalescing safety is settled by the existing device-query gate

The pipeline already refuses to coalesce frames carrying a device query,
precisely because the response buffer is single-slot. Whether that classifier
recognizes OSC color queries is unverified (SPEC assumption A7). **task0004
owns resolving A7**: it either extends the classifier or pins the
already-correct behavior with a test, and reports which it found (NFR2, SC-7).
Affected: task0004.

### D6 — A chained payload produces several responses within one dispatch

A chained default-color payload or a multi-pair palette payload answers more
than once per dispatch. The buffer must therefore accumulate within a parse
pass rather than overwrite (SC-3); the coalescing gate (D5) covers only the
across-frame case, not this one. Both protections are required.
Affected: task0001, task0002, task0004.

### D7 — Wiring ownership (no unowned placeholder)

The end-to-end wiring — registering the theme-backed responder into the SC-2
seam at the production call site — is owned by **task0002** and is one of its
Acceptance Criteria. task0001 ships the seam with an unregistered default
that is a genuine no-op (not a placeholder to be replaced). If task0002's
worktree does not yet contain the seam, it implements the seam itself,
exactly to SC-2; that is why the `term_core` files appear in both tasks'
`files` lists and why a merge conflict there is expected and resolved by the
implementer's parent-side-adoption protocol.

### D8 — Predicted file names inside `crates/term_core/src/`

Planning ran without filesystem discovery, so the OSC parser module and the
core-terminal module inside `crates/term_core/src/` are named by prediction.
The scoped unit is "the OSC parsing and core-terminal modules inside
`crates/term_core/src/`"; if the actual module file names differ from the
predicted ones, the equivalent modules in that directory are in scope and the
name difference is reported as a plan deviation rather than treated as a
license to widen the change beyond that directory.

### D9 — Exactly one core answers a query for a given pane

In a mux session, PTY output is parsed on more than one side of the bridge.
Registering a responder on more than one of them would emit duplicate
answers, which is indistinguishable to the program from corrupted input.
**task0004 owns confirming which side answers for a remote pane** and pins it
with a test; task0002's wiring registers the responder on exactly the side
that owns the live theme.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Duplicate answers across the mux bridge (two cores dispatch the same OSC) | Medium | High | D9: task0004 confirms and pins the answering side; registration happens on one side only |
| A6 unverified — the parser may not retain the terminator | Medium | Medium | D3: task0001 owns retention; scope covers both outcomes |
| A7 unverified — the classifier may not recognize color queries | Medium | High | D5: task0004 owns; a test pins the behavior either way |
| A chained payload's later response overwrites an earlier one | Medium | High | SC-3 accumulation contract, verified by TS-3 and TS-13 |
| Two tasks edit the theme module in parallel | High | Low | The regions are disjoint (query resolution vs reset paths); SC-4 pins the shared entry shape; conflicts resolve by parent-side adoption |
| FR8 / FR9 change what is on screen after a reset | Medium | Medium | Updated unit tests (TS-10) plus manual visual verification (TS-14); the new appearance is the scheme the user already configured |
| A response leaks into a running shell's stdin (the historical failure mode) | Low | High | FR6's single route plus NFR3's unchanged replay discard, both asserted by task0004 |

## Open Questions

- [ ] Which side of the mux bridge dispatches OSC for a remote pane, and
      therefore which one registers the responder (D9) — task0004 confirms
      before task0002's wiring is considered final.
- [ ] Whether the `term_core` parser already retains the string terminator
      (SPEC A6) — task0001 reports which case it found.
- [ ] Whether the frame classifier already recognizes OSC color queries
      (SPEC A7) — task0004 reports which case it found.
- [ ] OSC 12 is planned to report the live cursor color whether or not a
      cursor override is active (FR1). Confirm no existing consumer expects
      the pre-override value instead.
