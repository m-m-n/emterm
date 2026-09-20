# Verification Document: fetch-fonts-noto-color-emoji

## Overview

**Feature**: fetch-fonts-noto-color-emoji /
**SPEC.md**: `feature-docs/fetch-fonts-noto-color-emoji/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/fetch-fonts-noto-color-emoji/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the merged feature branch.
Per-task acceptance criteria live in
`feature-docs/fetch-fonts-noto-color-emoji/tasks/task0001.md`.

Precondition for every command below: the bundled fonts have been fetched once
(`bash scripts/fetch-fonts.sh`), and every command is run from the project
root.

## Build Verification

- Command (main component, from workflow.yaml):
  `CARGO_TARGET_DIR=src-tauri/target cargo build --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors.
- Command (webview component, from workflow.yaml):
  `bun run build:viewer && bun run build:settings`
- Expected: exit code 0. This component is untouched by the feature; the run
  only confirms no collateral damage.

### Additional build commands required by the success criteria

These are NOT part of the workflow.yaml approved command set, so running them
goes through the command approval gate:

- All example targets still build:
  `CARGO_TARGET_DIR=src-tauri/target cargo build --manifest-path src-tauri/Cargo.toml --examples`
- Feature-off compile check still passes:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Feature-off test run builds and passes (the documented feature-off command is
  a check and does not build test targets, so this one is needed for NFR2):
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --no-default-features`

## Test Verification

- Command (main component, from workflow.yaml):
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
- Expected: the command reaches the test-execution stage (it currently fails at
  the compile stage, which is the reported bug), and the new asset-manifest
  check passes.
- Command (webview component, from workflow.yaml): `bun test`
- Expected: exit code 0; unaffected by this feature.
- Coverage target: none defined for this project — no coverage threshold gates
  this feature.
- Known pre-existing instability, unrelated to this feature: the terminal-tab
  replay tests are non-deterministic under parallel execution (stable with a
  single test thread) and one socket-discovery test is rarely flaky. Judge them
  against the pre-feature baseline; do not count them as regressions.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Happy path: the check enumerates the seven fonts embedded by the font resolver and finds each of them in both the fetch script and the inventory table | Pass, with no findings | Integration |
| TS2 | A font is embedded but has no fetch-script declaration | Fail, message naming the font file and the missing fetch-script declaration | Integration (synthetic declaration text) |
| TS3 | A font is embedded and fetched but has no inventory row | Fail, message naming the font file and the missing inventory row | Integration (synthetic declaration text) |
| TS4 | Clean checkout: the bundled-font directory holds no font binary | The check passes; no font binary is required to exist | Integration |
| TS5 | Self-match guard: the checking test's own source contains the token it scans for | The scan excludes its own source (or the token cannot match itself); the check still passes | Integration |
| TS6 | Build-directory exclusion: a populated build output directory is present | Identical result; build output directories contribute nothing | Integration |
| TS7 | A compile-time embed whose path is not a font is present in the crate | No finding is produced for it | Integration |
| TS8 | Regression guard: the default-feature test run produces no new failure relative to the pre-feature baseline | No new failures; the two known-flaky suites are judged against the baseline | Integration |

## Code Quality Verification

- Format (main component, from workflow.yaml): `make fmt-check`
- Format (webview component, from workflow.yaml): `bunx biome check .`
- Expected: exit code 0 for both.
- Static analysis: no separate static-analysis command is declared for this
  project; the compiler warnings surfaced by the build commands above serve
  that role.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC1 | On a checkout with no font binary present, the fetch script followed by the documented test command compiles and runs tests, with no unreadable-font error | Run the fetch script, then the main test command; inspect the output for the compile-stage error |
| SC2 | The retired probe source no longer exists | Confirm the absence of `src-tauri/examples/swash_emoji.rs` |
| SC3 | The crate manifest declares no example target for the retired probe and no `png` dev-dependency, and the `png` feature of the image dependency is unchanged | Inspect `src-tauri/Cargo.toml` and its diff |
| SC4 | The three remaining example targets still build | The all-examples build command above |
| SC5 | The new asset-manifest check passes unmodified on the current tree, for all seven fonts | TS1 |
| SC6 | Negative check on the real files: removing one fetch-script entry, and separately one inventory row, each makes the check fail naming the font | Manual items M1 and M2 |
| SC7 | The feature-off compile check still passes | The feature-off check command above |
| SC8 | The feature-off test run builds and the new check passes there | The feature-off test command above |
| SC9 | No build-related file references the retired probe — the crate manifest, the Makefile, the scripts directory, the workflow directory, the project rule documents and the project specification document. Historical task documents may keep dangling references | Manual item M3 (repository-wide text search over the listed paths) |
| SC10 | The continuous-integration workflow file is unchanged | Manual item M4 (diff of the feature branch against its base for that path) |
| SC11 | SPEC.md states both the textual-invariant limitation and the known follow-up about the two probes | Manual item M5 (document inspection) |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS8, SC2, SC9 |
| FR2 | task0001 | TS8, SC3, SC4 |
| FR3 | task0001 | TS8, SC3 |
| FR4 | task0001 | TS1, TS2, TS3, TS5 |
| FR5 | task0001 | TS1, SC8 |
| FR6 | task0001 | TS1, TS2, TS3, TS5, TS7 |
| FR7 | task0001 | SC10 (no automated scenario — coverage gap, see Notes) |
| FR8 | — (satisfied by SPEC.md text; no implementing task) | SC11 |
| FR9 | task0001 | TS8, SC3 |
| NFR1 | task0001 | TS1 |
| NFR2 | task0001 | TS1, SC8 |
| NFR3 | task0001 | TS4 |
| NFR4 | — (satisfied by SPEC.md text; no implementing task) | SC11 |
| NFR5 | task0001 | TS6 |

**Notes on gaps**: FR7 (no new continuous-integration job) is a negative
constraint verified by SC10 rather than by a test scenario. FR8 and NFR4 are
documentation requirements already discharged by SPEC.md; they have no
implementing task and are confirmed by document inspection (SC11).

## E2E Testing

No E2E framework is configured for this project (both components declare an
empty E2E command in workflow.yaml, and no E2E inputs were resolved). Nothing
in this feature is E2E-testable.

## Manual Testing (E2E Not Possible)

- [ ] M1 (SC6, TS2 on the real files): temporarily remove one fetch entry from
      `scripts/fetch-fonts.sh`, run the main test command, confirm the check
      fails and the message names that font file and the missing fetch-script
      declaration, then revert the edit. Verification-only; the edit must not
      be committed.
- [ ] M2 (SC6, TS3 on the real files): temporarily remove one inventory row
      from `src-tauri/assets/fonts/README.md`, run the main test command,
      confirm the check fails and the message names that font file and the
      missing inventory row, then revert the edit. Verification-only; the edit
      must not be committed.
- [ ] M3 (SC9): search the crate manifest, the Makefile, the scripts directory,
      the workflow directory, the project rule documents and the project
      specification document for the retired probe's name; expect no match.
      Historical task documents are excluded from this check.
- [ ] M4 (SC10): confirm that `.github/workflows/ci.yml` does not appear in the
      feature branch's diff against its base.
- [ ] M5 (SC11): confirm SPEC.md contains both the accepted-limitation section
      (textual invariant, not a compile proof) and the known-follow-up section
      (the two probes' absolute font paths and their stale run instructions).
- [ ] M6 (SC1, TS4 on a real tree): with the bundled-font directory holding no
      font binary, run the main test command and confirm the new check passes
      and no unreadable-font compile error appears.

## Performance / Security Verification

Not applicable. The feature defines no performance requirement, and the new
check performs no network access, touches no credential or authorization path,
and reads only tracked repository text.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 5 | 5 | 0 | 0 |
| Test scenarios (TS1–TS8) | 8 | 8 | 0 | 0 |
| Code quality | 2 | 2 | 0 | 0 |
| Success criteria (SC1–SC11) | 11 | 5 | 0 | 6 |
| Manual items (M1–M6) | 6 | 0 | 0 | 6 |
