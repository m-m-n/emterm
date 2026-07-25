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

  The JSON MUST escape each ESC (0x1B) byte as `\u001b`, and the trailing backslash of `ESC \` as `\\`, so the emitted line parses as valid JSON.
- **FR4:** `plugins/emterm/hooks/hooks.json` MUST configure exactly these hooks, all in **exec form** (a `command` naming the script plus an `args` array), each with `timeout: 3`:
  - `UserPromptSubmit` (no matcher) → args `["working"]`
  - `PostToolUse` (no matcher) → args `["working"]`
  - `PostToolUseFailure` (no matcher) → args `["working"]`
  - `Stop` (no matcher) → args `["idle"]`
  - `PermissionRequest` (no matcher) → args `["blocked"]`
  - `Notification`, with a matcher restricted to the OS-notification cases that mean a human is being waited on → args `["blocked"]`

  The `command` MUST be `${CLAUDE_PLUGIN_ROOT}/hooks/scripts/notify-status.sh`. No hook is configured for `StopFailure` (Claude Code documents its output and exit code as ignored, so an entry there could never report anything — see Edge Cases) or `SubagentStop`.

  `PermissionRequest` is the event that carries `blocked` in the ordinary case: the documentation defines it as "Runs when the user is shown a permission dialog". It MUST carry no matcher, so every permission dialog sets the badge regardless of which tool raised it.
- **FR5:** `blocked` is reported from two distinct events, because Claude Code raises a permission dialog and an OS-level notification through different paths:
  - `PermissionRequest` covers the dialog itself and is unmatched (FR4).
  - The `Notification` hook's matcher MUST fire for `elicitation_dialog` and `agent_needs_input`, and MUST NOT fire for `permission_prompt`, `idle_prompt`, `auth_success`, `elicitation_complete`, `elicitation_response`, or `agent_completed`. The matcher is a regular expression over the notification type; it MUST be anchored so that it cannot match a longer type name as a substring.

  `permission_prompt` is deliberately excluded from the `Notification` matcher: `PermissionRequest` already covers that wait, and `Notification` is a standalone asynchronous event for OS-level notifications, which Claude Code raises when the user is NOT watching the terminal. Wiring `blocked` to `Notification(permission_prompt)` alone is what made the badge stay `working` throughout a visible permission dialog — see Edge Cases.
- **FR6:** `plugins/emterm/hooks/scripts/notify-status.ts` MUST be deleted. `plugins/emterm/hooks/scripts/notify-status.test.ts` MUST be replaced in place with tests over the new shell script (see Test Scenarios), not deleted — its filename and location stay the same while its contents change.
- **FR7:** The four display skills (`plugins/emterm/skills/display-{markdown,json,yaml,image}/SKILL.md`) MUST require, as the primary invocation form, the file path single-quoted and placed after the `--` end-of-options delimiter (`emterm <sub> [opts] -- '<path>'`). A Claude Code skill's only execution surface is the Bash tool, so a no-shell argv invocation MUST NOT be the primary requirement; it MAY be documented as an equally safe alternative where a caller has a no-shell exec path. Each SKILL.md MUST document the `'\''` escaping rule for a path containing an embedded single quote, and MUST NOT show a bare unquoted path as the canonical example. Each MUST carry at least one adversarial example (a path containing shell metacharacters) contrasting the safe quoted form with the unsafe bare form.

  Each SKILL.md MUST state one invariant that covers both the `~` rule and the `'\''` escaping rule, rather than stating one as an absolute and the other as an exception to it: every byte of the path is either inside a single-quoted span or is part of the fixed four-character `'\''` splice, and nothing else path-derived ever appears outside the single quotes. The `~` rule and the splice rule MUST each be presented as an application of that invariant, not a special case of it, and MUST appear adjacent to each other in the SKILL.md so neither reads as contradicting the other in isolation: each SKILL.md MUST require the model to resolve a leading `~` to an absolute path itself before quoting, rather than documenting `~` as a case where part of the argument sits outside the single quotes. Each SKILL.md MUST also state that double quotes are NOT a safe substitute for single quotes, because `$(...)`, backticks, and `${...}` all expand inside double quotes, and MUST carry at least one adversarial example that discriminates on quote TYPE — the same payload shown safe in the single-quoted form and unsafe in the double-quoted form — rather than merely on quote presence.
- **FR8:** `plugins/emterm/README.md` MUST be updated to: remove `bun` from the prerequisites; state that the agent-status hook requires Claude Code v2.1.141 or later (the `terminalSequence` minimum); state that `emterm` on `PATH` is required only for the display and mux skills, not for the hook; and drop the two now-obsolete known limitations (the `/dev/tty` reachability caveat and the up-to-3-seconds-per-prompt caveat) while keeping the mux-agent-status-api drain-wiring caveat.
- **FR9:** `marketplace.json` and `plugin.json` MUST keep `"version": "0.1.0"`. No version field changes in this feature.
- **FR10:** `plugins/emterm/skills/mux-send/SKILL.md` MUST document the file-redirection form `emterm mux send --pane <id> --stdin < '<file>'` as the required primary form for sending text from an untrusted source. The staged file MUST be documented as created with the Write tool — content is a parameter there, never shell text — and Bash-based creation (a heredoc, `printf`, `echo`, or an interpolated-variable redirect) MUST be documented as forbidden for untrusted text, because each of those re-assembles the whole untrusted blob into shell text before it reaches disk. The "nothing derived from the text enters the command line" claim MUST be qualified to hold only when the file was written without a shell. It MUST NOT document a heredoc form anywhere — neither to supply `--stdin` directly nor to create the staged file: a quoted-delimiter heredoc's body ends at any line equal to the delimiter, so an attacker-controlled body can terminate it early, and a heredoc's mandatory trailing newline is an Enter in the destination pane rather than a benign artifact; the adversarial example rejecting heredocs MUST cover both routes.

  The redirect target (the `<file>` in `--stdin < '<file>'`) MUST be documented as a requirement, not a free choice: model-chosen, absolute, under a temp directory, containing no `~`, and built from no byte derived from untrusted input. Where a caller supplies the path to redirect instead, the skill MUST apply the display skills' path rules (FR7) verbatim to it: resolve a leading `~` to an absolute path first, single-quote the whole path, and splice an embedded quote as `'\''`.

  It MUST retain `--text` for short, trusted, model-authored strings under the same single-quote-plus-`'\''` escaping rule FR7 uses, shown as a complete quoted example rather than the splice substring alone, and MUST retain pane-ID validation guidance. Its adversarial examples MUST describe the destination pane's actual behaviour, including that the pane executes what it receives, rather than claiming a payload is delivered as inert literal text. Its closing instruction MUST be conditional: when the text is untrusted and the destination pane is running a shell, the skill MUST require showing the user the exact bytes to be sent and obtaining explicit approval before invoking, and MUST state what happens if the user declines; the unconditional "invoke as-is" instruction MUST NOT be the closing line in that case.

### Non-Functional Requirements

- **NFR1 - Performance:** The hook performs no process spawn, no file I/O beyond writing one line to stdout, and no waiting. No internal timeout mechanism is needed or permitted (there is nothing to wait on). Total work is one shell startup plus one `printf`.
- **NFR2 - Security:** The state argument MUST be validated against a hard-coded allow-list before it is used in any output. Only the validated literal may be interpolated into the emitted sequence; no argument or environment value may reach a shell evaluation context. The script MUST NOT use `eval`, backticks, or `$(...)` over untrusted input.
- **NFR3 - Portability:** The script MUST be POSIX-shell portable — no bashisms (`[[`, arrays, `local`, `echo -e`, `$'...'`). Every path in shipped plugin config (`hooks.json`) MUST be relative to `${CLAUDE_PLUGIN_ROOT}`, never absolute and never containing `..`. Development-time test files (`*.test.ts`) are exempt: they legitimately read `src-tauri/` (for the wire-format cross-check) and the feature docs (for the byte-hygiene guard), and `skills.test.ts` requires the SKILL.md files themselves to contain no `${CLAUDE_PLUGIN_ROOT}` at all, since a skill has no `${CLAUDE_PLUGIN_ROOT}` substitution context of its own.
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
│   ├── mux-send/SKILL.md                   # FR10
│   └── skills.test.ts                      # FR10 / TS-10 mux-send coverage; AC-8 raw-control-byte guard over SPEC.md/VERIFICATION.md
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
- [ ] TS-6: `hooks.json` parses as JSON; declares exactly `UserPromptSubmit`, `PostToolUse`, `PostToolUseFailure`, `Stop`, and `Notification` in exec form (`command` present, `args` present, no state embedded in `command`); every `command` is `${CLAUDE_PLUGIN_ROOT}`-prefixed with no absolute path and no `..`; `timeout` is 3 on each; there is no `StopFailure` or `SubagentStop` entry.
- [ ] TS-7: The `Notification` hook's matcher matches `permission_prompt`, `elicitation_dialog`, and `agent_needs_input`, and does not match `idle_prompt`, `auth_success`, `elicitation_complete`, or `elicitation_response`.
- [ ] TS-8: `notify-status.sh` source contains no reference to `/dev/tty`, `bun`, `eval`, backticks, or command substitution (`$(`), and no invocation of the `emterm` binary; the string `emterm` occurs only as the FR3 payload literal `emterm;agent-status;`, its sole permitted occurrence. `notify-status.ts` no longer exists.
- [ ] TS-9: The sequence emitted for each state is byte-identical to what the Rust canonical builder produces for the same state with name `claude-code` (asserted as a literal, cross-checked against `src-tauri/src/agent_status.rs`).
- [ ] TS-10: All seven SKILL.md files still satisfy the existing static checks (frontmatter, name/directory match, non-empty English description). The four display skills additionally document the Bash-first quoted-and-`--`-delimited invocation form (including the `'\''` escaping rule), the resolve-`~`-before-quoting rule stated adjacent to the splice rule as one invariant (every byte of the path is either inside the single-quoted span or part of the fixed `'\''` splice, nothing else path-derived outside the quotes) rather than as an exception to it, the double-quote-is-not-a-substitute warning naming `$(...)`, backticks, and `${...}`, and carry both a shell-metacharacter adversarial example and a quote-type-discriminating adversarial example. `mux-send/SKILL.md` additionally documents: the file-redirection primary form (`--stdin < '<file>'`) with the staging file required to be created via the Write tool and Bash-based creation forbidden for untrusted text, with no heredoc form present anywhere (staging or `--stdin` supply); the "nothing enters the command line" claim qualified to the file having been written without a shell; the redirect-target requirement (model-chosen, absolute, temp directory, no `~`, no untrusted-derived bytes) with the display skills' path rules given for a caller-supplied path; the `--text` single-quote-plus-`'\''` rule for short trusted strings shown as a complete quoted value; pane-ID validation; adversarial examples that describe the destination pane's actual (executing) behaviour; and a closing instruction conditional on user consent for untrusted text to a shell pane, stating what happens on refusal.
- [ ] TS-11: `marketplace.json` and `plugin.json` both report `version` `0.1.0`.

### Manual Testing (E2E Not Possible)
- [ ] M-1: With the plugin installed locally in an eMterm tab, send a prompt and observe the tab badge become `working`, then `idle` when the response completes. Trigger a permission prompt, approve it, and let the call succeed: observe the badge return to `working` (via `PostToolUse`) before settling on `idle`. Trigger a second permission prompt, approve it, but let the call fail: observe the badge return to `working` via `PostToolUseFailure` rather than staying on `blocked`. Trigger a third permission prompt and deny it: observe the badge stays on `blocked` until the next tool call (success or failure) or `Stop`, since a denied call fires neither `PostToolUse` nor `PostToolUseFailure`. Confirm the badge does NOT flip to `blocked` on an ordinary idle notification after a completed response. Force an API error (e.g. a rate limit) to end the turn via `StopFailure`, which `hooks.json` no longer wires, and observe whether the badge changes; record the result even if inconclusive rather than treating it as a pass.
- [ ] M-2: Read `plugins/emterm/README.md` end to end and confirm every FR8 change is present.

### Edge Cases
- [ ] Running under a terminal that is not eMterm: the OSC 777 sequence is emitted but not interpreted; nothing is displayed and nothing breaks.
- [ ] Running under Claude Code older than v2.1.141: the `terminalSequence` field is ignored; no state is reported and nothing breaks. This floor is derived from `terminalSequence` alone; the exec-form hook configuration and `PostToolUseFailure` are assumed — not separately confirmed — to be available at that version, since neither carries its own minimum-version marker in the hooks documentation.
- [ ] Running inside tmux: no DCS wrapping is involved because the hook never invokes the `emterm` CLI; Claude Code's own write path handles the multiplexer.
- [ ] **Corrected by the M-1 live check — `blocked` was wired to the wrong event:** the original design carried `blocked` on `Notification(permission_prompt)`. The first live run showed the badge staying `working` for the entire duration of a visible permission dialog. `Notification` is a standalone asynchronous event for OS-level notifications and fires when the user is away from the terminal; the event defined as "Runs when the user is shown a permission dialog" is `PermissionRequest`. Four static review rounds examined that matcher in detail — narrowing it, anchoring it, testing all eight documented notification types — and none of them questioned whether the event itself was right. The lesson generalises: reviewing a value inside a construct never checks whether the construct was the correct one to use, and only a live run distinguishes the two.
- [ ] **Known deviation — a manually denied permission dialog leaves `blocked` set:** `PermissionDenied` fires only in auto mode (the documentation states it does not run when the user manually denies a dialog), and a denied call never executes, so neither `PostToolUse` nor `PostToolUseFailure` fires. The badge stays `blocked` until the next successful tool call or `Stop`.
- [ ] **Known deviation — `PostToolUse` can overwrite `blocked` while a permission dialog is open (finding `cm-posttooluse-overwrites-blocked`):** `PostToolUse` carries no matcher (FR4), so a tool call completing while a separate permission-prompt `Notification` is still awaiting the user's answer fires `working` and clears `blocked` before the user responds. The intended precedence — `blocked` persists until the next `UserPromptSubmit`, `Stop`, or a failed/successful tool call — is not implemented anywhere in this feature. The correct fix is a precedence rule inside eMterm's own agent-status state machine, which is out of scope here (`src-tauri/`).
- [ ] **`StopFailure` removed, not wired (finding `cm-stopfailure-noop`):** the hooks documentation states twice that `StopFailure`'s output and exit code are ignored by Claude Code. `notify-status.sh` transports state exclusively through the `terminalSequence` field of its stdout JSON, so an entry on that event could never report anything — the entry a previous round added was a no-op. A turn that ends on an API error fires `StopFailure` instead of `Stop`, so no `idle` report is sent for it; the badge stays on `working` until the next `UserPromptSubmit`. Reinstating the entry requires a live observation that Claude Code actually honours `terminalSequence` on this event (VERIFICATION.md M-1), not reasoning alone — this is the reverse of how the original entry shipped.
- [ ] **Permission-denied gap (finding `cm-posttooluse-failure-unwired`, residual):** a denied permission prompt has no hook event to clear it. Claude Code fires `PreToolUse` for the permission decision and nothing else on denial — the tool never runs, so neither `PostToolUse` nor the new `PostToolUseFailure` fires for that call. The badge that `Notification` set to `blocked` therefore stays `blocked` until the next tool call (success or failure) or the turn ends (`Stop`).

## Security Considerations

- **Input validation:** The state argument is checked against the hard-coded list `idle|working|blocked|done`, and the positional-argument count is required to be exactly 1, before any output is produced.
- **Shell injection:** Only the validated literal is interpolated into the output. The script uses no `eval`, no backticks, and no command substitution over its input. Rejected input produces no output at all, so a metacharacter-laden argument cannot reach the emitted sequence.
- **Escape-sequence injection:** Because only allow-listed literals are emitted, the script cannot be induced to emit an arbitrary escape sequence. Claude Code's own allowlist is a second, independent layer.
- **Skill guidance:** FR7 requires the four display skills to place the path after `--`, single-quoted and `'\''`-escaped, resolving any leading `~` to an absolute path before quoting rather than carving an exception into that rule, and to warn that double quotes are not a substitute because `$(...)`, backticks, and `${...}` expand inside them. This is a Bash-first requirement rather than an argv-based one — a Claude Code skill has no no-shell execution surface. FR10 extends the same discipline to `mux-send/SKILL.md`: its primary form for untrusted text is now file redirection (`--stdin < '<file>'`), which keeps every byte derived from the text off the command line entirely, so no quoting or delimiter rule applies to it at all; the heredoc form that previously carried a delimiter-collision hole is gone. `--text` remains for short, trusted, model-authored strings under the same single-quote-plus-`'\''` rule FR7 uses.
- **Residual risk — prose-escaping is not an enforced boundary:** the display skills' injection protection depends on the model correctly applying the documented single-quote-and-`'\''`-escaping rule at generation time; the plugin has no serialization layer that enforces it mechanically. A single omitted or malformed escape on an untrusted path can still produce a shell-interpreted command. An argv-array execution path would close this gap but is not available: the Bash tool is the only execution surface a Claude Code skill has. This risk is disclosed here rather than mitigated further.

## Error Handling

Every rejection path exits 0 with no output on either stream (NFR4). There are no runtime failure modes left: with no subprocess, no device open, and no timer, the only paths are "valid argument, emit" and "invalid argument, emit nothing".

## Success Criteria

- [ ] All functional requirements (FR1-FR10) are implemented.
- [ ] All automated test scenarios (TS-1 through TS-11) pass under `bun test`, and `bun run typecheck` passes.
- [ ] Manual scenario M-1 demonstrates the state transitions, including the `blocked`-to-`working` recovery on both the approve-and-succeed and approve-then-fail paths, the deny path, and the removed-`StopFailure` observation, on a real eMterm tab.
- [ ] `notify-status.ts` is gone; none of the shipped artifacts (`notify-status.sh`, `hooks.json`, the SKILL.md files, `README.md`) reference `bun` or `/dev/tty`. Development-time test files (`*.test.ts`) are exempt — they use Bun as the project's test runner, per the Testing approach above.
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
