# Verification Document: mux-agent-status-pane-key-collision

## Overview

**Feature**: mux-agent-status-pane-key-collision
**SPEC.md**: `feature-docs/mux-agent-status-pane-key-collision/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/mux-agent-status-pane-key-collision/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the feature. Per-task
acceptance criteria live in `tasks/task0001.md`.

Run every command from the project root. Do not `cd` into `src-tauri/`, and
always set `CARGO_TARGET_DIR` explicitly (project rule `core-build-location.md`).

## Build Verification

Two components are declared in workflow.yaml and both must build.

| Component | Command | Expected |
|---|---|---|
| main (GUI, default features) | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` | exit code 0, no errors, no new warnings in the touched modules |
| cli_only (`--no-default-features`) | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | exit code 0 — confirms the GUI-only feature gates still hold |

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0, zero failures, TS-1 .. TS-10 all present and passing.
- Coverage target: no coverage tooling is configured in this repository, so
  there is no percentage gate. The gate is scenario-based: every TS below is
  implemented and passing.

**Two project-specific execution notes** — both are pre-existing properties of
this repository, not of this change:

1. Unit tests live under the library target. Running the binary target
   (`--bin emterm`) yields zero tests, so `--lib` is mandatory in the command
   above.
2. The replay tests in the tabs module are non-deterministic under parallel
   execution. Before attributing any replay-test failure to this feature,
   re-run with a single test thread:
   `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`.
   A failure that survives the single-threaded re-run is a real regression.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Two connection scopes, same wire `pane_id`: apply a Working update under scope A and none under scope B; then apply Blocked under scope B | Aggregating scope A's key set yields Working and scope B's yields none; after scope B's update, scope A still reads Working | Unit |
| TS-2 | Discard scope A's pane-1 key | Scope B's pane-1 entry is still present and unchanged | Unit |
| TS-3 | A reported state exists under scope A only; query the "any pane has reported state" path for scope B | Scope B's same-numbered pane does not qualify for the next-agent-window cycle | Unit |
| TS-4 | Build the per-tab key set for two mux-attached tab fixtures whose window groups hold identical pane ids | The two key sets are disjoint — no key appears in both | Unit |
| TS-5 | Derive the notification rate-limit key for scope A pane 1 and scope B pane 1, once with each scope's own public pane id learned and once on the never-learned fallback path | The two keys differ in both cases, and each learned key is its own daemon's public pane id string | Unit |
| TS-6 | Resolve the notification tab title for two tabs whose groups both contain wire `pane_id` 1, from each tab's own scoped key; plus the visibility companion assertion, plus a transition whose owning tab is gone | Each scoped key resolves to its own tab's title; a non-active tab's pane is not reported visible because the active tab holds the same wire `pane_id`; a gone tab resolves to no tab (EC-4) | Unit |
| TS-7 | Feed the batch apply updates tagged for two different tab scopes carrying the same wire `pane_id` and different public pane ids | Both public-pane-id entries are stored; querying each scope returns its own daemon's string | Unit |
| TS-8 | Feed the batch apply a closed-pane entry tagged for scope A, wire `pane_id` 1, with scope B holding its own pane 1 | Scope A's model entry, public-pane-id entry and rate-limit bookkeeping are discarded; every scope B pane-1 entry survives. **Primary AC-7 regression test** | Unit |
| TS-9 | Read the per-tab badge for two mux-attached tab fixtures whose groups hold identical pane ids but different reported states | Each tab's badge reflects only its own pane — the unit-level analogue of the reported symptom | Unit |
| TS-10 | Run the existing plain-tab scenarios (set/clear, unseen preservation, replay-derived silence, latch-driven inferred clear, discard-on-close) with their original expectations | All pass unmodified — plain-tab key semantics are untouched | Unit |

### Additional Verification Checks (non-test)

Three requirements are structural rather than behavioral and are verified by
inspection of the integrated change rather than by a test scenario.

| ID | Requirement | Check |
|----|-------------|-------|
| VC-1 | NFR1 / AC-8 | The integrated diff touches no path under `crates/mux_ipc/`, and leaves `src-tauri/src/mux/session/manager.rs` unchanged — the wire types, the public pane id format and the daemon-side pane-id allocator are byte-for-byte as before |
| VC-2 | NFR2 | The per-frame badge read paths and the next-agent-window qualification are keyed lookups: no iteration over all tabs to find a pane's owner, and no per-frame allocation beyond the per-tab key-set build that already existed |
| VC-3 | NFR4 | The mux pane key's doc comment and the rate-limit key's doc comment state the new connection-scoped rule; neither still describes the wire-`pane_id`-only contract |

## Code Quality Verification

- Format check: `cargo fmt --all --check` — expected exit code 0.
  Formatting is applied with `make fmt`; per project rule, do not let a
  crate-wide reformat pull unrelated files into the change set.
- Static analysis: no clippy gate is configured for this project; the build
  commands above are the static gate.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1 .. FR6 implemented and tested | TS-1 .. TS-9 pass; the coverage table below has a task and a check for each |
| SC-2 | NFR1 .. NFR4 hold | VC-1 (NFR1), VC-2 (NFR2), TS-10 (NFR3), VC-3 (NFR4) |
| SC-3 | TS-1 .. TS-10 pass | Test Verification command, exit code 0 |
| SC-4 | AC-1 .. AC-8 met | AC-1 by MT-1 plus TS-9; AC-2 by TS-1; AC-3 by TS-5 and TS-7; AC-4 by TS-6 plus MT-2; AC-5 by TS-2 and TS-8; AC-6 by TS-10; AC-7 by TS-8; AC-8 by VC-1 |
| SC-5 | The regression test fails pre-fix and passes after | TS-8's assertions break when the scope is dropped from any single key derivation (task0001 Test Notes) |
| SC-6 | `crates/mux_ipc` byte-for-byte unmodified | VC-1 |
| SC-7 | Library test command passes | Test Verification |
| SC-8 | CLI-only check passes | Build Verification, cli_only row |
| SC-9 | Formatting applied | Code Quality Verification |
| SC-10 | The doc comments named in NFR4 describe the new rule | VC-3 |
| SC-11 | Code review completed | The review phase's own record |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-4 |
| FR2 | task0001 | TS-3, TS-4, TS-9 |
| FR3 | task0001 | TS-7, TS-8 |
| FR4 | task0001 | TS-5, TS-8 |
| FR5 | task0001 | TS-6 |
| FR6 | task0001 | TS-2, TS-8 |
| NFR1 | task0001 | VC-1 (no TS: structural check on the diff) |
| NFR2 | task0001 | VC-2 (no TS: structural check on the read paths) |
| NFR3 | task0001 | TS-10 |
| NFR4 | task0001 | VC-3 (no TS: doc-comment inspection) |

## E2E Testing

No E2E framework exists in this repository, and both declared components have an
empty `e2e_test_command`. No E2E scenario is added for this feature.

## Manual Testing (E2E Not Possible)

The reported symptom needs two mux daemons on two hosts, which the unit tests
reproduce in-process but cannot reproduce end to end. These are human-judgment
checks against a release build.

- [ ] MT-1 (AC-1): Attach tab 1 to server 1's mux daemon and tab 2 to server 2's
      mux daemon, both on their own daemon's window 1. Drive Claude Code on tab 1
      through working then done. Tab 1's badge follows the transitions; tab 2's
      badge keeps showing only its own pane's state throughout.
- [ ] MT-2 (AC-4): With the same two-daemon setup, let a transition on tab 1's
      pane raise a notification. The notification body names tab 1, not tab 2.
- [ ] MT-3 (NFR3, single-daemon regression): With one daemon attached, confirm
      the badges, the mux-sidebar entries, the next-agent-window cycle and the
      notifications behave exactly as before the change — no visible difference
      in the single-daemon case.
- [ ] MT-4 (EC-3): Confirm a mux tab whose daemon never installs a window group
      still behaves as a plain tab, with no badge regression.

The design step is skipped for this feature, so there is no mockup comparison
item.

## Performance / Security Verification

- NFR2 (performance): verified structurally by VC-2 — the per-frame badge reads
  stay keyed lookups; there is no dedicated load or stress test, matching SPEC's
  "Performance Tests: none" decision.
- Security: no new external-input parsing is introduced — the daemon-minted
  public pane id stays an opaque map value and rate-limit key, so its parsing
  surface is not newly exercised. The connection scope is a process-local value
  that is never transmitted on the mux wire and never rendered, so no new
  information is exposed between servers. Confirm both by inspection during
  review.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit tests (TS-1 .. TS-10) | 10 | 10 | 0 | 0 |
| Structural checks (VC-1 .. VC-3) | 3 | 0 | 0 | 3 |
| Code quality | 1 | 1 | 0 | 0 |
| Manual scenarios (MT-1 .. MT-4) | 4 | 0 | 0 | 4 |
| **Total** | **20** | **13** | **0** | **7** |
