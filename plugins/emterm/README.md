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
- Claude Code v2.1.141 or later, required for the agent-status hook. The hook reports state via the `terminalSequence` JSON output field, which that version introduced. On older versions of Claude Code the field is ignored and no state is reported; this is harmless and everything else in the plugin still works.

If `emterm` is not on `PATH`, the display and mux skills fail when invoked; the agent-status hook is unaffected.

## What gets wired

- Three hooks report Claude Code lifecycle events to eMterm agent-status: `UserPromptSubmit` → `working`, `Stop` → `idle`, `Notification` → `blocked`.
- Four display skills (`/emterm:display-markdown`, `/emterm:display-json`, `/emterm:display-yaml`, `/emterm:display-image`) render a file through eMterm's rich display.
- Three mux skills (`/emterm:mux-read`, `/emterm:mux-send`, `/emterm:mux-wait`) drive eMterm's mux API to coordinate with other panes.

## Supported platforms

Linux only for v0.1.0. Windows is not supported in this release; Windows support is planned for v0.2.0.

## Known limitations

- Some agent-status state changes may not display if the mux-agent-status-api drain wiring is incomplete on the installed eMterm build.

## Uninstall

```
/plugin uninstall emterm@emterm-plugins
```
