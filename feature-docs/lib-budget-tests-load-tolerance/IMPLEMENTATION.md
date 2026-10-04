# Implementation Plan: lib-budget-tests-load-tolerance

## Overview

Two named `--lib` tests stop failing under environment load (load average 10-20): the token-corpus replay test loses its wall-clock assert, and the strip_concat alternations/chains test judges its 10 s budget on the thread CPU time of the code under test only. The change is test-only, plus one feature on an existing Windows dependency.

## Technology Stack

- **Language / test runner**: Rust (the `src-tauri` crate), libtest, run through the `--lib` unit-test target.
- **Existing dependencies used**: libc (unix targets) and windows-sys (Windows) for the per-thread CPU clock.
- **Dependency change**: windows-sys gains the Win32_System_Threading feature (SPEC FR5).
- **New dependencies**: none. No license record is needed beyond this line: libc and windows-sys are already dependencies of the crate, and only a feature flag is added; `project.license` (MIT) is unaffected.

## Layer Structure

- **Test layer** — every Rust change lives under the test-only pty_spawn tests module (`src-tauri/src/mux/ipc/pty_spawn/tests.rs` and its `tests/` submodules), compiled only for the test build. Production code (the write strip, the snapshot strip, the scrollback write filter) is unchanged and is only called.
- **Build configuration** — `src-tauri/Cargo.toml`, Windows target dependency table only.
- **Allowed dependency direction**: test modules may depend on a sibling test module and on production code; production code never depends on any test module. The OS clock is reached only through the existing libc / windows-sys dependencies.

## Shared Components

None. The decomposition leaves no component that one task builds and another task uses:

- The thread CPU-time helper and its budget judgment are built and consumed inside task0001.
- task0002 removes a wall-clock assert without replacing it (decision D1 below), so it uses no new component.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| (none) | — | — | — |

## Conventions

- **Correctness assertions are untouched** in both named tests: every assertion keeps its condition and its message (SPEC A2). Only the time judgment changes.
- **Test names are unchanged.** Neither named test is renamed, so test-docs records of earlier features that list them keep resolving without an update.
- **Scope (SPEC FR9)**: no other wall-clock budget changes. The module-level `BUDGET` constants and the time imports of `round4_chain.rs` and `strip_concat_query.rs` stay, because other tests in those files still use them (SPEC A4); the change introduces no unused-import or dead-code warning.
- **Provenance comments** follow the existing convention of the pty_spawn tests module: feature slug, task ID and the SPEC requirement IDs.
- **No new crate** (SPEC NFR1). Platforms are Linux and Windows (SPEC NFR2); nothing macOS-specific is added.
- **Warnings**: the default build check, the `--no-default-features` check and the `--lib` test build produce no new compiler warning from the changed files.

## Cross-task Design Decisions

### D1: The token-corpus test gets no replacement budget

- **Decision**: the wall-clock start and the elapsed assertion are removed from `the_ring_at_a_cut_replays_like_the_raw_stream_over_the_token_corpus`; no CPU-time budget takes their place.
- **Rationale**: SPEC FR1 removes the time judgment from that test outright.
- **Affected tasks**: task0002 (owns the removal); task0001 (consequence: it owns the only consumer of the helper).

### D2: Disjoint file ownership

- **Decision**: task0001 owns `src-tauri/Cargo.toml`, `src-tauri/src/mux/ipc/pty_spawn/tests.rs`, the new helper module file and `strip_concat_query.rs`; task0002 owns `round4_chain.rs`. No file appears in both tasks.
- **Rationale**: the two tasks run fully in parallel; disjoint file sets leave nothing to reconcile at merge time.
- **Affected tasks**: task0001, task0002.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Thread CPU time still grows somewhat under contention (frequency scaling, cache, SMT — SPEC EC1, A3) | Medium | Low | The unloaded measured total is well under the 10 s budget (whole test about 3.7 s); TS-7 runs the test under synthetic load |
| The helper's own tests become load-sensitive and reintroduce the failure this feature removes | Medium | Medium | task0001 compares CPU time with CPU time or with a sleep length, keeps the one wall-time comparison as a low floor, and TS-7 runs those tests under load |
| The Windows clock branch is not executed in a Linux verify environment | High | Medium | Windows cross check of the test code where the toolchain is available, plus a manual Windows run listed in VERIFICATION.md |
| Out-of-scope wall-clock budget tests (kept by SPEC FR9) fail under TS-7's synthetic load | High | Low | TS-7 judges only the two named tests; other failures are recorded and not attributed to this feature |

## Open Questions

- [ ] Whether a Windows environment is available during verify for the manual run of the helper tests (the cross check of the test code covers compilation only).
