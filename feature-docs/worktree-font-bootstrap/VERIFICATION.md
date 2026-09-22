# Verification Document: worktree-font-bootstrap

## Overview

**Feature**: worktree-font-bootstrap
**SPEC.md**: `feature-docs/worktree-font-bootstrap/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/worktree-font-bootstrap/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the feature. Per-task
acceptance criteria live in `feature-docs/worktree-font-bootstrap/tasks/`.

Two properties make this feature's verification unusual and shape everything
below:

1. The change under test is **build-time behavior**, not library behavior, so
   most scenarios are driven by running builds under controlled conditions and
   asserting on their output and on the filesystem — not by the crate's own
   test harness.
2. Several scenarios require a **freshly created worktree whose font directory
   is empty**. Running them inside a tree that already has the fonts silently
   passes while the defect is present, so the starting state must be
   established explicitly each time.

## Build Verification

| Component | Command | Expected |
|-----------|---------|----------|
| main (GUI) | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` | exit code 0, no errors |
| cli (no default features) | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | exit code 0, no errors, and no font check and no acquisition attempt occur |
| web | `bun run typecheck` | exit code 0 (untouched by this feature; run as a regression guard) |

## Test Verification

| Component | Command | Expected |
|-----------|---------|----------|
| main | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` | exit code 0 **and a non-zero count of executed tests** — the count is part of the assertion, because "zero tests executed" is precisely the reported defect |
| web | `bun test` | exit code 0 (untouched by this feature; run as a regression guard) |
| feature | `bash scripts/verify-font-bootstrap.sh` | exit code 0, with every scenario reporting a pass |

**Coverage target**: the project defines no numeric coverage threshold, and this
feature adds no library code to cover. Coverage is expressed instead as the
requirement coverage table below: every FR/NFR maps to at least one task and at
least one verification item.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Fresh worktree, network available: an isolated tree with an empty font directory, then the reported library-test command | The command completes and reports a non-zero number of executed tests; the seven fonts land in that tree | Integration (automated — un-fetched scenario of the verification script) |
| TS2 | Opt-out engaged: the same empty starting state, with `EMTERM_SKIP_FONT_FETCH=1` set, then a GUI build | No acquisition attempt and no network access occur; the build stops with both lines of the actionable message present verbatim; the font directory is still empty | Integration (manual command run in a fresh worktree) |
| TS3 | Fetch failure: the same empty starting state with the acquisition path's required tooling unresolvable, then a GUI build | The build stops, identifying failure shape 3 with the exit status and the acquisition script's preserved error output, followed by both lines of the actionable message; no partial or unverified file remains in the font directory | Integration (automated — fetch-failure scenario of the verification script) |
| TS4 | Already-fetched steady state: all seven fonts present and hash-matching, then a GUI build | No network access and no re-download occur; no font file is rewritten | Integration (automated — already-fetched scenario of the verification script) |
| TS5 | CLI-only build in an un-fetched tree: the `--no-default-features` check in a tree with an empty font directory | Succeeds, with no font check and no acquisition attempt reached | Integration (manual command run in a fresh worktree) |
| TS6 | Destination pinning across worktrees: two sibling worktrees, both with empty font directories, one built from an arbitrary working directory | Fonts are written only into the built worktree's own font directory; the sibling tree is byte-for-byte untouched | Integration (manual, two sibling worktrees) |
| TS7 | CI job runs and catches the regression: a push or pull request against the feature branch | The new job runs on the same triggers as the bun job, from an isolated un-fetched state with no font-cache restore, and passes; reverting the build-script change makes it fail | Integration (CI run on the feature branch) |
| TS8 | No rebuild churn: a tree where the fetch ran during a build, then the same cargo command again with no source change | The build script does not re-run, no further acquisition occurs, and the build converges instead of looping | Integration (manual, build twice in a fresh worktree) |
| TS9 | Scope boundary held: review of the change to the build script | `viewer/dist`, `settings/dist` and the Windows icon-resource handling are unchanged; the seven-path set and the position of the GUI feature gate are unchanged | Inspection (diff review) |
| TS10 | Non-hermeticity stated openly: review of SPEC.md | The accepted trade-off and its four mitigations (HTTPS-only, per-file SHA256 pinning, idempotent skip, explicit opt-out) are stated in plain text | Inspection (document review) |
| TS11 | Release determinism preserved: review of `.github/workflows/release.yml` and of the opt-out's availability | The explicit acquisition step and the font cache are unchanged, so the automatic path is a no-op there; a packaging build can engage the opt-out to forbid an implicit fetch | Inspection (diff review) + the TS2 result |

### Failure-shape coverage (IMPLEMENTATION.md D8)

The four failure shapes must remain distinguishable in the build's output.
TS2 and TS3 cover the opt-out stop and shape 3 respectively; the remaining
shapes are checked at the integrated level:

- [ ] Shape 1 — the interpreter cannot be launched: the build stops carrying the
      launch error, both lines of the actionable message, and the additional
      line stating that bash must be made available first. Staged by driving a
      GUI build in a fresh tree with an environment whose `PATH` cannot resolve
      the interpreter.
- [ ] Shape 2 — another launch error: the build stops carrying the underlying
      launch error and both lines of the actionable message.
- [ ] Shape 4 — the acquisition exited zero but a font is still missing: the
      build stops naming the still-absent path, with both lines of the
      actionable message.

## Code Quality Verification

- Format: `make fmt-check` — exit code 0.
- Shell scripts: the new verification script is free of unquoted expansions in
  paths it constructs, and fails fast on error rather than continuing past a
  failed step. Verified by review; a shell linter may be used if available, but
  none is configured in the project today.
- Workflow file: `.github/workflows/ci.yml` parses as a valid workflow document
  after the change.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | Fresh worktree + network: the reported command completes with a non-zero executed-test count | TS1 |
| AC2 | The same run leaves the seven fonts in that worktree, and no font file is written outside it | TS1 for the placement; TS6 for the containment |
| AC3 | Opt-out engaged + fonts missing: the build stops with no network access, output containing both message lines | TS2 |
| AC4 | Fetch failure: the build stops with the same message and no partial or unverified file remains | TS3 |
| AC5 | All seven present and hash-matching: no network access, no re-download | TS4 |
| AC6 | The `--no-default-features` check succeeds in a fresh un-fetched worktree, triggering neither the font check nor a fetch | TS5 |
| AC7 | The bootstrap-verification script exists, exercises the three paths, and fails when the defect is reintroduced | TS1, TS3, TS4 for the paths; TS7 for the detection |
| AC8 | `ci.yml` has a job invoking that script on the same triggers as the bun job, with no font-cache restore, and it passes on the feature branch | TS7 |
| AC9 | The SPEC states the non-hermeticity trade-off and its mitigations in plain text | TS10 |
| AC10 | No change to `viewer/dist`, `settings/dist` or `icons/` handling in the build script | TS9 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS5 |
| FR2 | task0001 | TS1, TS6 |
| FR3 | task0001 | TS2 |
| FR4 | task0001 | TS2, TS3 |
| FR5 | task0001 | TS1, TS3 |
| FR6 | task0002 | TS1, TS3, TS4, TS7 |
| FR7 | task0003 | TS7 |
| FR8 | task0001 | TS9 |
| NFR1 | task0001 | TS10 |
| NFR2 | task0001 | TS4 |
| NFR3 | task0001 | TS3 |
| NFR4 | task0001 | TS6 |
| NFR5 | task0001 | TS5 |
| NFR6 | task0001 | TS8 |
| NFR7 | task0001 | TS11 |

## E2E Testing

The project configures no E2E framework for any component (every
`e2e_test_command` in `workflow.yaml` is empty), so there is no E2E suite to
extend. The closest equivalent — a full end-to-end reproduction of the reported
situation — is `bash scripts/verify-font-bootstrap.sh`, which is listed under
Test Verification above and runs in CI per TS7.

## Manual Testing (E2E Not Possible)

Each item below needs a starting state (a freshly created worktree, two sibling
worktrees, or a real CI run) that the automated scenarios cannot establish from
inside an existing checkout.

- [ ] TS2 — In a freshly created worktree with an empty font directory, run a
      GUI build with `EMTERM_SKIP_FONT_FETCH=1`. Confirm the build stops, that
      both lines of the actionable message appear verbatim, that the font
      directory is still empty, and that no acquisition attempt was made.
- [ ] TS5 — In the same tree, run the CLI-only check. Confirm it succeeds and
      that neither the font check nor an acquisition attempt occurred.
- [ ] TS6 — Create two sibling worktrees, both with empty font directories.
      Build one of them from an arbitrary working directory outside both trees.
      Confirm the fonts land only in the built tree and that the sibling tree
      is untouched.
- [ ] TS8 — In a tree where the fetch ran during a build, run the same cargo
      command again with no source change. Confirm the build script does not
      re-run and no further acquisition occurs. Then change the opt-out
      switch's value and confirm the build script does re-run.
- [ ] TS7 — Observe an actual CI run on the feature branch: the new job runs on
      the same triggers as the bun job, restores no font cache, and passes.
      Then confirm it fails on a tree where the build-script change is reverted.
- [ ] D8 shapes 1, 2 and 4 — stage each as described under Failure-shape
      coverage and confirm the diagnostic context and the actionable message.

No mockup comparison item applies: the design step was skipped and this feature
has no visual surface.

## Performance / Security Verification

Performance: the SPEC specifies no performance target. One operational
expectation is recorded instead — the steady-state build must not pay any
bootstrap cost (TS4), and the CI job's duration must stay within its declared
time limit (TS7).

Security:

- Transport — the acquisition path uses HTTPS only and never disables
  certificate verification. Confirmed by TS9-style inspection that
  `scripts/fetch-fonts.sh` is unmodified, plus review of the invocation for any
  added flag or environment that could weaken it.
- Integrity — per-file SHA256 verification precedes the atomic move; no
  unverified or partial file is ever left in the font directory (TS3).
- Network exposure — no network access at all when the seven fonts are present
  and hash-matching (TS4), and none at all when the opt-out is engaged (TS2).
- Write containment — writes reach only the building worktree's own font
  directory and the acquisition script's staging location (TS6).
- Supply chain — exactly one acquisition mechanism exists; no task added a
  second one, and the fetch-failure scenario induces failure by environment
  manipulation rather than by substituting a script (TS3, plus review of the
  verification script against IMPLEMENTATION.md's single-acquisition-path rule).
- Non-hermeticity — accepted deliberately and stated in the SPEC (TS10); the
  four mitigations above are what bound it.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build verification | 3 | 3 | 0 | 0 |
| Test verification | 3 | 3 | 0 | 0 |
| Test scenarios (TS1–TS11) | 11 | 3 (TS1, TS3, TS4) | 0 | 8 (TS2, TS5, TS6, TS7, TS8 manual; TS9, TS10, TS11 inspection) |
| Failure-shape coverage | 3 | 0 | 0 | 3 |
| Code quality | 3 | 2 | 0 | 1 |
| Security checks | 6 | 3 | 0 | 3 |
