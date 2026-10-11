# Threat Model: agent-latch-alt-screen-prompt-mark

## Verdict
no-applicable-threat

## Rationale
Inspected: SPEC.md, REQUIREMENTS.md and the plain-tab latch path the feature
changes (the OSC 133 branch in `src-tauri/src/callbacks.rs`,
`reconcile_latch_feed` in `src-tauri/src/agent_status_model.rs`, the latch
reconciliation in `src-tauri/src/tabs/output_pipeline.rs`). Tier: full.
task0001 declares `input-handling`, so the boundary below was analysed at deep
depth.

One trust boundary exists: bytes written to the PTY by programs running in the
tab (another process, possibly remote over SSH) cross into emterm's parser and
native callbacks, carrying OSC 133 marks, alternate-screen mode switches and
OSC 777 agent-status reports. No STRIDE category realistically applies to this
change at that boundary:

- Spoofing / Tampering: the inferred clear changes only the tab's
  self-reported agent-status display. The party that emits OSC 133 marks can
  already set or clear that display directly with OSC 777 (kept by FR3), so
  steering the latch through marks grants no capability it lacks. The change
  removes alternate-screen marks from the latch path (FR1) and adds none.
- Denial of service: the latch feed is drained every pump and holds at most
  one entry per OSC in that pump's bytes; the change only removes entries, and
  the reconciliation stays a single pass (NFR1).
- Information disclosure, Repudiation, Elevation of privilege: nothing leaves
  the process, no actor attribution is involved, and no privilege changes.

Domain consistency: task0001 declares `input-handling`; its files appear in no
`Boundary files` line because the short form carries no Trust Boundaries
section. The boundary above was analysed and has no applicable threat.
