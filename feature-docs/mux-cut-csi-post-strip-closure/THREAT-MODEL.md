# Threat Model: mux-cut-csi-post-strip-closure

## Verdict
threats-identified

## Rationale
Inspected: SPEC.md and REQUIREMENTS.md (FR1-FR6, NFR1-NFR5), and the code the
requirements name: the write filter's boundary scan, cut closing, overflow
flush and carried CSI state (`src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`)
and the shared strip it calls (`src-tauri/src/mux/scrollback_filter.rs`).
Tier: full. The design step was skipped (no UI surface). task0001 declares
`input-handling`, so the one boundary found is analyzed at deep depth.

The feature's only untrusted input is the byte stream a program running in a
mux pane writes to its PTY. The daemon's write filter turns that stream into
scrollback ring content, which the client's term_core parser replays at a
snapshot restore; the client answers device queries back into the pane's PTY.
A CSI state misjudged at a cut lets the program make the replaying client
complete and answer a query the live client never received (Tampering). The
same stream drives the filter's passes over its bytes (Denial of service).

Not recorded, and why:
- No second boundary. The `mux_ipc` wire format, the Snapshot /
  SnapshotRestore frame shape, the snapshot byte layout and the shared strip's
  output are unchanged (NFR1). No IPC, file or network path is added.
- Information disclosure: the feature only decides which bytes of a pane's own
  output reach that pane's ring. No path between panes or destinations
  changes.
- Spoofing, Repudiation, Elevation of privilege: no identity, audit trail or
  privilege is involved.

Known paths this feature does not mitigate: the three cut-free strip
concatenation cases of SPEC FR6, accepted by SPEC as-04 and recorded in
DECISIONS.md by task0002; and a construct the strip removes right after a kept
`ESC` (for example `ESC[6`, `ESC ESC`, a complete viewer launch, then a cut),
which leaves the emitted run in an escape state rather than a CSI. The latter
is outside FR1-FR3 and is listed under IMPLEMENTATION.md Open Questions. No
mitigation is claimed for either.

Domain consistency check (after task decomposition): task0001 declares
`input-handling`; its runtime files (`write_filter.rs`, `scrollback_filter.rs`)
are TB-1 boundary files, and its other files are test modules. task0002
declares no domain; its files are the decision record and a test module that
reads that record at test time, and neither handles input at run time.

## Trust Boundaries

### TB-1: Pane program output to the client terminal parser through the scrollback ring
Crossing: bytes written by any program running in a mux pane (untrusted) pass through the daemon's reader thread (main-span extraction, cuts, write filter, shared strip) into the per-pane scrollback ring, and reach the client's term_core parser as snapshot replay. The client answers device queries into the pane's PTY.
Boundary files: src-tauri/src/mux/ipc/pty_spawn/write_filter.rs, src-tauri/src/mux/scrollback_filter.rs
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | A program writes an open CSI (`ESC[6`) followed by a construct the strip removes together with its opening `ESC` (viewer launch, OSC 9999, agent-status, Kitty APC, SIXEL DCS, an answered CSI query such as `ESC[6n`), timed so that a cut falls right after it, so that a cut-free call carries the state into a later fallback closing, or so that an overflow flush precedes the cut. The closing decision uses the pre-strip state, which the removed `ESC` cleared, so no DEL is written; a later `n` completes the CSI on replay and the client answers a cursor-position query the live client never received. Relates to FR1, FR2, FR3, NFR3. | TM-1 | The cut closing and the carried CSI state follow the CSI state of the strip-applied output (IMPLEMENTATION.md D1) on the in-call cut, the empty-segment and fallback closing, and the overflow flush; one DEL is written when that output ends inside a CSI, and a held strip target that strips to nothing keeps the open CSI carried in. | task0001 AC-1, AC-2, AC-3, AC-4, AC-5, AC-6 | VERIFICATION.md Performance / Security Verification, TM-1 item |
| Denial of service | Adversarial output alternating open CSIs and strip targets, strip targets held near the 512 KiB pending cap, and byte-at-a-time reads aim to make the new state tracking run super-linearly, grow memory without bound or panic on the reader thread. Relates to NFR2, NFR4. | TM-2 | The post-strip CSI state is O(1) state advanced inside the strip's existing single pass; no new pass over the bytes is added on the reader's normal path; the pending cap and the strip-filtered overflow flush are unchanged; no panic path is added. | task0001 AC-8 | VERIFICATION.md Performance / Security Verification, TM-2 item |
