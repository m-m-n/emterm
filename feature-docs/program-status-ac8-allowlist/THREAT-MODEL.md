# Threat Model: program-status-ac8-allowlist

## Verdict
no-trust-boundary

## Rationale
Inspected: SPEC.md (FR1 to FR7, NFR1 to NFR3) at tier `reduced`, and the
single task task0001, whose only file is `src-tauri/src/program_status/tests.rs`.
The change is unit-test code compiled only into the test build. The checker
reads two inputs, both authored inside this repository: the text of
`src-tauri/src/program_status.rs` embedded at compile time, and string
literals written in tests.rs. Its only output is a test pass/fail result with
a failure message on the test runner. No user input, external service, file
from outside the project, other process, LLM prompt or privilege change is
crossed, and nothing reaches the shipped GUI or CLI binaries. task0001
declares the domain `config-infra` only; none of `auth`, `input-handling`,
`external-io` or `data-persistence` applies, so no boundary needed deeper
analysis.
