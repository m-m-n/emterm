# Implementation Plan: emterm-claude-plugin

## Overview

Add a Claude Code plugin marketplace at the repository root and one plugin (`emterm`) under `plugins/emterm/`. Manifests are JSON; the only executable code is one Bun+TypeScript hook script. No changes to `src-tauri/` or any existing file.

## Technology Stack

- **Bun + TypeScript** — hook script (`notify-status.ts`) and its unit tests. Bun is the project's existing package manager / test runner (see CLAUDE.md).
- **Claude Code plugin format** — marketplace.json / plugin.json / hooks.json / SKILL.md as defined by Claude Code plugin docs.

No new library dependencies (all Bun built-ins). License compatibility with `project.license: MIT` is therefore trivial — no additions to record.

## Layer Structure

Everything the plugin ships lives under one of two directories, both new:

- `.claude-plugin/` (repository root) — marketplace catalog file only.
- `plugins/emterm/` — the plugin. Self-contained: nothing under it references paths outside itself. Every internal path uses `${CLAUDE_PLUGIN_ROOT}` so the file survives the plugin cache copy.

Test code for the hook script goes next to the script (`plugins/emterm/hooks/scripts/notify-status.test.ts`), following the eMterm repo convention.

## Shared Components

None. Each task's outputs are independent files; there are no cross-task contracts to pin.

## Conventions

- **Skill directory naming**: `plugins/emterm/skills/<slug>/SKILL.md` where `<slug>` is the last segment of the slash command (`/emterm:display-markdown` → `skills/display-markdown/`).
- **SKILL.md frontmatter**: english `name` and `description`; description explicitly states the trigger condition (when Claude should invoke the skill) so auto-invocation works.
- **Hook script exit semantics**: any error path exits 0 (silent no-op). This is the design contract from SPEC.md FR4 / NFR2 and must not be relaxed anywhere.
- **State allow-list**: `["idle", "working", "blocked", "done"]` — the exact set the hook accepts. Duplicated between task0002 acceptance criteria and hook implementation; SPEC.md is the SSOT.
- **No shell interpolation**: the hook script spawns `emterm` with an argv array (Bun.spawn's array form), never a single shell string. Same rule for anything else the hook runs.
- **Path convention inside plugin**: every internal reference from a plugin file uses `${CLAUDE_PLUGIN_ROOT}` as the prefix — never absolute, never `../`.

## Cross-task Design Decisions

### D1: Where the plugin lives inside the repo

The repository doubles as the plugin marketplace. Plugin files go under `plugins/emterm/` (new directory) and never overlap with existing eMterm sources. `src-tauri/`, `crates/`, `web-shared/`, etc. are untouched. This isolation lets the plugin evolve independently of the eMterm binary release cycle.

### D2: Plugin ships no eMterm binary

`emterm` is external to the plugin (installed by the user from GitHub Releases). The hook script probes PATH and no-ops if absent. Skills invoke `emterm` by bare name and rely on PATH. This keeps the plugin small and cross-platform-agnostic and dodges the runtime dependency question (libwebkit2gtk etc.).

### D3: Hook error handling is silent

Every failure mode in `notify-status.ts` (bad state arg, missing `emterm`, `/dev/tty` open failure, child non-zero exit, thrown exception, timeout) resolves to `exit 0` with no stderr output that Claude Code would surface. A chatty hook fires on every prompt — silence is a UX requirement, not just a convenience. Tests must cover each failure mode returning exit 0.

### D4: POC verification location

The FR8 POC (real Claude Code session inside eMterm, observing tab state transitions) is a manual verification. It runs during the verify phase — recorded as a manual scenario in VERIFICATION.md rather than as an implement-phase task. Results, including measured Bun startup time, are captured in `feature-docs/emterm-claude-plugin/POC-RESULTS.md` during verify. SPEC.md FR8 wording is satisfied by "before the workflow completes"; forcing it inside implement adds no value because it cannot run in a task worktree.

### D5: Language on user-facing surfaces

SKILL.md `description` fields are in English (matching Claude Code's other skills). Plugin README is in English (target audience is Claude Code users generally). REQUIREMENTS.md and internal planning stay in Japanese per em-workflow convention. This split is deliberate: user-facing plugin surfaces vs. internal feature docs.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `/dev/tty` open fails in some Claude Code execution modes (headless / detached) | Medium | Low (silent degrade) | Test-cover the failure path; document as known limitation in README (FR7). |
| Bun startup pushes single-hook execution over 3 s on slow machines | Low | Medium (user sees hook timeout in Claude Code) | Measure Bun cold-start in POC; if problematic, follow-up feature can drop to a shell script. Not addressed in v0.1.0. |
| mux-agent-status-api drain wiring deferred items mask state changes even when the hook fires correctly | Medium | Low (visible only in specific mux setups) | Documented in README (FR7) as known limitation; not fixed in this feature. |

## Open Questions

- [ ] Whether Claude Code's plugin cache handles executable bit on `notify-status.ts` correctly across install methods. If not, the hook `command` in hooks.json can invoke `bun` explicitly (`bun ${CLAUDE_PLUGIN_ROOT}/hooks/scripts/notify-status.ts <state>`) as a fallback — decide during POC.
