# Threat Model: inherited-pty-drop-fd-flake-investigation

## Verdict
threats-identified

## Rationale
Inspected: `TASK.md` (tier `minimal`; no SPEC.md, REQUIREMENTS.md or
DESIGN.md, and `requirements: {}` in `workflow.yaml`). The single task
(task0001) is investigation-only. It reads code, runs the unmodified target
test with a filter (at most 5 sequential runs), builds and runs a small C
program outside the repository under `/tmp`, and writes one committed
document, `INVESTIGATION.md`. No tracked file under `src-tauri/` or
`crates/` changes, so no runtime trust boundary of eMterm itself is added or
altered.

task0001 declares the domains `concurrency` and `external-io`;
`external-io` (descriptor and PTY device I/O, file-system side effects
outside the repository) sets TB-1 to deep depth.

The one boundary found is TB-1: data produced on the developer's host
crossing into a document committed to the repository. Information
disclosure realistically applies there. The other STRIDE categories were
considered and not recorded:

- Tampering / Elevation of privilege: the C program, its inputs and the
  test run are written and executed by the same account in one session; no
  party of different trust supplies input to them.
- Spoofing / Repudiation: nothing is authenticated and no action needs
  attribution.
- Denial of service: CPU load from repeated runs is an operational limit
  that `TASK.md`'s Expected Result already carries, not a threat from
  another party.

Domain consistency: task0001's `external-io` declaration covers
`INVESTIGATION.md`, which is TB-1's boundary file. Its other file, the
test-docs record, is not a boundary file: it is not where the task pastes
command output, and it falls under the same redaction rule
(`references/doc-redaction.md`, enforced by its guard hook on Write/Edit
into `test-docs/`).

TM-1 is already present as a line in `TASK.md`'s Expected Result (written
at create-spec); this pass reuses that line and appends nothing to
`TASK.md`.

## Trust Boundaries

### TB-1: Local investigation output into a committed repository document
Crossing: command output, measured values and source references produced on
the developer's host (cargo test output, C program output, `fstat` / `stat`
values, descriptor listings, file:line references into dependency sources
outside the repository) cross from the private host into a document
committed to the repository and shareable through its remote.
Boundary files: feature-docs/inherited-pty-drop-fd-flake-investigation/INVESTIGATION.md
Depth: deep (external-io)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Information disclosure | Absolute paths under the home directory, in pasted command output or in file:line references (dependency sources under `~/.cargo/registry`, the worktree path), reveal the account name and directory layout once committed. No requirement IDs exist at this tier. | TM-1 | `INVESTIGATION.md` contains no absolute path under the home directory: paths inside the repository are written relative to the repository root, paths under the home directory outside the repository as `~/...`, and the same replacement is applied to pasted command output before it is written | TASK.md | TASK.md Expected Result (TM-1 line) |
