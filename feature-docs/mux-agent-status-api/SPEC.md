# Feature: mux Agent Status & Agent-Facing API

## Overview

Panes report the state of the AI agent running in them (idle / working /
blocked / done) via an active OSC report. eMterm aggregates those states into
tab/window-list badges and a status-bar summary, fires OS notifications on
blocked/done transitions, and exposes a read / send / wait API over the mux
daemon socket so agents can coordinate with each other.

## Objectives

- Let users see at a glance which agent needs human attention, without
  cycling through panes.
- Notify the user when a non-visible pane's agent becomes blocked or done.
- Let agents read other panes' output, send input, and wait for state
  changes programmatically.

## User Stories

### US1: State at a glance
As a user running multiple agents, I want each tab/window entry to show an
aggregated agent-state badge and the status bar to show per-state counts, so
that I can find the agent that needs me without switching tabs.

**Acceptance Criteria:**
- [ ] A state report in any pane updates the containing tab/window badge and
      the status-bar summary.
- [ ] Badge aggregation follows the priority blocked > unseen done >
      working > seen done > idle.
- [ ] Focusing the tab in the foreground OS window clears only the unseen
      emphasis, never the semantic state.

### US2: Notification on attention-needed transitions
As a user, I want an OS notification when a pane I am not looking at
transitions to blocked or done, so that I can react without polling.

**Acceptance Criteria:**
- [ ] Real transitions to blocked/done on panes not visible in the
      foreground window produce a notification.
- [ ] Snapshot/replay-derived state, same-state re-reports, and name-only
      changes never notify.
- [ ] Both the agent-notification setting (default on) and the global
      notification setting must be enabled.

### US3: Agent coordination API
As an agent, I want to read another pane's output, send input to it, and
wait until its state enters a given set, so that multi-agent workflows can
be scripted.

**Acceptance Criteria:**
- [ ] `emterm mux read/send/wait` behave per FR10-FR12, including error and
      timeout exit codes.
- [ ] `--pane current` resolves via `EMTERM_PANE_ID`.
- [ ] `wait --after <revision>` never succeeds on a revision at or below
      the watermark.

## Technical Requirements

### Functional Requirements

- **FR1 — agent-status OSC sequence:** Extend the existing
  `OSC 777;emterm;<kind>;…` dispatcher with kind `agent-status`.
  - Set form: `OSC 777;emterm;agent-status;v=1;state=<idle|working|blocked|done>[;name=<value>]`
  - Clear form: `OSC 777;emterm;agent-status;clear`
  - `name` is percent-encoded UTF-8; after decoding, normalize and truncate
    to 80 characters.
  - Unknown keys are ignored. A missing/invalid `state`, duplicate keys, or
    a failed decode invalidates the whole sequence (state and revision are
    left untouched).
  - The sequence affects only the originating pane; it carries no pane ID.
  - Multiple reporters on one pane: last received report wins.
- **FR2 — `emterm agent-status` CLI:**
  `emterm agent-status <idle|working|blocked|done> [--name <n>]` and
  `emterm agent-status clear` emit the FR1 sequence to stdout, stateless,
  with tmux DCS passthrough wrapping like the existing subcommands.
  Available in both the GUI build and the CLI-only build
  (`--no-default-features`).
- **FR3 — daemon per-pane state:** The mux daemon stores per pane:
  `state` (4-value or none), normalized `name`, and a monotonically
  increasing `revision` (u64). Every ACCEPTED report — set, clear, or a
  same-state re-report — increments `revision`. State is discarded on
  PtyExited / pane destroy and never persisted.
- **FR4 — replay stripping & post-snapshot sync:** The agent-status OSC is
  stripped from scrollback and snapshot replay (extending the existing
  strip mechanism). The Snapshot payload format is unchanged; after a
  snapshot is delivered, current states are synced via FR5 messages flagged
  as replay-derived. Replay-derived updates never notify.
- **FR5 — AgentStatusUpdate IPC message:** New daemon→GUI mux_ipc message
  carrying `pane id, state (or none/cleared), name, revision,
  replay_derived flag`. The existing StatusUpdate message is not modified.
  Define the PROTOCOL_VERSION handling for the addition (bump, or a
  backward-compatible extension) such that an old GUI / new daemon pairing
  fails cleanly rather than misparsing.
- **FR6 — unified GUI state model:** The GUI holds one AgentStatus model
  per pane. Plain (non-mux) tabs: the GUI parses the OSC itself and owns
  the state. Mux panes: the daemon owns the state; the GUI applies FR5
  updates. Tab close discards the state.
- **FR7 — tab/window badges:** Each tab/window list entry shows a single
  aggregated badge over its panes with priority
  blocked > unseen done > working > seen done > idle. `seen/unseen` is
  GUI-client-local, separate from semantic state; a pane becomes seen when
  its containing tab is displayed in the foreground OS window. Seeing
  clears emphasis only.
- **FR8 — status-bar summary:** The status bar shows per-state pane counts
  (by semantic state, regardless of seen). Hidden when no pane has a
  reported state.
- **FR9 — OS notifications:** Fire on real transitions to blocked/done for
  panes not visible in the foreground window. Suppress for: same-state
  re-reports, name-only changes, snapshot/replay-derived updates. Per-pane
  rate limiting. Gated on the agent-notification setting (default on) AND
  the global notification setting. Names shown in notifications are
  control-character-stripped.
- **FR10 — `emterm mux read`:**
  `emterm mux read --pane <id|current> [--lines N]` returns the tail N
  rendered rows (current screen + scrollback tail) as ANSI-stripped UTF-8
  plain text. Caps on N and on response bytes. Nonexistent pane → error;
  plain-tab target → `not_mux_pane` error.
- **FR11 — `emterm mux send`:**
  `emterm mux send --pane <id|current> (--text <s> | --stdin)` writes the
  UTF-8 string verbatim to the pane's PTY — no implicit Enter, no key
  interpretation, NUL forbidden, size cap, atomic per-request write. The
  response returns the pane's revision watermark from just before the
  successful write.
- **FR12 — `emterm mux wait`:**
  `emterm mux wait --pane <id|current> --state <set> [--timeout <sec>]
  [--after <revision>]`. Level-triggered: succeeds immediately when the
  current state is in the set AND (when `--after` is given) revision >
  watermark. A pane with no state waits until one is set. Pane destroyed →
  error exit; timeout → dedicated exit code; a disconnected client's
  waiter is discarded by the daemon.
- **FR13 — pane ID system:** Public opaque pane IDs, non-reusable across
  daemon restarts (include a daemon incarnation component). IDs never
  encode window/tab position or agent name. Mux pane spawn injects
  `EMTERM_PANE_ID` into the pane's environment; `--pane current` resolves
  from it. The GUI provides an affordance to copy a pane's ID.

### Non-Functional Requirements

- **NFR1 - Security:** Any PTY output (including SSH remotes) can forge
  agent-status OSC; document this trust boundary. Forgery affects display
  and notifications only — semantic state is never an input to API
  authorization or pane identification. The mux socket stays same-user
  only; document read/send as terminal-equivalent privilege. Names are
  sanitized (control characters stripped) before display/notification.
- **NFR2 - Compatibility:** Existing StatusUpdate message and Snapshot
  payload formats are unchanged. PROTOCOL_VERSION handling for the new
  message is explicit. CLI-only build keeps working (`agent-status`
  included; `mux read/send/wait` ships in the GUI build binary). tmux
  passthrough works via the existing DCS wrapping.
- **NFR3 - Performance:** State handling is event-driven; no polling and no
  added per-frame render cost when states are unchanged. read responses are
  size-capped.
- **NFR4 - UI/i18n:** Badge and status-bar visuals follow the MD3 tokens
  (doc/UI-DESIGN-GUIDELINES.yaml); user-facing GUI strings use the inline
  `t(ja, en)` i18n mechanism.

## Implementation Approach

### Architecture

```
agent / shell
  └─ emterm agent-status …  → OSC 777;emterm;agent-status  (stdout → PTY)
       │
       ├─ plain tab: GUI term_core OSC dispatch → GUI AgentStatus model
       └─ mux pane:  daemon OSC dispatch → per-pane {state,name,revision}
                        │ strip from scrollback/snapshot replay
                        └ AgentStatusUpdate (mux_ipc) → GUI AgentStatus model
                                                          ├─ tab/window badges
                                                          ├─ status-bar summary
                                                          └─ OS notification

agent
  └─ emterm mux read|send|wait  → mux socket (ClientType::Cli, one-shot RPC)
       └─ daemon: ReadPane / SendText / WaitAgentState handlers
```

### Data Flow

- State report: pane PTY output → OSC dispatcher (GUI or daemon) →
  AgentStatus update (revision++) → badge/status-bar refresh → notification
  check.
- Snapshot/reattach: snapshot replay (OSC stripped) → AgentStatusUpdate
  with `replay_derived: true` → model update, no notification.
- API: CLI connects to the mux socket as a control client, sends one
  request, receives one response (wait: response deferred until the
  condition or timeout).

### API Design (mux socket messages)

New request/response pairs (exact wire encoding decided in planning,
consistent with existing mux_ipc framing):

- `ReadPane { pane_id, lines }` → `ReadPaneResult { text } | Error`
- `SendText { pane_id, bytes }` → `SendTextResult { revision_watermark } | Error`
- `WaitAgentState { pane_id, states, timeout, after_revision }` →
  `WaitResult { state, revision } | Error(timeout | pane_gone | not_mux_pane | unknown_pane)`
- `AgentStatusUpdate { pane_id, state?, name?, revision, replay_derived }`
  (daemon → GUI, unsolicited)

### Dependencies

**Internal Dependencies:**
- `crates/term_core`: OSC 777 dispatch path (existing).
- `crates/mux_ipc`: message type additions, PROTOCOL_VERSION.
- `src-tauri/src/mux/*`: daemon state store, strip mechanism, socket
  handlers, `EMTERM_PANE_ID` injection.
- `src-tauri/src/cli/*`: `agent-status` subcommand (CLI-shared, no GUI
  crates); `mux` subcommand (GUI build).
- `src-tauri/src/ui/*`: badges, status-bar summary, pane-ID copy
  affordance (egui, MD3 tokens, inline t(ja,en)).
- Settings: new agent-notification toggle in `crates/app_settings` +
  settings panel mirror (`src-tauri/web-shared/settings/types.ts`).

**External Dependencies:**
- None new.

### File Structure

Expected touch points (indicative, refined by planning):

```
crates/mux_ipc/src/protocol.rs        # AgentStatusUpdate + API messages
crates/term_core/…                    # OSC 777 agent-status parse (if parsed here)
src-tauri/src/mux/…                   # daemon state store, strip, handlers, env injection
src-tauri/src/callbacks.rs            # OSC 777 dispatcher branch (plain tab path)
src-tauri/src/cli/…                   # agent-status subcommand
src-tauri/src/ui/…                    # badges, status-bar summary, ID copy
crates/app_settings/…                 # notification setting
src-tauri/web-shared/settings/…       # TS mirror + settings UI
```

## Test Scenarios

### Unit Tests
- [ ] FR1 parsing: valid set/clear, invalid state, duplicate keys, bad
      percent-encoding, name truncation at 80 chars, unknown keys ignored.
- [ ] FR3 revision semantics: increments on set/clear/same-state re-report;
      untouched on rejected sequences; cleared on pane destroy.
- [ ] FR7 aggregation priority and seen/unseen transitions.
- [ ] FR9 notification gating (visibility, transition-only, replay
      suppression, rate limit, settings).
- [ ] FR10-FR12 handler behavior incl. caps, not_mux_pane, unknown pane,
      NUL rejection, level-triggered wait with `--after`.
- [ ] FR13 ID format: opaqueness, incarnation component, non-reuse.

### Integration Tests
- [ ] State report through daemon → AgentStatusUpdate → GUI model.
- [ ] Snapshot reattach: OSC stripped from replay, state restored via
      replay-derived updates, no notification.
- [ ] CLI `agent-status` output wrapped/unwrapped by tmux passthrough.
- [ ] send → wait `--after` linearization (pre-existing done does not
      satisfy the wait).

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Manual scenario: two panes, one reporting states; badge, summary,
      notification, and read/send/wait round-trip verified on a live build.

### Edge Cases
- [ ] Old-GUI/new-daemon (and inverse) pairing fails cleanly per the
      PROTOCOL_VERSION decision.
- [ ] wait on a pane that is destroyed mid-wait errors out.
- [ ] Multiple waiters on one pane; client disconnect discards its waiter.
- [ ] Status-bar summary hides when the last reported-state pane closes.

## Security Considerations

- **Input Validation:** FR1 sequence validation (reject-whole-on-invalid);
  API request validation (caps, NUL, unknown pane).
- **Data Protection:** control-character stripping of names before any
  display or notification surface.
- **Trust boundary:** forged agent-status affects display/notification
  only; API authorization never derives from reported state. Socket stays
  same-user; read/send documented as terminal-equivalent privilege.

## Error Handling

### Error / exit codes (CLI)

| Case | Behavior |
|------|----------|
| unknown pane | error message, non-zero exit |
| not_mux_pane | dedicated error, non-zero exit |
| wait timeout | dedicated exit code (distinct from generic error) |
| pane destroyed during wait | error exit |
| invalid input (NUL, size cap) | error exit, nothing written |

## Success Criteria

- [ ] All functional requirements implemented and covered by the test
      scenarios above.
- [ ] Acceptance criteria of US1-US3 pass.
- [ ] No regression in existing mux replay/snapshot tests.
- [ ] CLI-only build (`--no-default-features`) still compiles and includes
      `agent-status`.

## Assumptions

Decisions taken without user confirmation (batch mode; consultation-backed):

- **A1:** Claude Code hooks integration is a non-goal for this feature.
  Rationale recorded: hook stdout does not reach the PTY, so a
  hooks-driven OSC path is unverified. The docs note only that a
  TTY-reaching configuration is required.
- **A2:** This is one feature (no split); task decomposition is left to
  planning.
- **A3:** `emterm mux read/send/wait` ships in the GUI build binary;
  `emterm agent-status` ships in both GUI and CLI-only builds.
- **A4:** Feature name `mux-agent-status-api`.
- **A5:** State priority order, seen semantics, revision/watermark design,
  and pane-ID incarnation scheme adopted as specified in FR7/FR3/FR11-13
  (consultation-refined, no user sign-off).
- **A6:** The agent-notification setting defaults to ON.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。

- None (all requirements are `ok`; assumptions above stand in for user
  confirmation).

## References

- 要件定義書: feature-docs/mux-agent-status-api/REQUIREMENTS.md
- MD3 tokens: doc/UI-DESIGN-GUIDELINES.yaml
- Logging constraints: .claude/rules/debugging-constraints.md
