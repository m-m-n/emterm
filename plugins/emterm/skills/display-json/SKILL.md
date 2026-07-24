---
name: display-json
description: Renders a JSON file in eMterm's child data viewer window. Use when the user asks to view, preview, or display a JSON (.json) file inside eMterm, rather than reading its raw text.
---

# Display JSON in eMterm

Run the eMterm CLI on the JSON file the user gave you:

```
emterm json <file>
```

Replace `<file>` with the path to the JSON file. `emterm` writes a display
escape sequence to stdout; eMterm's terminal intercepts it and opens the
formatted JSON in a child data viewer window.
