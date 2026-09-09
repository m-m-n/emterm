# Verification Document: mux-rate-limit-key-pane-identity

## Overview

**Feature**: mux-rate-limit-key-pane-identity /
**SPEC.md**: `feature-docs/mux-rate-limit-key-pane-identity/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/mux-rate-limit-key-pane-identity/IMPLEMENTATION.md`

This document is the INTEGRATED verification for the feature. Per-task
acceptance criteria live in `tasks/task0001.md` and `tasks/task0002.md`.

## Build Verification

| Component | Command | Expected |
|---|---|---|
| main | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` | exit code 0, no errors |
| cli_only | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | exit code 0, no errors (NFR4: the feature split is unchanged) |

The main build is also the mechanical check for the "no missed call site"
property: the derivation's narrowed parameter list makes any un-updated call
site a compile error (IMPLEMENTATION.md CD-2, assumption AS-7).

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: no numeric target is configured for this project. The
  coverage obligation here is by scenario: every TS below must exist and pass,
  and the count of derivation-focused tests must not decrease relative to the
  pre-feature suite (a premise-removed test is restated, never deleted).
- The `cli_only` component defines no test command; it is verified by its build
  command alone.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Derivation for a learned pane, an unlearned pane, a tab, and a second scope's same-numbered pane | The code-owned mux form in all three mux cases and the tab form for the tab; the two scopes' same-numbered panes differ | Unit |
| TS-2 | A learned value byte-identical to a tab's key is seeded, then the mux pane's key is derived | The mux pane's key is unchanged and still differs from the tab's key | Unit |
| TS-3 | A learned value byte-identical to another pane's key is seeded, then that other pane's key is derived | The derived key is unchanged; the seeded value cannot reach it | Unit |
| TS-4 | Per-connection scoping of the learned-id map and of the derived keys | Both scopes derive the code-owned form for their own pane, the two keys differ, and the accessor still returns each daemon-supplied identifier verbatim | Unit |
| TS-5 | Detach releases the model entry, the learned identifier and the rate-limit identity, probed with the real derived key | Pre-detach the window is armed; post-detach it is released | Integration |
| TS-6 | Detach on one tab, then compare a second tab's identically-numbered pane's key before and after | The key is identical before and after | Integration |
| TS-7 | Tab close, exited-tab reap and mux-pane exit each discard the closed pane's rate-limit state | The closed pane's key is absent afterwards, so an immediate re-report fires | Integration |
| TS-8 | The header comment of the key-format-independence test cites the mux-shaped key | The example cites the code-owned mux form, not a daemon identifier | Unit |
| TS-9 | Record one key, then record a second key a full cooldown period later | The first key is absent; the second is present and throttled | Unit |
| TS-10 | Repeated read-only checks far past the cooldown, plus a record inside the window with an unexpired sibling present | The limiter's contents are unchanged by the reads; the unexpired sibling survives the record with its original instant | Unit |
| TS-11 | Close-time discard of a key that has fired (existing test, unmodified) | The key is absent afterwards; the test passes without edits | Unit |

## Code Quality Verification

- Format: no format command is configured for either component in
  `workflow.yaml`. Follow the surrounding file's existing style; do not run a
  crate-wide reformat.
- Static analysis: no static-analysis command is configured. The compiler's own
  warnings on the two build commands above are the floor — a new warning
  introduced by this feature is a finding.
- Retired-prefix sweep: no file under `src-tauri/src/` may emit, assert, or
  describe the retired `muxpub:` prefix. Verified by searching the tree for that
  literal and finding no occurrence.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | Every mux pane derives the code-owned form, learned or not | TS-1, TS-4 |
| AC-2 | A tab pane still derives the tab form | TS-1 |
| AC-3 | No daemon-supplied string can appear in any key; the derivation does not accept the learned-id map | The main build command succeeds with no call site passing the map (compile-time property); TS-2 and TS-3 guard the consequence |
| AC-4 | Same wire pane id under two connections derives different keys; a byte-identical learned value cannot reach another key | TS-1, TS-2, TS-3, TS-4 |
| AC-5 | A pane's key is identical before and after the daemon re-mints its identifier | TS-6, plus TS-1's learned/unlearned equality |
| AC-6 | A record at or past the cooldown evicts an expired key; a record inside the window leaves an unexpired key untouched | TS-9, TS-10 |
| AC-7 | A suppressed attempt neither refreshes nor evicts anything | TS-10 |
| AC-8 | Tab close, tab reap and mux-pane exit each leave the closed key absent | TS-5, TS-7, TS-11 |
| AC-9 | The sidebar accessor still returns the learned string verbatim, or nothing | TS-4, plus manual item MV-1 |
| AC-10 | No source file emits or asserts the retired prefix, and the library test command passes | Retired-prefix sweep above, plus the test command |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-3 |
| FR2 | task0001 | TS-6, TS-7, plus the main build command (a stale call site cannot compile) |
| FR3 | task0001 | TS-1, TS-6 |
| FR4 | task0001 | TS-4 |
| FR5 | task0001 | TS-7, plus the retired-ordering comment check in the review phase |
| FR6 | task0002 | TS-9, TS-10 |
| FR7 | task0001, task0002 | TS-7, TS-11 |
| FR8 | task0001 | TS-8, plus doc-comment review of the derivation and the limiter field |
| FR9 | task0001 | TS-1, TS-2, TS-3, TS-4, TS-6 |
| FR10 | task0001 | TS-5 |
| NFR1 | task0002 | TS-10 |
| NFR2 | task0001 | No test scenario (declared gap): verified by inspection that a key is never serialized, never sent over the mux wire and never persisted |
| NFR3 | task0002 | No test scenario (by design): records accepted behaviour — lazy pruning and unshrunk capacity |
| NFR4 | task0001, task0002 | Both build commands above; no new dependency and no new public item |

## E2E Testing

**Existing E2E framework**: none. `e2e_test_command` is empty for both
components, so the two command checks below stand in for an E2E gate.

- [ ] The library test command passes on the integrated result.
- [ ] The CLI-only build command passes on the integrated result.

## Manual Testing (E2E Not Possible)

- [ ] MV-1: With a mux pane attached, the sidebar's copy-to-clipboard row still
      offers the daemon-supplied identifier for a learned pane, and offers
      nothing for an unlearned or released one. Human check of the rendered row;
      the accessor itself is covered by TS-4.
- [ ] MV-2 (optional, needs an instrumented daemon): attach a daemon that
      returns an empty identifier for two panes of one connection, fire an agent
      notification on the first pane, then transition the second. The second
      notification fires rather than being suppressed, and closing the first
      pane does not clear the second's throttle state. This reproduces the
      original report; after this feature it is structurally impossible, so a
      failure here means the derivation regressed.
- [ ] MV-3 (optional, needs an instrumented daemon): rotate one pane's
      identifier on every status update and confirm the pane's notification
      cooldown still applies — a fresh bucket is never minted.

## Performance / Security Verification

- NFR1 (decision neutrality): the eviction predicate uses the same threshold and
  the same comparison as the read-only check. Verified by TS-10 and by reading
  the predicate against the read check side by side during review.
- NFR3 (accepted limits): lazy pruning and unshrunk capacity are accepted. No
  measurement is required; a change that works around either is out of scope.
- Trust boundary: no daemon-supplied byte reaches a rate-limit key. Verified as
  a compile-time property (the derivation's parameter list) rather than a
  runtime check — see AC-3 above.
- Unbounded growth: expiry eviction bounds live entries (TS-9) while close-time
  discard keeps a promptly-reused key from inheriting a cooldown (TS-11).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios | 11 | 11 | 0 | 0 |
| Success criteria | 10 | 10 | 0 | 1 (MV-1 supplements AC-9) |
| Requirements | 14 | 11 | 0 | 0 |
| Manual scenarios | 3 | 0 | 0 | 3 (MV-2 and MV-3 optional) |
