# Threat Model: mux-strip-open-string-body-closure

## Verdict
threats-identified

## Rationale
Inspected: SPEC.md, REQUIREMENTS.md, the shared strip and the written-state
model (`src-tauri/src/mux/scrollback_filter.rs`), the scrollback write filter
(`src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`), the snapshot builders
(`src-tauri/src/mux/snapshot_bytes.rs`) and term_core's OSC abort behavior
(`crates/term_core/src/parser/osc.rs`). Tier: full. task0001 declares
`input-handling`, so the one boundary found is analyzed at deep depth.

The feature's only input is pane output written by whatever program runs in
the pane. A user only has to display crafted output (for example `cat` of a
crafted file) for it to reach the mux daemon. The daemon stores it in the
scrollback ring after the write-path strip. The client later replays the ring
in term_core, and term_core writes its answers to color queries into the PTY
input of the foreground program. That chain is a trust boundary, and
Tampering and Denial of service realistically apply to it.

No other category gets a row. The color response has a fixed format, so it
grants no privilege, and the theme colors it carries are already answered to
any program that asks for them directly. Nothing here identifies a party or
needs an audit trail.

SPEC.md's Security Considerations (what is protected) is not restated here.
The threats below cite the requirement IDs instead.

Boundary files list only the production modules that implement the boundary.
The new and changed test modules, DECISIONS.md and the predecessor test-docs
record are not boundary files. Domain consistency check: task0001 declares
`input-handling`, and its production files `scrollback_filter.rs`,
`write_filter.rs` and `snapshot_bytes.rs` all appear in TB-1's Boundary files.

## Trust Boundaries

### TB-1: Pane output into the scrollback ring and its client replay
Crossing: bytes written by the program running in a pane (untrusted, possibly
crafted output) cross into the mux daemon. There the write-path strip stores
them in the scrollback ring. The snapshot-time strip later sends them to the
client, whose term_core replays them and writes any query answers into the
PTY input of the foreground program.
Boundary files: `src-tauri/src/mux/scrollback_filter.rs`, `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`, `src-tauri/src/mux/snapshot_bytes.rs`
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | A removal inside a written, still-open OSC / DCS / APC body splices the bytes before and after the removed construct. An OSC color-query head (OSC 4/10/11/12) followed by a removed construct and `?` BEL becomes a complete color query in the ring that the raw stream never made. Snapshot replay then answers it. When the ring ends with the head plus a removed construct, a later live `?` BEL also gets an answer, because the replayed parser is left inside the body. Either way a color response is written into the foreground program's input (FR1, FR3, FR7). | TM-1 | At every removal made while the written stream is inside an open OSC body or an open ST-terminated (DCS / APC) body, the shared strip first writes the string-body closure: ESC then CAN, the shared STRING_BODY_CLOSING. The closure comes before any re-emitted C0 byte and leaves the written state in Ground. On replay the body is aborted where the raw stream's construct `ESC` aborted it, so no later byte can join the body. This holds for every strip entry point, for bodies written earlier in the same pass, and for bodies carried in by the write filter (including after an overflow flush and a held live lone `ESC`). | task0001 AC-1, AC-2, AC-3, AC-4 | VERIFICATION.md Performance / Security Verification item TM-1 (TS-1, TS-2, TS-3, TS-4, TS-5, TS-6) |
| Tampering | The inserted closure itself changes later processing, so the replay position again differs from the raw stream (FR5, FR6). Four ways this could happen: a later cut or the reader's fallback closing adds a second closure; the write-path or snapshot-time strip reads the closure as the start of a target or as ST; a target written right after the closure survives; the vt100 replay copy rewrites the closure. | TM-2 | The closure leaves the written state in Ground with no designator awaited, so a cut in the same call, a cut at fed offset 0 of the next call and the fallback closing all write nothing. Both strips keep ESC CAN unchanged and still remove a target that follows it, without adding a closure. The vt100 replay copy keeps the closure byte for byte, because it contains no DEL. | task0001 AC-6 | VERIFICATION.md Performance / Security Verification item TM-2 (TS-8) |
| Denial of service | Hostile output that packs many removals into open bodies makes the strip do superlinear work, or makes the ring grow, now that every such removal writes bytes (NFR2, SPEC A6). | TM-3 | The closure is written inside the existing single pass with O(1) extra state. Each closure (2 bytes plus the construct's own re-emitted C0 bytes) replaces a removed construct of at least 3 bytes, so the output is never longer than the input. | task0001 AC-10 | VERIFICATION.md Performance / Security Verification item TM-3 (TS-14) |
