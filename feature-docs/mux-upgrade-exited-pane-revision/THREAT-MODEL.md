# Threat Model: mux-upgrade-exited-pane-revision

## Verdict
no-applicable-threat

## Rationale

**Inspected**: SPEC.md (FR1–FR5, NFR1–NFR2; Security Considerations: none),
REQUIREMENTS.md, the snapshot / refresh / restore path in
`src-tauri/src/mux/upgrade.rs`, and its caller in
`src-tauri/src/mux/daemon/handoff.rs`. Tier: full. Task domains: concurrency
and data-persistence. data-persistence deepened the analysis of the handoff
document.

**Trust boundaries on the data path**: there are two. Both already exist, and
this feature changes neither.

1. **Program output inside a pane.** OSC 777 and OSC 7501 reports are parsed
   and validated upstream, in the pane reader thread and the daemon
   agent-status task, before they reach a pane's agent status. This feature
   does not touch that parsing or validation.
2. **The handoff file passed from the predecessor daemon to its successor
   across exec.**
   - It is written owner-only, with create-new, no-follow and atomic rename.
   - It is decoded with schema-version checks.
   - On restore, its program records are validated again and capped.

   This feature changes none of these steps, and it changes neither the
   schema nor the write, read or validation path (REQUIREMENTS.md 5.5, 9.1).

**Why no STRIDE category applies to the change**: the change only alters which
already-validated in-memory values the refresh writes for an exited pane:
agent state, agent name, revision and program records.

- The refresh already writes these same four fields, with the same encoding,
  for a live pane.
- The snapshot already writes them for an exited pane.

So no new kind of data, size, path, actor or privilege crosses either
boundary. The handoff file's exposure to tampering and information
disclosure, and the amount of data it carries, stay as they were.

**Domain consistency**: task0001 declares data-persistence, but its files
appear in no Boundary files line. That is because this short form has no
Trust Boundaries section. Boundary 2 above is where those files meet the
handoff document, and no threat applies there.
