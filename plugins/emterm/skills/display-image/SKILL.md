---
name: display-image
description: Renders an image file inline in eMterm using the Kitty Graphics Protocol or SIXEL. Use when the user asks to view, preview, or display an image file (e.g. .png, .jpg) inside eMterm, rather than opening it in an external application.
---

# Display an Image in eMterm

Run the eMterm CLI on the image file the user gave you:

```
emterm image -- '<path>'
```

Replace `<path>` with the path to the image file, single-quoted and placed
after the end-of-options delimiter `--`. `emterm` writes a display escape
sequence to stdout; eMterm's terminal intercepts it and renders the image
inline.

The `--protocol` argument is optional and selects the inline-image
protocol: `kitty` (the default) or `sixel`. By default `emterm image` uses
the Kitty Graphics Protocol, so omit `--protocol` unless the user asks for
a specific protocol. To select SIXEL explicitly, pass `--protocol sixel` —
an option, so it goes before the `--` delimiter, like any other option:

```
emterm image --protocol sixel -- '<path>'
```

## Argument-injection safety (required)

The file path may originate from an untrusted source (a path pasted by the
user, extracted from another file, or produced by earlier tool output). A
path such as `photo.png; touch PWNED`, if placed bare and unquoted on the
command line, would let the shell treat `; touch PWNED` as a second command
instead of part of the filename.

- Always single-quote the path and place it after the end-of-options
  delimiter `--`, never as a bare unquoted argument:
  `emterm image -- '<path>'` (options such as `--protocol` go before the
  `--`).
- eMterm's CLI is clap-based and accepts `--` as the end-of-options marker
  on this subcommand — everything after it is a positional argument, never
  parsed as a flag, regardless of what characters the path contains.
- If the path starts with `~` (home-directory shorthand), resolve it to an
  absolute path yourself before quoting — do not rely on the shell to
  expand it: single quotes suppress `~` expansion, and eMterm's CLI does
  not expand `~` itself, so a single-quoted `~` path always fails as
  file-not-found. Resolve the whole path to its absolute form first (for
  example, substitute the leading `~` with the value of `$HOME` yourself),
  then single-quote the entire resolved path as one token:
  `emterm image -- '/absolute/path/photo.png'`. This is the invariant
  with no exception: every byte of the path is either inside that
  single-quoted span or is part of the fixed four-character `'\''` splice
  described next; nothing else path-derived ever appears outside the
  quotes.
- If the path itself contains a single quote, close the quote, insert
  `'\''` (end-quote, escaped literal quote, reopen-quote), then continue:
  a path `it's.png` becomes `'it'\''s.png'`. Those four characters are
  fixed literal syntax, not bytes derived from the path, so they satisfy
  the invariant above rather than being an exception to it.
- Double quotes are NOT a safe substitute for single quotes: `$(...)`
  command substitution, backtick command substitution, and `${...}`
  parameter expansion all still expand inside double quotes. Only single
  quotes suppress all shell expansion of the path.
- If a no-shell exec path is available (e.g. an argv-array invocation such
  as Bun's `Bun.spawn`-style array form), passing the path as a single argv
  element is an equally safe alternative to the quoted-and-delimited shell
  form above.

### Adversarial examples

1. Path is `photo.png; touch PWNED`:
   - **Safe**: `emterm image -- 'photo.png; touch PWNED'` — `emterm`
     receives the literal path `photo.png; touch PWNED` as its file
     argument and reports the file as not found (no such file exists). The
     `; touch PWNED` fragment is inert: it is data inside the quotes, not
     shell syntax.
   - **Unsafe** (bare, unquoted path): `emterm image photo.png; touch
     PWNED` — the shell parses the unquoted `;` as a command separator and
     runs `touch PWNED` as a second command, creating a file the user
     never asked for.
2. Path is `photo$(touch PWNED).png`:
   - **Safe** (single-quoted):
     `emterm image -- 'photo$(touch PWNED).png'` — `emterm` receives the
     literal path `photo$(touch PWNED).png` and reports it as not found.
     The `$(...)` fragment is inert data inside the single quotes; no
     command runs.
   - **Unsafe** (double-quoted):
     `emterm image -- "photo$(touch PWNED).png"` — the shell evaluates
     `$(touch PWNED)` before `emterm` ever runs, creating `PWNED` as a
     side effect and passing `emterm` the substituted path `photo.png`.
