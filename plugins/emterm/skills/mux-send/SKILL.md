---
name: mux-send
description: Sends text or keystrokes to another eMterm mux pane. Use when the user asks Claude to type into, run a command in, or send input to another pane, window, or tab of eMterm's mux.
---

# Send Text to a Mux Pane in eMterm

Run the eMterm CLI's mux send command, passing through the arguments the
user's request implies:

```
emterm mux send --pane <id|current> (--text <s> | --stdin)
```

- `--pane` identifies the target pane: either its ID, or `current` to mean
  the pane the calling session is running in. Pane resolution is handled
  entirely by the `emterm` CLI.
- Exactly one of `--text <s>` (send a literal string) or `--stdin` (send
  bytes piped into the command) is required.

Invoke the command as-is and report the result back to the user.
