# Threat Model: mux-alt-tracker-detection-read-boundary

## Verdict
threats-identified

## Rationale
Inspected: SPEC.md and REQUIREMENTS.md (FR1-FR8, NFR1-NFR5), the reader's span
tracker (`AltSpanTracker`, its scan and its per-read entry point) in
`src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`, and its only caller,
`pty_reader_loop` in `src-tauri/src/mux/ipc/pty_spawn/mod.rs`. Tier: full.
Domains: task0001 declares `input-handling`, which sets TB-1 to deep; task0002
declares no domain because it adds plain-tab test rows only and changes no
production file.

The feature has one input: the byte stream that a mux pane's program writes to
its PTY. The task description says these bytes come from the pane's own program
and cross no trust boundary. That program can still relay content it did not
write, such as a file shown by a pager or the output of a remote host over ssh,
so the reader treats the stream as untrusted input, as the predecessor
feature's threat model does. The feature crosses no other boundary: it adds no
network access, file access, IPC message shape or privilege change. The edit to
the predecessor's test-docs record is a repository document change and crosses
no boundary.

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
| Tampering | Crafted output in a mismatched state desynchronizes the tracker from term_core's screen (FR1, FR2, FR4). The output is two compound DEC private sets or resets in one read, or a compound followed by `ESC (`, a lone `ESC` or the start of a CSI where the read ends. An OSC 133 mark inside alternate-screen content then counts as a live prompt mark and removes the pane's `working` record, or a prompt mark on the main screen is dropped. The pane's OSC 7501 table and OSC 777 state then differ from those of a plain tab. | TM-1 | After the first detection in a read, every later compound in that read is detected and sets the client-rule screen by its terminator (FR1). The carried item that the scan records after the last detected compound is kept for the next read (FR2). The rest of the detection read's handling stays as it is (FR3). | task0001 AC-1, AC-2, AC-3, AC-5; task0002 AC-1 | VERIFICATION.md TS-1, TS-2, TS-3, TS-5, TS-6; Performance / Security item TM-1 |
| Denial of service | Keeping the carried item after a detection read (FR2) lets crafted output reach the resume of a partial switch, a designator wait or a CSI parse right after a detection read, a path no input reached before. An unchecked index or an arithmetic overflow there panics the reader thread, which stops the pane from updating (NFR1). | TM-2 | The carried state keeps its kinds and its fixed size: one carried item at a time, a partial switch of at most 7 bytes, the designator wait, or the CSI-parse fields. Every index stays inside the read, and no input panics. | task0001 AC-6 | VERIFICATION.md TS-7; Performance / Security item TM-2 |
| Tampering | The change could reach the scrollback write path, that is, the extraction, `to_write`, `cuts` or the fallback write, or the feed scanner (FR8). Alternate-screen bytes would then enter the scrollback ring, or main-screen bytes would be lost. | TM-3 | Every change stays inside the tracker's scan and per-read entry point. The main-buffer extraction, the switch-pattern list, `ScrollbackWriteFilter`, `AgentStatusFeedScanner` and `pty_reader_loop` stay untouched, and the existing write-path tests pass unchanged. | task0001 AC-7 | VERIFICATION.md TS-8, TS-9; Performance / Security item TM-3 |
