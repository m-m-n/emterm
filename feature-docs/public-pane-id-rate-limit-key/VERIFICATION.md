# Verification Document: public-pane-id-rate-limit-key

## Overview

**Feature**: public-pane-id-rate-limit-key /
**SPEC.md**: `feature-docs/public-pane-id-rate-limit-key/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/public-pane-id-rate-limit-key/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the merged feature.
Task-level acceptance criteria live in
`feature-docs/public-pane-id-rate-limit-key/tasks/task0001.md`.

## Build Verification

Commands are taken verbatim from `workflow.yaml` `project.components`. Run
every command from the project root.

| Component | Command | Expected |
|-----------|---------|----------|
| main (GUI, default features) | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` | exit code 0, no errors |
| cli (CLI-only feature gate) | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | exit code 0, no errors — this is the NFR4 check |
| webviews | `bun run typecheck` | exit code 0. No TypeScript file is in the change set, so this is a no-regression check only |

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command (webviews): `bun test` — no-regression check only; no TypeScript is touched.
- Coverage target: no coverage tool is configured in this project, so no
  percentage target is enforced. Coverage is judged by the requirement-to-
  scenario mapping below instead.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Derivation over a hand-built learned-id map: learned pane, unlearned pane, plain tab | The learned pane derives the namespaced learned form; the unlearned pane still derives the scope-qualified fallback form; the plain tab still derives the tab form (FR1, FR2, FR3, NFR1, AC-1) | Unit |
| TS-2 | A learned id equal to a plain tab's key | The derived key differs from the key that plain tab derives; it carries the learned-id namespace prefix (FR1, AC-2) | Unit |
| TS-3 | A learned id equal to an unlearned pane's fallback key | The derived key differs from the key that unlearned pane derives, so the reserved fallback form is unreachable from a daemon string (FR1, FR2, AC-3) | Unit |
| TS-4 | Two mux connections driven end-to-end through the real message path, each learning a public id for its own pane | The two derived keys differ; the public-id accessor still returns each bare daemon string; expected strings are built from ids observed at runtime (FR1, NFR3, AC-4) | Integration |
| TS-5 | Arm and discard through the real call sites: transition, suppressed re-fire, pane close, transition again | The second transition inside the window is suppressed; after the pane closes the next transition fires again; no literal key string is asserted (FR4, NFR1, AC-5) | Integration |
| TS-6 | Ingest of a public id that fails the mux protocol's own parse | The public-id accessor returns the exact daemon string; no parse call and no rejection path exists on the ingest path; the five unparseable fixtures are intact (FR6, NFR3, AC-6) | Integration |
| TS-7 | Review-verified: both doc comments, plus the CLI-only feature gate | Both comments describe the post-change behaviour and neither claims that the mux prefix protects the learned-id branch; the CLI-only check compiles (FR5, NFR4, NFR5, AC-7) | Review + build command |

## Code Quality Verification

- Format: no `format_command` is configured for any component in
  `workflow.yaml`, so no formatter is run as part of verification.
- Static analysis: none configured beyond the compiler's own diagnostics; the
  two build commands above must produce no warnings introduced by this change.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SUC-1 | All functional requirements FR1-FR6 are implemented and tested | Requirement coverage table below; TS-1 through TS-7 |
| SUC-2 | All test scenarios TS-1 through TS-6 pass, and TS-7 is confirmed by review | Test command exit code 0; review record for TS-7 |
| SUC-3 | Performance meets NFR2 — derivation stays constant-time with at most one additional string allocation, and stays off the render path | Review of the derivation's shape; it is called once per drained transition and once per discarded pane, never per frame |
| SUC-4 | Security requirements SC-1, SC-2 and SC-3 are satisfied | TS-2 and TS-3 prove one pane cannot name another's bucket; review confirms the key is never logged as a probe-able identifier and never rendered |
| SUC-5 | Documentation is complete: both doc comments corrected and asserting no collision property the code does not implement | TS-7 (review) |
| SUC-6 | Code review is completed, including the review-verified TS-7 items | Review phase record |
| SUC-7 | The CLI-only build still compiles | The `--no-default-features` check above |
| SUC-8 | The public-id accessor and the other unaffected surfaces return today's values | TS-4, TS-6, plus the full test command showing no other scenario regressed |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-3, TS-4 |
| FR2 | task0001 | TS-1, TS-3 |
| FR3 | task0001 | TS-1 |
| FR4 | task0001 | TS-5 |
| FR5 | task0001 | TS-7 (review) |
| FR6 | task0001 | TS-6 |
| NFR1 | task0001 | No automated scenario — an absence property (never serialized, never persisted, never displayed). Confirmed by review, supported by TS-1 and TS-5, which show the key exists only inside the process |
| NFR2 | task0001 | No automated scenario — no load or stress test is proposed. Confirmed by review of the derivation's shape (SUC-3) |
| NFR3 | task0001 | TS-4, TS-6, plus the full test command: no scenario outside this feature's own updates may change its result |
| NFR4 | task0001 | TS-7 — the CLI-only feature check |
| NFR5 | task0001 | TS-7 — review of both corrected comments |

## E2E Testing

The project has no E2E infrastructure and no `e2e_test_command` is configured
for any component, so no automated E2E scenario is proposed.

## Manual Testing (E2E Not Possible)

- [ ] MT-1 (optional, adversarial): with a modified mux daemon that reports a
      `public_pane_id` equal to a victim tab's key, attach in one tab, use a
      plain tab as the victim, and fire repeated agent notifications from the
      attacking pane. The victim tab's notifications must NOT be suppressed,
      and closing the attacking pane must NOT clear the victim's rate-limit
      state. Requires a purpose-built daemon; TS-2 and TS-3 are the automated
      stand-in for it and are what the feature is gated on.
- [ ] MT-2: attach to a normal mux daemon and confirm the mux sidebar still
      displays the same public pane ids as before the change (NFR3, AC-6).
- [ ] MT-3: confirm normal agent-status notifications still rate-limit per
      pane — a second transition inside the window is suppressed, and after the
      pane closes a new transition notifies again (FR4, AC-5).

No mockup comparison item applies: the design step is `skipped` and this
feature has no visual surface.

## Performance / Security Verification

- NFR2: the derivation stays constant-time per call with at most one
  additional string allocation relative to today's cloned learned id, and runs
  once per drained transition and once per discarded pane — never per frame.
  Verified by review, not by measurement.
- SC-1 / SC-2: the daemon is outside the trust boundary; the shared rate-limit
  key space must stay partitioned so one pane can neither consume nor clear
  another pane's bucket. Verified by TS-2 and TS-3.
- SC-3: the derived key introduces no new disclosure or resource path — it is
  never logged as an identifier a daemon could use to probe other tabs, and is
  never rendered. Verified by review.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 3 | 3 | 0 | 0 |
| Test scenarios | 7 | 6 | 0 | 1 (TS-7, review-verified) |
| Success criteria | 8 | 5 | 0 | 3 (review-confirmed) |
| Requirements | 11 | 8 | 0 | 3 (NFR1, NFR2, NFR5 — review-confirmed) |
| Manual checks | 3 | 0 | 0 | 3 |
