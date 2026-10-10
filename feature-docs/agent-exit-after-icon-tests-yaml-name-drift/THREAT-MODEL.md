# Threat Model: agent-exit-after-icon-tests-yaml-name-drift

## Verdict
no-trust-boundary

## Rationale
Inspected SPEC.md (FR1-FR3, NFR1-NFR3) at the `reduced` tier; the only task,
task0001, declares the `input-handling` domain, which set the analysis to the
deeper reading of its inputs. The feature changes one line of a
repository-tracked test record (`test-docs/agent-exit-after-icon/task0006.tests.yaml`)
and adds one test-only function to `src-tauri/src/mux/upgrade/tests.rs`. That
test reads two repository-tracked files (the record and the test module's own
source) from paths fixed relative to the crate directory, and runs only inside
the test binary; nothing is compiled into the shipped binary. No user input,
network or external service, file from outside the project, other process,
LLM prompt, or privilege change is involved. task0001's `input-handling`
declaration reflects that the guard parses the record text; that text is
authored and versioned in the same repository, under the same trust as the
source code that reads it, so it is not a trust boundary and no `Boundary
files` line exists for it.
