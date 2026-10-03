# Threat Model: mux-write-filter-overflow-open-string-cut

## Verdict
threats-identified

## Rationale
Inspected: SPEC.md and REQUIREMENTS.md (the design step was skipped, no
DESIGN.md), at the full tier. The feature changes how the mux daemon's
scrollback write filter tracks and closes PTY output it stores in a pane's
ring. That output is produced by whatever runs in the pane (including remote
programs over SSH), so it is untrusted, and the ring is later replayed into a
client's terminal parser on reattach or window switch. This is one trust
boundary, TB-1. task0001 declares `input-handling`, so TB-1 is analysed at
deep depth. The feature adds no dependency, no network access, no persistence
format and no authentication or privilege change. No denial-of-service threat
is recorded: the added state is O(1), a cut writes at most two closure bytes,
and the pending cap is unchanged (FR1, FR5, NFR1, NFR5). SPEC.md's Security
Considerations is N/A; the threats below cite FR IDs.

Post-decomposition check: task0001 declares `input-handling`, and its
production files `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` and
`src-tauri/src/mux/scrollback_filter.rs` are TB-1's boundary files; its other
files are tests.

## Trust Boundaries

### TB-1: PTY output → scrollback write filter → pane ring → snapshot replay
Crossing: bytes a child process writes to its PTY (untrusted, attacker-influenceable) cross into the mux daemon's write filter, are stored in the pane's scrollback ring, and are later replayed into a client's terminal parser (term_core) on reattach or window switch.
Boundary files: src-tauri/src/mux/ipc/pty_spawn/write_filter.rs, src-tauri/src/mux/scrollback_filter.rs
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | Pane output opens an OSC / DCS / APC string and grows it past the 512 KiB cap, so the overflow flush writes the open body to the ring, and a cut follows. Output written after the cut, possibly by other programs, is absorbed into that body on replay, and a later BEL / ST dispatches it as the string's payload (for an OSC 0, a window title the client never received), so the replayed state diverges from what the client saw (FR1, FR2, FR3) | TM-1 | The written end state tracks the open OSC body and the open ST-terminated body, and every cut path writes `ESC` + CAN when the written stream ends in one, so the ring's string is aborted where the client's was | task0001 AC-2, AC-3, AC-4, AC-7 | VERIFICATION.md Performance / Security Verification, TM-1 (TS-1, TS-2, TS-3, TS-4) |
| Tampering | The changed state stored with a held lone `ESC`, or the new closure bytes, let a strip target split right after a flushed `ESC` reach the ring in executable form (for an answered device query, a replay that makes the client send a stale response into the pane's input), or let the closure form ST or open a construct the client never saw (FR4, FR5) | TM-2 | The overflow hold decision is unchanged: an `ESC` that ends a run inside an open body is still held and stripped together with its continuation; the closure is `ESC` followed by CAN, which is neither ST nor the opener of any strip target | task0001 AC-6, AC-8 | VERIFICATION.md Performance / Security Verification, TM-2 (TS-6, TS-10) |
