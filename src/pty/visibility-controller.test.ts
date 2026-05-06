/**
 * Unit tests for VisibilityController.
 *
 * Covers TS-8 (debounce + immediate visible), TS-9 (toggle suppression),
 * TS-21 (10-second health check), and TS-29〜TS-39 (rAF heartbeat).
 */

import { beforeEach, describe, expect, test } from "bun:test";
import {
  VisibilityController,
  HIDE_DEBOUNCE_MS,
  HEALTH_CHECK_MS,
  RAF_DEAD_THRESHOLD_MS,
  SUSPEND_GAP_MS,
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

interface MockMux {
  isConnected: boolean;
  sendSetVisibilityCalls: boolean[];
  sendSetVisibility(visible: boolean): Promise<void>;
}

function makeMockMux(): MockMux {
  const calls: boolean[] = [];
  return {
    isConnected: true,
    sendSetVisibilityCalls: calls,
    sendSetVisibility(visible: boolean) {
      calls.push(visible);
      return Promise.resolve();
    },
  };
}

/**
 * Manually-driven fake requestAnimationFrame. A pending callback is
 * stored without scheduling; tests call `fire()` to invoke it (mimicking
 * a real rAF callback) or simply leave it pending to simulate a stall.
 */
class FakeRaf {
  scheduledCount = 0;
  cancelCount = 0;
  private nextHandle = 1;
  private pending: { handle: number; cb: FrameRequestCallback } | null = null;

  request: typeof requestAnimationFrame = ((cb: FrameRequestCallback) => {
    this.scheduledCount++;
    const handle = this.nextHandle++;
    this.pending = { handle, cb };
    return handle;
  }) as typeof requestAnimationFrame;

  cancel: typeof cancelAnimationFrame = ((handle: number) => {
    this.cancelCount++;
    if (this.pending && this.pending.handle === handle) {
      this.pending = null;
    }
  }) as typeof cancelAnimationFrame;

  /** Returns true if a callback was waiting and got fired. */
  fire(perfTimestamp = 0): boolean {
    if (this.pending === null) return false;
    const cb = this.pending.cb;
    this.pending = null;
    cb(perfTimestamp);
    return true;
  }

  hasPending(): boolean {
    return this.pending !== null;
  }
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
  mux: MockMux | null;
  scheduler: FakeScheduler;
  raf: FakeRaf;
  visibilityState: { visible: boolean };
  focusListener: ((focused: boolean) => void) | null;
  visibilityTarget: MockEventTarget;
  flipDocumentVisibility(visible: boolean): void;
  flipFocus(focused: boolean): void;
}

interface HarnessOptions {
  withMux?: boolean;
}

async function makeHarness(opts: HarnessOptions = {}): Promise<Harness> {
  const scheduler = new FakeScheduler();
  const raf = new FakeRaf();
  const pty = makeMockPty();
  const mux = opts.withMux ? makeMockMux() : null;
  const visibilityState = { visible: true };
  const visibilityTarget = makeMockEventTarget();
  let focusListener: ((focused: boolean) => void) | null = null;

  const controller = new VisibilityController({
    getPtyClient: () => pty as unknown as import("./client").PtyClient,
    getMuxClient: () =>
      mux as unknown as import("../terminal/mux/mux-client").MuxClient | null,
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
    requestAnimationFrameFn: raf.request,
    cancelAnimationFrameFn: raf.cancel,
    nowFn: () => scheduler.now,
  });

  await controller.start();

  return {
    controller,
    pty,
    mux,
    scheduler,
    raf,
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

  test("F11: stop() during subscribeFocus await leaks no healthTimer", async () => {
    const scheduler = new FakeScheduler();
    const pty = makeMockPty();
    const visibilityTarget = makeMockEventTarget();

    let resolveSubscribe: ((unsub: FocusUnsubscribe) => void) | null = null;
    let unsubscribeCalls = 0;
    let setIntervalCalls = 0;

    const trackedSetInterval = ((cb: () => void, ms: number) => {
      setIntervalCalls++;
      return scheduler.setInterval(cb, ms);
    }) as typeof setInterval;

    const controller = new VisibilityController({
      getPtyClient: () => pty as unknown as import("./client").PtyClient,
      getMuxClient: () => null,
      getDocumentVisible: () => true,
      subscribeFocus: () =>
        new Promise<FocusUnsubscribe>((resolve) => {
          resolveSubscribe = resolve;
        }),
      visibilityTarget,
      setTimeoutFn: scheduler.setTimeout,
      clearTimeoutFn: scheduler.clearTimeout,
      setIntervalFn: trackedSetInterval,
      clearIntervalFn: scheduler.clearInterval,
    });

    const startPromise = controller.start();
    expect(resolveSubscribe).not.toBeNull();

    controller.stop();

    resolveSubscribe!(() => {
      unsubscribeCalls++;
    });
    await startPromise;

    expect(setIntervalCalls).toBe(0);
    expect(unsubscribeCalls).toBe(1);
    expect(pty.setVisibilityCalls).toEqual([]);

    scheduler.advance(HEALTH_CHECK_MS * 2);
    expect(pty.setVisibilityCalls).toEqual([]);
  });

  test("F11: stop() after fully completed start() clears healthTimer", async () => {
    expect(h.pty.setVisibilityCalls).toEqual([true]);
    h.controller.stop();
    h.scheduler.advance(HEALTH_CHECK_MS * 3);
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

  test("TS-29 (FR1, FR2): rAF stall >= 5s triggers setVisibility(false)", async () => {
    // Initial dispatch was true at start. The controller scheduled a rAF.
    expect(h.pty.setVisibilityCalls).toEqual([true]);
    expect(h.raf.scheduledCount).toBe(1);

    // Establish a baseline by firing the first rAF callback once.
    // Without this, the grace period (lastRafPerfMs===null) suppresses
    // dead detection (TS-32 covers that path explicitly).
    h.raf.fire(0);
    // The rAF callback re-schedules itself.
    expect(h.raf.scheduledCount).toBe(2);

    // Now stall: don't fire any further rAF callbacks. After 10s the
    // health-check fires, sees sinceRaf=10_000 > 5_000, flips rafAlive
    // to false, evaluate() schedules the 1s hide debounce. The
    // health-check also performs a `resendCurrent()` which re-sends
    // the current `true` state — so the call sequence will include
    // at least one extra `true` before `false` lands.
    h.scheduler.advance(HEALTH_CHECK_MS + HIDE_DEBOUNCE_MS + 10);

    // FR1 / FR2 contract: a hidden notification eventually fires.
    expect(h.pty.setVisibilityCalls).toContain(false);
    expect(h.pty.setVisibilityCalls[h.pty.setVisibilityCalls.length - 1]).toBe(false);
    expect(h.controller.getLastNotified()).toBe(false);
  });

  test("TS-30 (FR3): document.visible && focused but rAF dead → effective hidden", async () => {
    expect(h.pty.setVisibilityCalls).toEqual([true]);

    // Establish baseline.
    h.raf.fire(0);
    // Stall rAF; document and focus remain true.
    h.scheduler.advance(HEALTH_CHECK_MS + HIDE_DEBOUNCE_MS + 10);

    expect(h.visibilityState.visible).toBe(true);
    expect(h.controller.getLastNotified()).toBe(false);
  });

  test("TS-31 (FR4): rAF callback after dead state flips rafAlive=true and dispatches visible immediately", async () => {
    // Note on test strategy: in WebKit, a rAF callback queued before
    // cancelRaf may still execute (browser delivers the queued task
    // even after cancellation). To deterministically model that, this
    // test uses a custom FakeRaf whose cancel() is a no-op, so the
    // pending callback survives the controller's cancelRaf() call and
    // can be fired to drive the resume path.
    // Re-build a custom harness where cancelAnimationFrame is a no-op.
    // This models the WebKit edge-case where a rAF callback queued
    // before cancellation still runs.
    const scheduler = new FakeScheduler();
    const raf = new FakeRaf();
    // Override cancel to NOT clear the pending callback (mimic a
    // browser that still delivers the queued cb).
    raf.cancel = ((handle: number) => {
      raf.cancelCount++;
      // intentionally do not clear pending
      void handle;
    }) as typeof cancelAnimationFrame;
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
        return Promise.resolve(() => {
          focusListener = null;
        });
      },
      visibilityTarget,
      setTimeoutFn: scheduler.setTimeout,
      clearTimeoutFn: scheduler.clearTimeout,
      setIntervalFn: scheduler.setInterval,
      clearIntervalFn: scheduler.clearInterval,
      requestAnimationFrameFn: raf.request,
      cancelAnimationFrameFn: raf.cancel,
      nowFn: () => scheduler.now,
    });
    await controller.start();
    void focusListener; // not used directly

    // Baseline + stall.
    raf.fire(0);
    scheduler.advance(HEALTH_CHECK_MS + HIDE_DEBOUNCE_MS + 10);
    expect(pty.setVisibilityCalls).toContain(false);
    expect(controller.getLastNotified()).toBe(false);
    const callsAfterHide = pty.setVisibilityCalls.length;

    // Now imagine the browser delivers the queued rAF cb late.
    // FakeRaf.fire() invokes the pending cb (which was not cleared
    // by our overridden cancel). The cb body flips rafAlive=true and
    // calls evaluate(); since document.visible & focused are still
    // true, lastNotified flips back to true synchronously (no
    // debounce on visible).
    raf.fire(scheduler.now);
    // FR4 contract: a single dispatch happens immediately, and the
    // last notified state is now visible.
    expect(pty.setVisibilityCalls.length).toBe(callsAfterHide + 1);
    expect(pty.setVisibilityCalls[pty.setVisibilityCalls.length - 1]).toBe(true);
    expect(controller.getLastNotified()).toBe(true);

    controller.stop();
  });

  test("TS-32 (FR9): grace period — lastRafPerfMs===null suppresses dead detection", async () => {
    // No rAF callback has fired since start. The first health-check
    // tick at t=10s should not declare dead because lastRafPerfMs
    // is still null. (resendCurrent will re-send `true` but never
    // dispatch `false`.)
    expect(h.pty.setVisibilityCalls).toEqual([true]);
    expect(h.raf.hasPending()).toBe(true);

    h.scheduler.advance(HEALTH_CHECK_MS + 10);
    expect(h.pty.setVisibilityCalls).not.toContain(false);
    expect(h.controller.getLastNotified()).toBe(true);

    // Subsequent tick still in grace.
    h.scheduler.advance(HEALTH_CHECK_MS + HIDE_DEBOUNCE_MS + 10);
    expect(h.pty.setVisibilityCalls).not.toContain(false);
    expect(h.controller.getLastNotified()).toBe(true);
  });

  test("TS-33 (FR5): suspend gap > 30s skips dead detection and resets baseline", async () => {
    // Note: a real system suspend manifests as a wall-clock jump
    // between two consecutive setInterval ticks while the scheduler's
    // own perceived `now` was paused. The FakeScheduler advances time
    // monotonically, so we model suspend by feeding the controller a
    // skewed `nowFn` (scheduler.now + suspended_offset).
    const scheduler = new FakeScheduler();
    const raf = new FakeRaf();
    const pty = makeMockPty();
    const visibilityState = { visible: true };
    const visibilityTarget = makeMockEventTarget();
    let suspended = 0; // extra ms added to nowFn beyond scheduler.now

    const controller = new VisibilityController({
      getPtyClient: () => pty as unknown as import("./client").PtyClient,
      getMuxClient: () => null,
      getDocumentVisible: () => visibilityState.visible,
      subscribeFocus: () =>
        Promise.resolve(() => {
          /* noop */
        }),
      visibilityTarget,
      setTimeoutFn: scheduler.setTimeout,
      clearTimeoutFn: scheduler.clearTimeout,
      setIntervalFn: scheduler.setInterval,
      clearIntervalFn: scheduler.clearInterval,
      requestAnimationFrameFn: raf.request,
      cancelAnimationFrameFn: raf.cancel,
      nowFn: () => scheduler.now + suspended,
    });
    await controller.start();
    expect(pty.setVisibilityCalls).toEqual([true]);

    // Establish baseline.
    raf.fire(0);

    // Tick 1 at t=10s, refresh rAF before so dead is not detected.
    scheduler.advance(HEALTH_CHECK_MS - 100);
    raf.fire(scheduler.now);
    scheduler.advance(200);
    expect(pty.setVisibilityCalls).not.toContain(false);

    // Inject a suspend: nowFn jumps forward by 60s without scheduler
    // advancing. At the NEXT tick (t=20s scheduler-time), nowFn
    // returns 20s + 60s = 80s, and lastHealthTickPerfMs was set
    // around 10s. Gap = 70s > 30s → suspend gap path runs, lastRaf
    // gets reset, NO dead detection this tick.
    suspended = 60_000;
    scheduler.advance(HEALTH_CHECK_MS);
    expect(pty.setVisibilityCalls).not.toContain(false);

    // After the suspend reset, rAF resumes too. Fire it once so the
    // baseline matches reality going forward (real WebKit would deliver
    // a queued or new rAF callback shortly after wakeup).
    raf.fire(scheduler.now + suspended);

    // The next tick (t=30s scheduler-time, nowFn=90s) sees gap=10s,
    // dead detection runs but lastRafPerfMs was refreshed to ~90s,
    // so sinceRaf is small — no dead detection.
    scheduler.advance(HEALTH_CHECK_MS);
    expect(pty.setVisibilityCalls).not.toContain(false);
    expect(controller.getLastNotified()).toBe(true);

    controller.stop();
  });

  test("TS-34 (FR7): notify(false) cancels rAF and no further requestAnimationFrame", async () => {
    expect(h.raf.scheduledCount).toBe(1);
    expect(h.raf.hasPending()).toBe(true);

    // Drive into hidden via document signal (debounced).
    h.flipDocumentVisibility(false);
    h.scheduler.advance(HIDE_DEBOUNCE_MS + 10);
    expect(h.pty.setVisibilityCalls).toEqual([true, false]);
    expect(h.raf.cancelCount).toBe(1);
    expect(h.raf.hasPending()).toBe(false);

    // No new rAF should be scheduled while hidden.
    const scheduledBefore = h.raf.scheduledCount;
    h.scheduler.advance(HEALTH_CHECK_MS * 2);
    expect(h.raf.scheduledCount).toBe(scheduledBefore);
  });

  test("TS-35 (FR6): reason=raf-stall when only rAF signal is responsible", async () => {
    const originalWarn = console.warn;
    const lines: string[] = [];
    console.warn = (...args: unknown[]) => {
      lines.push(args.map((a) => String(a)).join(" "));
    };
    try {
      lines.length = 0;
      h.raf.fire(0);
      h.scheduler.advance(HEALTH_CHECK_MS + HIDE_DEBOUNCE_MS + 10);

      const hiddenLines = lines.filter((l) => l.includes("[DIAG-IDLE] visibility→hidden"));
      expect(hiddenLines.length).toBe(1);
      expect(hiddenLines[0]).toContain("reason=raf-stall");
      expect(hiddenLines[0]).not.toContain("document");
      expect(hiddenLines[0]).not.toContain("focus");
    } finally {
      console.warn = originalWarn;
    }
  });

  test("TS-36 (FR6): reason=document+focus when both DOM signals are responsible", async () => {
    const originalWarn = console.warn;
    const lines: string[] = [];
    console.warn = (...args: unknown[]) => {
      lines.push(args.map((a) => String(a)).join(" "));
    };
    try {
      lines.length = 0;
      // Flip document hidden AND focus lost together.
      h.flipDocumentVisibility(false);
      h.flipFocus(false);
      h.scheduler.advance(HIDE_DEBOUNCE_MS + 10);

      const hiddenLines = lines.filter((l) => l.includes("[DIAG-IDLE] visibility→hidden"));
      expect(hiddenLines.length).toBe(1);
      expect(hiddenLines[0]).toContain("reason=document+focus");
      expect(hiddenLines[0]).not.toContain("raf-stall");
    } finally {
      console.warn = originalWarn;
    }
  });

  test("TS-37 (NFR3 / FR8): constructs and starts when no rAF/now injected and globals are absent", async () => {
    const scheduler = new FakeScheduler();
    const pty = makeMockPty();
    const visibilityTarget = makeMockEventTarget();

    // Hide globals to model an environment without rAF.
    const g = globalThis as {
      requestAnimationFrame?: typeof requestAnimationFrame;
      cancelAnimationFrame?: typeof cancelAnimationFrame;
    };
    const origRaf = g.requestAnimationFrame;
    const origCancelRaf = g.cancelAnimationFrame;
    delete g.requestAnimationFrame;
    delete g.cancelAnimationFrame;

    try {
      const controller = new VisibilityController({
        getPtyClient: () => pty as unknown as import("./client").PtyClient,
        getMuxClient: () => null,
        getDocumentVisible: () => true,
        subscribeFocus: () =>
          Promise.resolve(() => {
            /* noop */
          }),
        visibilityTarget,
        setTimeoutFn: scheduler.setTimeout,
        clearTimeoutFn: scheduler.clearTimeout,
        setIntervalFn: scheduler.setInterval,
        clearIntervalFn: scheduler.clearInterval,
      });
      // Should not throw.
      await controller.start();
      // Initial visibility was dispatched without rAF available.
      expect(pty.setVisibilityCalls).toEqual([true]);
      controller.stop();
    } finally {
      if (origRaf !== undefined) g.requestAnimationFrame = origRaf;
      if (origCancelRaf !== undefined) g.cancelAnimationFrame = origCancelRaf;
    }
  });

  test("TS-38 (FR10): stop() cancels pending rAF; new instance restarts the loop cleanly", async () => {
    // First controller: pending rAF exists.
    expect(h.raf.hasPending()).toBe(true);
    h.controller.stop();
    expect(h.raf.cancelCount).toBe(1);
    expect(h.raf.hasPending()).toBe(false);

    // Fresh controller starts cleanly with its own rAF loop.
    const fresh = await makeHarness();
    try {
      expect(fresh.raf.scheduledCount).toBe(1);
      expect(fresh.raf.hasPending()).toBe(true);
      // The new loop ticks normally.
      fresh.raf.fire(0);
      expect(fresh.raf.scheduledCount).toBe(2);
    } finally {
      fresh.controller.stop();
    }
  });

  test("TS-39 (FR10): rAF dead dispatches MuxClient.sendSetVisibility(false) and (true) on resume", async () => {
    const muxHarness = await makeHarness({ withMux: true });
    try {
      expect(muxHarness.mux).not.toBeNull();
      // Initial visible dispatched on both PTY and Mux.
      expect(muxHarness.pty.setVisibilityCalls).toEqual([true]);
      expect(muxHarness.mux!.sendSetVisibilityCalls).toEqual([true]);

      // Establish rAF baseline.
      muxHarness.raf.fire(0);
      // Stall: dead detection + 1s debounce.
      muxHarness.scheduler.advance(HEALTH_CHECK_MS + HIDE_DEBOUNCE_MS + 10);

      // FR10 / TS-39 contract: a `false` notification reaches both the
      // PTY and the Mux client. Health-tick `resendCurrent` may also
      // emit additional `true` calls before the hide debounce fires,
      // so don't rely on exact array equality.
      expect(muxHarness.pty.setVisibilityCalls).toContain(false);
      expect(muxHarness.mux!.sendSetVisibilityCalls).toContain(false);
      expect(muxHarness.controller.getLastNotified()).toBe(false);
    } finally {
      muxHarness.controller.stop();
    }
  });

  test("TS-29 boundary: dead detection threshold and suspend constant match SPEC", async () => {
    // Establish baseline and confirm the dead-detection path fires
    // hidden on the next tick when sinceRaf > 5_000.
    h.raf.fire(0);
    h.scheduler.advance(HEALTH_CHECK_MS + HIDE_DEBOUNCE_MS + 10);
    expect(h.pty.setVisibilityCalls).toContain(false);
    expect(h.controller.getLastNotified()).toBe(false);

    // Range guard against off-by-one: confirm threshold constants
    // match the SPEC-documented values.
    expect(RAF_DEAD_THRESHOLD_MS).toBe(5000);
    expect(SUSPEND_GAP_MS).toBe(30000);
  });

  test("TS-40: recovery probe works under PORTABLE cancel semantics (no queued-cb-survives behavior)", async () => {
    // Coverage gap addressed: TS-31 simulates the WebKit edge case where
    // a queued rAF callback survives cancelAnimationFrame. Production
    // code should ALSO recover on browsers where cancel actually cancels
    // (the standard semantics) — this is exactly the path the default
    // harness's FakeRaf models. Drive into rAF-stall hidden, then verify
    // that healthTick schedules a one-shot recovery probe that, once
    // fired, restores effective-visible.
    h.raf.fire(0);
    h.scheduler.advance(HEALTH_CHECK_MS + HIDE_DEBOUNCE_MS + 10);
    expect(h.pty.setVisibilityCalls).toContain(false);
    expect(h.controller.getLastNotified()).toBe(false);

    // Default FakeRaf.cancel clears the pending callback (portable
    // semantics). After the hide debounce fires, no rAF is in flight.
    expect(h.raf.hasPending()).toBe(false);

    // Advance one more healthTick. The recovery probe condition holds
    // (rafAlive=false, document/focus visible-eligible) and scheduleRaf
    // is invoked from healthTick — a NEW rAF should now be queued.
    h.scheduler.advance(HEALTH_CHECK_MS + 10);
    expect(h.raf.hasPending()).toBe(true);

    // Fire the probe — rAF resumed in the simulated browser. The cb
    // flips rafAlive=true, evaluates, and dispatches setVisibility(true).
    const callsBeforeFire = h.pty.setVisibilityCalls.length;
    h.raf.fire(h.scheduler.now);
    expect(h.pty.setVisibilityCalls.length).toBeGreaterThan(callsBeforeFire);
    expect(h.pty.setVisibilityCalls[h.pty.setVisibilityCalls.length - 1]).toBe(true);
    expect(h.controller.getLastNotified()).toBe(true);
  });

  test("TS-41: late-delivered rAF callback after stop() does not dispatch on destroyed controller", async () => {
    // Coverage for the destroyed-guard at the top of the rAF cb.
    // Build a harness with no-op cancel so a queued cb survives stop()
    // and can be fired AFTER stop completes — modeling WebKit delivering
    // queued tasks late. The destroyed flag must short-circuit the cb
    // before any state mutation or notify(true) dispatch.
    const scheduler = new FakeScheduler();
    const raf = new FakeRaf();
    raf.cancel = ((handle: number) => {
      raf.cancelCount++;
      // Intentionally retain the pending cb (mimic WebKit late delivery).
      void handle;
    }) as typeof cancelAnimationFrame;
    const pty = makeMockPty();
    const visibilityState = { visible: true };
    const visibilityTarget = makeMockEventTarget();

    const controller = new VisibilityController({
      getPtyClient: () => pty as unknown as import("./client").PtyClient,
      getMuxClient: () => null,
      getDocumentVisible: () => visibilityState.visible,
      subscribeFocus: () => Promise.resolve(() => {}),
      visibilityTarget,
      setTimeoutFn: scheduler.setTimeout,
      clearTimeoutFn: scheduler.clearTimeout,
      setIntervalFn: scheduler.setInterval,
      clearIntervalFn: scheduler.clearInterval,
      requestAnimationFrameFn: raf.request,
      cancelAnimationFrameFn: raf.cancel,
      nowFn: () => scheduler.now,
    });
    await controller.start();

    // A rAF was scheduled at start(). Drive into a state where the
    // queued cb, if it ran, would attempt notify(true) (rafAlive=false,
    // signals visible-eligible). First induce dead-detection.
    raf.fire(0);
    scheduler.advance(HEALTH_CHECK_MS + HIDE_DEBOUNCE_MS + 10);
    expect(pty.setVisibilityCalls).toContain(false);
    const callsBeforeStop = pty.setVisibilityCalls.length;

    // Stop the controller. cancelRaf is invoked but our no-op cancel
    // leaves the pending cb queued.
    controller.stop();

    // Simulate WebKit delivering the queued cb after stop(). Without
    // the destroyed guard, this would mutate state and dispatch
    // setVisibility(true) on the destroyed controller.
    raf.fire(scheduler.now);

    // Contract: no further dispatch.
    expect(pty.setVisibilityCalls.length).toBe(callsBeforeStop);
    // And lastNotified stays at its pre-stop value (false).
    expect(controller.getLastNotified()).toBe(false);
  });
});
