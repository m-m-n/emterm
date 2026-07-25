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
pane's output, a log, user-pasted content). Never assign untrusted text into
a shell variable and re-expand it into a command line — that assignment
step is itself where injection happens, so it needs the same care as the
final invocation.

- **Preferred: `--stdin` via a quoted-delimiter heredoc.** A heredoc whose
  delimiter is single-quoted suppresses ALL shell expansion of its body —
  no `$(...)`, backticks, `${...}`, or word-splitting — so the untrusted
  text never needs escaping and never passes through a variable at all:

  ```
  emterm mux send --pane <id> --stdin <<'EOF'
  <untrusted text goes here, verbatim>
  EOF
  ```

  This is the safe default for any text that came from outside the current
  conversation (file contents, another pane's output, a log, etc.). The one
  condition it requires: the body must not contain a line consisting solely
  of the chosen delimiter (`EOF` above) — pick a delimiter unlikely to
  collide, or check the text first.

- **If `--text` is used**, single-quote the value and place `--pane` (and
  any other options) before it:
  `emterm mux send --pane <id> --text '<value>'`. If the value itself
  contains a single quote, close the quote, insert `'\''` (end-quote,
  escaped literal quote, reopen-quote), then continue: `it's` becomes
  `it'\''s`. Double quotes are NOT a safe substitute: `$(...)`, backticks,
  and `${...}` all still expand inside double quotes.

- **If a no-shell exec path is available** (e.g. an argv-array invocation
  such as Bun's `Bun.spawn`-style array form), passing the text as a single
  argv element is an equally safe alternative to the heredoc/quoted forms
  above.

- **Pane ID validation**: `--pane` must match a simple identifier
  (`^[a-z0-9-]+$`) or be the literal string `current`. Never take a pane ID
  verbatim from untrusted input without this check.

### Adversarial examples

These show what the safe forms above produce (the pane receives the string
literally) versus what an unsafe, shell-interpolated invocation would do
(the shell executes it):

1. Text is `"; rm -rf ~"` — Safe (heredoc or single-quoted `--text`): the
   pane receives the literal characters `; rm -rf ~` as typed input.
   Unsafe (the text assigned into a variable and interpolated into a
   double-quoted or unquoted command line): the shell would treat `;` as a
   command separator and execute `rm -rf ~`.
2. Text is `"$(whoami)"` — Safe: the pane receives the literal characters
   `$(whoami)`. Unsafe: the shell would expand the command substitution
   and send the output of `whoami` instead of the string itself.
3. Text containing an embedded newline (e.g. `"first line\nrm -rf /"`) —
   Safe: the heredoc passes the text verbatim, including the newline, as
   one `--stdin` input. Unsafe: a shell string built from this text could
   be split into two separate commands, executing `rm -rf /` as a second
   command.

Invoke the command as-is and report the result back to the user.
