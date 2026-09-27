# Threat Model: mux-snapshot-output-boundary

## Verdict
threats-identified

## Rationale
- **Inputs inspected**:
  - The PTY output of child processes that the mux daemon's reader thread now parses in a new way. That output is untrusted: arbitrary programs, remote hosts reached over SSH, and file contents printed to the terminal.
  - The mux IPC requests from client processes (GUI or SSH bridge) that reach the modified snapshot, attach and visibility handlers.
- **Tier and depth**:
  - Tier: full.
  - Declared domains: `concurrency`, `input-handling`. `input-handling` makes the PTY-output boundary (TB-1) deep.
- **Why threats-identified**: the feature adds new daemon-side parsing of untrusted output (the replacement payload for a suppressed chunk, SPEC FR9 and FR10). It also restructures request handlers that carry the SPEC Security authorization rule. Both give realistic STRIDE threats.
- **Excluded**:
  - The wire format and frames are unchanged (NFR1).
  - The feature adds no network or file I/O.
  - Sequence numbers and boundaries live only in memory and are not added to the hot-upgrade hand-over format (ASM-3).
  - The feature makes no authentication change.
  - Deadlock and fairness risks of the new exclusions are reliability concerns, tracked in IMPLEMENTATION.md Risk Assessment, not threats.
  - Rich-content sequences that complete across the boundary may reach the client once (FR10). This equals the pre-change behavior and adds no new exposure: viewer isolation is unchanged.

## Trust Boundaries

### TB-1: Child-process PTY output → mux daemon reader → attached client terminal
Crossing: bytes written by an untrusted program on the pane's PTY. The daemon reader scans them, and for a suppressed chunk the new replacement builder extracts terminal queries and an incomplete trailing sequence and re-delivers them to the client, whose terminal answers queries by writing responses back into the same PTY as input.
Boundary files: `src-tauri/src/mux/ipc/pty_spawn/mod.rs`, `src-tauri/src/mux/ipc/pty_spawn/suppressed_output.rs`, `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`, `src-tauri/src/mux/scrollback_filter.rs`, `src-tauri/src/pty/passthrough_scanner.rs`
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | Crafted output makes the builder see a query that the client would not recognize there: query-shaped bytes inside an OSC, DCS or APC string payload, or inside the re-delivered tail. The builder might also re-deliver a query the snapshot kept. The client then writes extra device or color responses into the pane's PTY input, which the raw stream would not have triggered (relates to FR9, FR10, ASM-7). | TM-1 | The builder recognizes queries only where the client parser would start a control sequence, never inside a string payload or the tail region. It reuses the snapshot strip predicate as the single source of truth for device queries. Each query in the raw chunk reaches the client at most once in total across the snapshot and the replacement. | task0001 AC-6 | VERIFICATION.md TS-9; Performance / Security Verification item TM-1 |
| Denial of service | Crafted output makes suppressed-chunk handling stall the pane's reader thread. Examples: pathological input to a non-linear scan, or an unbounded replacement. Handling could also emit an empty replacement chunk, which the client reads as PTY exit and so closes the pane (relates to FR5, FR10, NFR4). | TM-2 | The builder runs only for suppressed chunks, in one forward pass over its input. The replacement is bounded by the write filter's pending cap or one read. An empty replacement is never sent. | task0001 AC-6 | VERIFICATION.md TS-10, TS-17; Performance / Security Verification item TM-2 |

### TB-2: mux IPC client connection → daemon snapshot, attach and visibility handlers
Crossing: requests from another process (the GUI or an SSH bridge client) over the mux socket. They select a pane and receive its snapshot, and after this feature they also cause a per-sender suppression boundary to be recorded on that pane.
Boundary files: `src-tauri/src/mux/ipc/handlers/mod.rs`, `src-tauri/src/mux/ipc/handlers/attach.rs`, `src-tauri/src/mux/ipc/reattach.rs`, `src-tauri/src/mux/session/pane/output_target.rs`, `src-tauri/src/mux/session/pane/output_queue.rs`, `src-tauri/src/mux/session/pane/output_capture.rs`
Depth: standard

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Information disclosure | The on-demand snapshot path is restructured: its reads move under the capture exclusion, and it enqueues together with the boundary. A connection could then read or receive a pane outside its session, or replacement bytes could reach a connection other than the snapshot's destination (relates to SPEC Security, FR11). | TM-3 | The existing session authorization check stays ahead of the captured read. A rejected request records no boundary and enqueues nothing. Replacement bytes and boundary effects are bound to the destination that received the snapshot. | task0001 AC-4 | VERIFICATION.md TS-11; Performance / Security Verification item TM-3 |
| Denial of service | Clients that connect, receive a snapshot and disconnect, repeatedly over a long-lived daemon, leave one boundary entry each on the pane. The record grows without bound (relates to FR11). | TM-4 | Recording a boundary prunes the entries of closed senders, so the record only ever holds entries for live senders. Recording again for the same sender updates its entry in place. | task0001 AC-4 | Performance / Security Verification item TM-4 |
