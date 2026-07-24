---
name: mux-read
description: Reads recent output from another eMterm mux pane. Use when the user asks Claude to check, read, or inspect what is happening in another pane, window, or tab of eMterm's mux.
---

# Read a Mux Pane in eMterm

Run the eMterm CLI's mux read command, passing through the arguments the
user's request implies:

```
emterm mux read --pane <id|current> [--lines N]
```

- `--pane` identifies the target pane: either its ID, or `current` to mean
  the pane the calling session is running in. Pane resolution is handled
  entirely by the `emterm` CLI.
- `--lines` is optional and limits how many trailing lines to read.

Invoke the command as-is and report its stdout back to the user.
