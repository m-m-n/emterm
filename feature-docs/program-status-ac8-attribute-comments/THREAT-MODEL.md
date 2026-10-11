# Threat Model: program-status-ac8-attribute-comments

## Verdict
no-trust-boundary

## Rationale
Inspected SPEC.md (FR1-FR5, NFR1-NFR3) at the reduced tier and the single task task0001,
whose `input-handling` domain set the boundary search to deep. The change is confined to
the ac8 allowlist checker in `src-tauri/src/program_status/tests.rs`, a module compiled
only for tests and never part of a built binary. The text it parses is
`src-tauri/src/program_status.rs`, embedded from the repository at compile time, and
string literals written in the same test module. No user input, external service, file
from outside the project, other process, LLM prompt or privilege change is involved, so
no data crosses a trust boundary. task0001's file appears in no `Boundary files` line for
that reason: the parser's input is project-owned source, not external input.
