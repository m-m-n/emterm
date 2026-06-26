# Feature: Scroll-stick on PTY output + auto-resume on key input

## Overview

Anchor the visible content while the user is parked in scrollback (`ScrollPosition::OffsetFromLive(n)`), and snap back to live tail when the user presses any key that would normally be sent to the PTY. Today the visible content shifts down by one row per PTY-pushed line while capacity is not yet reached, because `visible_start = scrollback_len - scroll_offset` advances with `scrollback_len`. The fix preserves the visual anchor by incrementing `scroll_offset` by the same delta `Δ`, and reuses the existing `scroll_to_live()` from the key-input path so typing automatically returns to live tail.

## Objectives

- While `OffsetFromLive(n)` and below scrollback capacity, the row currently displayed at any given screen position stays put when new PTY lines arrive.
- Pressing any key that `winit_key_to_bytes` translates into PTY bytes resumes live-tail tracking (`scroll_position = Live`).
- No changes to `term_core` — reuse `TerminalCore::get_scrollback_length()` as the sole signal.

## User Stories

### US1: Stay anchored while reading scrollback

As an eMterm user reading scrollback, I want PTY output not to shift the rows under my eyes, so that I can finish reading without losing my place.

**Acceptance Criteria:**
- [ ] In `OffsetFromLive(n)` and below scrollback capacity, a single PTY-pushed line leaves the contents of every visible row unchanged on the next frame.
- [ ] `scroll_offset()` increased by exactly the number of lines pushed during the pump (`Δ`), clamped to `settings.scrollback_lines`.
- [ ] When scrollback capacity has been reached, `Δ == 0` and the visible content shifts (existing behaviour — accepted).

### US2: Type to resume

As an eMterm user, I want any key I type to bring me back to the live tail, so that the echo of what I'm typing is immediately visible.

**Acceptance Criteria:**
- [ ] Pressing any key for which `winit_key_to_bytes(&event, mods, target)` returns `Some(bytes)` switches `scroll_position` to `Live` *and* writes the bytes to the PTY.
- [ ] Pressing a bare modifier (Shift / Ctrl / Alt alone) leaves `scroll_position` unchanged (because `winit_key_to_bytes` returns `None`).
- [ ] Scrollback chords (Shift+PageUp / Shift+PageDown / Shift+Home / Shift+End) do **not** snap to live (they take the `handle_special_chord` early-exit path).
- [ ] Keys consumed by search overlay / profile selector / mux dialog / IME / mux prefix latch / settings keybinds do **not** snap to live (they never reach the `write_input` call site).

## Technical Requirements

### Functional Requirements

- **FR1 (scroll-stick / F01):** Track the scrollback length of the **active tab** across one `pump_all` pass and, on the post-pump `App::on_pty_output` call, advance `scroll_position` from `OffsetFromLive(n)` to `OffsetFromLive(min(n + Δ, settings.scrollback_lines))` where `Δ = after_len.saturating_sub(before_len)`. `Live` is left untouched.
- **FR2 (key-resume / F02):** In `window_host`'s `KeyboardInput` handler, immediately after `tab.write_input(bytes)` for the `winit_key_to_bytes` → `Some(bytes)` case, call `self.app.scroll_to_live()`.
- **FR3 (doc fix / F03):** Replace the misleading "the offset stays anchored because `term_core`'s ring buffer shifts the old content into scrollback under us" sentence on `App::on_pty_output` with text that reflects the new contract (capacity-bound delta following).

### Non-Functional Requirements

- **NFR1 — Performance:** Two extra `core.lock().get_scrollback_length()` calls per `pump_all` pass on the active tab. The call returns `RingBuffer::len()` in O(1); no measurable overhead.
- **NFR2 — Safety:** No `term_core` modification. No new global state. No allocations on the hot path.
- **NFR3 — Backward compatibility:** Existing call sites of `App::on_pty_output` (both production in `pump_all` and unit tests in `app.rs`) must update to the new signature. The new arg is a `u32` delta.

## Implementation Approach

### Architecture

**Affected components:**
```
┌────────────────────────────────────────────────────────────┐
│ window_host (winit event loop)                             │
│   KeyboardInput Pressed                                    │
│     ├─ winit_key_to_bytes(...) → Some(bytes)               │
│     │     ├─ tab.write_input(bytes)                        │
│     │     └─ self.app.scroll_to_live()  ← NEW              │
│     └─ (otherwise unchanged)                               │
└────────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────────────────┐
│ App                                                        │
│   pump_all()                                               │
│     ├─ before_len = active core.get_scrollback_length()    │
│     ├─ for tab in self.tabs: tab.pump() ...                │
│     ├─ after_len  = active core.get_scrollback_length()    │
│     ├─ delta = after_len.saturating_sub(before_len)        │
│     └─ if changed: self.on_pty_output(active_changed, delta)│
│                                                            │
│   on_pty_output(active_changed: bool, scrollback_delta: u32)│
│     ├─ search dirty-flag (unchanged)                       │
│     └─ if OffsetFromLive(n):                               │
│           n = min(n + scrollback_delta, scrollback_lines)  │
│           scroll_position = OffsetFromLive(n)              │
│           needs_full_redraw = true                         │
└────────────────────────────────────────────────────────────┘
```

`term_core` is **not** modified.

### Data Flow

```
PTY bytes ──▶ Tab::pump() ──▶ term_core RingBuffer
                                 │
                                 ▼
            after_len = get_scrollback_length()
                                 │
                                 ▼
         delta = after_len - before_len   (saturating)
                                 │
                                 ▼
         App::on_pty_output(active_changed, delta)
                                 │
                                 ▼
         OffsetFromLive(n) → OffsetFromLive(n + delta clamped)
```

### API Design

This is a Rust internal-API change.

#### Change 1: `App::on_pty_output` signature

**Before** (`src-tauri/src/app.rs:3547`):
```rust
pub fn on_pty_output(&mut self, active_changed: bool) {
    if active_changed && self.search.visible {
        self.search.mark_buffer_dirty();
    }
    if matches!(self.scroll_position, ScrollPosition::OffsetFromLive(_)) {
        self.needs_full_redraw = true;
    }
}
```

**After:**
```rust
/// React to new PTY output on the active tab. `scrollback_delta` is the
/// number of rows that spilled into scrollback during this `pump_all`
/// pass (`after_len - before_len` of the active tab's
/// `get_scrollback_length`). It is used to keep the visible content
/// anchored when the user is parked at an `OffsetFromLive` view:
///
/// * Below scrollback capacity, every pushed row grows `scrollback_len`
///   by 1, so the visible-row formula `scrollback_len - scroll_offset`
///   would advance unless we increment `scroll_offset` by the same Δ.
/// * Once capacity is reached, `scrollback_len` is pinned and Δ == 0
///   — the visible row composition shifts (as intended; the user has
///   accepted that "you can't keep your place forever").
///
/// `active_changed` is `true` only when the **active** tab produced
/// the new bytes; the search overlay is resolved against the active
/// tab's buffer.
pub fn on_pty_output(&mut self, active_changed: bool, scrollback_delta: u32) {
    if active_changed && self.search.visible {
        self.search.mark_buffer_dirty();
    }
    if let ScrollPosition::OffsetFromLive(n) = self.scroll_position {
        let max = self.settings.scrollback_lines;
        let new_n = n.saturating_add(scrollback_delta).min(max);
        self.scroll_position = ScrollPosition::OffsetFromLive(new_n);
        self.needs_full_redraw = true;
    }
}
```

#### Change 2: `pump_all` — sample `scrollback_len` before and after

In `App::pump_all` (`src-tauri/src/app.rs:2692`), capture the active tab's
`scrollback_len` *before* the per-tab pump loop and *after* it, then pass
the saturating difference into `on_pty_output`:

```rust
let active = self.active;
let before_len = self
    .tabs
    .get(active)
    .map(|t| t.core.lock().get_scrollback_length())
    .unwrap_or(0);

// ... existing per-tab loop unchanged ...

if changed {
    let after_len = self
        .tabs
        .get(active)
        .map(|t| t.core.lock().get_scrollback_length())
        .unwrap_or(before_len);
    let delta = after_len.saturating_sub(before_len);
    self.on_pty_output(active_changed, delta);
}
```

Notes:
- We sample the active tab specifically because background-tab output is
  not what the visible-row anchor depends on.
- If the active tab index moves during the pump (reap path at
  `app.rs:3043-3047`), the `after_len` look-up uses the *post-reap*
  `self.active`. Reading scrollback length on a different tab makes the
  delta meaningless for the anchor; that case is rare and acceptable
  (the user has just lost the tab they were reading anyway, so the
  `Live` snap from `set_alt_screen` / tab reactivation dominates).
  Implementation MAY guard against this by caching the active-tab id
  before pump and skipping the delta if the active index shifted —
  optional and only if a test surfaces a regression.

#### Change 3: `window_host` key path — call `scroll_to_live` after `write_input`

In `WindowEvent::KeyboardInput { event, .. }` for `ElementState::Pressed`
(`src-tauri/src/window_host.rs:2473-2489`), call `scroll_to_live` *after*
the `if let Some(tab) = self.app.active_tab()` block releases its shared
borrow of `self.app`. (`active_tab()` returns `Option<&Tab>`, which
borrows `self.app` immutably; `scroll_to_live` needs `&mut self`, so it
cannot be called while the `tab` reference is alive.) Capture whether a
key was forwarded into a local flag, exit the block, then call
`scroll_to_live` conditionally:

```rust
let forwarded = if let Some(tab) = self.app.active_tab() {
    let target = if tab.mux_session_name.is_some() {
        EncodeTarget::PosixPty
    } else {
        EncodeTarget::HostPty
    };
    if let Some(bytes) = winit_key_to_bytes(&event, mods, target) {
        tab.write_input(bytes);
        true
    } else {
        false
    }
} else {
    false
};
if forwarded {
    // FR2: any key that we forward to the PTY also snaps the
    // viewport back to live tail. Bare modifiers return None
    // (so `forwarded == false`); search-overlay / profile-selector /
    // mux-dialog / IME-consume / special-chord / mux-prefix /
    // settings-keybinds all early-return before reaching here.
    self.app.scroll_to_live();
}
```

`scroll_to_live` is a no-op visually if `scroll_position` is already
`Live` (it re-assigns `Live` and sets `needs_full_redraw`); the existing
behavior is harmless in that case.

#### Change 4: Update all `on_pty_output` call sites in tests

`grep "on_pty_output(" src-tauri/src/app.rs` returns the production
call site (`app.rs:3040`) plus eight test invocations
(`app.rs:4053, 4085, 4094, 4330, 4378, 4408, 4415, 4815`). Update each
to pass a `scrollback_delta` argument; for tests that do not exercise
the new branch, `0` is the right value.

### Dependencies

**Internal Dependencies:**
- `crate::scroll::ScrollPosition` — unchanged
- `crate::app::App` — `pump_all`, `on_pty_output`, `scroll_to_live` (existing)
- `term_core::TerminalCore::get_scrollback_length()` — unchanged
- `crate::window_host` — `winit_key_to_bytes` (unchanged)

**External Dependencies:** none.

### File Structure

No new files. Modifications:

```
src-tauri/src/
  app.rs             # on_pty_output signature + pump_all delta wiring + tests
  window_host.rs     # KeyboardInput handler: append scroll_to_live() call
```

## Test Scenarios

### Unit Tests

Add to the existing `#[cfg(test)] mod tests` block in `src-tauri/src/app.rs`:

- [ ] `on_pty_output_in_live_ignores_delta_and_stays_live`
  - Setup: `App::new(...)`, `scroll_position = Live`.
  - Action: `app.on_pty_output(true, 5)`.
  - Assert: `scroll_position` stays `Live`. `needs_full_redraw` unchanged from prior state (no need to mutate it from `Live`).
- [ ] `on_pty_output_in_offset_adds_delta`
  - Setup: `scroll_position = OffsetFromLive(10)`, `settings.scrollback_lines = 1000`.
  - Action: `app.on_pty_output(true, 3)`.
  - Assert: `scroll_position == OffsetFromLive(13)`, `needs_full_redraw == true`.
- [ ] `on_pty_output_in_offset_clamps_to_scrollback_lines`
  - Setup: `scroll_position = OffsetFromLive(995)`, `settings.scrollback_lines = 1000`.
  - Action: `app.on_pty_output(true, 10)`.
  - Assert: `scroll_position == OffsetFromLive(1000)` (clamped, not 1005).
- [ ] `on_pty_output_zero_delta_in_offset_preserves_offset_but_sets_redraw`
  - Setup: `scroll_position = OffsetFromLive(7)`.
  - Action: `app.on_pty_output(false, 0)`.
  - Assert: `scroll_position == OffsetFromLive(7)`, `needs_full_redraw == true` (the explicit branch documents intent — keeping the existing observable contract).
- [ ] `on_pty_output_search_overlay_dirties_on_active_change`
  - (existing test if any — confirm it still passes with the new `0` delta argument)

### Integration Tests

No new integration test required. The `pump_all` glue is exercised by
the existing `pump_all` tests in `src-tauri/src/app.rs` (search dirty,
notification fan-out, reap path). Update those tests to call the new
`on_pty_output(active_changed, delta)` signature.

### E2E Tests

**Existing E2E tests**: None (test/README.md, Phase 0.3 detection).
**Run command**: Not detected.

Manual verification by the user (the test/README.md does say E2E is
manual). Specifically:
- [ ] Run `cat /var/log/syslog` (or any long-output command), scroll up
  a few rows with Shift+PageUp, then watch a stream like
  `while true; do date; sleep 1; done`. The scrolled-up content stays
  on the same screen rows.
- [ ] In the same setup, type a single key — viewport snaps to live tail.
- [ ] In the same setup, press Shift / Ctrl alone — viewport stays put.

### Edge Cases

- [ ] **Capacity-bound** — When `scrollback_len == settings.scrollback_lines` (ring full), pump pushes `k > 0` lines but `after_len == before_len`, so `Δ == 0` and `scroll_position` stays put while the *contents* of scrollback shift by one row. Accepted: this matches "if you stay parked long enough, eventually the buffer rolls under you".
- [ ] **Mixed evict + push frame** — One frame pushes `k` lines past the cap; `before_len == cap`, `after_len == cap`, `Δ == 0`. Same as above. The "off by one" scenario from the discussion (`evict + push` mid-frame) is impossible to distinguish from "fully capped" using `RingBuffer::len()` alone — and the discussion accepted this drift.
- [ ] **Alt-screen** — `scroll_position` is forced to `Live` by `set_alt_screen(true)` (`app.rs:3577`). `on_pty_output` then hits the `Live` branch and the delta is ignored. No additional guard needed.
- [ ] **Tab reap during pump** — If the active tab is reaped (`exited`) between `before_len` capture and the post-pump sample, `self.tabs.get(self.active)` reads a *different* tab's length and the delta is meaningless. Discussion accepts this as out-of-scope; the user has lost the buffer they were reading anyway, and the reap path follows up with an `on_pty_output` against the new active tab in subsequent pumps.
- [ ] **Mux mode** — The active tab in mux mode receives `PtyOutput` frames that update the local `term_core` exactly like a local PTY (`tab.pump` decodes the bridge frames into the same core). The same `get_scrollback_length()` reading applies.
- [ ] **Search overlay open** — Existing key-handler early-return (`window_host.rs:2305`) prevents the `winit_key_to_bytes → write_input → scroll_to_live` branch from running. `scroll_position` is untouched.
- [ ] **mux prefix latch** — `observe_mux_key` consumes the key (`mux_consumed == true`, `window_host.rs:2374`), then the `winit_key_to_bytes → write_input → scroll_to_live` branch is skipped because the latch consumed it. The *next* (post-latch) plain key passes through and snaps to live as expected.
- [ ] **IME composition** — The IME backend's `dispatch_key_event_via_ime` returns `Consumed` (`window_host.rs:2337`) and the handler `return`s early. No `scroll_to_live` call.
- [ ] **Scrollback chords (Shift+PageUp/Down, Shift+Home/End)** — `handle_special_chord` (`window_host.rs:2358`) consumes them via the `handled` flag, so the encoder branch is skipped entirely.

### Performance Tests

- [ ] Not required. Two extra `lock + read len` ops per `pump_all` pass on an O(1) field — well under noise threshold of the existing `mux_throughput.rs` integration test.

## Security Considerations

- Not applicable. The change is a pure internal state update.

## Error Handling

### Error Codes

None — the change does not introduce any new error paths. All arithmetic
uses `saturating_add` and `min` to prevent overflow / out-of-range
values.

## Performance Optimization

### Performance Goals

- Hot path (`pump_all`): no measurable overhead vs. baseline.

### Optimization Strategies

- The `lock + read len` pair is the cheapest available signal. No need
  for a delta counter inside `term_core` (option B in the discussion)
  because the visual difference at "capacity ± 1" is one frame, which
  the user has accepted.

### Caching Strategy

- None.

## Success Criteria

- [ ] All FR1 / FR2 / FR3 implemented.
- [ ] All new unit tests pass.
- [ ] `cargo check --no-default-features` still passes (CLI build does
      not regress — `App::on_pty_output` is GUI-feature-gated already
      via `#[cfg(feature = "gui")]` on its module).
- [ ] Existing `app.rs` test suite passes with the updated signature.
- [ ] Manual verification confirms US1 / US2 acceptance criteria.
- [ ] `App::on_pty_output` doc comment updated.

## Open Questions

> **Note**: 未解決の要件は sdd.yaml で `status: tbd` として管理されています。
> `/em-sdd:sdd.2-create-plan` の実行前に解決してください。

- None. The discussion document explicitly closes with `未解決の疑問: なし（実装に進める粒度）`.

## References

- Discussion document: `tmp/discussion-scroll-stick-and-key-resume.md`
- Requirements (Japanese): `doc/tasks/scroll-stick-and-key-resume/要件定義書.md`
- `src-tauri/src/scroll.rs` — `ScrollPosition` enum
- `src-tauri/src/app.rs:3262-3329` — existing scroll API (`scroll_up_by`, `scroll_down_by`, `scroll_set_offset`, `scroll_to_top`, `scroll_to_live`)
- `src-tauri/src/app.rs:3547` — current `on_pty_output`
- `src-tauri/src/app.rs:2692` — `pump_all`
- `src-tauri/src/window_host.rs:1135-1141` — visible-row formula
- `src-tauri/src/window_host.rs:2484-2487` — key-input → `write_input`
- `crates/term_core/src/ring_buffer.rs` — scrollback ring buffer
