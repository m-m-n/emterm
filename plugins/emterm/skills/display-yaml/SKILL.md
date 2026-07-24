---
name: display-yaml
description: Renders a YAML file in eMterm's child data viewer window. Use when the user asks to view, preview, or display a YAML (.yaml or .yml) file inside eMterm, rather than reading its raw text.
---

# Display YAML in eMterm

Run the eMterm CLI on the YAML file the user gave you:

```
emterm yaml <file>
```

Replace `<file>` with the path to the YAML file. `emterm` writes a display
escape sequence to stdout; eMterm's terminal intercepts it and opens the
formatted YAML in a child data viewer window.
