# Implementation Plan: emterm --version flag

## Overview

Add a `--version` flag to the emterm binary and a `sync-version` job to the
GitHub release workflow. Two independent tasks with no shared components.

## Technology Stack

- **Language**: Rust (existing `src-tauri` crate) — no new dependencies.
- **CI**: GitHub Actions (existing `.github/workflows/release.yml`).

New dependencies: none (no license constraints triggered; project license MIT).

## Layer Structure

Unchanged. The flag lives in the binary entry dispatch layer
(`src-tauri/src/main.rs`), before any feature-gated GUI code. The CI change
is confined to the release workflow file.

## Shared Components

None — the two tasks touch disjoint files and share no contracts.

## Conventions

- The `--version` dispatch follows the existing pattern in `main()`: argument
  inspection before `logging::init()`, explicit process exit with a status
  code (same style as the bare-word subcommand dispatch).
- Workflow edits follow the file's existing step conventions: version
  resolution identical to the `get-version` step (input tag takes precedence
  over the git ref), env-var indirection for tainted inputs, `bash` shell.

## Cross-task Design Decisions

### D1: --version is handled in the pre-logging dispatch block

The check runs on the first argument before `logging::init()` and before any
`#[cfg(feature = "gui")]` code, so both the GUI and CLI-only builds share the
exact same behavior and no side effects occur. Affected: task0001.

Note: the known issue "unknown flags fall through to GUI startup" lives in
this same dispatch area and is explicitly out of scope — tasks must not
change behavior for any argument other than `--version`.

### D2: sync-version is a job inside release.yml, ordered via needs

The version bump must land before the release exists, and `needs:` is the
workflow-native way to express that ordering. `create-release` gains
`needs: sync-version`. The build jobs' existing build-time version stamping
stays untouched (the pushed commit is not what the tag builds from).
Affected: task0002.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| sync-version push rejected (branch protection) | low | release blocked | job fails loudly; release is not created (intended per REQUIREMENTS F02) |
| workflow_dispatch run without a usable tag/version | low | job error | resolve version exactly like get-version; skip commit when nothing changes |
| Cargo.lock drift after version rewrite | medium | broken `--locked` builds | task0002 updates the lockfile entry in the same commit |

## Open Questions

- [ ] None.
