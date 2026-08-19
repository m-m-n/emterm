# UI Design

The UI follows a Material Design 3 baseline. Design tokens (color / typescale /
shape / motion / elevation) live in `doc/UI-DESIGN-GUIDELINES.yaml`.

## Single source of truth

`doc/UI-DESIGN-GUIDELINES.yaml` is normative. Two layers mirror it:

| Layer | Location |
| --- | --- |
| Native (egui) | `src-tauri/src/ui/md3.rs` |
| WebView (CSS) | `src-tauri/web-shared/styles.css` — `--md-sys-*` variables |

Dialogs have their own normative section (`dialogs:`), mirrored by
`src-tauri/src/ui/dialog/` (native) and `src-tauri/web-shared/dialog/`
(WebView). Drift between the yaml, the Rust constants, and the CSS variables is
caught by `ui::dialog::tests`.

## Changing a token

Edit the yaml first, then propagate to both mirrors. A change applied to only
one mirror fails the drift tests.
