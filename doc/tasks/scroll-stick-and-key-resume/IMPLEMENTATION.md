# Implementation Plan: Scroll-stick on PTY output + auto-resume on key input

## Overview

Anchor the active tab's visible scrollback view across PTY output (`scroll-stick`) by carrying the per-pump scrollback-length delta `Δ` into the existing `App::on_pty_output` step, and snap the viewport back to the live tail whenever a user keystroke is forwarded to the PTY (`auto-resume`).

## Objectives

- Preserve the visible content while parked in `OffsetFromLive(n)` below scrollback capacity.
- Resume `Live` tracking on any key that `winit_key_to_bytes` translates to PTY bytes, leaving modifier-only / chord / dialog / IME paths untouched.
- Keep `term_core` untouched — reuse `TerminalCore::get_scrollback_length()` as the only signal.

## Prerequisites

### Development Environment

- Rust toolchain (workspace pinned via `rust-toolchain.toml`).
- `cargo` accessible from project root with `CARGO_TARGET_DIR=src-tauri/target` for tests / checks.
- No new external installs.

### Dependencies

- `src-tauri/src/scroll.rs::ScrollPosition` (unchanged).
- `src-tauri/src/app.rs::App` (signature extension on `on_pty_output`; wiring in `pump_all`).
- `src-tauri/src/window_host.rs` keyboard handler (new call site for `scroll_to_live`).
- `term_core::TerminalCore::get_scrollback_length()` (read-only use).
- No external crate additions.

## Architecture Overview

### Technology Stack

- **Language**: Rust (workspace member `src-tauri`).
- **Framework**: native windowed app (winit + wgpu + egui).
- **Key Libraries**:
  - `winit` — keyboard event source for the live-resume call site.
  - `term_core` — scrollback ring buffer; only `get_scrollback_length()` is consumed.
- **Feature gating**: all touched modules already live under the `gui` feature, so the CLI build (`--no-default-features`) is unaffected.

### Design Approach

- Option A from the discussion: derive `Δ` from `TerminalCore::get_scrollback_length()` sampled before and after the active tab's pump within `App::pump_all`. No new counters inside `term_core`.
- Push the delta into the existing `App::on_pty_output` step. That step is the single funnel for "new PTY bytes landed"; extending its signature keeps the state-machine ownership in one place.
- Auto-resume reuses the existing `App::scroll_to_live` — the only new code is one call inside the `winit_key_to_bytes → Some(bytes) → tab.write_input(bytes)` branch in `window_host`.

### Component Interaction

```
winit KeyboardInput (Pressed)
  → window_host handler
    → (modifier/chord/dialog/IME early-returns — unchanged)
    → winit_key_to_bytes
        → Some(bytes)  ▸ tab.write_input(bytes) ▸ app.scroll_to_live()   ◀ NEW
        → None         ▸ no-op for scroll state

App::pump_all
  ┌─ sample before_len from active tab core
  ├─ per-tab pump loop (unchanged)
  ├─ sample after_len from active tab core
  ├─ delta = saturating_sub(after_len, before_len)
  └─ if changed: App::on_pty_output(active_changed, delta)
        └─ if scroll_position is OffsetFromLive(n):
              n ← min(n + delta, settings.scrollback_lines)
              needs_full_redraw ← true
```

## Implementation Phases

### Phase 1: State-machine API extension

**Goal**: Replace `App::on_pty_output(active_changed)` with a version that consumes the per-pump scrollback delta and advances `OffsetFromLive(n)` by it, leaving `Live` and the search-dirty branch behavior unchanged.

**Files to Create**: none.

**Files to Modify**:
- `src-tauri/src/app.rs` — `on_pty_output` signature extension, branch logic, and rewritten doc comment (FR3).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `App::on_pty_output(active_changed, scrollback_delta)` | Funnel for "active pump produced bytes": flag search overlay dirty (active-only), and advance `OffsetFromLive(n)` by `scrollback_delta` clamped to `settings.scrollback_lines`. | Called from `pump_all` after at least one tab produced bytes. `scrollback_delta` is the active tab's `after_len - before_len` (saturating). | If `scroll_position` was `OffsetFromLive(n)`, it is `OffsetFromLive(min(n + Δ, scrollback_lines))` and `needs_full_redraw` is true. `Live` is left untouched (no new behavior). |

**Processing Flow**:

1. If `active_changed` and search overlay visible -> mark search buffer dirty (existing behavior, unchanged).
2. Inspect `scroll_position`:
   - `Live` -> no-op for the scroll branch (no redraw flag mutation from this branch).
   - `OffsetFromLive(n)` -> compute new offset as saturating-add of `n` and `Δ`, then clamp to `settings.scrollback_lines`; assign back and set `needs_full_redraw`.

**Implementation Steps**:

1. **Signature extension** — Add the `scrollback_delta: u32` parameter to `on_pty_output`; do not introduce defaults or wrappers.
2. **Branch rewrite** — Replace the previous "matches OffsetFromLive then only set needs_full_redraw" with the saturating-add + clamp + assign behavior described above.
3. **Doc comment rewrite (FR3)** — Replace the misleading "the offset stays anchored because `term_core`'s ring buffer shifts the old content into scrollback under us" wording with the capacity-bound delta-follow contract.
4. **Test-call-site sweep** — Every test invocation of `on_pty_output` in `src-tauri/src/app.rs` is updated to the new signature; tests that previously exercised the old branch pass `0` unless they specifically assert delta follow.

**Dependencies**: Blocks Phase 2 (delta wiring needs the new signature) and Phase 3 (no, Phase 3 is independent but conventionally lands after the API change).

**Testing Approach**:
- Unit: see TS-1..TS-4 in VERIFICATION.md.
- Integration: none new; existing `App` tests must continue to pass with the updated argument.
- E2E: not applicable to this phase.
- Manual: none.

**Acceptance Criteria**:
- [ ] `on_pty_output(active_changed: bool, scrollback_delta: u32)` is the only public signature.
- [ ] In `Live`, the function is a no-op for `scroll_position` and the redraw flag.
- [ ] In `OffsetFromLive(n)`, the post-state is `OffsetFromLive(min(n + Δ, settings.scrollback_lines))`.
- [ ] Doc comment reflects the capacity-bound delta-follow contract.

**Estimated Effort**: small.

---

### Phase 2: pump_all delta wiring

**Goal**: Sample the active tab's scrollback length before and after the per-tab pump loop in `App::pump_all`, and pass the saturating difference into `on_pty_output`.

**Files to Create**: none.

**Files to Modify**:
- `src-tauri/src/app.rs` — local variables in `pump_all` for `before_len` / `after_len` and the call into `on_pty_output`.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `pump_all` pre-loop active-tab sampler | Read the active tab's scrollback length under its lock before any tab pump runs. | `self.active` is in range or the tabs vec is empty. | `before_len` equals the active tab's `get_scrollback_length()` at pump entry, or `0` when no active tab exists. |
| `pump_all` post-loop active-tab sampler | Read the active tab's scrollback length under its lock after the per-tab pump loop and the reap-related fix-ups have not yet shifted `self.active`. | Called only when `changed` is true. | `after_len` equals the active tab's `get_scrollback_length()` at this moment; the saturating difference from `before_len` is `Δ`. |
| `App::on_pty_output(active_changed, Δ)` call | Single point of truth for "new bytes arrived this pump"; carries the delta into Phase 1's state machine. | `changed == true`. | State machine update per Phase 1 contract. |

**Processing Flow**:

1. At the top of `pump_all`, before the per-tab loop, sample `before_len` from the active tab (or `0` if none).
2. Run the existing per-tab loop unmodified.
3. Immediately after the per-tab loop, before reap, sample `after_len` from the active tab.
4. Compute `Δ = after_len.saturating_sub(before_len)`.
5. If `changed`, invoke `on_pty_output(active_changed, Δ)`.

**Implementation Steps**:

1. **Pre-loop sampling** — Acquire the active tab's core lock briefly and read scrollback length; release before the pump loop begins.
2. **Post-loop sampling** — Same shape as step 1, placed just after the per-tab loop and any active-tab bookkeeping that is *not* `on_pty_output`.
3. **Wire delta into on_pty_output call** — Replace the existing single-argument call with the two-argument form.
4. **Empty-tab guard** — If the active index is out of range at either sample point, both samples yield `0`; the resulting `Δ == 0` matches "no delta" (no behavior change).

**Dependencies**: Requires Phase 1 (new signature). Blocks Phase 4 (test updates).

**Testing Approach**:
- Unit: existing `pump_all` tests must continue to pass; no new unit test added because the sampling is structural and exercised by the existing PTY-output-driven tests.
- Integration: not applicable.
- E2E: see Phase 3 manual / TS-6..TS-8.
- Manual: covered by Phase 3 manual scenarios.

**Acceptance Criteria**:
- [ ] `pump_all` samples `scrollback_len` on the active tab both before and after the per-tab pump loop.
- [ ] `on_pty_output` is invoked with the saturating-difference of those samples.
- [ ] No regression in existing `pump_all` behavior (search dirty, notifications, reap, mux bookkeeping).

**Estimated Effort**: small.

---

### Phase 3: Key-input live-resume call site

**Goal**: Call `App::scroll_to_live()` immediately after `tab.write_input(bytes)` in the `winit_key_to_bytes → Some(bytes)` branch of the `window_host` `KeyboardInput { Pressed }` handler.

**Files to Create**: none.

**Files to Modify**:
- `src-tauri/src/window_host.rs` — one additional call inside the existing key-encoder branch.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Key-encoder branch in `KeyboardInput { Pressed }` | Forward translated keystrokes to the PTY (inside the active-tab borrow) and, after that borrow ends, snap the viewport to live tail. | All upstream early-returns (search overlay, profile selector, mux dialog, IME consume, special chord, mux prefix latch, settings keybinds) have not consumed the event, and `winit_key_to_bytes` returned `Some(bytes)`. | `tab.write_input(bytes)` has been called, and `App::scroll_position == Live` with `needs_full_redraw == true`. The mutable `App` borrow needed for `scroll_to_live` is taken only after the shared `Tab` borrow ends. |

**Processing Flow**:

1. The existing handler chain runs unmodified through all early-returns up to the encoder branch.
2. `winit_key_to_bytes(&event, mods, target)` is evaluated:
   - `None` (modifier-only or unhandled key) -> no action; viewport state unchanged.
   - `Some(bytes)` -> call `tab.write_input(bytes)` and record that the key was forwarded.
3. After the `if let Some(tab) = self.app.active_tab() { ... }` block releases its shared borrow of `self.app`, if the key was forwarded, call `App::scroll_to_live()`.

**Implementation Steps**:

1. **Capture "forwarded" outside the active-tab borrow** — `active_tab()` returns `Option<&Tab>`, an immutable borrow of `self.app`. `scroll_to_live` is `&mut self` and cannot be called while that borrow is alive, so the resume call must run *after* the `if let` block. Use a local boolean (or equivalent) inside the block to record whether `winit_key_to_bytes` returned `Some(bytes)`, then invoke `scroll_to_live` conditionally below the block.
2. **No new guards** — The existing early-returns (search overlay, profile selector, mux dialog, IME consume, special chord, mux prefix latch, settings keybinds) prevent the branch from being reached when those modes own the keyboard; no additional gating is needed.
3. **Alt-screen** — No special handling; `scroll_to_live()` re-assigns `Live`, which is the alt-screen invariant anyway.

**Dependencies**: Independent of Phases 1 and 2 in code terms; sequenced after them for review continuity. Blocks Phase 4 (manual verification scenarios reference this behavior).

**Testing Approach**:
- Unit: not feasible — the call lives inside the winit event handler, which has no test harness in this project. Behavior verified via manual scenarios.
- Integration: not applicable.
- E2E: none (no E2E framework in repo).
- Manual: TS-6, TS-7, TS-8 in VERIFICATION.md.

**Acceptance Criteria**:
- [ ] PTY-forwarded keystrokes snap `scroll_position` to `Live`.
- [ ] Modifier-only keystrokes do not snap.
- [ ] Scrollback chords (Shift+PageUp / PageDown / Home / End) do not snap.
- [ ] Search overlay / profile selector / mux dialog / IME composition / mux prefix latch do not snap.

**Estimated Effort**: small.

---

### Phase 4: Test updates and unit coverage

**Goal**: Update every existing `App::on_pty_output` call in tests to the new signature and add unit coverage for the new branch behavior.

**Files to Create**: none.

**Files to Modify**:
- `src-tauri/src/app.rs` — test module (`#[cfg(test)] mod tests { ... }`).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Updated existing tests | Compile and pass with the new two-argument signature. | A test currently calls `on_pty_output(bool)`. | The same test calls `on_pty_output(bool, 0)` (or a meaningful delta) and asserts the same observable outcomes. |
| New tests (TS-1..TS-4) | Lock down the new `OffsetFromLive` advance behavior. | None. | The four scenarios in VERIFICATION.md pass on `cargo test --lib`. |

**Processing Flow**:

1. Sweep `app.rs` for every `on_pty_output(` call inside the test module and add the `0` delta argument unless the test specifically exercises the new branch.
2. Add the four new unit tests (TS-1..TS-4 in VERIFICATION.md). Each constructs an `App`, sets `scroll_position` and `settings.scrollback_lines` as needed, invokes `on_pty_output`, and asserts on `scroll_position` and `needs_full_redraw`.
3. Verify `tabs.rs` replay tests (the known non-deterministic ones) still pass under `--test-threads=1` (existing project constraint, no new work).

**Implementation Steps**:

1. **Mechanical sweep** — Update every `on_pty_output(` call site to the new signature.
2. **Add four new tests** — Use the same construction style as existing `on_pty_output_in_live_is_noop` (no shared fixtures, explicit per-test `App`).
3. **Run the lib test suite** — `cargo test --lib` with the project-mandated `CARGO_TARGET_DIR` and `--manifest-path`.

**Dependencies**: Requires Phase 1 (signature). Final phase.

**Testing Approach**:
- Unit: TS-1..TS-4 in VERIFICATION.md.
- Integration: existing `pump_all` integration is exercised indirectly by the existing tests.
- E2E: not applicable.
- Manual: not applicable to this phase.

**Acceptance Criteria**:
- [ ] All previously-passing tests still pass.
- [ ] Four new tests for the `OffsetFromLive` advance behavior pass.
- [ ] CLI feature check (`cargo check --no-default-features`) still passes.

**Estimated Effort**: small.

---

## Complete File Structure

```
src-tauri/
  src/
    app.rs              # signature change + pump_all sampling + new tests
    scroll.rs           # unchanged
    window_host.rs      # one new scroll_to_live() call site
crates/
  term_core/            # unchanged (read-only consumer of get_scrollback_length)
doc/tasks/scroll-stick-and-key-resume/
  要件定義書.md
  SPEC.md
  IMPLEMENTATION.md     # this file
  VERIFICATION.md
  sdd.yaml
  tasks.yaml
```

## Testing Strategy

- **Unit**: target the new `on_pty_output` branch with four scenarios (Live / OffsetFromLive advance / clamp at capacity / zero delta). All existing `app.rs` tests retained.
- **Integration**: existing `pump_all`-driven tests in `src-tauri/src/app.rs` continue to cover the delta wiring indirectly.
- **E2E**: no E2E framework in this repo (per `test/README.md`).
- **Manual**: scroll-stick and live-resume behaviors are validated by the user against the running release binary (three scenarios in VERIFICATION.md).

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none new) | — | All required functionality already in-tree. |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Tab reap shifts `self.active` between `before_len` and `after_len` samples, producing a meaningless delta. | Low | Cosmetic (single-frame misalignment); user has already lost the buffer they were reading. | Accept per discussion; no implementation guard. |
| Capacity-boundary frame where evict + push coexist produces `Δ == 0` while contents shift by one row. | Low | One-row drift in scrollback view at the moment of capacity transition. | Accept per discussion (matches "you can't park forever"). |
| Tests parallelism unrelated to this change (known `tabs.rs` replay flake). | Low | Spurious failures only when running default-parallel. | Run with `--test-threads=1` if a flake is observed (documented in `test/README.md`). |
| Hidden call site of `on_pty_output` outside `app.rs` not caught by the test sweep. | Very low | Compile error. | Compile + grep sweep before final commit. |

## Open Questions

- [ ] None. The discussion document closed with "未解決の疑問: なし（実装に進める粒度）" and all options were resolved before Phase 1 started.

## Success Metrics

- [ ] All FR1 / FR2 / FR3 implemented; NFR1 / NFR2 / NFR3 satisfied.
- [ ] Four new unit tests pass.
- [ ] Existing `cargo test --lib` suite passes (modulo the known `tabs.rs` flake under default parallelism).
- [ ] `cargo check --no-default-features` passes.
- [ ] Manual verification of scroll-stick and live-resume confirmed by the user.
