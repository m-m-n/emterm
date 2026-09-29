# Threat Model: mux-suppressed-output-round2-fixes

## Verdict
threats-identified

## Rationale

Inspected REQUIREMENTS.md, SPEC.md (FR1–FR9, NFR1–NFR6, Security
Considerations) and the mux daemon's reader pipeline, snapshot paths and
boundary record that the four tasks change. The tier is full, and there is
no DESIGN.md (design step skipped).

The feature handles bytes that any program running in a pane writes to its
PTY. Those bytes are untrusted: a remote host over SSH, a file being `cat`ed,
or a malicious tool can all write them. The daemon forwards replacement bytes
derived from them to a client terminal, and that terminal answers queries by
writing responses back into the pane's input and opens viewer windows for
viewer-launch sequences.

- **TB-1** (PTY output into the daemon and on to the client parser) is
  analyzed at deep depth, because every task declares `input-handling`.
- **TB-2** (per-destination delivery inside the daemon) is analyzed at
  standard depth; task0004 declares `concurrency` for it.
- No task declares `auth`, `external-io` or `data-persistence`. The ring is
  in-memory, and no new file, network or storage path is added.
- Client-to-daemon requests (snapshot request, reattach, attach) are
  unchanged, so they add no new threat.
- Information disclosure is not recorded. Color-query responses expose
  theme colors only, and this feature does not change what is answered, only
  whether a query the raw stream contained is answered once.

Domain consistency check after decomposition: every file of task0001,
task0002 and task0003 appears in the TB-1 Boundary files, apart from test
files and the decision table. task0004's files appear in TB-1 (the decider
and the builder rule) or TB-2 (the boundary record and snapshot paths).

## Trust Boundaries

### TB-1: Child-process PTY output → daemon reader pipeline → client terminal parser

Crossing: bytes written by the program in a pane (untrusted) enter the
daemon's PTY reader. The write filter and the scrollback strip shape what
reaches the ring and the snapshot. For a suppressed chunk, the replacement
builder derives the bytes delivered to the client terminal, whose parser
answers queries into the pane's input and opens viewer windows.
Boundary files: `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`, `src-tauri/src/mux/ipc/pty_spawn/client_parity_scan.rs`, `src-tauri/src/mux/ipc/pty_spawn/suppressed_output.rs`, `src-tauri/src/mux/ipc/pty_spawn/mod.rs`, `src-tauri/src/mux/scrollback_filter.rs`, `src-tauri/src/mux/osc_identify.rs`, `src-tauri/src/mux/snapshot_tail.rs`, `src-tauri/src/mux/mod.rs`
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | Crafted output makes the replacement contain a query or viewer launch at a position where the client parser never starts a control sequence. The client then injects a response into the pane's input, or opens a viewer, that the raw stream would not have caused. Routes: an ESC in a retention-window designator slot (FR1), an awaiting designator lost across reads (FR3), a string the client already closed at a removed screen switch and re-sent from pending (FR4), and a carried-over sequence re-detected and delivered twice (FR6). Relates to NFR4 and SPEC TM-1. | TM-1 | Every scan and filter decision follows the client's parser transitions. Undecidable restart positions count as misses. The write filter carries the awaiting-designator state across reads and closes held strings at every removed switch. Carried-over completions are deduplicated by their end position in the chunk. | task0002 AC-1, AC-2; task0003 AC-1, AC-2, AC-4, AC-6 | VERIFICATION.md Performance / Security Verification: TM-1 |
| Denial of service | Adversarial output makes a scan or decision non-linear or panic. Examples: designator chains, repeated switch sequences with incomplete introducers, strings near the 512 KiB cap, overflowing OSC numbers, snapshot payloads ending in long constructs. A panic kills the pane's reader thread; super-linear work stalls output; an empty replacement is read as PTY exit. Relates to NFR5 and SPEC TM-2. | TM-2 | Every added or changed scan and decision is a single forward pass bounded by its input, with no panicking arithmetic or indexing: OSC recovery, restart selection, the write filter's boundary scan including overflow, and the snapshot trailing-construct decider. The pending cap and its strip-filtered flush are unchanged. The decider caps reported constructs at 256 bytes. An empty replacement is never sent. | task0001 AC-1; task0002 AC-1; task0003 AC-6; task0004 AC-1 | VERIFICATION.md Performance / Security Verification: TM-2 |

### TB-2: Daemon → per-destination client output channels

Crossing: replacement bytes, and FR8's snapshot trailing construct, pass from
the reader thread to one specific client connection. Several clients (and
successive snapshots) can target the same pane concurrently.
Boundary files: `src-tauri/src/mux/session/pane/output_capture.rs`, `src-tauri/src/mux/session/pane/output_target.rs`, `src-tauri/src/mux/session/pane/output_queue.rs`, `src-tauri/src/mux/ipc/reattach.rs`, `src-tauri/src/mux/ipc/handlers/mod.rs`, `src-tauri/src/mux/ipc/handlers/attach.rs`, `src-tauri/src/mux/ipc/pty_spawn/mod.rs`
Depth: standard

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | A replacement decision uses snapshot information belonging to another destination, or to a different snapshot of the same destination. Examples: a record read from the current output target after a reattach by another client, or a construct from a later boundary. Bytes are then omitted from or injected into a client's stream, corrupting its display and parse state. Relates to NFR4 and SPEC TM-3. | TM-3 | The trailing construct is stored in the boundary record of the destination that received the snapshot, and is returned only for the chunk number equal to that boundary. It is read under the same boundary hold as the covered decision, and conflicting records collapse to "absent". The replacement goes only to the destination captured at the suppression decision. | task0004 AC-2, AC-6 | VERIFICATION.md Performance / Security Verification: TM-3 |
