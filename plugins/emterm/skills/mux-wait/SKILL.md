---
name: mux-wait
description: Waits for another eMterm mux pane to reach a given agent state before continuing. Use when the user asks Claude to wait for another pane to finish, go idle, or reach a specific state within eMterm's mux.
---

# Wait on a Mux Pane in eMterm

Run the eMterm CLI's mux wait command, passing through the arguments the
user's request implies:

```
emterm mux wait --pane <id|current> --state <set> [--timeout <sec>] [--after <revision>]
```

- `--pane` identifies the target pane: either its ID, or `current` to mean
  the pane the calling session is running in. Pane resolution is handled
  entirely by the `emterm` CLI.
- `--state` is a comma-separated set of states to wait for (e.g.
  `idle,done`).
- `--timeout` and `--after` are optional.

Invoke the command as-is and report the result back to the user.
