# Threat Model: program-status-ac8-block-comment-carry-test

## Verdict
no-trust-boundary

## Rationale
Inspected at the reduced tier: SPEC.md (FR1-FR4, NFR1-NFR3) and the single
task's file set. `src-tauri/src/program_status/tests.rs` gains unit tests that
feed fixed source text written in the tests themselves to the existing
test-only allowlist checker; the checker is compiled only into the test build
and its real scan target is the project's own module source, embedded at
compile time. `test-docs/program-status-ac8-attribute-comments/task0001.tests.yaml`
is a project-internal test record whose AC-3 entry is edited. The change adds
no runtime code path and reads no user input, external service, file from
outside the project or other-process data; it builds no LLM prompt and changes
no privilege, so no data or control crosses between parties of different
trust. task0001 declares none of the depth-setting domains (auth,
input-handling, external-io, data-persistence), so the post-decomposition
consistency check has nothing to reconcile.
