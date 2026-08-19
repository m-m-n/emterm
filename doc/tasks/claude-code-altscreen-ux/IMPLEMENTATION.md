# Implementation Plan: Claude Code AltScreen UX Improvements

## Overview

Add three terminal capabilities that Claude Code's alternate-screen UI relies
on — DECSET 1007 wheel-to-arrow translation, xterm CSI modifier sequences for
modified navigation keys, and host-side OSC 8 hyperlink interaction — so
Claude Code's mouse wheel, `Ctrl+Home`/`End`, and PR-ID links behave the same
as on WezTerm.

## Objectives

- Forward AltScreen mouse wheel events as arrow-key bytes to the PTY when DEC
  private mode 1007 is enabled.
- Encode `Ctrl` / `Shift` / `Alt` modified Home / End / PageUp / PageDown /
  Arrow / Function keys using the xterm `ESC[<n>;<mods>X` convention.
- Surface `cell.hyperlink_id` and the `hyperlink_table` to the host so OSC 8
  cells underline on `Ctrl+hover` and open the URI on `Ctrl+click`.

## Prerequisites

### Development Environment

- Rust toolchain pinned by the project (rustfmt style_edition=2024).
- Bun (TypeScript bundle / tests for child WebViews).
- The build-location rule: always pass `--manifest-path` and
  `CARGO_TARGET_DIR` (see `.claude/rules/core-build-location.md`).

### Dependencies

- `term_core` already exposes `get_cell_hyperlink_id(col, row)` and
  `get_hyperlink_uri(id)` — no new accessor work needed.
- `links::is_safe_uri` exists and is reused unchanged for FR3.
- `crates/app_settings::AppSettings` uses the `deserialize_null_with!` pattern
  for fields with custom defaults — FR1's new bool follows that pattern.
- No new external crates.

## Architecture Overview

### Technology Stack

- **Language**: Rust (`src-tauri` + `crates/*`), TypeScript (child WebView
  settings panel for FR1's toggle UI).
- **Framework**: winit (event loop), wgpu (GPU surface), egui (in-process UI),
  swash (font rasterization), wry (settings panel WebView).
- **Key components**:
  - `crates/term_core/src/csi_modes.rs` — DEC private mode handler.
  - `crates/term_core/src/terminal_core.rs` — mode bit layout, defaults.
  - `crates/term_core/src/terminal_cells.rs` — cell + hyperlink table
    accessors (already present).
  - `src-tauri/src/pty/input.rs` — host-side key→bytes encoder.
  - `src-tauri/src/window_host.rs` — winit handlers (mouse / keyboard / link
    hover / link click).
  - `src-tauri/src/render/` — wgpu/egui renderer; underline draw path.
  - `crates/app_settings/src/settings.rs` — `AppSettings` struct.
  - `src-tauri/web-shared/settings/*` — TS settings UI + mirror types.

### Design Approach

- Layer the new behaviour onto the *existing* code paths rather than adding
  parallel ones: the wheel handler already branches on
  `profile_selector.visible` and a tab-strip band before it falls through to
  `app.scroll_up_by/down_by`; the new alternate_scroll branch slots in right
  before that final fall-through. The OSC 8 hover / click branches slot in
  before the existing regex link path in the same handlers.
- `term_core` owns the **mode bit** (DECSET 1007 state) and the **default**
  (ON at terminal-construction time). The host reads the mode bit through a
  small accessor and combines it with the user setting + AltScreen state +
  mouse-reporting state to decide whether to translate.
- `pty::input::encode()` keeps its current public signature only if the
  caller-side change is small; otherwise add a modifier-aware path that the
  existing call sites opt into. Either way, the *plain* (modifier-less) byte
  sequences are byte-identical to today, preserving the existing test
  expectations.
- OSC 8 host wiring stays *independent* of the regex URL/path detector. The
  AltScreen guard on the regex path is unchanged; OSC 8 is a separate code
  path that runs regardless of AltScreen.

### Component Interaction

```
            ┌────────────────────────────────────────────────┐
            │ winit                                          │
            └─────┬────────────────────────────┬─────────────┘
                  │ MouseWheel                 │ KeyboardInput
                  ▼                            ▼
        ┌─────────────────────┐     ┌─────────────────────┐
        │ window_host         │     │ window_host         │
        │  wheel handler      │     │  chord / encode     │
        └───┬────────────┬────┘     └─────────┬───────────┘
            │            │                    │
            │            ▼ AltScreen+1007+OK  ▼ modifier present
            │     ┌───────────────┐    ┌────────────────────┐
            │     │ pty.write     │    │ pty::input::encode │
            │     │ arrow bytes   │    │ CSI modifier form  │
            │     └───────────────┘    └────────────────────┘
            ▼ otherwise
        ┌────────────────────────────────────────────────────┐
        │ app.scroll_up_by/down_by (existing scrollback view)│
        └────────────────────────────────────────────────────┘

            ┌────────────────────────────────────────────────┐
            │ winit  CursorMoved / MouseInput(Ctrl+click)    │
            └─────┬──────────────────────────────────────────┘
                  │
                  ▼
        ┌──────────────────────────────────────────────────────┐
        │ window_host                                          │
        │   1. cell = (row, col) under pointer                 │
        │   2. id = core.get_cell_hyperlink_id(col, row)       │
        │   3. if id != 0 → uri = core.get_hyperlink_uri(id)   │
        │      → underline + hand cursor / opener on click     │
        │   4. else → existing regex link path                 │
        └──────────────────────────────────────────────────────┘
```

## Implementation Phases

### Phase 1: DECSET 1007 (FR1)

**Goal**: Convert AltScreen wheel events into arrow-key bytes when DEC mode
1007 is enabled, with a user-facing on/off setting.

**Files to Create**:

- _None_ — every change lives in existing files.

**Files to Modify**:

- `crates/term_core/src/terminal_core.rs` — add a new mode bit constant for
  alternate_scroll alongside the existing `MODE_*` constants; include it in
  the "set on construction" default bitmask so a fresh `TerminalCore` has
  alternate_scroll = ON.
- `crates/term_core/src/csi_modes.rs` — add a `1007` arm to
  `handle_set_mode()` that toggles the new mode bit and returns
  `MODE_ACTION_NONE`.
- `crates/app_settings/src/settings.rs` — add an `alternate_scroll_enabled:
  bool` field to `AppSettings` following the existing
  `deserialize_null_with!` + `default_*` helper pattern; include the field in
  the snapshot/restore test fixture so the round-trip test stays green.
- `src-tauri/src/window_host.rs` — in the `WindowEvent::MouseWheel` arm,
  insert a new branch *before* the existing `app.scroll_up_by/down_by`
  fall-through. The branch fires when ALL of these are true: AltScreen is
  active, the `alternate_scroll_enabled` setting is ON, and the terminal-side
  mode bit is ON. When it fires, write arrow-key bytes to the active PTY
  (`ESC[A` for wheel-up, `ESC[B` for wheel-down) sized by the `notches × 3`
  rule. When it doesn't fire, the existing scrollback-view branch runs
  unchanged. (No mouse-reporting gate: eMterm has no host-side mouse-report
  path today — DECSET 1000/1002/1003/1006 fall through `MODE_ACTION_TS_FALLBACK`
  without a consumer, so there is nothing to compete with the wheel.)
- `src-tauri/web-shared/settings/types.ts` — mirror the new field in the
  `AppSettings` interface.
- `src-tauri/web-shared/settings/sections/terminal-behavior-section.ts` —
  add a toggle for `alternate_scroll_enabled` adjacent to `scroll_speed`,
  saving via the existing `ctx.saveSetting` helper.
- `src-tauri/web-shared/i18n/locales/en.json`,
  `src-tauri/web-shared/i18n/locales/ja.json` — add the toggle label and
  description strings.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `TerminalCore` mode bit `MODE_ALTERNATE_SCROLL` | Hold the per-terminal DECSET 1007 state | Bit set on construction (default ON) | Read by host via a getter (`get_mode`) |
| `csi_modes` arm `1007` | Toggle mode bit on `ESC[?1007h`/`l` | Caller invokes `handle_set_mode(1007, …)` | Mode bit reflects new value; action = NONE |
| `AppSettings.alternate_scroll_enabled` | Persisted user opt-out (default true) | Settings JSON loaded | Wheel branch reads it as the gate |
| `window_host` wheel branch | Decide AltScreen wheel → arrows vs scrollback | wheel event reached terminal area | bytes written to active PTY OR scrollback view moved |
| Settings panel toggle | Surface the bool to the user | Settings UI rendered | `ctx.saveSetting("alternate_scroll_enabled", v)` persists |

**Processing Flow**:

1. Mouse wheel event arrives at the terminal area (not over modal / tab strip).
2. Read `app.alt_screen`, the terminal-side mode bit, and the user setting.
3. Branch:
   - AltScreen AND mode bit ON AND setting ON → write `ESC[A`/`ESC[B` ×
     `notches × 3` to the active PTY; invalidate link hover; redraw.
   - Otherwise → existing `app.scroll_up_by/down_by` branch unchanged.
4. Mode bit toggled at runtime by `ESC[?1007h`/`l` flows through the new
   `csi_modes` arm; the next wheel event picks up the new value.

**Implementation Steps** (≤ 7):

1. **Add mode bit** in `terminal_core.rs`: pick the next free `MODE_*`
   position, define the constant, include it in the construction-time
   default-on bitmask.
2. **Add `csi_modes` arm** for `1007` that sets/clears the bit and returns
   `MODE_ACTION_NONE`; mirror the boolean-mode pattern used by `?7`/`?25`.
3. **Add settings field** to `AppSettings` with the existing custom-default
   pattern; update the round-trip snapshot test fixture.
4. **Add TS mirror** + i18n keys + Terminal Behavior section toggle.
5. **Wire wheel branch** in `window_host.rs` before the existing
   scrollback fall-through; gate on AltScreen + mode bit + setting; emit
   `notches × 3` arrow bytes.
6. **Unit-test** the mode bit + the csi_modes arm + the bytes the wheel
   branch produces under each gate combination.

**Dependencies**: None within this plan; FR1 is self-contained.

**Testing Approach**:

- Unit (term_core): default-on for the new bit, set/reset via `?1007`.
- Unit (host wheel): table-driven cases over `(alt_screen, mode_bit, setting,
  mouse_report)` checking either "bytes written" or "scroll path called".
- Manual: Claude Code wheel scroll, `vim` / `less` wheel scroll.

**Acceptance Criteria**:

- [ ] AltScreen wheel-up sends three `ESC[A` per notch when all gates are ON.
- [ ] AltScreen wheel does not send PTY bytes when the setting is OFF.
- [ ] MainScreen wheel never sends PTY bytes (existing behaviour preserved).
- [ ] `Shift+wheel` produces the same arrow bytes as plain wheel (Shift
  ignored).
- [ ] Settings panel toggle persists and takes effect on the next wheel
  event.

**Estimated Effort**: small.

---

### Phase 2: CSI Modifier Extension (FR2)

**Goal**: Emit `ESC[<base>;<mods>X` for `Ctrl`/`Shift`/`Alt`-modified
navigation and function keys, while leaving plain (modifier-less) bytes
byte-identical to today.

**Files to Create**:

- _None_.

**Files to Modify**:

- `src-tauri/src/pty/input.rs` — extend `encode()` so that when `mods` has at
  least one of ctrl/shift/alt set AND `key` is one of the modifier-eligible
  navigation/function keys, emit the xterm CSI modifier form. When no
  modifier is held, the existing legacy byte sequences are unchanged. The
  modifier-bits-to-`<mods>` mapping is `1 + (shift?1:0) + (alt?2:0) +
  (ctrl?4:0)`.

  - `Arrow{Up|Down|Right|Left}` → `ESC[1;<mods>{A|B|C|D}`
  - `Home`/`End` → `ESC[1;<mods>{H|F}`
  - `PageUp`/`PageDown` → `ESC[{5|6};<mods>~`
  - `Insert`/`Delete` → `ESC[{2|3};<mods>~`
  - `F1`-`F4` → `ESC[1;<mods>{P|Q|R|S}`
  - `F5`-`F12` → `ESC[<n>;<mods>~` (`n` in {15,17,18,19,20,21,23,24})

  The Alt-only path through the existing `out.push(0x1b);` ESC-prefix block
  must not double-encode when the new path emits a CSI form — make sure the
  ESC-prefix block runs ONLY when the new path didn't fire.

- `src-tauri/src/window_host.rs` — if any keyboard call sites pass
  `Modifiers::NONE` even when modifiers were present (defensive check —
  encoder reads what it's given), update them so the encoder sees the real
  modifier state. The existing host-side chord interception
  (`Shift+PageUp/Down/Home/End` for scrollback) MUST continue to run before
  the encoder, so the new modifier-CSI path doesn't compete with those
  chords. Confirm there is no regression to the modifier-less Home/End/PgUp/
  PgDn/Arrow/F-key emission.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `encode()` modifier branch | Emit `ESC[<base>;<mods>X` for modified keys | At least one modifier in `mods`; key is one of the modifier-eligible set | Returns CSI modifier bytes; never emits the legacy short form for the same key |
| `encode()` legacy branch | Preserve current bytes for plain keys | `mods == NONE` (no ctrl/shift/alt) | Byte-identical to today |
| Host chord layer (unchanged) | Intercept scrollback chords before encode | Chord matched | Encoder not reached for the chord keys |

**Processing Flow**:

1. `KeyboardInput` reaches the host.
2. Host chord layer fires for matched scrollback chords (unchanged) and
   returns early without calling `encode`.
3. Otherwise host calls `encode(key, mods, target)`.
4. `encode` branches:
   - `mods == NONE` → legacy byte sequence (unchanged).
   - `mods != NONE` AND key in modifier-eligible set → CSI modifier form.
   - `mods != NONE` AND key not in the set → existing behaviour (Char path,
     Tab Shift back-tab path, Backspace Win32 shim, etc.) unchanged.

**Implementation Steps** (≤ 7):

1. **Add a helper** that maps `Modifiers` to the xterm `1..=8` `<mods>`
   parameter; treat NONE as "no modifier form" (caller should not call
   helper in that case).
2. **Extend `encode()`** for arrow + Home/End by emitting the `ESC[1;<m>X`
   form when `mods != NONE`.
3. **Extend `encode()`** for PageUp/PageDown/Insert/Delete with the
   `ESC[<n>;<m>~` form.
4. **Extend `encode()`** for F1-F12 (F1-F4 → `ESC[1;<m>{P..S}`; F5-F12 →
   `ESC[<n>;<m>~`).
5. **Guard the ESC-prefix block** against double-encoding when the CSI
   modifier form already handled Alt.
6. **Unit-test** the bytes for each (key, modifier-combo) representative.
7. **Regression-test** plain (no-modifier) Home/End/PgUp/PgDn/Arrow/F1/F12
   are unchanged.

**Dependencies**: None within this plan; FR2 is independent of FR1 and FR3.

**Testing Approach**:

- Unit: 6-8 representative `(key, mods) → bytes` cases plus 4 regression
  cases for the legacy path.
- Manual: Claude Code `Ctrl+Home` / `Ctrl+End` / `Ctrl+PgUp`; vim
  `Ctrl+Right`.

**Acceptance Criteria**:

- [ ] `Ctrl+Home` produces `ESC[1;5H`.
- [ ] `Ctrl+End` produces `ESC[1;5F`.
- [ ] `Ctrl+PageUp` produces `ESC[5;5~`.
- [ ] `Ctrl+ArrowUp` produces `ESC[1;5A`.
- [ ] `Shift+F1` produces `ESC[1;2P`.
- [ ] Plain `Home` still produces `ESC[H`; plain `PageUp` still produces
  `ESC[5~`.
- [ ] `Shift+PageUp` is still intercepted by the host scrollback chord and
  does NOT reach the encoder.

**Estimated Effort**: small.

---

### Phase 3: OSC 8 Hyperlink Host Wiring (FR3)

**Goal**: Make OSC 8 hyperlinked cells clickable: underline + hand cursor on
`Ctrl+hover` and OS opener launch on `Ctrl+click`. Works inside AltScreen
unlike the regex URL detector. **No new renderer code** — reuses existing
`hover.link_cells` / `hover.link` infrastructure.

**Design note**: the existing `HoverState` in `window_host.rs:310` already
owns `link_cells: Vec<(row, col_start, col_end)>` (consumed by `render/mod.rs`
to draw the URL underline) and `link: Option<DetectedLink>` (consumed by
`update_link_cursor` for the hand cursor and by `try_open_link_at_pointer`
for the click). The OSC 8 path reuses this infrastructure by **synthesizing a
`DetectedLink` from the cell's `hyperlink_id`** instead of adding a parallel
draw + cursor + click pipeline.

**Files to Create**:

- _None_.

**Files to Modify**:

- `src-tauri/src/window_host.rs` —
  - Add a helper, **`detect_osc8_link_at(core, row, col) -> Option<DetectedLink>`**,
    that, when `core.get_cell_hyperlink_id(col, row) != 0`, looks up
    `core.get_hyperlink_uri(id)`, validates with `links::is_safe_uri`, then
    expands the cell range to cover the contiguous run of cells with the
    same `hyperlink_id` on the same row. Returns a `DetectedLink` whose
    `kind` is `LinkKind::Url(uri)` and `cells` is the populated
    `Vec<(row, col_start, col_end)>` covering that run. Returns `None` for
    unsafe URIs (with a `log::warn!`), missing `id` entries, or empty URIs.
  - In `refresh_link_hover` (around line 680, the
    `if (detect_urls || detect_paths) && !app.alt_screen { … }` block),
    move the OSC 8 detection ABOVE the AltScreen / settings gates. The new
    order is: (1) try `detect_osc8_link_at` first — if `Some`, populate
    `hover.link_cells` / `hover.link` and skip the regex path; (2) if `None`,
    fall through to the existing regex path (still gated on
    `!app.alt_screen`). Because the OSC 8 hit pre-populates the same hover
    fields the regex path uses, `update_link_cursor` and the renderer
    require no change.
  - In `try_open_link_at_pointer` (line 813), move the AltScreen
    short-circuit (line 821-823 `if app.alt_screen { return false; }`) AFTER
    a new OSC 8 lookup at the click cell. If `detect_osc8_link_at` returns
    `Some(link)`, dispatch via the existing `LinkKind::Url(url) → open_url`
    arm (the same arm currently handling regex URL hits — no new opener
    plumbing) and return `true`; otherwise the AltScreen short-circuit still
    runs and the existing regex code path is unchanged.
  - In `refresh_link_hover_on_pty_change` (line 720), the early `if
    app.alt_screen { invalidate_link_hover(); return; }` (line 725) must be
    refined so OSC 8 hover state survives AltScreen PTY output. Replace the
    blanket guard with a condition that invalidates+returns only when the
    current `hover.link` is NOT an OSC 8 hit (i.e. it was a regex hit, which
    has the existing AltScreen suppression policy). A new `HoverState` flag
    (or `LinkKind` variant — see Implementation Step 1 below) discriminates
    OSC 8 vs regex hits.

- (No changes to `term_core` — `get_cell_hyperlink_id` and
  `get_hyperlink_uri` are already exposed; the snapshot/replay path already
  round-trips `hyperlink_id` per the existing `slim_cell.rs` /
  `ring_buffer.rs` code.)

- (No renderer changes — `render/mod.rs:629` `hovered_link` parameter is
  already a `&[(u16, u16, u16)]` slice consumed directly from
  `hover.link_cells`, so populating that vec from the OSC 8 path is enough
  to get the underline.)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `detect_osc8_link_at(core, row, col)` | Synthesize a `DetectedLink` for an OSC 8 cell | Pointer mapped to a cell | Returns `Some(DetectedLink)` (URL kind, populated cell run) for safe OSC 8 hits; `None` (with `warn` log for unsafe URIs) otherwise |
| Hover branch reuse | Populate `hover.link_cells` + `hover.link` from OSC 8 hit | Pointer hover updated | Existing `update_link_cursor` + renderer paths fire unchanged |
| Click branch reuse | Dispatch via existing `LinkKind::Url(url) → open_url` arm | `Ctrl+click` over OSC 8 cell at click time | URI opened with the same opener used by regex URL hits |
| AltScreen guard refinement | Let OSC 8 hover/click survive AltScreen | Existing regex AltScreen guard runs only when the active hover is NOT OSC 8 | OSC 8 works in AltScreen; regex path still skipped |
| OSC 8 vs regex discriminator | Tell PTY-change handler whether to invalidate | New flag/variant on `HoverState` or `LinkKind` | `refresh_link_hover_on_pty_change` only invalidates regex hits in AltScreen |

**Processing Flow** (hover, runs unconditionally — no AltScreen guard):

1. CursorMoved → map pointer to `(row, col)` (existing code).
2. Call `detect_osc8_link_at(core, row, col)`.
   - `Some(link)` → populate `hover.link_cells` + `hover.link` (mark it as
     OSC 8 via the discriminator); existing `update_link_cursor` paints
     the hand cursor when Ctrl is held; renderer underlines.
   - `None` → fall through to existing regex path (still gated on
     `!app.alt_screen` for regex hits only).

**Processing Flow** (click):

1. `Ctrl+click` reaches `try_open_link_at_pointer`.
2. Call `detect_osc8_link_at`.
   - `Some(link)` → dispatch via the existing `LinkKind::Url(url) → open_url`
     arm; return `true`.
   - `None` → existing AltScreen short-circuit + regex `find_link_at` path
     run unchanged.

**Implementation Steps** (≤ 7):

1. **Add the OSC 8 discriminator** — either an `is_osc8: bool` field on
   `HoverState`, or a new `LinkKind::OscHyperlink(String)` variant. (Pick at
   implementation time based on what fits the existing struct shapes; both
   options preserve the contract above.)
2. **Add `detect_osc8_link_at` helper** with the contract above. Use
   `get_cell_hyperlink_id` to read the id, expand the run by scanning the
   row for adjacent cells with the same id, look up the URI, validate, and
   build the `DetectedLink`.
3. **Wire hover** in `refresh_link_hover` to call the helper before the
   `(detect_urls || detect_paths) && !app.alt_screen` gate.
4. **Wire click** in `try_open_link_at_pointer` to call the helper before
   the AltScreen short-circuit.
5. **Refine PTY-change guard** in `refresh_link_hover_on_pty_change` to
   skip invalidation when the active hover is an OSC 8 hit.
6. **Unit-test** the helper for: unsafe URI → None; missing id → None;
   safe URI on a 5-cell run → Some with the right cell range.
7. **Manual verify** in Claude Code (PR ID), and in a synthetic
   `printf '\e]8;;https://example.com\e\\link\e]8;;\e\\'` test inside both
   MainScreen and AltScreen (e.g. vim `:!cat file`).

**Dependencies**: None within this plan; FR3 is independent.

**Testing Approach**:

- Unit (helper): three cases (unsafe URI, missing id, safe URI).
- Unit (host hover/click branches): assert the helper is consulted before
  the regex path and that the AltScreen guard does not block OSC 8.
- Manual: Claude Code PR ID, `gh pr view --web`, synthetic printf.

**Acceptance Criteria**:

- [ ] OSC 8 cell underlines on `Ctrl+hover` in both MainScreen and AltScreen.
- [ ] `Ctrl+click` on an OSC 8 cell opens the URI via the OS opener.
- [ ] Unsafe URI (`javascript:`) does not open and emits a `warn` log.
- [ ] Missing `hyperlink_id` (e.g. scrollback evicted) does not underline.
- [ ] Existing regex URL detector still works in MainScreen and is still
  inert in AltScreen (no behaviour regression on that path).

**Estimated Effort**: medium.

---

## Complete File Structure

Files touched by this feature (ASCII tree):

```
crates/
  term_core/
    src/
      terminal_core.rs       # FR1: add MODE_ALTERNATE_SCROLL constant + default-on
      csi_modes.rs           # FR1: add arm for ?1007 h/l
  app_settings/
    src/
      settings.rs            # FR1: add alternate_scroll_enabled field + helpers

src-tauri/
  src/
    pty/
      input.rs               # FR2: extend encode() for modifier CSI sequences
    window_host.rs           # FR1: wheel branch
                             # FR3: detect_osc8_link_at helper + hover/click reuse +
                             #      PTY-change guard refinement + OSC 8 discriminator
  web-shared/
    settings/
      types.ts               # FR1: mirror new bool
      sections/
        terminal-behavior-section.ts   # FR1: toggle UI
    i18n/
      locales/en.json        # FR1: strings
      locales/ja.json        # FR1: strings

doc/
  tasks/
    claude-code-altscreen-ux/
      要件定義書.md
      SPEC.md
      IMPLEMENTATION.md      # this file
      VERIFICATION.md
      sdd.yaml
      tasks.yaml
```

## Testing Strategy

- **Unit**: Rust `cargo test --lib` is the primary surface (see
  `test/README.md`). All FR-level logic (mode bits, encoder branches, helper)
  is unit-testable in-process; no scaffolding required.
- **Integration**: `src-tauri/tests/` is the right home if any of the
  hover/click branches need wiring through `App` state; otherwise inline
  `#[cfg(test)] mod tests {}` next to the code is the project convention.
- **E2E**: none — no E2E harness exists in this project.
- **Manual**: scripted in VERIFICATION.md.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| _none_ | — | Implementation reuses existing crates only |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `Shift+PageUp/Down` host scrollback chord clashes with the FR2 CSI modifier path | Medium | Low | Host chord intercept runs *before* `encode`; explicit acceptance test for "host chord still wins" |
| Hyperlink `id` evicted from `hyperlink_table` between hover and click | Low | Low | `detect_osc8_link_at` returns `None` for missing `id`; behaviour falls back to regex path |
| `refresh_link_hover_on_pty_change` AltScreen short-circuit (line 725) wipes OSC 8 hover on every PTY chunk | Medium | Medium | Refine the guard via the OSC 8 discriminator so only regex hits get invalidated in AltScreen |
| `pty::input::encode()` signature change forces large call-site fanout | Low | Low | Read modifiers from existing call-site state; do not change `encode()` signature unless necessary |
| FR1 toggle UI string keys collide with existing i18n entries | Low | Low | Namespace under `settings.terminal.alternateScroll*` |

## Open Questions

All three Open Questions from the initial draft are resolved (verify-plan
findings, 2026-06-26):

- **OQ1 (FR1)** — *Resolved*: eMterm has no host-side mouse-report path
  today. The wheel branch gate is `alt_screen && mode_bit && setting`; no
  mouse-reporting check is needed. SPEC.md and VERIFICATION.md updated to
  match.
- **OQ2 (FR3)** — *Resolved*: the renderer reads from
  `hover.link_cells` (`src-tauri/src/render/mod.rs:629`); no renderer
  changes are needed. Phase 3 populates that existing vec.
- **OQ3 (FR3)** — *Resolved*: underline the **whole OSC 8 run**
  (contiguous cells with the same `hyperlink_id` on the same row) to match
  WezTerm. `detect_osc8_link_at` walks the run; cost is bounded by the row
  width.

Remaining open items (implementation-time decisions, not blockers):

- [ ] **OQ4**: Pick the discriminator shape (`HoverState.is_osc8` flag vs
  new `LinkKind::OscHyperlink` variant). Phase 3 step 1 decides.
- [ ] **OQ5**: Pick the bit position for `MODE_ALTERNATE_SCROLL`. Bit 15 is
  `MODE_ALT_SCREEN`; bit 16 is the next free slot. Phase 1 step 1 decides.

## Success Metrics

- [ ] All Phase 1/2/3 unit tests pass under `cargo test --lib`.
- [ ] `cargo check --no-default-features` (CLI-only build) still passes —
  none of the new code is gated, so any accidental GUI-only dependency leak
  surfaces immediately.
- [ ] `bun run typecheck` passes (FR1 TS mirror).
- [ ] Manual checklist in VERIFICATION.md is fully ticked.
- [ ] `alternate_scroll_enabled = false` reproduces pre-change wheel
  behaviour exactly.
