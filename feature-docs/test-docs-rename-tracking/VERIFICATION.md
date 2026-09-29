# Verification Document: test-docs-rename-tracking

## Overview
**Feature**: test-docs-rename-tracking / **SPEC.md**: `feature-docs/test-docs-rename-tracking/SPEC.md` / **IMPLEMENTATION.md**: not written (reduced tier; no file is declared by more than one task) / **THREAT-MODEL.md**: `feature-docs/test-docs-rename-tracking/THREAT-MODEL.md` (verdict `no-trust-boundary`)

The feature edits two test-docs records and adds one `.claude/rules/` file; it changes no
source or test code. The build, test-suite and code-quality commands below are regression
sanity checks; the feature-specific evidence is TS-1 to TS-7.

`<base>` below is `workflow.implement.base_commit`. Every command runs from the repository
root (core-build-location.md).

## Build Verification
- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (webview-ts): `bun run build:viewer && bun run build:settings`
- Expected: exit code 0, no errors

## Test Verification
- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command (webview-ts): `bun test`
- Expected: exit code 0
- Coverage target: not applicable — the feature adds and changes no source or test code.
- Known nondeterminism unrelated to this feature: `tabs.rs` replay tests can fail under
  parallel execution, and `tmux_sockets` discover tests fail rarely under parallel
  execution. Rerun a failing test of either group with `--test-threads=1` before treating
  it as a regression.

### Test Scenarios from SPEC.md

Records under test: `test-docs/notification-summary-markup-escape/task0001.tests.yaml`
(summary record) and `test-docs/notification-body-markup-escape/task0001.tests.yaml` (body
record). Stale paths: the four paths on the left of the SPEC FR1 mapping.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Search both records for each of the four stale paths | No match in any `acceptance_tests[].tests` list entry. Matches inside `red_reason` values (body record AC-4) are expected and allowed | Automated command |
| TS-2 | Run `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list` and look up every test path listed in both records | Each of the 15 distinct paths (summary record: 15 entries; body record: 8 entries, all also in the summary record) appears in the output with the `: test` suffix | Automated command |
| TS-3 | For each of the four current names, run `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib <name>` | Every run reports at least one test run and passed; none reports `0 passed; 0 failed; N filtered out` | Automated command |
| TS-4 | Parse both records at `<base>` and at HEAD with a YAML safe loader; substitute the SPEC FR1/FR2 mapping into the base data and compare. Also run `git diff <base>..HEAD` on both records | Parsed data is equal after the substitution: `task_id`, `baseline_failures`, `final_failures`, every `red_confirmed` / `red_reason` and every list order unchanged; the only changed strings are the 4 (summary) + 3 (body) mapped paths. The line diff shows only the mapped path lines and added comment-only lines | Automated command |
| TS-5 | Inspect the summary AC-2, summary AC-5 and body AC-4 blocks | Each block contains a YAML comment citing `feature-docs/notification-markup-fail-closed/SPEC.md` FR1/FR3 that states the fetch-failure expectation was superseded and is scoped to the fetch-failure case; no other AC block carries such a note | Manual (search-assisted) |
| TS-6 | Read `.claude/rules/test-docs-records.md` | The rule states the follow-current-names policy, its scope (test-docs `acceptance_tests[].tests` lists; renames with a successor test) and the four FR6 elements; the file name has neither a `project-` nor a `core-` prefix | Manual |
| TS-7 | Run `git diff --stat <base>..HEAD` | Changed paths are limited to the two records, `.claude/rules/test-docs-records.md`, `feature-docs/test-docs-rename-tracking/**` and `test-docs/test-docs-rename-tracking/**`. Nothing under `src-tauri/src/`, `crates/` or build configuration, and nothing under `feature-docs/notification-markup-fail-closed/` or `test-docs/notification-markup-fail-closed/`, is changed | Automated command |

## Code Quality Verification
- Format (main): `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Format / lint (webview-ts): `bunx biome check .`
- Expected: exit code 0. This feature changes no source file, so a finding that also
  exists at `<base>` is pre-existing and not attributable to it.
- YAML well-formedness of the two edited records is covered by TS-4.

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | Every functional requirement is implemented and verified | Every row of Functional Requirements Coverage below passes |
| SC-2 | Every test scenario passes | TS-1 to TS-7 pass |
| SC-3 | SPEC AC-1 to AC-8 are all satisfied | SPEC Acceptance Criteria table below |

### SPEC Acceptance Criteria
| SPEC AC | Requirements | Verified by |
|---------|--------------|-------------|
| AC-1 | FR1 | TS-2, TS-4 |
| AC-2 | FR2 | TS-2, TS-4 |
| AC-3 | FR1, FR2 | TS-1 |
| AC-4 | FR5 | TS-2, TS-3 |
| AC-5 | FR3 | TS-5 |
| AC-6 | FR4, NFR2 | TS-4 |
| AC-7 | FR6, NFR3 | TS-6 |
| AC-8 | NFR1, NFR2 | TS-7 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-4 |
| FR2 | task0001 | TS-1, TS-2, TS-4 |
| FR3 | task0001 | TS-5 |
| FR4 | task0001 | TS-4 |
| FR5 | task0001 | TS-2, TS-3 |
| FR6 | task0002 | TS-6 |
| NFR1 | task0001, task0002 | TS-7 |
| NFR2 | task0001 | TS-4, TS-7 |
| NFR3 | task0002 | TS-6, TS-7 |

## Manual Testing (E2E Not Possible)
- [ ] TS-5: Read the three supersede notes. Each names `feature-docs/notification-markup-fail-closed/SPEC.md` FR1/FR3, says the fetch-failure expectation (title/body left unescaped when `get_capabilities()` fails) was replaced by the fail-closed behavior, and does not claim that the block's other entries were superseded. Summary AC-2's note in particular leaves the unconfirmed-capability-list entries standing.
- [ ] TS-6: Read the new rule and confirm each FR6 element is stated in a form a later feature can apply without this feature's context: the update duty, the supersede note (YAML comment naming SPEC and FR; `red_reason` unchanged), the renaming feature's own records kept as written, and the listing-command check.

## Performance / Security Verification (if applicable)
Not applicable. SPEC.md has no performance requirement, and THREAT-MODEL.md's verdict is
`no-trust-boundary`, so there is no TM-n to verify.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test suite | 2 | 2 | 0 | 0 |
| Test scenarios (TS-1 to TS-7) | 7 | 5 | 0 | 2 |
| Code quality | 2 | 2 | 0 | 0 |
| Security (TM-n) | 0 | 0 | 0 | 0 |
| **Total** | **13** | **11** | **0** | **2** |
