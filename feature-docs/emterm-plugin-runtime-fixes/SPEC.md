# Feature: emterm-plugin-runtime-fixes

## Overview

Make the `emterm` Claude Code plugin's agent-status integration actually work. The current hook writes to `/dev/tty`, which Claude Code hooks cannot open as of v2.1.139 — the feature is silently dead. Replace that transport with the officially supported `terminalSequence` JSON output, drop the `emterm` subprocess entirely by building the OSC sequence in the hook, and rewrite the hook in POSIX sh. Also resolve the High/Medium findings from the 2026-07-25 Codex review and the display-skill hardening deferred from the previous feature's round 2.

The plugin version stays at `0.1.0`: it has never been published, so this is completion work on v0.1.0, not a patch release.

## Objectives

- Restore agent-status reporting through a transport that works on current Claude Code.
- Eliminate the subprocess chain, and with it the tmux DCS-wrapping problem, the per-prompt latency, the SIGKILL escalation gap, and the `bun` prerequisite.
- Report `blocked` only when Claude is genuinely waiting on a human.
- Close the display-skill argument-injection gap left open in the previous feature.

## User Stories

### US1: See Claude Code state on the eMterm tab

As a Claude Code user running Claude Code inside eMterm, I want the tab badge to follow Claude Code's lifecycle, so that I can see at a glance whether Claude is thinking, done, or waiting on me.

**Acceptance Criteria:**
- [ ] Sending a prompt sets the tab badge to `working`.
- [ ] Claude finishing a response sets the tab badge to `idle`.
- [ ] Claude waiting on human input sets the tab badge to `blocked`.
- [ ] The hook never opens `/dev/tty` and never spawns a child process.
- [ ] Running outside eMterm is harmless: the escape sequence is simply not interpreted.

### US2: `blocked` means a human is actually needed

As a Claude Code user, I want `blocked` to appear only when Claude is waiting on me, so that the badge is trustworthy.

**Acceptance Criteria:**
- [ ] A `permission_prompt`, `elicitation_dialog`, or `agent_needs_input` notification sets `blocked`.
- [ ] An `idle_prompt` or `auth_success` notification does NOT fire the hook, so it cannot overwrite the `idle` that `Stop` just set.

### US3: Install without a Bun runtime

As a Claude Code user, I want the agent-status hook to work with no runtime prerequisites, so that installing the plugin is a single step.

**Acceptance Criteria:**
- [ ] The hook script runs under POSIX `sh` with no other interpreter installed.
- [ ] The hook works whether or not the `emterm` binary is on `PATH`.
- [ ] The README no longer lists `bun` as a prerequisite for the hook.

## Technical Requirements

### Functional Requirements

- **FR1:** `plugins/emterm/hooks/scripts/notify-status.sh` MUST replace `notify-status.ts`. It MUST use `#!/bin/sh`, be committed with the executable bit set (mode 0755), and depend on nothing beyond a POSIX shell (no `bun`, no `emterm`, no `python3`, no non-POSIX utilities).
- **FR2:** The script MUST accept exactly one positional argument. It MUST exit 0 producing NO output when the positional-argument count is not exactly 1, or when the argument is not one of `idle`, `working`, `blocked`, `done`. (`done` is accepted but no hook currently emits it.)
- **FR3:** For a valid state, the script MUST write to stdout a single JSON object with exactly one key, `terminalSequence`, whose value is the eMterm agent-status escape sequence for that state with the reporter name `claude-code`:

  ```
  ESC ] 777 ; emterm ; agent-status ; v=1 ; state=<state> ; name=claude-code ESC \
  ```

  The canonical byte sequence, matching `crate::agent_status::build` in `src-tauri/src/agent_status.rs` (constants `OSC_INTRODUCER` = `\x1b]777;`, `PAYLOAD_PREFIX` = `emterm;agent-status;`, `WIRE_VERSION` = `1`, `ST` = `\x1b\`), is:

  ```
  \x1b]777;emterm;agent-status;v=1;state=<state>;name=claude-code\x1b\
  ```

  `claude-code` consists only of URI-unreserved characters, so the Rust builder's percent-encoding of `name` is the identity here and the literal is byte-identical to what `emterm agent-status <state> --name claude-code` emits.

  The JSON MUST escape the two control bytes as ``, and the trailing backslash of `ESC \` as `\\`, so the emitted line parses as valid JSON.
- **FR4:** `plugins/emterm/hooks/hooks.json` MUST configure exactly these hooks, all in **exec form** (a `command` naming the script plus an `args` array), each with `timeout: 3`:
  - `UserPromptSubmit` (no matcher) → args `["working"]`
  - `Stop` (no matcher) → args `["idle"]`
  - `Notification`, with a matcher restricted to human-input-awaited notification types → args `["blocked"]`

  The `command` MUST be `${CLAUDE_PLUGIN_ROOT}/hooks/scripts/notify-status.sh`. No hook is configured for `SubagentStop`.
- **FR5:** The `Notification` hook's matcher MUST fire for `permission_prompt`, `elicitation_dialog`, and `agent_needs_input`, and MUST NOT fire for `idle_prompt`, `auth_success`, `elicitation_complete`, or `elicitation_response`. The matcher is a regular expression over the notification type; it MUST be anchored so that it cannot match a longer type name as a substring.
- **FR6:** `plugins/emterm/hooks/scripts/notify-status.ts` and `plugins/emterm/hooks/scripts/notify-status.test.ts` MUST be deleted. Their test coverage MUST be replaced by tests over the new shell script (see Test Scenarios).
- **FR7:** The four display skills (`plugins/emterm/skills/display-{markdown,json,yaml,image}/SKILL.md`) MUST document that the file path is passed as a single argv element through a no-shell invocation, and MUST NOT instruct the model to interpolate the path into a shell command string. Each MUST carry at least one adversarial example (a path containing shell metacharacters). This mirrors the hardening already present in `mux-send/SKILL.md`.
- **FR8:** `plugins/emterm/README.md` MUST be updated to: remove `bun` from the prerequisites; state that the agent-status hook requires Claude Code v2.1.141 or later (the `terminalSequence` minimum); state that `emterm` on `PATH` is required only for the display and mux skills, not for the hook; and drop the two now-obsolete known limitations (the `/dev/tty` reachability caveat and the up-to-3-seconds-per-prompt caveat) while keeping the mux-agent-status-api drain-wiring caveat.
- **FR9:** `marketplace.json` and `plugin.json` MUST keep `"version": "0.1.0"`. No version field changes in this feature.

### Non-Functional Requirements

- **NFR1 - Performance:** The hook performs no process spawn, no file I/O beyond writing one line to stdout, and no waiting. No internal timeout mechanism is needed or permitted (there is nothing to wait on). Total work is one shell startup plus one `printf`.
- **NFR2 - Security:** The state argument MUST be validated against a hard-coded allow-list before it is used in any output. Only the validated literal may be interpolated into the emitted sequence; no argument or environment value may reach a shell evaluation context. The script MUST NOT use `eval`, backticks, or `$(...)` over untrusted input.
- **NFR3 - Portability:** The script MUST be POSIX-shell portable — no bashisms (`[[`, arrays, `local`, `echo -e`, `$'...'`). Every path referenced from plugin files MUST be relative to `${CLAUDE_PLUGIN_ROOT}`.
- **NFR4 - Silent degradation:** Every rejection path exits 0 with no stdout and no stderr. A hook that prints diagnostics on every prompt is unusable; silence is the contract. Unlike the previous implementation, this is now genuinely achievable because there are no failure modes beyond argument validation.

## Implementation Approach

### Transport: why `terminalSequence`

The Claude Code hooks documentation states that on macOS and Linux, command hooks run in their own session without a controlling terminal as of v2.1.139; the hook process and any child process cannot open `/dev/tty`. The documented replacement is the `terminalSequence` field of the hook's JSON output: Claude Code emits the escape sequence through its own terminal write path.

Constraints of that field:

- Allowlist: OSC `0`, `1`, `2`, `9`, `99`, `777`, and a bare BEL.
- Terminator may be BEL or ST.
- Anything outside the allowlist causes the **whole field** to be ignored.
- It is a universal JSON-output field, valid on every hook event.
- Requires Claude Code v2.1.141 or later.

eMterm's agent-status sequence is OSC `777` terminated with ST, so it satisfies the allowlist on both counts. That the *payload* — `emterm;agent-status;...` rather than the conventional `notify;title;body` — is also accepted was verified empirically before this spec was written (see Verified Assumptions).

### Data flow

```
Claude Code event
   │
   ▼
hooks.json (exec form) → notify-status.sh <state>
                              │
                              ├── arg count != 1        -> exit 0, no output
                              ├── state not allow-listed -> exit 0, no output
                              └── printf '{"terminalSequence":"...<state>..."}'
                                        │
                                        ▼
                              Claude Code emits the sequence
                              through its own terminal write path
                                        │
                                        ▼
                                  eMterm parses OSC 777
                                  and updates the tab badge
```

There is no subprocess, no `/dev/tty`, no tmux DCS wrapping, and no timeout.

### File structure

```
plugins/emterm/
├── hooks/
│   ├── hooks.json                          # FR4, FR5 — exec form + matcher
│   └── scripts/
│       ├── notify-status.sh                # FR1-FR3 (new, mode 0755)
│       ├── notify-status.test.ts           # tests invoking the script
│       ├── notify-status.ts                # FR6 — DELETE
│       └── (notify-status.test.ts replaced in place)
├── skills/
│   ├── display-markdown/SKILL.md           # FR7
│   ├── display-json/SKILL.md               # FR7
│   ├── display-yaml/SKILL.md               # FR7
│   ├── display-image/SKILL.md              # FR7
│   └── skills.test.ts                      # unchanged unless assertions need it
└── README.md                               # FR8
```

`.claude-plugin/marketplace.json` and `plugins/emterm/.claude-plugin/plugin.json` are untouched except that their `version` must remain `0.1.0` (FR9).

### Wire-format duplication

The canonical sequence is now built in two places: `src-tauri/src/agent_status.rs` (Rust, for the `emterm agent-status` CLI and the GUI) and `notify-status.sh` (the plugin hook). This duplication is accepted: the format is a single stable line, and FR3 pins the exact bytes. `notify-status.test.ts` derives its expected sequence from the four wire-format constants (`OSC_INTRODUCER`, `PAYLOAD_PREFIX`, `WIRE_VERSION`, `ST`) read directly out of `src-tauri/src/agent_status.rs`, rather than from a hardcoded literal, so a Rust-side change to any of those constants shows up as a failing test rather than silent drift.

### Testing approach

The script is invoked as a subprocess from a Bun test file and its stdout/exit code are asserted. This keeps the existing `bun test` harness (the repository's test runner) while the shipped artifact has no Bun dependency — Bun is a development-time tool here, not a runtime prerequisite.

## Verified Assumptions

The following was **verified empirically before this spec was finalized**, not assumed. The previous feature failed precisely because a "requires measurement" note in its plan was frozen into a spec without measuring.

**Claim:** `terminalSequence` accepts eMterm's custom OSC 777 payload.

**Method:** A temporary `UserPromptSubmit` hook was registered in `.claude/settings.local.json` returning `{"terminalSequence": "<seq>"}`. The discriminator was the badge-shape logic in `src-tauri/src/ui/tab_bar.rs` (`agent_badge_filled`): Blocked/Done render as a ring when seen, while Working/Idle always render filled. Working therefore can never appear as a ring.

| Round | State sent | Badge observed |
| --- | --- | --- |
| 1 | `blocked` | ring |
| 2 | `working` | filled |

**Result:** The badge shape tracked the state. This establishes three things at once: the allowlist inspects only the OSC number (not the payload grammar), Claude Code does emit the sequence through its own write path, and eMterm receives and acts on it.

**Method note:** An OSC 2 window-title marker was initially included as an allowlist control, but Claude Code continuously rewrites the terminal title, so it was overwritten instantly and proved useless as a control. The badge-shape A/B replaced it. When designing a probe, first confirm the observable is not rewritten by the system under test.

## Test Scenarios

### Unit Tests
- [ ] TS-1: Each of `idle` / `working` / `blocked` / `done` produces stdout that parses as JSON with exactly the key `terminalSequence`, whose value equals the FR3 canonical sequence for that state.
- [ ] TS-2: An argument outside the allow-list (`""`, `"invalid"`, `"WORKING"`) produces empty stdout and exit 0.
- [ ] TS-3: Zero positional arguments produces empty stdout and exit 0.
- [ ] TS-4: Two or more positional arguments (e.g. `working extra`) produces empty stdout and exit 0.
- [ ] TS-5: An argument containing shell metacharacters (e.g. `"working; touch PWNED"`, `"$(id)"`) produces empty stdout, exit 0, and no side effect on the filesystem.

### Integration Tests
- [ ] TS-6: `hooks.json` parses as JSON; all three hooks are in exec form (`command` present, `args` present, no state embedded in `command`); every `command` is `${CLAUDE_PLUGIN_ROOT}`-prefixed with no absolute path and no `..`; `timeout` is 3 on each; there is no `SubagentStop` entry.
- [ ] TS-7: The `Notification` hook's matcher matches `permission_prompt`, `elicitation_dialog`, and `agent_needs_input`, and does not match `idle_prompt`, `auth_success`, `elicitation_complete`, or `elicitation_response`.
- [ ] TS-8: `notify-status.sh` source contains no reference to `/dev/tty`, `emterm`, `bun`, `eval`, or backticks, and `notify-status.ts` no longer exists.
- [ ] TS-9: The sequence emitted for each state is byte-identical to what the Rust canonical builder produces for the same state with name `claude-code` (asserted as a literal, cross-checked against `src-tauri/src/agent_status.rs`).
- [ ] TS-10: All seven SKILL.md files still satisfy the existing static checks (frontmatter, name/directory match, non-empty English description), and the four display skills additionally document argv-based invocation and contain an adversarial example.
- [ ] TS-11: `marketplace.json` and `plugin.json` both report `version` `0.1.0`.

### Manual Testing (E2E Not Possible)
- [ ] M-1: With the plugin installed locally in an eMterm tab, send a prompt and observe the tab badge become `working`, then `idle` when the response completes. Trigger a permission prompt and observe `blocked`. Confirm the badge does NOT flip to `blocked` on an ordinary idle notification after a completed response.
- [ ] M-2: Read `plugins/emterm/README.md` end to end and confirm every FR8 change is present.

### Edge Cases
- [ ] Running under a terminal that is not eMterm: the OSC 777 sequence is emitted but not interpreted; nothing is displayed and nothing breaks.
- [ ] Running under Claude Code older than v2.1.141: the `terminalSequence` field is ignored; no state is reported and nothing breaks.
- [ ] Running inside tmux: no DCS wrapping is involved because the hook never invokes the `emterm` CLI; Claude Code's own write path handles the multiplexer.

## Security Considerations

- **Input validation:** The state argument is checked against the hard-coded list `idle|working|blocked|done`, and the positional-argument count is required to be exactly 1, before any output is produced.
- **Shell injection:** Only the validated literal is interpolated into the output. The script uses no `eval`, no backticks, and no command substitution over its input. Rejected input produces no output at all, so a metacharacter-laden argument cannot reach the emitted sequence.
- **Escape-sequence injection:** Because only allow-listed literals are emitted, the script cannot be induced to emit an arbitrary escape sequence. Claude Code's own allowlist is a second, independent layer.
- **Skill guidance:** FR7 extends the argv-based invocation requirement from `mux-send` to the four display skills, closing the last documented shell-string-interpolation path in the plugin's skill set.

## Error Handling

Every rejection path exits 0 with no output on either stream (NFR4). There are no runtime failure modes left: with no subprocess, no device open, and no timer, the only paths are "valid argument, emit" and "invalid argument, emit nothing".

## Success Criteria

- [ ] All functional requirements (FR1-FR9) are implemented.
- [ ] All automated test scenarios (TS-1 through TS-11) pass under `bun test`, and `bun run typecheck` passes.
- [ ] Manual scenario M-1 demonstrates the three state transitions on a real eMterm tab.
- [ ] `notify-status.ts` is gone and nothing in the plugin references `bun` or `/dev/tty`.
- [ ] Plugin version remains `0.1.0`.
- [ ] No changes to `src-tauri/`, `crates/`, or any other pre-existing project source outside `plugins/` and `feature-docs/`.

## Open Questions

None. The one assumption that could have invalidated the design was measured before the spec was frozen (see Verified Assumptions).

## References

- REQUIREMENTS.md: `feature-docs/emterm-plugin-runtime-fixes/REQUIREMENTS.md`
- Plan and Codex review findings: `tmp/emterm-plugin-hook-transport-plan.md`
- Previous feature: `feature-docs/emterm-claude-plugin/` (SPEC.md, reviews/round1.yaml, reviews/round2.yaml, retrospect.yaml)
- Canonical wire format: `src-tauri/src/agent_status.rs` (`build`, `build_set_payload`, and the `OSC_INTRODUCER` / `PAYLOAD_PREFIX` / `WIRE_VERSION` / `ST` constants)
- Badge rendering used as the POC discriminator: `src-tauri/src/ui/tab_bar.rs` (`agent_badge_filled`, `agent_state_color`)
