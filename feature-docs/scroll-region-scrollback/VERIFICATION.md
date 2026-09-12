# Verification Document: scroll-region-scrollback

## Overview

**Feature**: scroll-region-scrollback / **SPEC.md**:
`feature-docs/scroll-region-scrollback/SPEC.md` / **IMPLEMENTATION.md**:
`feature-docs/scroll-region-scrollback/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the merged feature. Each
task's own Acceptance Criteria live in its task plan and are verified by the
implementer before merge.

## Build Verification

Run every component's build command; all must exit 0 with no errors.

| Component | Command |
|-----------|---------|
| term-core | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml` |
| app-settings | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/app_settings/Cargo.toml` |
| main | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` |
| cli-only | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` |
| webview-ts | `bun run build:viewer && bun run build:settings` |

- Expected: exit code 0, no errors. The cli-only line is the NFR3 feature-gate
  evidence and is not optional.

## Test Verification

| Component | Command |
|-----------|---------|
| term-core | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib` |
| app-settings | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/app_settings/Cargo.toml --lib` |
| main | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` |
| cli-only | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib --no-default-features` |
| webview-ts | `bun run typecheck && bun test` |

- Coverage target: not defined for this project; no coverage threshold is
  enforced. Coverage of this feature is judged by the scenario table below.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Region whose top margin is the topmost row and whose bottom margin is above the last row, line feeds at the region bottom | Exactly N new scrollback lines, contents and order matching the successive former top rows; rows below the bottom margin unmoved; bottom-margin row blank | Unit (term-core) |
| TS-2 | Byte-driven negative cases: non-zero top margin, alternate screen enter and leave, line insert and delete, scroll down and reverse index, gate disabled, zero scrollback capacity | Zero new scrollback lines and the pre-change screen state in every case; transcription resumes after leaving the alternate screen | Unit (term-core) |
| TS-3 | Scroll-up control sequence inside a top-margin-zero region, including a count larger than the region height; full-screen scroll path | Transcribes, clamped to the region height; full-screen path and its single-line scroll-event / dirty-row optimization unchanged; region path emits no full-screen scroll event | Unit (term-core) |
| TS-4 | Persisted settings key serde round-trip: default, explicit false, explicit null | Default enabled; false round-trips; null resolves to enabled | Unit (app-settings) |
| TS-5 | Settings application across tabs and tab construction | The value reaches every already-open tab's core and is seeded into newly built tabs; no tab restart, PTY re-spawn or scrollback discard | Unit (main) |
| TS-6 | Settings panel behaviour subsection | The new toggle renders beside the alternate-scroll toggle and saves under the exact key string | Unit (webview-ts) |
| TS-7 | Static checks | Type check across the web entries passes; the CLI-only configuration compiles with the new settings key | Integration (static) |
| TS-8 | Codex TUI v0.153.4 on a release build, multi-turn conversation | Shift+PageUp, wheel and scrollbar each reach earlier turns; history reads in chronological order with no duplicated or interleaved turns | Manual |
| TS-9 | Claude Code on a release build (alternate screen plus alternate scroll) | Wheel still drives the application's own scrolling; no alternate-screen content enters scrollback; leaving the alternate screen restores the previous main-screen view | Manual |
| TS-10 | vim and less on a release build | No duplicated lines, no reordering and no visible corruption during or after region scrolling; scrollback contents match a pre-change build | Manual |

## Code Quality Verification

- Format (Rust): `cargo fmt --manifest-path crates/term_core/Cargo.toml --check`
  and `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Format (TypeScript): `bunx biome check .`
- NFR5 constraint: formatting **writes** are applied only to files this
  feature touched; a crate-wide or repository-wide format write is not run.
- Static analysis: no separate linter beyond the above; the type check
  (`bun run typecheck`) and the compiler warnings from the build commands are
  the static-analysis surface.
- Run result (2026-09-13, integration branch): `cargo fmt --manifest-path
  crates/term_core/Cargo.toml --check` exits 0 and `bunx biome check .` exits
  0. `cargo fmt --manifest-path src-tauri/Cargo.toml --check` exits 1 on
  **pre-existing** drift in 8 files, none of which this feature touched
  (`agent_status_exit_latch.rs`, `app/tests/agent_status.rs`, `app/tests.rs`,
  `arg_dispatch.rs`, `mux/inherited_pty.rs`, `mux/session/session.rs`,
  `tests/cli_subcommands.rs`, `tests/mux_hot_upgrade.rs` — all absent from
  `git diff e061b541..HEAD`). Clearing it would require a crate-wide format
  write, which NFR5 forbids, so NFR5 and SC-6 are judged on the touched-files
  basis and this exit code is not a feature defect.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1 through FR14 are implemented and tested | The coverage table below: every FR maps to at least one task and at least one verification method |
| SC-2 | The automated criteria AC-1 to AC-12 pass | TS-1 to TS-7 all green |
| SC-3 | The on-device criteria AC-13 to AC-15 are completed | TS-8, TS-9, TS-10 checked off in Manual Testing |
| SC-4 | No per-byte work and no allocation added to the output hot path | Review of the merged diff against the Performance section below, plus TS-3's unchanged-full-screen-path assertions |
| SC-5 | The CLI-only build and the web type check pass | Build Verification cli-only line and TS-7 |
| SC-6 | Only touched files are formatted | Code Quality Verification, plus a diff review showing no unrelated formatting churn |
| SC-7 | Requirement IDs agree between REQUIREMENTS.md and SPEC.md | Both documents list FR1 to FR14 and NFR1 to NFR5 with matching titles |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-8, TS-10 |
| FR2 | task0001 | TS-2, TS-10 |
| FR3 | task0001 | TS-1, TS-3, TS-8 |
| FR4 | task0001 | TS-1, TS-3 |
| FR5 | task0001 | TS-1, TS-10 |
| FR6 | task0001 | No test — horizontal margins are unimplemented, so the clause is trivially true today; verified by review of the condition set (recorded gap) |
| FR7 | task0001 | TS-2 |
| FR8 | task0001 | TS-2, TS-9 |
| FR9 | task0002 | TS-4, TS-7 |
| FR10 | task0002, task0003 | TS-6, TS-7 |
| FR11 | task0002 | TS-5 |
| FR12 | task0002 | TS-5 |
| FR13 | task0001 | TS-3, TS-10 |
| FR14 | task0001 | TS-8 |
| NFR1 | task0001 | TS-1, TS-3 |
| NFR2 | task0001 | TS-3, plus the Performance section below |
| NFR3 | task0002, task0003 | TS-7 |
| NFR4 | task0001, task0002, task0003 | TS-2, TS-5, TS-6 |
| NFR5 | task0001, task0002, task0003 | No test — verified by the Code Quality Verification commands above (recorded gap) |

## Manual Testing (E2E Not Possible)

This project has no E2E framework, and none is introduced by this feature.
Regression evidence is the byte-stream unit tests above plus the three
on-device checks below, each run against a release build.

Run status (2026-09-13, unattended batch): TS-8, TS-9 and TS-10 were **not
performed** — each needs a human driving a release-build GUI terminal, which an
unattended run cannot do. They remain open and require on-device confirmation
by the user before the branch is merged. The byte-level equivalents (TS-1, TS-2,
TS-3) are green, so the automated surface is complete.

- [ ] TS-8: Codex TUI v0.153.4 — hold a multi-turn conversation, then reach
      earlier turns with Shift+PageUp, with the mouse wheel and with the
      scrollbar. History must read in chronological order with no duplicated
      or interleaved turns.
- [ ] TS-9: Claude Code — the wheel keeps driving the application's own
      scrolling, no alternate-screen content lands in scrollback, and leaving
      the alternate screen restores the previous main-screen view.
- [ ] TS-10: vim and less — scroll through content in both; no duplicated
      lines, no reordering, no visible corruption during or after scrolling,
      and scrollback contents matching a pre-change build.

## Performance / Security Verification

- NFR2 (hot path): review the merged diff and confirm that the full-screen
  scroll branch, the ASCII fast path and the bulk-scroll skip-ahead are
  unmodified, that the new condition is evaluated only on the region branch
  that was already being taken, and that the non-transcribing path performs no
  new allocation.
- NFR2 (no regression in practice): during TS-8, output-heavy interaction
  should feel unchanged; no numeric threshold is defined for this feature.
- Security — parsing surface: confirm no new escape sequence, control
  sequence or external input format is parsed. The change only alters what
  happens to already-parsed scroll operations.
- Security — input validation: the new settings key is a plain boolean behind
  the existing null-tolerant deserialization, so a malformed or hostile
  settings document cannot produce a value outside true/false (TS-4).
- Security — data retention: terminal content that was previously discarded
  now stays in the in-memory scrollback ring. Confirm it is bounded by the
  existing scrollback capacity, with no new persistence to disk and no new
  exposure over IPC.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Terminal-core behaviour | 3 | 3 (TS-1, TS-2, TS-3) | 0 | 0 |
| Settings wiring | 4 | 4 (TS-4, TS-5, TS-6, TS-7) | 0 | 0 |
| On-device regression | 3 | 0 | 0 | 3 (TS-8, TS-9, TS-10) |
| Build / format gates | 10 | 10 (5 build, 5 test) + 3 format commands | 0 | 0 |
| **Total scenarios** | **10** | **7** | **0** | **3** |
