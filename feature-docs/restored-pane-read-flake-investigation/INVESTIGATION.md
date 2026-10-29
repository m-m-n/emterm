# Investigation: intermittent failure of the restored-pane write/read-back test

Target test:
`mux::session::pane::tests::from_restored_pane_can_write_and_read_through_its_adopted_master`
(`src-tauri/src/mux/session/pane/tests.rs:3253-3291`).

Investigated on 2026-10-29 at the tip of the integration branch (`a4d42c23`). No tracked file
under `src-tauri/` or `crates/` was changed or instrumented (`git diff -- src-tauri crates` is
empty before and after). All instrumentation lives outside the repository (section 5.2).

## 1. Conclusion

A short read of the PTY echo, triggered by CPU contention. Test order is not a cause.

The test writes `restored-write\n` once (`pane/tests.rs:3278`), then makes exactly one blocking
`read` of up to 64 bytes (`pane/tests.rs:3281-3283`) and asserts that the returned bytes contain
the 14-byte marker `restored-write` (`pane/tests.rs:3284-3290`). What the read sees is the
echo that the slave side's line discipline produces: `restored-write\r\n`, 16 bytes. The echo is
not handed to the master's read side as one unit. The first `read` can return only a prefix of it,
and the rest arrives shortly after (0.025-0.591 ms later in the logged runs, typically 0.04-0.06 ms). A first read shorter than 14 bytes cannot
contain the marker, so the assertion at `pane/tests.rs:3284:5` fails. The test has no loop, no
deadline and no retry, so nothing absorbs a prefix-only read.

Supporting facts (details in sections 3-5):

- **Observed in the target test itself.** With the test source unchanged and a read/write logger
  attached from outside the repository (5.2), the whole `--lib` suite was run 30 times: the target
  test failed 6 times, and in all 6 the first read returned 4 or 5 bytes (`rest` x4, `resto` x2).
  The remaining 12 or 11 bytes of `restored-write\r\n` were read 0.039-0.059 ms later. In the 24
  passing runs the first read returned the full 16 bytes. The test alone under injected CPU load
  (24 busy threads, no other test running) failed 19 of 150 runs with the logger attached, and every
  failing first read was a strict prefix (`re` x16, `res` x1, `restore` x2).
- **CPU contention is sufficient; other tests and test order are not needed.** The test run alone
  without extra load passed 120 of 120 times (20 through cargo, 100 through the test binary). The
  same unmodified test run alone while 24 busy-loop threads occupied the machine's 24 logical CPUs
  failed 28 of 150 runs (18.7 %), every one at `pane/tests.rs:3284:5` with the same message. The
  unmodified `--lib` suite failed it 5 of 37 times (13.5 %).
- **Same place in the order in passing and failing runs.** The target test finishes at result
  position 2273-2283 of about 4820 in all 37 unmodified full runs, failing runs (2276, 2278, 2283,
  2282, 2276) and passing runs alike (section 4.3).
- **The window is microseconds wide.** In the logged runs the read was issued 0.001-0.007 ms after
  the write returned, in failing and passing runs alike. A logger that spent about 0.035 ms between
  the write and the read (an extra `write` syscall for its own log line) saw 0 failures in 25 runs;
  in a separate reproduction program, a deliberate delay between write and read lowered the failure
  rate from 16-22 % (no delay) to 0.9-3.0 % (30 us) and 0.04-0.07 % (1000 us) (section 5.5).
- **The assertion message cannot tell this from other causes.** It does not print the bytes read
  (`pane/tests.rs:3285-3289`), and every failure in the unmodified runs shows only that text. The
  earlier investigation recorded the failure with "Cause not analysed" for the same reason
  (section 6).

Limit of the evidence: the kernel side was observed only through what `read` returns; no kernel
tracing was done. The statement that the echo is produced and delivered incrementally rests on the
prefixes seen (every length from 1 to 13 bytes appeared in the reproduction program, section 5.5)
and on the remainder always arriving afterwards. Why contention makes the prefix-only read more
likely was not traced. The logger itself changes timing (the first version, which delayed the read by about 0.035 ms, saw no failure in 25 runs),
so the failure rates in section 5 come from the unmodified runs and the logger runs are used only
for the bytes and timings.

## 2. Path from the write to the read

`portable-pty 0.8.1` and `filedescriptor 0.8.3` are the versions in `Cargo.lock`; their paths are
relative to each crate's root.

| # | Step | Location | What happens |
|---|---|---|---|
| P1 | Open the PTY pair | `pane/tests.rs:3255` -> helper `pane/tests.rs:2909-2918` -> `portable-pty src/unix.rs:18-61` | `libc::openpty` with a NULL termios (`unix.rs:29-39`), so the slave gets the kernel's default settings. Both fds are close-on-exec (`unix.rs:57-58`). The master fd is not non-blocking (checked, section 3). |
| P2 | Take the writer | `pane/tests.rs:3256` -> `portable-pty src/unix.rs:319-326` | A `dup` of the master fd (`filedescriptor src/unix.rs:144-168`) wrapped in `UnixMasterWriter` (`portable-pty src/unix.rs:347-372`). Its `drop` writes `\n` plus VEOF to the master (`unix.rs:351-363`), which happens after the assertion. |
| P3 | Clone a reader | `pane/tests.rs:3257-3260` -> `portable-pty src/unix.rs:314-317` | Another `dup` of the master fd, created before the write and before the pane exists. Its `read` maps `EIO` to `Ok(0)` (`unix.rs:86-99`). |
| P4 | Hand the master to a pane | `pane/tests.rs:3262-3276` -> `src-tauri/src/mux/session/pane/mod.rs:931-1003` -> `mod.rs:308-415` | `from_restored` builds the pane around the writer and master (`mod.rs:408-409`), replays an empty scrollback and spawns nothing: `pane/mod.rs` has no `thread::spawn`, and nothing in the pane reads the master. The test's cloned reader is the only reader. `pair.slave` stays in `pair` until the end of the test (only `pair.master` is moved), so the slave is open during the read. |
| P5 | Write | `pane/tests.rs:3278` -> `pane/mod.rs:457-463` -> `src-tauri/src/mux/session/pane/handles.rs:314-323` | Lock the writer mutex (uncontended: only the pane holds the `Arc`), `write_all` the 15 bytes, `flush`. `FileDescriptor::write` is one `write(2)` (`filedescriptor src/unix.rs:219-227`) and `flush` does nothing (`unix.rs:228-230`). The logger saw `ret=15` every time. |
| P6 | Echo (kernel) | not in the repository | The slave's line discipline echoes the input to the master's read side. The pair keeps the default settings: ICANON, ECHO and ONLCR on (read back with `tcgetattr` in the reproduction program), so `\n` is echoed as `\r\n` and the full echo is `restored-write\r\n`, 16 bytes. |
| P7 | Read | `pane/tests.rs:3280-3283` -> `portable-pty src/unix.rs:86-99` -> `filedescriptor src/unix.rs:208-217` | One `read(2)` into a 64-byte buffer on a blocking fd. No timeout, no loop, no retry. It returns as soon as at least one byte is available. |
| P8 | Assert | `pane/tests.rs:3284-3290` | `buf[..n].windows(14).any(== b"restored-write")`. A read of fewer than 14 bytes (including 0) fails. The message does not include `buf[..n]`. |

## 3. What the read can return

| Result of the read | Outcome | Seen |
|---|---|---|
| 16 bytes `restored-write\r\n` | pass | 24 of 30 logged `--lib` runs, 131 of 150 logged loaded runs; 19 920 of 20 000 first reads in the idle single-worker run of the reproduction program |
| 14 bytes `restored-write` (the `\r\n` not yet there) | pass | reproduction program only (23 of 20 000 first reads on the idle single-worker run) |
| 1-13 bytes, a prefix of `restored-write\r\n` | **fail** at `pane/tests.rs:3284:5` | target test: `rest`, `resto`, `re`, `res`, `restore` (section 5.3); reproduction program: every length 1-13 |
| 15 bytes | would pass | never seen |
| 0 bytes (`EIO` mapped to `Ok(0)`, slave closed) | would fail | never seen |
| `Err` | panics at `pane/tests.rs:3283` (`master read must succeed`), a different line | never seen |

In every logged failure the missing part of the echo was read next, within 0.6 ms at most, so no
byte was lost: the echo was late, not absent. The follow-up read was itself a prefix in 3 of the
19 loaded failures (`s`, `st`, `stored-`), so the echo can arrive in more than two pieces.

The master fd is blocking (`O_NONBLOCK` false, checked with `fcntl` in the reproduction program;
the writer and the reader are `dup`s of it and share that flag). The slave's settings, read back
through the master fd with `tcgetattr`, have ICANON, ECHO, ECHOE, ECHOCTL, ISIG, OPOST, ONLCR and
ICRNL on.

## 4. Waits and ordering

### 4.1 Wait points

The test has one wait, the read itself.

| # | Location | Waits for | Timeout | Style |
|---|---|---|---|---|
| W1 | `pane/tests.rs:3281-3283` | At least one echoed byte on the master | none | Blocking `read(2)`, the thread sleeps in the kernel |

There is no `recv_timeout`, sleep, join or channel wait in the test, and the writer mutex
(`handles.rs:318-320`) is never contended. `make_output_target` (`pane/tests.rs:5-8`) creates a
channel whose receiver is dropped at once; nothing in the test waits on it. What the test assumes,
and does not enforce, is that "at least one byte" means "the whole echo". The comment in
`src-tauri/src/mux/upgrade/tests.rs:295-299` calls this test's read side deterministic; it is not.

### 4.2 Parallel run

`cargo test` runs libtest with one test thread per logical CPU (24 here), so the target test shares
the machine with up to 23 other running tests, many of which spawn processes and PTYs. The target
test uses only its own PTY pair and local state; no process-global state was found in the code it
reaches (`pane/mod.rs:308-415`, `931-1003`). The only coupling to the other tests is competition
for CPU time.

### 4.3 Order

libtest starts tests in name order by default (no shuffle was used). Position of the target test
in the result listing of each unmodified full run (result lines are in completion order):

| Runs | Target test position range | Total result lines |
|---|---|---|
| 32 passing runs | 2273-2282 | 4814-4822 |
| 5 failing runs | 2276, 2276, 2278, 2282, 2283 | 4816-4819 |

The failing and passing runs are indistinguishable by position, and the same test run alone under
load fails (5.4), so order is not needed to produce the failure.

## 5. Measurements

### 5.1 Environment and commands

Linux, 24 logical CPUs, rustc 1.96.0 / cargo 1.96.0, test profile (unoptimized + debuginfo), 4831
tests in the `--lib` suite (4828 run, 3 ignored). Other sessions' processes were running on the
machine (1-minute load average 2-11 at the start of the unmodified `--lib` runs). Runs were
sequential; no two measurements overlapped. Cargo commands were run from the project root.
`<test>` below is `mux::session::pane::tests::from_restored_pane_can_write_and_read_through_its_adopted_master`.
`<bin>` is the test binary cargo produced, `src-tauri/target/debug/deps/emterm-48705c2bd8f909e6`,
run from `src-tauri/` (as cargo does).

- (a1) `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib <test> -- --exact`, 20 runs.
- (a2) `<bin> <test> --exact`, 100 runs.
- (b1) `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` (the project test command), 12 runs. Run 1 is the baseline run made before any file was created.
- (b2) `<bin>` with no arguments (whole `--lib` suite), 25 runs.
- (c1) `LD_PRELOAD=<logger v1> <bin>`, whole suite, 25 runs.
- (c2) `LD_PRELOAD=<logger v2> <bin>`, whole suite, 30 runs.
- (d1) `<bin> <test> --exact` while 24 busy-loop threads ran (a separate process started by the runner), 150 runs.
- (d2) `LD_PRELOAD=<logger v2> <bin> <test> --exact` under the same 24 busy threads, 150 runs.
- (e) The reproduction program (5.5).

### 5.2 Instrumentation kept outside the repository

- **Logger** (a small shared library loaded with `LD_PRELOAD`, source not committed). It wraps `write`
  and `read`. When the process writes exactly `restored-write\n` it remembers the thread and the
  time. The next 64-byte `read` on that thread is timed and logged with the returned bytes. If the
  read returned fewer than 16 bytes it then polls the same fd for up to 300 ms and logs the next
  chunk (this extra read happens after the test's own read returned and does not change the
  buffer the test sees). The test binary is the unmodified one.
  - v1 logged the write line immediately after the write, i.e. between the test's write and read.
    That put 0.026-0.074 ms (median 0.035 ms) between them.
  - v2 logs everything after the test's read returned. The write-to-read gap is 0.001-0.007 ms,
    matching what the unlogged test does.
- **Busy threads.** 24 threads spinning on integer arithmetic for 200 s, started by the runner
  script before the loop and finishing by themselves. This is added load and is used only in
  (d1), (d2) and the reproduction program.
- **Reproduction program.** A standalone Rust program linked against the same `portable-pty 0.8.1`.
  It repeats the target test's shape: open a 24x80 pair, `take_writer`, `try_clone_reader`,
  `write_all(b"restored-write\n")` + `flush`, then one `read` into 64 bytes, keeping the slave open.
  It records the first read's length and bytes and, on a failing read, polls the master for up to
  300 ms for the rest of the echo. Options: worker threads (each with its own PTY pair), busy
  threads, a busy-wait of N microseconds between write and read, and a `loop` mode that keeps
  reading until the marker is found.

Observations from the logger and the reproduction program are reported separately from the
unmodified runs below.

### 5.3 Results for the target test

| Condition | Runs | Passed | Failed | Failure rate (95 % Wilson) | Bytes read at the failure |
|---|---|---|---|---|---|
| (a1) alone, cargo | 20 | 20 | 0 | - | - |
| (a2) alone, test binary | 100 | 100 | 0 | - | - |
| **(a) alone, total** | 120 | 120 | 0 | 0-3.1 % | - |
| (b1) `--lib`, cargo | 12 | 10 | 2 (runs 1, 3) | 4.7-44.8 % | not printed by the assertion |
| (b2) `--lib`, test binary | 25 | 22 | 3 (runs 3, 6, 10) | 4.2-30.0 % | not printed by the assertion |
| **(b) `--lib`, total** | 37 | 32 | 5 | 13.5 % (5.9-28.0 %) | not printed by the assertion |
| (c1) `--lib` + logger v1 | 25 | 25 | 0 | 0-13.3 % | no failure; all 25 reads were the full 16 bytes |
| (c2) `--lib` + logger v2 | 30 | 24 | 6 (runs 12, 14, 16, 18, 21, 30) | 20.0 % (9.5-37.3 %) | `rest` x4, `resto` x2 (below) |
| (d1) alone + 24 busy threads | 150 | 122 | 28 | 18.7 % (13.2-25.7 %) | not printed by the assertion |
| (d2) alone + 24 busy threads + logger v2 | 150 | 131 | 19 | 12.7 % (8.3-18.9 %) | `re` x16, `res` x1, `restore` x2 |

All failures in (a)-(d) are at `src-tauri/src/mux/session/pane/tests.rs:3284:5` with the message
`bytes written through the adopted master's writer must be readable back through the adopted
master (echoed by the PTY line discipline)`; the unmodified logs contain no byte values.
(b1) run 1 is the baseline. One more full cargo run, made after this document was written (for the
test record), also failed the target test at the same line; it is not counted in the table.

Failing reads in (c2), with what the logger read next on the same fd:

| Run | First read (bytes) | Next chunk | Gap to next read | Write-to-read gap | Read blocked |
|---|---|---|---|---|---|
| 12 | `rest` (4) | `ored-write\r\n` (12) | 0.059 ms | 0.001 ms | 0.010 ms |
| 14 | `rest` (4) | `ored-write\r\n` (12) | 0.048 ms | 0.002 ms | 0.003 ms |
| 16 | `resto` (5) | `red-write\r\n` (11) | 0.041 ms | 0.002 ms | 0.006 ms |
| 18 | `rest` (4) | `ored-write\r\n` (12) | 0.039 ms | 0.002 ms | 0.003 ms |
| 21 | `resto` (5) | `red-write\r\n` (11) | 0.040 ms | 0.001 ms | 0.003 ms |
| 30 | `rest` (4) | `ored-write\r\n` (12) | 0.054 ms | 0.002 ms | 0.004 ms |

The 24 passing reads in (c2) took 0.003-0.023 ms (median 0.004 ms) to return and were issued
0.001-0.004 ms after the write, the same range as the failing ones. In (d2) the 19 failing reads
were issued 0.002-0.006 ms after the write and blocked 0.002-0.014 ms; the 131 passing reads
0.002-0.007 ms and 0.002-2.637 ms. The follow-up chunk in (d2) was the rest of the echo in 16 of
19 cases (`stored-write\r\n` x13, `tored-write\r\n` x1, `d-write\r\n` x2) and another prefix in 3
(`s`, `st`, `stored-`). The gap to the next read in (c2) and (d2) was 0.025-0.591 ms.

Other tests that failed in these `--lib` runs (not the target test, not analysed beyond the text):

| Test | Failed in | Location | Note |
|---|---|---|---|
| `mux::inherited_pty::tests::dropping_the_adapter_closes_the_owned_descriptor` | 1 of 25 in (b2), 1 of 25 in (c1) | `src-tauri/src/mux/inherited_pty.rs:474:27` | `assertion left != right failed`, both sides 1282, which is the device number of `/dev/ptmx` here (`stat` prints 5:2, i.e. 5*256+2); the test compares `st_rdev` of a reused fd number (`inherited_pty.rs:266-273`) |
| `mux::ipc::pty_spawn::tests::visible_reattach_redelivers_alt_screen_and_main_screen_color_queries_once` | 1 of 25 in (c1) | `src-tauri/src/mux/ipc/pty_spawn/tests.rs:5145:10` | `the reader must reach P2 on the main-buffer chunk: Timeout`; already recorded as follow-up 1 of the earlier investigation |

### 5.4 What (a), (b) and (d) separate

- (a) 0 of 120 and (d1) 28 of 150 are the same unmodified test and binary; the difference is the
  24 busy threads. No other test ran in either, so test order and the other tests' presence are not
  needed for the failure.
- (b) 5 of 37 shows the parallel `--lib` run supplies enough contention to fail at a similar order
  of rate (13.5 % against 18.7 % under the busy threads; the intervals overlap).
- With 0 of 120 the idle rate is below 3.1 % (95 % upper bound), consistent with the reproduction
  program's idle rate of 0.14-0.36 % (5.5), at which 0 failures in 120 runs has a probability of
  about 65-85 %. A 0 in (a) therefore does not show that the failure cannot happen when idle.

### 5.5 Reproduction program (observation outside the target test)

Release build unless noted. "Workers" are threads each running the target test's shape on its own
PTY pair; "busy" are spinning threads. 20 000 iterations per row (19 992 when divided by 24).

| Workers | Busy | Delay write -> read | Failing first reads | Notes |
|---|---|---|---|---|
| 1 | 0 | 0 | 57 / 20 000 (0.28 %) | debug build, 3 runs: 0.14 %, 0.34 %, 0.36 % |
| 8 | 0 | 0 | 1210 / 20 000 (6.05 %) | |
| 24 | 0 | 0 | 599 / 19 992 (3.00 %) | debug build, 3 runs: 3.15 %, 3.25 %, 4.02 % |
| 1 | 24 | 0 | 3347 / 20 000 (16.7 %) | debug build, 3 runs: 9.6 %, 13.8 %, 22.4 % |
| 24 | 24 | 0 | 595 / 19 992 (2.98 %) | debug build, 3 runs: 3.4 %, 3.6 %, 3.9 % |
| 1 | 24 | 0 | 16.3 %, 21.9 %, 20.5 % | 3 further runs, the baseline of the delay series |
| 1 | 24 | 10 us | 8.7 %, 13.1 %, 12.4 % | 3 runs |
| 1 | 24 | 30 us | 1.2 %, 3.0 %, 0.9 % | 3 runs |
| 1 | 24 | 100 us | 1.5 %, 1.4 %, 1.3 % | 3 runs |
| 1 | 24 | 1000 us | 0.05 %, 0.07 %, 0.04 % | 3 runs |

In the release-build runs, whose failing bytes were printed, every failing first read was a strict
prefix of `restored-write\r\n`: lengths 1 to 13 all occurred (for example the 1210 failures at 8
workers split over 13 lengths, 68 to 114 each). In the two runs whose follow-up result was printed
(idle single worker, 8 workers) the 300 ms follow-up found the complete `restored-write\r\n` in
every failing case (57 of 57 and 1210 of 1210). A first read of 14 bytes (marker complete, `\r\n`
missing) occurred and passes; 15 bytes never occurred.

The same program in `loop` mode (keep reading until the marker is found, 5 s deadline) at 24
workers and 24 busy threads: 0 failures in 19 992 iterations, of which 631 first reads were
shorter than the 14-byte marker.

## 6. Existing result quoted from the earlier investigation

`feature-docs/pty-reattach-timeout-investigation/INVESTIGATION.md` (section 5, table of other
failures, line 177) recorded this test failing in 11 of 51 full `--lib` runs (5 of 21 through
cargo, 6 of 30 through the test binary), at `pane/tests.rs:3284:5`, with the note "One `read`
(`pane/tests.rs:3281-3282`) on a real PTY after one write (`pane/tests.rs:3278`). Cause not
analysed". Those numbers were measured by that investigation and are not part of the counts in
section 5. They are the same order as this investigation's 5 of 37 (the 95 % Wilson intervals,
12.5-34.6 % and 5.9-28.0 %, overlap) and the failure site is the same. Earlier `test-docs/` records
mention the same read-back failure without bytes, for example
`test-docs/mux-suppressed-output-round4-fixes/task0002.tests.yaml:2-9` ("a PTY echo read-back").

## 7. Other causes considered

| Candidate | Result | Basis |
|---|---|---|
| Test order or state left by an earlier test | Not supported | Same position in passing and failing runs (4.3); failure reproduced with no other test running (5.3 d1); the test reaches no process-global state (4.2) |
| Parallel execution as such | Only through CPU contention | Alone under busy threads fails at a similar rate as inside `--lib` (5.3); the reproduction program fails with busy threads and 1 worker (16.7 %) |
| Echo lost or absent | Excluded | The remaining bytes were read every time they were looked for (5.3, 5.5) |
| EOF / slave closed (`Ok(0)`) | Not seen | No zero-length first read in any logged run, nor in the length histograms printed for about 120 000 reproduction iterations; `pair.slave` is alive during the read (P4) |
| Another reader consuming the echo | Excluded for this test | `pane/mod.rs` spawns no thread and reads nothing from the master (P4); the logged follow-up read, made by the same thread, got the missing bytes |
| fd number reused by another test | Not seen for this test | The read returned `Ok` with PTY bytes in every logged run; the one test that does fail on fd reuse is a different one (5.3) |

## 8. Follow-up task candidates

1. Make the target test tolerate a prefix-only read and report what it read.
   - Target: `src-tauri/src/mux/session/pane/tests.rs:3253-3291` (and the comment at
     `src-tauri/src/mux/upgrade/tests.rs:295-299`, which calls the read deterministic).
   - Problem: one `read` after one write, asserted as if the whole echo arrived together; the
     assertion message omits the bytes.
   - Basis: sections 1, 3, 5. Shape of a fix that was measured in the reproduction program: keep
     reading until the accumulated bytes contain the marker or a deadline passes; this gave 0
     failures in 19 992 iterations where 631 first reads were short (5.5). The blocking reader has
     no timeout, so the deadline needs `poll` on the master fd or a helper thread. The repository
     already has the accumulate-until-deadline shape and a message that prints the collected text in
     `src-tauri/src/pty/mod.rs:680-703`.
   - Verification suggestion: the (d1) condition (alone under 24 busy threads, 150 runs; 28 failures
     before) and repeated full `--lib` runs.
2. Check the other tests that make one PTY `read` and compare the exact bytes.
   - Target: `src-tauri/src/mux/inherited_pty.rs:322-340`, `:345-368` and `:373-401` (assertions at
     `:339`, `:367` and `:400`), which write in raw mode and make one `read` on the other end of
     the PTY followed by `assert_eq!` on the whole payload.
   - Problem: the same single-read-then-compare shape. Whether a raw-mode write to one end can come
     back in pieces on the other end was not measured here; this is a candidate to check, not a
     finding.
   - Basis: section 5.5 shows the master side can return a prefix of what the line discipline
     produced; the raw-mode paths, which involve no echo, were not tested.
3. Investigate `mux::inherited_pty::tests::dropping_the_adapter_closes_the_owned_descriptor`.
   - Target: `src-tauri/src/mux/inherited_pty.rs:456-479`.
   - Problem: failed in 1 of 25 runs in each of (b2) and (c1) at `inherited_pty.rs:474:27` with
     both sides 1282.
   - Basis: section 5.3. 1282 is the device number of `/dev/ptmx` here, and the test compares the
     device number of a reused fd number; a reused number that refers to another test's PTY master
     compares equal. The comment at `inherited_pty.rs:466-471` assumes the reused descriptor "can
     only ever refer to a DIFFERENT device"; the measured values contradict that. Cause not
     confirmed.
