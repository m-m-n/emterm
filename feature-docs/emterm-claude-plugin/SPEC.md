# Feature: emterm-claude-plugin

## Overview

Publish the eMterm repository as a Claude Code plugin marketplace. Ship one plugin (`emterm`) inside `plugins/emterm/` that wires Claude Code lifecycle hooks to `emterm agent-status`, exposes the `emterm markdown|json|yaml|image` rich display commands as skills, and exposes the `emterm mux read|send|wait` API as skills. Linux only for v0.1.0.

## Objectives

- Give Claude Code users a one-command install path to eMterm integration.
- Reuse existing eMterm capabilities (agent-status OSC, rich display CLI, mux API) without touching the eMterm binary itself.
- Prove locally that the `/dev/tty` hook write reaches eMterm during a real Claude Code session.

## User Stories

### US1: Install the plugin from the marketplace

As a Claude Code user, I want to add the eMterm marketplace and install the plugin, so that hooks and skills become available in Claude Code.

**Acceptance Criteria:**
- [ ] `/plugin marketplace add m-m-n/emterm` resolves to `.claude-plugin/marketplace.json` at the repository root.
- [ ] `/plugin install emterm@emterm-plugins` installs the plugin from `plugins/emterm/`.
- [ ] After install, the plugin's hooks and skills are visible in Claude Code.

### US2: See Claude Code state on the eMterm tab

As a Claude Code user running Claude Code inside eMterm, I want the tab state (working / idle / blocked) to follow Claude Code's lifecycle, so that I can see at a glance whether Claude is thinking, done, or waiting on me.

**Acceptance Criteria:**
- [ ] Sending a prompt sets the tab to `working`.
- [ ] Claude finishing a response sets the tab to `idle`.
- [ ] A Claude `Notification` (waiting on human input) sets the tab to `blocked`.
- [ ] If `emterm` is not on PATH, the hook is a no-op and Claude Code continues normally.
- [ ] If `/dev/tty` cannot be opened, the hook is a no-op and Claude Code continues normally.

### US3: Render Markdown / JSON / YAML / images from Claude Code

As a Claude Code user, I want a skill that hands a file to `emterm`'s rich display, so that I can view formatted content in a child window without leaving the chat.

**Acceptance Criteria:**
- [ ] `/emterm:display-markdown <file>`, `/emterm:display-json <file>`, `/emterm:display-yaml <file>`, `/emterm:display-image <file>` each invoke the matching `emterm` CLI subcommand.
- [ ] Each SKILL.md carries an English `description` that describes when Claude should auto-invoke it.

### US4: Drive other mux panes from Claude Code

As a Claude Code user, I want skills for `emterm mux read|send|wait`, so that Claude can coordinate with other panes.

**Acceptance Criteria:**
- [ ] `/emterm:mux-read`, `/emterm:mux-send`, `/emterm:mux-wait` invoke the matching `emterm mux` subcommand.
- [ ] `--pane current` resolution is delegated to the existing `emterm` CLI (via `EMTERM_PANE_ID`).

## Technical Requirements

### Functional Requirements

- **FR1:** A `.claude-plugin/marketplace.json` at the repository root MUST declare a marketplace named `emterm-plugins`, owner `{ "name": "m-m-n" }`, and a single plugin entry `{ "name": "emterm", "source": "./plugins/emterm", "description": "...", "version": "0.1.0" }`. The `source` MUST be an explicit `./`-prefixed relative path: Claude Code's plugin-source resolver requires relative paths to start with `./`, and the `metadata.pluginRoot` shorthand (`"source": "emterm"`) is rejected as an unsupported source type (verified against Claude Code 2.1.219 during the FR8 POC).
- **FR2:** `plugins/emterm/.claude-plugin/plugin.json` MUST declare the plugin's metadata (name, version, description) consistent with the marketplace entry.
- **FR3:** `plugins/emterm/hooks/hooks.json` MUST configure exactly these hooks, each with `type: command`, `timeout: 3`, invoking `${CLAUDE_PLUGIN_ROOT}/hooks/scripts/notify-status.ts <state>`:
  - `UserPromptSubmit` → state `working`
  - `Stop` → state `idle`
  - `Notification` → state `blocked`
  No hook is configured for `SubagentStop`.
- **FR4:** `plugins/emterm/hooks/scripts/notify-status.ts` MUST:
  - Be executable and use `#!/usr/bin/env bun` as its shebang.
  - Accept a single positional argument that MUST be one of `idle`, `working`, `blocked`, `done`. Any other value results in a no-op with exit 0. (`done` is accepted but unused in v0.1.0.)
  - If `emterm` is not resolvable on `PATH`, exit 0 without doing anything else.
  - Otherwise, spawn `emterm agent-status <state> --name "claude-code"`, capture its stdout, and write that stdout to `/dev/tty` (NOT the script's stdout).
  - Never propagate errors: `/dev/tty` open failure, `emterm` non-zero exit, timeouts, or thrown exceptions MUST result in exit 0.
  - Not invoke a shell to expand user input; pass arguments to the `emterm` process as an argv array.
- **FR5:** `plugins/emterm/skills/display-markdown/SKILL.md`, `display-json/SKILL.md`, `display-yaml/SKILL.md`, `display-image/SKILL.md` MUST exist. Each SKILL.md's body MUST instruct the model to invoke the matching `emterm markdown|json|yaml|image` CLI on the provided file. The `display-image` skill MUST document the optional `--protocol kitty|sixel` argument.
- **FR6:** `plugins/emterm/skills/mux-read/SKILL.md`, `mux-send/SKILL.md`, `mux-wait/SKILL.md` MUST exist. Each body MUST instruct the model to invoke the matching `emterm mux read|send|wait` CLI. Pane resolution is delegated to the eMterm CLI (no additional wiring in the skill).
- **FR7:** `plugins/emterm/README.md` MUST document:
  - Install command (`/plugin marketplace add` + `/plugin install`)
  - Prerequisite: install the `emterm` (or `emterm-cli`) binary separately, with a link to the GitHub Releases page
  - Prerequisite: `bun` on PATH (for `notify-status.ts`)
  - Linux-only support in v0.1.0
  - Known limitations: mux-agent-status-api drain wiring deferred items may cause some state changes not to display; no `EMTERM_PANE_ID` fallback in the hook
- **FR8:** A local POC MUST be executed inside the implement phase: install this plugin via `claude`'s local plugin dev path (e.g. `--plugin-dir` or equivalent) and confirm that a real Claude Code session triggers the state transitions in an eMterm tab. Results (including Bun startup timing measurements) MUST be recorded in a doc under `feature-docs/emterm-claude-plugin/` (e.g. `POC-RESULTS.md`).

### Non-Functional Requirements

- **NFR1 - Performance:** End-to-end hook execution (Bun startup + `emterm agent-status` + `/dev/tty` write) MUST complete within the 3 s hook timeout under normal conditions. Bun cold-start time MUST be measured during the POC and recorded.
- **NFR2 - Security:** The hook script MUST validate the state argument against a hard-coded allow-list before use, and MUST never pass unvalidated environment variables or arguments through a shell.
- **NFR3 - Portability:** Every path inside the plugin MUST be relative to `${CLAUDE_PLUGIN_ROOT}` (nothing under `../` or absolute paths outside the plugin directory), because the Claude Code plugin cache does not preserve external references.
- **NFR4 - Distribution:** No compiled binaries are shipped in the plugin. The README's install path directs users to the eMterm GitHub Releases for the `emterm` binary.

## Implementation Approach

### File Structure

```
{repo-root}/
├── .claude-plugin/
│   └── marketplace.json                            # FR1
└── plugins/
    └── emterm/
        ├── .claude-plugin/
        │   └── plugin.json                         # FR2
        ├── hooks/
        │   ├── hooks.json                          # FR3
        │   └── scripts/
        │       └── notify-status.ts                # FR4 (Bun, +x)
        ├── skills/
        │   ├── display-markdown/SKILL.md           # FR5
        │   ├── display-json/SKILL.md               # FR5
        │   ├── display-yaml/SKILL.md               # FR5
        │   ├── display-image/SKILL.md              # FR5
        │   ├── mux-read/SKILL.md                   # FR6
        │   ├── mux-send/SKILL.md                   # FR6
        │   └── mux-wait/SKILL.md                   # FR6
        └── README.md                               # FR7
```

The plugin lives entirely inside `plugins/emterm/`. Nothing outside that directory (including `src-tauri/`) is modified by this feature.

### Hook wiring (FR3/FR4)

```
Claude Code event
   │
   ▼
hooks.json → notify-status.ts <state>
                   │
                   ├── validate state (allow-list)
                   ├── which emterm → missing? exit 0
                   ├── spawn: emterm agent-status <state> --name claude-code
                   └── write child stdout → /dev/tty (open failure → exit 0)
```

Failure semantics are uniform: any failure path exits 0 so Claude Code is never blocked or shown a hook error.

### marketplace.json skeleton (FR1)

```json
{
  "name": "emterm-plugins",
  "owner": { "name": "m-m-n" },
  "description": "eMterm integration plugins for Claude Code",
  "plugins": [
    {
      "name": "emterm",
      "source": "./plugins/emterm",
      "description": "eMterm agent-status hook + rich-display skills + mux API skills",
      "version": "0.1.0"
    }
  ]
}
```

### plugin.json skeleton (FR2)

```json
{
  "name": "emterm",
  "version": "0.1.0",
  "description": "eMterm agent-status hook + rich-display skills + mux API skills"
}
```

### hooks.json skeleton (FR3)

```json
{
  "description": "Report Claude Code lifecycle to eMterm agent-status",
  "hooks": {
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "${CLAUDE_PLUGIN_ROOT}/hooks/scripts/notify-status.ts working", "timeout": 3 }] }],
    "Stop":              [{ "hooks": [{ "type": "command", "command": "${CLAUDE_PLUGIN_ROOT}/hooks/scripts/notify-status.ts idle",    "timeout": 3 }] }],
    "Notification":      [{ "hooks": [{ "type": "command", "command": "${CLAUDE_PLUGIN_ROOT}/hooks/scripts/notify-status.ts blocked", "timeout": 3 }] }]
  }
}
```

### Dependencies

**Internal Dependencies:**
- `emterm` CLI (`emterm agent-status`, `emterm markdown|json|yaml|image`, `emterm mux read|send|wait`) — invoked as an external process; no source changes in `src-tauri/`.

**External Dependencies:**
- Claude Code — consumes the marketplace / plugin format.
- `bun` runtime — required at hook execution time.

## Test Scenarios

### Unit Tests
- [ ] `notify-status.ts` — invalid state argument exits 0 with no `emterm` call.
- [ ] `notify-status.ts` — missing `emterm` binary exits 0 with no side effects.
- [ ] `notify-status.ts` — `/dev/tty` open failure exits 0.
- [ ] `notify-status.ts` — happy path writes captured stdout to `/dev/tty`.

Test approach: `bun test`, using dependency injection or a small shim (e.g. a mock `emterm` in a temp PATH) so tests never touch a real terminal or a real `/dev/tty`.

### Integration Tests
- [ ] Static: `marketplace.json` and `plugin.json` are valid JSON with the required keys.
- [ ] Static: `hooks.json` references `${CLAUDE_PLUGIN_ROOT}` and no absolute paths.
- [ ] Static: every SKILL.md has a non-empty English `description` frontmatter field.

### E2E Tests
**Existing E2E tests**: None detected.
**Run command**: N/A
- [ ] POC scenario: install the plugin locally (via `claude --plugin-dir` or equivalent), start Claude Code inside an eMterm tab, send a prompt and observe the tab transition to `working` → `idle`; trigger a Claude notification and observe `blocked`.

### Edge Cases
- [ ] Running Claude Code in a headless / non-tty context: `/dev/tty` open fails → no-op, Claude Code unaffected.
- [ ] `emterm` on PATH but returns non-zero (e.g. mux daemon down): no-op, Claude Code unaffected.
- [ ] Rapid prompt submission: multiple `UserPromptSubmit` hooks fire back-to-back within 3 s each — none deadlocks Claude Code.

## Security Considerations

- **Input Validation:** State argument checked against allow-list `["idle", "working", "blocked", "done"]` before use.
- **Shell Injection:** `notify-status.ts` MUST use argv-array spawn (`Bun.spawn` with an array), never a shell string.
- **Environment:** The plugin does not read secrets or write any file outside `/dev/tty` (which is a device node the user's own session owns).
- **Distribution:** No compiled artifacts. Binary distribution is out of scope for the plugin; users pull `emterm` from GitHub Releases.

## Error Handling

All hook script error paths funnel to `exit 0`. There are no user-facing error messages from `notify-status.ts`: silent degradation is the design contract, because a chatty hook would leak noise into Claude Code's output on every prompt.

## Success Criteria

- [ ] All functional requirements (FR1-FR8) are implemented.
- [ ] All test scenarios pass (`bun test`, `bun run typecheck`).
- [ ] Local POC (FR8) demonstrates state changes reaching the eMterm tab and records Bun startup timing.
- [ ] Plugin `README.md` documents install path, prerequisites, and known limitations.
- [ ] No changes to `src-tauri/` in this feature.

## Open Questions

None open at spec-freeze time (all clarifications recorded in REQUIREMENTS.md §14.1).

## References

- REQUIREMENTS.md: `feature-docs/emterm-claude-plugin/REQUIREMENTS.md`
- Original plan: `tmp/emterm-plugin-plan.md`
- Claude Code plugin docs: [https://docs.claude.com/en/docs/claude-code/plugins](https://docs.claude.com/en/docs/claude-code/plugins)
- eMterm CLI (agent-status, markdown/json/yaml/image, mux): project `CLAUDE.md`
