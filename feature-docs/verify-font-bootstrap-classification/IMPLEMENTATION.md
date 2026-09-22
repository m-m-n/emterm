# Implementation Plan: verify-font-bootstrap-classification

## Overview

The un-fetched scenario of `scripts/verify-font-bootstrap.sh` decides its
verdict from the exit status alone, so "the build stopped before any test ran"
and "the test suite ran and something failed" are reported with the same
false, count-hardcoded message. This plan moves that decision into a pure,
sourceable verdict helper that evaluates the executed-test count first,
returns one of three outcomes, and is exercised by an automated regression
test that never runs a real build.

## Technology Stack

- **Bash** — the verification script (`scripts/verify-font-bootstrap.sh`) and
  the extracted verdict helper. Same interpreter and same `set -uo pipefail`
  discipline the script already uses; no new shell feature is introduced.
- **Bun test runner** — the regression test, collected by the project's
  default `bun test` run (`bunfig.toml` preloads `test-setup.ts`; no test
  root is configured, so a test file next to the script under test is
  collected).
- **Key libraries**: none added.

### Dependency / license record (project license: MIT)

This feature introduces **no new dependency**. The verdict helper is a file in
this repository, and the regression test uses only the test runner and process
facilities the project already relies on (the same ones
`plugins/emterm/hooks/scripts/notify-status.test.ts` uses today). There is
therefore no third-party license to check against `project.license: MIT`, and
`project.license` is unchanged by this feature.

## Layer Structure

Three layers, with a strictly one-directional dependency:

| Layer | Element | Responsibility |
|---|---|---|
| Caller | `.github/workflows/ci.yml`, verify-font-bootstrap job | Invokes the script with no arguments. **Unchanged** (FR8). |
| Orchestration | `scripts/verify-font-bootstrap.sh` | Creates isolated trees, runs the literal reproduction command, captures the log and the exit status, maps the helper's outcome onto the reported lines and the pass/fail tally, cleans up. |
| Decision | `scripts/verify-font-bootstrap-verdict.sh` (new) | Pure decision: derives the executed-test count from a captured log and classifies the (count, status) pair. |

Allowed dependency directions:

- Orchestration depends on Decision (the script loads the helper).
- Decision depends on nothing: it reads only its two arguments and the log
  file they name, writes no file, spawns no build, reads no environment
  variable, and produces no side effect other than its own stdout.
- Nothing depends on Orchestration except the CI job's single invocation.

The helper file contains function definitions only — loading it must execute
no work and must not require a Rust toolchain, a network, a git repository or
a particular working directory. That property is what lets the regression test
drive the real shipped decision logic inside the bun CI job, which installs no
Rust toolchain (NFR1).

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| `count_executed_tests` (relocated into the verdict helper) | Derive the number of tests cargo actually executed from a captured log | **Pre**: exactly one argument, a readable path to a captured log file. **Post**: prints exactly one non-negative integer on stdout — the sum, across every `test result:` line in the log, of that line's passed count plus its failed count; prints 0 when the log holds no such line. The counting rule is preserved exactly as it stands today (FR10); only its location changes. | task0001 |
| `classify_unfetched_verdict` (new, in the verdict helper) | Classify the un-fetched scenario's outcome, evaluating the executed count before the exit status | **Pre**: exactly two arguments — the reproduction command's exit status as an integer, and a readable path to the captured log. **Post**: prints exactly one line on stdout and returns success. The line's first whitespace-delimited token is the outcome, one of `pass`, `warn`, `fail`; the remainder of the line is the message (empty for `pass`). Selects the branch from the derived executed count first, per the classification table below. No file is written, no process is spawned, no global state is mutated. | task0001 |
| Outcome-to-report mapping (in the script) | Turn an outcome token into reported lines and tally movement | **Pre**: one outcome line as produced above, plus the scenario name and the captured log's path. **Post**: `pass` → one PASS line, tally +1 pass; `warn` → one WARN line carrying the message, then the same PASS line and tally +1 pass, and the captured log's tail on stderr; `fail` → one FAIL line carrying the message, tally records a failure, and the captured log's tail on stderr. Total scenario count advances by exactly one in every case. | task0001 |

## Classification table (the decision the helper owns)

| Derived executed count | Exit status | Outcome token | Message content requirement | Requirement |
|---|---|---|---|---|
| 0 | non-zero | `fail` | Names a build stop, and states both the observed exit status and the derived executed count | FR2, FR5 |
| greater than 0 | non-zero | `warn` | States that tests executed and the run reported a failure; states both the observed exit status and the derived executed count; must not contain the build-stop wording | FR3, FR5 |
| 0 | 0 | `fail` | The existing wording `command exited 0 but 0 tests executed`, unchanged | FR4 |
| greater than 0 | 0 | `pass` | Empty | — |

The branch is selected by the derived executed count before the exit status is
consulted (FR1): the count, not the status, decides which row applies.

## Conventions

- **Output prefix**: every reported line keeps the script's existing
  `verify_font_bootstrap.`-prefixed shape, emitted through the script's
  existing logging helper. The verdict helper never prefixes anything itself —
  it returns an outcome and a message, and the script owns the prefix, the
  scenario name and the verdict keyword. This keeps a single place responsible
  for the log's shape.
- **Verdict keywords**: `PASS`, `FAIL` keep their current spelling and
  position. The warned branch introduces exactly one new keyword, on its own
  prefixed line, distinct from both (NFR5).
- **Summary line**: the aggregate line keeps its existing
  `summary: N/M scenarios passed` shape; the warned scenario is counted among
  the passed ones (FR6), so the aggregate exit status is success when the
  other two scenarios pass.
- **No hardcoded counts**: no message may state a test count that was not
  derived from the captured log for that very run (FR5). The current literal
  `0 tests executed` inside the build-stop message is replaced by the derived
  value.
- **Error-handling policy**: the script keeps `set -uo pipefail` and keeps
  *not* using `set -e`, so one scenario's failure never prevents the others
  from running and reporting (NFR2). The helper reports its classification
  through stdout and always returns success — a non-success return from the
  helper would be indistinguishable from a classified failure and is therefore
  never used as a signalling channel.
- **Cleanup policy**: unchanged (NFR3). The exit-time cleanup still removes
  every scenario worktree, prunes any stale worktree registration and removes
  the scratch directory, on success and on failure alike. Nothing in this
  feature adds a new resource that needs cleaning up.

## Cross-task Design Decisions

### D1 — The verdict decision is extracted into a sourceable helper file

**Decision**: the executed-count derivation and the new classification live in
a new file, `scripts/verify-font-bootstrap-verdict.sh`, which contains
function definitions only and is loaded by `scripts/verify-font-bootstrap.sh`.

**Rationale**: NFR1 requires the regression test to run inside the bun CI job,
which installs no Rust toolchain. The main script cannot be loaded for its
functions alone: its preconditions abort when cargo is absent from PATH, and
its top level allocates a scratch directory and installs an exit-time cleanup
hook. A helper file with no top-level work is loadable from anywhere, which
lets the test exercise the *shipped* decision logic rather than a copy of it.
The SPEC's assumption a4 names this option explicitly.

**Affected tasks**: task0001.

### D2 — The helper decides; the script reports and tallies

**Decision**: the helper returns an outcome token plus a message; it never
prints a prefixed line and never touches the tally. The script maps the
outcome onto its existing reporting and counting helpers.

**Rationale**: keeps the helper pure (and therefore testable without a
repository, a build or a network), and keeps exactly one owner for the log's
shape and for the pass/fail accounting. It also keeps the fetch-failure and
already-fetched scenarios' reporting path untouched (FR10).

**Affected tasks**: task0001.

### D3 — A warned un-fetched scenario counts as passed, and stays loud

**Decision**: the warned branch emits its own prefixed WARN line *and* the
captured log's tail on stderr, then counts toward the passed tally exactly as
a clean pass does.

**Rationale**: FR6/AC4 require the aggregate exit status to be success so the
job stops going red for an unrelated flake; NFR5 requires a warned run to stay
visibly distinct from a clean pass so a genuinely failing test is not silently
normalized away. Emitting the log tail — the same diagnostic a failing
scenario already produces — is what makes the warning actionable rather than
cosmetic.

**Affected tasks**: task0001.

### D4 — The literal reproduction command is not touched

**Decision**: the un-fetched scenario keeps invoking the literal reproduction
command byte-for-byte:
`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`.
No cargo flag and no test-harness flag is added — in particular no
test-thread-count flag.

**Rationale**: FR7 and the create-spec decision recorded as assumption a3.
Flake absorption comes from the classification change alone. Note that
`workflow.yaml`'s `project.components.rust.test_command` *does* carry a
single-threaded harness flag — that is this project's own verification
command, not the command the script reproduces. The two must not be
conflated, and no flag may be propagated from one to the other.

**Affected tasks**: task0001.

### D5 — The CI call site and the prior feature's SPEC are untouched

**Decision**: `.github/workflows/ci.yml` gains no step, loses no step and
keeps its exact one-line invocation; no cache step, no acquisition-script
reference and no opt-out environment variable is introduced.
`feature-docs/worktree-font-bootstrap/SPEC.md` is not edited, and
`.github/workflows/release.yml` stays byte-for-byte identical.

**Rationale**: FR8, FR10 and the create-spec decisions recorded as assumptions
a5 and a7. The AC1/FR6 reading this feature aligns with is recorded in this
feature's own SPEC and in the script's comments instead.

**Affected tasks**: task0001.

### D6 — The counting rule is relocated, never rewritten

**Decision**: when the executed-count derivation moves into the helper file,
its counting program is carried over character-for-character. Its behaviour on
every input — several `test result:` lines summed, a log with none, a suite
whose tests are all ignored — is identical before and after.

**Rationale**: FR10 pins the counting rule as unchanged. Relocation is what
makes it testable; rewriting it would be a silent behaviour change in the one
value the whole classification now depends on.

**Affected tasks**: task0001.

### D7 — The regression test drives the real helper as a subprocess

**Decision**: the regression test writes synthetic captured logs to a
temporary location, invokes the shipped helper as a real subprocess with a
chosen exit status and log path, and asserts on the outcome token and the
message. It never runs cargo, never reaches the network and never creates a
git worktree. It follows the subprocess-driven precedent already in the
repository (`plugins/emterm/hooks/scripts/notify-status.test.ts`).

**Rationale**: NFR1 and assumption a4. Driving the shipped file is what makes
the test a regression test rather than a test of a re-implementation.

**Affected tasks**: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| A genuinely broken library test is normalized away by the warned branch, and nobody notices | Medium | High | D3: the warned branch emits its own distinct line plus the captured log's tail; NFR5 is a verification item, and TS-4 pins the distinctness. |
| The helper extraction changes the counting behaviour by accident | Low | High | D6: the counting program is carried over character-for-character; an acceptance criterion and TS-6 pin its behaviour on multi-line, empty and all-ignored logs. |
| The script fails to locate its helper when invoked from a different working directory | Low | High | The script resolves the helper relative to its own file location, not to the working directory, and fails loudly with the script's existing usage-error path if it is missing. |
| A flag leaks from the project's rust test command into the reproduction command | Low | Medium | D4 states the distinction explicitly; TS-5 asserts the reproduction command is free of any test-thread flag. |
| The regression test is placed where the default test run does not collect it | Low | Medium | TS-8 asserts the test is collected by the default `bun test` invocation, with no argument naming the file. |
| Cleanup or scenario independence regresses while editing the script | Low | High | FR10/NFR2/NFR3 are verification items (TS-7); the edit is confined to the un-fetched scenario's verdict block plus the helper load line. |

## Open Questions

- [ ] None. Every requirement in `workflow.yaml` is `ok` or `assumed`
      (FR7 is `assumed`, resolved at create-spec: the literal reproduction
      command is preserved and no test-thread flag is added), and SPEC.md's
      Open Questions section is empty.
