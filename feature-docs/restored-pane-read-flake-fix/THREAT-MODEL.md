# Threat Model: restored-pane-read-flake-fix

## Verdict
no-trust-boundary

## Rationale
Inspected: REQUIREMENTS.md, SPEC.md (FR1-FR5, NFR1-NFR4), the full-tier task
decomposition (task0001: test code in `src-tauri/src/mux/session/pane/tests.rs`,
domains `concurrency`; task0002: a comment in `src-tauri/src/mux/upgrade/tests.rs`,
no domains), and the target test's current body. The design step was skipped.
No task declares `auth`, `input-handling`, `external-io` or
`data-persistence`, so every boundary search ran at standard depth.

No trust boundary is crossed. The change is test-only (NFR1): no production
code path, API, or stored data changes. The bytes the tests read are the
test's own input, echoed back by the kernel line discipline of a PTY pair the
test opens in-process with no child process on the slave. The scripted
readers are in-process fixtures. Nothing comes from a user, an external
service, a file from outside the project, another process, or an LLM prompt,
and no privilege changes. The failure message shows only those
self-produced bytes, escaped. The helper reading thread runs in the same test
process and at the same trust level as the waiting side. No new dependency
is introduced (NFR2).
