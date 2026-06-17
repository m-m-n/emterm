/**
 * Unit tests for mux-window-manager pure helpers.
 *
 * Only the state-free helpers are exercised here (`reorderMuxWindows`).
 * The heavier lifecycle functions (`switchMuxWindow`, `handleMuxPaneCreated`,
 * etc.) depend on the full WASM/PTY/renderer stack and are covered by E2E.
 */

import { describe, expect, test } from "bun:test";

import { reorderMuxWindows, type MuxReorderContext } from "./mux-window-manager";

interface EmittedState {
  windowCount: number;
  activeWindow: number;
  windowNames: string[];
}

function makeContext(
  windows: { id: number; name: string }[],
  paneIds: number[],
  activeIndex: number,
): {
  ctx: MuxReorderContext;
  windows: { id: number; name: string }[];
  paneIds: number[];
  getActive: () => number;
  emitted: EmittedState[];
} {
  let active = activeIndex;
  const emitted: EmittedState[] = [];
  const ctx: MuxReorderContext = {
    getMuxWindows: () => windows,
    getMuxPaneIds: () => paneIds,
    getActiveMuxWindowIndex: () => active,
    setActiveMuxWindowIndex: (i) => {
      active = i;
    },
    emitMuxStateChange: () => {
      emitted.push({
        windowCount: windows.length,
        activeWindow: active,
        windowNames: windows.map((w) => w.name),
      });
    },
  };
  return { ctx, windows, paneIds, getActive: () => active, emitted };
}

describe("reorderMuxWindows", () => {
  test("[A,B,C] move B(1) to 2 => [A,C,B]", () => {
    const { ctx, windows, paneIds, emitted } = makeContext(
      [
        { id: 1, name: "A" },
        { id: 2, name: "B" },
        { id: 3, name: "C" },
      ],
      [10, 20, 30],
      0,
    );
    const changed = reorderMuxWindows(ctx, 1, 2);
    expect(changed).toBe(true);
    expect(windows.map((w) => w.name)).toEqual(["A", "C", "B"]);
    expect(paneIds).toEqual([10, 30, 20]);
    expect(emitted.length).toBe(1);
  });

  test("active follows its own move: B is active, move B(1) -> 2", () => {
    const { ctx, getActive } = makeContext(
      [
        { id: 1, name: "A" },
        { id: 2, name: "B" },
        { id: 3, name: "C" },
      ],
      [10, 20, 30],
      1, // B active
    );
    reorderMuxWindows(ctx, 1, 2);
    expect(getActive()).toBe(2);
  });

  test("active is A (0), move B(1) -> 0, active shifts to 1", () => {
    const { ctx, windows, getActive } = makeContext(
      [
        { id: 1, name: "A" },
        { id: 2, name: "B" },
        { id: 3, name: "C" },
      ],
      [10, 20, 30],
      0, // A active
    );
    reorderMuxWindows(ctx, 1, 0);
    // New order: [B, A, C], A is at index 1
    expect(windows.map((w) => w.name)).toEqual(["B", "A", "C"]);
    expect(getActive()).toBe(1);
  });

  test("active is C (2), move A(0) -> 2, active shifts to 1", () => {
    const { ctx, windows, getActive } = makeContext(
      [
        { id: 1, name: "A" },
        { id: 2, name: "B" },
        { id: 3, name: "C" },
      ],
      [10, 20, 30],
      2, // C active
    );
    reorderMuxWindows(ctx, 0, 2);
    // New order: [B, C, A], C is at index 1
    expect(windows.map((w) => w.name)).toEqual(["B", "C", "A"]);
    expect(getActive()).toBe(1);
  });

  test("active is B (1), move C(2) -> 0, active shifts to 2", () => {
    const { ctx, windows, getActive } = makeContext(
      [
        { id: 1, name: "A" },
        { id: 2, name: "B" },
        { id: 3, name: "C" },
      ],
      [10, 20, 30],
      1, // B active
    );
    reorderMuxWindows(ctx, 2, 0);
    // New order: [C, A, B], B is at index 2
    expect(windows.map((w) => w.name)).toEqual(["C", "A", "B"]);
    expect(getActive()).toBe(2);
  });

  test("active is A(0), move A(0) -> 2, active follows to 2", () => {
    const { ctx, windows, getActive } = makeContext(
      [
        { id: 1, name: "A" },
        { id: 2, name: "B" },
        { id: 3, name: "C" },
      ],
      [10, 20, 30],
      0,
    );
    reorderMuxWindows(ctx, 0, 2);
    expect(windows.map((w) => w.name)).toEqual(["B", "C", "A"]);
    expect(getActive()).toBe(2);
  });

  test("from === to returns false and does not emit", () => {
    const { ctx, windows, paneIds, emitted } = makeContext(
      [
        { id: 1, name: "A" },
        { id: 2, name: "B" },
      ],
      [10, 20],
      0,
    );
    const changed = reorderMuxWindows(ctx, 1, 1);
    expect(changed).toBe(false);
    expect(windows.map((w) => w.name)).toEqual(["A", "B"]);
    expect(paneIds).toEqual([10, 20]);
    expect(emitted.length).toBe(0);
  });

  test("out-of-range indices return false", () => {
    const { ctx, windows, emitted } = makeContext(
      [
        { id: 1, name: "A" },
        { id: 2, name: "B" },
      ],
      [10, 20],
      0,
    );
    expect(reorderMuxWindows(ctx, -1, 0)).toBe(false);
    expect(reorderMuxWindows(ctx, 0, -1)).toBe(false);
    expect(reorderMuxWindows(ctx, 2, 0)).toBe(false);
    expect(reorderMuxWindows(ctx, 0, 2)).toBe(false);
    expect(windows.map((w) => w.name)).toEqual(["A", "B"]);
    expect(emitted.length).toBe(0);
  });

  test("empty list returns false", () => {
    const { ctx } = makeContext([], [], 0);
    expect(reorderMuxWindows(ctx, 0, 0)).toBe(false);
  });

  test("emit payload reflects the reordered state", () => {
    const { ctx, emitted } = makeContext(
      [
        { id: 1, name: "A" },
        { id: 2, name: "B" },
        { id: 3, name: "C" },
      ],
      [10, 20, 30],
      0, // A active
    );
    reorderMuxWindows(ctx, 2, 0); // [C,A,B], A now at 1
    expect(emitted.length).toBe(1);
    expect(emitted[0]!.windowCount).toBe(3);
    expect(emitted[0]!.activeWindow).toBe(1);
    expect(emitted[0]!.windowNames).toEqual(["C", "A", "B"]);
  });
});
