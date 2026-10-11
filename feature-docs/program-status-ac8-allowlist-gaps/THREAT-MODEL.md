# Threat Model: program-status-ac8-allowlist-gaps

## Verdict
no-trust-boundary

## Rationale
Inspected SPEC.md (FR1-FR6, NFR1-NFR4) at the reduced tier, and the single
task task0001, which modifies only `src-tauri/src/program_status/tests.rs`.
The change is confined to test-only helpers of the ac8 allowlist checker and
their self-tests; it runs only inside the libtest harness and is never part
of a shipped binary. The checker's inputs are `src-tauri/src/program_status.rs`,
embedded at compile time from the same repository, and string literals
written in the same test file. Both carry the same trust as the code under
test. No user input, external service, file from outside the project, other
process, LLM prompt or privilege change is involved, so no trust boundary is
crossed.

task0001 declares the `input-handling` domain because the checker parses
source text. That domain deepened the boundary search; its file appears in
no Boundary files line because the parsed text is repository-internal, as
stated above.
