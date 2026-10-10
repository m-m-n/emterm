# Threat Model: osc7501-leading-zero-length

## Verdict
threats-identified

## Rationale
Inspected: SPEC.md, REQUIREMENTS.md and the code paths the feature changes
(term_core's OSC string state and dispatch, the plain-tab OSC responder,
program_status, the mux feed scanner, osc_identify, the daemon and pane
OSC 7501 / OSC 133 handling, the mux query gates). Tier: full. Both tasks
declare `input-handling`, so both boundaries are analysed at deep depth;
task0002's `concurrency` domain does not change the depth.

The only input the feature handles is PTY output written by programs in a
tab or pane, local or over SSH, which is untrusted. It reaches two
boundaries: the plain-tab parser path and the mux daemon path. The feature
changes how sequences crossing both are measured and routed, so threats
exist.

`src-tauri/src/tabs/input.rs` (task0002) appears in no boundary: the
coalesce gate only decides whether a frame is parsed alone or with its
neighbours, and the answer to a query is produced either way. The test-docs
record (task0002) carries no input.

Predecessor mitigations are cited as "osc7501-program-status TM-n"; TM-n
without a prefix are this document's.

## Trust Boundaries

### TB-1: PTY output into the plain-tab parser and Program Status ingestion
Crossing: bytes written by any program running in a plain tab cross into
term_core's OSC string state, the host OSC responder, the plain-tab OSC 7501
record table, and the query answer written back to the PTY.
Boundary files: crates/term_core/src/parser/osc.rs, crates/term_core/src/parser/mod.rs, crates/term_core/src/osc_handler.rs, src-tauri/src/callbacks.rs, src-tauri/src/program_status.rs
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Denial of service | Leading zeros in the OSC number are not counted, so a report far over the whole-sequence limit (osc7501-program-status TM-1) is accepted — repro 1 accepts 4115 bytes as 19 (FR1, FR2, AC1); bytes past `MAX_OSC_LEN` are not counted at all | TM-1 | term_core counts the received OSC-string length at the receiving edge (leading zeros, the first `;`, bytes before U+FFFD replacement, bytes past `MAX_OSC_LEN`, saturating) and program_status applies the 4096-byte check to 2 + that count + terminator (SC-2, SC-3) | task0001 AC-1, AC-4, AC-5 | VERIFICATION.md Performance / Security Verification: TM-1 |
| Tampering | The OSC number add is not saturating: above the u16 range it wraps in release builds (and panics in debug builds), so `6553652` is dispatched as OSC 52 and `655367501` as OSC 7501 while osc_identify classifies the same bytes as no number (FR7, AC7) | TM-2 | Saturating multiply and add; an above-range number stays at 65535, which matches no native or registered code (A5) | task0001 AC-2 | VERIFICATION.md Performance / Security Verification: TM-2 |

### TB-2: PTY output into mux daemon ingestion and mux history stripping
Crossing: bytes written by any program running in a mux pane cross into the
daemon's per-pane reader thread (the feed scanner), the daemon's OSC 7501
record table and OSC 777 exit latch, the scrollback / snapshot strip and the
suppressed-output delivery scan.
Boundary files: src-tauri/src/mux/ipc/pty_spawn/write_filter.rs, src-tauri/src/mux/osc_identify.rs, src-tauri/src/mux/daemon/tasks.rs, src-tauri/src/mux/session/pane/mod.rs, src-tauri/src/mux/ipc/pty_spawn/client_parity_scan.rs, src-tauri/src/program_status.rs
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Denial of service | Once the daemon ingests leading-zero and non-digit-head spellings (FR3), measuring a report by its reconstructed body instead of its received bytes lets an over-limit report through on the mux path (FR1; osc7501-program-status TM-1) | TM-3 | The scanner hands the daemon the raw body length as the received OSC-string length; the daemon applies SC-3 to it; the 8 KiB carry bound (NFR3) is kept | task0002 AC-1, AC-2 | VERIFICATION.md Performance / Security Verification: TM-3 |
| Tampering | A scanner-local number accumulation that wraps or truncates would route an above-range or near-7501 number (`655367501`, `75010`) to OSC 7501 or OSC 133 handling while the strip verdict says otherwise (FR3, FR4, FR7) | TM-4 | The scanner takes the number only from osc_identify's SC-1 entry (above the u16 range is no number); no second accumulation exists | task0002 AC-1, AC-4 | VERIFICATION.md Performance / Security Verification: TM-4 |
