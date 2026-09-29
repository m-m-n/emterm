# Verification Document: mux-suppressed-output-round2-fixes

## Overview
**Feature**: mux-suppressed-output-round2-fixes / **SPEC.md**: `feature-docs/mux-suppressed-output-round2-fixes/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-suppressed-output-round2-fixes/IMPLEMENTATION.md`

All commands run from the project root (the integration worktree root),
never from `src-tauri/`.

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors (CLI-only build; every touched module is
  CLI-shared).

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Registry tests only (all eight regression tests share the `round2_` name
  prefix): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib round2_`
- Coverage target: not measured; no coverage tool is configured for this
  project. Completeness is judged by the Acceptance Criteria → test mapping
  of each task plan.
- Known nondeterministic existing tests: `tabs` replay tests (parallel run)
  and the `tmux_sockets` discover test. A failure in one of them is re-run
  alone with `--test-threads=1` before it counts as a failure.

### Test Scenarios from SPEC.md

TS-1 to TS-10 come from SPEC.md. TS-11 to TS-15 are added by this plan so
that every NFR and FR9 has a verifying scenario.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | FR1, NFR4: suppress a chunk with each of three full 256-byte windows: `ESC ( ESC ]10;` plus 249 spaces with chunk `?` BEL; `ESC ( ESC [` plus 252 NUL bytes with chunk `c`; an `ESC ( ESC ( …` chain. Then repeat with a window containing a decidable ESC. | The three designator-slot cases give an empty replacement and no response. The decidable case starts scanning there and delivers the query exactly once. | Unit |
| TS-2 | FR2: on the main screen, suppress a chunk holding `ESC ]11;?` BEL. Apply the snapshot with `reset_and_replay_segments` (responses discarded), then feed the replacement. Repeat through the reader for visible reattach, on-demand and visibility restore. | Exactly one response in every case. | Integration |
| TS-3 | FR3, NFR4: feed `ESC ( ESC ]11;?tail` in two calls split right after `ESC (`. Then run split input → suppression → replacement → BEL. | Emitted bytes and pending equal the single-call result. No color-query response. | Unit + Integration |
| TS-4 | FR4, NFR4: on the main screen, suppress a chunk of `ESC ]11;?`, `ESC [?47h`, `ESC [?47l` and one space, then send BEL after the replacement. Repeat for 1047 and 1049. | No response. No terminated OSC remains in pending after the feed. | Unit + Integration |
| TS-5 | FR5: suppress a main-screen chunk `ESC ]2;x`, `ESC [?1049h`, `ESC [6n`, `ESC [?1049l`, `y`. Repeat with a genuinely incomplete sequence left in the last main range. | CSI 6n is delivered once. Only the incomplete sequence becomes the tail, and the preceding alternate-range queries and launches are delivered. | Integration |
| TS-6 | FR6: split an OSC 777 emterm markdown longer than 256 bytes across reads and suppress the completing chunk. Variants: a short launch starting in the window; two identical launches; mixed with alternate-range queries; a carried-over color query. | Delivered once after the snapshot. The short launch is delivered once (dedup). Identical launches are delivered twice. Order follows the raw stream. The color query is answered once. | Integration |
| TS-7 | FR7: place `0777;emterm;markdown;…` and `09999;emterm-md;…` in a suppressed main-screen chunk, and also in an alternate-screen range. Also exercise `777emterm;;markdown;…`, `0777;emterm;image;…`, agent-status, fold, emterm-mux and an overflowed number. | The first two are absent from the snapshot and delivered once in total, and once from the alternate range. `777emterm;;markdown;…` is stripped and identified. Image is stripped and not delivered. Agent-status is stripped. Fold and emterm-mux are kept. The overflowed number is not identified. | Unit |
| TS-8 | FR8: visibility restore of a main-screen pane, with the ring not wrapped and with the ring wrapped and an empty screen dump. Split mid UTF-8, right after `ESC (`, and inside an incomplete CSI, and suppress the first half. Then the same with a query before the tail (coexistence). | Display, cursor and parsing of the following chunks match the reference, with no U+FFFD and no `(` shown. In coexistence the query is answered once and parsing matches. The existing visible-reattach tail tests pass unchanged. | Integration |
| TS-9 | NFR5 (TM-2): adversarial inputs: designator chains, repeated switch sequences with incomplete introducers, long carried-over sequences, cap overflow, 2 MiB snapshot payloads for the trailing-construct decider. | Replacement assembly, the write filter and the decider finish within the test time budget and do not panic. | Unit (performance) |
| TS-10 | NFR6: the build command above. | Exit code 0. | Build |
| TS-11 | NFR1: for a fixed ring and shadow state, compare the snapshot bytes and frames of visibility restore, reattach and on-demand with the pre-feature assembly output. Run the existing snapshot-bytes and `mux_ipc` tests. | Byte-identical, except the FR7 strip-target change. Existing tests pass unchanged. | Integration |
| TS-12 | NFR2: review the diff for lock order (`output_target` → capture exclusion → ring / shadow parser), for any `blocking_send` under `output_target`, and for new work under the capture exclusion. Run the existing capture/boundary concurrency tests (pause hooks) and task0004's same-hold test. | No new nesting or held-lock sends. The decider runs outside the capture exclusion. All the listed tests pass. | Review + Integration |
| TS-13 | NFR3: review the reader diff for the non-suppressed path. | No new pass over the chunk. Added work is limited to state recorded inside the write filter's existing scan and cuts derived from the span list. | Review |
| TS-14 | NFR4 (TM-3): back-to-back snapshots to one destination, two destinations for one pane, and a replacement after a destination change. | Each chunk uses the trailing construct of the record that covers it for its own destination. The replacement goes only to the destination captured at the suppression decision. | Integration |
| TS-15 | FR9: inspect `feature-docs/mux-suppressed-output-round2-fixes/DECISIONS.md` and the diff. | Eight rows, one per stable_id, each with verdict, rationale and registry test. Each registry test exists under its pinned name. `feature-docs/mux-suppressed-output-fixes/reviews/round2.yaml` is unchanged. | Inspection |

## Code Quality Verification
- Format: no format command is configured for this project. Do not run a
  crate-wide formatter; the diff must not touch files outside the tasks'
  declared file sets.
- Static analysis: the build command above, plus the default-feature
  compile that the test command performs.

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | Each of the 8 findings has a regression test that fails on the pre-fix code and passes after the fix | Run the `round2_` filter command and check that the eight registry names from IMPLEMENTATION.md pass. Check red-phase evidence in each task's record under `test-docs/mux-suppressed-output-round2-fixes/`. |
| AC-2 | Snapshot + replacement + following chunks equals the raw-stream reference (screen, cursor, responses, displayed characters) for FR3/FR4/FR5/FR6/FR8, with no continuation byte or U+FFFD displayed | TS-3 to TS-6 and TS-8 (task0003 AC-5, task0004 AC-4/AC-5) |
| AC-3 | Queries arrive exactly once, in order, after the snapshot. No already-delivered, fabricated or terminated query arrives. | TS-1 to TS-5, TS-8 coexistence |
| AC-4 | Viewer launches arrive exactly once, including launches that began before the window and leading-zero launches. Identical launches each arrive. Images do not arrive. | TS-6, TS-7 |
| AC-5 | Write-filter output and pending are split-invariant, and the pending postcondition holds with switch sequences | TS-3, TS-4 (task0003 AC-1/AC-2) |
| AC-6 | Snapshot bytes unchanged except for the FR7 exception; wire format unchanged | TS-11 |
| AC-7 | Decision table with 8 rows; round2.yaml unchanged | TS-15 |
| AC-8 | Existing tests pass unchanged except the listed behavior-changing tests | Full test run; compare the diff of existing tests with the list below |
| AC-9 | Test and build commands pass | Test Verification and Build Verification commands |

### Behavior-changing tests (SPEC AC-8)
Only these existing tests may change expectation. They are renamed to
describe the new behavior, and DECISIONS.md lists their old and new names:
- `osc_color_query_inside_ring_written_ranges_is_not_redelivered` (`src-tauri/src/mux/ipc/pty_spawn/suppressed_output.rs`)
- `visible_reattach_redelivers_an_alt_screen_color_query_but_not_one_that_reached_the_ring` (`src-tauri/src/mux/ipc/pty_spawn/tests.rs`)
- `on_demand_snapshot_redelivers_an_alt_screen_color_query_but_not_one_that_reached_the_ring` (`src-tauri/src/mux/ipc/pty_spawn/tests.rs`)

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0002 | TS-1 |
| FR2 | task0002 | TS-2 |
| FR3 | task0003 | TS-3 |
| FR4 | task0003 | TS-4 |
| FR5 | task0003 | TS-5 |
| FR6 | task0003 | TS-6 |
| FR7 | task0001 | TS-7 |
| FR8 | task0004 | TS-8 |
| FR9 | task0002 | TS-15 |
| NFR1 | task0001, task0004 | TS-11 |
| NFR2 | task0004 | TS-12 |
| NFR3 | task0003, task0004 | TS-13 |
| NFR4 | task0002, task0003, task0004 | TS-1, TS-3, TS-4, TS-14 |
| NFR5 | task0001, task0002, task0003, task0004 | TS-9 |
| NFR6 | task0001, task0004 | TS-10 |

## Manual Testing (E2E Not Possible)

The project has no E2E framework (SPEC.md: existing E2E tests none). The
following checks are confirmatory only. They need a release build and a
restarted mux daemon, and a human performs them.
- [ ] MT-1: In a mux pane, run a loop that keeps printing mixed Japanese
      text with colors. Switch tabs away and back repeatedly. After each
      return, no stray `(`, no U+FFFD and no garbled line appears.
- [ ] MT-2: In a mux pane, start a program that sends a color query at
      startup (for example neovim or Claude Code) while switching tabs
      rapidly. The program does not hang waiting for a response, and no
      response text appears on the shell line.
- [ ] MT-3: In a mux pane, run `emterm markdown` on a large Markdown file
      while switching tabs. The viewer window opens exactly once.

## Performance / Security Verification
- NFR3: no new pass on the reader's non-suppressed path — TS-13 (review of
  the reader diff).
- NFR5: bounded single passes, no panic, cap unchanged — TS-9.
- TM-1: never fabricate a query or viewer launch from a position where the
  client parser does not start a sequence (restart exclusion,
  awaiting-designator carry-over, closing at removed switches, dedup of
  carried-over completions). Checked by the negative cases of TS-1, TS-3,
  TS-4 and TS-6 (no response, no second launch). The registry tests of
  task0002 and task0003 must pass.
- TM-2: every added or changed scan and decision is a bounded single pass
  without panic, and no empty replacement is sent. Checked by TS-9, plus a
  review of the changed scans for panicking arithmetic or indexing, plus
  the existing empty-replacement test.
- TM-3: the replacement and FR8's trailing construct come only from the
  record tied to the destination that received the snapshot. Checked by
  TS-14 and task0004's record-rule tests (AC-2, AC-6).

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Test scenarios (TS-1 to TS-15) | 15 | 12 | 0 | 3 (TS-12 partly, TS-13, TS-15: review / inspection) |
| Success criteria (AC-1 to AC-9) | 9 | 8 | 0 | 1 (AC-7 inspection) |
| Manual checks (MT-1 to MT-3) | 3 | 0 | 0 | 3 |
| Performance (NFR3, NFR5) | 2 | 1 | 0 | 1 (NFR3 review) |
| Security (TM-1, TM-2, TM-3) | 3 | 3 | 0 | 0 |
