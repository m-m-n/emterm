# Verification Result: Claude Code AltScreen UX Improvements

**Feature**: Claude Code AltScreen UX Improvements (DECSET 1007 + CSI Modifier
extension + OSC 8 host wiring)
**Date**: 2026-06-26
**Commit (baseline)**: `b29f83c99a1681e65ecc620b6f17b376f11f774c`
**VERIFICATION.md**: `doc/tasks/claude-code-altscreen-ux/VERIFICATION.md`
**SPEC.md**: `doc/tasks/claude-code-altscreen-ux/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/claude-code-altscreen-ux/IMPLEMENTATION.md`
**Scope of this pass**: sdd.6 comprehensive verify — file-structure spot
check, SPEC compliance matrix, scenario coverage, performance/security
code review, manual checklist extraction. Build / unit-test / format /
clippy / dead-code already covered by sdd.5-check and **not re-run** here.

---

## 1. File Structure Verification

All 10 files listed in VERIFICATION.md "Files to Modify" were spot-checked
with `grep`. Every expected marker is present.

| # | File | Expected marker | Evidence | Status |
|---|------|-----------------|----------|--------|
| 1 | `crates/term_core/src/terminal_core.rs` | `MODE_ALTERNATE_SCROLL` constant + included in default-on bitmask at both init sites | L40 `pub const MODE_ALTERNATE_SCROLL: u8 = 16;` / L376 `| (1u32 << MODE_ALTERNATE_SCROLL);` (construction) / L904 same (reset) | OK |
| 2 | `crates/term_core/src/csi_modes.rs` | `1007 =>` arm setting `MODE_ALTERNATE_SCROLL` + two new tests | L106 `1007 =>` arm in `handle_set_mode` calls `self.set_mode(MODE_ALTERNATE_SCROLL, enable)` and returns `MODE_ACTION_NONE`; L255 `decset_1007_toggles_alternate_scroll_bit` + `alternate_scroll_default_on` tests present | OK |
| 3 | `crates/app_settings/src/settings.rs` | `alternate_scroll_enabled: bool` field + `default_alternate_scroll_enabled` helper | L104 `fn default_alternate_scroll_enabled() -> bool { true }`; L384 `#[serde(default = "default_alternate_scroll_enabled", deserialize_with = "deserialize_null_alternate_scroll_enabled")]`; L387 `pub alternate_scroll_enabled: bool`; L691 default in `Default` impl; L842/L918 round-trip test fixture and assertion | OK |
| 4 | `src-tauri/src/settings.rs` | Runtime mirror field (documented deviation from plan) | L632 `pub alternate_scroll_enabled: bool` on runtime `Settings`; L989 default in `Settings::default`; L1171 `RawSettings.alternate_scroll_enabled: Option<bool>`; L1593-1594 merge step | OK |
| 5 | `src-tauri/src/pty/input.rs` | Modifier CSI sequences in `encode()` | L88 `xterm_mods_param`, L108 `csi_mods_letter` producing `\x1b[1;{m}{letter}`, L114 `csi_mods_tilde` producing `\x1b[{n};{m}~`; L186-218 modifier branch in `encode()` for Arrow / Home / End / PageUp / PageDown / Insert / Delete / F1-F12 returning before the Alt-prefix block | OK |
| 6 | `src-tauri/src/window_host.rs` | wheel branch using `alternate_scroll_wheel_bytes`; `detect_osc8_link_at` helper; `HoverState.is_osc8` | L326 `is_osc8: bool` on `HoverState`; L1913 `fn detect_osc8_link_at(...)`; L2226 `fn alternate_scroll_wheel_bytes(...)`; L3032 wheel branch in `MouseWheel` call site invoking the helper and writing via `tab.write_input` | OK |
| 7 | `src-tauri/web-shared/settings/types.ts` | `alternate_scroll_enabled: boolean` in `AppSettings` | L52 `alternate_scroll_enabled: boolean;` | OK |
| 8 | `src-tauri/web-shared/settings/sections/terminal-behavior-section.ts` | Toggle reading `settings.alternate_scroll_enabled` | L140-143 toggle entry with `t("settings.terminal.alternateScrollEnabled")`, `value: settings.alternate_scroll_enabled`, `onSave: (v) => ctx.saveSetting("alternate_scroll_enabled", v)` | OK |
| 9 | `src-tauri/web-shared/i18n/locales/en.json` | `settings.terminal.alternateScrollEnabled` (+ `*Desc`) | L99 `"alternateScrollEnabled": "Alternate Scroll (DECSET 1007)"` and L100 `"alternateScrollEnabledDesc": ...` | OK |
| 10 | `src-tauri/web-shared/i18n/locales/ja.json` | Same keys, Japanese strings | L99 `"alternateScrollEnabled": "代替スクロール (DECSET 1007)"`, L100 description string present | OK |

**File-structure verdict**: **PASS** — every expected modification is
present.

---

## 2. SPEC.md Compliance Matrix

### Functional Requirements

| ID | Requirement | Code evidence | Status |
|----|-------------|---------------|--------|
| FR1 | DECSET 1007 (alternate_scroll) — wheel→arrow in AltScreen, default-ON mode bit, user toggle | Mode bit `MODE_ALTERNATE_SCROLL` (L40, terminal_core.rs) default-on in both init paths (L376, L904); DECSET 1007 arm in `csi_modes.rs::handle_set_mode` (L106); `alternate_scroll_wheel_bytes` helper gating on `alt_screen && mode_bit_on && setting_on` (L2232, window_host.rs); wheel call site reads mode bit via `get_mode(MODE_ALTERNATE_SCROLL)` and `settings.alternate_scroll_enabled` (window_host.rs L3025-3047); user setting present in both `app_settings::AppSettings` and runtime `Settings` | Satisfied |
| FR2 | CSI Modifier extension for Home/End/PageUp/PageDown/Insert/Delete/Arrow/F1-F12 | `xterm_mods_param` (L88, input.rs) maps Modifiers to `1..=8`; `csi_mods_letter` builds `ESC[1;<m>X` (L108); `csi_mods_tilde` builds `ESC[<n>;<m>~` (L114); `encode()` modifier branch (L186-218) returns BEFORE the Alt-prefix block so no double-encoding; `Modifiers::NONE` returns `None` from `xterm_mods_param` so plain keys keep the legacy short form | Satisfied |
| FR3 | OSC 8 host-side support — hover underline + Ctrl-click open | `detect_osc8_link_at` helper (L1913, window_host.rs) reads `core.get_cell_hyperlink_id` + `core.get_hyperlink_uri`, validates via `links::is_safe_uri`, expands the contiguous run on the same row, returns a `DetectedLink { kind: LinkKind::Url(uri), cells: [(row, col_start, col_end)] }`; wired into hover (L698-705 in `refresh_link_hover`) BEFORE the regex+AltScreen gate; wired into click (L873-886 in `try_open_link_at_pointer`) BEFORE the AltScreen short-circuit; `HoverState.is_osc8` discriminator (L326) drives the PTY-change guard refinement (L759, L769, L794) | Satisfied |

### Non-Functional Requirements

| ID | Requirement | Code evidence | Status |
|----|-------------|---------------|--------|
| NFR1 | Performance (<100µs wheel / <50µs key / <200µs hit-test) | Wheel: arrow byte slice is `b"\x1b[A"` or `b"\x1b[B"` static (L2239); buffer is `Vec::with_capacity(arrow.len() * count)` then `extend_from_slice` per notch (L2241-2244) — single allocation per wheel event, no per-notch realloc. Hit-test: `get_cell_hyperlink_id(col, row)` + `get_hyperlink_uri(id)` are direct cell accessors (no row scan beyond the cheap run-expansion adjacent-cell read). Modifier encode: `format!` into a small `Vec<u8>` of ≤8 bytes per call — single small heap alloc, not the pre-sized 6-byte stack buffer the SPEC describes as ideal, but well within the <50µs envelope at this size. No micro-bench required per VERIFICATION.md ("inspect-only"). | Satisfied (smoke / code-review level) |
| NFR2 | Security — OSC 8 URI validated by `is_safe_uri`; unsafe schemes dropped with `log::warn!` | `detect_osc8_link_at` checks `links::is_safe_uri(&uri)` and on failure calls `log::warn!("native-poc: refusing OSC 8 URI with unsafe scheme: {uri}")` then returns `None` (L1933-1936). `try_open_link_at_pointer` re-checks `links::is_safe_uri` before `open_url` (L878) as defence-in-depth and logs `warn` if it ever bypassed (L881). FR1 wheel bytes are static slices, no user-controlled interpolation. | Satisfied |
| NFR3 | Compatibility (xterm) — DECSET 1007 + xterm CSI modifier convention | Bytes asserted in TS-3 (`ESC[A` × 3 per wheel-up notch) and TS-9..TS-18 (e.g. `Ctrl+Home → ESC[1;5H`, `Ctrl+End → ESC[1;5F`, `Ctrl+PageUp → ESC[5;5~`, `Shift+F1 → ESC[1;2P`, `Ctrl+Alt+F5 → ESC[15;7~`). `xterm_mods_param` formula `1 + shift + 2·alt + 4·ctrl` matches xterm. | Satisfied |
| NFR4 | User control — `alternate_scroll_enabled` opts out of FR1 without affecting FR2 or FR3 | Wheel branch gates explicitly on `settings.alternate_scroll_enabled` (L3036, L2232); FR2 encode branch reads `mods` only (no settings consult); FR3 hover/click branches read no FR1 setting. | Satisfied |

**SPEC compliance verdict**: **PASS** — all FR1/FR2/FR3 + NFR1-4 are
satisfied. No partials, no gaps.

---

## 3. Test Scenario Coverage

VERIFICATION.md's Test Scenarios table reports these statuses for the
automated scenarios (cross-checked against the file at sdd.4-implement
commit):

| ID | Scenario | Status |
|----|----------|--------|
| TS-1 | term_core `MODE_ALTERNATE_SCROLL` default ON | PASS — `csi_modes::tests::alternate_scroll_default_on` |
| TS-2 | DECSET/DECRST 1007 toggles bit | PASS — `csi_modes::tests::decset_1007_toggles_alternate_scroll_bit` |
| TS-3 | Wheel-up in AltScreen → 3× `ESC[A` | PASS — `window_host::tests::fr1_wheel_up_in_alt_screen_emits_three_arrow_up` |
| TS-4 | Setting OFF → no bytes | PASS — `fr1_wheel_suppressed_when_setting_off` |
| TS-5 | Mode bit OFF → no bytes | PASS — `fr1_wheel_suppressed_when_mode_bit_off` |
| TS-6 | AltScreen OFF → no bytes | PASS — `fr1_wheel_inert_outside_alt_screen` |
| TS-8 | Shift+wheel = plain wheel | PASS (structural — helper ignores modifier; positive case covers it) |
| TS-9 | `Ctrl+Home → ESC[1;5H` | PASS — `pty::input::tests::ctrl_home_emits_csi_modifier_form` |
| TS-10 | `Ctrl+End → ESC[1;5F` | PASS — `ctrl_end_emits_csi_modifier_form` |
| TS-11 | `Ctrl+PageUp → ESC[5;5~` | PASS — `ctrl_pageup_emits_csi_modifier_form` |
| TS-12 | `Ctrl+Shift+PageDown → ESC[6;6~` | PASS — `ctrl_shift_pagedown_emits_csi_modifier_form` |
| TS-13 | `Ctrl+ArrowUp → ESC[1;5A` | PASS — `ctrl_arrow_up_emits_csi_modifier_form` + `ctrl_arrow_keys_emit_csi_modifier_form` |
| TS-14 | `Shift+F1 → ESC[1;2P` | PASS — `shift_f1_emits_csi_modifier_form` |
| TS-15 | `Ctrl+Alt+F5 → ESC[15;7~` | PASS — `ctrl_alt_f5_emits_csi_modifier_form` |
| TS-16 | Plain `Home → ESC[H` (regression) | PASS — `plain_home_unchanged_regression` |
| TS-17 | Plain `PageUp → ESC[5~` (regression) | PASS — `plain_pageup_unchanged_regression` |
| TS-18 | Plain `F1 → ESC OP` (regression) | PASS — `plain_f1_unchanged_regression` |
| TS-19 | Safe-URI OSC 8 cell → Some(link) + run | PASS — `fr3_osc8_safe_uri_returns_link_with_run` |
| TS-20 | `javascript:` URI → None (+ warn log) | PASS — `fr3_osc8_unsafe_uri_returns_none` |
| TS-21 | `hyperlink_id == 0` → None | PASS — `fr3_osc8_plain_cell_returns_none` |
| TS-22 | URI missing / empty → None | PASS — `fr3_osc8_missing_uri_returns_none` |
| TS-23 | Ctrl+hover OSC 8 in AltScreen → underline + hand cursor; regex skipped | IMPLEMENTED (helper unit-tested; full hover wire-through verified by code review at L698-705 + L759) |
| TS-24 | Ctrl+click OSC 8 in AltScreen → opener | IMPLEMENTED (`try_open_link_at_pointer` calls `detect_osc8_link_at` BEFORE the AltScreen short-circuit at L873-886) |
| TS-25 | Regression — Ctrl+click plain URL in MainScreen | DEFERRED — manual (see checklist below) |
| TS-26 | Regression — regex URL hover still suppressed in AltScreen | DEFERRED — manual (code review: L707 still has `&& !app.alt_screen`) |
| TS-27 | `Shift+PageUp` host scrollback chord still intercepted before `encode()` | DEFERRED — manual (out of automated scope; code review of `handle_special_chord` precedence confirmed by sdd.4-implement) |

**Scenario verdict**: **PASS** — TS-1..TS-24 covered by automated tests
or code review; TS-25/26/27 explicitly deferred to the manual checklist
below.

---

## 4. Performance Verification (code review)

| Check | Result |
|-------|--------|
| Wheel arrow bytes use `extend_from_slice` of a static slice | YES — `alternate_scroll_wheel_bytes` (window_host.rs L2239) binds `arrow: &[u8] = if lines > 0.0 { b"\x1b[A" } else { b"\x1b[B" }` — both are `'static` byte literals. The output `Vec` is pre-sized via `Vec::with_capacity(arrow.len() * count)` so no realloc during the per-notch `extend_from_slice` loop. |
| OSC 8 lookup uses cell-direct accessor (no row scan) | YES — `detect_osc8_link_at` reads the id with one `core.get_cell_hyperlink_id(col, row)` call (L1922), then one `core.get_hyperlink_uri(id)` lookup (L1926). The run-expansion loops (L1939, L1944) are O(run-length) bounded by the row width, which is fine for the hit-test path (run ≤ row width, single-digit µs). |
| CSI modifier encode allocates only a small per-call Vec | YES — `csi_mods_letter` / `csi_mods_tilde` use `format!(...).into_bytes()` producing ≤8 bytes per call. One small heap allocation per modified key event. Acceptable at this size; SPEC's "pre-sized 6-byte stack buffer" is an optimisation target not a correctness requirement. |

**Performance verdict**: **PASS** (smoke / code-review level — no
micro-benches required per VERIFICATION.md).

---

## 5. Security Verification (code review)

| Check | Result |
|-------|--------|
| OSC 8 click path calls `links::is_safe_uri` before `open_url` | YES — defence in depth: `detect_osc8_link_at` already validates at the helper layer (L1933) and returns `None` for unsafe; `try_open_link_at_pointer` re-validates at the click dispatch site (L878) before invoking `open_url`. |
| Unsafe URI emits a `log::warn!` and does not call the opener | YES — helper logs `"native-poc: refusing OSC 8 URI with unsafe scheme: {uri}"` (L1934) and returns `None` (no `DetectedLink` reaches the click path); the click branch also logs `"native-poc: refusing to open unsafe OSC 8 URI scheme: {url}"` (L881) and skips `open_url` if the helper is ever bypassed. |
| FR1 wheel bytes are static slices (no user-controlled string interpolation) | YES — `arrow` is bound to `b"\x1b[A"` or `b"\x1b[B"` (`'static` slice). No string interpolation; the only inputs are the four booleans (`alt_screen`, `mode_bit_on`, `setting_on`) and the float `lines` value, used only for length/direction. |

**Security verdict**: **PASS**.

---

## 6. Manual Checklist for the User

E2E harness does not exist in this project, so the items below cannot be
run by automation — they need a fresh build (`make build` or
`CARGO_TARGET_DIR=src-tauri/target-host cargo build --release
--manifest-path src-tauri/Cargo.toml`, run on user's explicit instruction
only per project rule) and a hands-on session in the launched terminal.

### Pre-flight

- [ ] Build a fresh release binary
      (`make build` or the release `cargo build` command above).
- [ ] Run the binary and confirm it launches a terminal as today.

### FR1 (DECSET 1007 — wheel→arrow in AltScreen)

- [ ] Launch Claude Code (`claude`) and scroll the wheel up — Claude
      Code's log/dialog scrolls.
- [ ] Scroll the wheel down — scrolls the other way.
- [ ] In `vim` (`vim /etc/profile`), wheel up/down moves the cursor /
      viewport.
- [ ] In `less /etc/profile`, wheel scrolls.
- [ ] In a shell (no AltScreen), wheel still moves eMterm's scrollback
      view (regression).
- [ ] `Shift+wheel` in Claude Code scrolls (Shift ignored — same as
      plain).
- [ ] Open Settings → Terminal → toggle "DECSET 1007" OFF. In Claude
      Code, wheel no longer scrolls Claude Code's log
      (regression-to-pre-change). Re-enable.
- [ ] In a TUI that opts out at runtime (e.g. `printf '\e[?1007l'`),
      wheel reverts to scrollback view.

### FR2 (CSI Modifier extension)

- [ ] In Claude Code, `Ctrl+Home` jumps to the top of the log;
      `Ctrl+End` jumps to the bottom.
- [ ] In Claude Code, `Ctrl+PgUp` / `Ctrl+PgDn` page jump (if the
      version supports it).
- [ ] In vim, `Ctrl+Right` moves a word right (if vim is mapped for
      it).
- [ ] Plain `Home` / `End` still moves to start/end of line
      (regression).
- [ ] `Shift+PageUp` still scrolls eMterm's scrollback view — does NOT
      reach Claude Code (host chord intercept still wins). **(TS-27)**

### FR3 (OSC 8)

- [ ] In Claude Code, find a PR ID `#1` in a log message. Hold `Ctrl`
      and hover over it — the cell underlines and the cursor becomes a
      hand.
- [ ] `Ctrl+click` opens the PR page in the OS default browser.
- [ ] Synthetic safe-URI test (copy-paste into the terminal):
      ```
      printf '\e]8;;https://example.com\e\\link\e]8;;\e\\\n'
      ```
      Then repeat `Ctrl+hover` and `Ctrl+click` over the word `link`.
- [ ] Open `vim` and run `:!cat <file with OSC 8>` (puts the OSC 8 link
      in AltScreen). Confirm hover + click still works inside
      AltScreen.
- [ ] Malicious-scheme synthetic test (copy-paste):
      ```
      printf '\e]8;;javascript:alert(1)\e\\x\e]8;;\e\\\n'
      ```
      `Ctrl+click` over `x` does NOT launch anything;
      `~/.local/share/net.laser5.app.emterm/logs/emterm.log` contains a
      `warn` line about the unsafe URI.
- [ ] Regression: plain text `https://example.com` in MainScreen still
      detected by the existing regex hover (no behaviour change).
      **(TS-25)**
- [ ] Regression: plain text `https://example.com` in AltScreen is
      still NOT detected by the regex (AltScreen guard on regex path
      intact). **(TS-26)**

**Manual item count**: **22** (2 pre-flight + 8 FR1 + 5 FR2 + 7 FR3).

---

## 7. Known Pre-existing Failure (NOT a regression)

`tabs::tests::welcome_without_windows_leaves_group_none` fails on the
baseline HEAD `b29f83c` with this feature's working tree stashed
(`assertion failed: tab.mux_group.is_none()`). No file in
`src-tauri/src/tabs.rs` was touched by Phases 1/2/3 of this feature.
**Treat as out of scope for this verification pass.**

---

## 8. Final Verdict

**READY** — every automated check (file structure, SPEC compliance
matrix, scenario coverage for TS-1..TS-24, performance/security code
review) is green. TS-25/26/27 plus the 22-item manual checklist above
remain for the user to execute in a fresh release build before sign-off.
Once those pass, this feature is shippable.

The one failing test in the suite is pre-existing on the baseline
commit and unrelated to this branch.
