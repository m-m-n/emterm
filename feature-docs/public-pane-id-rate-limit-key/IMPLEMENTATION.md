# Implementation Plan: public-pane-id-rate-limit-key

## Overview

Namespace the daemon-learned `public_pane_id` inside the agent-notification
rate-limit key derivation so the three produced key forms are mutually
disjoint by construction, and correct the two doc comments that assert a
collision property the code does not have. No wire format, no persisted
state and no user-visible identifier changes.

## Technology Stack

- **Language**: Rust — the existing `src-tauri` crate. No new language, no new
  framework, no new build step.
- **Test harness**: the project's existing Rust unit/integration test setup,
  invoked through the commands in `workflow.yaml` `project.components`.
- **New dependencies**: none. This feature adds no crate, no vendored code and
  no tooling, so no dependency license is introduced. `project.license`
  remains MIT and is unaffected.

## Layer Structure

Three layers participate; the arrows are the only permitted dependency
directions.

| Layer | Responsibility | Changed by this feature |
|-------|----------------|-------------------------|
| Ingest | Applies an agent-status batch and records the daemon-supplied public pane id verbatim into the learned-id map | No |
| Derivation | The single producer of a notification rate-limit key from the learned-id map plus a pane key | Yes — the learned-id branch only |
| Consumers | Arm and discard sites that hold a rate-limit window keyed by the derived string | No |

Direction: consumers depend on derivation; derivation reads the learned-id
map; ingest writes that map. No consumer may reach around the derivation
layer and build a key itself, and no derivation-layer change may reach back
into ingest (that is what keeps this feature confined to an internal derived
value).

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Rate-limit key derivation — `agent_notification_rate_limit_key` (`src-tauri/src/app/agent_status.rs`) | The ONLY producer of a notification rate-limit key | **Pre**: the caller supplies the learned-id map and a pane key; for a discard site, the pane's learned-id entry has NOT yet been removed. **Post**: returns exactly one of three mutually disjoint forms — plain tab, unlearned mux pane, learned mux pane (see Key namespace convention below). Total: every input reaches one of the three forms, there is no failure path and no panic, including for an empty learned string. Deterministic: identical inputs yield an identical key. Pure: the map is not mutated. | task0001 |
| Learned-public-id map — `mux_public_pane_ids` (`src-tauri/src/app`) | Holds the daemon-supplied public pane id per connection scope and pane | **Pre**: a value is inserted exactly as received from the daemon. **Post**: the stored value is byte-identical to what the daemon sent; it is never parsed, validated, escaped or truncated on the way in or out. | task0001 |
| Public-id accessor — `App::mux_public_pane_id` | Exposes the learned string to readers (mux sidebar and other UI surfaces) | **Post**: returns the stored string unchanged for every pane, including ids that would fail the mux protocol's own public-pane-id parse. | task0001 |
| Rate limiter — `App::agent_notification_rate_limiter` (`src-tauri/src/app/mod.rs`) | Process-local, ephemeral per-key notification window store | **Pre**: every key it is given comes from the derivation component above. **Post**: arming and discarding a pane use the same derived key, so a discard reopens that pane's window and never another pane's. Rebuilt from scratch each run — nothing is persisted, so a key-format change needs no migration. | task0001 |

## Conventions

### Key namespace convention (the core invariant)

Every produced rate-limit key begins with a code-owned literal prefix ending
in a colon, followed only by code-owned values, before any externally
supplied bytes can appear:

| Form | Shape | Origin of every byte before the first daemon byte |
|------|-------|---------------------------------------------------|
| Plain tab | `tab:` + the tab's stable id | entirely code-owned |
| Mux pane, no learned id | `mux:` + connection scope number + `:` + pane id | entirely code-owned |
| Mux pane, learned id | `muxpub:` + connection scope number + `:` + the learned string verbatim | prefix and scope number are code-owned; daemon bytes start only after the second colon |

Rules that follow from it, and that any later change must keep:

- `tab:`, `mux:` and `muxpub:` are reserved prefixes. A fourth form added
  later must be distinguishable by its own literal prefix, chosen so that no
  prefix is a prefix-plus-separator of another.
- Disjointness is established by the prefix, never by inspecting the daemon
  string. The daemon string is therefore embedded as-is: not parsed, not
  validated, not escaped, not truncated.
- A rate-limit key is compared only for equality. It is never parsed back
  into its parts, so ambiguity inside the daemon-supplied suffix is harmless.

### Error handling policy

The derivation has no failure mode and introduces no rejection path, no error
code and no user-facing message. An empty learned string is still a learned
string and produces the learned form.

### Logging policy

The derived key is never logged as an identifier a daemon could use to probe
other tabs, and is never rendered in any UI surface.

### Documentation policy

Any comment inside the touched region must describe the behaviour that exists
after the change. A comment asserting a collision property the code does not
implement is treated as a defect, not as stale prose.

## Cross-task Design Decisions

### CD-1: The fix lives in the derivation, not at ingest

The learned string keeps flowing to every reader unchanged; only the derived
key is namespaced. Rationale: the public id is a display and diagnostic value
whose readers (notably the mux sidebar) must keep observing exactly today's
value, and rejecting unparseable ids at ingest would change that surface and
invalidate existing fixtures. Affected: task0001 (constrains it to leave the
ingest path and the accessor untouched).

### CD-2: One derivation point, and its ordering obligation

All four call sites — the closed-mux-pane loop and the transition-drain loop
in the agent-status module, the reaped-exited-tab loop in the app module, and
the tab-close path in the tab-lifecycle module — obtain the key by calling the
shared derivation. No call site constructs the string itself, which is what
guarantees the arm site and the discard site can never disagree. The two sites
that remove a pane's learned-id entry MUST derive the key BEFORE the removal;
deriving after it falls through to the unlearned form and discards the wrong
bucket. This ordering is pre-existing and is preserved, not introduced.
Affected: task0001.

### CD-3: The scope component sits before the daemon bytes

The connection scope's numeric value is placed between the prefix and the
learned string rather than after it. Rationale: two connections that learn
byte-identical public ids must still derive different keys, and the separating
colon must be code-owned so a daemon cannot shift the boundary. Affected:
task0001.

### CD-4: No new dependency, no license impact

The change is expressible with what the crate already has. No library is
added, so there is nothing to check against `project.license` (MIT) and
nothing to record beyond this statement. Affected: task0001.

### CD-5: The change is rated `high` complexity despite its size

The edit is small, but it sits on a trust boundary: it is the point where
bytes from an untrusted external process (the mux daemon, commonly reached
over SSH) enter a key space shared by every tab and every connection. The
complexity rating drives the review floor, so it is set by the boundary the
change touches rather than by its diff size. Affected: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| A future call site builds a key inline instead of calling the shared derivation, letting arm and discard disagree | Low | High — a pane's window is never reopened, or another pane's is | CD-2 pins the single-derivation-point rule; the arm/discard agreement is covered end-to-end by a test that hard-codes no key string |
| A later edit reorders a discard site so the learned-id entry is removed before derivation | Low | High — the wrong bucket is discarded | The ordering obligation is stated in CD-2 and already recorded by comments at both affected call sites; the same end-to-end test detects it |
| Tests pin literal key strings that depend on allocation order (stable ids) and become flaky | Medium | Low — false failures in the end-to-end scenario | Expected strings in the end-to-end scenario are derived from the observed ids at runtime rather than hard-coded |
| The doc-comment correction is skipped or half-applied, leaving a comment that still claims the old collision property | Medium | Medium — the next reader is misled about which branch is protected | Both comments are acceptance criteria of task0001 and are verified in review (no doc-drift test covers them) |
| A daemon evades its OWN rate limit by minting a fresh public id per update | Medium | Low — not a regression; a compromised daemon can already spam its own panes | Explicitly out of scope; recorded here so a reviewer does not read the feature as closing it |

## Open Questions

- [ ] NFR1 (internal-only key: never serialized, persisted or displayed) has
      no automated verification scenario. It is an absence property, checked
      by review and by the unchanged-surface assertions, not by a test that
      can fail.
- [ ] NFR2 (O(1) derivation, off the render path) has no automated
      verification scenario. No load or stress test is proposed; the property
      is confirmed by review of the derivation's shape.
- [ ] The adversarial daemon reproduction from the original bug report needs a
      modified or compromised mux daemon and is not automatable here. The
      derivation-level adversarial scenarios stand in for it; the full
      reproduction stays a manual, optional check.
