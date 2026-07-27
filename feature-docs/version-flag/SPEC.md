# Feature: emterm --version flag

## Overview

Add a `--version` flag to the `emterm` binary that prints `CARGO_PKG_VERSION`
to stdout and exits 0, in both the GUI build and the CLI-only build. Also add
a version-sync job to the GitHub release workflow that, on tag push, commits
the tag's version into the repository before the release is created.

## Objectives

- Give the binary a way to report its own version.
- Keep the committed `src-tauri/Cargo.toml` version in sync with release tags.

## Accepted Constraints (from the task)

- The Cargo.toml version is a hand-written string.
- The displayed version does not identify the actual build between tags; this
  is explicitly accepted.

## Technical Requirements

### Functional Requirements

- **FR1:** `emterm --version` prints the value of `env!("CARGO_PKG_VERSION")`
  followed by a newline to stdout and exits with status code 0.
- **FR2:** The `--version` handling works identically in the GUI build
  (default features) and the CLI-only build (`--no-default-features`). It is
  dispatched in `src-tauri/src/main.rs` before `logging::init()` and before
  any GUI startup — no event loop, no window, no logger, no settings load.
- **FR3:** The release workflow (`.github/workflows/release.yml`) gains a
  `sync-version` job that runs on tag push (`v*`) before `create-release`:
  it checks out the default branch (`main`), rewrites the `[package]` version
  in `src-tauri/Cargo.toml` to the tag version (with the `v` prefix
  stripped), updates the `emterm` package entry in the workspace lockfile
  `Cargo.lock` (repo root), and commits and pushes the change to `main`. When the version already
  matches, the job succeeds without committing. `create-release` depends on
  `sync-version`.

### Non-Functional Requirements

- **NFR1 - Isolation:** `--version` performs no side effects (no log file,
  no config read, no window). Output is exactly the version string + `\n`.
- **NFR2 - Workflow compatibility:** The existing build jobs' build-time
  `sed` version stamping stays unchanged; `sync-version` must not break the
  `workflow_dispatch` path (when no tag ref is available, the job is skipped
  or resolves the version from the `tag` input the same way `get-version`
  does).

## Implementation Approach

### Dispatch (FR1/FR2)

`src-tauri/src/main.rs` `main()` currently dispatches bare-word subcommands
(`markdown` / `json` / ... / `mux`) from `args[1]` before `logging::init()`.
Add a check for `args[1] == "--version"` in the same dispatch block:

- Print `env!("CARGO_PKG_VERSION")` via `println!`.
- `std::process::exit(0)`.

This code path is outside any `#[cfg(feature = "gui")]` gate, so it is shared
by both builds. Note: this is the same dispatch area referenced by the known
issue "unknown flags fall through to GUI startup instead of a CLI error" —
that issue is out of scope and must not be fixed here.

### Release workflow (FR3)

Add a `sync-version` job to `.github/workflows/release.yml`:

- Trigger context: the workflow already runs on `push: tags: v*` and
  `workflow_dispatch`. The job resolves the version exactly like the
  existing `get-version` step (input tag wins over `github.ref`).
- Steps: checkout `main` (not the tag), rewrite the version with the same
  `sed` expression the build jobs use, update the workspace root `Cargo.lock`
  entry for the package whose manifest is `src-tauri/Cargo.toml` (e.g.
  `cargo update --workspace` scoped to that package, or an equivalent
  targeted edit), then `git commit` + `git push` using the workflow's
  `GITHUB_TOKEN` (the workflow already has `permissions: contents: write`).
- Idempotency: if `git diff` is empty after the rewrite, skip commit/push
  and succeed.
- Ordering: `create-release: needs: sync-version` so the release is created
  only after the version bump has been pushed.
- The tag itself is never moved or recreated.

### File Structure

```
src-tauri/src/main.rs           # --version dispatch (FR1, FR2)
.github/workflows/release.yml   # sync-version job (FR3)
```

## Test Scenarios

### Unit / Integration Tests

- [ ] TS-1: Running the binary with `--version` prints exactly
  `CARGO_PKG_VERSION` + newline to stdout and exits 0 (integration test in
  `src-tauri/tests/`, alongside the existing `cli_subcommands.rs` patterns).
- [ ] TS-2: stderr is empty and no log file is touched when `--version` runs.

### Build-Gate Checks

- [ ] TS-3: `cargo check` passes with default features.
- [ ] TS-4: `cargo check --no-default-features` passes (CLI-only build keeps
  the flag).

### Workflow Checks (static — CI runs are not executable locally)

- [ ] TS-5: `release.yml` parses as valid YAML; `sync-version` exists and
  `create-release.needs` includes it.
- [ ] TS-6: `sync-version` resolves the version identically to `get-version`
  (input tag precedence) and pushes only when the rewrite produced a diff.

### E2E Tests

**Existing E2E tests**: None applicable (feature has no UI).

### Edge Cases

- [ ] `--version` passed together with other args (`emterm --version foo`):
  `--version` in `args[1]` wins; later args are ignored.
- [ ] Tag whose version equals the committed version: `sync-version` makes
  no commit and the release proceeds.

## Security Considerations

- The `sync-version` job pushes with the ephemeral `GITHUB_TOKEN` under the
  already-declared `contents: write` permission; no new secrets.
- Tag-derived version strings are used inside `sed`/file edits in CI; the
  existing `${VAR#v}` handling pattern is reused as-is.

## Success Criteria

- [ ] FR1/FR2 implemented and covered by tests TS-1..TS-4.
- [ ] FR3 implemented; TS-5..TS-6 verified.
- [ ] Existing tests keep passing.

## Assumptions

Recorded per batch mode (no user to ask; Codex CLI unavailable in this
environment, so all decisions were made by the orchestrating agent):

- **A1:** Output is the bare version string (e.g. `0.1.0`) — no binary-name
  prefix, matching the task's literal wording ("`CARGO_PKG_VERSION` を
  stdout に出して").
- **A2:** No `-V` short alias (not requested).
- **A3:** FR3 is implemented as a job inside the existing `release.yml`
  (not a separate workflow file), because the task requires strict ordering
  "before the release is created" and `needs:` expresses that directly.
- **A4:** The push target is the default branch `main`; the tag is not
  re-pointed (the task explicitly accepts the resulting version skew).
- **A5:** The workspace root `Cargo.lock` is updated together with
  `src-tauri/Cargo.toml` so the committed lockfile stays consistent. (The
  tracked `src-tauri/Cargo.lock` is a stale pre-workspace leftover carrying
  the old `emterm-native-poc` package name; cargo does not read it and it is
  not a sync target.)

## Open Questions

None.

## References

- Notion task: emterm --version を実装する
  (https://www.notion.so/3a83509ec8ee81e38bb6d3741ffdbb17)
- REQUIREMENTS.md: feature-docs/version-flag/REQUIREMENTS.md
