# Feature: Pin Viewport When Scrolled Up

## Overview

When the user has scrolled up to view past output (`scrollOffset > 0`), incoming PTY output that pushes lines into the scrollback must not shift the displayed lines on screen. The viewport is pinned to the currently visible absolute row range. When the scrollback capacity is exceeded, the viewport clamps at the top of the available scrollback and older lines flow off the top.

When the viewport is at the bottom (`scrollOffset === 0`), the existing follow-the-tail behavior is preserved.

## User Stories

### US1: Reading past logs while output continues
As a terminal user, I want my scrollback view to stay pinned on the lines I am reading when new output arrives, so that I can finish reading without the screen scrolling away.

**Acceptance Criteria:**
- [ ] Scroll up so `scrollOffset > 0`, then trigger continuous PTY output: the visible absolute rows do not change frame-to-frame as long as those rows remain inside the scrollback ring.
- [ ] `scrollOffset === 0` keeps the existing follow-the-tail behavior.

### US2: Falling off the top of the ring
As a terminal user, when I am pinned near the oldest scrollback line and the ring evicts the lines I am viewing, the screen should remain pinned at the oldest available line rather than jumping back to the bottom.

**Acceptance Criteria:**
- [ ] When pinned scrolling would require `scrollOffset > scrollbackLength`, the offset clamps to `scrollbackLength` (top of scrollback) and the topmost row of the display shows the oldest available scrollback line.

### US3: Returning to live tail
As a terminal user, scrolling all the way down returns me to the live tail and new output is appended at the bottom as before.

**Acceptance Criteria:**
- [ ] After `scrollDown` brings `scrollOffset` to 0, subsequent PTY output appears at the bottom of the viewport.

## Technical Requirements

### Functional Requirements

- **FR1: Pin offset on PTY scrollback growth** — When the active buffer's `scrollbackLength` increases by `Δ` lines as a result of PTY-driven scroll-up (a line is pushed from the viewport into the scrollback), and the renderer's `scrollOffset > 0`, the renderer increases `scrollOffset` by `Δ` so that `scrollbackLength - scrollOffset` (the absolute index of the topmost visible row) is preserved.

- **FR2: Follow-tail when offset is zero** — When `scrollOffset === 0`, no adjustment is performed. New PTY output appears at the bottom of the viewport (existing behavior).

- **FR3: Clamp at scrollback top** — After the increase in FR1, if `scrollOffset > scrollbackLength`, clamp `scrollOffset` to `scrollbackLength`. In this state, the topmost on-screen row is the oldest scrollback line, and subsequent PTY-driven evictions cause that row to be replaced by the next-oldest available line (lines flow off the top).

- **FR4: User-initiated scroll unchanged** — Wheel, keyboard (PageUp/PageDown/Home/End/Up/Down), and `setScrollOffset()` callers continue to set `scrollOffset` directly. The pinning behavior only applies to growth originating from PTY output and ANSI scroll handlers (`scrollUp` on the active buffer with no scroll region, or with the full-screen scroll region).

- **FR5: Alt-screen unaffected** — `state.getScrollbackLength()` always returns the **primary buffer's** scrollback length regardless of whether alt-screen is active (see `src/terminal/state.ts: getScrollbackLength()`). Therefore alt-screen entry/exit does not change the observed `scrollbackLength` (Δ === 0 on transition), and pinning produces no spurious adjustment. The alt screen itself accumulates no scrollback, so no PTY growth event is observed while it is active. Switching between primary and alt screen does not modify `scrollOffset`.

- **FR6: Partial scroll region unaffected** — When a partial DECSTBM scroll region is active (top != 0 OR bottom != rows-1), no line is pushed to the scrollback; only viewport rows rearrange in place. FR1 does not apply in this case.

### Non-Functional Requirements

- **NFR1 — Performance:** No per-frame work added beyond a comparison and a counter update on scrollback growth. PTY throughput must not regress measurably (`bun test`, existing canvas-renderer tests, mux-output-throughput E2E).
- **NFR2 — Compatibility:** Public renderer interface (`ITerminalRenderer.scrollUp/scrollDown/getScrollOffset/setScrollOffset`) is unchanged.

## Implementation Approach

### Architecture

Today's data flow (relevant portion):

```
PTY chunk
  → WASM parser (terminal_dispatch / csi_scroll / esc_handler / c0_handler)
  → TerminalCore.handle_scroll_up(n)  // pushes n viewport rows into scrollback_slim
  → scrollback_slim length grows by n
TS render path
  → renderer.render(state)
  → getVisibleLines(state, scrollOffset)
  → startIndex = scrollbackLength - scrollOffset
```

The bug: `startIndex` advances by `n` every time the scrollback grows, so the on-screen content shifts down by `n` rows even though the user has not moved.

The fix introduces a single hook between "scrollback grew" and "next render":

```
PTY chunk
  → WASM scroll
  → scrollbackLength grows by Δ
  → renderer observes Δ before next render
  → if scrollOffset > 0: scrollOffset = min(scrollOffset + Δ, scrollbackLength)
  → render uses adjusted scrollOffset → same absolute rows shown
```

### Observation point

The simplest robust observation point is the renderer itself, evaluated at the start of each render pass. The renderer caches the previous `scrollbackLength` it saw; on the next render it computes `Δ = scrollbackLength_now - scrollbackLength_prev` (only positive deltas are considered — clears or session swaps go through a separate reset path).

Pseudocode in `canvas-renderer.ts`:

```ts
private prevScrollbackLength: number = 0;

private adjustScrollOffsetForGrowth(state: TerminalState): void {
  const sbLen = state.getScrollbackLength();
  if (this.scrollOffset > 0 && sbLen > this.prevScrollbackLength) {
    const delta = sbLen - this.prevScrollbackLength;
    this.scrollOffset = Math.min(this.scrollOffset + delta, sbLen);
  }
  this.prevScrollbackLength = sbLen;
}
```

Called once per render pass before `getVisibleLines(state, this.scrollOffset)`.

### Reset cases

`prevScrollbackLength` must be reset (without triggering FR1 adjustment) in the following cases, all of which already exist as code paths in the renderer/state layer:

1. **Buffer clear** (ESC [3J / `clear`): `scrollbackLength` drops to 0. Implementation reads `if (sbLen < prevScrollbackLength) prevScrollbackLength = sbLen;` so a decrease silently re-baselines without growth handling.
2. **Active pane switch (mux)**: when the renderer's bound `state` or active pane changes, the next render's `prevScrollbackLength` is re-initialized from the new state's `getScrollbackLength()` via the same `prevScrollbackLength = sbLen` tail assignment.
3. **Alt-screen toggle**: `state.getScrollbackLength()` always returns the primary buffer's scrollback length, so alt-screen entry/exit observes Δ === 0 and is a no-op for the pin logic. (The active buffer pointer changes, but the scrollbackLength getter is hard-wired to primary in `state.ts:545-547`.)
4. **Resize / reflow**: `scrollbackLength` may change non-monotonically. Treated identically to clear (decrease → re-baseline; increase → growth handling).
5. **User scroll**: `setScrollOffset`, `scrollUp`, `scrollDown` from `ui-handler.ts` / `keyboard.ts` continue to operate on `scrollOffset` directly; no interaction with `prevScrollbackLength`.

### Files to modify

- `src/terminal/canvas-renderer.ts`
  - Add `prevScrollbackLength` field.
  - Add `adjustScrollOffsetForGrowth(state)` helper.
  - Call it at the start of `render()`, `forceRender()`, and `renderImmediate()` before the existing visible-line computation.
  - Reset `prevScrollbackLength = 0` in any existing initialization / pane-rebind path that also resets `scrollOffset` (search call sites of `this.scrollOffset = 0`).

### Files NOT modified

- `wasm/src/*` — no WASM changes; `scrollbackLength` is already exposed.
- `src/terminal/buffer-scroll.ts`, `src/terminal/unified-buffer.ts` — scroll operations themselves are unchanged.
- `src/terminal-app/ui-handler.ts`, `src/terminal-app/handlers/keyboard.ts` — user-initiated scroll paths unchanged.

## Test Scenarios

### Unit tests (`bun test`, in `src/terminal/canvas-renderer.test.ts`)

- **T1:** Given `scrollOffset = 5`, `scrollbackLength = 10`, simulate scrollback growth of 3 → expect `scrollOffset = 8`, `startIndex = scrollbackLength - scrollOffset = 5` unchanged.
- **T2:** Given `scrollOffset = 0`, `scrollbackLength = 10`, growth of 3 → `scrollOffset = 0` (no change).
- **T3:** Given `scrollOffset = 95`, `scrollbackLength = 100`, capacity-cap growth pushes `scrollbackLength` to 100 again (no growth) but with eviction → no offset change. Then growth that would push offset above `scrollbackLength` is clamped to `scrollbackLength`.
- **T4:** `scrollbackLength` drops to 0 (clear) → next growth treated as fresh baseline, no spurious offset change.
- **T5:** Alt-screen entry/exit observes Δ === 0 because `state.getScrollbackLength()` always returns the primary buffer's value. The pure function is invoked with `prev === curr` and returns `scrollOffset` unchanged. After exit, the next PTY-driven growth on the primary increases scrollOffset normally per FR1.

### Integration / E2E

- **E1 (existing suites):** `bun test`, `cargo test --manifest-path src-tauri/Cargo.toml`, and existing E2E specs (`scripts/run-e2e-docker.sh test`) must keep passing.
- **E2 (new E2E, `e2e-tests/specs/scroll-pin.e2e.js`):** Spawn shell, fill scrollback with `yes` (or `seq`), stop it with Ctrl-C, scroll up, then trigger another `yes`-like burst; capture screenshots before and during the burst at the same scroll position; assert the visible row at a fixed display index is identical.

## Open Questions

None.
