# Implementation Plan: sftp-send-scan-crlf

## Overview

Make the SFTP send-site structural scan (the detection routine inside the
test-only module of `src-tauri/src/sftp/service.rs`) produce the same result
whether its input text uses LF, CRLF, or a mixture of both, and pin that
behavior with CRLF regression cases. The feature is a single task
(task0001).

## Technology Stack

- **Language**: Rust — the existing `src-tauri` crate; all changes are unit
  test code inside the module's `#[cfg(test)] mod tests` item.
- **New dependencies**: none. No license check applies; `project.license`
  (MIT) is unaffected.

## Layer Structure

The change lives entirely in the test layer of one module: the
`#[cfg(test)] mod tests` item of `src-tauri/src/sftp/service.rs`.

| Element | Role in this feature | Changed? |
|---------|----------------------|----------|
| Production code of `service.rs` | Scanned subject | No (NFR1) |
| `#[cfg(test)] impl SftpService` seam block | Scanned subject | No (NFR1) |
| Boundary locator (`find_mod_tests_boundary`) | Finds the `mod tests` item start in the text it is given | Signature and anchor unchanged (A-2) |
| Detection routine (`scan_for_bare_send_sites`) | Normalizes line endings, then locates the boundary and inspects the region before it | Yes (FR1, FR2) |
| Real-source structural test | Feeds the module's own file text to the detection routine | No behavioral change |
| Red-case test (`send_site_scan_detection_routine_red_cases`) | Feeds synthetic texts to the detection routine | Gains CRLF / mixed cases (FR3) |

Call direction is unchanged: both tests call the detection routine; the
detection routine calls the boundary locator.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| (none) | This feature has a single task; no component is built by one task and consumed by another. The detection-routine contract is stated in task0001's plan and in decision D1 below. | — | — |

## Conventions

- **Change scope (NFR1)**: edits stay inside the `#[cfg(test)] mod tests`
  item of `src-tauri/src/sftp/service.rs`. Production code, the seam block,
  and public API are untouched.
- **Boundary anchor (A-2)**: the boundary is still "a `#[cfg(test)]` line
  immediately followed by a `mod tests` line"; the boundary locator keeps its
  signature.
- **Forbidden-shape assembly (NFR2)**: the two forbidden send shapes (the
  progress sender name joined with the send call, and the result sender name
  joined with the send call) are only ever produced by joining parts at run
  time. Neither the detection routine nor any test source spells a joined
  shape verbatim; a whole-file text search for either joined shape returns
  zero matches.
- **Line-ending scope (A-1)**: LF and CRLF, including mixtures within one
  text, are supported. A lone CR is not treated as a line break and is left
  as-is.
- **Out of scope (A-3)**: repository-side line-ending pinning (for example
  adding `.gitattributes`) is not part of this feature.
- **Commands**: cargo runs from the project root with `--manifest-path` and
  `CARGO_TARGET_DIR=src-tauri/target`. Formatting is verified with the
  check-only format command; no crate-wide formatter write is run, because it
  would touch files outside the declared change set.

## Cross-task Design Decisions

### D1: Normalize once at the detection-routine entry

- **Decision**: the detection routine first derives a normalized copy of its
  input in which every CR LF pair is replaced by a single LF. Boundary search
  and region extraction both operate on that normalized copy only, so every
  offset is taken from and applied to the same string (FR2, A-2).
- **Consequence**: callers keep passing raw text (the real-source test passes
  the file content as read; the red-case test passes synthetic text) and get
  line-ending independence without changes on their side. The boundary
  locator's documented offset refers to whatever text it is given — the
  normalized copy when called from the detection routine.
- **Affected tasks**: task0001, and any later rework task that touches the
  scan.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| An offset taken on the normalized text is applied to the original text (or the reverse), misplacing the scanned region on CRLF input | Low | Medium — a bare send directly above the boundary could be missed, or sends inside the `mod tests` body could be reported | task0001 pins a CRLF case whose bare send sits directly above the boundary and a CRLF case with sends only inside the `mod tests` body (TS-5) |
| A new test literal spells a forbidden shape verbatim | Low | Low — the scan ignores the `mod tests` body, so it would not fail on its own | NFR2 convention plus the whole-file static search (TS-6) |
| A real CRLF working copy cannot be exercised by unit tests running in the LF checkout | High (by construction) | Low | Synthetic CRLF and mixed cases stand in (TS-1, TS-4, TS-5); the manual check TS-3 exercises a real CRLF working copy |
| Unrelated flaky tests in the `--lib` run (tab replay tests under parallel execution, tmux socket discovery) | Medium | Low | Re-run a failing unrelated test with a single test thread before attributing the failure to this feature |

## Open Questions

- None.
