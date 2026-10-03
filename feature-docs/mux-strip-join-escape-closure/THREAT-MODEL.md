# Threat Model: mux-strip-join-escape-closure

## Verdict
threats-identified

## Rationale

**What was inspected.** SPEC.md and REQUIREMENTS.md (FR1-FR5, NFR1-NFR3), plus the three attack scenarios in the task description. The tier is full. The only task, task0001, declares `input-handling`, which makes TB-1's depth deep.

**Why the verdict holds.** The feature changes no production code (NFR2). It adds regression tests and a decision record that pin an existing trust boundary: program output, which an attacker can shape (for example a crafted file shown with `cat`), crosses into the mux scrollback and comes back out as replayed bytes. Two threats realistically apply at that boundary. In production both are mitigated by the predecessor closure, mux-strip-concat-query-closure D1, which this feature leaves unchanged. Each mitigation row below is carried by task0001 as the regression test that pins it.

**Residual outside this feature's mitigation scope (FR5).** A construct removed inside an unterminated kept OSC/DCS/APC body writes no closing, so the bytes after the construct join the body. An OSC 11 colour query can then be completed, either on replay or by a later live continuation. task0001 AC-5 records this residual in DECISIONS.md. It has no TM row, because this feature introduces no mitigation for it.

**Domain consistency re-check.** task0001 declares `input-handling`. Its files (the new and sibling test modules, the test module registration, and DECISIONS.md) pin the boundary but do not implement it. The Boundary files line therefore lists the production files that implement TB-1, which this feature does not modify.

## Trust Boundaries

### TB-1: Pane program output -> mux scrollback strip -> replaying term_core

**Crossing.** Bytes written by a program in a mux pane cross into the daemon's write strip and scrollback ring. The attacker can influence those bytes. On reattach or a window switch, the bytes then reach the snapshot strip and the client's replaying term_core. term_core's responses are written to the PTY input of whatever foreground program is running at that moment.

**Boundary files:** src-tauri/src/mux/ipc/pty_spawn/write_filter.rs, src-tauri/src/mux/scrollback_filter.rs, src-tauri/src/mux/snapshot_bytes.rs

**Depth:** deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | A removed construct between a written lone ESC or open CSI and the bytes after it joins them into a terminal query. This happens in the ring from one cut-free feed (scenario 1, FR3), or via the snapshot re-strip after the write strip (scenario 2, FR1). On replay, term_core then answers into the foreground program's PTY input. | TM-1 | The predecessor closure (mux-strip-concat-query-closure D1) writes one DEL before such a removal. This feature pins that closure with two regression tests: one on the one-call ring, and one on the write strip -> snapshot strip -> term_core chain giving zero responses. | task0001 AC-1, AC-3 (control: AC-7) | VERIFICATION.md TS-1, TS-3 and the TM-1 security item |
| Tampering | ESC + removed construct + `c` or `(0` joins into a RIS or a G0 designation on replay. This resets the replayed terminal state or switches its character set (scenario 3, FR2). | TM-2 | The same predecessor closure applies: term_core ignores ESC DEL as an unknown escape final. This feature pins it with a replay test over every removed construct kind and both continuations, compared with the raw-stream reference on rows, cursor and responses. | task0001 AC-2 (control: AC-7) | VERIFICATION.md TS-2 and the TM-2 security item |
