/**
 * Unit tests for VisibilityController.
 *
 * Covers TS-8 (debounce + immediate visible), TS-9 (toggle suppression),
 * and TS-21 (10-second health check).
 */

import { beforeEach, describe, expect, test } from "bun:test";
import {
  VisibilityController,
  HIDE_DEBOUNCE_MS,
  HEALTH_CHECK_MS,
  type FocusUnsubscribe,
} from "./visibility-controller";

/** Manually-driven fake timer scheduler. */
class FakeScheduler {
  now = 0;
  private nextId = 1;
  private timers = new Map<number, { fireAt: number; cb: () => void; period?: number }>();

  setTimeout = ((cb: () => void, ms: number) => {
    const id = this.nextId++;
    this.timers.set(id, { fireAt: this.now + ms, cb });
    return id as unknown as ReturnType<typeof setTimeout>;
  }) as typeof setTimeout;

  clearTimeout = ((id: ReturnType<typeof setTimeout>) => {
    this.timers.delete(id as unknown as number);
  }) as typeof clearTimeout;

  setInterval = ((cb: () => void, ms: number) => {
    const id = this.nextId++;
    this.timers.set(id, { fireAt: this.now + ms, cb, period: ms });
    return id as unknown as ReturnType<typeof setInterval>;
  }) as typeof setInterval;

  clearInterval = ((id: ReturnType<typeof setInterval>) => {
    this.timers.delete(id as unknown as number);
  }) as typeof clearInterval;

  /** Advance simulated clock; fires any timers whose fireAt <= new now. */
  advance(ms: number): void {
    const target = this.now + ms;
    while (true) {
      const due = [...this.timers.entries()]
        .filter(([, t]) => t.fireAt <= target)
        .sort((a, b) => a[1].fireAt - b[1].fireAt);
      if (due.length === 0) break;
      const [id, t] = due[0]!;
      this.now = t.fireAt;
      if (t.period !== undefined) {
        this.timers.set(id, { fireAt: t.fireAt + t.period, cb: t.cb, period: t.period });
      } else {
        this.timers.delete(id);
      }
      t.cb();
    }
    this.now = target;
  }
}

interface MockPty {
  setVisibilityCalls: boolean[];
  setVisibility(visible: boolean): Promise<void>;
}

function makeMockPty(): MockPty {
  const calls: boolean[] = [];
  return {
    setVisibilityCalls: calls,
    setVisibility(visible: boolean) {
      calls.push(visible);
      return Promise.resolve();
    },
  };
}

interface MockEventTarget {
  listeners: Map<string, Array<EventListenerOrEventListenerObject>>;
  addEventListener: typeof document.addEventListener;
  removeEventListener: typeof document.removeEventListener;
  dispatch(name: string): void;
}

function makeMockEventTarget(): MockEventTarget {
  const listeners = new Map<string, Array<EventListenerOrEventListenerObject>>();
  const target: MockEventTarget = {
    listeners,
    addEventListener: ((name: string, listener: EventListenerOrEventListenerObject) => {
      if (!listeners.has(name)) listeners.set(name, []);
      listeners.get(name)!.push(listener);
    }) as typeof document.addEventListener,
    removeEventListener: ((name: string, listener: EventListenerOrEventListenerObject) => {
      const arr = listeners.get(name);
      if (!arr) return;
      const idx = arr.indexOf(listener);
      if (idx >= 0) arr.splice(idx, 1);
    }) as typeof document.removeEventListener,
    dispatch(name: string) {
      const arr = listeners.get(name);
      if (!arr) return;
      for (const l of arr) {
        if (typeof l === "function") l({} as Event);
        else l.handleEvent({} as Event);
      }
    },
  };
  return target;
}

interface Harness {
  controller: VisibilityController;
  pty: MockPty;
  scheduler: FakeScheduler;
  visibilityState: { visible: boolean };
  focusListener: ((focused: boolean) => void) | null;
  visibilityTarget: MockEventTarget;
  flipDocumentVisibility(visible: boolean): void;
  flipFocus(focused: boolean): void;
}

async function makeHarness(): Promise<Harness> {
  const scheduler = new FakeScheduler();
  const pty = makeMockPty();
  const visibilityState = { visible: true };
  const visibilityTarget = makeMockEventTarget();
  let focusListener: ((focused: boolean) => void) | null = null;

  const controller = new VisibilityController({
    getPtyClient: () => pty as unknown as import("./client").PtyClient,
    getMuxClient: () => null,
    getDocumentVisible: () => visibilityState.visible,
    subscribeFocus: (cb) => {
      focusListener = cb;
      const unsub: FocusUnsubscribe = () => {
        focusListener = null;
      };
      return Promise.resolve(unsub);
    },
    visibilityTarget,
    setTimeoutFn: scheduler.setTimeout,
    clearTimeoutFn: scheduler.clearTimeout,
    setIntervalFn: scheduler.setInterval,
    clearIntervalFn: scheduler.clearInterval,
  });

  await controller.start();

  return {
    controller,
    pty,
    scheduler,
    visibilityState,
    get focusListener() {
      return focusListener;
    },
    visibilityTarget,
    flipDocumentVisibility(visible: boolean) {
      visibilityState.visible = visible;
      visibilityTarget.dispatch("visibilitychange");
    },
    flipFocus(focused: boolean) {
      if (focusListener) focusListener(focused);
    },
  };
}

describe("VisibilityController", () => {
  let h: Harness;

  beforeEach(async () => {
    h = await makeHarness();
  });

  test("TS-8: visible -> hidden is debounced by 1000ms", async () => {
    // After start, the initial evaluate dispatched true.
    expect(h.pty.setVisibilityCalls).toEqual([true]);
    h.flipDocumentVisibility(false);
    // No notification yet.
    expect(h.pty.setVisibilityCalls).toEqual([true]);
    h.scheduler.advance(HIDE_DEBOUNCE_MS - 1);
    expect(h.pty.setVisibilityCalls).toEqual([true]);
    h.scheduler.advance(2);
    expect(h.pty.setVisibilityCalls).toEqual([true, false]);
  });

  test("TS-8: hidden -> visible is immediate", async () => {
    // Move to confirmed hidden first.
    h.flipDocumentVisibility(false);
    h.scheduler.advance(HIDE_DEBOUNCE_MS + 10);
    expect(h.pty.setVisibilityCalls).toEqual([true, false]);
    // Now flip back to visible -> immediate.
    h.flipDocumentVisibility(true);
    expect(h.pty.setVisibilityCalls).toEqual([true, false, true]);
  });

  test("TS-9: hide -> show within 1000ms produces no backend notify", async () => {
    expect(h.pty.setVisibilityCalls).toEqual([true]);
    h.flipDocumentVisibility(false);
    h.scheduler.advance(500);
    h.flipDocumentVisibility(true);
    // The hide debounce was cancelled by the visible transition. lastNotified
    // is already true so no new dispatch happens either.
    h.scheduler.advance(HIDE_DEBOUNCE_MS + 10);
    expect(h.pty.setVisibilityCalls).toEqual([true]);
  });

  test("TS-21: health check resends current state every 10s", async () => {
    expect(h.pty.setVisibilityCalls.length).toBe(1);
    h.scheduler.advance(HEALTH_CHECK_MS + 5);
    expect(h.pty.setVisibilityCalls.length).toBe(2);
    expect(h.pty.setVisibilityCalls[1]).toBe(true);
    h.scheduler.advance(HEALTH_CHECK_MS);
    expect(h.pty.setVisibilityCalls.length).toBe(3);
  });

  test("focus change drives effective state", async () => {
    expect(h.pty.setVisibilityCalls).toEqual([true]);
    h.flipFocus(false);
    h.scheduler.advance(HIDE_DEBOUNCE_MS + 10);
    expect(h.pty.setVisibilityCalls).toEqual([true, false]);
    h.flipFocus(true);
    expect(h.pty.setVisibilityCalls).toEqual([true, false, true]);
  });

  test("stop() removes listeners and cancels timers", async () => {
    h.controller.stop();
    expect(h.visibilityTarget.listeners.get("visibilitychange")?.length ?? 0).toBe(0);
    h.flipDocumentVisibility(false);
    h.scheduler.advance(HIDE_DEBOUNCE_MS + 10);
    // Should NOT have fired (handler was removed).
    expect(h.pty.setVisibilityCalls).toEqual([true]);
  });

  test("DIAG-IDLE: warn log emitted on confirmed visibility transitions", async () => {
    const originalWarn = console.warn;
    const lines: string[] = [];
    console.warn = (...args: unknown[]) => {
      lines.push(args.map((a) => String(a)).join(" "));
    };
    try {
      // Initial evaluate at start dispatched true; that produced one DIAG-IDLE
      // visible line. Reset to focus on subsequent transitions.
      lines.length = 0;
      h.flipDocumentVisibility(false);
      h.scheduler.advance(HIDE_DEBOUNCE_MS + 10);
      h.flipDocumentVisibility(true);

      const hiddenLines = lines.filter((l) => l.includes("[DIAG-IDLE] visibility→hidden"));
      const visibleLines = lines.filter((l) => l.includes("[DIAG-IDLE] visibility→visible"));
      expect(hiddenLines.length).toBe(1);
      expect(visibleLines.length).toBe(1);
      expect(visibleLines[0]).toMatch(/hiddenForMs=\d+/);
    } finally {
      console.warn = originalWarn;
    }
  });
});
