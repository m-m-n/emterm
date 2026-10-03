# Threat Model: mux-strip-concat-query-closure

## Verdict
threats-identified

## Rationale
Inspected: SPEC.md, REQUIREMENTS.md (FR1-FR8, NFR1-NFR4, EC-1-EC-11) and the
mux daemon byte-stream filtering the feature changes (shared strip, write
filter, snapshot assembly). Tier: full. The design step was skipped (no UI
surface). The single task, task0001, declares `input-handling`, so its
boundary is analyzed at deep depth. task0001's code files appear in TB-1's
`Boundary files`; its other files are tests and DECISIONS.md, which carry no
boundary of their own.

The feature crosses one trust boundary: the output of a program running in a
mux pane is attacker-influenceable (a crafted file shown with `cat`, a remote
host reached over SSH, any program the user runs). That output passes through
the daemon's filters into the scrollback ring, is replayed to a client
terminal on a snapshot, and the client's term_core answers device queries by
writing replies into the same PTY. The reply is input to the foreground
program. Tampering (an unsolicited reply forged through the strip's joins) and
Denial of service (filter cost on adversarial streams) apply. Spoofing,
Repudiation, Information disclosure and Elevation of privilege do not apply on
their own: the feature adds no identity, audit, secret or privilege surface.
The cursor position or device attributes that an unsolicited reply carries
are part of the Tampering threat (the reply arrives as forged input). They
add no separate disclosure path.

## Trust Boundaries

### TB-1: Pane program output into the scrollback ring and client replay
Crossing: bytes written by the program in a mux pane (untrusted) cross into
the mux daemon's reader, the write filter and the shared strip, then into the
scrollback ring. From there snapshot assembly replays them to the client
terminal, whose term_core may write a device reply back into the pane's PTY
as input.
Boundary files: `src-tauri/src/mux/scrollback_filter.rs`, `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`, `src-tauri/src/mux/snapshot_bytes.rs`
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | Output is arranged so that the shared strip removes a construct together with its opening ESC while the written stream is inside an open CSI or right after a superseded lone ESC. The bytes on either side then join into an escape or an answered device query that the raw stream never made. A replayed snapshot or a live continuation makes the client inject an unsolicited reply into the PTY (FR1, FR2, FR4, NFR3; the residuals 1 and 2 recorded by the predecessor feature). | TM-1 | At a removed construct, every strip entry point writes exactly one CSI_CLOSING (DEL) when the written stream is inside a CSI or in the escape state there. The closing goes before any re-emitted C0 byte, and nothing is written in ground. The closing leaves the stream in ground, so no snapshot is left inside a strip-induced open CSI. No closing is added at the snapshot end. | task0001 AC-1, AC-2, AC-4 | VERIFICATION.md TS-1, TS-2, TS-3, TS-6 and the TM-1 item |
| Tampering | A CSI device query is split across PTY reads, by timing or on purpose. The write filter does not hold CSI bytes, so the query is written to the ring in executable form (residual 3). The ring's safety then rests on the snapshot-time strip alone (FR3). | TM-2 | The write filter carries an O(1) classification of the open CSI that follows term_core's transitions and response conditions. When a later read completes the CSI as an answered device query, CSI_CLOSING is written in place of the completing final byte. C0 bytes in the continuation are written once and in order, non-query CSIs are written unchanged, and the classification is cleared at a cut. | task0001 AC-3 | VERIFICATION.md TS-4, TS-5 and the TM-2 item |
| Denial of service | Adversarial output makes the per-read filter super-linear or unbounded in memory, or makes it panic, which stalls the daemon reader. Examples: long alternations of open CSIs and strip targets, very long CSI parameter runs fed whole or byte by byte, long ESC ESC chains before removed constructs, the overflow path (NFR1, NFR2). | TM-3 | The strip stays a single O(n) pass with O(1) extra state. The carried classification is O(1) with saturating parameter accumulation, and no CSI byte is held in pending. Budget tests bound each adversarial shape. | task0001 AC-7 | VERIFICATION.md TS-9 and the TM-3 item |
