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

## Argument-injection safety (required)

The text being sent may originate from an untrusted source (a file, another
pane's output, a log, user-pasted content). Never build a shell string by
substituting that text into a command line — assemble the invocation as an
argv array (e.g. Bun's `Bun.spawn`-style array form) so the text is never
interpreted by a shell.

- **Preferred: `--stdin`.** Pipe the text into the command instead of
  putting it on the argv line at all:

  ```
  printf '%s' "$TEXT" | emterm mux send --pane <id> --stdin
  ```

  This is the safe default for any text that came from outside the current
  conversation (file contents, another pane's output, a log, etc.), because
  the text never occupies an argv position and cannot be parsed as shell
  syntax.

- **If `--text` is used**, the value MUST be validated to contain no
  newline and no shell metacharacters (`` ` ``, `$`, `;`, `|`, `&`, `(`,
  `)`, `<`, `>`, quotes, backslash), and MUST be passed as a single argv
  element — never interpolated into a joined shell string.

- **Pane ID validation**: `--pane` must match a simple identifier
  (`^[a-z0-9-]+$`) or be the literal string `current`. Never take a pane ID
  verbatim from untrusted input without this check.

### Adversarial examples

These show what the safe forms above produce (the pane receives the string
literally) versus what an unsafe, shell-interpolated invocation would do
(the shell executes it):

1. Text is `"; rm -rf ~"` — Safe (`--stdin` or validated single argv
   element): the pane receives the literal characters `; rm -rf ~` as
   typed input. Unsafe (string-interpolated into a shell command): the
   shell would treat `;` as a command separator and execute `rm -rf ~`.
2. Text is `"$(whoami)"` — Safe: the pane receives the literal characters
   `$(whoami)`. Unsafe: the shell would expand the command substitution
   and send the output of `whoami` instead of the string itself.
3. Text containing an embedded newline (e.g. `"first line\nrm -rf /"`) —
   Safe: the pane receives one input containing a literal newline
   character. Unsafe: a shell string built from this text could be split
   into two separate commands, executing `rm -rf /` as a second command.

Invoke the command as-is and report the result back to the user.
