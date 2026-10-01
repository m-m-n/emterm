# Threat Model: mux-suppressed-output-round3-fixes

## Verdict
threats-identified

## Rationale

Inspected REQUIREMENTS.md, SPEC.md (FR1–FR9, NFR1–NFR6, Security
Considerations) and the mux daemon code the five tasks change: the write
filter, the shared scrollback strip, OSC identification, the reader's
capture step and suppressed pipeline, the client-parity scan, replacement
assembly and the per-destination boundary record. The tier is full. There
is no DESIGN.md (design step skipped).

The feature handles bytes that any program running in a pane writes to its
PTY. Those bytes are untrusted: a remote host over SSH, a file being
printed, or a malicious tool can write them. The daemon writes a filtered
form of them to the scrollback ring, replays the ring to clients in
snapshots, and for a suppressed chunk sends replacement bytes derived from
them. The client terminal answers queries by writing responses into the
pane's input, and opens viewer windows for viewer-launch sequences.

- **TB-1** (PTY output into the daemon and on to the client parser) is
  analyzed at deep depth, because tasks task0001–task0004 declare
  `input-handling` for it.
- **TB-2** (per-destination delivery inside the daemon) is analyzed at
  standard depth; task0004 declares `concurrency` for it.
- No task declares `auth`, `external-io` or `data-persistence`. The ring is
  in memory, and no file, network or storage path is added.
- Client-to-daemon requests (snapshot request, reattach, attach) are
  unchanged and add no threat.
- Information disclosure is not recorded. A color-query response exposes
  theme colors only. This feature changes whether a response is fabricated,
  not what a response contains.

Domain consistency check after decomposition: every non-test file of
task0001, task0002 and task0003 appears in the TB-1 boundary files.
task0004's non-test files appear in TB-1 (replacement assembly, the reader
pipeline) or TB-2 (the boundary record). The remaining files of those tasks
are tests and a predecessor test record, which are not boundary code.
task0005 declares no domain.

## Trust Boundaries

### TB-1: Child-process PTY output → daemon ring, snapshot and replacement → client terminal parser
Crossing: bytes written by the program in a pane (untrusted) enter the
daemon's PTY reader. The write filter and the shared strip decide what
reaches the ring, and so what a later snapshot replays into the client
parser. For a suppressed chunk, the client-parity scan and replacement
assembly derive the bytes delivered to the client, whose parser answers
queries into the pane's input and opens viewer windows.
Boundary files: `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`, `src-tauri/src/mux/scrollback_filter.rs`, `src-tauri/src/mux/osc_identify.rs`, `src-tauri/src/mux/ipc/pty_spawn/mod.rs`, `src-tauri/src/mux/ipc/pty_spawn/client_parity_scan.rs`, `src-tauri/src/mux/ipc/pty_spawn/suppressed_output.rs`
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | Crafted output makes the client answer a query, or open a viewer, at a position where the raw stream's parser never starts a control sequence. The response is then injected into the pane's input. Routes: an OSC or lone ESC closed by a removed screen switch is written unterminated to the ring, so a snapshot replay leaves the client inside it and a later BEL completes it (FR1). A designator ESC is taken as an introducer by the strip, so output changes with the split position and bytes are removed or kept wrongly (FR2, FR3). A switch that straddles reads leaves a held construct open, so a later read completes a query that never existed (FR4). The as-05 fallback scans from a chunk byte the client consumed as a designator (FR5). Relates to NFR4 and SPEC TM-1. | TM-1 | Every filter, strip and scan decision follows the client parser's transitions. A construct closed at a cut, or by the fallback closing, is dropped from the ring, not terminated. The strip and the write filter treat the byte after `ESC (` / `ESC )` as a designator, with the state carried across calls. The fallback closes what the filter holds whenever the alternate screen is involved. An undecidable trailing designator slot in a full window counts as a miss. | task0002 AC-1, AC-3, AC-4, AC-5; task0003 AC-1, AC-3 | VERIFICATION.md Performance / Security Verification: TM-1 |
| Denial of service | Adversarial output makes a strip, filter, scan or identification pass non-linear or panic. Examples: long designator chains, switch sequences straddling many reads, a held OSC near the 512 KiB cap, a one-million-digit OSC number, a 1 MiB OSC body. A panic kills the pane's reader thread, super-linear work stalls output, and an empty replacement chunk is read as PTY exit. Relates to NFR5 and SPEC TM-2. | TM-2 | Every added or changed pass stays a single forward pass bounded by its input, with no panicking arithmetic or indexing: the designator-aware strip, the write filter's cut, drain and overflow paths, the fallback closing, the as-05 rule, and the allocation-free identification (which stops at u16 overflow). The pending cap and its strip-filtered flush are unchanged. An empty final replacement is never sent. | task0001 AC-4; task0002 AC-6; task0003 AC-4; task0004 AC-3 | VERIFICATION.md Performance / Security Verification: TM-2 |

### TB-2: Daemon → per-destination client output channels
Crossing: replacement bytes pass from the reader thread to one specific
client connection. Their tail-omission decision depends on the snapshot
recorded for that destination. Several snapshots, and several clients, can
target the same pane concurrently with the reader.
Boundary files: `src-tauri/src/mux/session/pane/output_capture.rs`, `src-tauri/src/mux/ipc/pty_spawn/mod.rs`, `src-tauri/src/mux/ipc/pty_spawn/suppressed_output.rs`
Depth: standard

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | The tail-omission decision uses snapshot information that is stale or belongs to another destination. A second snapshot recorded between the covered decision and the send, or two records at an equal boundary collapsing to none, makes the replacement re-send a tail the client already holds or omit one it needs. The client then shows a replacement character or a stray `(`, or consumes the next chunk's first byte wrongly (FR6, FR7). Relates to NFR4 and SPEC TM-3. | TM-3 | Each destination's record carries a generation. The reader re-checks that destination's record under `output_target` and the boundary exclusion after securing its send slot, and sends under the same hold. The construct applies only when the chunk number equals the recorded boundary. At an equal boundary the later record wins. The record is read only for the destination captured at the suppression decision, and the replacement is sent only there. | task0004 AC-1, AC-4 | VERIFICATION.md Performance / Security Verification: TM-3 |
