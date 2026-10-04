# Threat Model: mux-strip-non-sixel-dcs-linear

## Verdict
threats-identified

## Rationale
Inspected: SPEC.md (FR1-FR8, NFR1-NFR4, A1-A6), REQUIREMENTS.md, the
feature goal (a program writing adversarial output into a pane can occupy the
mux daemon's CPU) and the code paths SPEC.md names. Tier: full. The single
task (task0001) declares `input-handling`, which sets TB-1 to deep.

The feature adds tests and changes production code only under FR7, but the
code those tests exercise sits on the boundary where a pane program's output
enters the daemon, so that boundary is modeled here. task0001's test module,
module registration and test-docs record are verification files and implement
no boundary; its conditional FR7 files (the shared strip, the write filter,
the client-parity scan, the suppressed replacement) are TB-1 boundary files.

## Trust Boundaries

### TB-1: Pane program output into the mux daemon
Crossing: bytes a program running in a mux pane writes to its PTY (untrusted,
attacker-influenceable) cross into the daemon's reader thread, which runs them
through the write filter into the scrollback ring and, for a suppressed read,
through the client-parity scan; the ring's bytes are later run through the
shared strip by the snapshot builders on reattach and on visibility resume.
Boundary files: src-tauri/src/mux/ipc/pty_spawn/mod.rs, src-tauri/src/mux/ipc/pty_spawn/write_filter.rs, src-tauri/src/mux/ipc/pty_spawn/client_parity_scan.rs, src-tauri/src/mux/ipc/pty_spawn/suppressed_output.rs, src-tauri/src/mux/scrollback_filter.rs, src-tauri/src/mux/snapshot_bytes.rs
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Denial of service | A program writes repeated non-SIXEL DCS introducers followed by one ST; a scan that re-walks the same bytes for each introducer turns this into quadratic work and occupies the daemon's CPU on the write path or at snapshot time (FR1-FR6, NFR1) | TM-1 | Every scan on these paths stops at the first `ESC` of a body, so each byte is scanned a bounded number of times; budget regression tests at sizes where a quadratic scan exceeds the 10 s budget by orders of magnitude guard the write path (write filter, Detached reader, suppressed-read scan) and the snapshot path (shared strip, both snapshot builders, direct scan) | task0001 AC-1, AC-2, AC-3, AC-4, AC-5 | VERIFICATION.md Performance / Security Verification, TM-1 |
| Tampering | A linearity fix made under FR7 changes what the strip removes, so a pane program's output replays content the strip removed before (a SIXEL DCS) or loses bytes it kept before (a non-SIXEL DCS) (FR7, NFR4) | TM-2 | A FR7 change keeps every strip-target decision: a non-SIXEL DCS stays byte for byte and a SIXEL DCS is removed as before; the new tests assert the non-SIXEL input is written unchanged and every existing strip-target test stays green | task0001 AC-1, AC-6 | VERIFICATION.md Performance / Security Verification, TM-2 |
