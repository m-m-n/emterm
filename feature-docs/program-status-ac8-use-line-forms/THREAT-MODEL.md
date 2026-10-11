# Threat Model: program-status-ac8-use-line-forms

## Verdict
no-trust-boundary

## Rationale
Inspected: SPEC.md FR1-FR6 and NFR1-NFR2 at the reduced tier (no
REQUIREMENTS.md, no DESIGN.md), and the single file of task0001,
`src-tauri/src/program_status/tests.rs`. The change is confined to test-only
checker code that is compiled only for the test build. The text the checker
parses is synthetic source literals written inside the test module and the
repository's own `program_status.rs`, both authored and reviewed under the same
trust as the checker itself. Nothing reaches it from a user, a network service,
a file outside the project, another process or an LLM prompt, and no privilege
changes. task0001 declares the `input-handling` domain because its work is a
text scanner; that scanning crosses no trust boundary, so its file appears in
no `Boundary files` line. A scanner defect would weaken the checker's guard of
the `gui` feature invariant; that is a correctness concern pinned by task0001's
Acceptance Criteria, not a threat across a boundary.
