# eMterm Claude Code Plugin

## What this plugin does

This plugin connects Claude Code to [eMterm](https://github.com/m-m-n/emterm), a native terminal emulator. It wires three feature families: a hook that reports Claude Code's lifecycle state (working / idle / blocked) to eMterm's agent-status so you can see it on the tab, skills that hand a file to eMterm's rich Markdown / JSON / YAML / image display, and skills that drive eMterm's mux API to read from, send to, and wait on other panes.

## Install

```
/plugin marketplace add m-m-n/emterm
/plugin install emterm@emterm-plugins
```

## Prerequisites

- The `emterm` (or `emterm-cli`) binary, installed separately from [eMterm's GitHub Releases page](https://github.com/m-m-n/emterm/releases). The plugin does not ship the binary. `emterm` on `PATH` is required for the display and mux skills; the agent-status hook does not invoke it.
- Claude Code v2.1.141 or later, required for the agent-status hook. The hook reports state via the `terminalSequence` JSON output field, which that version introduced; the stated floor is derived from `terminalSequence` alone, with the hook's exec-form configuration (`command` plus `args`) and the `PostToolUseFailure` event assumed available at that version too, since Claude Code's hooks documentation carries no separate minimum-version marker for either. On older versions of Claude Code the field is ignored and no state is reported; this is harmless and everything else in the plugin still works.

If `emterm` is not on `PATH`, the display and mux skills fail when invoked; the agent-status hook is unaffected.

## What gets wired

- Six hooks report Claude Code lifecycle events to eMterm agent-status: `UserPromptSubmit` → `working`, `PostToolUse` → `working`, `PostToolUseFailure` → `working`, `Stop` → `idle`, `PermissionRequest` → `blocked`, `Notification` → `blocked`. `PermissionRequest` fires when a permission dialog is shown to you, which is the ordinary way `blocked` appears. The `Notification` hook covers the cases where Claude Code raises an OS-level notification instead — its matcher fires only for `elicitation_dialog` and `agent_needs_input`, so an ordinary idle notification cannot overwrite the `idle` that `Stop` just set.
- Four display skills (`/emterm:display-markdown`, `/emterm:display-json`, `/emterm:display-yaml`, `/emterm:display-image`) render a file through eMterm's rich display.
- Three mux skills (`/emterm:mux-read`, `/emterm:mux-send`, `/emterm:mux-wait`) drive eMterm's mux API to coordinate with other panes.

## Supported platforms

Linux only for v0.1.0. Windows is not supported in this release; Windows support is planned for v0.2.0.

## Known limitations

- Some agent-status state changes may not display if the mux-agent-status-api drain wiring is incomplete on the installed eMterm build.
- The display skills' argument-injection protection relies on the model correctly applying the documented single-quote-and-`'\''`-escaping rule when it constructs the Bash invocation; there is no enforced serialization boundary because a Claude Code skill's only execution surface is the Bash tool. A single incorrect or omitted escape on an untrusted path can still result in shell-interpreted content.
- `PostToolUse` has no matcher and fires on every tool completion. If a different tool call's permission dialog is still open when another tool finishes, the resulting `working` report can clear a `blocked` badge before the user has answered the dialog. No matcher is added to narrow this, since doing so would reintroduce the missed-recovery gap an earlier round fixed; this is a known precedence gap in the current hook wiring.
- Claude Code's hooks documentation states that `StopFailure`'s output and exit code are ignored, so no hook is wired to it. A turn that ends on an API error fires `StopFailure` instead of `Stop`, and the badge stays on `working` until the next prompt.
- A denied permission prompt has no hook to clear it. `PermissionDenied` fires only in auto mode — the documentation states it does not run when you manually deny a dialog — and a denied call never executes, so neither `PostToolUse` nor `PostToolUseFailure` fires either. The badge stays on `blocked` until the next successful tool call or `Stop`.

## Uninstall

```
/plugin uninstall emterm@emterm-plugins
```
