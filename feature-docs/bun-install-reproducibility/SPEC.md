# Feature: bun-install-reproducibility

> Requirements source: `feature-docs/bun-install-reproducibility/REQUIREMENTS.md`.
> This document is the implementation-facing rendering of the same requirements.

## Overview

`bun install` currently resolves to a different dependency graph depending on the
environment, because `bun.lock` is gitignored (`.gitignore:42`, under the `# Bun`
heading) and a fresh worktree therefore re-migrates from `package-lock.json`. The
re-resolved graph pulls dompurify 3.4.x, which drops the leading `<h1>` from
`MarkdownRenderer.render()` output and fails two of the fourteen viewer entry
tests. This feature freezes the dependency graph by committing the bun lockfile
and deleting `package-lock.json`, determines the mechanism behind the h1 loss,
and puts the project in a state where dompurify 3.4.x is safely adoptable — all
while keeping the existing h1 regression assertions intact.

## Objectives

- Make `bun install` resolve to an identical dependency graph in every
  environment, so `bun test` results no longer depend on which worktree or runner
  executed them.
- Remove the recurring verify-phase cost of triaging the two viewer entry
  failures as "unrelated to the change", which currently risks masking a genuine
  regression.
- Keep the existing h1 sanitization regression coverage intact while reaching a
  state where dompurify 3.4.x can be adopted safely, rather than freezing the
  sanitizer at 3.3.1 indefinitely.

## User Stories

### US1: Deterministic install in a fresh worktree
As an eMterm developer, I want `bun install` in a freshly created worktree to
resolve the same dependency graph as everywhere else, so that `bun test` results
reflect my change rather than which worktree I happened to use.

**Acceptance Criteria:**
- [ ] AC-1: In a worktree created fresh via `git worktree add`, `bun install`
      followed by `bun test src-tauri/viewer/web/entry.test.ts` reports 14 pass /
      0 fail.
- [ ] AC-2: `bun.lock` is tracked by git and `.gitignore` no longer contains a
      `bun.lock` entry.
- [ ] AC-3: `package-lock.json` is absent from the working tree and from the
      committed index.
- [ ] AC-4: Two clean installs from the same commit, in two different worktrees,
      resolve `dompurify` to the identical version string.

### US2: h1 sanitization behavior understood, not worked around
As an eMterm developer, I want the dompurify 3.4.x h1-loss mechanism recorded
from observation and the h1 assertions kept as they are, so that adopting 3.4.x
is a decision with a known basis rather than a blind workaround.

**Acceptance Criteria:**
- [ ] AC-5: The dompurify 3.4.x h1-loss mechanism is recorded with a reproducible
      observation that pins down which of the three candidate layers (dompurify
      behavior change / PURIFY_CONFIG option semantics / happy-dom interaction)
      is responsible.
- [ ] AC-6: `entry.test.ts:67` and `entry.test.ts:128` still assert that the
      rendered `h1` textContent contains `Title` and `Hi` respectively, with no
      relaxation of the matcher or the selector.
- [ ] AC-9: `PURIFY_CONFIG` contains no newly-allowed tag or attribute, and no
      removed `FORBID_*` entry, relative to the pre-change file.

### US3: CI catches lockfile drift on a clean, locked install
As the CI system, I want to install the locked dependency graph on a clean
checkout and run the viewer entry tests, so that a `package.json` change
committed without a regenerated lockfile fails loudly instead of silently
resolving a different graph.

**Acceptance Criteria:**
- [ ] AC-7: A CI workflow runs `bun test` on a clean checkout after a
      frozen-lockfile install, and `src-tauri/viewer/web/entry.test.ts` is
      included in that run.
- [ ] AC-8: A `package.json` dependency edit committed without regenerating
      `bun.lock` causes the CI install step to fail rather than proceed.
- [ ] AC-10: The full `bun test` suite shows no newly-failing test beyond the two
      pre-existing `plugin/marketplace version regression guard (task0002 AC-9)`
      failures, which also fail on `main` and are out of scope.

## Technical Requirements

### Functional Requirements

- **FR1 — Commit the bun lockfile:** `bun.lock` is removed from `.gitignore`
  (currently listed at `.gitignore:42` under the `# Bun` heading) and the
  generated lockfile is committed, so `bun install` in a fresh worktree resolves
  from a frozen graph instead of re-migrating from `package-lock.json`.
  *(status: resolved)*
- **FR2 — Delete the stale package-lock.json:** `package-lock.json` is deleted
  from the repository. The bun lockfile becomes the single source of truth for
  the JavaScript dependency graph, and no second JS lockfile is reintroduced.
  *(status: resolved)*
- **FR3 — Clean-worktree viewer entry tests pass:** After `bun install` in a
  freshly created worktree, `bun test src-tauri/viewer/web/entry.test.ts` reports
  14 pass / 0 fail — including `renders an injected sample into the fullscreen
  content structure` (entry.test.ts:55) and `parses the shared Rust/TS payload
  fixture with all fields` (entry.test.ts:108). *(status: resolved)*
- **FR4 — Identify the dompurify 3.4.x h1-loss mechanism:** The mechanism by
  which dompurify 3.4.x drops the leading `<h1>` from `MarkdownRenderer.render()`
  output — the `DOMPurify.sanitize(rawHtml, PURIFY_CONFIG)` call at
  `src-tauri/web-shared/markdown/renderer.ts:216`, executing against happy-dom
  via `test-setup.ts` — is determined by observation and recorded, rather than
  worked around blindly. The determination distinguishes between a dompurify
  behavior change, a `PURIFY_CONFIG` option that 3.4.x interprets differently,
  and a happy-dom/dompurify interaction. *(status: resolved)*
- **FR5 — Reach a state where dompurify 3.4.x is safely adoptable:** Once FR4's
  mechanism is known, the project is brought to a state where dompurify 3.4.x can
  be adopted with the h1 assertions passing and sanitization strictness
  unchanged. The version recorded in the committed lockfile reflects that
  outcome; if 3.4.x cannot yet be adopted, the pinned version carries the FR4
  finding as its recorded reason rather than an unexplained pin.
  *(status: resolved)*
- **FR6 — Preserve the h1 regression coverage:** The assertions
  `expect(content.querySelector("h1")?.textContent).toContain("Title")`
  (entry.test.ts:67) and `...toContain("Hi")` (entry.test.ts:128) are kept as-is.
  Weakening, loosening, or deleting them is not an acceptable route to FR3.
  *(status: resolved)*
- **FR7 — Viewer entry tests run on a clean, locked install in CI:** CI runs
  `bun test` (covering `src-tauri/viewer/web/entry.test.ts`) on a clean install of
  the locked dependency graph. `.github/workflows/release.yml` — the only workflow
  supplied to the analysis — runs `bun install` at line 218 (build-linux) and line
  316 (build-windows) but runs no test command at all, so satisfying this requires
  adding a test execution path, not merely confirming an existing one. The
  implementation enumerates `.github/workflows/` to confirm whether any other
  workflow already runs `bun test` before deciding where the step belongs.
  *(status: resolved)*
- **FR8 — Lockfile drift fails loudly in CI:** The CI install path uses a
  frozen-lockfile install (`bun install --frozen-lockfile`) so a `package.json`
  change committed without a regenerated `bun.lock` fails the run instead of
  silently resolving a different graph. *(status: resolved)*

### Non-Functional Requirements

- **NFR1 — Security (sanitization strictness is not relaxed):** No change to
  `PURIFY_CONFIG`'s `ALLOWED_TAGS` / `FORBID_TAGS` / `FORBID_ATTR` /
  `ALLOWED_URI_REGEXP` that widens what reaches the child WebView DOM is
  acceptable as the fix. XSS protection in child WebViews is a stated product
  pillar (CLAUDE.md, "Robust isolation"). *(status: resolved)*
- **NFR2 — Compatibility (rendered output is unchanged for currently-passing
  cases):** The 12 viewer entry tests that already pass, and the rest of the
  `bun test` suite, keep passing. The front-matter, outline, theme-token, and
  MD3-token-parity behaviors asserted in `entry.test.ts` are not altered.
  *(status: resolved)*
- **NFR3 — Compatibility (GUI build inputs stay buildable on both platforms):**
  `bun run build:viewer` and `bun run build:settings` still produce the bundles
  that `src-tauri/build.rs` embeds, on the Linux and Windows CI paths alike
  (release.yml:318-321 builds the bundles on Windows; `scripts/build-dpkg.sh`
  covers Linux). *(status: resolved)*
- **NFR4 — Maintainability (reproducibility is verifiable, not asserted):** The
  reproducibility claim is demonstrated by two independent clean installs from
  the same commit resolving to the same dompurify version — not by inspection of
  `package.json` ranges alone. *(status: resolved)*

## Implementation Approach

### Architecture

This feature has no runtime architecture of its own; it changes the dependency
resolution inputs and the CI install/test path. The affected layers:

```
┌───────────────────────────────────────────────────────────┐
│ Dependency declaration      package.json                  │
├───────────────────────────────────────────────────────────┤
│ Dependency resolution       bun.lock  (FR1: committed)    │
│                             package-lock.json (FR2: gone) │
├───────────────────────────────────────────────────────────┤
│ Install                     bun install                   │
│                             bun install --frozen-lockfile │
│                                          (FR8, CI path)   │
├───────────────────────────────────────────────────────────┤
│ Test / build consumers                                    │
│   bun test src-tauri/viewer/web/entry.test.ts  (FR3, FR7) │
│   bun run build:viewer / build:settings        (NFR3)     │
├───────────────────────────────────────────────────────────┤
│ Sanitization under investigation                          │
│   web-shared/markdown/renderer.ts:216                     │
│     DOMPurify.sanitize(rawHtml, PURIFY_CONFIG)            │
│   happy-dom via test-setup.ts        (FR4, FR5, NFR1)     │
└───────────────────────────────────────────────────────────┘
```

**Component relationships:**

- `.gitignore` currently hides `bun.lock` from git (line 42). FR1 removes that
  entry; FR2 removes `package-lock.json`, so the migration source that causes
  re-resolution disappears.
- `src-tauri/build.rs` embeds `viewer/dist` and `settings/dist`, which are
  produced by `bun run build:viewer` / `bun run build:settings` — the reason NFR3
  constrains this change.
- `src-tauri/viewer/web/entry.test.ts` exercises `MarkdownRenderer.render()`,
  whose sanitize call at `renderer.ts:216` is where the dompurify version
  difference becomes observable.

### Data Flow

```
git worktree add  →  bun install  →  resolved graph  →  bun test entry.test.ts
                        ↑                                       ↓
                     bun.lock (frozen, committed)      h1 assertions (FR6)

CI:  clean checkout  →  bun install --frozen-lockfile  →  bun test
                              ↓ (package.json ≠ bun.lock)
                          non-zero exit  (FR8 / AC-8)
```

### API Design

Not applicable. This feature introduces no API surface.

### Database Schema

Not applicable. This feature introduces no persisted data model.

### Dependencies

**Internal Dependencies:**

- `src-tauri/web-shared/markdown/renderer.ts` — hosts the
  `DOMPurify.sanitize(rawHtml, PURIFY_CONFIG)` call at line 216 that FR4
  investigates and NFR1 constrains.
- `src-tauri/viewer/web/entry.test.ts` — the 14 viewer entry tests, including the
  h1 assertions at lines 67 and 128 that FR6 preserves.
- `test-setup.ts` — supplies the happy-dom environment the sanitizer runs
  against, one of FR4's three candidate layers.
- `src-tauri/build.rs` — embeds the `viewer/dist` / `settings/dist` bundles
  covered by NFR3.

**External Dependencies:**

- `dompurify` — 3.4.x is the version whose h1 behavior FR4 investigates and FR5
  targets for safe adoption; 3.3.1 is the version the sanitizer must not be
  frozen at indefinitely (business objective 3).
- `marked`, `happy-dom` — already ruled out by the reporter through individual
  pinning (ASM-6), so the investigation starts from dompurify.
- Bun — the package manager whose lockfile (`bun.lock`) becomes the single source
  of truth (FR1, FR2), and whose `--frozen-lockfile` flag implements FR8.

### File Structure

Files named by the requirements as involved in this change:

```
.gitignore                                  # line 42: bun.lock entry (FR1)
bun.lock                                    # committed lockfile (FR1)
package-lock.json                           # deleted (FR2)
package.json                                # dependency declarations (FR8 drift source)
test-setup.ts                               # happy-dom setup (FR4)
.github/workflows/release.yml               # bun install at 218 / 316; bundle build 318-321 (FR7, NFR3)
.github/workflows/                          # enumerated before choosing the test step's home (FR7, ASM-5)
scripts/build-dpkg.sh                       # Linux bundle build path (NFR3)
src-tauri/build.rs                          # embeds viewer/dist + settings/dist (NFR3)
src-tauri/web-shared/markdown/renderer.ts   # line 216 sanitize call (FR4, FR5, NFR1)
src-tauri/viewer/web/entry.test.ts          # lines 55, 67, 108, 128 (FR3, FR6)
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored list:
the feature-specific paths above are derived at create-plan from every task's
`files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated entries in
addition to the feature-specific paths above:

- `feature-docs/bun-install-reproducibility/**`
- `test-docs/bun-install-reproducibility/**`

`feature-docs/bun-install-reproducibility/**` covers `REQUIREMENTS.md`,
`SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the phase
documents and by `references/phase-state.md`; this section cites them and
restates none of their rules.

`test-docs/bun-install-reproducibility/**` covers
`test-docs/bun-install-reproducibility/{T}.tests.yaml`, the per-task test record.
It is generated and owned by `implement-phase.md`; this section cites it and
restates none of its rules.

These two default entries are part of the declaration unless the SPEC author
explicitly removes them; their absence is never assumed by silence — removal is a
deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed at
verification time must be CONTAINED IN the declared set, not equal to it. A
feature that produces no implement tasks generates no
`test-docs/bun-install-reproducibility/` directory at all; the declared
`test-docs/bun-install-reproducibility/**` entry is still correct in that case —
a declared path that never materializes is not a violation.

## Test Scenarios

### Unit Tests

- [ ] **TS-4** (h1 sanitization regression) — covers FR3, FR4, FR5, FR6: the
      existing `entry.test.ts` h1 assertions, run against the locked dompurify
      version, on both the main worktree and a clean worktree.
- [ ] **TS-5** (sanitization strictness) — covers NFR1: existing suite coverage of
      the renderer's forbidden-tag / forbidden-attribute behavior still passes
      after any renderer or config change made for FR5.

### Integration Tests

- [ ] **TS-1** (clean-worktree reproduction) — covers FR1, FR2, FR3: create a
      fresh worktree, `bun install`, run
      `bun test src-tauri/viewer/web/entry.test.ts`; expect 14 pass / 0 fail.
      This is the exact path that fails today.
- [ ] **TS-2** (resolution determinism) — covers FR1, NFR4: from the same commit,
      install into two separate fresh worktrees and compare the resolved
      `dompurify` version; expect equality.
- [ ] **TS-6** (full suite baseline) — covers NFR2: `bun test` and
      `bun run typecheck` from a clean install; only the two known out-of-scope
      marketplace-guard failures remain.
- [ ] **TS-7** (bundle build) — covers NFR3: `bun run build:viewer` and
      `bun run build:settings` succeed from the clean, locked install, so the Rust
      GUI build's embedded assets are still produced.
- [ ] **TS-9** (lockfile is not ignored) — covers FR1:
      `git check-ignore bun.lock` finds no matching rule, and `git status` shows
      the lockfile as tracked rather than untracked.

### E2E Tests

**Existing E2E tests**: None — `resolved_input_paths.e2e` is empty for this
dispatch.
**Run command**: Not detected.

- [ ] **TS-8** (CI end-to-end) — covers FR7: the CI workflow run itself shows the
      clean-install + `bun test` steps executing and the viewer entry tests
      appearing in the output.

### Edge Cases

- [ ] **TS-3** (frozen-lockfile guard) — covers FR8: mutate a `package.json`
      dependency range without regenerating `bun.lock`, then run
      `bun install --frozen-lockfile`; expect a non-zero exit.
- [ ] dompurify 3.4.x cannot yet be adopted — FR5: the pinned version carries the
      FR4 finding as its recorded reason rather than an unexplained pin.
- [ ] No workflow under `.github/workflows/` runs `bun test` — FR7 / ASM-5: the
      implementation enumerates `.github/workflows/` and adds a test execution
      path rather than assuming one exists.
- [ ] The two `plugin/marketplace version regression guard (task0002 AC-9)`
      failures — AC-10 / ASM-4: they also fail on `main` and stay out of scope;
      only failures beyond those two count as newly-failing.

### Performance Tests

Not applicable. This feature declares no performance requirement.

## Security Considerations

- **Input Validation / XSS Prevention:** `PURIFY_CONFIG` governs what reaches the
  child WebView DOM. Per NFR1, no change to `ALLOWED_TAGS` / `FORBID_TAGS` /
  `FORBID_ATTR` / `ALLOWED_URI_REGEXP` that widens that surface is an acceptable
  fix; XSS protection in child WebViews is a stated product pillar (CLAUDE.md,
  "Robust isolation"). AC-9 verifies this by diffing `PURIFY_CONFIG` against the
  pre-change file for newly-allowed tags or attributes and removed `FORBID_*`
  entries.
- **Supply chain:** Committing `bun.lock` (FR1) and deleting `package-lock.json`
  (FR2) makes the resolved dependency graph a single, reviewable source of truth;
  the frozen-lockfile CI install (FR8) prevents an unreviewed graph from being
  resolved silently.
- **Authentication / Authorization / CSRF / SQL injection:** Not applicable —
  this feature introduces no request handling or data store.

## Error Handling

| Condition | Where | Expected behavior |
|---|---|---|
| `package.json` dependency edit committed without a regenerated `bun.lock` | CI install step (FR8) | `bun install --frozen-lockfile` exits non-zero; the run fails rather than proceeding (AC-8 / TS-3) |
| dompurify 3.4.x drops the leading `<h1>` | `renderer.ts:216` sanitize under happy-dom (FR4) | The mechanism is determined by observation and recorded, distinguishing dompurify behavior change / `PURIFY_CONFIG` option semantics / happy-dom interaction (AC-5) |
| dompurify 3.4.x not yet adoptable | Lockfile pin (FR5) | The pinned version carries the FR4 finding as its recorded reason |

### Error Flow

```
package.json changed → bun.lock not regenerated → --frozen-lockfile install → non-zero exit → CI run fails
```

## Performance Optimization

Not applicable. This feature declares no performance goal.

## Success Criteria

- [ ] AC-1: In a worktree created fresh via `git worktree add`, `bun install`
      followed by `bun test src-tauri/viewer/web/entry.test.ts` reports 14 pass /
      0 fail.
- [ ] AC-2: `bun.lock` is tracked by git and `.gitignore` no longer contains a
      `bun.lock` entry.
- [ ] AC-3: `package-lock.json` is absent from the working tree and from the
      committed index.
- [ ] AC-4: Two clean installs from the same commit, in two different worktrees,
      resolve `dompurify` to the identical version string.
- [ ] AC-5: The dompurify 3.4.x h1-loss mechanism is recorded with a reproducible
      observation that pins down which of the three candidate layers (dompurify
      behavior change / PURIFY_CONFIG option semantics / happy-dom interaction) is
      responsible.
- [ ] AC-6: `entry.test.ts:67` and `entry.test.ts:128` still assert that the
      rendered `h1` textContent contains `Title` and `Hi` respectively, with no
      relaxation of the matcher or the selector.
- [ ] AC-7: A CI workflow runs `bun test` on a clean checkout after a
      frozen-lockfile install, and `src-tauri/viewer/web/entry.test.ts` is
      included in that run.
- [ ] AC-8: A `package.json` dependency edit committed without regenerating
      `bun.lock` causes the CI install step to fail rather than proceed.
- [ ] AC-9: `PURIFY_CONFIG` contains no newly-allowed tag or attribute, and no
      removed `FORBID_*` entry, relative to the pre-change file.
- [ ] AC-10: The full `bun test` suite shows no newly-failing test beyond the two
      pre-existing `plugin/marketplace version regression guard (task0002 AC-9)`
      failures, which also fail on `main` and are out of scope.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. Every functional and non-functional requirement (FR1–FR8, NFR1–NFR4) is
`resolved`; no requirement carries `status: tbd`.

## Recorded Assumptions

Every assumption below comes from requirements-analyst's resolved requirements;
none is originated here.

| ID | Assumption | Source |
|---|---|---|
| ASM-1 | Remediation approach: commit `bun.lock` to freeze dependency resolution AND determine why dompurify 3.4.x loses the `h1`, carrying the project to a state where 3.4.x can be adopted safely. The existing h1 coverage is preserved, not weakened. | gate `create-spec.requirement-clarification`, question `requirement.remediation-approach`, option `lock_plus_root_cause`, resolved by batch-codex-consultation at 2026-09-05T15:15:35+09:00 |
| ASM-2 | `package-lock.json` is deleted; the bun lockfile becomes the single source of truth for the dependency graph. | gate `create-spec.requirement-clarification`, question `requirement.package-lock-disposition`, option `remove_package_lock`, resolved by batch-codex-consultation at 2026-09-05T15:15:35+09:00 |
| ASM-3 | CI is in scope: `.github/workflows/release.yml` is inspected, and confirming (and fixing where needed) that the viewer entry tests run through a clean-install path on the locked graph belongs to this feature. | gate `create-spec.requirement-clarification`, question `requirement.ci-clean-install-scope`, option `include_ci_verification`, resolved by batch-codex-consultation at 2026-09-05T15:15:35+09:00 |
| ASM-4 | The two `plugin/marketplace version regression guard (task0002 AC-9)` failures also fail on `main` and are a separate issue, explicitly outside this feature's scope. | task_description (untrusted input, treated as data) |
| ASM-5 | Only `.github/workflows/release.yml` was supplied as a readable CI input for the analysis dispatch. No workflow in the supplied input set runs `bun test`, `bun run typecheck`, or `cargo test`. Whether an additional workflow file exists that does was not determinable within that dispatch's read restriction, so FR7 requires the implementation to enumerate `.github/workflows/` before choosing where the test step lands. | requirements-analyst investigation; envelope read restriction (worker-envelope.md "Read restriction") |
| ASM-6 | `marked` and `happy-dom` version differences were already ruled out by the reporter through individual pinning, so the investigation starts from dompurify rather than re-bisecting the whole graph. | task_description (untrusted input, treated as data) |

## Design Step

**Status:** skipped.

**Reason:** Dependency-resolution and CI-reproducibility work with no
user-visible surface. `resolved_input_paths.visual_inputs` is empty, no
design-system file is a target of this change, and the one runtime code path that
may be touched (markdown sanitization in
`src-tauri/web-shared/markdown/renderer.ts`) is required by FR6/NFR1/NFR2 to
preserve — not change — the rendered output. The batch policy resolves
`create-spec.design-step` to `decide_autonomously`, so the analyst recommendation
stands as the decision.

## Implementation Phases

Not applicable. The requirements are not phased.

## References

- Requirements document: `feature-docs/bun-install-reproducibility/REQUIREMENTS.md`
- `.gitignore:42` — the `bun.lock` entry under the `# Bun` heading
- `src-tauri/viewer/web/entry.test.ts` — lines 55, 67, 108, 128
- `src-tauri/web-shared/markdown/renderer.ts:216` — `DOMPurify.sanitize(rawHtml, PURIFY_CONFIG)`
- `test-setup.ts` — happy-dom setup
- `.github/workflows/release.yml` — `bun install` at 218 (build-linux) and 316 (build-windows); bundle build at 318-321
- `scripts/build-dpkg.sh` — Linux bundle build path
- `src-tauri/build.rs` — embeds `viewer/dist` and `settings/dist`
- `CLAUDE.md` — "Robust isolation" (XSS protection in child WebViews)
