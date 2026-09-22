# Feature: verify-font-bootstrap-classification

## Overview

`scripts/verify-font-bootstrap.sh`'s un-fetched scenario currently decides its verdict from the `cargo test` exit status alone, so "the build stopped before any test ran" and "the library test suite executed and some test failed" are reported identically. This feature makes the scenario evaluate the executed-test count first and classify the two situations separately, aligning its PASS condition with worktree-font-bootstrap AC1/FR6 — a non-zero executed-test count — and pins the new behaviour with an automated regression test.

Requirements source: `feature-docs/verify-font-bootstrap-classification/REQUIREMENTS.md`.

## Objectives

- The un-fetched scenario of `scripts/verify-font-bootstrap.sh` distinguishes "the build stopped before any test ran" from "the library test suite executed and some test failed", so the reported verdict states what actually happened.
- The un-fetched scenario's PASS condition matches worktree-font-bootstrap AC1/FR6 exactly — a non-zero executed-test count — instead of additionally demanding that every test pass.
- The verify-font-bootstrap CI job stops going red for reasons unrelated to font bootstrapping, in particular the project's known parallelism-dependent test flakes.
- The new classification behaviour is pinned by an automated regression test and recorded under `test-docs/`.

## User Stories

### US1: Read a truthful un-fetched verdict
As a developer of this repository, I want the un-fetched scenario's verdict to name what actually happened, so that I can tell a build stop from a test failure without re-reading the raw log.

**Acceptance Criteria:**
- [ ] AC1: Given a run where cargo executed tests and at least one failed (status != 0, executed > 0), the un-fetched scenario reports a warning whose wording names a test failure, and does not report "the build script stopped the build".
- [ ] AC2: Given a run where the build stopped before any test ran (status != 0, executed == 0), the un-fetched scenario reports FAIL with wording that names a build stop and the observed exit status.
- [ ] AC3: No verdict message contains a hardcoded test count; the count shown is the value derived from the captured log.
- [ ] AC5: Given status == 0 and executed == 0, the scenario still reports FAIL.

### US2: A CI job that only goes red for font-bootstrap reasons
As a developer of this repository, I want the verify-font-bootstrap job to stay green when the failure is an unrelated test flake, so that a red job means a real font-bootstrap regression.

**Acceptance Criteria:**
- [ ] AC4: Given the AC1 situation and both other scenarios passing, the script's summary reports every scenario as passed and the script exits 0.
- [ ] AC6: The un-fetched scenario's cargo invocation is byte-for-byte the literal reproduction command, with no added flag, and the CI step's command string is unchanged.
- [ ] AC7: An automated regression test covers AC1, AC2, AC3 and AC5, is part of the default `bun test` suite, and its record exists under `test-docs/verify-font-bootstrap-classification/`.

## Technical Requirements

### Functional Requirements

- **FR1 — Executed-count is evaluated first:** `scenario_unfetched` evaluates the executed-test count before the exit status, so the executed count — not the exit status — selects the branch that reports the verdict.
- **FR2 — Build-stop classification:** When `executed == 0` and `status != 0`, the scenario reports FAIL with wording that names a build stop and the observed exit status.
- **FR3 — Test-failure classification:** When `executed > 0` and `status != 0`, the scenario satisfies AC1's non-zero-executed-count condition: it does not report FAIL. It emits a warning, on its own `verify_font_bootstrap.`-prefixed line, whose wording is distinct from FR2's and which names both the exit status and the observed executed count.
- **FR4 — Zero-executed-with-success stays a failure:** When `status == 0` and `executed == 0`, the scenario continues to report FAIL (`command exited 0 but 0 tests executed`) — unchanged behaviour.
- **FR5 — No hardcoded count in any message:** No verdict message states a test count it did not observe; the existing literal `0 tests executed` in the build-stop message is replaced by the value `count_executed_tests` returned.
- **FR6 — Aggregate outcome of a warned scenario:** A warned un-fetched scenario counts toward `RESULTS_PASS`, so the script's aggregate exit status is 0 when the other two scenarios pass.
- **FR7 — The literal reproduction command is preserved:** The un-fetched scenario keeps invoking the literal reproduction command worktree-font-bootstrap FR6/AC1 pins — `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` — with no added cargo or test-harness flag, in particular no `-- --test-threads=1`. Flake absorption is achieved by FR3's classification, not by changing the command.
- **FR8 — CI call site unchanged:** `.github/workflows/ci.yml`'s verify-font-bootstrap job keeps invoking `bash scripts/verify-font-bootstrap.sh` with no arguments, no `actions/cache` step, no `fetch-fonts.sh` reference and no `EMTERM_SKIP_FONT_FETCH`.
- **FR9 — Automated regression test:** An automated test pins both classification branches (`executed == 0` with a non-zero status, and `executed > 0` with a non-zero status), and its record lands under `test-docs/verify-font-bootstrap-classification/`.
- **FR10 — Scope boundary:** The change is limited to `scenario_unfetched`'s classification, the CI job's continued wiring, and the new regression test. The fetch-failure and already-fetched scenarios, the cleanup trap, `count_executed_tests`' counting rule, `feature-docs/worktree-font-bootstrap/SPEC.md` and `.github/workflows/release.yml` are unchanged.

### Non-Functional Requirements

- **NFR1 — Test isolation:** The regression test runs inside the existing `bun test` suite without network access and without a real cargo build — the bun CI job installs no Rust toolchain.
- **NFR2 — Scenario independence:** The script keeps `set -uo pipefail` without `set -e`, so one scenario's failure never prevents the others from running and reporting.
- **NFR3 — Cleanup guarantee:** No scenario worktree, no stale `.git/worktrees` entry and no scratch directory survives the run, on success or failure.
- **NFR4 — No regression in existing assertions:** Every assertion already in `ci-workflows.test.ts` keeps passing, including the pinned `release.yml` sha256 and the exact CI run string.
- **NFR5 — Warned runs stay visible:** A warned-but-passing run is visibly distinct from a clean pass in the CI log, so a genuinely failing test is not silently normalized away.

## Implementation Approach

### Architecture

**System Architecture:**

```
┌─────────────────────────────────────────────────┐
│  .github/workflows/ci.yml                       │
│  verify-font-bootstrap job                      │
│  run: bash scripts/verify-font-bootstrap.sh     │  ← unchanged (FR8)
├─────────────────────────────────────────────────┤
│  scripts/verify-font-bootstrap.sh               │
│   ├─ scenario_fetch_failure   (cargo check)     │  ← unchanged (FR10)
│   ├─ scenario_unfetched       (cargo test)      │  ← classification changes here
│   ├─ scenario_already_fetched (cargo check)     │  ← unchanged (FR10)
│   ├─ count_executed_tests                       │  ← counting rule unchanged (FR10)
│   └─ cleanup trap                               │  ← unchanged (FR10, NFR3)
├─────────────────────────────────────────────────┤
│  bun test suite                                 │
│   └─ regression test (synthetic logs, FR9)      │
└─────────────────────────────────────────────────┘
```

**Component Diagram:**

```
scenario_unfetched
  ├─ runs the literal reproduction command  → captured log + exit status
  ├─ count_executed_tests(log)              → executed
  └─ classification (evaluates `executed` first, then `status`)
        ├─ executed == 0 && status != 0 → FAIL  "build stop" + observed status   (FR2)
        ├─ executed >  0 && status != 0 → WARN  distinct wording + status +
        │                                        observed executed count         (FR3)
        ├─ executed == 0 && status == 0 → FAIL  "command exited 0 but
        │                                        0 tests executed"               (FR4)
        └─ executed >  0 && status == 0 → PASS
```

### Data Flow

```
cargo test (literal reproduction command)
    → captured log + exit status
    → count_executed_tests → executed count
    → classification branch selected by `executed` (FR1)
    → verdict line(s) on `verify_font_bootstrap.`-prefixed output
    → RESULTS_PASS / RESULTS_FAIL tally
    → summary line + aggregate exit status
```

### API Design

Not applicable — this feature exposes no API. Its externally observable surface is the script's stdout verdict lines and its aggregate exit status, specified by FR2–FR6.

### Database Schema

Not applicable — this feature stores no data.

### Dependencies

**Internal Dependencies:**
- `scripts/verify-font-bootstrap.sh`: the script whose `scenario_unfetched` classification changes.
- `.github/workflows/ci.yml`: the verify-font-bootstrap job; its wiring stays as it is (FR8).
- `feature-docs/worktree-font-bootstrap/SPEC.md`: source of the AC1/FR6 reading this feature aligns with; not edited by this feature.
- `ci-workflows.test.ts`: existing assertions that must keep passing (NFR4).

**External Dependencies:**
- Bun test runner: runs the regression test as part of the default `bun test` suite (FR9, NFR1).

### File Structure

```
scripts/
└── verify-font-bootstrap.sh          # scenario_unfetched classification (FR1–FR7)
.github/workflows/
└── ci.yml                            # verify-font-bootstrap job, unchanged wiring (FR8)
test-docs/verify-font-bootstrap-classification/
└── {T}.tests.yaml                    # regression test record (FR9)
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/verify-font-bootstrap-classification/**`
- `test-docs/verify-font-bootstrap-classification/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/{feature}/` directory at all; the declared
`test-docs/{feature}/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests

- [ ] **TS1** (FR1, FR3, FR5): Feed the classification logic a synthetic cargo log containing a `test result:` line with a non-zero failed count, together with exit status 101. Expect the warning wording, the observed executed count in the message, and no build-stop wording.
- [ ] **TS2** (FR1, FR2, FR5): Feed the classification logic a synthetic log with no `test result:` line and exit status 101. Expect the build-stop FAIL wording naming exit status 101 and an executed count of 0 derived from the log, not hardcoded.
- [ ] **TS3** (FR4): Feed a synthetic log with no `test result:` line and exit status 0. Expect the existing FAIL wording `command exited 0 but 0 tests executed`.

### Integration Tests

- [ ] **TS4** (FR6): With the TS1 situation, confirm the scenario counts toward `RESULTS_PASS` and the aggregate exit status is 0 when the other scenarios pass.
- [ ] **TS5** (FR7, FR8): Assert the script's un-fetched cargo invocation contains no `--test-threads` and matches the literal reproduction command, and that `ci-workflows.test.ts`'s existing assertions on the CI step string still pass.

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases

- [ ] Multiple `test result:` lines: cargo prints more than one `test result:` line (lib plus doctests); `count_executed_tests` sums them.
- [ ] Build stop with a non-zero exit: the build stops before any `test result:` line while cargo still exits 101 — `executed == 0`, so the build-stop branch applies.
- [ ] Non-test failure after tests ran: a doctest compile error, or a signal-derived exit such as 124 or 137, yields `executed > 0` and is classified as "tests ran but failed"; the warning wording stays deliberately neutral about the cause.
- [ ] All-ignored suite: a suite whose tests are all `ignored` reports `0 passed; 0 failed`, so `count_executed_tests` returns 0 and the zero-executed branch applies even though the harness ran.
- [ ] Regression test isolation: the regression test must not depend on the network or on a Rust toolchain.

### Performance Tests

Not applicable.

## Security Considerations

Not applicable — this feature adds no authentication, authorization, user input handling, data storage or network surface. It changes verdict classification inside an existing CI-only shell script.

## Error Handling

### Verdict Classification

| Condition | Verdict | Message content | Requirement |
|-----------|---------|-----------------|-------------|
| `executed == 0` && `status != 0` | FAIL | names a build stop and the observed exit status | FR2 |
| `executed > 0` && `status != 0` | WARN (not FAIL) | wording distinct from the build-stop message, naming both the exit status and the observed executed count, on its own `verify_font_bootstrap.`-prefixed line | FR3 |
| `executed == 0` && `status == 0` | FAIL | `command exited 0 but 0 tests executed` | FR4 |

### Error Flow

```
cargo test exits → count executed tests → evaluate executed count first (FR1)
  → select branch → emit verdict line(s) → tally into RESULTS_PASS / RESULTS_FAIL (FR6)
  → summary line → aggregate exit status
```

The script keeps `set -uo pipefail` without `set -e`, so a failing scenario never prevents the remaining scenarios from running and reporting (NFR2), and the cleanup trap removes every scenario worktree, stale `.git/worktrees` entry and scratch directory on both success and failure (NFR3).

## Performance Optimization

Not applicable.

## Success Criteria

- [ ] All functional requirements (FR1–FR10) are implemented and tested
- [ ] All test scenarios (TS1–TS5) pass
- [ ] All non-functional requirements (NFR1–NFR5) are satisfied
- [ ] Acceptance criteria AC1–AC7 hold
- [ ] `feature-docs/worktree-font-bootstrap/SPEC.md` and `.github/workflows/release.yml` are unchanged
- [ ] Code review is completed

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None — every requirement is `resolved`.

## Assumptions

- **a1** (reversible): The classification change is confined to `scenario_unfetched`; the fetch-failure and already-fetched scenarios' verdict logic is untouched, because only the un-fetched scenario runs `cargo test` (the others run `cargo check`).
- **a2** (reversible): A "tests ran but failed" un-fetched scenario counts toward `RESULTS_PASS`, so the script exits 0 when the other two scenarios pass.
- **a3** (reversible): No new cargo flag is added anywhere; the un-fetched scenario keeps running the literal reproduction command that worktree-font-bootstrap FR6 pins. Decided by Codex consultation in batch mode.
- **a4** (reversible): The regression test drives the shipped script, or a sourceable classification helper extracted from it, as a real subprocess with synthetic logs — following `plugins/emterm/hooks/scripts/notify-status.test.ts`'s `Bun.spawnSync` precedent — never a real cargo build or a real download.
- **a5** (reversible): `.github/workflows/release.yml` stays byte-for-byte unmodified, and `.github/workflows/ci.yml`'s verify-font-bootstrap step keeps the exact run string `bash scripts/verify-font-bootstrap.sh`.
- **a6** (reversible): The aggregate summary line keeps its existing `verify_font_bootstrap.summary: N/M scenarios passed` shape; a warning is surfaced on its own `verify_font_bootstrap.`-prefixed line.
- **a7** (reversible): `feature-docs/worktree-font-bootstrap/SPEC.md` is NOT edited by this feature. The AC1/FR6 reading is recorded in this feature's own SPEC and in the script's comments. Decided by Codex consultation in batch mode.

## Design Step

Skipped. The change is confined to a bash script's verdict classification, a CI job and a bun regression test. There is no UI, visual or design-token surface, and no design-system artifact is read or written.

## References

- Requirements document: `feature-docs/verify-font-bootstrap-classification/REQUIREMENTS.md`
- Verification script: `scripts/verify-font-bootstrap.sh`
- CI workflow: `.github/workflows/ci.yml`
- Prior feature specification (not edited by this feature): `feature-docs/worktree-font-bootstrap/SPEC.md`
- Subprocess-driven test precedent: `plugins/emterm/hooks/scripts/notify-status.test.ts`
