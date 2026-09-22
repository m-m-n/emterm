# Feature: worktree-font-bootstrap

## Overview

A freshly created `git worktree add` checkout has no bundled font binaries, so
`src-tauri/build.rs` aborts the build and zero tests run. This feature makes the
GUI build bootstrap the bundled fonts automatically through the single existing
acquisition path, `scripts/fetch-fonts.sh`, while keeping an explicit opt-out
that forbids the fetch outright, and adds a bootstrap-verification script wired
into CI so the regression is caught automatically.

The full requirement text, in Japanese, lives in
`feature-docs/worktree-font-bootstrap/REQUIREMENTS.md`; this document is the
implementation-focused rendering of the same requirements.

## Objectives

- A fresh `git worktree add` checkout is immediately testable:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  runs the library test suite instead of aborting in `build.rs` with zero tests
  executed.
- The bundled-font prerequisite is satisfied through one canonical,
  already-verified acquisition path (`scripts/fetch-fonts.sh`) rather than a
  second, divergent mechanism.
- The regression is detected automatically: a check that exercises the reported
  command from an un-fetched state runs on every push and pull request, where
  today no Rust job runs in CI at all.

## Current state

`build.rs`'s `check_bundled_fonts()` already prints the actionable message, so
the "stop with guidance" half of the report is already satisfied. The live
defect is only that zero tests can run in a fresh worktree.

## User Stories

### US1: Test a fresh worktree without a manual prerequisite step

As a developer working in a freshly created `git worktree add` tree, I want
`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
to run the library test suite, so that the checkout is immediately testable.

**Acceptance Criteria:**
- [ ] AC1 — In a freshly created `git worktree add` tree with no font binaries
      and network available, the command above completes and reports a non-zero
      number of executed tests; the reported reproduction no longer reproduces.
- [ ] AC2 — The same command in the same tree leaves the seven bundled fonts
      present in that worktree's `src-tauri/assets/fonts`, and no font file is
      written anywhere outside that worktree.
- [ ] AC5 — With all seven fonts already present and hash-matching, a build
      performs no network access and does not re-download.
- [ ] AC6 —
      `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
      succeeds in a fresh un-fetched worktree and triggers neither the font
      check nor a fetch.

### US2: Build offline without any implicit network access

As someone running an offline, sandboxed or packaging build, I want an explicit
opt-out that forbids the automatic fetch, so that the build never reaches the
network and stops with actionable guidance instead.

**Acceptance Criteria:**
- [ ] AC3 — With the opt-out engaged and fonts missing, the build stops without
      any network access and its output contains
      `build_rs.font_missing: bundled font missing at` and
      ``Run `make fetch-fonts` (or `bash scripts/fetch-fonts.sh`) to download bundled fonts.``
- [ ] AC4 — When the fetch attempt fails (non-zero exit: unreachable host, hash
      mismatch, or missing tooling), the build stops with the same message and
      `src-tauri/assets/fonts` contains no partial or unverified file.
- [ ] AC9 — This document states the accepted non-hermeticity trade-off (NFR1)
      and its mitigations (HTTPS-only, SHA256-pinned, idempotent, explicit
      opt-out) in plain text.

### US3: Catch the regression automatically in CI

As the project's CI, I want a bootstrap-verification script that reproduces the
un-fetched situation and runs on every push and pull request, so that the
regression cannot return unnoticed.

**Acceptance Criteria:**
- [ ] AC7 — The bootstrap-verification script exists, exercises the un-fetched,
      fetch-failure and already-fetched paths, and fails when the reported
      defect is reintroduced.
- [ ] AC8 — `.github/workflows/ci.yml` contains a job invoking that script on
      the same push / pull_request triggers as the existing bun job, with no
      font-cache restore in the fresh-state scenario, and the job passes on the
      feature branch.
- [ ] AC10 — No change is made to `viewer/dist`, `settings/dist` or `icons/`
      handling in `build.rs`.

## Technical Requirements

### Functional Requirements

- **FR1 — Preserve the GUI-gated bundled-font presence check:**
  `src-tauri/build.rs` continues to detect missing bundled fonts only when
  `CARGO_FEATURE_GUI` is set, over the same seven relative paths
  (`assets/fonts/Noto-COLRv1.ttf`, `NotoSansCJKjp-Regular.otf`,
  `NotoSansCJKjp-Bold.otf`, `NotoEmoji-Regular.ttf`,
  `Inconsolata-Regular.otf`, `Inconsolata-Bold.otf`,
  `NotoSansSymbols2-Regular.ttf`). The `--no-default-features` CLI build keeps
  returning before the check runs.
- **FR2 — Automatic fetch attempt on detected absence:** When the check of FR1
  finds at least one missing bundled font and the opt-out of FR3 is not engaged,
  `build.rs` invokes `scripts/fetch-fonts.sh` exactly once per build-script run,
  with the destination pinned to the building worktree's own
  `src-tauri/assets/fonts` directory (derived from the build script's manifest
  location, never from the process cwd and never a shared or cross-worktree
  location).
- **FR3 — Explicit opt-out that forbids the fetch:** An explicit opt-out exists
  that forbids the automatic fetch outright, so offline, sandboxed and packaging
  builds never reach the network. When it is engaged and fonts are missing,
  `build.rs` performs no fetch attempt and stops immediately with the actionable
  message of FR4. The concrete mechanism this spec fixes is described under
  "Opt-out mechanism" below.
- **FR4 — Stop with the existing actionable message on failure:** If the fetch
  attempt exits non-zero, or if any of the seven fonts is still missing after
  the attempt, or if the opt-out of FR3 is engaged, `build.rs` stops the build
  with the message already implemented today:

  ```
  build_rs.font_missing: bundled font missing at {path}
  Run `make fetch-fonts` (or `bash scripts/fetch-fonts.sh`) to download bundled fonts.
  ```

- **FR5 — `scripts/fetch-fonts.sh` as the single acquisition path:**
  `scripts/fetch-fonts.sh` is the only font-acquisition mechanism, used both for
  the automatic attempt of FR2 and as the command named in the human-facing
  message of FR4. No second acquisition mechanism is introduced, and the
  script's HTTPS-only, per-file SHA256-pinned, idempotent, atomic-move behavior
  is used unchanged.
- **FR6 — Bootstrap-verification script:** A bootstrap-verification script is
  added that reproduces the reported situation from an isolated un-fetched state
  with no cache restore, runs the reported command
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`,
  and fails when the library test suite does not execute. It also covers the
  fetch-failure path (a clear stop carrying the FR4 message) and the
  already-fetched path (no network access).
- **FR7 — CI wiring for the bootstrap-verification script:**
  `.github/workflows/ci.yml` gains a new job that runs the FR6 script on the
  same push / pull_request triggers as the existing bun job, so the check
  actually runs. The job must not restore a font cache for the fresh-state
  scenario.
- **FR8 — Scope boundary:** The change is limited to `src-tauri/assets/fonts/`
  bootstrapping plus the CI wiring FR7 requires. `viewer/dist`,
  `settings/dist` and `src-tauri/icons/` are out of scope; `build.rs` already
  degrades gracefully for the first two (`cargo:warning` in a debug profile,
  panic only when `PROFILE == "release"`) and they do not block
  `cargo test --lib`.

### Non-Functional Requirements

- **NFR1 — Accepted non-hermeticity, stated openly:** A build script that
  reaches the network is non-hermetic and writes outside `OUT_DIR`, against
  Cargo's own guidance. This trade-off is accepted because the definition of
  done requires the literal reproduction command to stop reproducing, and it
  must be stated explicitly in this spec rather than hidden. See "Accepted
  trade-off: non-hermetic build script" below.
- **NFR2 — No network when the fonts are already correct:** A build whose seven
  fonts are present and SHA256-matching performs no network access at all — the
  idempotent skip in `scripts/fetch-fonts.sh` is what makes the steady-state
  build offline-safe.
- **NFR3 — Supply-chain integrity preserved:** The automatic path introduces no
  weakening of `scripts/fetch-fonts.sh`: HTTPS only (no `--insecure`), per-file
  SHA256 verification before the atomic move, and no unverified or partial file
  ever left in `src-tauri/assets/fonts`.
- **NFR4 — Write containment:** The build script writes only into the building
  worktree's own `src-tauri/assets/fonts` directory and its temporary staging
  location. Concurrent builds of sibling worktrees never write into each other's
  trees, and no shared or cwd-dependent destination is used.
- **NFR5 — CLI-only build untouched:**
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  performs no font check, no fetch and no network access, exactly as today.
- **NFR6 — No rebuild churn:** Fetching fonts during a build must not put cargo
  into a rebuild loop or force a rebuild on every subsequent invocation; the
  build script's rerun conditions stay stable once the fonts are present.
- **NFR7 — Determinism for packaging and release builds:** Distribution and
  release builds remain deterministic: `.github/workflows/release.yml` keeps its
  explicit `bash scripts/fetch-fonts.sh` step and its cache, so the automatic
  path is a no-op there, and packaging builds can engage the FR3 opt-out to
  forbid any implicit fetch.

## Accepted trade-off: non-hermetic build script

This feature deliberately makes `src-tauri/build.rs` non-hermetic. A build
script that reaches the network, and that writes files outside `OUT_DIR`, goes
against Cargo's own guidance: a build is supposed to be reproducible from the
checked-out sources alone, and a build script is supposed to confine its writes
to `OUT_DIR`. Both properties are given up here on purpose.

The trade-off is accepted because the definition of done for this work is that
the literal reproduction command stops reproducing. A fresh `git worktree add`
tree must be able to run
`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
and execute tests. Satisfying that without letting the build acquire the fonts
is not possible, so the non-hermeticity is stated here in plain text instead of
being hidden (NFR1).

Four mitigations bound the exposure:

1. **HTTPS-only.** The download runs through `scripts/fetch-fonts.sh`, which
   uses HTTPS and never `--insecure` (NFR3).
2. **Per-file SHA256 pinning.** Every downloaded file is verified against its
   pinned SHA256 before the atomic move into place, so no unverified or partial
   file is ever left in `src-tauri/assets/fonts` (NFR3).
3. **Idempotent skip.** When the seven fonts are present and hash-matching, the
   script does nothing and the build performs no network access at all — the
   steady state is fully offline (NFR2).
4. **Explicit opt-out.** Offline, sandboxed and packaging builds engage the FR3
   opt-out, which forbids the fetch outright rather than merely tolerating its
   failure, and the build stops with the FR4 actionable message (NFR7).

## Implementation Approach

### Architecture

```
cargo build / cargo test (GUI features on)
        │
        ▼
src-tauri/build.rs :: check_bundled_fonts()
        │  CARGO_FEATURE_GUI gate (FR1)
        ▼
   7 font paths present? ──yes──► emit cargo:rerun-if-changed per path ──► continue build
        │ no
        ▼
   FR3 opt-out engaged? ──yes──► stop with FR4 message (no network)
        │ no
        ▼
   invoke scripts/fetch-fonts.sh once, destination pinned to this
   worktree's src-tauri/assets/fonts (FR2, FR4, FR5, NFR4)
        │
        ├─ exit != 0, or any font still missing ──► stop with FR4 message
        └─ all seven present ──────────────────────► continue build
```

**Components:**

- `src-tauri/build.rs` — owns the GUI gate, the seven-path presence check, the
  opt-out decision, the single fetch invocation, and the actionable stop.
- `scripts/fetch-fonts.sh` — the single acquisition path, used unchanged
  (FR5). Its HTTPS-only, SHA256-pinned, idempotent, atomic-move behavior is
  what satisfies NFR2 and NFR3.
- Bootstrap-verification script (new; its path is fixed at create-plan) —
  reproduces the un-fetched state in isolation, exercises the fetch-failure and
  already-fetched paths, and asserts that the reported command executes tests
  (FR6).
- `.github/workflows/ci.yml` — gains the job that runs that script on push and
  pull_request, with no font-cache restore for the fresh-state scenario (FR7).

### Data Flow

```
build.rs ──(presence check)──► src-tauri/assets/fonts/*        (read, FR1)
build.rs ──(opt-out read)────► environment                      (FR3)
build.rs ──(one invocation)──► scripts/fetch-fonts.sh ──HTTPS──► font sources
                                       │
                                       └─ SHA256-verify ──► atomic move ──►
                                          <this worktree>/src-tauri/assets/fonts
build.rs ──(on failure)──────► build output: FR4 message ──► build stops
```

### Opt-out mechanism

FR3's opt-out is exposed as an **environment variable read by `build.rs`**:

```
EMTERM_SKIP_FONT_FETCH=1
```

When it is set to `1`, `build.rs` performs no fetch attempt at all. If fonts are
also missing, it stops immediately with the FR4 message; it does not merely
tolerate a failed fetch.

The alternative mechanism, a cargo feature, was considered and not chosen. This
is the choice assumption a5 asked this spec to fix; both mechanisms satisfy FR3,
and the difference is naming and documentation surface only.

### Destination pinning

The fetch destination is derived from the build script's manifest location, so
it always resolves to the building worktree's own `src-tauri/assets/fonts`. It
is never derived from the process cwd, and never points at a shared or
cross-worktree location (FR2, NFR4). This is what TS6 exercises.

### Rerun conditions

`src-tauri/build.rs` emits `cargo:rerun-if-changed={path}` for each of the seven
font paths inside `check_bundled_fonts()`, immediately after each existence
check passes — so on a missing font it panics before emitting any of them.
Writing those same files from the build script therefore interacts with cargo's
change tracking, and NFR6 must be satisfied deliberately rather than assumed.
What remains assumed is only that a single post-fetch settling rebuild, if any,
converges instead of looping (assumption a7); TS8 verifies convergence.

### API Design

Not applicable — this feature exposes no API.

### Database Schema

Not applicable — this feature persists no data.

### Dependencies

**Internal Dependencies:**
- `src-tauri/build.rs`: the GUI-gated presence check and the actionable message
  that already exist today.
- `scripts/fetch-fonts.sh`: the single acquisition path, used unchanged (FR5).
- `.github/workflows/ci.yml`: gains the FR7 job.
- `.github/workflows/release.yml`: keeps its explicit
  `bash scripts/fetch-fonts.sh` step and its cache, so the automatic path is a
  no-op there (NFR7).

**External Dependencies:**
- bash, because `scripts/fetch-fonts.sh` is a bash script. The automatic fetch
  of FR2 therefore depends on bash being available to the build script's host.
  On a native Windows host build (`release.yml`'s windows-latest job) bash may
  be absent; in that case the invocation fails and the FR4 actionable stop is
  produced, which is exactly today's behavior and therefore not a regression.
  Whether the Windows host path should instead engage the FR3 opt-out implicitly
  is left to the plan (assumption a4).
- The HTTPS font sources `scripts/fetch-fonts.sh` already downloads from, with
  their pinned per-file SHA256 values.

### File Structure

```
src-tauri/
├── build.rs                    # FR1 check, FR2 fetch, FR3 opt-out, FR4 stop
└── assets/fonts/               # the seven bundled fonts (FR1), write-contained (NFR4)
scripts/
└── fetch-fonts.sh              # the single acquisition path (FR5), unchanged
.github/workflows/
├── ci.yml                      # gains the FR7 bootstrap-verification job
└── release.yml                 # unchanged: explicit fetch step + cache (NFR7)
```

The bootstrap-verification script of FR6 is a new file; its concrete path is
fixed at create-plan.

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/worktree-font-bootstrap/**`
- `test-docs/worktree-font-bootstrap/**`

`feature-docs/worktree-font-bootstrap/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/worktree-font-bootstrap/**` covers
`test-docs/worktree-font-bootstrap/{T}.tests.yaml`, the per-task test record.
It is generated and owned by `implement-phase.md`; this section cites it and
restates none of its rules.

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

| ID | Title | Given / When / Then | Covers |
|----|-------|---------------------|--------|
| TS1 | Fresh worktree, network available | Given a fresh worktree with no font binaries and network access, when the reported command is run, then the library test suite executes with a non-zero test count and the fonts land in that worktree. | FR1, FR2, FR5 |
| TS2 | Opt-out engaged | Given a fresh worktree with no font binaries and the FR3 opt-out engaged, when a GUI build is run, then no fetch is attempted and the build stops with the FR4 actionable message. | FR3, FR4 |
| TS3 | Fetch failure | Given a fresh worktree where `scripts/fetch-fonts.sh` exits non-zero (unreachable host, hash mismatch, or missing tooling), when a GUI build is run, then the build stops with the FR4 message and no partial or unverified file remains under `src-tauri/assets/fonts`. | FR4, NFR3 |
| TS4 | Already-fetched steady state | Given all seven fonts present and SHA256-matching, when a GUI build is run, then no network access occurs and no file is re-downloaded. | NFR2 |
| TS5 | CLI-only build in an un-fetched tree | Given a fresh un-fetched worktree, when the `--no-default-features` check is run, then it succeeds without the font check and without any fetch. | FR1, NFR5 |
| TS6 | Destination pinning across worktrees | Given two sibling worktrees, when one is built from an arbitrary cwd, then fonts are written only into that worktree's `src-tauri/assets/fonts` and the sibling tree is untouched. | FR2, NFR4 |
| TS7 | CI job runs and catches the regression | Given a push or pull request, when CI runs, then the new bootstrap-verification job runs from an isolated un-fetched state with no cache restore, passes on the fixed tree, and fails on a tree where the fix is reverted. | FR6, FR7 |
| TS8 | No rebuild churn | Given a worktree where the fetch already ran during a build, when the same cargo command is run again with no source change, then the build script does not force an unnecessary rebuild or re-enter a fetch loop. | NFR6 |

### Bootstrap-verification paths (FR6)

- [ ] Un-fetched state, isolated, no cache restore — the reported command
      executes the library test suite (TS1).
- [ ] Fetch-failure path — a clear stop carrying the FR4 message (TS3).
- [ ] Already-fetched path — no network access (TS4).

### Edge Cases

- [ ] Sibling worktrees built concurrently — no cross-worktree write (TS6,
      NFR4).
- [ ] Build invoked from an arbitrary cwd — destination still resolves to the
      building worktree (TS6, FR2).
- [ ] Post-fetch rebuild — converges rather than looping (TS8, NFR6,
      assumption a7).
- [ ] bash absent on a native Windows host — the invocation fails and the FR4
      actionable stop is produced, matching today's behavior (assumption a4).

### Performance Tests

Not applicable — no performance target is specified for this feature.

## Security Considerations

- **Transport:** HTTPS only; `scripts/fetch-fonts.sh` never uses `--insecure`
  (NFR3).
- **Integrity:** Per-file SHA256 verification before the atomic move; no
  unverified or partial file is ever left in `src-tauri/assets/fonts` (NFR3,
  AC4).
- **Network exposure:** No network access at all when the seven fonts are
  present and hash-matching (NFR2), and no network access at all when the FR3
  opt-out is engaged (AC3).
- **Write containment:** Writes are confined to the building worktree's own
  `src-tauri/assets/fonts` and its temporary staging location; no shared or
  cwd-dependent destination (NFR4).
- **Supply chain:** No second acquisition mechanism is introduced (FR5), so
  there is only one path to audit.

## Error Handling

| Condition | Behavior |
|-----------|----------|
| Fetch attempt exits non-zero (unreachable host, hash mismatch, missing tooling) | Stop the build with the FR4 message; leave no partial or unverified file (FR4, NFR3) |
| Any of the seven fonts still missing after the attempt | Stop the build with the FR4 message (FR4) |
| FR3 opt-out engaged and fonts missing | No fetch attempt at all; stop the build with the FR4 message (FR3, FR4) |

Message emitted on every one of the above:

```
build_rs.font_missing: bundled font missing at {path}
Run `make fetch-fonts` (or `bash scripts/fetch-fonts.sh`) to download bundled fonts.
```

## Success Criteria

- [ ] AC1 — In a freshly created `git worktree add` tree with no font binaries
      and network available,
      `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
      completes and reports a non-zero number of executed tests; the reported
      reproduction no longer reproduces.
- [ ] AC2 — The same command in the same tree leaves the seven bundled fonts
      present in that worktree's `src-tauri/assets/fonts`, and no font file is
      written anywhere outside that worktree.
- [ ] AC3 — With the FR3 opt-out engaged and fonts missing, the build stops
      without any network access and its output contains
      `build_rs.font_missing: bundled font missing at` and
      ``Run `make fetch-fonts` (or `bash scripts/fetch-fonts.sh`) to download bundled fonts.``
- [ ] AC4 — When the fetch attempt fails (non-zero exit: unreachable host, hash
      mismatch, or missing tooling), the build stops with the same FR4 message
      and `src-tauri/assets/fonts` contains no partial or unverified file.
- [ ] AC5 — With all seven fonts already present and hash-matching, a build
      performs no network access and does not re-download.
- [ ] AC6 —
      `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
      succeeds in a fresh un-fetched worktree and triggers neither the font
      check nor a fetch.
- [ ] AC7 — The bootstrap-verification script exists, exercises the un-fetched,
      fetch-failure and already-fetched paths, and fails when the reported
      defect is reintroduced.
- [ ] AC8 — `.github/workflows/ci.yml` contains a job invoking that script on
      the same push / pull_request triggers as the existing bun job, with no
      font-cache restore in the fresh-state scenario, and the job passes on the
      feature branch.
- [ ] AC9 — The SPEC states the accepted non-hermeticity trade-off (NFR1) and
      its mitigations (HTTPS-only, SHA256-pinned, idempotent, explicit opt-out)
      in plain text.
- [ ] AC10 — No change is made to `viewer/dist`, `settings/dist` or `icons/`
      handling in `build.rs`.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

No requirement carries `status: tbd`; FR1–FR8 are all resolved.

The following reversible assumptions are carried into the plan phase:

- **a4** — `scripts/fetch-fonts.sh` is a bash script, so the automatic fetch of
  FR2 depends on bash being available to the build script's host. On a native
  Windows host build (`release.yml`'s windows-latest job) bash may be absent; in
  that case the invocation fails and the FR4 actionable stop is produced, which
  is exactly today's behavior and therefore not a regression. Whether the
  Windows host path should instead engage the FR3 opt-out implicitly is left to
  the plan.
  *Impact:* determines whether Windows GUI builds get automatic bootstrapping or
  only the actionable message.
  *Verification:* run the Windows-host build path, or inspect the runner image
  for bash availability.
- **a5** — The FR3 opt-out is exposed as an environment variable read by
  `build.rs` (a cargo feature is the alternative). Neither its concrete name nor
  its mechanism was fixed by the answers; this spec fixes one
  (`EMTERM_SKIP_FONT_FETCH`, see "Opt-out mechanism").
  *Impact:* naming and documentation surface only; both mechanisms satisfy FR3.
  *Verification:* the name is recorded above; assert on it in TS2.
- **a6** — The new CI job of FR7 runs on `ubuntu-latest`, matching the existing
  bun job's runner, and installs the Rust toolchain itself because `ci.yml` has
  no Rust job today.
  *Impact:* CI runtime and setup steps; does not change the required behavior.
  *Verification:* read `.github/workflows/ci.yml` after the change and observe a
  green run.
- **a7** — `src-tauri/build.rs` DOES emit `cargo:rerun-if-changed={path}` for
  each of the seven font paths, inside `check_bundled_fonts()`, immediately
  after each existence check passes (so on a missing font it panics before
  emitting any of them). Writing those files from the build script therefore
  interacts with cargo's change tracking, and NFR6 must be satisfied
  deliberately rather than assumed. What remains assumed is only that a single
  post-fetch settling rebuild, if any, converges instead of looping.
  *Impact:* if it does not converge, every subsequent cargo invocation re-runs
  the build script.
  *Verification:* TS8 — build twice in a fresh worktree with no source change in
  between and observe that the second run does not re-run the build script.
- **a8** — The seven font files are the complete set the GUI build requires; no
  additional bundled font is needed for the library test suite to build.
  *Impact:* an eighth required font would leave the repro partially reproducing.
  *Verification:* TS1 executing a non-zero test count in a fresh worktree
  confirms it empirically.

## Design Step

Skipped. Answered as `skip_design` at gate `create-spec.design-step` (batch
policy `decide_autonomously`, accepting the analyst recommendation). The change
is confined to build-script behavior, a build-output message, a verification
script and a CI job — there is no UI, visual or design-token surface.

## References

- Requirements document (Japanese):
  `feature-docs/worktree-font-bootstrap/REQUIREMENTS.md`
- `src-tauri/build.rs`: `check_bundled_fonts()` — GUI gate, seven-path presence
  check, `cargo:rerun-if-changed` emission, actionable message
- `scripts/fetch-fonts.sh`: the single acquisition path — HTTPS-only, per-file
  SHA256-pinned, idempotent, atomic move
- `.github/workflows/ci.yml`: existing bun job (push / pull_request) and the FR7
  job to be added
- `.github/workflows/release.yml`: explicit `bash scripts/fetch-fonts.sh` step
  and cache (NFR7)
