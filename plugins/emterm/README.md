# eMterm Claude Code Plugin

## What this plugin does

This plugin connects Claude Code to [eMterm](https://github.com/m-m-n/emterm), a native terminal emulator. It wires three feature families: a hook that reports Claude Code's lifecycle state (working / idle / blocked) to eMterm's agent-status so you can see it on the tab, skills that hand a file to eMterm's rich Markdown / JSON / YAML / image display, and skills that drive eMterm's mux API to read from, send to, and wait on other panes.

## Install

```
/plugin marketplace add m-m-n/emterm
/plugin install emterm@emterm-plugins
```

## Prerequisites

- The `emterm` (or `emterm-cli`) binary, installed separately from [eMterm's GitHub Releases page](https://github.com/m-m-n/emterm/releases). The plugin does not ship the binary.
- `bun` on `PATH`, required to run the hook script.

If `emterm` is not on `PATH`, the hook no-ops silently and Claude Code continues normally.

## What gets wired

- Three hooks report Claude Code lifecycle events to eMterm agent-status: `UserPromptSubmit` → `working`, `Stop` → `idle`, `Notification` → `blocked`.
- Four display skills (`/emterm:display-markdown`, `/emterm:display-json`, `/emterm:display-yaml`, `/emterm:display-image`) render a file through eMterm's rich display.
- Three mux skills (`/emterm:mux-read`, `/emterm:mux-send`, `/emterm:mux-wait`) drive eMterm's mux API to coordinate with other panes.

## Supported platforms

Linux only for v0.1.0. Windows is not supported in this release; Windows support is planned for v0.2.0.

## Known limitations

- Some agent-status state changes may not display if the mux-agent-status-api drain wiring is incomplete on the installed eMterm build.
- The hook has no `EMTERM_PANE_ID` fallback: if `/dev/tty` cannot reach the eMterm PTY (some non-standard shell/multiplexer stacks), state won't propagate.
- The hook adds up to 3 s per Claude Code prompt in the worst case (Bun cold-start + `emterm` invocation).

## Uninstall

```
/plugin uninstall emterm@emterm-plugins
```
