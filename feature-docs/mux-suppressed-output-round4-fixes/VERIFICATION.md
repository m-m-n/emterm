# Verification Document: mux-suppressed-output-round4-fixes

## Overview
**Feature**: mux-suppressed-output-round4-fixes / **SPEC.md**: `feature-docs/mux-suppressed-output-round4-fixes/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-suppressed-output-round4-fixes/IMPLEMENTATION.md`

Run every command from the project root (the integration worktree root),
never from `src-tauri/`.

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only, NFR6): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0 and no errors, for both.

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Registry and round4 modules only: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib round4_`
- Test-name resolution: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list`
- Coverage target: not measured, because no coverage tool is configured
  for this project. Each task plan's Acceptance Criteria and their test
  mapping decide completeness.
- Known nondeterministic existing tests: the `tabs` replay tests (in a
  parallel run) and the `tmux_sockets` discover test. A failure in one of
  them counts only if it also fails when re-run alone with
  `--test-threads=1`.

### Test Scenarios from SPEC.md

TS-1 to TS-11 and EC-1 to EC-6 come from SPEC.md. This plan adds TS-12 to
TS-16, so that FR6, SPEC AC-7 and every NFR have a verifying scenario.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | FR1, NFR4: main screen, ring not wrapped, a visibility restore. A chunk around each of the three FR1 forms is suppressed or covered by the snapshot: `ESC]11;? ESC]0;x`, `ESC]11;? ESC ESC`, `X ESC ESC`. The form is placed before each 47 / 1047 / 1049 `h`/`l` pair, and a later read holds BEL, `\` or `]11;? BEL` respectively. Repeated with the chain head emitted by an earlier read. | No response. The ring holds no OSC introducer from the chain. The client equals the raw-stream reference in screen, cursor, responses and displayed characters. | Integration |
| TS-2 | FR1: write-filter split invariance and the pending postcondition over a chain corpus: `ESC]11;? ESC]0;x`, `ESC]11;? ESC ESC`, `X ESC ESC`, a DCS aborted by an OSC, and an APC aborted by a lone ESC. Covers every split position, byte-at-a-time feeding, and cuts at every position. | Emitted bytes and `pending` equal the single-call result. After every call, `pending` is empty or exactly the chain that ends in the incomplete construct after the last cut. When a cut closes a chain, no byte from its head on is emitted. | Unit |
| TS-3 | FR1: carried-over completion with a held chain; the tail re-delivery and excluded pieces for a suppressed chunk whose `pending` holds a chain. | The construct at the construct start completes in the next read and is reported with its own bytes and fed end. An aborted chain head is never reported. No byte is both an item and part of the tail. | Unit |
| TS-4 | FR1, FR3: overflow paths. A chain held near the 512 KiB cap overflows, then a cut follows. A flushed run that ends waiting for a designator is followed by a cut. | The filter state after the flush matches its definition. The second case writes one ESC at the cut. No query appears in the reference comparison. | Integration |
| TS-5 | FR2, NFR4: an as-05 fallback window (full window, no decidable ESC, trailing `ESC (`). Chunks: `ESC ( ESC ( ESC ]11;? BEL`, and `ESC ( ESC ( ESC [` followed by `6n`. Control: constructs both readings agree on, and the same chunks behind a ground window. Also windows outside the fallback condition. | No item for the first chunk, no tail for the second, no response. Agreed items are kept. Windows outside the condition are unaffected, and the existing `round3_as05_*` results hold. | Unit + Integration |
| TS-6 | FR3: one ESC is written after a waiting `ESC (` / `ESC )` on each path: a cut in the same call; a cut at fed 0 after an earlier read left the wait with an empty `pending`; a cut after an overflow flush whose run ended waiting; and the `mod.rs` fallback closing. Also the cases with no designator awaited. | Exactly one ESC on every waiting path, with the flag cleared afterwards. Nothing extra when no designator is awaited. | Unit + Integration |
| TS-7 | FR3, NFR4: the task example `X ESC ( ESC[?1049h ESC[?1049l ESC ( ESC ]11;? BEL` through snapshot + replacement + following chunks. | No OSC 11 response. term_core's parse of the following bytes and its responses equal the raw-stream reference. | Integration |
| TS-8 | FR4: conditional path check. `ESC[6`, then a switch, then `n`, in one read and split across reads, with a visibility-restore snapshot in between, compared with the raw-stream reference in term_core. | The decision record states whether it reproduced. If it did, after the fix: no response the reference does not produce, and the client equals the reference. | Integration |
| TS-9 | FR5: conditional path check. `ESC (` / `ESC )` directly followed by `ESC[?1049h` / `ESC[?1047h` / `ESC[?47h` (and the `l` forms), within one read and with `ESC (` ending the previous read. Compared with the raw-stream reference (screen, cursor, responses, displayed characters) in term_core. | The decision record states whether it reproduced. If it did, after the fix the client equals the reference, or a residual outside the fix boundary is recorded and reported as a plan deviation. | Integration |
| TS-10 | NFR3, NFR5: adversarial input. Long aborted-string chains near the cap, fed whole and byte by byte. Long `ESC (` chains in the fallback window with FR2's two readings. Repeated switch sequences straddling reads. Long CSI parameter runs straddling reads (if FR4 reproduced). | Every case finishes within its test time budget and does not panic. No empty `PtyOutput` other than EOF is sent. | Unit (performance) |
| TS-11 | NFR6: the CLI-only build command above. | Exit code 0. | Build |
| EC-1 | FR1: a chain whose head lies in an earlier read. | It is held across reads and dropped from its head when a cut closes it. | Unit |
| EC-2 | FR1: a chain that settles before the cut. | It is written through the strip. The carried-over completion is reported for the construct start. The construct opened after it, closed by the cut, is not written. | Unit |
| EC-3 | FR1: a chain made of DCS/APC strings, such as a DCS aborted by `ESC ]`. | It is dropped from its head, so a later ST cannot complete the DCS in the ring. | Unit |
| EC-4 | FR3: a complete designation (`ESC ( A`) before a cut. | No extra ESC is written. | Unit |
| EC-5 | FR3, FR5: the switch sequence sits in the designator slot. | FR3's ESC write prevents fabrication whatever FR5 finds; responses equal the reference. A remaining display difference belongs to FR5 (TS-9). | Integration |
| EC-6 | FR1, NFR5: a long run of aborted strings grows the held chain to the 512 KiB cap and takes the overflow flush. | No panic. The post-flush state matches its definition. | Unit |
| TS-12 | FR6, SPEC AC-9: inspect `feature-docs/mux-suppressed-output-round4-fixes/DECISIONS.md`, the libtest listing, and the diff against `workflow.implement.base_commit`. | Three decision rows (3e2024dce619ed9f FR1, 989ec5c588abce06 FR2, 48caec6f5b0b5810 FR3), each with verdict, rationale and regression test. A separate section records the FR4 and FR5 outcomes. The behavior-changing tests are listed with old name, new name, file and reason. No region reads `pending (taskNNNN)`. Every registry path from IMPLEMENTATION.md appears in the listing as a test. `feature-docs/mux-suppressed-output-round3-fixes/reviews/round1.yaml` is unchanged. | Inspection |
| TS-13 | SPEC AC-7, NFR1: run the full suite, and compare the diff of existing tests with IMPLEMENTATION.md's changed-expectation registry and DECISIONS.md. Check `test-docs/mux-suppressed-output-fixes/task0002.tests.yaml`, `test-docs/mux-suppressed-output-round2-fixes/task0003.tests.yaml` and `test-docs/mux-suppressed-output-round3-fixes/task0002.tests.yaml`. | All tests pass. Only the registered tests (plus any recorded in DECISIONS.md as a reported deviation) change expectation. Renamed tests resolve in the listing. The predecessor records name the current tests, carry supersede notes naming this SPEC's FR1 / FR3, and keep `red_reason` unchanged. | Integration + Inspection |
| TS-14 | NFR1: diff check against `workflow.implement.base_commit` for `crates/mux_ipc/` and `src-tauri/src/mux/snapshot_bytes.rs`. Run the existing snapshot-assembly tests. | No changed file under either path. The existing snapshot-assembly tests pass unchanged. The replacement is still sent as ordinary `PtyOutput` chunks. Ring content changes only in the ways NFR1 (a), (b), (c) state. | Inspection + Integration |
| TS-15 | NFR2: review the diff for lock acquisition and blocking waits in the capture step, the suppressed pipeline and the client-parity scan. Run the existing concurrent snapshot-path tests. | The lock order stays `output_target`, then capture exclusion, then ring / shadow parser, and `output_target`, then boundary exclusion. No new lock. No blocking wait under `output_target` or the boundary exclusion. FR2's second reading runs outside the capture and boundary exclusions. Existing tests pass. | Integration + Review |
| TS-16 | NFR3: review the reader's non-suppressed path in the diff. | No new pass over the chunk. Chain-head, construct-start and CSI tracking are inside the existing boundary scan. Designator-slot tracking is inside the existing extraction pass. The closing writes are O(1). FR2's second reading runs only under the as-05 fallback condition. | Review |

## Code Quality Verification
- Format: no format command is configured for this project. Do not run a
  crate-wide formatter. The diff must not touch files outside the tasks'
  declared file sets.
- Static analysis: the two build commands above.

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | Each of the 3 findings has a regression test that fails on the pre-fix code and passes after the fix | Run the `round4_` filter command; the three FR1-FR3 registry paths from IMPLEMENTATION.md pass. Red-phase evidence is in each task's record under `test-docs/mux-suppressed-output-round4-fixes/`. |
| AC-2 | The term_core comparison (snapshot through `reset_and_replay_segments`, then replacement and following chunks) matches the raw-stream reference for FR1's three forms (in one read, and with the chain head in an earlier read) and for FR3 on every path | TS-1, TS-4, TS-6, TS-7, EC-5 |
| AC-3 | No color query or CSI query is delivered or answered for the three task examples | TS-1, TS-5, TS-7 |
| AC-4 | Write-filter split invariance and the restated pending postcondition over chains | TS-2, EC-1, EC-3 |
| AC-5 | Items and tails agreed by both readings are still delivered; the `round3_as05_*` results hold | TS-5 |
| AC-6 | FR4 and FR5 each have a term_core regression test, and the record states whether each reproduced; a reproduced one fails before the fix and passes after | TS-8, TS-9, TS-12 |
| AC-7 | Existing tests pass unchanged except the recorded ones; predecessor records are updated | TS-13 |
| AC-8 | The test command and the CLI-only build pass | Test Verification and Build Verification commands, TS-11 |
| AC-9 | The decision table records all 3 stable_ids, with FR4/FR5 in a separate section; round1.yaml is unchanged | TS-12 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-3, TS-4 |
| FR2 | task0003 | TS-5 |
| FR3 | task0002 | TS-4, TS-6, TS-7 |
| FR4 | task0004 | TS-8 |
| FR5 | task0005 | TS-9 |
| FR6 | task0001, task0002, task0003, task0004, task0005 | TS-12 |
| NFR1 | task0001, task0002, task0004, task0005 | TS-13, TS-14 |
| NFR2 | task0003, task0005 | TS-15 |
| NFR3 | task0001, task0002, task0003, task0004, task0005 | TS-10, TS-16 |
| NFR4 | task0001, task0002, task0003, task0004, task0005 | TS-1, TS-5, TS-7, TS-8, TS-9 |
| NFR5 | task0001, task0002, task0003, task0004, task0005 | TS-10 |
| NFR6 | task0001, task0002, task0003, task0004, task0005 | TS-11 |

## Manual Testing (E2E Not Possible)

The project has no E2E framework (SPEC.md lists no existing E2E tests).
The following checks only confirm the automated results. They need a
release build and a restarted mux daemon, and a person performs them.
- [ ] MT-1: In a mux pane, alternate between a full-screen program (for
      example neovim or `less`) and shell output while switching tabs
      repeatedly. After each return, no response text (such as an OSC 11
      color report or a cursor position report) appears on the shell line.
- [ ] MT-2: In a mux pane, run a program that draws borders with the line
      drawing character set (for example `mc` or `dialog`) while switching
      tabs. After each return the borders render correctly, and no stray
      `(`, `0` or `B` appears.

## Performance / Security Verification
- NFR3: no new pass on the reader's non-suppressed path; FR2's second
  reading only under the as-05 fallback condition. Checked by TS-16
  (review) and the byte-at-a-time cases of TS-10.
- NFR5: bounded single passes, no panic, cap unchanged with a held chain
  counted toward it, no empty replacement chunk. Checked by TS-10 and EC-6.
- TM-1: never fabricate a query or viewer launch at a position where the
  client parser does not start a control sequence. The mitigations: chains
  closed by a cut are dropped from their head, and held across reads until
  they settle; the as-05 fallback keeps only results both readings agree
  on; one ESC follows an awaiting designator at a cut; and, if they
  reproduce, an open CSI is closed at a cut and designator-slot switches
  are extracted as term_core parses them. Checked by the negative cases of
  TS-1, TS-5, TS-6, TS-7, TS-8 and TS-9 (no response, no fabricated item or
  tail). The registry tests of all five tasks must pass.
- TM-2: every added or changed pass is a bounded single pass without
  panic. A held chain counts toward the 512 KiB cap and takes the
  strip-filtered overflow flush. No empty replacement is sent. Checked by
  TS-10 and EC-6, and by a review of the changed passes for panicking
  arithmetic or indexing.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios (TS-1 to TS-16) | 16 | 13 | 0 | 3 (TS-12 inspection; TS-15 partly review; TS-16 review) |
| Edge cases (EC-1 to EC-6) | 6 | 6 | 0 | 0 |
| Success criteria (AC-1 to AC-9) | 9 | 8 | 0 | 1 (AC-9 inspection) |
| Manual checks (MT-1, MT-2) | 2 | 0 | 0 | 2 |
| Performance (NFR3, NFR5) | 2 | 1 | 0 | 1 (NFR3 review) |
| Security (TM-1, TM-2) | 2 | 2 | 0 | 0 |
