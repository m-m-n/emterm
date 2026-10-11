# Threat Model: mux-osc-double-esc-abort

## Verdict
threats-identified

## Rationale
Inspected: SPEC.md, REQUIREMENTS.md and the code paths the feature touches —
the mux feed scanner and the main-buffer span extraction
(`src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`), the reader's call into the
scanner (`src-tauri/src/mux/ipc/pty_spawn/mod.rs`), the daemon task that
applies the scanner's items (`src-tauri/src/mux/daemon/tasks.rs`), and, as the
unchanged reference, term_core's OSC escape state and the plain-tab callbacks'
handling of unterminated strings. Tier: full. task0001 declares
`input-handling`, so its boundary is analysed at deep depth.

The only input the feature handles is PTY output written by programs in a mux
pane, local or over SSH, which is untrusted. It crosses one boundary the
feature changes: the per-pane reader thread's feed scanner, whose items change
the daemon's OSC 7501 table, OSC 777 state and exit latch, and the broadcast
to attached GUIs. The feature changes how strings cut short at that boundary
are classified, so threats exist.

The plain-tab path (term_core, the callbacks, the tab) is the unchanged
reference (SPEC A2, NFR5) and gains no boundary. task0002 declares
`input-handling` because its tests drive PTY-shaped bytes through that path,
but its only file, `src-tauri/src/tabs/tests/output_pipeline.rs`, is test code
and appears in no boundary line. The test files of task0001
(`src-tauri/src/mux/ipc/pty_spawn/tests.rs`, `src-tauri/src/mux/daemon/tests.rs`)
carry no input either. The reader and the daemon task are not changed, and the
item shapes they exchange stay the same.

Predecessor mitigations osc7501-leading-zero-length TM-3 (received length)
and TM-4 (shared number rule) are unchanged by this feature; TM-n without a
prefix are this document's.

## Trust Boundaries

### TB-1: PTY output into the mux daemon's feed scanner
Crossing: bytes written by any program running in a mux pane cross into the
per-pane reader thread's feed scanner, and from its items into the daemon's
OSC 7501 record table, OSC 777 state and inferred-clear latch, and the
`AgentStatusUpdate` broadcast to attached GUIs.
Boundary files: src-tauri/src/mux/ipc/pty_spawn/write_filter.rs
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | An OSC 7501 string cut short by an in-body `ESC` and a following `ESC` is taken by the scanner as an ST-terminated report (`ESC ESC \`), so a string the protocol treats as unterminated changes the pane's records, sends a broadcast and moves the revision, while the plain tab drops it (FR2, FR7; AC1–AC3) | TM-1 | Every byte other than `\` after an in-body `ESC` ends the string as unterminated; an unterminated OSC 7501 string yields no item, so it changes no record, sends no broadcast and moves no revision | task0001 AC-1, AC-2, AC-6 | VERIFICATION.md Performance / Security Verification: TM-1 |
| Spoofing | Once cut OSC 133 strings are applied (FR4), a mark printed on the alternate screen — for example by a pager showing untrusted text — and cut short could be taken as a live prompt mark if its liveness were judged at the cutting byte or not at all, removing working / blocked / idle records and firing the OSC 777 inferred clear (FR4, FR7; AC6, AC7) | TM-2 | A cut OSC 133 string yields an item only when its own last byte before the in-body `ESC` lay in a live main-screen span; the liveness is carried across reads; a string that ended on the alternate screen yields nothing | task0001 AC-3, AC-5, AC-6 | VERIFICATION.md Performance / Security Verification: TM-2 |
| Denial of service | The cut path runs on every byte of untrusted output in the per-pane reader thread: a backward position lookup at a read boundary, a body not cleared on a cut, or state that grows per cut would panic the reader thread or grow its memory under a hostile stream of cut strings (NFR1) | TM-3 | The cut path adds only O(1) state recorded while the string's bytes arrive (no backward position arithmetic), clears the body on every cut, keeps the 8 KiB carry bound, and cannot panic on any input | task0001 AC-1, AC-5 | VERIFICATION.md Performance / Security Verification: TM-3 |
