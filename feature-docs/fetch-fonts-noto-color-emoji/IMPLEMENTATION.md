# Implementation Plan: fetch-fonts-noto-color-emoji

## Overview

Retire the `swash_emoji` example together with the two Cargo-manifest
declarations that exist only for it, so the documented test command compiles on
a clean checkout, and add a text-level asset-manifest consistency check that
fails whenever a font embedded at compile time is missing from the fetch script
or from the font inventory table. Shipped behavior and the bundled font set are
unchanged.

## Technology Stack

- **Language**: Rust — the `emterm` crate rooted at `src-tauri/Cargo.toml`.
- **Test target**: an auto-discovered integration test binary under
  `src-tauri/tests/`. The manifest declares no explicit test sections, so a new
  file in that directory is picked up by the documented test command and builds
  independently of the `gui` feature.
- **New dependencies**: none. This feature adds no crate, so no license
  evaluation applies against the project license (`MIT`).
- **Removed dependency**: `png` (dev-dependency, version 0.17) — MIT OR
  Apache-2.0, permissive, previously compatible; removed because the only
  consumer is the retired example. The unrelated `png` *feature* of the `image`
  crate stays as it is.

## Layer Structure

| Layer | Location | Role in this feature |
|-------|----------|----------------------|
| Build configuration | `src-tauri/Cargo.toml` (+ its lockfile) | Declares example targets and dev-dependencies. Two declarations are removed. |
| Examples | `src-tauri/examples/` | Ad-hoc local probes, built by the documented test command. One retired probe is deleted; the three remaining probes are untouched. |
| Integration tests | `src-tauri/tests/` | Feature-agnostic test binaries. The new consistency check lives here. |
| Production | `src-tauri/src/` | Read as text by the new check; never edited by this feature. |

Allowed dependency direction for the new check: it reads repository text files
only. It must not reference any module behind the `gui` feature gate, and must
not depend on font binaries being present on disk.

## Shared Components

None. This feature is planned as a single task (decision D4 below), so no
component contract crosses a task boundary. The contracts that matter —
scanner input/output, declaration lookup, failure reporting — are internal to
`task0001` and are stated in its task plan.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| (none) | — | — | — |

## Conventions

- **Failure reporting**: every violation names the offending font file and
  states which of the two declarations is missing (fetch script entry,
  inventory row, or both). A failure must be readable without opening the test
  source.
- **Scan scope**: repository text under the crate root, with build directories
  (`src-tauri/target`, `src-tauri/target-host`, `src-tauri/target-win` and any
  other sibling build output directory sharing the `target` prefix) excluded,
  and with the checking test's own source excluded from its own scan.
- **Path resolution**: all paths the check reads are resolved from the crate
  manifest directory supplied by the build environment, never from the
  process's current directory and never from an absolute developer-machine
  path.
- **No production edit**: nothing under `src-tauri/src/`, `scripts/`,
  `src-tauri/assets/fonts/`, `Makefile`, `doc/` or `.github/workflows/` is
  modified.

## Cross-task Design Decisions

### D1 — Fix by deleting the retired probe, not by fetching another font

The failing embed belongs to a probe whose subject (bitmap color-emoji strikes)
no longer exists in the product; the recorded migration replaced that font with
the outline-color one. Adding the missing font to the fetch script would
re-introduce roughly five megabytes that the migration deliberately shed, on
every machine and every run, purely to compile a retired probe. Affected
requirements: FR1, FR2, FR3.

### D2 — Regression detection is a textual invariant inside the existing test surface

The check asserts a declaration invariant over repository text; it does not
prove that the crate compiles. That limitation is accepted deliberately, in
exchange for needing no network access, no desktop toolkit packages, no font
binaries and no new continuous-integration job. Affected requirements: FR4,
FR7, NFR1, NFR4.

### D3 — Placement under the crate's integration test directory

The check is placed in the crate's integration test directory rather than as a
unit test beside the font resolver, because the resolver module sits behind the
`gui` feature gate and would not build in the feature-off configuration. A file
in the integration test directory is auto-discovered and is feature-agnostic.
Affected requirements: FR5, NFR2.

### D4 — One task, because the check and the deletion cannot be green apart

Tasks are implemented fully in parallel with no ordering between them, and each
task must be able to reach a green state inside its own worktree. The retired
probe embeds a font that is declared in neither the fetch script nor the
inventory table, so in any worktree where the probe still exists the new check
fails by construction. Splitting "delete the probe" and "add the check" into two
tasks would therefore produce a task that cannot pass its own acceptance
criteria. The two changes are planned as a single task instead.

### D5 — Separate the comparison from the file reading

The check is structured as two responsibilities: (a) deriving three sets from
supplied text — embedded font file names, fetch-script declarations, inventory
rows — and (b) obtaining that text from the repository. Keeping the comparison
independent of file access lets the negative scenarios (a font missing from the
fetch script, a font missing from the inventory) be exercised with synthetic
text instead of by mutating tracked files, while the repository-facing part
asserts the current tree's real invariant. Affected requirements: FR4, FR6, and
test scenarios TS2 / TS3.

### D6 — One-directional assertion only

Only the direction "every embedded font is declared" is asserted. The reverse
direction (every fetched font is still embedded) is deliberately out of scope,
even though the correspondence happens to be one-to-one today. Affected
requirement: FR6.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The removed dev-dependency turns out to have another consumer | Low | Build failure on the feature branch | The implementer compiles the crate with default features and with features off after the removal; a surviving consumer surfaces immediately as a compile error. |
| The scanner matches its own source and reports a phantom font | Medium | Test fails on an untouched tree | The scan excludes the checking test's own source; a scenario covers exactly this case. |
| The scanner walks build output directories | Medium | Non-deterministic result, slow test | Build output directories are excluded by the scan-scope convention; a scenario covers a populated build directory. |
| A non-font embedded asset is treated as a font | Low | False failure | The predicate matches only literals ending in the two font extensions; a scenario covers a non-font embed. |
| The failure message does not identify the font | Low | The next occurrence is as hard to diagnose as this one was | Message content is an explicit acceptance criterion, not an implementation detail. |
| Lockfile churn from the dependency removal | High | Noise in the diff | The lockfile is declared in the task's file set up front, so the change is expected rather than a deviation. |

## Open Questions

- [ ] FR8 and NFR4 are documentation requirements already satisfied by SPEC.md
      text; they have no implementing task and no automated scenario. Confirm
      that document-level confirmation during verification is sufficient.
- [ ] FR7 is a negative constraint (no new continuous-integration job, the
      workflow file untouched). It is owned by the single task but has no
      dedicated test scenario; it is verified through the success-criteria
      checklist instead.
- [ ] The two remaining probes that read absolute developer-machine font paths
      are recorded as a known follow-up and are intentionally left untouched
      here; filing that follow-up is outside this feature.
