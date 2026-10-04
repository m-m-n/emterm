# Implementation Plan: mux-write-filter-overflow-lone-esc

## Overview
The scrollback write filter's overflow flush stops writing a final live lone ESC and holds it in `pending` (one byte), so the next PTY read strips it together with its continuation, as the non-overflow path already does. The feature is one task (task0001); this document records the feature-level contract and decisions that the task, the review and the verify phase share.

## Technology Stack
- **Language / crate**: Rust, `src-tauri` crate, `mux` module (compiled into both the default GUI build and the `--no-default-features` CLI build).
- **Test oracle**: `term_core` (the workspace crate the mux tests already use) replays written bytes; the write-path strip of `scrollback_filter.rs` is the exact-bytes oracle.
- **New dependencies**: none. No dependency license to record; `project.license` (MIT) is unaffected.

## Layer Structure
PTY reader loop (`src-tauri/src/mux/ipc/pty_spawn/mod.rs`) → scrollback write filter (`src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`) → strip module (`src-tauri/src/mux/scrollback_filter.rs`) → scrollback ring.

- The change is confined to the overflow branch of the write filter's cut-aware feed and to the write filter's doc comments.
- The dependency direction is unchanged: the write filter calls the strip module; the strip module never depends on the IPC layer.
- Not modified: `scrollback_filter.rs`, the reader loop in `pty_spawn/mod.rs`, `pty_spawn/suppressed_output.rs` (NFR3, SPEC A6).

## Shared Components
| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|---|---|---|---|
| Held state of `ScrollbackWriteFilter` after an overflow flush (`pending()`, `pending_len()`, held construct start, written state, awaiting-designator flag, attribution dims of the held bytes) | Bytes held back for the next call, and the state that goes with them | Post (widened by FR2 / NFR4): right after an overflow flush, `pending()` is empty, or holds exactly one byte: the live lone ESC that FR1 holds, which happens only when the flush is in the call's last segment. That held ESC is in the same state as a lone trailing ESC held by the non-overflow path: held construct start 0 (a chain of one construct), no designator wait, written state = end state of the bytes written before it, attribution dims of the held byte = the overflowing call's `current_dims`. Every other postcondition of `pending()` is unchanged. | task0001 (producer). Existing consumers, not modified: the reader's suppressed-chunk pipeline (passes `pending()` to the replacement builder as the pending tail) and the reader's per-outcome dims attribution |
| Strip state-reporting form (`scrollback_filter.rs`) | Strip a run and report the written end state | Unchanged (NFR3). The overflow branch calls it once per flushed run: over the run without its final byte when that byte is ESC, over the whole run otherwise | task0001 |

## Conventions
- **Test placement**: the feature's tests live in the new module `src-tauri/src/mux/ipc/pty_spawn/tests/overflow_lone_esc.rs`, registered from `src-tauri/src/mux/ipc/pty_spawn/tests.rs` in the same way as the sibling modules. Test names start with `overflow_lone_esc_`.
- **Helper reuse**: existing test helpers are used through their current visibility (for example `osc_held_at_the_cap`, `view_of`, `view_after_a_cut`, `emitted_through`, `all_targets`, `switch_pairs`, `run_reader_without_owner`, `client_written_state`). Other test modules are not edited; a helper that is private elsewhere is written locally in the new module.
- **Oracle convention** (the one mux-strip-escape-state-carry uses): replay checks compare term_core views (rows, cursor, responses). For a cut, the reference is term_core fed the raw stream with a 47 / 1047 / 1049 `h` / `l` pair in place of the cut, with the removed construct's own effect kept out. Exact-bytes checks compare against the write-path strip.
- **Existing tests**: no existing test's expectation changes and no existing test is renamed (NFR3), so the update duty of `.claude/rules/test-docs-records.md` does not arise. A failing existing test is a plan deviation to report, not an expectation to edit.
- **Logging**: the existing overflow `warn` stays as it is; no log line is added.
- **Builds**: no platform-gated code and no GUI-only crate in the change; it compiles in the default and the `--no-default-features` builds on Linux and Windows (NFR5).

## Cross-task Design Decisions

### D1: One task
- **Decision**: the feature is a single task.
- **Rationale**: the fix is one branch of one function, and every new test of the feature fails until that branch changes. Tasks run in parallel from the same base, so a split would leave a test-only task that cannot pass in its own worktree.
- **Affected tasks**: task0001.

### D2: Hold mechanism (SPEC A2 adopted as written)
- **Decision**: the final live lone ESC is kept in `pending` as a chain of one construct, in the held state of the Shared Components row above. No field, flag or state is added to the filter; the next call takes the existing carried-run path unchanged.
- **Rationale**: the later-call behavior FR4 requires (strip together with the continuation, a non-strip continuation written whole, drop plus one closure at a cut or the reader's fallback, carried completion, the suppressed-chunk tail) is then the existing behavior of a non-overflow held ESC, with no second code path.
- **Affected tasks**: task0001.

### D3: What counts as a live lone ESC (FR1 scope)
- **Decision**: the flushed run's last byte is ESC and, starting from the end state of the bytes the strip writes before it, that ESC leaves the written stream in the Escape state (the strip's written-state model). This excludes the designator byte of `ESC (` / `ESC )` (the written state after it is Ground) and every run whose last byte is not ESC (the R4 / R9 forms, `ESC[6` + a removed construct, plain bytes). It includes an ESC after a written ESC, an ESC after an open CSI, and an ESC that ends inside an open OSC / DCS / APC string body (the written-state model treats a string body as ground).
- **Rationale**: FR1's definition taken literally; NFR2 (from each of these positions the non-overflow path strips the same continuations).
- **Affected tasks**: task0001.

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| An existing overflow test pins an empty `pending` after a last-segment run that ends in a live lone ESC, and fails (NFR3) | Low: the overflow tests read for this plan end their runs in plain bytes, `ESC (`, `ESC ( ESC`, `ESC[6` or a complete construct | Medium | Full `--lib` run (TS-7); a failure is reported as a deviation, never edited |
| A run that ends inside an open OSC / DCS / APC string with a trailing ESC (D3), followed later by a cut or the reader's fallback: the held ESC is dropped and the closure follows a written state that models the string body as ground, so nothing closes the string and ring bytes written after the cut replay inside it until a BEL or ESC ends it. Before this feature the written ESC plus the Escape closure closed it. | Low: needs an unterminated string past 512 KiB whose overflowing read ends in ESC, then a screen switch | Low: display only; no query or launch | Same outcome as the existing overflow flush of an open string followed by a cut (pinned by `round4_overflow_flush_then_a_cut_without_a_wait_writes_nothing_extra`). task0001 pins the bytes so the behavior is deliberate; listed under Open Questions |
| The hold adds a pass over the run or keeps more than one byte | Low | Medium | Single strip pass, O(1) extra work, `pending` at most one byte (TM-2; TS-3, TS-7) |
| The suppressed-chunk replacement path treats the held ESC differently from a non-overflow held ESC (SPEC A6) | Low | Medium | Held state identical to the non-overflow hold (D2); production-reader test (TS-6) |

## Open Questions
- [ ] Should a later feature close, at a cut, an open string that an overflow flush left in the ring (Risk row 2, and the existing case without a trailing ESC)? Outside this feature's scope; not blocking.
