# Implementation Plan: worktree-font-bootstrap

## Overview

The GUI build-time font check gains a bounded, explicitly opt-out-able bootstrap
step that satisfies the bundled-font prerequisite through the project's single
existing acquisition path, and a new verification script wired into CI proves
that the reported "zero tests run in a fresh worktree" regression cannot return
unnoticed.

## Technology Stack

- **Language / runtime**: Rust (the `src-tauri` build script), bash (the
  acquisition script and the new verification script), GitHub Actions workflow
  YAML (the CI job).
- **Key libraries**: none added. The build-time logic needs only facilities the
  Rust standard library already gives every build script (filesystem queries,
  environment reads, child-process execution).
- **New dependencies and their licenses**: **none**. No crate, package, or
  GitHub Action that the project does not already use is introduced. The CI job
  of FR7 reuses only actions already present in `.github/workflows/release.yml`
  (repository checkout, Rust toolchain install, Rust cache). Because no new
  dependency exists, there is no license question to resolve against
  `project.license: MIT`; this line is the record the license review
  perspective cross-checks.

## Layer Structure

Three layers, no new module boundaries and no new architectural element beyond
the build-time gate's new ability to invoke the acquisition layer.

| Layer | Owner | Responsibility | May depend on |
|-------|-------|----------------|---------------|
| Build-time gate | `src-tauri/build.rs` | Decide whether the bundled fonts are present, whether fetching is permitted, and whether the build stops. Emits the diagnostic context and the human-facing actionable message. | The acquisition layer (as a child-process contract) and the opt-out switch |
| Acquisition | `scripts/fetch-fonts.sh` | The sole writer of font bytes. Used unchanged. | Nothing in this feature |
| Verification / automation | `scripts/verify-font-bootstrap.sh`, `.github/workflows/ci.yml` | Observe the two layers above from outside and fail when the reported defect is present. | Both layers above, through their published contracts only |

The dependency direction is one-way: automation → build-time gate → acquisition.
The acquisition layer knows nothing about the gate, and the gate knows nothing
about the verification script.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Font acquisition entry point — `scripts/fetch-fonts.sh` | The only mechanism permitted to write font bytes (FR5) | **Pre**: launched through a `bash` interpreter resolved from the build process's `PATH`, with the script supplied as an argument and no further arguments; the destination directory is supplied through the script's `DEST_DIR` environment variable as an absolute path in a representation the host's bash accepts, overriding any inherited value; the child's working directory is the worktree root. A download tool and a SHA256 tool must be on `PATH`, and the script also relies on the ordinary POSIX text and file utilities. **Post**: exit 0 ⇒ every one of the seven bundled fonts is present under `DEST_DIR` and SHA256-matches its pinned value; non-zero exit ⇒ no partial or unverified file is left under `DEST_DIR`. Present-and-matching files are skipped without network access. **Invariant**: this file is not modified by any task. | task0001 (invokes it), task0002 (observes its effects) |
| Opt-out switch — `EMTERM_SKIP_FONT_FETCH` | Forbids the automatic fetch outright, on every host (FR3) | **Pre**: read from the build script's own process environment, and registered as a cargo rerun input so a change of its value re-runs the gate. The value `1` means engaged; unset, empty, or any other value means not engaged. **Post**: when engaged, zero invocation attempts occur on any platform, and a missing font produces the actionable stop below with no network access whatsoever. | task0001 (reads it), task0002 (engages it in one scenario), task0003 (must not set it) |
| Actionable stop message | The stable human-facing tail of every failure presentation (FR4) | The two lines already emitted today, unchanged in wording and order: first `build_rs.font_missing: bundled font missing at ` followed by the offending path; second the line instructing the reader to run `make fetch-fonts` (or `bash scripts/fetch-fonts.sh`) to download bundled fonts. **Post**: both lines appear verbatim in the build's output on every stop path, as literal substrings the verification script matches. Failure-shape diagnostic context (D8) precedes them; on the "bash could not be launched" shape one additional line follows them, stating that bash must be made available first. No stop path replaces or reorders the two lines. | task0001 (emits it), task0002 (asserts on it) |
| Bootstrap-verification entry point — `scripts/verify-font-bootstrap.sh` | Runs the three bootstrap scenarios and reports a single pass/fail (FR6) | **Pre**: invoked as `bash scripts/verify-font-bootstrap.sh` from the repository root, with no arguments and no required environment variables; the host has a Rust toolchain, git, the GUI build's system libraries, download + SHA256 tooling, and network access. **Post**: exit 0 if and only if every scenario passed; a non-zero exit otherwise, with the failing scenario named in the output under a stable identifier. It is long-running: it performs real cargo builds and at least one real download. It never requires elevated privileges and never writes outside the repository's ignored paths and its own scratch area. | task0002 (creates it), task0003 (invokes it) |

## Conventions

- **Diagnostic prefixes**: the project already uses `build_rs.` for build-script
  diagnostics and `fetch_fonts.` for acquisition diagnostics. New diagnostics
  emitted by the verification script use a third, distinct prefix
  (`verify_font_bootstrap.`) so that a failure can be attributed to a layer by
  reading one line.
- **Error-handling policy for the build-time gate**: every stop condition ends
  the build with the same actionable message above; none is downgraded to a
  warning. What differs between stop conditions is only the diagnostic context
  printed **before** that message, which names the failure shape (D8) and
  carries the underlying cause. No alternative or competing actionable message
  is introduced.
- **Single-acquisition-path rule (FR5)**: no task may add a second way to
  obtain font bytes. In particular, the fetch-failure scenario induces failure
  **by environment manipulation only** — removing the download or hashing
  tooling from the child process's `PATH`, or making the upstream host
  unreachable — and never by substituting, stubbing, copying, or shadowing the
  acquisition script with a different one.
- **Scope-boundary rule (FR8)**: the handling of `viewer/dist`,
  `settings/dist` and `icons/` inside the build script is not touched by any
  task. Their existing behavior (a build warning in a debug profile, a stop
  only in a release profile) stays exactly as it is.

## Cross-task Design Decisions

### D1 — Destination pinning is derived from the manifest location, never the cwd

The building worktree's own font directory is derived from the build script's
manifest location: the crate manifest directory is the `src-tauri` directory of
the worktree being built, and the font destination is its `assets/fonts`
subdirectory. The acquisition script's own default destination is a
cwd-relative path, so the gate always supplies an **absolute** destination
through the script's `DEST_DIR` contract, overriding whatever the environment
may already carry, rather than relying on that default. The acquisition script
itself is located by the same rule (the manifest directory's parent, then its
`scripts` directory), and the child process's working directory is set
explicitly to the worktree root rather than inherited implicitly. The
destination is passed in a representation the host's bash accepts, so a Windows
host with a Git-Bash-style interpreter receives a usable path rather than one
bash will reject.

*Rationale*: NFR4 requires that sibling worktrees never write into each other's
trees and that no shared or cwd-dependent destination is used. Deriving both the
destination and the script location from the manifest makes the invariant
structural instead of incidental.
*Affected tasks*: task0001 implements it; task0002 verifies it (a build driven
from an arbitrary working directory must still land the fonts in the building
tree and leave a sibling tree untouched).

### D2 — Exactly one acquisition attempt per build-script run

The gate checks all seven paths first, and makes **at most one** invocation of
the acquisition script per build-script run regardless of how many fonts are
missing. After a successful invocation it re-checks all seven paths and stops
with the actionable message if any is still absent. There is no retry loop, no
per-font invocation, and no second attempt after a failure.

*Rationale*: FR2 states the single-invocation requirement explicitly; the
acquisition script is already idempotent and already fetches the whole set, so
per-font invocation would multiply network work without adding coverage. A
retry loop would also make the fetch-failure path's timing unbounded.
*Affected tasks*: task0001 implements it; task0002 asserts the failure path
stops rather than retrying.

### D3 — The opt-out forbids, it does not merely tolerate

When the opt-out is engaged and any font is missing, the gate stops immediately
with the actionable message **before** any acquisition attempt is prepared, so
no network syscall is reachable on that path. Engaged opt-out plus all seven
fonts present is an ordinary successful build: the opt-out only suppresses the
attempt, it never fails a build that would otherwise have succeeded. The switch
takes effect on every host, including Windows.

*Rationale*: FR3's business rule is "do not attempt", not "tolerate failure";
NFR7 relies on packaging builds being able to forbid an implicit fetch.
*Affected tasks*: task0001 implements it; task0002 engages it in one scenario;
task0003 must leave it unset so the CI job exercises the automatic path.

### D4 — Assumption a4 resolved: attempt the invocation on every host

**This is a fixed decision, not an open question.** The gate attempts the
acquisition invocation on every host, with no platform-conditional skip and no
interpreter-resolvability preflight. It does not implicitly engage the opt-out
on Windows. When the interpreter cannot be launched, that is an ordinary
failure shape (D8 shape 1) and produces the actionable stop — which is exactly
today's behavior and therefore not a regression.

*Rationale*:
1. A bash-presence preflight cannot establish readiness anyway. The acquisition
   script depends on more than the interpreter — a SHA256 tool, the usual text
   and file utilities, and a download tool — so a presence check would add a
   branch without closing the gap it appears to close.
2. A Windows developer who does have a working Git-Bash-style environment would
   be denied automatic bootstrapping under an implicit skip, for no gain.
3. The release workflow's Windows job already runs the acquisition script
   explicitly, with a font cache, before building, so the automatic path is a
   no-op there under either option.

Uniform behavior also leaves exactly one code path to reason about and review.
*Affected tasks*: task0001 (behavior and the four failure shapes of D8),
task0003 (the CI job runs on Linux and encodes no platform exception).

### D5 — Rerun conditions are declared deliberately, not left to chance

The gate declares cargo rerun conditions so that (a) the build script re-runs
when any of the seven font files changes, and (b) it re-runs when the opt-out
switch's value changes. The declarations covering the seven font paths are
emitted on **every** gate execution that reaches the check — including the runs
that stop — rather than being emitted incrementally as each individual
existence check passes, so that the set of declared inputs does not depend on
how far the check got. Once all seven fonts are present, the declared input set
is stable and identical between consecutive runs.

*Rationale*: NFR6 must be satisfied deliberately (SPEC assumption a7). Writing
the very files the build script declares as its inputs interacts with cargo's
change tracking; declaring the full, invariant input set and an explicit
environment dependency is what makes the post-fetch state converge instead of
re-triggering on every subsequent invocation.
*Affected tasks*: task0001 implements it; task0002's already-fetched scenario
and the feature-wide TS8 verify convergence.

### D6 — Isolation requirement for the verification script

Each scenario the verification script runs starts from a tree whose font
directory is **empty**, obtained without restoring any cache and without
copying font bytes from the primary checkout, except for the already-fetched
scenario, which populates the fonts through the acquisition path itself before
the measured build. The mechanism for obtaining the isolated tree (an
additional git worktree, a clean copy, or a scratch clone) is left to the
implementing task; what is contractual is that the primary checkout's own font
directory is never pre-populated for the un-fetched scenario, that a scenario
never observes another scenario's leftovers, and that nothing the script
creates survives a successful run.

*Rationale*: FR6 and FR7 both hinge on "no cache restore, isolated, un-fetched";
a scenario that silently inherits fonts would pass while the defect is present.
*Affected tasks*: task0002 implements it; task0003 must not add a font-cache
restore step that would defeat it.

### D7 — The CI job proves the check actually runs

The new CI job runs on the same push and pull-request triggers as the existing
bun job and therefore needs no trigger change at the workflow level. It supplies
what the repository's CI does not have today — a Rust toolchain and the system
libraries the GUI build links against — and then invokes the verification
entry point at its contracted path. It restores no font cache. The existing bun
job and the release workflow are left untouched.

*Rationale*: FR7's stated failure mode is a check that exists but never runs;
and the GUI-featured build the reported command performs cannot link on a bare
runner without the desktop system libraries the release workflow's Linux job
already installs.
*Affected tasks*: task0003 implements it; task0002 owns the entry point it
invokes.

### D8 — Four failure shapes stay distinguishable

Because the gate now launches an external process, its failures are no longer
one thing. The build's output distinguishes these four shapes rather than
collapsing them into a single "fetch failed":

| # | Shape | What the diagnostic must carry |
|---|-------|--------------------------------|
| 1 | The interpreter could not be launched at all (e.g. bash absent from `PATH`) | The underlying launch error, plus a statement that bash must be made available first |
| 2 | Some other launch error occurred (permission denied, script path unreadable, and similar) | The underlying launch error |
| 3 | The acquisition script ran and exited non-zero | The exit status, with the script's own error output preserved rather than swallowed |
| 4 | The acquisition script exited zero but at least one of the seven fonts is still missing | Which path is still missing |

On shape 1 the actionable message's instruction to run the acquisition command
cannot by itself resolve the situation — repeating it cannot conjure the
interpreter it needs — so that shape, and only that shape, adds one further
line stating that bash has to be made available first. The two lines of the
actionable message itself are still emitted verbatim on all four shapes, so
that every literal-substring assertion holds on every path.

*Rationale*: "the build stopped" without the reason is what made the original
report ambiguous in the first place; a swallowed child-process stderr would
reproduce exactly that ambiguity one layer down. This is also what makes TS3's
assertion meaningful: the fetch-failure scenario can require the shape as well
as the message.
*Affected tasks*: task0001 implements all four shapes; task0002 asserts on
shape 3 (and on the actionable message's two lines) in its fetch-failure
scenario.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Post-fetch rebuild does not converge and every later cargo invocation re-runs the build script | Medium | High — turns a one-time cost into a permanent one | D5's deliberate rerun-condition declaration; TS8 verifies convergence explicitly rather than assuming it |
| Two cargo invocations in the same worktree race on the acquisition script and both download | Medium | Low — the acquisition script verifies each file and moves it atomically, so the outcome is correct but duplicated work | Accepted. The atomic-move behavior of the acquisition path (used unchanged) is what keeps the destination free of partial files; no locking is introduced, which would be a second mechanism |
| A Windows GUI build without a usable bash environment now stops on shape 1 where the failure used to be a plain missing-font stop | Medium | Low | D4 accepted this deliberately; D8 shape 1 makes the cause explicit and names the actual remedy, so the outcome is strictly more informative than today's |
| The verification script is slow enough (real builds plus a real download) that CI turnaround suffers | High | Medium | Accepted as the cost of FR7. The job is independent of the existing bun job, so it never delays it; caching the Rust build (not the fonts) is permitted and expected |
| An eighth bundled font becomes required later and the seven-path set silently drifts from the acquisition script's set | Low | Medium | The two sets are already bound by the same failure surface: a font the script does not fetch cannot satisfy the gate, so D8 shape 4 fires loudly rather than degrading |
| The fetch-failure scenario's environment manipulation masks a genuine defect (e.g. the tooling removal also breaks the build for an unrelated reason) | Medium | Medium | The scenario asserts on the actionable message's literal text and on failure shape 3, not merely on a non-zero exit, so an unrelated failure does not satisfy it |
| The non-hermetic build script surprises a downstream packager who does not know about the opt-out | Medium | Medium | The opt-out is a stable switch named in this plan and asserted by TS2; the actionable message names the manual acquisition command on every stop |

## Open Questions

- [ ] Documenting `EMTERM_SKIP_FONT_FETCH` in the project's own command rules
      (`.claude/rules/core-commands.md`) is deliberately **not** included in any
      task, because FR8 limits this change to the font bootstrap plus the CI
      wiring. If the reviewers judge that an undocumented build-affecting
      environment variable is itself a defect, it should be raised as a
      follow-up rather than absorbed into these tasks.
- [ ] The verification script's already-fetched scenario asserts "no network
      access" behaviorally (the acquisition path reports every file as
      up-to-date and no download line is produced). Asserting it at the syscall
      or sandbox level would need a network-isolation facility the project does
      not currently have; if CI later gains one, the assertion can be
      strengthened.
