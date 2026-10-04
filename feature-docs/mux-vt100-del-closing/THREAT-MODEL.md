# Threat Model: mux-vt100-del-closing

## Verdict
threats-identified

## Rationale
Inspected: this feature's SPEC.md and REQUIREMENTS.md (tier: full) and the
three code paths it changes: the new vt100 replay-copy conversion in the
shared strip module, ReadPane rendering, and the restored shadow-parser
replay. The conversion reads the bytes that programs running in mux panes
wrote to their PTYs, as held in the scrollback ring. That is another
process's output, so a trust boundary exists (TB-1). task0001 declares the
`input-handling` domain, which sets this boundary's depth to deep.

Only Denial of service realistically applies. The conversion is new code
that runs on bytes another program chose, and no panic guard surrounds it
on the ReadPane path. Tampering was considered and not recorded. The
feature makes the vt100 view follow `term_core`'s handling of DEL, and the
ring and the client bytes stay unchanged (NFR1), so the feature adds no
new way to alter what a pane shows. The existing raw-CAN difference is out
of scope per SPEC.md. Spoofing, Repudiation, Information disclosure and
Elevation of privilege do not apply, because no identity, audit record,
secret or privilege is involved. The feature does not change the local IPC
between the mux CLI and the daemon.

## Trust Boundaries

### TB-1: Pane PTY output in the scrollback ring to the daemon's vt100 parsers
Crossing: bytes written to a pane's PTY by any program running in that pane (another process, with content fully chosen by that program), stored in the pane's scrollback ring (live, or carried into a restored pane), and read by the mux daemon into the vt100 replay copy for ReadPane rendering and for the restored shadow-parser replay.
Boundary files: src-tauri/src/mux/scrollback_filter.rs, src-tauri/src/mux/ipc/handlers/agent_api.rs, src-tauri/src/mux/session/pane/mod.rs
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Denial of service | A program writes a byte sequence (any byte value, in any escape, CSI or string state the scan can reach) that makes the new conversion panic or do more than linear work. On the ReadPane path (FR1) this fails the request, with no panic guard around it. On the restore path (FR2) it holds up or aborts the pane's restore (FR3, NFR2). | TM-1 | The conversion is total. For every input it returns, without panicking, a copy of exactly the input's length. It walks the input once with O(1) state besides the copy and uses only the existing `WrittenState` transition, which is defined for every byte. | task0001 AC-6 | VERIFICATION.md, Performance / Security Verification item TM-1 (TS-7) |
