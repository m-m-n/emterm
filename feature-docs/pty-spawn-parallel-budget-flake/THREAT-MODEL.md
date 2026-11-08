# Threat Model: pty-spawn-parallel-budget-flake

## Verdict
no-trust-boundary

## Rationale
Inspected: SPEC.md and REQUIREMENTS.md (FR1-FR6, NFR1-NFR4), the planned task
(task0001, domains: concurrency) at the full tier, and the files it changes:
`src-tauri/src/mux/ipc/pty_spawn/tests.rs`, three files under
`src-tauri/src/mux/ipc/pty_spawn/tests/` and one new guard module there. The change
is test code only. The budget tests feed fixed in-test byte inputs to the write
filter; the reattach tests synchronize the test thread with the reader thread of
the same test process; the guard reads the repository's own source text, embedded
at compile time. No user or external input, network or external service, file from
outside the project, other process, privilege change or LLM prompt is involved, and
none of the deep-analysis domains (auth, input-handling, external-io,
data-persistence) is declared, so no trust boundary is crossed.
