# Threat Model: mux-suppressed-output-fixes

## Verdict
threats-identified

## Rationale
Inspected: SPEC.md (FR1-FR13, NFR1-NFR6, as-01..as-06), REQUIREMENTS.md, and the
daemon code paths the feature changes (the PTY reader and its suppressed-chunk
pipeline, the suppressed-chunk replacement builder, the scrollback write filter,
the visibility-resume snapshot builder, the output-target state machine). Tier:
full. Design step skipped (no UI change).

The feature's only trust boundary is the PTY output of the child process running
in a mux pane. That byte stream is attacker-influenceable (any program, remote
content printed by `cat`/`ssh`, a malicious file). The daemon parses it to decide
what to re-deliver to the attached client after a snapshot, and the client acts on
what it receives: it answers terminal queries by writing responses back into the
child's input, and it opens child WebView windows for viewer launches. Everything
this feature changes on the replacement builder and the write filter sits on that
boundary, so the analysis depth is deep (domain `input-handling`).

Changes that cross no new boundary:
- FR8 changes only where a daemon-generated screen-mode switch sits inside the
  visibility-resume snapshot. The untrusted bytes that snapshot carries (the
  stripped ring and the shadow parser's cell dump) are unchanged.
- FR10 removes a code path with no production caller.
- FR11 and FR12 change and add tests. FR13 adds a document.

Post-decomposition consistency check: the tasks declaring `input-handling`
(task0001, task0002) have their production files listed under TB-1's Boundary
files. No task declares `auth`, `external-io` or `data-persistence`.

## Trust Boundaries

### TB-1: Child-process PTY output -> daemon suppressed-chunk replacement and write filter -> attached client
Crossing: untrusted PTY output bytes from the child process in a pane (another
process) are classified by the mux daemon and partly re-sent to the attached GUI
client after a snapshot. The client then answers queries into the child's input
and opens viewer windows.
Boundary files: src-tauri/src/mux/ipc/pty_spawn/client_parity_scan.rs, src-tauri/src/mux/ipc/pty_spawn/suppressed_output.rs, src-tauri/src/mux/ipc/pty_spawn/mod.rs, src-tauri/src/mux/ipc/pty_spawn/write_filter.rs
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | Crafted PTY output makes the replacement carry a query or a viewer launch at a position where the client parser would not have started one. Examples: an ESC consumed as the designator of ESC ( or ESC ), an 8-bit C1 byte, an OSC cut short by ESC, or a start state guessed from a retained window that cannot determine it. The client then answers a query nobody sent, which injects bytes into the child's input, or opens a viewer the stream never launched (NFR4, FR2, FR6, AC-3). | TM-1 | The classifier follows the term_core transitions in the SPEC FR2 table, including designator consumption, ESC ESC, ESC-aborted strings, and C1 bytes as printable data. A query or viewer launch is only extracted when it is complete and begins where those transitions start a sequence. CSI queries are classified by the existing strip predicate SSOT. Color queries follow the client's theme rules, and an OSC number the classifier cannot route counts as not-a-query. Start-state derivation from the retained window begins only at a decidable synchronization point; otherwise it falls back to a state that can miss an item but never fabricate one (as-05). | task0001 AC-7 | VERIFICATION.md Performance / Security Verification: TM-1 |
| Denial of service | Hostile PTY output tries to take the reader down or stall delivery. Vectors: large chunks with many unterminated introducers, very long strings, huge OSC numbers, or long runs of partial sequences. Effects: a super-linear replacement or boundary scan, unbounded retained state, an arithmetic panic in the reader thread, or an empty PtyOutput chunk that the client reads as PTY exit (NFR5). | TM-2 | Both the replacement scan and the write-filter boundary scan are a single bounded forward pass: over the retained window (at most N bytes) plus the chunk, and over pending plus the chunk respectively. The retained window is capped at N bytes. OSC-number arithmetic is non-panicking. An empty replacement is never sent. The pending cap (512 KiB) and its overflow flush through the strip are unchanged. | task0001 AC-7, task0002 AC-6 | VERIFICATION.md Performance / Security Verification: TM-2 |
| Information disclosure | A replacement goes to a destination other than the one that received the covering snapshot, for example after another connection took over the pane. Viewer-launch payload content, or a query the other client then answers, would reach the wrong client (NFR4). | TM-3 | The replacement is sent only to the sender captured when the chunk was judged covered, i.e. the destination that received the covering snapshot. The current output target is never re-read to choose a destination. | task0001 AC-6 | VERIFICATION.md Performance / Security Verification: TM-3 |
