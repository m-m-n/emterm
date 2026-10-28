# Investigation: read timeout in the visible-reattach colour-query test

Target test:
`mux::ipc::pty_spawn::tests::visible_reattach_redelivers_alt_screen_and_main_screen_color_queries_once`
(`src-tauri/src/mux/ipc/pty_spawn/tests.rs:5101-5170`).

Investigated on 2026-10-29 at the tip of the integration branch. No tracked file
under `src-tauri/` or `crates/` was changed or instrumented
(`git diff -- src-tauri crates` is empty before and after).

## 1. Conclusion

Category (2): a problem in the test's wait/synchronisation design. Scheduling
jitter under the parallel `--lib` run is what exposes it, but it is not a
tolerable transient. Category (1) does not apply and category (3) (mux body
behaviour) is not supported by anything observed.

The cause: the test re-arms the P2 pause point **after** it has released the
reader thread. `PauseHook::hit` is a no-op when nothing is armed
(`src-tauri/src/mux/session/pane/output_capture.rs:413-419`), so if the reader
thread reaches the P2 point of the second chunk before the test thread has
executed the second `arm()` (`tests.rs:5142`), the "arrived" signal is never
sent. The test then waits out its full 5 s `recv_timeout` at `tests.rs:5143-5145`
and fails with `the reader must reach P2 on the main-buffer chunk: Timeout`.
The signal is lost, not late; a longer timeout would not help.

Supporting facts (details in the sections below):

- All observed failures are at the second wait (`tests.rs:5145:10`): 3 of 51
  full `--lib` runs (5.9 %). The first wait (`tests.rs:5128`), which gets the
  same load but is armed before the reader thread exists, failed 0 times. Four
  earlier `test-docs` records carry the second wait's text (two for the target
  test, two for its on-demand counterpart) and none carries the first wait's
  text (section 6).
- Failing waits took 5.087 s and 5.081 s, i.e. the timeout bound; passing runs
  of the test take 0.000-0.003 s. There is no "slow but would have arrived"
  population.
- The repository already treats this exact ordering as a defect in sibling tests
  and fixed it there (`tests.rs:1885-1893`, `tests.rs:6363-6371`,
  `tests.rs:6696-6698`, `tests/round3_write_path.rs:663-664`; commit `685acb17`
  says "closing the race where the reader could blow past P2 before the test
  re-armed it"). The target test and its on-demand counterpart
  (`tests.rs:5406-5408`) still use the release-then-arm order.
- Nothing on the reader's path between the two P2 points can block in this
  test (section 4), so the mux body is not what is waiting.

Limit of the evidence: the lost-signal interleaving itself was not observed
directly, because no instrumentation of tracked files was allowed and no CPU
load was to be added. It is established by code reading, by the first-wait /
second-wait asymmetry, by the elapsed times, and by the repository's own
record of the same race in sibling tests.

## 2. Wait points of the target test

Test thread = the thread running the `#[tokio::test]` body. Reader thread = the
`pty_reader_loop` thread started by `spawn_reader_with_chunks_in_session`.

| # | Location | Waits for | Timeout | Wait style | On timeout / stall |
|---|---|---|---|---|---|
| W1 | `tests.rs:5126-5128` | Reader thread's first P2 hit (chunk 1, the alternate-screen chunk) sending `arrived` (`output_capture.rs:416`) | 5 s | Blocking `std::sync::mpsc::Receiver::recv_timeout`; the thread parks, no polling | `expect` panics at `tests.rs:5128:10`: `the reader must reach P2 on the alt-screen chunk: Timeout`. P2 was armed at `tests.rs:5121`, before the reader thread is spawned (`tests.rs:5122-5124`), so this wait has no ordering race |
| W2 | `tests.rs:5143-5145` | Reader thread's second P2 hit (chunk 2, the main-buffer chunk) | 5 s | Same as W1 | `expect` panics at `tests.rs:5145:10`: `the reader must reach P2 on the main-buffer chunk: Timeout`. P2 is armed at `tests.rs:5142`, after the release at `tests.rs:5137` |
| W3 | `tests.rs:5133-5136`, `5147-5150` | `collect_reattach_data(..).await`, which awaits `session_manager.lock()` (`src-tauri/src/mux/ipc/reattach.rs:114`, tokio mutex) | none | Async await | Would hang the test; uncontended here (nothing else touches the manager) |
| W4 | `tests.rs:4826-4828` | `mgr.lock().await` on a freshly created `SessionManager` inside the spawn helper | none | Async await | Uncontended |
| W5 | `tests.rs:5153` | `handle.join()` of the reader thread | none | Blocking join | Would hang; the thread ends after chunk 2 and the EOF read once `release_tx_2` is sent (`tests.rs:5151`) |
| W6 (reader side) | `output_capture.rs:417` | Inside `PauseHook::hit`: `release.recv()` until the test sends on `release_tx` / `release_tx_2` | none | Blocking `mpsc::Receiver::recv` | Released by `tests.rs:5137` and `tests.rs:5151` |
| - | `tests.rs:5155-5157` | `try_recv` drain after the join | none | Non-blocking, not a wait | Assertion failure only |

The only timeouts in the test are the two 5 s `recv_timeout` calls (W1, W2). A
"read timeout" failure of this test is therefore W1 or W2.

Reader-side facts relevant to the waits:

- Input is a `ScriptedReader` (`tests.rs:1140-1163`) that returns each scripted
  chunk immediately and then `Ok(0)`; it never blocks.
- P2 is hit once per read, after the capture step and before the forward
  decision (`src-tauri/src/mux/ipc/pty_spawn/mod.rs:607-610`).
- The destination channel has capacity 16 (`tests.rs:5130`) and receives at most
  three sends, so the `blocking_send` / `reserve_owned` paths
  (`mod.rs:417`, `mod.rs:789`, `mod.rs:996-1009`) are not reached.

## 3. Mechanism

T = test thread, R = reader thread.

1. T arms P2 (`tests.rs:5121`) and spawns R. R reads chunk 1, updates the shadow
   parser and ring, and enters `hit()`: it takes the armed state, sends
   `arrived`, and blocks in `release.recv()` (`output_capture.rs:414-417`).
2. T receives `arrived` (W1) and runs the first `collect_reattach_data`, which
   switches the pane to `Connected(new_tx)` and records the boundary for
   `new_tx`.
3. T sends the release (`tests.rs:5137`). R wakes up.
4. R finishes chunk 1 (suppressed pipeline, `mod.rs:942-1045`), reads chunk 2
   from the `ScriptedReader`, runs the shadow parser and ring write, and reaches
   `hit()` for chunk 2 (`mod.rs:610`).
5. T, after `send` returns, calls `p2.arm()` (`tests.rs:5142`;
   `output_capture.rs:399-407`: two channel allocations, one mutex lock, one
   store).

Steps 4 and 5 run concurrently. If T runs first, R finds the hook armed and the
test proceeds. If T is descheduled between the `send` in step 3 and the store
in step 5 for longer than R's path in step 4 (a debug build: ring write,
shadow-parser update), R passes an unarmed `hit()` (a no-op) and runs on to EOF
and exits. T then arms a hook that no thread will ever hit, and W2 times out at
5 s.

`hit()` takes the armed state (`Option::take`, `output_capture.rs:414`) before
it blocks, so arming the next pause while R is still blocked inside the previous
`hit()` is safe. That is what the fixed sibling tests do.

## 4. Why not category (1) or (3)

- Not (1): the failing interleaving is a property of the test's protocol, not
  of how much work the machine had. Waiting longer cannot recover it (the signal
  was never sent), so the 5 s value is not the problem and a timeout increase
  would not fix it. The flake rate (5.9 % of `--lib` runs) is also far above
  what a gate can tolerate, and the repository has already fixed the same order
  as a defect elsewhere.
- Not (3): the reader's path from the first release to the second P2 contains
  no blocking operation in this test: `ScriptedReader` does not block, the
  channel (capacity 16) is never full, the manager lock is uncontended, and
  `PauseHook::hit` is a no-op when unarmed. The first wait W1, which exercises
  the same reader code under the same load, never failed.
- Load is the trigger, not the cause: the race needs T to be preempted at a
  specific point. Failing runs had a 1-minute load average of about 8-9 at their
  end, inside the 4-14 range of passing runs, so these data do not show a
  dependence on load level (section 5). The earlier records describe the failure
  under load (load average above 6, or while the worktree was compiling), which
  is consistent with, but not proof of, a scheduling trigger.

## 5. Measurements

Environment: Linux, 24 logical CPUs, rustc 1.96.0 / cargo 1.96.0, test profile
(unoptimized + debuginfo), 4831 tests in the `--lib` suite (4828 run, 3
ignored). Other sessions' processes were running on the machine (1-minute load
average about 4 before the first run). No load was added; every run below was
started only after the previous one had finished.

Cargo commands were run from the project root:

- (a) `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib mux::ipc::pty_spawn::tests::visible_reattach_redelivers_alt_screen_and_main_screen_color_queries_once -- --exact`, 30 runs.
- (b) `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` (the project test command), 21 runs. Run 1 is the pre-edit baseline run.
- (c) Supplementary, to obtain the per-test elapsed time that stable libtest does not print: the test binary produced by those cargo runs, run from `src-tauri/` (as cargo does) with `RUSTC_BOOTSTRAP=1 ./target/debug/deps/emterm-48705c2bd8f909e6 -Z unstable-options --report-time`, whole `--lib` suite, 30 runs. `RUSTC_BOOTSTRAP=1` is read by libtest at run time to accept the unstable flag; no rebuild, same test selection and thread count as (b).

Results for the target test:

| Condition | Runs | Passed | Failed | Failing wait | Elapsed of failing test |
|---|---|---|---|---|---|
| (a) target test alone, cargo | 30 | 30 | 0 | - | - (each run: libtest `finished in 0.00s`; cargo wall time 6.00-6.46 s, mean 6.17 s, almost all of it the crate rebuild cargo performs on every invocation) |
| (b) `--lib`, cargo | 21 | 20 | 1 (run 4) | W2, `tests.rs:5145:10` | cargo/stable libtest does not print it; the failing call is the 5 s `recv_timeout`, so >= 5 s |
| (c) `--lib`, test binary with `--report-time` | 30 | 28 | 2 (runs 7, 28) | W2, `tests.rs:5145:10` | 5.087 s and 5.081 s |
| (b) + (c) `--lib` combined | 51 | 48 | 3 (5.9 %; 95 % Wilson interval about 2-16 %) | W2 in 3 of 3, W1 in 0 | - |

Notes on the numbers:

- Passing runs of the target test in (c): 0.000-0.003 s (n = 28).
- (a) has 0 failures in 30 runs (95 % upper bound about 10 %). With 0/30 versus
  3/51 the difference between "alone" and "inside `--lib`" is not statistically
  established (one-sided p of about 0.24); the data do not support a claim about
  load dependence in either direction.
- The failure text, identical in (b) run 4 and (c) runs 7 and 28:

  ```
  thread 'mux::ipc::pty_spawn::tests::visible_reattach_redelivers_alt_screen_and_main_screen_color_queries_once' (...) panicked at src-tauri/src/mux/ipc/pty_spawn/tests.rs:5145:10:
  the reader must reach P2 on the main-buffer chunk: Timeout
  ```

- The on-demand counterpart
  (`on_demand_snapshot_redelivers_alt_screen_and_main_screen_color_queries_once`,
  `tests.rs:5366`, same release-then-arm order at `tests.rs:5406-5408`) failed 0
  times in these 51 `--lib` runs. Two earlier records show it failing with the
  same text (section 6), so this sample is simply too small to see it.

Other failures seen in the same `--lib` runs (not the target test):

| Test | Failed in | Location | Note |
|---|---|---|---|
| `mux::session::pane::tests::from_restored_pane_can_write_and_read_through_its_adopted_master` | 11 of 51 runs (5 of 21 in (b), 6 of 30 in (c)) | `src-tauri/src/mux/session/pane/tests.rs:3284:5`, assertion `bytes written through the adopted master's writer must be readable back through the adopted master` | One `read` (`pane/tests.rs:3281-3282`) on a real PTY after one write (`pane/tests.rs:3278`). Cause not analysed |
| `pty::tests::reader_routes_to_ring_buffer_when_paused` | 1 of 30 runs in (c) | `src-tauri/src/pty/mod.rs:789:9`, the `assert_eq!` on the drained ring | Waits for the reader thread with a fixed 50 ms sleep (`pty/mod.rs:783-784`) before asserting. One occurrence; cause not analysed |

Three further full `--lib` cargo runs, made after the 51 above to confirm the
final state of the suite, failed in two of three runs on a different test,
`mux::ipc::pty_spawn::tests::strip_open_string_body_closure::open_body_closure_a_dcs_or_apc_body_left_open_by_an_overflow_flush_is_closed_at_the_next_removal`
(`strip_open_string_body_closure.rs:244:5`, `took 10.275111515s` against a
wall-time budget of 10 s, `pty_spawn/tests/post_strip_cut_csi.rs:40`). In the
(c) runs the same test passed with elapsed times of 0.7-7.9 s (median 2.1 s,
n = 30), and it did not fail in any of the 51 runs; the machine was busier
during these last runs (suite
`finished in` 57-60 s, other sessions active). The second of those runs also
failed `from_restored_pane_can_write_and_read_through_its_adopted_master`; the
third run was clean. The target test did not fail in those three runs.

The suite's own `finished in` time varied between 32 s and 80 s across runs. In
3 of the 30 (c) runs `status_bar::providers::time::tests::time_provider_version_is_pure_load`
reported `<60.000s>`. This was not analysed and is not connected to the target test.

Raw per-run logs are kept in the worktree's untracked `tmp/runs/` and are not
committed.

## 6. Earlier occurrences recorded in the repository

Records in `test-docs/` already describe failures with the same text:

- `test-docs/mux-alt-tracker-ris-compound-decset/task0002.tests.yaml:5-11`: two
  full runs each failed the target test with
  `the reader must reach P2 on the main-buffer chunk: Timeout`, "while other
  worktrees loaded the machine (load average above 6)".
- `test-docs/osc133-mark-token-exact-match/task0001.tests.yaml:2-4`: the target
  test failed in a baseline run, "observed while the worktree was still
  compiling for the first time".
- `test-docs/mux-osc-abort-altscreen-split-read/task0001.tests.yaml:2-3` and
  `test-docs/mux-suppressed-output-round4-fixes/task0002.tests.yaml:2-9`: the
  on-demand counterpart failed in a baseline run with the same text. The latter
  also lists a "PTY echo read-back" failure, which matches
  `from_restored_pane_can_write_and_read_through_its_adopted_master` below.

These are statements by earlier task authors, not measurements made here; they
agree in text and in the failing wait with the runs in section 5.

## 7. Follow-up task candidates

1. Fix the re-arm order in the two tests that still have it.
   - Target: `src-tauri/src/mux/ipc/pty_spawn/tests.rs`, the target test (arm at
     `5142`, release at `5137`) and
     `on_demand_snapshot_redelivers_alt_screen_and_main_screen_color_queries_once`
     (arm at `5408`, release at `5406`).
   - Problem: the second `p2.arm()` runs after the release, so the reader can
     pass the unarmed P2 point first and the 5 s `recv_timeout` always expires.
   - Basis: sections 1-5; the sibling tests at `tests.rs:1885-1893`,
     `6363-6371`, `6696-6698` and `tests/round3_write_path.rs:663-664` already arm
     before releasing, and commit `685acb17` records the same race. Because
     `hit()` takes the armed state before blocking (`output_capture.rs:414`),
     arming before the release is valid. No production change is needed.
   - Verification suggestion: repeated full `--lib` runs; 51 runs here gave 3
     failures, so a similar sample should give none.
2. Investigate `from_restored_pane_can_write_and_read_through_its_adopted_master`.
   - Target: `src-tauri/src/mux/session/pane/tests.rs:3253-3290`.
   - Problem: failed in 11 of 51 `--lib` runs (about 22 %), a higher rate than
     the target test, at the assertion on the bytes read back from a PTY.
   - Basis: section 5. The test performs one `read` after one write; whether a
     short read, an ordering effect or something else causes it was not
     investigated here.
3. Optional hardening: the same release-then-re-arm mistake is possible for any
   future test using `PauseHook`. A helper on `PauseHook` that re-arms and
   releases in the safe order, or a note on `PauseHook::arm`
   (`output_capture.rs:396-407`), would remove the need to rediscover it.
   This is a design suggestion, not something the measurements require.

Two more tests are noted without a task candidate because the data are thin:

- `pty::tests::reader_routes_to_ring_buffer_when_paused` (one failure in 30
  runs); its wait is a fixed 50 ms sleep (`src-tauri/src/pty/mod.rs:783-784`).
- `strip_open_string_body_closure::open_body_closure_a_dcs_or_apc_body_left_open_by_an_overflow_flush_is_closed_at_the_next_removal`
  (two failures in the last three runs, none in the 51 earlier ones); a 10 s
  wall-time budget against typical elapsed times of 0.7-7.9 s leaves little
  margin when the machine is busy.

## 8. What was not done

- No CPU load was added and CPU affinity was not restricted, as the task
  excludes load runs; the effect of a constrained or loaded scheduler on the
  failure rate was therefore not measured.
- No instrumentation of tracked files, so the interleaving of T and R at the
  failure was not captured directly.
- The fix in candidate 1 was not applied or tested (investigation only).
