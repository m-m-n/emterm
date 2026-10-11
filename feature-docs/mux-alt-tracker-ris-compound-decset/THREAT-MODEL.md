# Threat Model: mux-alt-tracker-ris-compound-decset

## Verdict
threats-identified

## Rationale
Inspected: SPEC.md and REQUIREMENTS.md (FR1-FR8, NFR1-NFR5), the reader's span
tracker (`AltSpanTracker`) in `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`,
and its only caller, `pty_reader_loop` in `src-tauri/src/mux/ipc/pty_spawn/mod.rs`.
Tier: full. Domains: task0001 declares `input-handling`, which sets TB-1 to deep;
task0002 declares no domain because it adds test rows only and changes no
production file.

The feature has one input: the byte stream that a mux pane's program writes to
its PTY. The task description says these bytes come from the pane's own program.
That program can still relay content it did not write, such as a file shown by a
pager or the output of a remote host over ssh. The reader therefore treats the
stream as untrusted input. The feature crosses no other boundary. It adds no
network access, file access, IPC message shape or privilege change. The daemon
agent-status task, the client protocol and the scrollback write path keep their
current shape.

## Trust Boundaries

### TB-1: PTY output into the mux reader's span tracker
Crossing: Bytes written by the program in a mux pane, which may relay untrusted
content, reach the mux reader thread. There, `AltSpanTracker` decides which
bytes are live main-screen content for OSC 133 mark gating, and the reader's
write path decides what reaches the scrollback ring.
Boundary files: src-tauri/src/mux/ipc/pty_spawn/write_filter.rs, src-tauri/src/mux/ipc/pty_spawn/mod.rs
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Denial of service | A crafted DEC private CSI can overflow the new parameter value or grow the carried state (NFR1). Examples: a parameter with an unbounded digit run, a very large number of parameters, a CSI that never terminates, or any byte value after any prefix of the sequence. The result is a panic in the reader thread, which stops the pane from updating, or unbounded retained memory. | TM-1 | The parameter value saturates at a cap that never reads as 47, 1047 or 1049. The carried state has a fixed size: a partial switch of at most 7 bytes, the designator wait, and three CSI-parse fields. Every index stays inside the read, and no input panics. | task0001 AC-6 | VERIFICATION.md TS-3; Performance / Security item TM-1 |
| Tampering | A crafted sequence can desynchronize the tracker from term_core's screen (FR1-FR6). The sequence is a RIS, an `ESC c` in a designator slot, or a compound DECSET, sent while the client-rule screen and the shadow-rule screen disagree. An OSC 133 mark inside alternate-screen content then counts as a live prompt mark, or a shell's prompt mark on the main screen is dropped. The pane's OSC 7501 table and OSC 777 state then differ from those of a plain tab. | TM-2 | The tracker follows term_core's screen for RIS (FR1, FR2). An `ESC c` in a designator slot moves the shadow-rule screen and leaves the client-rule screen in place (FR3). In a mismatched state, an alternate-screen compound gets the conservative fallback: the compound's final byte sets the client-rule screen, and the shadow-rule screen resynchronizes to the parser (FR4, FR5). | task0001 AC-1, AC-2, AC-3, AC-4, AC-5, AC-7; task0002 AC-1 | VERIFICATION.md TS-1, TS-2, TS-4, TS-5, TS-6; Performance / Security item TM-2 |
| Tampering | The change could reach the scrollback write path, that is, the extraction, `to_write`, `cuts` or the fallback write (FR8). Alternate-screen bytes would then enter the scrollback ring, or main-screen bytes would be lost. | TM-3 | Every change stays inside `AltSpanTracker`. The main-buffer extraction, the switch-pattern list, `ScrollbackWriteFilter` and `AgentStatusFeedScanner` stay untouched. The existing write-path tests, including the recorded ring pin, pass unchanged. | task0001 AC-8 | VERIFICATION.md TS-7; Performance / Security item TM-3 |
