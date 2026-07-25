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

The destination pane executes what it receives if it is running a shell.
Sending untrusted text to a pane running a shell is an action that needs
the user's consent, the same as running a shell command directly.

- **Required for untrusted text: `--stdin` via file redirection.** Write
  the untrusted text to a file, then redirect that file into `--stdin`:

  ```
  emterm mux send --pane <id> --stdin < '<file>'
  ```

  Write the file with exactly the bytes you intend to send — nothing
  derived from the text ever enters the command line in this form, so no
  delimiter, quoting, or escaping rule applies to it at all; the only
  shell-quoted token is the file path, which you chose. A trailing newline
  in the file is an Enter in the destination pane: include one only if you
  want the pane to act on the text as a submitted command, and omit it if
  you want the text merely typed without being submitted.

- **`--text` is for short, model-authored, trusted strings only** — never
  for untrusted text; use the file-redirection form above for that.
  Single-quote the value and place `--pane` (and any other options) before
  it: `emterm mux send --pane <id> --text '<value>'`. If the value itself
  contains a single quote, close the quote, insert `'\''` (end-quote,
  escaped literal quote, reopen-quote), then continue: `it's` becomes
  `it'\''s`. Double quotes are NOT a safe substitute: `$(...)`, backticks,
  and `${...}` all still expand inside double quotes.

- **If a no-shell exec path is available** (e.g. an argv-array invocation
  such as Bun's `Bun.spawn`-style array form), passing the text as a single
  argv element is an equally safe alternative to the forms above.

- **Pane ID validation**: `--pane` must match a simple identifier
  (`^[a-z0-9-]+$`) or be the literal string `current`. Never take a pane ID
  verbatim from untrusted input without this check.

### Adversarial examples

These show what actually happens with the forms above — including that the
destination pane executes what it receives, not merely displays it.

1. Text is `"; rm -rf ~"`, written to a file with no trailing newline and
   sent via `emterm mux send --pane <id> --stdin < '<file>'` — Safe: the
   pane receives the literal characters `; rm -rf ~` as typed input, and
   because the file has no trailing newline nothing submits them. If the
   file had ended with a newline, the pane would receive an Enter right
   after those characters and, if it is running a shell, would execute
   `rm -rf ~` — this is exactly why the trailing newline is a deliberate
   choice you make in the file, not an accident of the syntax. Unsafe (the
   text assigned into a shell variable and interpolated into a
   double-quoted or unquoted command line): the shell itself would treat
   `;` as a command separator and execute `rm -rf ~` before `emterm` ever
   ran.
2. Text is `"$(whoami)"`, written to a file and sent via
   `--stdin < '<file>'` — Safe: the pane receives the literal characters
   `$(whoami)`; file redirection never asks a shell to interpret the
   file's bytes, so no command substitution happens on the way in. Unsafe
   (`--text "$(whoami)"`, double-quoted): the shell expands the command
   substitution before `emterm` ever runs, and the pane receives the
   output of `whoami` instead of the string itself.
3. Text contains a line that reads exactly `EOF` (e.g. attacker-controlled
   file content, pane output, or a log excerpt), sent via
   `--stdin < '<file>'` — Safe: file redirection has no delimiter to
   collide with at all, so an `EOF` line is just another line of data; it
   changes nothing about where the input ends. Unsafe (a heredoc whose
   body is fed into `--stdin`): a heredoc's body ends at the first line
   equal to its chosen delimiter word, so an embedded line matching that
   word truncates it early and whatever follows in the text is
   interpreted as further shell input — this is why a heredoc is never
   used for untrusted text in this skill.

Invoke the command as-is and report the result back to the user.
