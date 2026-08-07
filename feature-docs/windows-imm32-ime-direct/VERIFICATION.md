# Verification Document: windows-imm32-ime-direct

## Overview

**Feature**: windows-imm32-ime-direct
**SPEC.md**: `feature-docs/windows-imm32-ime-direct/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/windows-imm32-ime-direct/IMPLEMENTATION.md`

This documents the INTEGRATED verification run by the verify phase.
Task-level acceptance criteria live in `tasks/task0001.md`.

## Build Verification

All three component gates must exit 0 with no errors. The fresh-worktree
prerequisites (font fetch, bun install, web bundles) are already part of the
recorded command strings.

| Component | Command |
|-----------|---------|
| main (GUI check) | `bash scripts/fetch-fonts.sh && bun install && bun run build:viewer && bun run build:settings && CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` |
| cli (no-default-features check) | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` |
| windows (cross-target check) | `bash scripts/fetch-fonts.sh && bun install && bun run build:viewer && bun run build:settings && CARGO_TARGET_DIR=src-tauri/target-win cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc` |

The windows command is the ONLY automated gate that compiles the
`#[cfg(windows)]` IMM32 code path; a verify pass without it has not
exercised the Windows code at all.

## Test Verification

- Command: `bash scripts/fetch-fonts.sh && bun install && bun run build:viewer && bun run build:settings && CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Expected: exit code 0, all tests pass (Rust unit tests live under `--lib`;
  `--test-threads=1` is required — some replay tests are non-deterministic
  in parallel).
- Coverage target: no numeric coverage threshold is defined for this
  project. The gate is: the full `--lib` suite is green, including new
  tests covering TS1 and TS2, with no pre-existing `winit_bridge` test
  modified (TS3).

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | With a composition open (Enabled observed, no Disabled yet), focus loss + flush | No detach delivered; after Disabled arrives, the next flush delivers the detach exactly once | Unit (mock window, host-runnable) |
| TS2 | Focus-in arrives while a detach is held | Pending allow-state overwritten (last-writer-wins); no detach is ever delivered | Unit (mock window, host-runnable) |
| TS3 | Existing deferred-flush / dedup / ordering suite | Every pre-existing `winit_bridge` unit test passes without modification | Unit (regression) |
| TS4 | Automated build gates | All three component commands above exit 0 (main test + cli check + windows cross-target check) | Integration (build) |
| TS5 | Windows + CorvusSKK real device | Repeated conversion commits without freeze; Alt+Tab mid-conversion without freeze; candidate window tracks the caret | Manual (real device) |
| TS6 | Linux host X11 / Wayland composition round-trip | Observable IME behavior unchanged | Manual (host) |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
- Static analysis: none configured for this project.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | Windows `set_ime_cursor_area` calls IMM32 directly, never through winit's request path | Code review of the Windows sink against the FR2 recipe + TS3/TS4 automated gates |
| AC2 | While a composition is alive, the detach is not sent; it is sent by the flush after Disabled is received | Unit tests (TS1, TS2) |
| AC3 | Repeated conversion commits do not freeze on the real device | Manual TS5 (real device only) |
| AC4 | Alt+Tab mid-conversion does not freeze on the real device | Manual TS5 (real device only) |
| AC5 | Candidate window follows the cursor on the real device | Manual TS5 (real device only) |
| AC6 | X11 / Wayland IME behavior unchanged | TS3 (existing suite green, unmodified) + manual TS6 |
| AC7 | CLI-only build passes | TS4 (cli component command) |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS3, TS4 (automated) + TS5 (manual real device) |
| FR2 | task0001 | TS4 (windows cross-target compile) + TS5 (manual real device); recipe conformance by code review |
| FR3 | task0001 | TS1, TS2 (unit) + TS5 (manual real device) |
| FR4 | task0001 | TS3 (existing suite; Enable path unchanged) + code review |
| FR5 | task0001 | TS3 (existing suite unmodified) + TS6 (manual Linux host) |
| FR6 | task0001 | TS4 (windows cross-target compile proves the feature flag + HWND path build) |
| NFR1 | task0001 | TS4 + manifest review: winit pin unchanged, no `[patch.crates-io]` |
| NFR2 | task0001 | TS4 (cli component command passes) |
| NFR3 | task0001 | TS5 (manual real device) + code review: IMM32 calls reachable only from the flush run in `about_to_wait` |

## Manual Testing (E2E Not Possible)

**Real-device gate — Windows + CorvusSKK physical machine (TS5).**
This gate cannot run on the Linux development host or in CI; it is performed
by the user on a Windows machine with CorvusSKK enabled, running a Windows
build that contains this change. It is the acceptance gate for AC3, AC4 and
AC5.

- [ ] TS5-a (AC3): compose and commit conversions repeatedly — the app never
      enters "not responding".
- [ ] TS5-b (AC4): with a conversion in progress, Alt+Tab away from the
      window — the app never freezes; returning and typing again works.
- [ ] TS5-c (AC5): move the cursor, start conversions at several positions —
      the candidate window follows the caret position.

**Linux host spot check (TS6, AC6).**

- [ ] TS6: on the Linux host, X11 and Wayland composition round-trip
      (compose → convert → commit) behaves exactly as before this change.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build gates (3 components) | 3 | 3 | 0 | 0 |
| Unit / regression scenarios (TS1-TS3) | 3 | 3 | 0 | 0 |
| Format check | 1 | 1 | 0 | 0 |
| Real-device scenarios (TS5-a..c) | 3 | 0 | 0 | 3 |
| Linux host spot check (TS6) | 1 | 0 | 0 | 1 |
| **Total** | **11** | **7** | **0** | **4** |
