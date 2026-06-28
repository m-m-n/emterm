# Feature: SGR Reverse Default-Color Swap

## Overview

Fix a rendering bug where `\e[7m` (SGR 7 / reverse video) has no visible effect on cells whose `fg` and `bg` are both `PackedColor::DEFAULT`. The root cause is that the existing packed-level swap becomes a NOP when both inputs are `DEFAULT`, and the `unwrap_or_else` fallbacks (`theme.fg` / `theme.bg`) are never swapped to match. The fix keeps the existing packed-level swap (so `bold_brighten_packed` still sees the post-reverse perceived foreground) and additionally swaps the `theme.fg` / `theme.bg` fallback values when `reverse` is set, so a both-`DEFAULT` cell resolves to `theme.bg` for `fg` and `theme.fg` for `bg`.

## Objectives

- `printf '\e[7mREVERSE\e[0m NORMAL\n'` renders the `REVERSE` run with theme fg/bg swapped, matching WezTerm and xterm.
- Existing reverse behavior for indexed and truecolor cells is preserved.
- Selection-and-reverse composition (XOR) is preserved.
- Bold-brighten ordering against reverse is preserved (no semantics change for indexed `0..8` foregrounds with bold).

## User Stories

### US1: Reverse on DEFAULT-color cells works
As an eMterm user, I want `\e[7m...\e[0m` to render with reverse-video, so that CLI tools that rely on SGR 7 display correctly without explicit color codes.

**Acceptance Criteria:**
- [ ] After `printf '\e[7mREVERSE\e[0m NORMAL\n'`, the `REVERSE` run shows `theme.bg` as foreground and `theme.fg` as background.
- [ ] The `NORMAL` run is unaffected.

### US2: Reverse on indexed / truecolor cells unchanged
As an eMterm user, I want previously-working reverse cases to keep working, so that this fix introduces no regression.

**Acceptance Criteria:**
- [ ] `\e[31;42m\e[7mX\e[0m` renders `X` with fg=green, bg=red.
- [ ] Truecolor (`\e[38;2;R;G;B...`) reverse cells swap fg and bg.

### US3: Selection over reverse cells re-inverts cleanly
As an eMterm user, I want selecting a reverse-video cell to look like a selected non-reverse cell, so that selection highlight stays consistent.

**Acceptance Criteria:**
- [ ] For a both-DEFAULT cell with `STYLE_REVERSE` and `selected = true`, the final fg/bg equals the unstyled non-selected case (`fg = theme.fg`, `bg = theme.bg`).

## Technical Requirements

### Functional Requirements

- **FR1: Reverse covers both DEFAULT and explicitly-colored cells.**
  `resolve_cell_style_from_packed` keeps the existing packed-level swap (so `bold_brighten_packed` sees the post-reverse foreground) and, in addition, swaps the `theme.fg` / `theme.bg` fallback values passed to `packed_to_egui` (and the `unwrap_or_else` arms) when `reverse` is set. This makes the `DEFAULT, DEFAULT` case resolve to `theme.bg` for `fg` and `theme.fg` for `bg`, while leaving the indexed / truecolor cases unchanged (they already invert via the packed swap).

- **FR2: Bold-brighten ordering preserved.**
  When `STYLE_BOLD` is set and `theme.bold_brightens_ansi_colors` is on, `bold_brighten_packed` continues to be applied to the *post-reverse* `effective_fg_packed` (i.e., the cell's perceived foreground after reverse), matching today's behavior.

- **FR3: Selection swap unchanged.**
  The existing `if selected { std::mem::swap(&mut fg, &mut bg); }` block continues to run after the resolve step so the XOR composition is unchanged.

- **FR4: Dim / hidden ordering unchanged.**
  `dim` blends `fg` toward `bg`, and `hidden` clamps `fg = bg`, both after selection. No change.

### Non-Functional Requirements

- **NFR1 - Performance:** No measurable regression. The new work is two constant-time conditionals (selecting the fallback pair); no new allocation, no new resolution work.
- **NFR2 - Compatibility:** Reverse rendering matches WezTerm/xterm/alacritty for both DEFAULT-color and explicitly-colored cells.
- **NFR3 - Scope:** No changes to `crates/term_core` SGR parsing or `STYLE_REVERSE` flag semantics. DECSCNM (`MODE_REVERSE_SCREEN`) integration into render remains out of scope.

## Implementation Approach

### Architecture

The change is local to `src-tauri/src/render/mod.rs`:

```
resolve_cell_style_from_packed(theme, packed_fg, packed_bg, flags, selected)
  │
  ├─ compute flag booleans (bold, dim, italic, ..., reverse, hidden, ...)
  │
  ├─ effective_fg_packed = if reverse { packed_bg } else { packed_fg }       ← KEEP
  │  effective_bg_packed = if reverse { packed_fg } else { packed_bg }       ← KEEP
  │  (needed for bold-brighten to see the post-reverse foreground)
  │
  ├─ if bold && bold_brightens_ansi_colors:
  │      effective_fg_packed = bold_brighten_packed(effective_fg_packed)     ← KEEP
  │
  ├─ (fg_fallback, bg_fallback) = if reverse { (theme.bg, theme.fg) }        ← NEW
  │                               else        { (theme.fg, theme.bg) }
  │
  ├─ fg = packed_to_egui(effective_fg_packed, fg_fallback, theme)            ← UPDATED
  │         .unwrap_or_else(|| rgb_to_egui(fg_fallback))                     ←  (fg_fallback
  │  bg = packed_to_egui(effective_bg_packed, bg_fallback, theme)            ←   was theme.fg,
  │         .unwrap_or_else(|| rgb_to_egui(bg_fallback))                     ←   bg was theme.bg)
  │
  ├─ if selected { std::mem::swap(&mut fg, &mut bg); }                       ← UNCHANGED
  ├─ dim handling                                                            ← UNCHANGED
  └─ hidden handling                                                         ← UNCHANGED
```

Rationale:

- `effective_fg_packed` is what bold-brighten inspects. It must equal the *post-reverse* foreground because `bold_brighten_packed` only promotes indexed `0..8`, and we want bold-brighten applied to the cell's perceived foreground (matching the existing comment at L1204-1207).
- For indexed / truecolor cells the `effective_*_packed` values are non-DEFAULT, so `packed_to_egui` returns `Some(...)` and the fallback is never consulted. The packed-level swap alone produces a correct fg/bg swap. No additional work is needed.
- For both-DEFAULT cells the packed swap is a NOP and `packed_to_egui` returns `None`, so the `unwrap_or_else` arm decides the final color. Swapping the fallback values (`theme.fg` / `theme.bg`) when `reverse` is set is what makes the `DEFAULT, DEFAULT` case actually invert.
- For mixed cells (one DEFAULT side, one indexed/truecolor side) the same logic is sufficient: the non-DEFAULT side resolves directly, the DEFAULT side resolves to the swapped theme color.

Concretely, for `reverse = true`:

| input | `fg` (rendered) | `bg` (rendered) |
|-------|-----------------|-----------------|
| `DEFAULT, DEFAULT` | `theme.bg` (fallback swap) | `theme.fg` (fallback swap) |
| `indexed(1), DEFAULT` | `theme.bg` (fallback swap on DEFAULT side) | `R(indexed(1))` |
| `DEFAULT, indexed(1)` | `R(indexed(1))` | `theme.fg` (fallback swap on DEFAULT side) |
| `truecolor(A), truecolor(B)` | `R(B)` | `R(A)` |
| `DEFAULT, indexed(1)` + bold + brighten | `R(indexed(9))` | `theme.fg` (fallback swap) |

In every row the rendered `fg` slot holds the cell's *perceived foreground after reverse* (which is what users see as the glyph color) and the `bg` slot holds the perceived background. Bold-brighten is correctly applied to the perceived foreground (`effective_fg_packed` after the packed swap).

### Affected Code

`src-tauri/src/render/mod.rs` — `resolve_cell_style_from_packed` function (~L1185-1258).

The only behavioral edit is selecting `(fg_fallback, bg_fallback)` based on `reverse` and using those values both as the `packed_to_egui` second argument and inside the matching `unwrap_or_else` arms. Comments on the existing packed-level swap and the fallback-selection block are updated to explain the role split (packed swap → bold-brighten visibility on the perceived fg; fallback swap → DEFAULT-color correctness when packed_to_egui returns `None`).

No other files change. The `_fallback` parameter of `packed_to_egui` remains unused inside the function body; cleaning it up is left for a follow-up.

### Dependencies

**Internal Dependencies:**
- `crates/term_core` cell flags (`STYLE_REVERSE`, `STYLE_BOLD`, ...): no change.
- `crate::render::theme::Theme`: read-only.

**External Dependencies:**
- None.

### File Structure

```
src-tauri/src/render/
├── mod.rs               # resolve_cell_style_from_packed (edited)
                         # mod tests (new test cases added)
```

## Test Scenarios

### Unit Tests

Add to `src-tauri/src/render/mod.rs`'s `mod tests`:

- [ ] **reverse_with_both_default_swaps_to_theme_bg_and_fg** — packed_fg=DEFAULT, packed_bg=DEFAULT, flags=STYLE_REVERSE, selected=false. Expect `fg == rgb_to_egui(theme.bg)` and `bg == rgb_to_egui(theme.fg)`.
- [ ] **reverse_with_indexed_fg_default_bg_swaps** — packed_fg=indexed(1) red, packed_bg=DEFAULT, flags=STYLE_REVERSE. Expect `fg == rgb_to_egui(theme.bg)`, `bg == indexed(1) resolved color`.
- [ ] **reverse_with_truecolor_swaps** — packed_fg=truecolor(R1,G1,B1), packed_bg=truecolor(R2,G2,B2), flags=STYLE_REVERSE. Expect `fg = (R2,G2,B2)`, `bg = (R1,G1,B1)`.
- [ ] **reverse_then_selection_cancels** — packed_fg=DEFAULT, packed_bg=DEFAULT, flags=STYLE_REVERSE, selected=true. Expect `fg == rgb_to_egui(theme.fg)`, `bg == rgb_to_egui(theme.bg)` (i.e., the swap done by reverse is undone by selection).
- [ ] **no_reverse_no_selection_uses_theme_defaults** — control case. flags=0, selected=false, both DEFAULT. Expect `fg == rgb_to_egui(theme.fg)`, `bg == rgb_to_egui(theme.bg)`.
- [ ] **reverse_with_bold_brighten_promotes_perceived_fg** — packed_fg=DEFAULT, packed_bg=indexed(1) red, flags=STYLE_REVERSE|STYLE_BOLD, theme.bold_brightens_ansi_colors=true. Expect rendered `fg` (= the perceived foreground / glyph color under reverse) to be the *bright* indexed(9) red, and rendered `bg` to be `rgb_to_egui(theme.fg)` (the original DEFAULT fg materialized through the swapped fallback). Confirms that bold-brighten sees the post-reverse perceived foreground.

### Integration / Manual Tests

- [ ] Manual: run `printf '\e[7mREVERSE\e[0m NORMAL\n'` in eMterm; the `REVERSE` run shows reverse-video, the `NORMAL` run is unaffected. Compare side-by-side with WezTerm.
- [ ] Manual: run `printf '\e[31;42m\e[7mX\e[0m Y\n'`; `X` is fg=green, bg=red; `Y` is the default style.

### E2E Tests
**Existing E2E tests**: not detected as a covering feature for this render path. None added.
**Run command**: `./scripts/run-e2e-docker.sh test` (project-level harness; not required for this fix).
- [ ] Existing E2E tests pass without regression.

### Edge Cases
- [ ] Reverse on a cell whose fg or bg is `PackedColor::DEFAULT` but the other side has an indexed/truecolor value.
- [ ] Reverse combined with dim — dim still blends post-swap `fg` toward post-swap `bg`.
- [ ] Reverse combined with hidden — final `fg` clamps to post-swap `bg`, so the glyph stays invisible (existing tests should already cover hidden; ensure they still pass).

### Performance Tests
- [ ] None required. Change is O(1) per cell and reuses existing math.

## Security Considerations

- Not applicable. Pure rendering-path color-resolution fix; no input parsing, IPC, or filesystem changes.

## Error Handling

- Not applicable. The function is pure and infallible.

## Performance Optimization

- Not applicable. No new allocations or branches on hot paths beyond an `if reverse { swap }`.

## Success Criteria

- [ ] FR1-FR4 are implemented as specified.
- [ ] All new and existing unit tests in `src-tauri/src/render/mod.rs` pass under `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`.
- [ ] Manual reproduction with `printf '\e[7mREVERSE\e[0m NORMAL\n'` shows reverse-video on the `REVERSE` run.
- [ ] No regression in `bold_brighten_packed_*` and existing rendering tests.

## Open Questions

> **Note**: 未解決の要件は sdd.yaml で `status: tbd` として管理されています。
> `/em-sdd:sdd.2-create-plan` の実行前に解決してください。

(なし — 修正方針は議論で合意済み)

## Implementation Phases

### Phase 1: Edit `resolve_cell_style_from_packed` and add tests
**Goals:** Apply FR1; preserve FR2-FR4.
**Deliverables:**
- Edit to `src-tauri/src/render/mod.rs` selecting `(fg_fallback, bg_fallback)` based on `reverse` and threading those values into the `packed_to_egui` second argument plus the matching `unwrap_or_else` arms; updating in-function comments to describe the packed-swap / fallback-swap role split.
- New unit tests covering the cases listed under "Unit Tests".
- `cargo test --lib` green; `cargo check --no-default-features` green.

## References

- 要件定義書: `doc/tasks/sgr-reverse-default-color-swap/要件定義書.md`
- 議論レポート: `tmp/discussion-reverse-attr-not-rendered.md`
- WezTerm 実装: `wezterm-gui/src/termwindow/render/screen_line.rs` (background/glyph pass reverse swap)
- 既存コード: `src-tauri/src/render/mod.rs:1185` `resolve_cell_style_from_packed`
- 既存コード: `src-tauri/src/render/mod.rs:1329` `packed_to_egui` (`_fallback` is currently unused — cleanup deferred)
- SGR パース: `crates/term_core/src/sgr.rs:31` (SGR 7 set), `:38` (SGR 27 clear)
