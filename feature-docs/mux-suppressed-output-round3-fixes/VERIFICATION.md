# Verification Document: mux-suppressed-output-round3-fixes

## Overview
**Feature**: mux-suppressed-output-round3-fixes / **SPEC.md**: `feature-docs/mux-suppressed-output-round3-fixes/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-suppressed-output-round3-fixes/IMPLEMENTATION.md`

All commands run from the project root (the integration worktree root),
never from `src-tauri/`.

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only, NFR6): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors, for both.

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Registry and round3 modules only: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib round3_`
- Test-name resolution: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list`
- Coverage target: not measured; no coverage tool is configured for this
  project. Completeness is judged by each task plan's Acceptance Criteria
  and their test mapping.
- Known nondeterministic existing tests: the `tabs` replay tests (parallel
  run) and the `tmux_sockets` discover test. A failure in one of them is
  re-run alone with `--test-threads=1` before it counts as a failure.

### Test Scenarios from SPEC.md

TS-1 to TS-10 and EC-1 to EC-3 come from SPEC.md. TS-11 to TS-15 are added
by this plan so that FR9, SPEC AC-8 and every NFR have a verifying
scenario.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | FR1, NFR4: on the main screen, with the ring not wrapped and a visibility restore, suppress a chunk of `ESC ]11;?` followed by a 47, 1047 or 1049 `h`/`l` pair with no trailing space, then send a read holding BEL. Repeat with a trailing space, with the OSC carried in pending from an earlier read, and with a lone ESC before the switch. | No response. The ring holds no unterminated OSC. The client equals the raw-stream reference in screen, cursor, responses and displayed characters. | Integration |
| TS-2 | FR2: write-filter split invariance for `ESC ( ESC [6n` and `ESC ( ESC ]777;emterm;markdown;x BEL` (added to the existing corpus), over every split position and byte-at-a-time feeding. Also the cut and overflow paths starting in the awaiting-designator state. | Emitted bytes and pending equal the one-call result in every case. | Unit |
| TS-3 | FR3: ring-write strip and snapshot strip of `ESC ( ESC ]0777;emterm;markdown;begin;id=x BEL X`, in one call and split, with watch offsets around the designator byte. | All bytes kept in both strips. Remapped offsets stay consistent. | Unit |
| TS-4 | FR4, NFR4: reads `ESC` / `[?1049h` / `ESC` / `[?1049l` / `]11;? BEL`, and the 47 and 1047 forms, with the last read suppressed. | No carried-over completion is reported. The replacement holds no color query. No response. | Integration |
| TS-5 | FR5, NFR4: 129 consecutive `ESC (` in earlier reads (full window, no decidable ESC), then a suppressed chunk `ESC ]11;? BEL`. Also a full window whose trailing `(` is not preceded by ESC. | The first case sends no replacement and gives no response. The second still scans from the chunk start and delivers its query. | Unit + Integration |
| TS-6 | FR6, NFR4: with the reader paused at P4 after the covered decision, record a second snapshot for the same destination: same boundary `ESC (` to none, none to `ESC (`, a higher boundary, and none to a UTF-8 lead byte. Also a record for another destination. | The tail decision follows the current record. A tail omitted under the stale record is sent when the current record requires it, including the empty-replacement case. No replacement character appears. The other destination's record has no effect, and it receives nothing. | Integration |
| TS-7 | FR7: same destination and boundary: on-demand (none) then visibility restore (`ESC (`), and the reverse order. Record-rule unit tests for equal, higher and lower boundaries. | The first order leaves `ESC (` and re-sends no `(`. The reverse leaves none and re-sends the tail. Every insert, higher and equal record advances the generation; a lower one changes nothing. | Unit + Integration |
| TS-8 | FR8, NFR3: the allocation-free identification over the osc_identify corpus plus invalid UTF-8, overflow, empty and long bodies, with the test-only thread-local counter armed around its calls only. | Zero allocations. Results equal the recover-then-identify reference. | Unit |
| TS-9 | NFR5 (TM-2): adversarial inputs: designator chains through the strip, the filter and the as-05 rule; switch sequences straddling many reads; an OSC held near the 512 KiB cap and closed at a cut and at the fallback; a 1 MiB OSC body and a one-million-digit OSC number. | Every case finishes within its test time budget and does not panic. No empty `PtyOutput` other than EOF is sent. | Unit (performance) |
| TS-10 | NFR6: the CLI-only build command above. | Exit code 0. | Build |
| EC-1 | FR1, FR6: a carried-over sequence completes before a cut, and an incomplete construct after it is closed by the cut. | The completion is still reported with the same bytes and fed end. The closed construct is not written. | Unit |
| EC-2 | FR1: an awaiting-designator `ESC (` at a cut. | Still emitted, and the flag is cleared (round2 as-08 handling). | Unit |
| EC-3 | FR2: an overflow flush starting in the awaiting-designator state. | The state is passed to the strip, and the flag afterwards equals the client-parity state at the end of the flushed run. | Unit |
| TS-11 | FR9: inspect `feature-docs/mux-suppressed-output-round3-fixes/DECISIONS.md`, the libtest listing and the diff against `workflow.implement.base_commit`. | Eight rows, one per stable_id, each with verdict, rationale and regression test. Every registry test path appears in the listing as a test. `feature-docs/mux-suppressed-output-round2-fixes/reviews/round1.yaml` is unchanged. | Inspection |
| TS-12 | SPEC AC-8, NFR1: run the full suite, and compare the diff of existing tests with IMPLEMENTATION.md's changed-expectation registry. Check `test-docs/mux-suppressed-output-round2-fixes/task0004.tests.yaml`. | All tests pass. Only the registry's tests change expectation. The renamed tests resolve in the listing. The predecessor record names the new FR7 test with a supersede note naming this SPEC's FR7, and its `red_reason` is unchanged. | Integration + Inspection |
| TS-13 | NFR2: task0004's full-channel test, where another thread takes `output_target` and then the boundary exclusion while the reader is blocked securing a slot; the P4 free-exclusions check; the existing concurrent snapshot-path stress test. Review the diff for blocking waits under either lock. | All tests pass. No `reserve_owned` or `blocking_send` runs under `output_target` or the boundary exclusion. Scanning, prepare and notification processing run before the locks are re-taken. | Integration + Review |
| TS-14 | NFR1: diff check against `workflow.implement.base_commit` for `crates/mux_ipc/` and `src-tauri/src/mux/snapshot_bytes.rs`. Run the existing snapshot-assembly tests. | No changed file under either path. Existing snapshot-assembly tests pass unchanged. The replacement is still sent as ordinary `PtyOutput` chunks. | Inspection + Integration |
| TS-15 | NFR3: review the reader's non-suppressed path in the diff. | No new pass over the chunk. Designator tracking is inside the strip's single pass. The cut path reuses the existing boundary scan. The generation adds O(1) work per record and per covered check. | Review |

## Code Quality Verification
- Format: no format command is configured for this project. Do not run a
  crate-wide formatter; the diff must not touch files outside the tasks'
  declared file sets.
- Static analysis: the two build commands above.

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | Each of the 8 findings has a regression test that fails on the pre-fix code and passes after the fix | Run the `round3_` filter command; the eight registry paths from IMPLEMENTATION.md pass. Red-phase evidence in each task's record under `test-docs/mux-suppressed-output-round3-fixes/`. |
| AC-2 | Snapshot + replacement + later chunks equal the raw-stream reference for FR1, FR4, FR5, FR6 and FR7, with no replacement character and no stray `(` | TS-1, TS-4, TS-5, TS-6, TS-7 |
| AC-3 | No color query or CSI query is delivered or answered in the FR1, FR4 and FR5 cases | TS-1, TS-4, TS-5 |
| AC-4 | Write-filter split invariance, including the two designator inputs | TS-2, EC-3 |
| AC-5 | The FR3 input keeps its bytes in both strips, one call or split | TS-3 |
| AC-6 | The tail decision uses the record current at send time; at an equal boundary the later record wins | TS-6, TS-7 |
| AC-7 | Zero allocations and reference equivalence for the new identification | TS-8 |
| AC-8 | Existing tests pass unchanged except the listed ones; predecessor records updated for renames | TS-12 |
| AC-9 | The test command and the CLI-only build pass | Test Verification and Build Verification commands, TS-10 |
| AC-10 | Decision table for all 8 stable_ids; round1.yaml unchanged | TS-11 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0002 | TS-1 |
| FR2 | task0002 | TS-2 |
| FR3 | task0002 | TS-3 |
| FR4 | task0002 | TS-4 |
| FR5 | task0003 | TS-5 |
| FR6 | task0004 | TS-6 |
| FR7 | task0004 | TS-7 |
| FR8 | task0001 | TS-8 |
| FR9 | task0005 | TS-11 |
| NFR1 | task0002, task0004 | TS-12, TS-14 |
| NFR2 | task0004 | TS-13 |
| NFR3 | task0001, task0002, task0004 | TS-8, TS-15 |
| NFR4 | task0002, task0003, task0004 | TS-1, TS-4, TS-5, TS-6 |
| NFR5 | task0001, task0002, task0003, task0004 | TS-9 |
| NFR6 | task0001, task0002, task0003, task0004 | TS-10 |

## Manual Testing (E2E Not Possible)

The project has no E2E framework (SPEC.md: existing E2E tests none). The
following checks are confirmatory only. They need a release build and a
restarted mux daemon, and a human performs them.
- [ ] MT-1: In a mux pane, alternate between a full-screen program (for
      example neovim or `less`) and shell output while switching tabs
      repeatedly. After each return, no response text (such as an OSC 11
      color report or a cursor position report) appears on the shell line.
- [ ] MT-2: In a mux pane, run a program that draws borders with the line
      drawing character set (for example `mc` or `dialog`) while switching
      tabs. After each return the borders render correctly, and no stray
      `(`, `0` or `B` appears.

## Performance / Security Verification
- NFR3: no new pass on the reader's non-suppressed path, and no per-OSC
  copy on the write path — TS-15 (review) and TS-8 (zero allocations).
- NFR5: bounded single passes, no panic, cap unchanged, no empty
  replacement chunk — TS-9.
- TM-1: never fabricate a query or viewer launch at a position where the
  client parser does not start a control sequence: closed constructs
  dropped from the ring, designators honored in the strip and the filter,
  the fallback closing held constructs, and the as-05 trailing-designator
  rule. Checked by the negative cases of TS-1, TS-3, TS-4 and TS-5 (no
  response, no fabricated or removed launch). The registry tests of
  task0002 and task0003 must pass.
- TM-2: every added or changed pass is a bounded single pass without
  panic, and no empty replacement is sent. Checked by TS-9, by a review of
  the changed passes for panicking arithmetic or indexing, and by the
  empty-replacement assertions of task0002 AC-6 and task0004 AC-3.
- TM-3: the tail-omission decision uses only the current record of the
  destination that received the covering snapshot, and the replacement is
  sent only there. Checked by TS-6 (including the other-destination case)
  and TS-7's record-rule tests.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios (TS-1 to TS-15) | 15 | 12 | 0 | 3 (TS-11, TS-15 inspection or review; TS-13 partly review) |
| Edge cases (EC-1 to EC-3) | 3 | 3 | 0 | 0 |
| Success criteria (AC-1 to AC-10) | 10 | 9 | 0 | 1 (AC-10 inspection) |
| Manual checks (MT-1, MT-2) | 2 | 0 | 0 | 2 |
| Performance (NFR3, NFR5) | 2 | 1 | 0 | 1 (NFR3 review) |
| Security (TM-1, TM-2, TM-3) | 3 | 3 | 0 | 0 |
