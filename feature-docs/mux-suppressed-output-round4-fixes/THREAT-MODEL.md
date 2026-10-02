# Threat Model: mux-suppressed-output-round4-fixes

## Verdict
threats-identified

## Rationale
Inspected: SPEC.md and REQUIREMENTS.md (FR1-FR6, NFR1-NFR6), and the code
the requirements name under `src-tauri/src/mux/ipc/pty_spawn/`: the write
filter and main-span extraction (`write_filter.rs`), the reader's capture
step and cut derivation (`mod.rs`), the client-parity scan
(`client_parity_scan.rs`) and the suppressed-chunk replacement
(`suppressed_output.rs`). Tier: full. The design step was skipped (no UI
surface). Every task declares `input-handling`, so the one boundary found is
analyzed at deep depth.

The feature's only untrusted input is the byte stream a program running in
a mux pane writes to its PTY. The daemon turns that stream into scrollback
ring content, snapshot replay and replacement `PtyOutput` chunks, which the
client's terminal parser executes. The client answers queries back into the
pane's PTY and launches viewers, so a misparse on the daemon side lets the
program make the client act on a sequence the client never started
(Tampering). The same stream drives the daemon's scans, so it can also aim
at the reader thread's time and memory (Denial of service).

Not recorded, and why:
- No second boundary. The per-destination boundary record, the `mux_ipc`
  wire format and the snapshot frame shape are unchanged (NFR1). No new
  IPC, file or network path is added.
- Information disclosure: the feature only decides which bytes of a pane's
  own output reach that pane's ring and the destination that received the
  covering snapshot. No path between panes or destinations changes.
- Spoofing, Repudiation, Elevation of privilege: no identity, audit trail
  or privilege is involved.

Domain consistency check (after task decomposition): all five tasks declare
`input-handling`, and every runtime file they change (`write_filter.rs`,
`mod.rs`, `client_parity_scan.rs`, `suppressed_output.rs`) is a TB-1
boundary file. Their other files are test modules under
`src-tauri/src/mux/ipc/pty_spawn/`, the decision record `DECISIONS.md` and
predecessor `test-docs/` records; none of them handles input at run time,
so they are not boundary files. task0005 also declares `concurrency`, which
is not a depth domain.

## Trust Boundaries

### TB-1: Pane program output to the client terminal parser through the mux daemon
Crossing: bytes written by any program running in a mux pane (untrusted) pass through the daemon's reader thread (main-span extraction, cut derivation, write filter, scrollback ring, client-parity scan, suppressed-chunk replacement), then reach the client's term_core parser as snapshot replay and `PtyOutput` chunks. The client answers queries into the pane's PTY and launches viewers.
Boundary files: src-tauri/src/mux/ipc/pty_spawn/write_filter.rs, src-tauri/src/mux/ipc/pty_spawn/mod.rs, src-tauri/src/mux/ipc/pty_spawn/client_parity_scan.rs, src-tauri/src/mux/ipc/pty_spawn/suppressed_output.rs
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | A crafted byte sequence that overlaps a snapshot capture makes ring replay, a replacement item or the re-delivered tail start a query (OSC 11 color query, CSI DSR) or a viewer launch that the live client never started. Paths: the head of a chain closed by a cut stays in the ring (FR1, 3e2024dce619ed9f); an item is found after an `ESC (` chain inside the chunk in the as-05 fallback (FR2, 989ec5c588abce06); an awaiting `ESC (` left at a cut consumes a later byte as its designator (FR3, 48caec6f5b0b5810); a CSI is left open at a cut (FR4); a switch in the designator slot is misread by extraction (FR5). Relates to NFR4. | TM-1 | At every cut and fallback closing, the ring, the replacement items and the tail leave the client parser in the live client's state. A chain closed by a cut is not written from its head on, and a chain is held across reads until it settles or is closed. The as-05 fallback keeps only the items and tails that both readings of the chunk's first byte agree on. One ESC is written after an awaiting `ESC (` / `ESC )` at a cut or fallback closing. If the adjacent paths reproduce, an open CSI is closed without dispatch at a cut, and extraction treats a designator-slot ESC as term_core does. | task0001 AC-1, AC-2; task0002 AC-1, AC-2; task0003 AC-1; task0004 AC-2; task0005 AC-2 | VERIFICATION.md Performance / Security Verification, TM-1 item |
| Denial of service | Adversarial PTY output can try to hurt the reader thread: long aborted-string chains, long `ESC (` chains in the fallback window, repeated switches straddling reads, many tiny reads. The goal is to make the boundary scan, the strip, the two-reading client-parity scan or extraction run super-linearly, grow memory without bound, panic, or send an empty `PtyOutput` chunk. Relates to NFR5 and NFR3. | TM-2 | Every changed scan stays a bounded single pass that never panics. A held chain counts toward the 512 KiB pending cap and takes the strip-filtered overflow flush. The scan of a held chain resumes at its stored construct start, so byte-at-a-time feeding stays linear. FR2's second reading runs only under the as-05 fallback condition. No empty replacement is sent. | task0001 AC-5; task0002 AC-5; task0003 AC-4; task0004 AC-4; task0005 AC-4 | VERIFICATION.md Performance / Security Verification, TM-2 item |
