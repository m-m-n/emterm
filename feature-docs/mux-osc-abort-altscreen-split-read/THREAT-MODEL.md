# Threat Model: mux-osc-abort-altscreen-split-read

## Verdict
threats-identified

## Rationale
Inspected: SPEC.md, REQUIREMENTS.md and the code paths the feature touches —
the mux reader loop's per-read capture step
(`src-tauri/src/mux/ipc/pty_spawn/mod.rs`, `pty_reader_loop`), the main-buffer
extraction and the feed scanner (`src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`),
the shadow parser the reader cross-checks against (vt100 0.16), the
suppressed-chunk pipeline that consumes the ring-written ranges, and, as the
unchanged reference, term_core's alternate-screen and charset-designator
handling. Tier: full. task0001 declares `input-handling`, so its boundary is
analysed at deep depth.

The only input the feature handles is PTY output written by programs running
in a mux pane, local or over SSH, which is untrusted. It crosses one boundary
the feature changes: the per-pane reader thread, which now carries the partial
state of an alternate-screen switch sequence from one read to the next and
derives from it the live spans that decide whether an OSC 133 mark may change
the daemon's OSC 7501 table, OSC 777 state and inferred-clear latch. The same
reader step also derives what reaches the scrollback ring and the
suppressed-chunk replacement; FR5 keeps that path unchanged, but the feature
places a second span set next to it.

The plain-tab path (term_core, the callbacks, the tab) is the unchanged
reference (SPEC A2, NFR5) and gains no boundary. task0002 declares
`input-handling` because its tests drive PTY-shaped bytes through that path,
but its only file, `src-tauri/src/tabs/tests/output_pipeline.rs`, is test code
and appears in no boundary line. The test files of task0001
(`src-tauri/src/mux/ipc/pty_spawn/tests.rs`,
`src-tauri/src/mux/ipc/pty_spawn/tests/altscreen_split_read.rs`,
`src-tauri/src/mux/daemon/tests.rs`) carry no input either. The daemon
agent-status task and the scanner's item shapes are not changed.

Predecessor mitigations mux-osc-double-esc-abort TM-1 to TM-3 (a cut OSC 7501
string yields nothing, a cut OSC 133 string is live by its own last byte, the
cut path is bounded) are unchanged by this feature; TM-n without a prefix are
this document's.

## Trust Boundaries

### TB-1: PTY output into the mux reader's per-read live-span derivation
Crossing: bytes written by any program in a mux pane cross into the per-pane
reader thread. The reader's carried switch-sequence state and the live spans
it derives decide which OSC 133 marks reach the daemon's OSC 7501 table, OSC
777 state and inferred-clear latch, and the `AgentStatusUpdate` broadcast to
attached GUIs; the unchanged write path in the same step decides what reaches
the scrollback ring and the suppressed-chunk replacement sent to an attached
client.
Boundary files: src-tauri/src/mux/ipc/pty_spawn/write_filter.rs, src-tauri/src/mux/ipc/pty_spawn/mod.rs
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Spoofing | A mark printed on the alternate screen — for example by a pager or TUI displaying untrusted text — is taken as a live prompt mark when the screen state is derived per read: today the bytes right after a switch to the alternate screen whose sequence straddled a read boundary count as main-screen bytes (FR3), and a carried partial state that completes a switch term_core does not make (a switch whose `ESC` sits in a charset-designator slot, FR4) or that misses one would do the same. Such a mark removes working / blocked / idle records and fires the OSC 777 inferred clear (FR1, FR3, FR4, FR6; AC4, AC5) | TM-1 | The scanner's live spans follow term_core's rule on every byte a read can decide: a switch completed across reads makes the following alternate-screen bytes non-live; a switch whose `ESC` is a designator never changes liveness; the bytes of a read's undecided tail are never live; a read whose screen change the tracker cannot reconcile with the shadow parser gets today's fallback (no live span when the alternate screen is involved) | task0001 AC-1, AC-3, AC-6 | VERIFICATION.md Performance / Security Verification: TM-1 |
| Denial of service | The new per-read state runs on every read of untrusted output in the reader thread: carried state that grows with the input, a position computation at a read boundary that reaches outside the read, or a rescan per carried byte would panic or stall the reader thread under a hostile stream of partial switch sequences (NFR1) | TM-2 | The carried state is a constant number of flags plus at most 7 bytes of a switch-pattern prefix; each read is processed in one forward pass; no input — any byte after any partial prefix, reads of a single byte — can panic | task0001 AC-5 | VERIFICATION.md Performance / Security Verification: TM-2 |
| Tampering | With two span sets in one reader step, using the scanner's exact live spans in the write path would let crafted split switch sequences change what is written to the scrollback ring and how the suppressed-chunk replacement maps ring-written ranges back to the read, so alternate-screen bytes or partial switch sequences reach the ring or an attached client's replay (FR5; AC6) | TM-3 | The exact live spans are used only as the feed scanner's input; the bytes to write, the cuts, the fallback's empty write and the ring-written ranges handed to the suppressed-chunk pipeline are derived exactly as before | task0001 AC-4 | VERIFICATION.md Performance / Security Verification: TM-3 |
