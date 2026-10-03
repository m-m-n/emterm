# Threat Model: mux-write-filter-overflow-lone-esc

## Verdict
threats-identified

## Rationale
Inspected: SPEC.md and REQUIREMENTS.md (tier: full; the design step was skipped, so there is no DESIGN.md), the overflow branch of the scrollback write filter's cut-aware feed (`src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`), the strip pass and its written-state model (`src-tauri/src/mux/scrollback_filter.rs`), the reader loop that feeds the filter and writes the ring (`src-tauri/src/mux/ipc/pty_spawn/mod.rs`), and the suppressed-chunk replacement builder that reads the filter's held bytes.

The feature changes how bytes produced by a pane's child process are written into the scrollback ring, which the daemon later replays into a client terminal's parser. That is one trust boundary (TB-1). The single task declares `input-handling`, so TB-1 is analysed at deep depth.

Two STRIDE categories realistically apply at TB-1: Tampering (the ring stores, in executable form, a construct the client did not initiate) and Denial of service (the change sits on the pending cap that bounds per-pane memory against a hostile stream). No identity, audit record or secret crosses this boundary, and the change adds no capability beyond what the strip already removes, so Spoofing, Repudiation, Information disclosure and Elevation of privilege get no row.

## Trust Boundaries

### TB-1: Child-process PTY output into the scrollback ring
Crossing: bytes a pane's child process writes to its PTY (attacker-influenceable: contents of a printed file, output relayed from a remote host) cross from the child process into the mux daemon's per-pane scrollback ring, which the daemon replays into a client terminal parser on reattach and window switch.
Boundary files: src-tauri/src/mux/ipc/pty_spawn/write_filter.rs, src-tauri/src/mux/scrollback_filter.rs, src-tauri/src/mux/ipc/pty_spawn/mod.rs
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | Output grows a pending run past the 512 KiB cap so that the overflowing read ends in a live lone ESC, and sends the rest of a strip target (`[6n` and the other CSI device queries, an OSC 777 viewer launch, OSC 9999 emterm-md, an OSC 777 agent-status report, a Kitty APC, a SIXEL DCS) in the next read. The next read's strip sees the target without its opening ESC and writes it verbatim, so the ring holds the target in executable form and a replay answers a query, or repeats a launch, the client did not initiate (FR1, NFR2; SPEC.md Security Considerations, Input Validation). | TM-1 | The overflow flush in a call's last segment does not write a final live lone ESC; it holds that one byte in pending, so the next read strips it together with its continuation exactly as the non-overflow path does. A cut or the reader's fallback closing drops the held ESC unwritten and writes only the closure the written state before it decides (FR1, FR4). | task0001 AC-1, AC-2, AC-5, AC-7 | VERIFICATION.md Performance / Security Verification: TM-1 (TS-1, TS-2, TS-4, TS-6) |
| Denial of service | A hostile stream can force the overflow path on every read (any run past the cap). A hold that kept more than that one ESC, or a second walk over the flushed run, would weaken the cap's memory bound or add per-read CPU cost the stream controls (FR2, NFR1). | TM-2 | Pending after an overflow flush holds at most the one held ESC byte. The flushed run is stripped in a single pass (over the run without its final byte when that byte is ESC), and the extra work is O(1): the run's last byte and the state the strip reports (FR2, NFR1). | task0001 AC-3, AC-8 | VERIFICATION.md Performance / Security Verification: TM-2 (TS-3, TS-7) |
