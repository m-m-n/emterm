---
name: display-markdown
description: Renders a Markdown file in eMterm's child Markdown viewer window. Use when the user asks to view, preview, render, or display a Markdown (.md) file inside eMterm, rather than reading its raw text.
---

# Display Markdown in eMterm

Run the eMterm CLI on the Markdown file the user gave you:

```
emterm markdown <file>
```

Replace `<file>` with the path to the Markdown file. `emterm` writes a
display escape sequence to stdout; eMterm's terminal intercepts it and opens
the rendered Markdown in a child viewer window.
