# Threat Model: settings-profile-remove-launch

## Verdict
no-trust-boundary

## Rationale
Inspected: the change set of task0001 — the Launch action block in
`src-tauri/web-shared/settings/sections/profiles-section.ts`, the
`settings.profiles.launch` entries of `src-tauri/web-shared/i18n/locales/en.json`
and `ja.json`, and the colocated test file. Tier: full. Task domains: `ui`
only; none of `auth`, `input-handling`, `external-io` or `data-persistence`
applies, so no part of the change is analysed at deep depth.

The feature only deletes code and one locale key. It reads no new input,
adds no IPC message, file, network or process interaction, and changes no
privilege. The removed `profile:launch` event was dispatched inside the
settings WebView document and had no listener on the TypeScript or Rust side
(REQUIREMENTS.md 14.1; `settings-panel.ts`, `settings/web/entry.ts` and
`settings_window/commands.rs` contain no reference to it), so its removal
changes no data flow across the WebView-to-native IPC boundary. The remaining
profile rendering and actions (FR4) are not modified. No data crosses between
parties of different trust as a result of this feature.
