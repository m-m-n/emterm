# Verification Document: bun-install-reproducibility

## Overview

**Feature**: bun-install-reproducibility /
**SPEC.md**: `feature-docs/bun-install-reproducibility/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/bun-install-reproducibility/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the whole feature. Per-task
acceptance criteria live in the task plans and are not repeated here.

All commands below run from the project root. Every install-dependent step must
be run after a frozen-lockfile install, so that what is verified is the frozen
graph rather than whatever the local dependency directory happens to hold.

## Build Verification

- Command (web component): `bun run build:viewer && bun run build:settings`
- Command (types component): `bun run typecheck`
- Expected: exit code 0, no errors; the viewer and settings bundle outputs the
  Rust GUI build embeds are produced.

## Test Verification

- Command: `bun test`
- Command (targeted): `bun test src-tauri/viewer/web/entry.test.ts`
- Expected: the viewer entry file reports 14 pass / 0 fail; the full suite shows
  no failing test other than the two pre-existing marketplace version
  regression guard failures, which also fail on the base branch and are out of
  scope (ASM-4).
- Coverage target: not applicable — this project defines no coverage threshold,
  and this feature adds none.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Clean-worktree reproduction: create a fresh worktree, install, then run the viewer entry file | 14 pass / 0 fail — the path that fails today | Integration |
| TS-2 | Resolution determinism: install the same commit into two separate fresh worktrees and compare the resolved dompurify version | The two version strings are identical | Integration |
| TS-3 | Frozen-lockfile guard: mutate a dependency range without regenerating the lockfile, then run a frozen-lockfile install | Non-zero exit | Integration |
| TS-4 | Heading sanitization regression: the existing viewer entry heading assertions against the locked dompurify version, in both the main worktree and a clean worktree | Both assertions pass in both worktrees, with matcher and selector unchanged | Unit |
| TS-5 | Sanitization strictness: existing coverage of the renderer's forbidden-tag and forbidden-attribute behavior after any change made for the adoption decision | Still passes; no widened tag, attribute or URI surface | Unit |
| TS-6 | Full suite baseline: run the test suite and the typecheck from a clean, locked install | Only the two known out-of-scope marketplace-guard failures remain; typecheck exits 0 | Integration |
| TS-7 | Bundle build: build the viewer and settings bundles from the clean, locked install | Both succeed and produce the embedded assets | Integration |
| TS-8 | CI end-to-end: the CI run itself shows the clean install and test steps executing, with the viewer entry tests in the output | The run shows both steps and the entry test output | Manual |
| TS-9 | Lockfile is not ignored: check the ignore rules for the lockfile and its tracking state | No matching ignore rule; the lockfile is tracked, not untracked | Integration |

## Code Quality Verification

- Format / static analysis: `bunx biome check .`
- Expected: exit code 0. Files this feature adds under the project's TypeScript
  scope are formatted by the same rules as the rest of the repository.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | Fresh worktree: install then run the viewer entry file gives 14 pass / 0 fail | TS-1 |
| AC-2 | The lockfile is tracked and no ignore rule matches it | TS-9 |
| AC-3 | The npm lockfile is absent from the working tree and the index | Inspect the working tree and the committed index at the integration commit |
| AC-4 | Two clean installs of the same commit resolve dompurify identically | TS-2 |
| AC-5 | The heading-loss mechanism is recorded with a reproducible observation naming one of the three candidate layers | Read `doc/dompurify-h1-sanitization.md` and re-run its stated procedure |
| AC-6 | The two viewer entry heading assertions are unchanged, matcher and selector included | Diff `src-tauri/viewer/web/entry.test.ts` against the feature base commit |
| AC-7 | A CI workflow runs the test suite on a clean checkout after a frozen-lockfile install, covering the viewer entry file | TS-8, plus the workflow-structure test added by task0003 |
| AC-8 | A declaration edit committed without a regenerated lockfile fails the CI install step | TS-3, plus the workflow-structure assertion that CI uses the frozen install form |
| AC-9 | The sanitizer config gains no allowed tag or attribute and loses no forbid entry | Diff the sanitizer configuration against the feature base commit |
| AC-10 | The full suite shows no newly failing test beyond the two known marketplace-guard failures | TS-6 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0002 | TS-9, TS-1 |
| FR2 | task0002 | TS-1 |
| FR3 | task0001, task0002 | TS-1, TS-4 |
| FR4 | task0001 | TS-4 |
| FR5 | task0001, task0002 | TS-4, TS-5 |
| FR6 | task0001 | TS-4 |
| FR7 | task0003 | TS-8 |
| FR8 | task0002, task0003 | TS-3 |
| NFR1 | task0001 | TS-5 |
| NFR2 | task0001 | TS-6 |
| NFR3 | task0003 | TS-7 |
| NFR4 | task0002 | TS-2 |

## E2E Testing

No E2E framework is configured for this project (the dispatch's resolved E2E
input set is empty, and the web component declares no E2E command). The
CI-level end-to-end observation is handled as a manual item below.

## Manual Testing (E2E Not Possible)

- [ ] TS-8: Observe an actual CI run on the integration branch. Confirm the
      run performs a clean checkout, performs a frozen-lockfile install, runs
      the test suite, and that the viewer entry tests appear in its output.
- [ ] Confirm the enumeration outcome recorded by task0003 matches the workflow
      directory as it now stands: exactly one workflow runs the test suite, and
      no duplicate job was introduced.
- [ ] Read `doc/dompurify-h1-sanitization.md` and re-run its observation
      procedure once by hand, confirming the recorded outputs are reproduced
      and the named layer is the one the observation actually implicates.

## Performance / Security Verification

- NFR1 (sanitization strictness): diff the sanitizer configuration against the
  feature base commit. Any newly allowed tag or attribute, any removed forbid
  entry, or any widened URI pattern fails verification, regardless of the test
  results.
- Supply chain: confirm exactly one JavaScript lockfile exists at the
  repository root, that it is tracked, and that the CI install path is frozen
  on every workflow — an unfrozen install path leaves an unreviewed graph
  reachable.
- License: confirm the adopted dompurify version's declared license, as
  recorded in the findings document from that version's own package metadata,
  is compatible with the project's MIT license. No new dependency is introduced
  by this feature.
- Performance: not applicable — this feature declares no performance
  requirement.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios | 9 | 8 | 0 | 1 |
| Code quality | 1 | 1 | 0 | 0 |
| Success criteria | 10 | 8 | 0 | 2 |
| Security / supply chain / license | 3 | 1 | 0 | 2 |
