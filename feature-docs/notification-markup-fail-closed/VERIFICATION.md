# Verification Document: notification-markup-fail-closed

## Overview

**Feature**: notification-markup-fail-closed /
**SPEC.md**: `feature-docs/notification-markup-fail-closed/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/notification-markup-fail-closed/IMPLEMENTATION.md`

## Build Verification

- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI feature gate): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors (both commands)

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0, all tests pass
- Coverage target: not measured — the project defines no coverage tooling in
  its approved commands; coverage is judged by the scenario table below
- Note: if the `tabs.rs` replay tests prove flaky in the full run, re-run the
  same command with the single-thread test-harness option (known
  non-deterministic parallel-run issue, per SPEC TS4)

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Capability query fails (the GetCapabilities-fails / Notify-succeeds window) | `escape_for_send` returns (escaped title, escaped body); no raw `<` / `>` / `&` survive in either field | Unit |
| TS2 | Capability query succeeds with an empty list or a list without `body-markup` | Both fields pass through byte-identical | Unit |
| TS3 | Capability query succeeds with a list containing `body-markup` | Existing 3-character escaping with `&` replaced first applied to both fields (regression-free) | Unit |
| TS4 | Whole `--lib` suite | Passes (sanitize pipeline / rate limiter / all other behavior unregressed) | Integration |
| TS5 | CLI-only feature-gate build (`--no-default-features` check command above) | Compiles with exit code 0 | Build |
| TS6 | Windows notification path unchanged | The integrated diff contains no change to the Windows notification path and nothing added outside the `#[cfg(unix)]` gate | Manual (diff inspection) |
| TS7 | Fail-closed recorded as normative | SPEC.md states fail-closed as normative and supersedes the previous feature's FR3; no doc/test comment in the modified files still describes fail-open semantics; the predicate name matches fail-closed meaning | Manual (document/code inspection) |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Static analysis: none configured in the approved project commands

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC1 | A unit test pins that `escape_for_send` escapes both title and body when the capability query fails | TS1 |
| SC2 | Success with a list omitting `body-markup` passes through unescaped (expectations maintained/updated for the new specification) | TS2 |
| SC3 | Success with a list containing `body-markup` escapes (no regression) | TS3 |
| SC4 | The `--lib` test suite passes | TS4 |
| SC5 | The SPEC states fail-closed as normative and serves as the closure basis for finding `eade9e7f97a29a29` | TS7 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1 (unit test pinning the failure path) |
| FR2 | task0001 | TS2, TS3 (both `Ok` branches regression-pinned) |
| FR3 | task0001 | TS1 (single evaluation escapes both fields in one call) |
| FR4 | task0001 | TS6 (manual diff inspection — no Windows-path change) |
| FR5 | — (satisfied by SPEC.md itself, written at create-spec) | TS7 (manual document inspection) |
| NFR1 | task0001 | TS5 (CLI gate check) + main build command |
| NFR2 | task0001 | TS7 (manual inspection — no residual fail-open wording; predicate name matches semantics) |
| NFR3 | task0001 | TS4 (full `--lib` suite, incl. sanitize / rate-limiter tests) |

## Manual Testing (E2E Not Possible)

The project defines no E2E command for this component
(`e2e_test_command` is empty), and a real D-Bus capability-query failure
cannot be arranged deterministically in an automated test.

- [ ] TS6: inspect the integrated diff — no change to the Windows
      notification path; every changed line sits inside the existing
      `#[cfg(unix)]` gate or in test/doc content.
- [ ] TS7: inspect SPEC.md (fail-closed stated as normative, superseding
      `feature-docs/notification-body-markup-escape/SPEC.md` FR3) and search
      the modified source files for residual fail-open wording.

## Performance / Security Verification

- Security (FR1, finding `eade9e7f97a29a29`): the fail-closed decision is
  pinned by TS1 — a failed capability query must never result in unescaped
  markup reaching the notification server. Covered by the unit test plus the
  review phase's security perspective.
- Performance: not applicable — no performance requirement is defined.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit tests | TS1, TS2, TS3 | 3 | 0 | 0 |
| Integration / build | TS4, TS5 | 2 | 0 | 0 |
| Inspection | TS6, TS7 | 0 | 0 | 2 |
| Total | 7 | 5 | 0 | 2 |
