# Threat Model: osc133-mark-token-exact-match

## Verdict
no-applicable-threat

## Rationale
Inspected: SPEC.md (FR1-FR3, NFR1) at the reduced tier, and the one input the
feature handles — OSC 133 payloads in a plain tab's PTY output, written by any
program running in the tab (locally or over SSH) and delivered by term_core to
`NativeCallbacks::handle_semantic_prompt`. That program-output-to-terminal-state
crossing is a trust boundary. task0001 declares `input-handling`, so the
boundary was analysed at deep depth; task0001's files
(`src-tauri/src/callbacks.rs`, `src-tauri/src/callbacks/tests.rs`,
`src-tauri/src/tabs/tests/output_pipeline.rs`) are where it is handled. No
STRIDE category realistically applies to the change:

- Spoofing / Tampering: a program that can emit OSC 133 into the tab can also
  emit OSC 7501 reports and OSC 777 agent-status Set/Clear into the same tab
  directly, so mis-reconciled marks give it no control over the record table or
  the latch that it lacks. The change only narrows which payloads produce
  candidates (FR1, FR2) and keeps well-formed handling (FR3).
- Denial of service: the decision reads the first segment of a payload term_core
  has already bounded and delivered; every payload still yields at most one
  candidate per feed, with no new allocation, retention or loop.
- Information disclosure, Repudiation, Elevation of privilege: nothing leaves the
  process, and no identity or privilege is involved.
- term_core's parsing and delivery are unchanged (NFR1).
