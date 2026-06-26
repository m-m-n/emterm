# Feature: Claude Code AltScreen UX Improvements

## Overview

eMterm currently fails to support three terminal features that Claude Code's
alternate-screen ("fullscreen") UI relies on: DECSET 1007 (alternate_scroll),
xterm-style CSI modifier sequences, and host-side handling of OSC 8 hyperlinks.
This feature adds all three so that Claude Code's mouse wheel, modified
navigation keys, and PR-ID hyperlinks behave the same as on WezTerm.

## Objectives

- Forward mouse wheel events as arrow-key presses to the PTY while the
  alternate screen is active (DEC private mode 1007).
- Encode `Ctrl` / `Shift` / `Alt` modified Home / End / PageUp / PageDown /
  Arrow / Function keys using the xterm CSI modifier extension
  (`ESC[1;<mods>X` / `ESC[<n>;<mods>~`).
- Render OSC 8 hyperlinked cells as clickable links: underline + hand cursor on
  `Ctrl + hover`, open the URI in the OS opener on `Ctrl + click`.

## User Stories

### US1: Scroll Claude Code logs with the mouse wheel

As a Claude Code user, I want the mouse wheel to scroll Claude Code's log /
dialog while the terminal is in alternate-screen mode, so I don't have to use a
keyboard chord for every scroll.

**Acceptance Criteria:**
- [ ] In Claude Code (AltScreen), wheel-up sends three `ESC[A` bytes (and
  wheel-down sends `ESC[B`) per notch.
- [ ] On the main screen, the wheel still moves eMterm's scrollback view (no
  PTY bytes are sent).

> **Note**: eMterm currently has no host-side mouse-reporting path (DECSET
> 1000/1002/1003/1006 fall through `MODE_ACTION_TS_FALLBACK` without a
> consumer), so wheel events never compete with mouse reports today. If
> mouse-reporting is added later, that path will need its own AltScreen
> precedence policy; until then the alternate_scroll branch fires whenever
> the AltScreen + setting + DECSET 1007 gates allow.

### US2: Jump with Ctrl+Home / Ctrl+End in Claude Code

As a Claude Code user, I want `Ctrl+Home` / `Ctrl+End` / other modified
navigation keys to reach Claude Code so I can jump to start/end of log.

**Acceptance Criteria:**
- [ ] `Ctrl+Home` sends `ESC[1;5H`, `Ctrl+End` sends `ESC[1;5F`,
  `Ctrl+PageUp` sends `ESC[5;5~`.
- [ ] Modifier-free Home/End/PageUp/PageDown/Arrow/F-keys keep their existing
  short encodings (`ESC[H`, `ESC[5~`, `ESC[A`, …).
- [ ] Existing host-level scrollback chords (`Shift+PageUp/Down/Home/End`)
  still intercept before the PTY path; no regression.

### US3: Click PR-ID links in Claude Code

As a Claude Code user, I want PR IDs like `#1` to behave like real links so I
can open the PR in my browser.

**Acceptance Criteria:**
- [ ] Cells with `cell.hyperlink_id != 0` show an underline + hand cursor on
  `Ctrl + hover`.
- [ ] `Ctrl + click` on such a cell opens `hyperlink_table[id].uri` via the OS
  opener after passing `links::is_safe_uri`.
- [ ] The behaviour is **enabled inside the alternate screen** (unlike the
  regex-based URL detector, which stays AltScreen-disabled).

### US4: Opt out of DECSET 1007

As a user who prefers eMterm's classic wheel-to-scrollback behaviour, I want a
setting that turns the alternate_scroll translation off.

**Acceptance Criteria:**
- [ ] `Settings → Terminal` exposes an `alternate_scroll_enabled` toggle
  (default ON).
- [ ] When OFF, AltScreen + wheel does **not** emit arrow keys (and the host
  falls back to the current scrollback-move behaviour, which is effectively a
  no-op inside AltScreen).

## Technical Requirements

### Functional Requirements

- **FR1: DECSET 1007 (alternate_scroll)** — implement DEC private mode 1007 in
  `term_core` (default ON on terminal creation), gate the host wheel handler
  on `app.alt_screen && term_core.alternate_scroll && alternate_scroll_enabled`,
  and send `ESC[A` / `ESC[B` × `3 × notches` over the PTY. Treat `Shift+wheel`
  identically to plain wheel (xterm-compatible). The `alternate_scroll_enabled`
  user setting is the initial value used when starting a new terminal session,
  and remains the host-side opt-out switch at runtime. (No mouse-reporting
  competition today — see US1's note.)

- **FR2: CSI modifier extension** — extend `src-tauri/src/pty/input.rs`
  `encode()` to take the current `Modifiers` (Ctrl/Shift/Alt) and, when at
  least one modifier is held, emit the xterm CSI form:

  - `Arrow{Up|Down|Right|Left}` → `ESC[1;<mods>{A|B|C|D}`
  - `Home` → `ESC[1;<mods>H`
  - `End`  → `ESC[1;<mods>F`
  - `PageUp` → `ESC[5;<mods>~`
  - `PageDown` → `ESC[6;<mods>~`
  - `Insert` → `ESC[2;<mods>~`
  - `Delete` → `ESC[3;<mods>~`
  - `F1`-`F4` → `ESC[1;<mods>{P|Q|R|S}` (legacy xterm SS3 cousins use the same
    1;mods prefix in modifier form)
  - `F5`-`F12` → `ESC[<n>;<mods>~` (n = 15/17/18/19/20/21/23/24)

  When no modifier is held, keep the existing legacy encoding bytes
  unchanged. The new path applies to both `Target::HostPty` and
  `Target::PosixPty` (matching the existing modifier-less path).

- **FR3: OSC 8 host-side support** — surface `cell.hyperlink_id` and the
  `hyperlink_table` to the host. Add a hyperlink lookup helper on the host
  side that returns a URI for a given screen cell. Wire it into:
  - the link-hover path in `src-tauri/src/window_host.rs` (around lines 680 /
    725 / 821): on `Ctrl + hover` over an OSC 8 cell, draw the underline and
    switch to the hand cursor regardless of AltScreen state;
  - the click handler (`try_open_link_at_pointer` and equivalents): on
    `Ctrl + click`, prefer the OSC 8 URI over the regex match, validate via
    `links::is_safe_uri`, then call the existing OS opener.

### Non-Functional Requirements

- **NFR1 - Performance:** wheel translation must add < 100µs latency per
  event; modifier encoding < 50µs per key; OSC 8 hit-test < 200µs per mouse
  move (cell-direct lookup, O(1)).
- **NFR2 - Security:** all OSC 8 URIs must pass `links::is_safe_uri` before
  being handed to the opener. Unsafe URIs (`javascript:`, `data:`, etc.) are
  logged with `log::warn!` and dropped.
- **NFR3 - Compatibility:** behaviour must match xterm / WezTerm / Alacritty
  for DECSET 1007 (AltScreen-only, Shift ignored, 3 lines per notch) and the
  xterm CSI modifier convention.
- **NFR4 - User control:** `alternate_scroll_enabled` setting opts out of
  FR1 without affecting FR2 or FR3.

## Implementation Approach

### Architecture

eMterm already has the three subsystems involved; this feature is a wiring
job, not new infrastructure.

```
┌───────────────────────────────────────────────────────────────────┐
│ winit                                                             │
│  ├─ WindowEvent::MouseWheel  ──┐                                  │
│  └─ WindowEvent::KeyboardInput ┼──► window_host (host bindings)   │
│                                │                                  │
│ host (src-tauri/src)           ▼                                  │
│  ├─ window_host.rs       ─► pty/input.rs::encode (FR2)            │
│  │                       ─► alternate_scroll path     (FR1)       │
│  │                       ─► hyperlink hit-test        (FR3)       │
│  └─ render/*.rs          ─► underline OSC 8 cells     (FR3)       │
│                                                                   │
│ term_core (crates/term_core)                                      │
│  ├─ csi_modes.rs         ─► DECSET 1007 bit & accessor (FR1)      │
│  ├─ cell.rs              ─► cell.hyperlink_id (already)           │
│  └─ snapshot.rs          ─► hyperlink_table replay (already)      │
└───────────────────────────────────────────────────────────────────┘
```

### Data Flow

**FR1 wheel → PTY:**
```
MouseWheel → window_host::on_mouse_wheel
  └── if app.alt_screen && core.alternate_scroll && alternate_scroll_enabled
       → pty.write(b"\x1b[A".repeat(notches*3))   // or [B]
       else
       → app.scroll_up_by/scroll_down_by          // existing path
```

**FR2 modified key → PTY:**
```
KeyboardInput → window_host (chord check) → pty::input::encode(key, mods, target)
  └── if mods != NONE && key in MODIFIER_KEYS
       → ESC[<base>;<mods>X
       else
       → existing legacy bytes
```

**FR3 OSC 8 click:**
```
MouseInput(Pressed, Ctrl) → window_host::try_open_link_at_pointer
  ├── cell = screen.cell_at(pointer_row, pointer_col)
  ├── if cell.hyperlink_id != 0
  │    uri = core.hyperlink_table.get(cell.hyperlink_id)
  │    if links::is_safe_uri(uri) → open::that(uri)
  └── else
       → existing regex link::find_link_at path (skipped in AltScreen as today)
```

### File Structure

```
crates/term_core/
  src/
    csi_modes.rs           # add DECSET 1007 bit + alternate_scroll() getter
    cell.rs                # (existing) hyperlink_id
    snapshot.rs            # (existing) hyperlink_table serialization
    terminal_dispatch.rs   # (existing) cell hyperlink_id assignment

src-tauri/
  src/
    pty/
      input.rs             # FR2: extend encode() to take Modifiers and
                           # emit CSI-modifier sequences
    window_host.rs         # FR1: alternate_scroll branch in mouse-wheel handler
                           # FR3: OSC 8 hyperlink hover/click branch
    render/
      <hyperlink draw>.rs  # FR3: underline OSC 8 cells on Ctrl+hover
    app.rs                 # FR1: pass alternate_scroll setting to TerminalCore;
                           # cache alternate_scroll_enabled setting value
    settings.rs            # FR1: expose alternate_scroll_enabled (re-export from
                           # app_settings)

crates/app_settings/
  src/lib.rs               # FR1: add alternate_scroll_enabled: bool field

src-tauri/web-shared/
  settings/types.ts        # FR1: mirror AppSettings.alternate_scroll_enabled
src-tauri/settings/web/
  <settings UI panel>      # FR1: add toggle to Terminal section, with i18n
```

### Dependencies

**Internal:**
- `term_core` — owns DEC modes, cells, hyperlink table (already in place).
- `links::is_safe_uri` — existing host-side URI validator, reused as-is.
- `app_settings::AppSettings` — settings struct extended by one bool.

**External:**
- No new crates. `open` crate (or whichever is already used) for OS opener.

## Test Scenarios

### Unit Tests (Rust, `cargo test --lib`)

- [ ] `term_core::csi_modes::alternate_scroll_default_on`
- [ ] `term_core::csi_modes::decset_1007_sets_bit`
- [ ] `term_core::csi_modes::decrst_1007_clears_bit`
- [ ] `pty::input::encode_ctrl_home_sends_csi_modifier`
- [ ] `pty::input::encode_ctrl_end_sends_csi_modifier`
- [ ] `pty::input::encode_ctrl_pageup_sends_csi_modifier`
- [ ] `pty::input::encode_ctrl_arrow_up_sends_csi_modifier`
- [ ] `pty::input::encode_shift_f1_sends_csi_modifier`
- [ ] `pty::input::encode_plain_home_unchanged` (regression guard)
- [ ] `pty::input::encode_plain_pageup_unchanged` (regression guard)
- [ ] Host: `wheel_in_alt_screen_emits_arrow_keys` (with DECSET 1007 ON)
- [ ] Host: `wheel_in_alt_screen_suppressed_when_setting_off`
- [ ] Host: `wheel_in_main_screen_moves_scrollback`
- [ ] Host: `wheel_with_mouse_reporting_takes_mouse_path`
- [ ] Host: `shift_wheel_in_alt_screen_emits_arrow_keys` (Shift ignored)
- [ ] Host: `osc8_cell_hover_with_ctrl_underlines_in_alt_screen`
- [ ] Host: `osc8_cell_ctrl_click_opens_uri_after_is_safe_uri`
- [ ] Host: `osc8_cell_ctrl_click_with_unsafe_uri_logs_warn_and_skips`
- [ ] Host: `osc8_id_missing_from_table_falls_back_to_noop`

### Integration Tests

- [ ] `src-tauri/tests/cli_subcommands.rs` parity (no regression).
- [ ] mux/snapshot replay path keeps `cell.hyperlink_id` round-trip intact
  (existing snapshot tests should continue to pass; add one explicit test if
  the round-trip is not already covered).

### E2E Tests

None — this project has no E2E harness (`test/README.md`).
End-to-end behaviour is validated manually:

- [ ] In `claude` (the Claude Code CLI), confirm wheel scroll works.
- [ ] In `claude`, confirm `Ctrl+Home` / `Ctrl+End` jump to start/end.
- [ ] In `claude`, confirm a PR ID `#1` underlines on `Ctrl+hover` and opens
  the GitHub PR on `Ctrl+click`.
- [ ] In `vim`, confirm wheel scrolls the file.
- [ ] In `less`, confirm wheel scrolls.

### Edge Cases

- [ ] AltScreen + DECSET 1007 ON + mouse reporting ON → mouse-report path
  wins, no arrow keys emitted.
- [ ] DECSET 1007 dynamically toggled by the application
  (`ESC[?1007h` / `ESC[?1007l`) during a session is honoured immediately on
  the next wheel event.
- [ ] User toggles `alternate_scroll_enabled` in settings while AltScreen is
  active — the next wheel event reflects the new value.
- [ ] OSC 8 cell at the edge of the viewport — hit-test uses the same cell
  coordinate path as existing hover.
- [ ] OSC 8 sequence with empty URI → treated as "no link" (no underline, no
  click).

### Performance Tests

- [ ] Wheel handler hot path: micro-benchmark with `--include-ignored`-style
  ignored bench is optional; only add one if regression is suspected.

## Security Considerations

- **Input Validation:** OSC 8 URIs are validated by `links::is_safe_uri`
  before being passed to the OS opener. Unsafe schemes (`javascript:`,
  `data:`, etc.) are dropped with a `log::warn!`.
- **No new network/IO surfaces:** all changes are local key/mouse encoding +
  local opener calls.
- **PTY byte injection safety:** the new arrow-key bytes for FR1 are a fixed
  byte slice; no user-controlled string interpolation.

## Error Handling

| Code | Description | Surface | User Message |
|------|-------------|---------|--------------|
| OSC8_UNSAFE_URI | OSC 8 URI failed `is_safe_uri` | log::warn! only | (silent — no UI surface; behaves as "no link") |
| OSC8_ID_MISSING | `hyperlink_id` not in table (evicted) | no log | (silent — behaves as "no link") |
| OPENER_FAILED | OS opener returned error | log::warn! | (silent — already the policy for existing URL clicks) |

## Performance Optimization

- O(1) cell-direct lookup for OSC 8 hit-test (no scan).
- Pre-sized 6-byte buffer for the longest CSI modifier sequence
  (`ESC[24;8~`).
- Wheel translation uses `extend_from_slice` with a static 3-byte template per
  notch.

## Success Criteria

- [ ] All functional requirements implemented and unit-tested.
- [ ] Manual verification list under "E2E Tests" passes.
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  passes (full suite, including new tests).
- [ ] `bun run typecheck` passes.
- [ ] `cargo check --no-default-features` still compiles (CLI-only build
  unaffected).
- [ ] `alternate_scroll_enabled = false` reproduces pre-change wheel
  behaviour.

## Open Questions

None — all three originally-unresolved decisions were closed during
requirements gathering (Shift+wheel = ignore, F1 scope = AltScreen-only,
F3 underline = Ctrl+hover only).

## Implementation Phases

### Phase 1: DECSET 1007 (FR1)

**Goals:** wheel-to-arrow translation in AltScreen with user toggle.

**Deliverables:**
- `term_core` DECSET 1007 mode bit + getter.
- `app_settings::alternate_scroll_enabled` field + TS mirror + UI toggle.
- `window_host::on_mouse_wheel` branch + unit tests.

### Phase 2: CSI modifier extension (FR2)

**Goals:** modified key emission matching xterm.

**Deliverables:**
- `pty::input::encode()` extended for modifier-aware paths.
- Unit tests covering Ctrl / Shift / Alt across Home / End / PgUp / PgDn /
  Arrow / F-keys.

### Phase 3: OSC 8 host wiring (FR3)

**Goals:** clickable hyperlinks for OSC 8 cells.

**Deliverables:**
- Host-side hyperlink hit-test helper.
- Renderer underline path for `cell.hyperlink_id != 0` cells (Ctrl+hover).
- Click handler `Ctrl+click` → `is_safe_uri` → opener.
- Unit tests (hover + click + unsafe URI + missing id).

## References

- 要件定義書: `doc/tasks/claude-code-altscreen-ux/要件定義書.md`
- Discussion: `tmp/discussion-claude-code-altscreen-and-pr-link.md`
- Claude Code Issue #42002 (AltScreen confirmation)
- Claude Code Issue #27047 (OSC 8 emission)
- xterm Control Sequences (DECSET 1007, CSI modifier convention)
- OSC 8 hyperlink spec
