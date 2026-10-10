# Threat Model: pane-state-rank-unify

## Verdict
no-trust-boundary

## Rationale
Inspected SPEC.md (FR1-FR5, NFR1-NFR3) at the reduced tier, and the one task
(task0001, domains: api-contract — none of auth / input-handling /
external-io / data-persistence, so standard depth). The change set is
`src-tauri/src/agent_status.rs` (doc comment only),
`src-tauri/src/program_status.rs` (the ProgramState rank derivation and a new
ProgramState -> AgentState correspondence) and
`src-tauri/src/program_status/tests.rs` (new tests). The changed functions
take and return only closed in-process enum values and small integers. The
places where untrusted terminal output becomes these enums — the OSC 7501
report parser in `program_status.rs` and the OSC 777 parser in
`agent_status.rs` — are not changed, and NFR1 keeps every output of
`Table::summary`, `agent_status::compose` and `composite_name` identical. The
feature adds no input, output, file, network, process or privilege crossing,
so no trust boundary exists within its scope.
