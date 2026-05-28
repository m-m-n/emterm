# Verification Document: mux Scrollback Retention

## Overview
**Feature**: mux-scrollback-retention
**SPEC.md**: `doc/tasks/mux-scrollback-retention/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/mux-scrollback-retention/IMPLEMENTATION.md`

## Build Verification
- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo build --manifest-path src-tauri/Cargo.toml"`
- Expected: exit code 0, no errors, no new warnings beyond baseline.
- **Result**: PASS. `Finished dev profile [unoptimized + debuginfo] target(s) in 37.81s`. No new warnings.

## Test Verification
- Rust: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"`
- TypeScript typecheck (regression safety): `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"`
- TypeScript unit (regression safety): `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"`
- Coverage target: scrollback_buffer.rs at 100% line coverage on the algorithm (pure data structure); pane.rs and reattach.rs deltas at 80%+ line coverage on touched code.

### Actual Results
- Rust unit test result: **1007 passed; 0 failed; 1 ignored**. All mux-scoped tests pass (261 in `mux::*`).
- All TS-1..TS-19 scenarios pass (see test names in Test Scenarios table).
- TypeScript suites not re-run (no TypeScript files were touched in this change).

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Empty `ScrollbackRingBuffer`: `len()==0`, `read_all()==[]` | Pass | Unit (scrollback_buffer.rs) |
| TS-2 | Single small write reads back identically | Pass | Unit (scrollback_buffer.rs) |
| TS-3 | Multiple writes concatenate in order | Pass | Unit (scrollback_buffer.rs) |
| TS-4 | Write past capacity wraps and keeps the tail | Pass | Unit (scrollback_buffer.rs) |
| TS-5 | Single write larger than capacity keeps last `capacity` bytes | Pass | Unit (scrollback_buffer.rs) |
| TS-6 | Write exactly `capacity` bytes fits without wrap | Pass | Unit (scrollback_buffer.rs) |
| TS-7 | `clear()` empties the buffer | Pass | Unit (scrollback_buffer.rs) |
| TS-8 | Write after clear starts fresh | Pass | Unit (scrollback_buffer.rs) |
| TS-9 | `capacity()` returns configured size | Pass | Unit (scrollback_buffer.rs) |
| TS-10 | Many 1-byte writes wrap correctly | Pass | Unit (scrollback_buffer.rs) |
| TS-11 | `DEFAULT_SCROLLBACK_CAPACITY == 2 * 1024 * 1024` | Pass | Unit (scrollback_buffer.rs) |
| TS-12 | Fresh `MuxPane` exposes scrollback with `len()==0`, `capacity()==2MiB` | Pass | Unit (pane.rs) |
| TS-13 | Writes accumulate while `PaneOutputTarget::Connected` | Pass | Unit (pane.rs) |
| TS-14 | Writes accumulate while `PaneOutputTarget::Detached` | Pass | Unit (pane.rs) |
| TS-15 | `collect_reattach_data` emits bytes ordered: clear → scrollback → shadow → passthrough | Pass | Unit (reattach.rs) |
| TS-15b | `resume_pane_with_permit` (hidden→visible) emits bytes in the same FR5 order | Pass | Unit (pane.rs) |
| TS-16 | `collect_reattach_data` does not clear `pane.scrollback` (`len` unchanged) | Pass | Unit (reattach.rs) |
| TS-16b | `resume_pane_with_permit` does not clear `pane.scrollback` (`len` unchanged) | Pass | Unit (pane.rs) |
| TS-17 | `PaneOutputTarget::Detached` no longer has `ring` field | Compile-time pass | Unit (pane.rs) |
| TS-18 | Existing reattach unit tests in reattach.rs still pass after the field shape change | Pass | Unit (reattach.rs) |
| TS-19 | Edge: pane created with no client attached still allocates scrollback | Pass | Unit (pane.rs) |

## Code Quality Verification
- Format: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo fmt --manifest-path src-tauri/Cargo.toml -- --check"`
- Static analysis: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings"`
- Expected: no formatting drift, no new clippy errors.
- **Format result**: Pre-existing fmt drift across the repo (e.g. `tauri_commands.rs`, `app.rs`, `image/*`, etc.) — same set of files reports `Diff in` on a clean baseline (verified via `git stash`). No new drift introduced by this change.

## File Structure Verification

### Files to Create
- `src-tauri/src/mux/scrollback_buffer.rs` - Renamed module hosting `ScrollbackRingBuffer` and `DEFAULT_SCROLLBACK_CAPACITY`.

### Files to Modify
- `src-tauri/src/mux/mod.rs` - Module declaration switched from `ring_buffer` to `scrollback_buffer`.
- `src-tauri/src/mux/session/pane.rs` - `MuxPane.scrollback` field, `PaneOutputTarget::Detached` no longer holds `ring`, tests updated and added.
- `src-tauri/src/mux/ipc/handlers.rs` - `Detached` construction sites updated.
- `src-tauri/src/mux/ipc/pty_spawn.rs` - Always-on `scrollback.write` in the reader handler; legacy in-variant ring writes removed.
- `src-tauri/src/mux/ipc/reattach.rs` - New reattach send order; no scrollback clear; legacy ring drains removed.

### Files to Remove
- `src-tauri/src/mux/ring_buffer.rs` - Replaced by `scrollback_buffer.rs`.

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1–FR9 implemented | Checklist below + unit tests |
| SC-2 | All unit tests in this document pass | `cargo test` |
| SC-3 | `bun run typecheck` and `bun test` remain green | Run via Docker |
| SC-4 | Existing E2E specs pass without regression | `./scripts/run-e2e-docker.sh test` |
| SC-5 | No remaining references to `DetachRingBuffer`, `DEFAULT_RING_CAPACITY`, `ring_buffer.rs` | `grep -rn` in `src-tauri/src` returns no hits |
| SC-6 | Manual: post-reattach scrollback contains pre-detach output | Live `bun tauri dev` walkthrough |

### Functional Requirements Coverage
| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 (rename) | Phase B | grep for old identifiers returns empty; TS-1..TS-10 cover algorithm |
| FR2 (2 MiB constant) | Phase A + B | TS-11 |
| FR3 (pane-resident) | Phase B | TS-12 / TS-19 |
| FR4 (always-on write) | Phase C | TS-13 / TS-14 |
| FR5 (reattach send order) | Phase C | TS-15, TS-15b |
| FR6 (no clear at reattach) | Phase C | TS-16, TS-16b |
| FR7 (no ESC trimming) | Phase C | TS-15 implicitly (shadow follows scrollback); manual check that wrapped scrollback does not corrupt the final screen |
| FR8 (carry-over tests) | Phase B | TS-1..TS-10 present in scrollback_buffer.rs |
| FR9 (passthrough unchanged) | Phase C | TS-15 confirms passthrough is still the last segment; TS-18 confirms regression-free |
| NFR1 (memory bound) | Phase A + B | TS-11 (capacity constant) + design review (no per-detach 64 MB allocation) |
| NFR2 (write overhead) | Phase C | Manual inspection: write call equivalent to old detach-path memcpy |
| NFR3 (IPC compatibility) | Phase C | E2E specs pass (modulo pre-existing mux E2E regression — see VERIFICATION_RESULT.md) |

## E2E Testing

E2E command: `./scripts/run-e2e-docker.sh test <spec>`

Mux-related specs that exist in the repository (verified at branch `feat/mux-scrollback-retention`):

- [~] `mux.e2e.js` — **6 / 11 fail**. Reproduces on `main @ 2a0d903` with this branch reverted; pre-existing regression, not caused by this feature.
- [~] `mux-reattach.e2e.js` — fails identically on `main`; tracked separately.
- [~] `mux-multi-session.e2e.js` — fails identically on `main`; tracked separately.
- [~] `mux-move-window.e2e.js` — not re-run as part of this verification (out of scope for the regression-on-main investigation).
- [x] `viewer-tab-switch-keyboard.e2e.js` passes (1 / 1) — confirms E2E infrastructure itself is healthy.

`mux-osc-title-propagation.e2e.js` referenced by earlier drafts does **not exist** in the repository at `e2e-tests/specs/`.

No new spec is added (per SPEC FR / design decision — scrollback retention is validated by Rust unit tests and manual verification).

## Manual Testing (E2E Not Possible)

- [ ] Start `bun tauri dev`; open mux; produce ~5 screens of output while attached; detach; reattach; confirm the scrollback in the GUI reaches pre-detach output.
- [ ] Start `bun tauri dev`; spawn a pane that emits output during detach; reattach; confirm both pre-detach attached output and detach-window output appear in scrollback.
- [ ] Repeat detach/reattach 3 times in a row; confirm scrollback keeps growing toward the 2 MB cap and is not reset on each reattach.

## Performance Verification

- Steady-state scrollback memory: `pane_count × 2 MB`. Expected with 10 panes ≈ 20 MB additional RSS attributable to scrollback. Verification by process inspection (`top` / `ps`) in a live session.
- No 64 MB allocation spike at the moment of detach (previously occurred on the `DetachRingBuffer::new(64MB)` allocation). Verification by code review of the detach branches in pty_spawn.rs and reattach.rs (no `DetachRingBuffer::new` call remains).

## Security Verification

- [ ] Scrollback contents remain memory-resident only; no new disk writes.
- [ ] Reattach owner/identity checks (`DetachReason::HiddenByVisibility`, `owner.same_channel`) remain in `evaluate_output_target` / `collect_reattach_data` and are unaffected by this change.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Unit tests | 21 | 21 | 0 | 0 |
| Code quality | 2 | 2 | 0 | 0 |
| E2E regression | 5 | 0 | 5 | 0 |
| Manual UX | 3 | 0 | 0 | 3 |
| Performance | 2 | 0 | 0 | 2 |
| Security | 2 | 0 | 0 | 2 |
| **Total** | **36** | **24** | **5** | **7** |
