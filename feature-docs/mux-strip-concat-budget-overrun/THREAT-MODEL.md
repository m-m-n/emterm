# Threat Model: mux-strip-concat-budget-overrun

## Verdict
threats-identified

## Rationale
Inspected: SPEC.md, REQUIREMENTS.md, the target test and its shared definitions
(`src-tauri/src/mux/ipc/pty_spawn/tests/strip_concat_query.rs`), the thread
CPU-time meter (`src-tauri/src/mux/ipc/pty_spawn/tests/thread_cpu_time.rs`), the
shared strip (`src-tauri/src/mux/scrollback_filter.rs`) and the write filter
(`src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`). Tier: full. No DESIGN.md.

Most of the feature is test code: the target test's input sizes, its FR1 cost
record, the scaling-ratio judgment and the scaling-ratio tests. That code reads no
external data and crosses no trust boundary.

One trust boundary is in play: PTY output of child processes enters the
scrollback ring through the write filter and the shared strip, and is replayed
from the ring to an attaching client. FR2's non-linear branch may change the
internals of that code (task0001 declares `input-handling`, which sets the depth
to deep), and FR3 adds the regression guard on its cost. In FR2's linear branch
the boundary files are not changed. task0002 declares none of the domains
`auth`, `input-handling`, `external-io` or `data-persistence`; its files are
test-only and appear in no `Boundary files` line.

## Trust Boundaries

### TB-1: Child-process PTY output into the scrollback ring and its replay
Crossing: bytes written by processes running in a pane (attacker-influenceable,
for example a file printed to the terminal or remote output over SSH) pass
through the daemon's write filter and shared strip into the scrollback ring, and
from the ring into the snapshot replayed to an attaching client.
Boundary files: src-tauri/src/mux/scrollback_filter.rs, src-tauri/src/mux/ipc/pty_spawn/write_filter.rs
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | A performance change to the strip pass or the write filter in FR2's non-linear branch changes which bytes are removed or written, so a viewer launch, an agent-status report or an answered device query that the strip removes today reaches the ring and is replayed to a client (FR2, NFR3) | TM-1 | The production output stays byte-identical for every input: every existing strip and write-filter test passes unchanged, the target test keeps every output assertion, and in the linear branch the boundary files are not changed | task0001 AC-6 | VERIFICATION.md, Performance / Security Verification item TM-1 (TS-7) |
| Denial of service | A super-linear path in the strip or the write filter lets crafted PTY output make the daemon spend CPU time that grows faster than the output length (FR1, FR2, FR3) | TM-2 | The scaling-ratio guard runs the real write-path strip and the real one-call write-filter feed at N and 4N and fails when the CPU-time ratio reaches 8; a non-linear path that FR1 finds is fixed in FR2's non-linear branch and re-measured below 8 | task0002 AC-3, AC-4; task0001 AC-6 | VERIFICATION.md, Performance / Security Verification item TM-2 (TS-3) |
