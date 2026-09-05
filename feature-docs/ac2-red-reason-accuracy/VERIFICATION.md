# Verification Document: ac2-red-reason-accuracy

## Overview

**Feature**: ac2-red-reason-accuracy
**SPEC.md**: `feature-docs/ac2-red-reason-accuracy/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/ac2-red-reason-accuracy/IMPLEMENTATION.md`

The feature rewrites one folded block scalar —
`acceptance_tests['AC-2']['red_reason']` in
`test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` (the **target
record**) — so that it describes only states the **base blob** (the **described
record** `test-docs/stale-test-name-refs/task0001.tests.yaml` at revision
`9eee6161`) supports. Terms in bold are defined in IMPLEMENTATION.md convention
C1; this document uses them throughout, because three files in play are named
`task0001.tests.yaml` and four distinct things are numbered "AC-n".

Per IMPLEMENTATION.md decision D4, the primary evidence is inspection — parsed
content, parse shape, raw formatting, diff scope — not the project's build and
test suites.

## Build Verification

- Command (Rust): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (TypeScript): `bun run typecheck`
- Expected: exit code 0, no errors.
- **Applicability**: the change set contains no Rust and no TypeScript file
  (NFR5), which TS-3 establishes mechanically. When TS-3 shows the change set is
  the single YAML file, these commands exercise nothing this feature touched and
  are recorded as not run, with TS-3's output as the rationale. If TS-3 ever
  shows a Rust or TypeScript file in the change set, that is itself a
  containment failure (FR6) and both commands are then run before anything else.

## Test Verification

- Command (Rust): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command (TypeScript): `bun test`
- Coverage target: not applicable — the feature adds no executable code, so
  there is no coverage surface to move.
- **Applicability**: same rule as Build Verification above.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Parse the target record with PyYAML before and after the change and compare `acceptance_tests['AC-2']['red_reason']`; assert on the parsed value, never on raw lines (the folded scalar re-wraps) | Before: the scalar claims none of the four required elements were present and that the base text made no connection to the described-record AC-2 count explanation. After: it states three of four were missing, names those three ("invariant guard" phrase, described-record AC-6 mention, "no observable pre-state" phrasing), states the fourth pre-existed and was preserved, and retains the post-edit half (all four elements present afterwards; no "confirmed by" / "observed" red-observation language). The "none of the four" claim is absent | Unit |
| TS-2 | Read `acceptance_tests['AC-2']['red_confirmed']` from the target record before and after the change | The boolean `true` in both states — an invariant guard with no observable red pre-state | Unit |
| TS-3 | Diff the target record against the base revision and list the whole change set (`git diff`, `git status --porcelain`) | Every hunk lies inside the AC-2 entry's `red_reason` scalar; `task_id`, `baseline_failures`, `final_failures`, the AC-2 entry's `tests` and `red_confirmed`, the AC-1 and AC-3 through AC-7 entries and the trailing `notes` block are byte-identical; the change set lists only `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`, with no Rust and no TypeScript file | Integration |
| TS-4 | Read the base blob out of git history (`git show 9eee6161:test-docs/stale-test-name-refs/task0001.tests.yaml`) and inspect its AC-7 entry's `red_reason` | The linkage explaining why the described record's AC-2 repository-wide count stands at 6 rather than 7 is present there, so the rewritten justification matches the evidence it cites. Run BEFORE the rewrite (IMPLEMENTATION.md D2); a negative result stops the feature instead of being written around | Integration |
| TS-5 | Inspect the raw target-record text after the change | The AC-2 `red_reason` still uses the `>-` folded block-scalar indicator at its existing indentation, the file's other folded scalars are unchanged in style, the top-level key order is unchanged, and the `acceptance_tests` mapping still holds exactly seven entries | Unit |

## Code Quality Verification

- Format (Rust): `cargo fmt --check` / Format (TypeScript): `bunx biome format .`
- Static analysis: none beyond the above; the project declares no separate
  linter in `workflow.yaml`.
- **Applicability**: same rule as Build Verification — neither formatter has a
  file to inspect in this change set. The YAML record's formatting is verified
  by TS-5 instead.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SPEC AC-1 | The parsed target-record AC-2 `red_reason` states that three of four required elements were missing before the edit | TS-1 |
| SPEC AC-2 | The same parsed scalar states the fourth element — the described-record AC-2 count linkage — already existed in the base text and was preserved | TS-1, corroborated by TS-4 |
| SPEC AC-3 | The parsed scalar no longer claims that none of the four elements were present | TS-1 (negative assertion) |
| SPEC AC-4 | `acceptance_tests['AC-2']['red_confirmed']` is the boolean `true` | TS-2 |
| SPEC AC-5 | The whole file loads as valid YAML with its original shape | TS-5 |
| SPEC AC-6 | No acceptance entry other than AC-2, and no other top-level key, differs | TS-3 |
| — | `git status --porcelain` lists only `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` | TS-3 |
| — | FR1–FR6 implemented, NFR1–NFR5 hold, TS-1 through TS-5 pass | the coverage table below |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1 |
| FR2 | task0001 | TS-1, TS-4 |
| FR3 | task0001 | TS-1 |
| FR4 | task0001 | TS-1 |
| FR5 | task0001 | TS-2 |
| FR6 | task0001 | TS-3 |
| NFR1 | task0001 | TS-5 |
| NFR2 | task0001 | TS-5 |
| NFR3 | task0001 | MV-1 (manual; no numbered scenario exists — see IMPLEMENTATION.md Open Questions) |
| NFR4 | task0001 | TS-4, and MV-2 |
| NFR5 | task0001 | TS-3 |

## E2E Testing

Not applicable. The project declares no E2E command for either component
(`e2e_test_command` is empty for both), and the feature has no runtime surface
to drive.

## Manual Testing (E2E Not Possible)

- [ ] MV-1 (NFR3): Read the rewritten scalar and confirm it is English, in the
      same register as the target record's other entries. No automated check
      exists for register, and SPEC.md defines no scenario for NFR3.
- [ ] MV-2 (NFR4): Read the rewritten scalar as prose against what TS-4's read
      actually returned, and confirm every statement it makes about the pre-edit
      state is supported by that read — in particular that it introduces no new
      claim about the base text beyond the corrected count and the pre-existing
      fourth element. This is the judgement the whole feature exists to enforce,
      so it is checked by a human rather than by string matching.
- [ ] MV-3 (IMPLEMENTATION.md C2 / D3): Read this feature's own record
      `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml` and confirm that
      every criterion classified as an invariant guard in the task plan's Test
      Notes carries `red_confirmed: false` with a reason saying why no red was
      observable. A `true` there would reproduce, in a third file, the defect
      this feature removes.

No mockup comparison item applies: the design step is `skipped` and the feature
has no visual surface.

## Performance / Security Verification (if applicable)

Not applicable. No code path, no input handling, and no data surface is touched
(SPEC.md Security Considerations, Performance Tests).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Test scenarios (TS-1..TS-5) | 5 | 5 | 0 | 0 |
| SPEC success criteria (AC-1..AC-6) | 6 | 6 | 0 | 0 |
| Requirements (FR1–FR6, NFR1–NFR5) | 11 | 10 | 0 | 1 (NFR3) |
| Manual judgement items (MV-1..MV-3) | 3 | 0 | 0 | 3 |
| Build / test / format commands | 6 | 0 | 0 | 0 (not applicable — see Build Verification) |
