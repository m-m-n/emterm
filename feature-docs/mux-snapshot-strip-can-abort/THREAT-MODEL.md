# Threat Model: mux-snapshot-strip-can-abort

## Verdict
threats-identified

## Rationale
Inspected: SPEC.md and REQUIREMENTS.md (FR1-FR7, NFR1-NFR3), tier full, the
strip pass in src-tauri/src/mux/scrollback_filter.rs and its caller on the
write path, the scrollback write filter in
src-tauri/src/mux/ipc/pty_spawn/write_filter.rs.

The bytes the strip classifies are the PTY output of whatever runs in a mux
pane: a shell, any program it starts, a remote host relayed over SSH, the
contents of a file printed with cat. The mux daemon does not control that
output. The stripped ring is replayed by the GUI client's term_core on every
reattach. The strip is the layer that keeps that untrusted output from
re-running side effects on replay (viewer launches, inline images, answered
device queries) and from losing what the client displayed. This feature
changes how the strip ends a Kitty APC / DCS body, so it sits on that
boundary.

task0001 declares the input-handling domain, which sets the depth of TB-1 to
deep. Its file set includes src-tauri/src/mux/scrollback_filter.rs and
src-tauri/src/mux/ipc/pty_spawn/write_filter.rs, both on TB-1's Boundary
files line, so the post-decomposition consistency check holds. No
authentication, persistence format, or external service is involved. SPEC.md's
Security Considerations reads "Not applicable". The threats below are stated
here and cite the requirement IDs whose behavior prevents them.

## Trust Boundaries

### TB-1: Pane process output into the mux strip and the snapshot replay
Crossing: bytes written by a process running in a mux pane (another process, possibly relaying a remote host or a file's contents) enter the mux daemon's scrollback write filter and strip pass, are stored in the pane ring, and are replayed by the GUI client's term_core (another process) on reattach.
Boundary files: src-tauri/src/mux/scrollback_filter.rs, src-tauri/src/mux/ipc/pty_spawn/write_filter.rs
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | A Kitty APC / SIXEL DCS body that a cut closed with `ESC` + CAN, or that any ESC not followed by `\` aborted, makes the strip remove everything up to the next ST anywhere later in the ring. Plain text and kept OSC written afterwards, including the output of later processes in the same pane, vanish from the snapshot replay. The replay then differs from what the client displayed (FR1, FR2). | TM-1 | The body end scan stops at the first ESC in the body. An aborted body's removal ends just before the aborting ESC, and strip judgement resumes there, so the bytes after it are kept (FR1, FR2, FR3). | task0001 AC-1, AC-2 | VERIFICATION.md Performance / Security Verification, TM-1 |
| Tampering | Crafted output uses an aborted body to carry a replay side effect past the strip. One form keeps the aborted Kitty APC / SIXEL DCS body itself, which the client's parser hands to its APC / DCS image handling when the body is aborted (create-spec analysis). The other keeps a strip target (OSC 777 viewer launch, OSC 9999 emterm-md, agent-status report, Kitty APC, SIXEL DCS, answered CSI device query) that starts at the aborting ESC. Either one is kept in the ring and runs again on every reattach (FR2, FR3, FR5). | TM-2 | An aborted Kitty APC body is removed. An aborted DCS body is removed when its pre-abort range is SIXEL. A strip target that starts at or after the aborting ESC is removed as its own construct under the existing rules (FR2, FR3, FR5). | task0001 AC-3, AC-8 | VERIFICATION.md Performance / Security Verification, TM-2 |
| Denial of service | Output made of many aborted or unterminated APC / DCS introducers makes the strip rescan the tail once per introducer (quadratic). That stalls the mux daemon's PTY reader, which runs the write-path strip on every read, and the snapshot build (NFR1). | TM-3 | The body end scan is bounded by the next ESC. The ranges scanned for distinct introducers never overlap, so the strip stays one linear pass (NFR1). | task0001 AC-7 | VERIFICATION.md Performance / Security Verification, TM-3 |
