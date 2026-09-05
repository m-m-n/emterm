# Implementation Plan: bun-install-reproducibility

## Overview

Freeze the JavaScript dependency graph on a committed bun lockfile, determine
and record why dompurify 3.4.x drops the leading heading from sanitized
markdown output, and give CI a clean-checkout install/test path whose install
step fails loudly on lockfile drift.

## Technology Stack

- **Package manager / test runner**: Bun — resolves the dependency graph,
  produces the lockfile, runs the test suite, bundles the child WebView entries.
- **Sanitizer**: dompurify — already a declared dependency; this feature only
  moves its version constraint and (if required) the way it is invoked.
- **CI**: GitHub Actions — hosts the clean-checkout install and test path.
- **Test DOM**: happy-dom, wired through the repository-root test setup module —
  one of the three candidate layers the investigation must distinguish.

### New dependencies and licenses

Project license: `MIT` (workflow.yaml `project.license`).

**No new dependency is introduced by this feature.** Nothing is added to the
dependency or devDependency sets; the only declaration that changes is the
existing dompurify version constraint. Because the adopted dompurify version
may differ from the currently installed one, the task that changes that
constraint records, in the findings document, the license the adopted version's
own package metadata declares. dompurify has historically shipped a dual
permissive / weak-copyleft grant (Apache-2.0 or MPL-2.0), and both options are
compatible with an MIT project; metadata showing anything else is a finding to
report, not a change to adopt silently.

## Layer Structure

| Layer | Artifact | Responsibility | Depends on |
|---|---|---|---|
| Declaration | `package.json` | States which dependency versions are acceptable | — |
| Resolution | `bun.lock` | Freezes one concrete graph for the declaration | Declaration |
| Install | plain install / frozen-lockfile install | Materializes the frozen graph | Resolution |
| Consumers | test suite, typecheck, viewer / settings bundle builds | Run against the materialized graph | Install |
| Sanitization | markdown renderer's sanitize boundary and its config | Turns rendered markup into what reaches the child WebView DOM | Install (dompurify version) |

Allowed dependency direction is downward only. No consumer edits the
declaration or the resolution to make itself pass; a consumer failure is either
a declaration decision (a version constraint) or a sanitization decision, never
a test-side relaxation.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|---|---|---|---|
| Dependency declaration (`package.json`) | The single expression of the dompurify version decision | Pre: the version decision is grounded in the recorded investigation finding. Post: exactly one dompurify constraint exists, and the frozen resolution of it satisfies the viewer entry tests | task0001 (owner), task0002, task0003 |
| Committed lockfile (`bun.lock`) | The frozen resolution of the declaration, tracked by git | Pre: not matched by any ignore rule. Post: a frozen-lockfile install from a clean checkout exits zero, and two independent clean installs of the same commit resolve dompurify to the same version string | task0002 (owner), task0001, task0003 |
| Lockfile / manifest sync duty | Keeps declaration and resolution consistent across independently merged branches | Pre: a merge has brought in either a changed declaration or the tracked lockfile. Post: before the task's own commit, the lockfile is regenerated from the merged declaration so a frozen-lockfile install exits zero. Owned by BOTH task0001 and task0002, each stating it in its own Acceptance Criteria | task0001, task0002 |
| h1-loss findings record (`doc/dompurify-h1-sanitization.md`) | Durable record of the mechanism, the observation that pins it down, and the reason behind the adopted version | Pre: the observation is reproducible by a third party from the document alone. Post: names exactly one of the three candidate layers as responsible, and carries the adopted version's license line | task0001 (owner); read by the review and verify phases |
| CI install path | Materializes the frozen graph on a clean checkout before any consumer step | Pre: the tracked lockfile exists at the repository root. Post: every install invocation in CI is a frozen-lockfile install; CI never generates or commits a lockfile, and a drifted declaration fails the run at the install step | task0003 (owner); provided by task0002 |

## Conventions

- **Sanitization strictness is a floor, not a knob.** No task widens the
  sanitizer config's allowed tag set, allowed attribute set, or URI pattern, and
  no task removes a forbid entry. A fix that needs any of those is not a fix; it
  is a finding to report.
- **Existing viewer entry assertions are immutable.** No task edits the viewer
  entry test file. Making a failing assertion pass by changing the assertion,
  the matcher, or the selector is out of bounds for every task in this feature.
- **One JavaScript lockfile.** After this feature, the bun lockfile is the only
  lockfile at the repository root. No task reintroduces a second one.
- **The lockfile is generated, never hand-edited.** Any lockfile content a task
  commits is the unmodified output of an install run against the declaration
  present on its branch.
- **Failure evidence lives in the findings record, not in commit messages.** The
  observation that pins the mechanism, and the before/after outputs behind it,
  belong in the findings document so the verify phase can re-run them.
- **Error-handling policy for this feature**: drift and missing preconditions
  fail loudly at the earliest step (install), rather than degrading into a
  silently different graph. No task adds a fallback that re-resolves when the
  frozen install fails.

## Cross-task Design Decisions

### D1 — The version decision is expressed only in the declaration

The investigation task decides which dompurify version the project adopts, and
expresses that decision solely as the constraint in `package.json`. The lockfile
task never chooses a version; it freezes whatever the declaration resolves to.
Rationale: tasks run fully in parallel in separate worktrees, so a decision
cannot flow from one task's output into another's input. Keeping the decision in
a single declared field makes the outcome well-defined regardless of merge
order. Affected tasks: task0001, task0002.

### D2 — The lockfile / manifest sync duty is owned by both lockfile-touching tasks

Either merge order can leave the lockfile stale relative to the declaration. The
duty is therefore stated in the Acceptance Criteria of both tasks that touch
either file: whichever integrates second regenerates the lockfile from the
merged declaration before committing. Rationale: with no ordering mechanism
between tasks, an unowned integration step is an unwired integration step.
Affected tasks: task0001, task0002.

### D3 — "Clean worktree, 14 pass / 0 fail" is an integrated outcome

FR3 spans the version decision and the frozen graph, so no single task can
demonstrate it alone. Task-level Acceptance Criteria are scoped to what each
task can verify inside its own worktree; the end-to-end clean-worktree run is
verified feature-wide by VERIFICATION.md TS-1. Rationale: an Acceptance
Criterion a task cannot evaluate is not a TDD contract. Affected tasks:
task0001, task0002.

### D4 — The CI test step's home is decided by enumeration, not assumption

Only one workflow file was visible to the requirements analysis, and it runs no
test command (ASM-5). The CI task enumerates the workflow directory first: if a
workflow already runs the bun test suite, the frozen-install and test guarantees
are added there and the predicted new workflow file is not created — reported as
a plan deviation rather than silently duplicating a job. Affected task: task0003.

### D5 — Every CI install invocation is a frozen-lockfile install

The drift guard is worthless if one CI job still runs a plain install and
re-resolves. The CI task converts every install invocation across the workflow
directory, not only the one on the new test path. Rationale: a single
non-frozen install path is enough to reintroduce the environment-dependent
graph this feature exists to remove. Affected task: task0003.

### D6 — The investigation's outcome is recorded either way

If the newer dompurify line turns out not to be adoptable without weakening
sanitization, the adopted constraint stays where the tests pass and the findings
record carries the blocking mechanism as the pin's stated reason. Rationale: FR5
accepts both outcomes but rejects an unexplained pin. Affected task: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| The h1 loss turns out to be a happy-dom / sanitizer interaction that only manifests in the test environment | Medium | Medium | The findings record must name the layer; a test-environment-only cause is fixed in the test setup, leaving production sanitization untouched (NFR1, NFR2) |
| Pressure to make the entry tests pass by relaxing the sanitizer config | Low | High | Stated as a convention floor above, and as an explicit Acceptance Criterion in task0001 (no widened tag/attribute set, no removed forbid entry) |
| Merge order leaves the committed lockfile stale relative to the declaration | High | Medium | D2's sync duty, owned by both tasks, with a frozen-lockfile install exiting zero as the mechanical check |
| An existing workflow already runs the test suite and a second one is added | Medium | Low | D4's enumeration-first rule, with substitution reported as a plan deviation |
| The newer dompurify line is not adoptable without weakening sanitization | Medium | Medium | D6: pin where the tests pass and record the blocking mechanism as the reason |
| The adopted dompurify version's license differs from the historically dual grant | Low | High | The adopted version's license line is recorded in the findings document and cross-checked by the license review perspective |

## Open Questions

- [ ] Whether any workflow in the CI workflow directory already runs the bun
      test suite is unknown at planning time (ASM-5, envelope read restriction).
      Resolved by task0003's enumeration; a substitution is a reportable plan
      deviation, not a blocker.
- [ ] Whether dompurify 3.4.x is adoptable without weakening sanitization is
      unknown until the investigation completes. Both outcomes are in scope
      (D6); neither requires a new user decision.
