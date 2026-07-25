---
name: display-yaml
description: Renders a YAML file in eMterm's child data viewer window. Use when the user asks to view, preview, or display a YAML (.yaml or .yml) file inside eMterm, rather than reading its raw text.
---

# Display YAML in eMterm

Run the eMterm CLI on the YAML file the user gave you:

```
emterm yaml -- '<path>'
```

Replace `<path>` with the path to the YAML file, single-quoted and placed
after the end-of-options delimiter `--`. `emterm` writes a display escape
sequence to stdout; eMterm's terminal intercepts it and opens the
formatted YAML in a child data viewer window.

## Argument-injection safety (required)

The file path may originate from an untrusted source (a path pasted by the
user, extracted from another file, or produced by earlier tool output). A
path such as `report.yaml; touch PWNED`, if placed bare and unquoted on the
command line, would let the shell treat `; touch PWNED` as a second command
instead of part of the filename.

- Always single-quote the path and place it after the end-of-options
  delimiter `--`, never as a bare unquoted argument:
  `emterm yaml -- '<path>'`.
- eMterm's CLI is clap-based and accepts `--` as the end-of-options marker
  on this subcommand — everything after it is a positional argument, never
  parsed as a flag, regardless of what characters the path contains.
- If the path itself contains a single quote, close the quote, insert
  `'\''` (end-quote, escaped literal quote, reopen-quote), then continue:
  a path `it's.yaml` becomes `'it'\''s.yaml'`.
- If a no-shell exec path is available (e.g. an argv-array invocation such
  as Bun's `Bun.spawn`-style array form), passing the path as a single argv
  element is an equally safe alternative to the quoted-and-delimited shell
  form above.

### Adversarial example

Path is `report.yaml; touch PWNED`:

- **Safe**: `emterm yaml -- 'report.yaml; touch PWNED'` — `emterm`
  receives the literal path `report.yaml; touch PWNED` as its file argument
  and reports the file as not found (no such file exists). The `; touch
  PWNED` fragment is inert: it is data inside the quotes, not shell syntax.
- **Unsafe** (bare, unquoted path): `emterm yaml report.yaml; touch PWNED`
  — the shell parses the unquoted `;` as a command separator and runs
  `touch PWNED` as a second command, creating a file the user never asked
  for.
