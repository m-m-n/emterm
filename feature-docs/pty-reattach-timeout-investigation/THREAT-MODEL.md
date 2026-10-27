# Threat Model: pty-reattach-timeout-investigation

## Verdict
threats-identified

## Rationale
Inspected: TASK.md, the only planning input at the `minimal` tier (no SPEC.md,
REQUIREMENTS.md or DESIGN.md; `requirements` is empty), and the deliverable of
its single task. The task modifies no tracked file under `src-tauri/` or
`crates/`. It runs the existing, unmodified test repeatedly on the local
machine (alone, inside the full `--lib` run, and under concurrent CPU load) and
writes the findings to `INVESTIGATION.md`, which is committed.

The one crossing between parties of different trust is local command output
(test runner and build output, the executed command lines, details of the
concurrent load processes) that TASK.md's Change section requires to be
recorded in that committed document: TB-1.

Not modeled as boundaries:
- The PTY child processes and mux code exercised by the test. They are
  existing code that this task runs unchanged and does not alter.
- The load processes the investigation starts. They are local and
  self-started, and TASK.md already requires managing them through the Bash
  tool's timeout parameter or run_in_background and TaskStop. Resource
  exhaustion from them is an operational risk with no adversary crossing a
  boundary.

task0001 declares the `concurrency` domain only. It declares none of `auth`,
`input-handling`, `external-io` or `data-persistence`, so TB-1 is analysed at
standard depth. The post-decomposition consistency check finds no boundary
domain without a boundary.

## Trust Boundaries

### TB-1: Local command output into the committed investigation document
Crossing: output of commands run on the developer's machine (cargo test and
build output, executed command lines, process details of the concurrent load)
flows into `INVESTIGATION.md`, which is committed and shared beyond the local
machine through the repository.
Boundary files: feature-docs/pty-reattach-timeout-investigation/INVESTIGATION.md
Depth: standard

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Information disclosure | Raw output pasted as is carries absolute paths under the user's home directory, exposing the local account name and directory layout in a committed file. Sources: cargo prints a path crate's absolute source directory while compiling, the integration worktree sits under the home directory, and process listings show absolute command paths. Recording the executed commands, conditions and load details is part of TASK.md's Change section, so this copying is part of the task's normal flow. Text written through a shell redirect also bypasses the doc-redaction guard, which checks only Write / Edit. | TM-1 | INVESTIGATION.md writes paths inside the repository relative to the repository root and paths outside it as `~/...`. The same replacement is applied to any pasted command output before it is written, so the document contains no absolute path under the home directory. | TASK.md (Expected Result, TM-1 line) | TASK.md Expected Result (TM-1 line) |
