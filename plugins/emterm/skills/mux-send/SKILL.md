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

  **The file MUST be created with the Write tool** — it takes the content
  as a parameter and never constructs shell text, which is what keeps the
  untrusted bytes off a command line at all. Creating the file through the
  Bash tool is forbidden for untrusted text, in every form: a heredoc,
  `printf '%s' '<text>' > f`, `echo '<text>' > f`, or redirecting an
  interpolated shell variable. Each of those re-assembles the whole
  untrusted blob into shell text before it ever reaches disk. The heredoc
  route is the worst of the four: an embedded line matching its delimiter
  terminates the body early, exactly the collision this skill already
  rejects for `--stdin` supply (see the adversarial examples below) — only
  now reproduced one step upstream, on an arbitrary-length body instead of
  a single path.

  Write the file with exactly the bytes you intend to send. Given that,
  nothing derived from the text ever enters the command line in the form
  above, so no delimiter, quoting, or escaping rule applies to the text
  itself — but that holds only because the file was written without a shell;
  a file created via Bash reopens exactly the hole this form exists to
  close. A trailing newline in the file is an Enter in the destination pane:
  include one only if you want the pane to act on the text as a submitted
  command, and omit it if you want the text merely typed without being
  submitted.

  **The redirect target (`<file>`) is a requirement, not a free choice.**
  Prefer a path you choose yourself: absolute, under a temp directory
  (e.g. under `/tmp/`), containing no `~`, and built from no byte derived
  from untrusted input. If a caller supplies the path to redirect instead
  — "send the contents of `~/notes/draft.txt` to pane 2" — apply the
  display skills' path rules to that path verbatim: resolve a leading `~`
  to an absolute path yourself first, then single-quote the whole path,
  and if it contains an embedded single quote, splice it as `'\''`
  (end-quote, escaped literal quote, reopen-quote). Every byte of the path
  is either inside that single-quoted span or is part of the fixed
  four-character splice; nothing else path-derived ever appears outside
  the quotes.

- **`--text` is for short, model-authored, trusted strings only** — never
  for untrusted text; use the file-redirection form above for that.
  Single-quote the value and place `--pane` (and any other options) before
  it: `emterm mux send --pane <id> --text '<value>'`. If the value itself
  contains a single quote, close the quote, insert `'\''` (end-quote,
  escaped literal quote, reopen-quote), then continue: the value `it's`
  becomes the complete quoted token `'it'\''s'`, giving the full command
  `emterm mux send --pane <id> --text 'it'\''s'`. Double quotes are NOT a
  safe substitute: `$(...)`, backticks, and `${...}` all still expand
  inside double quotes.

- **If a no-shell exec path is available** (e.g. an argv-array invocation
  such as Bun's `Bun.spawn`-style array form), passing the text as a single
  argv element is an equally safe alternative to the forms above.

- **Pane ID validation**: `--pane` must match a simple identifier
  (`^[a-z0-9-]+$`) or be the literal string `current`. Never take a pane ID
  verbatim from untrusted input without this check.

### Adversarial examples

These show what actually happens with the forms above — including that the
destination pane executes what it receives, not merely displays it.

1. Text is `"; rm -rf ~"`, written to a file with the Write tool with no
   trailing newline and sent via
   `emterm mux send --pane <id> --stdin < '<file>'` — Safe: the pane
   receives the literal characters `; rm -rf ~` as typed input, and
   because the file has no trailing newline nothing submits them. If the
   file had ended with a newline, the pane would receive an Enter right
   after those characters and, if it is running a shell, would execute
   `rm -rf ~` — this is exactly why the trailing newline is a deliberate
   choice you make in the file, not an accident of the syntax. Unsafe (the
   text assigned into a shell variable and interpolated into a
   double-quoted or unquoted command line): the shell itself would treat
   `;` as a command separator and execute `rm -rf ~` before `emterm` ever
   ran.
2. Text is `"$(whoami)"`, written to a file with the Write tool and sent
   via `--stdin < '<file>'` — Safe: the pane receives the literal
   characters `$(whoami)`; file redirection never asks a shell to
   interpret the file's bytes, so no command substitution happens on the
   way in. Unsafe (`--text "$(whoami)"`, double-quoted): the shell expands
   the command substitution before `emterm` ever runs, and the pane
   receives the output of `whoami` instead of the string itself.
3. Text contains a line that reads exactly `EOF` (e.g. attacker-controlled
   file content, pane output, or a log excerpt) — Safe: written to a file
   with the Write tool and sent via `--stdin < '<file>'`, file redirection
   has no delimiter to collide with at all, so an `EOF` line is just
   another line of data; it changes nothing about where the input ends.
   Unsafe two different ways, both of which put the untrusted body back on
   a shell command line: a heredoc whose body is fed directly into
   `--stdin` ends at the first line equal to its chosen delimiter word, so
   an embedded line matching that word truncates it early and whatever
   follows in the text is interpreted as further shell input; the same
   file created via a heredoc through the Bash tool instead of the Write
   tool hits the identical delimiter collision one step upstream, so
   routing the write through `--stdin` afterward does not rescue it. This
   is why a heredoc is never used for untrusted text anywhere in this
   skill, whether to supply `--stdin` directly or to create the file that
   gets redirected into it.

When the text is untrusted and the destination pane is running a shell,
show the user the exact bytes about to be sent and get their explicit
approval before invoking the command. If the user declines, do not invoke
the command — report back that the send was skipped, and why. Otherwise
(trusted, model-authored text, or a pane that is not running a shell),
invoke the command as-is and report the result back to the user.
