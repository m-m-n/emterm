# Threat Model: mux-strip-escape-state-carry

## Verdict
threats-identified

## Rationale
Inspected: SPEC.md (FR1-FR6, NFR1-NFR5, Security Considerations), REQUIREMENTS.md, the
scrollback write filter (`src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`) and the shared
strip (`src-tauri/src/mux/scrollback_filter.rs`). Tier: full; the design step was skipped
(no UI). task0001 declares `input-handling`, which sets a deep analysis of TB-1; task0002
declares no domain (it writes a decision record inside the repository and a test that reads
it, and crosses no trust boundary).

The feature changes how bytes written by an untrusted child process are filtered, stored in
the per-pane scrollback ring and later replayed into the client's terminal parser, and that
parser answers device queries by writing responses back into the pane's PTY input. That
crossing is a trust boundary with realistic Tampering and Denial-of-service threats, so the
verdict is `threats-identified`. The mux_ipc wire format, the Snapshot / SnapshotRestore
frame shape and the GUI-daemon channel are unchanged (NFR1), so no other boundary is touched.

## Trust Boundaries

### TB-1: Child-process PTY output to the scrollback ring and its snapshot replay into the client parser
Crossing: bytes written by any process running in a pane (untrusted and
attacker-influenceable, e.g. a program printing a crafted file) are read by the mux daemon's
reader, filtered by the scrollback write filter, stored in the pane's scrollback ring, and
replayed into the GUI client's term_core parser on reattach or window switch; term_core
answers device queries by writing responses into the pane's PTY input, where whichever
program is in the foreground at replay time reads them.
Boundary files: src-tauri/src/mux/ipc/pty_spawn/write_filter.rs, src-tauri/src/mux/scrollback_filter.rs
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | Crafted output puts a written ESC right before a construct the strip removes, a PTY read split falls right after that construct, and a snapshot cut follows (review finding a879a02de382209f; SPEC FR1, FR3, FR4, NFR3). Because the carried state narrows the written end state Escape to ground, the ring ends in an unclosed `ESC[6` or a lone ESC, so on replay the bytes after the cut complete a query (CPR, DA) or a reset (RIS) the client never started, and the client's answer is injected into the PTY input of whatever program reads it at replay time. | TM-1 | Carry the written end state with Escape and Designator kept distinct from ground, and at every cut write at most one closure chosen from that state (DEL for an open CSI, the Escape closure for Escape, the designator ESC for a designator wait, nothing for ground). The Escape closure returns term_core to ground with no display, cursor, response, mode or charset effect, is neither ESC nor `\`, and so never forms ST for the snapshot strip's Kitty APC / SIXEL DCS terminator search (IMPLEMENTATION.md D1, D2). | task0001 AC-3, AC-6, AC-7 | VERIFICATION.md Performance / Security Verification, item TM-1 |
| Denial of service | Adversarial output (long runs alternating written ESCs and removed constructs, an OSC held up to the 512 KiB pending cap and then flushed) makes the end-state tracking add per-byte rescans or unbounded holding, or makes the daemon's reader thread panic (SPEC NFR2, NFR4). | TM-2 | Derive the end state as O(1) state inside the existing single strip pass, add no scan of the fed bytes to the reader path, hold no CSI or Escape bytes, keep the 512 KiB pending cap and the overflow flush through the strip, and use no panicking arithmetic or indexing on the changed paths. | task0001 AC-8 | VERIFICATION.md Performance / Security Verification, item TM-2 |
