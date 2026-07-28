# Agent Status Reporting & Agent-Facing API

Panes can report the state of the AI agent (or any process) running in them.
eMterm aggregates those reports into tab/window badges, fires OS
notifications on attention-needed transitions, and exposes a read / send /
wait API over the mux socket so agents can coordinate with each other.

## State Model

A pane's agent status is one of four states, or no state at all:

| State | Meaning |
|-------|---------|
| `idle` | The agent is running but not currently working. |
| `working` | The agent is actively working. |
| `blocked` | The agent is waiting for human input (e.g. an approval). |
| `done` | The agent finished its current task. |
| *(no state)* | Default. No report has been received, or the pane cleared its state. |

A pane's state carries an optional display `name` and a monotonically
increasing `revision`. Every accepted report — a state set, a `clear`, or a
re-report of the same state — increments the revision. If a pane has
multiple reporters, the last received report wins; there is no merging.

State is held only for the life of the pane. It is discarded when the pane
exits or is destroyed, and it is never persisted across daemon or
application restarts.

## Reporting a State

### `emterm agent-status` CLI

```
emterm agent-status <idle|working|blocked|done> [--name <name>]
emterm agent-status clear
```

The command is stateless: it emits an OSC escape sequence to stdout and
exits. It is available in both the GUI build and the CLI-only build
(`--no-default-features`).

Examples:

```
emterm agent-status working
emterm agent-status blocked --name "waiting for approval"
emterm agent-status done
emterm agent-status clear
```

### Wire format

Reporting uses the existing `OSC 777;emterm;<kind>;...` namespace with kind
`agent-status`.

Set form:

```
OSC 777;emterm;agent-status;v=1;state=<idle|working|blocked|done>[;name=<value>]
```

Clear form:

```
OSC 777;emterm;agent-status;clear
```

`name` is percent-encoded UTF-8. After decoding, it is normalized (control
characters stripped) and truncated to 80 characters. Unknown keys are
ignored. A missing or invalid `state`, a duplicate key, or a failed
percent-decode invalidates the whole sequence — the pane's state and
revision are left untouched (no partial application).

The sequence affects only the pane that emitted it; it carries no pane ID.

### tmux passthrough

Inside tmux, the sequence must be wrapped in DCS passthrough
(`ESC P tmux; ... ESC \`) to reach the terminal, the same mechanism used by
`emterm markdown` / `emterm json` / `emterm yaml` / `emterm image`.
`emterm agent-status` does this automatically when run inside tmux. tmux
must have `set -g allow-passthrough on` configured.

### SSH transparency

The sequence travels through stdout like any other terminal output, so it
works unmodified over SSH.

### Reaching the terminal's PTY

The sequence only has an effect if it reaches the terminal's PTY. Any
integration that reports agent status — including hook systems or wrapper
scripts — must ensure its output is written to a file descriptor that is
the terminal's TTY, not captured or redirected elsewhere. Hook systems that
capture a process's stdout for their own purposes will not deliver the
sequence to the terminal without a configuration that routes it to the TTY.

## Agent-Facing API

The mux daemon exposes a read / send / wait API over its socket for agents
to coordinate with other panes. This API ships in the GUI build binary
(`emterm mux ...`).

### Pane addressing

Every mux pane has a stable, opaque, non-reusable public ID (it is never
reused across daemon restarts and encodes no window/tab position or name).
Mux pane spawn injects the pane's ID into the pane's environment as
`EMTERM_PANE_ID`. Every `--pane` flag below accepts either an explicit ID or
the literal `current`, which resolves via `EMTERM_PANE_ID`.

The GUI provides a copy-to-clipboard affordance for a pane's public ID.

### `emterm mux read`

```
emterm mux read --pane <id|current> [--lines N]
```

Returns the tail `N` rendered rows (current screen plus scrollback tail) as
ANSI-stripped UTF-8 plain text. `N` and the total response size are capped.

### `emterm mux send`

```
emterm mux send --pane <id|current> (--text <string> | --stdin)
```

Writes the UTF-8 string verbatim to the target pane's PTY. There is no
implicit Enter and no key interpretation. NUL bytes are rejected, input size
is capped, and the write is atomic per request. The response returns the
pane's revision as observed immediately before the successful write (the
watermark) — see `--after` below.

### `emterm mux wait`

```
emterm mux wait --pane <id|current> --state <set> [--timeout <seconds>] [--after <revision>]
```

`--state` takes a comma-separated set of states (e.g. `--state done,blocked`).
`wait` is level-triggered: it succeeds immediately if the pane's current
state is in the given set and, when `--after` is given, the pane's revision
is greater than `<revision>`. Otherwise it blocks until a qualifying report
arrives, or until `--timeout` elapses. A pane with no state waits until one
is set.

`--after` exists to linearize a `send` followed by a `wait`: pass the
watermark returned by `send` as `--after` so that a `done`/`blocked` state
that was already present before the send does not satisfy the wait — only a
report accepted after the send does.

If the pane is destroyed while a wait is pending, the wait errors out. If
the CLI process disconnects, the daemon discards its waiter.

### Exit codes

| Exit code | Meaning |
|-----------|---------|
| `0` | Success. |
| `1` | All other errors (e.g. connection failure). |
| `2` | Usage or invalid input. |
| `3` | `wait` timed out. |
| `4` | Unknown pane, or the pane was gone/destroyed. |
| `5` | Target pane is not a mux pane (`not_mux_pane`). |

## Trust Boundary

Any process that can write to a pane's PTY — including a process on a
remote host reached over SSH — can forge an `agent-status` report for that
pane. This is an inherent property of the reporting mechanism (an active OSC
report), not a bug.

The consequences of a forged report are limited to display and
notifications: a forged state can mislead the badge or trigger a spurious
notification. Reported agent state is never used as an
authorization input and never used to identify or select a pane for the API
— pane identification uses the opaque public pane ID only, independent of
any reported agent state.

The mux socket itself is restricted to the same OS user as the daemon.
`emterm mux read` and `emterm mux send` are terminal-equivalent privilege:
anyone able to reach the socket can read a pane's screen contents and send
arbitrary input to it, equivalent to typing at that terminal directly.
Display names carried in reports are sanitized (control characters stripped)
before they are shown in the UI or in a notification.

## Notifications

An OS notification fires when a pane not visible in the foreground window
has a real transition to `blocked` or `done`. The following never produce a
notification:

- A same-state re-report.
- A name-only change (no state change).
- A state update derived from snapshot/replay (e.g. after reattaching to a
  mux session).

Notifications are further limited by a per-pane rate limit, and only fire
when both the `agent_status_notifications` setting (default on) and the
existing global notification setting are enabled. The name shown in a
notification body is the sanitized (control-character-stripped) display
name.
