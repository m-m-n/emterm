# Threat Model: new-tab-menu-shortcut-hints

## Verdict
no-trust-boundary

## Rationale
Inspected: SPEC.md, REQUIREMENTS.md and the change set of task0001 —
`src-tauri/src/ui/keybinds.rs` (chord label formatting and the
reachable-label query), `src-tauri/src/ui/profile_selector.rs` (row-label
assignment and the row painter), `src-tauri/src/app/chooser.rs` (App-level
label query), `src-tauri/src/render/overlays.rs` (row wiring) and their test
files. Tier: full. Task domains: `ui` only; none of `auth`,
`input-handling`, `external-io` or `data-persistence` applies, so no part of
the change is analysed at deep depth.

The feature reads only in-process state: the already-resolved keybind table
(`App.keybinds`), the `is_default` flags of `settings.profiles`, and the
selector's own mode and tmux row count. It reads no file, IPC message,
network response or process output of its own. The settings.json keybind
specs are parsed into the table by the existing `KeybindTable::from_settings`
/ `parse_chord` path, which this feature does not change; the feature
consumes only that path's resolved chords. The label text it produces comes
from a closed vocabulary (the modifier names Ctrl / Shift / Alt and the fixed
main-key tokens `parse_chord` accepts) and is painted only in the same
process's egui dialog, for the same user whose settings produced it. Profile
names and shell paths are rendered by the existing dialog as before. No
privilege changes. No data crosses between parties of different trust as a
result of this feature (SPEC.md Security Considerations: 該当なし).
