# Threat Model: osc52-clipboard-write-drain

## Verdict
threats-identified

## Rationale
Inspected: SPEC.md, REQUIREMENTS.md, and the code paths the feature connects. These are `NativeCallbacks::handle_clipboard` and its queues (`src-tauri/src/callbacks.rs`), the per-tab parse paths and the replay recipe (`src-tauri/src/tabs/`), `App::pump_all` (`src-tauri/src/app/mod.rs`), the `WindowHost` arboard primitives (`src-tauri/src/window_host/mod.rs`), and the daemon snapshot strip (`src-tauri/src/mux/scrollback_filter.rs`), which removes no OSC 52 today. Tier: full. Design step skipped.

The feature connects untrusted terminal output to the user's system clipboard, a resource shared with every other local application. That is a trust boundary (TB-1). Mux snapshot replay re-feeds historic pane output from another process, the mux daemon (local or remote), so it is a second boundary (TB-2). task0001 declares `input-handling` and `external-io`, and task0002 declares `input-handling`; both boundaries are analysed at deep depth.

Residual, accepted: any program allowed to print to a terminal tab, including a remote host over SSH or a file printed with `cat`, can replace the clipboard content. Enabling that is the feature's purpose (SPEC Objectives). SPEC asks for no setting to turn it off, so the model invents no mitigation beyond the gates recorded below. Output of non-active mux panes is dropped by existing pane routing and is out of scope.

## Trust Boundaries

### TB-1: Terminal output to the system clipboard (OSC 52)
Crossing: OSC 52 sequences in PTY output, written by any local or remote program, cross into the user's system clipboard (CLIPBOARD / PRIMARY). They pass through the callback queues, the per-pump drain, the App batch and the `WindowHost` arboard instance on the UI thread.
Boundary files: `src-tauri/src/callbacks.rs`, `src-tauri/src/tabs/mod.rs`, `src-tauri/src/app/mod.rs`, `src-tauri/src/window_host/osc52_clipboard.rs`, `src-tauri/src/window_host/mod.rs`, `src-tauri/src/window_host/event_loop.rs`
Depth: deep (input-handling, external-io)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | Untrusted output writes malformed, oversized or non-text content into the system clipboard by reaching it outside the existing acceptance gates (FR1, FR9) | TM-1 | Only entries `handle_clipboard` queued after its unchanged gates (valid base64, decoded size within `clipboard_max_size_osc52`, UTF-8) reach the clipboard. Nothing else feeds the batch. | task0001 AC-7 | VERIFICATION.md Performance / Security Verification TM-1 (TS-6) |
| Information disclosure | An OSC 52 query lets the emitting program read the user's clipboard content back through the PTY (FR4) | TM-2 | Queued reads are drained and discarded every pump. The clipboard is never read for them, and no response byte is written to the PTY. | task0001 AC-4 | VERIFICATION.md Performance / Security Verification TM-2 (TS-2) |
| Information disclosure | Clipboard text, which may be a secret the remote program copied, is persisted in `emterm.log` through a failure log line (NFR3) | TM-3 | Failure log lines are built from destination, text length and error description only. They never hold the decoded text or the base64 payload. | task0001 AC-3 | VERIFICATION.md Performance / Security Verification TM-3 (TS-2, NFR3 code review check) |
| Denial of service | A burst of OSC 52 writes grows the queues without bound across pumps, or makes one OS clipboard call per entry on the UI thread and stalls the terminal (NFR1, NFR2) | TM-4 | Both queues are emptied on every tab every pump. Each pump performs at most one clipboard operation per destination, the one the last entry addressed to it asked for. | task0001 AC-2, AC-4 | VERIFICATION.md Performance / Security Verification TM-4 (TS-2) |

### TB-2: Mux daemon snapshot replay to the tab's callbacks
Crossing: Snapshot / SnapshotRestore content, the historic raw output of a pane stored by the mux daemon (another process, possibly on a remote host), is replayed into the tab's live core, whose callbacks are the tab's `NativeCallbacks`. The off-thread worker's core has no callbacks until the swap.
Boundary files: `src-tauri/src/tabs/replay.rs`, `src-tauri/src/tabs/mux_link.rs`
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | A historic OSC 52 write or clear inside snapshot content overwrites or clears the user's current clipboard on a window switch or reattach (FR7) | TM-5 | The synchronous replay recipe leaves the OSC 52 queues exactly as they were before the replay. Off-thread snapshot content cannot reach the callbacks. Live output applied after a replay keeps its entries. | task0002 AC-1, AC-3, AC-4 | VERIFICATION.md Performance / Security Verification TM-5 (TS-5) |
