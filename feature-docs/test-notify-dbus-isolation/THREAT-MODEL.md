# Threat Model: test-notify-dbus-isolation

## Verdict
threats-identified

## Rationale
Inspected: SPEC.md and REQUIREMENTS.md (FR1-FR7, NFR1-NFR4) and the code they
change — `App::with_settings` in `src-tauri/src/app/mod.rs`, and
`NotifyRustSink`, its worker loops and its worker start in
`src-tauri/src/callbacks.rs`, plus the `worker_thread` tests in
`src-tauri/src/callbacks/tests.rs`. Tier: full. Domains that set the depth:
`external-io` (task0001, task0002) and `input-handling` (task0002), so both
boundaries below are analysed deep; `concurrency` and `api-contract` do not
change the depth.

Two trust boundaries exist. TB-1: notification text derived from terminal
output crosses into the OS notification service and into the application
log; this feature rewires the production code at that boundary (the sink
constructor, the worker start and the Windows worker body), so the existing
escape and redaction protections can regress. TB-2: the lib's test processes
reach the user's notification daemon — the defect this feature removes.
Spoofing, Repudiation and Elevation of privilege do not realistically apply:
the feature adds no identity, audit or privilege surface, persists no data
and parses no new input format.

## Trust Boundaries

### TB-1: Terminal-output-derived notification text → OS notification service and log
Crossing: title and body originating from PTY output (OSC 9 and the other
in-app notification producers; influenceable by whatever runs in the
terminal) pass through `NotifyRustSink`'s queue to its worker thread, which
hands them to notify-rust (the D-Bus notification daemon on Linux, the toast
API on Windows — another process) and writes dispatch records to the
application log.
Boundary files: src-tauri/src/callbacks.rs
Depth: deep (external-io, input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | Reworking the production construction (FR3) or the worker start lets markup in the title or body reach the unix notification daemon without passing the fail-closed escape gate, so the daemon renders terminal-chosen markup (NFR2 invariant) | TM-1 | Injected functions replace only the outermost capability query and send; `NotifyRustSink::new()` passes the notify-rust functions, and every notification still passes the unchanged unix worker order redact → on-demand capability gate → escape → send | task0002 AC-4 | VERIFICATION.md Performance / Security Verification: TM-1 |
| Information disclosure | Rewriting the Windows worker body (FR4) logs the raw notification text instead of the redacted rendering, copying terminal content into the persistent log (NFR2 log wording) | TM-2 | The Windows worker's success record carries only the redacted rendering computed from the received values, and its failure record carries only the error value | task0002 AC-5 | VERIFICATION.md Performance / Security Verification: TM-2 |

### TB-2: Test-build process → user's desktop notification daemon
Crossing: notifications emitted from `cargo test --lib` processes into the
user's session notification daemon (another process that keeps a history of
received notifications).
Boundary files: src-tauri/src/app/mod.rs, src-tauri/src/callbacks.rs, src-tauri/src/callbacks/tests.rs
Depth: deep (external-io)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Denial of service | App-constructing tests that do not replace the sink send real notifications through the production sink `App::new()` builds; the daemon's history grows, consuming its memory and burying the user's own notifications (FR1, NFR1) | TM-3 | The test-build `App::with_settings` stores a no-op sink, and `App::new()` inherits it | task0001 AC-1, AC-2 | VERIFICATION.md Performance / Security Verification: TM-3 |
| Denial of service | The `worker_thread` tests build `NotifyRustSink` through the production constructor and push their notifications (up to the queue capacity per burst) to the daemon (FR5, NFR1) | TM-4 | The `worker_thread` tests build the sink through the injection constructor with recording fakes on both OSes, and no lib test uses the production constructor | task0002 AC-1, AC-6 | VERIFICATION.md Performance / Security Verification: TM-4 |
