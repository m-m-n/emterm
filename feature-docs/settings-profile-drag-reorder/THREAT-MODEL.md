# Threat Model: settings-profile-drag-reorder

## Verdict
threats-identified

## Rationale
Inspected: SPEC.md, REQUIREMENTS.md, and the two drag-reorder setups in
`src-tauri/web-shared/settings/sections/profiles-section.ts` and
`src-tauri/web-shared/settings/sections/ssh-section.ts`. Tier: full. Both
tasks (task0001, task0002) declare only the `ui` domain; none of `auth`,
`input-handling`, `external-io` or `data-persistence` applies, so the
boundary below is analysed at standard depth.

The feature makes each list's dragstart offer drag data through the desktop
drag-and-drop channel, which other applications can receive, and each list's
drop handler already receives drops that did not start on that list. That
channel is one trust boundary (TB-1), with one Tampering and one Information
disclosure threat.

Not a new crossing: persistence through `saveSetting` sends the same
`profiles` / `ssh_connections` arrays over the existing settings IPC path, and
the Rust/wry side is unchanged (NFR1). The `.ssh/config` host list is not
touched by this feature. Domain consistency re-check: no task declares one of
the four depth domains, and both tasks' source files appear in the TB-1
Boundary files line.

## Trust Boundaries

### TB-1: Desktop drag-and-drop channel around the settings lists
Crossing: drag data offered by a settings list item to any drop target on the
desktop (another application or window), and drop events delivered to a
settings list from drags that did not start on that list (another
application, another window, or the other settings list).
Boundary files: src-tauri/web-shared/settings/sections/profiles-section.ts, src-tauri/web-shared/settings/sections/ssh-section.ts
Depth: standard

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | A drop that did not start on the same list, such as another application's drag carrying the list's custom MIME type, drives a reorder that is persisted through `saveSetting` (FR1, FR2, FR4) | TM-1 | The drop handler acts only on the in-memory `dragIndex` set by a dragstart on the same list, returns when it is null, and never reads data from the drop event's DataTransfer | task0001 AC-4, task0002 AC-4 | VERIFICATION.md, Performance / Security Verification, TM-1 |
| Information disclosure | The drag data is offered to whatever application receives the drop; a payload carrying profile or SSH connection details, or a `text/plain` entry, would hand those details to that application (FR1, FR2, FR3) | TM-2 | The dragstart handler sets exactly one entry: the list's custom MIME type holding the item's index as a decimal string, with no `text/plain` entry and no other type | task0001 AC-2, task0002 AC-2 | VERIFICATION.md, Performance / Security Verification, TM-2 |
