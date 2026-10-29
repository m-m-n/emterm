# Threat Model: restored-pane-echo-deadline-start-delay

## Verdict
no-trust-boundary

## Rationale
Inspected: SPEC.md (FR1-FR3, NFR1-NFR4) at the reduced tier, and the single
task task0001 (domains: concurrency). The change is test-only code in the
`restored_echo_wait` module of `src-tauri/src/mux/session/pane/tests.rs`
(NFR2); production code is unchanged. The data it handles are scripted
in-memory byte sequences defined by the tests themselves, passed between the
test thread and a helper reader thread of the same process, and the new start
gate is a flag shared by those two threads. No user or external input,
external service, file from outside the project, other process, untrusted
prompt text or privilege change is involved, so no trust boundary exists.
task0001 declares none of auth, input-handling, external-io or
data-persistence, so no depth adjustment applies and no boundary-consistency
gap exists.
