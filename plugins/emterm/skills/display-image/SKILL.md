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
