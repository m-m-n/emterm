---
name: display-image
description: Renders an image file inline in eMterm using the Kitty Graphics Protocol or SIXEL. Use when the user asks to view, preview, or display an image file (e.g. .png, .jpg) inside eMterm, rather than opening it in an external application.
---

# Display an Image in eMterm

Run the eMterm CLI on the image file the user gave you:

```
emterm image <file> [--protocol kitty|sixel]
```

Replace `<file>` with the path to the image file. `emterm` writes a display
escape sequence to stdout; eMterm's terminal intercepts it and renders the
image inline.

The `--protocol` argument is optional and selects the inline-image protocol:
`kitty` (the default) or `sixel`. Omit it unless the user specifies a
protocol.

## Argument-injection safety (required)

The file path may originate from an untrusted source (a path pasted by the
user, extracted from another file, or produced by earlier tool output). A
contributor-controlled path such as `photo.png; touch PWNED` would, if
interpolated into a shell command string, execute the appended command
instead of being treated as a filename.

- The path MUST be passed as a single argv element through a no-shell
  invocation (e.g. Bun's `Bun.spawn`-style array form, or any
  exec-without-shell call) — never interpolated into a joined shell command
  string.
- **If a shell is unavoidable**, the path must be shell-quoted, and an
  end-of-options delimiter (`--`) must be used where the command supports
  it, so the path can never be parsed as a flag or as shell syntax.

### Adversarial example

Path is `photo.png; touch PWNED`:

- **Safe** (single argv element / no-shell invocation, or `--` +
  shell-quoted): `emterm` receives the literal path `photo.png; touch
  PWNED` as its `<file>` argument, and reports the file not found (there is
  no such file). The `; touch PWNED` fragment is inert — it is data, not
  shell syntax.
- **Unsafe** (path spliced into a shell command string): the shell parses
  `;` as a command separator and runs `touch PWNED` as a second command,
  creating a file the user never asked for.
