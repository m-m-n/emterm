# Threat Model: restored-pane-read-flake-investigation

## Verdict
threats-identified

## Rationale
Inspected TASK.md alone: the tier is `minimal`, so SPEC.md, REQUIREMENTS.md
and DESIGN.md are subtracted and `workflow.yaml` carries no requirement IDs
to cite; threats cite TASK.md sections instead.

The task changes no tracked file under `src-tauri/` or `crates/`. It runs the
unchanged test under two conditions, may observe a temporary reproduction
program kept outside the repository, and records the findings in one new
git-tracked document, `INVESTIGATION.md`. Running the tests and the temporary
program exchanges data only between processes the developer starts on the
same machine under the same account, so neither is a trust boundary. The one
boundary is where text produced on that machine leaves it by being committed
into the repository document.

task0001 declares the domain `concurrency`, which is none of `auth`,
`input-handling`, `external-io` or `data-persistence`, so TB-1 is analysed at
standard depth and the post-decomposition consistency check has nothing to
reconcile.

## Trust Boundaries

### TB-1: Local execution output into the committed investigation document
Crossing: text produced on the developer's machine (cargo and test-binary
output, assert messages, the task worktree path, the location of the
temporary reproduction program) copied by the implementer into a git-tracked
document shared through the repository
Boundary files: feature-docs/restored-pane-read-flake-investigation/INVESTIGATION.md
Depth: standard

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Information disclosure | TASK.md `## Change` has the executed commands, conditions and failure-time assert messages recorded in the document; absolute paths under the home directory inside them (task worktree path, reproduction program location, pasted command output) expose the local account name and directory layout once committed | TM-1 | `INVESTIGATION.md` contains no absolute path under the home directory: paths inside the repository are written relative to the repository root, paths outside it under the home directory are written as `~/...`, and the same replacement is applied to command output before it is pasted | TASK.md | TASK.md Expected Result (TM-1 line) |
