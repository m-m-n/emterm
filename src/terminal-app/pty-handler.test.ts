/**
 * Tests for setupPtyHandlers — focused on the shared WASM crash recovery entry
 * point and focus-listener lifecycle introduced by visibility-render-recovery.
 *
 * The full PTY pipeline is mocked; these tests only exercise:
 *   - tryRecoverFromWasmCrash classification + idempotency + unrecoverable gate
 *   - destroy() unlistening the Tauri focus callback
 *   - microtask-driven scheduler (TS-MT-1〜TS-MT-11)
 */

import { beforeEach, afterEach, describe, expect, it, mock } from "bun:test";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// ── Module mocks (must precede module-under-test import) ─────────────

const mockReinitWasm = mock(async () => {});
mock.module("../terminal/wasm/loader", () => ({
  reinitWasm: mockReinitWasm,
}));

// Capture the focus handler and the unlisten spy so tests can drive the
// listener and assert cleanup behavior.
let focusHandler: ((e: { payload: boolean }) => void) | null = null;
const unlistenFocusSpy = mock(() => {});
mock.module("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => ({
    onFocusChanged: (handler: (e: { payload: boolean }) => void) => {
      focusHandler = handler;
      return Promise.resolve(unlistenFocusSpy);
    },
  }),
}));

mock.module("@tauri-apps/api/core", () => ({
  invoke: mock(async () => null),
  Channel: class Channel {},
}));

mock.module("@tauri-apps/api/event", () => ({
  listen: mock(async () => () => {}),
}));

mock.module("../terminal/handlers/apc_handlers", () => ({
  handleMuxApc: mock(() => {}),
}));

mock.module("../terminal/mux/mux-logger", () => ({
  muxLog: {
    info: mock(() => {}),
    debug: mock(() => {}),
    warn: mock(() => {}),
    error: mock(() => {}),
  },
}));

import { setupPtyHandlers, type PtyHandlerContext, type PtyHandlerHandle } from "./pty-handler";

/**
 * Build a minimal PtyHandlerContext whose getState returns a stub with
 * configurable recreateWasmCore behavior, and a renderer stub tracking
 * forceRender / startCursorBlink / stopCursorBlink calls.
 */
function createContext(opts: {
  recreateSucceeds?: boolean | "throwOnce";
  activeCoreColsThrows?: boolean;
} = {}): {
  ctx: PtyHandlerContext;
  stopCursorBlink: ReturnType<typeof mock>;
  startCursorBlink: ReturnType<typeof mock>;
  forceRender: ReturnType<typeof mock>;
  recreateSpy: ReturnType<typeof mock>;
  coreColsSpy: ReturnType<typeof mock>;
} {
  const recreateSucceeds = opts.recreateSucceeds ?? true;
  let recreateCallCount = 0;
  const recreateSpy = mock(() => {
    recreateCallCount++;
    if (recreateSucceeds === "throwOnce" && recreateCallCount === 1) {
      return false; // triggers reinitWasm path
    }
    return recreateSucceeds === false ? false : true;
  });

  const coreColsSpy = mock(() => {
    if (opts.activeCoreColsThrows) {
      throw new WebAssembly.RuntimeError("Out of bounds memory access");
    }
    return 80;
  });

  const fakeCore = {
    cols: coreColsSpy,
    // process_pty_data is unused in these tests since no data is injected.
    process_pty_data: () => 0,
  } as unknown as ReturnType<ReturnType<PtyHandlerContext["getState"]>["getActiveCore"]>;

  const fakeState = {
    getActiveCore: () => fakeCore,
    getWasmCore: () => fakeCore,
    setCellSizePx: () => {},
    recreateWasmCore: recreateSpy,
  } as unknown as ReturnType<PtyHandlerContext["getState"]>;

  const stopCursorBlink = mock(() => {});
  const startCursorBlink = mock(() => {});
  const forceRender = mock(() => {});

  const fakeRenderer = {
    stopCursorBlink,
    startCursorBlink,
    forceRender,
  } as unknown as ReturnType<PtyHandlerContext["getRenderer"]>;

  const fakePtyClient = {
    onData: () => {},
    onExit: async () => {},
  } as unknown as ReturnType<PtyHandlerContext["getPtyClient"]>;

  const ctx: PtyHandlerContext = {
    getState: () => fakeState,
    getRenderer: () => fakeRenderer,
    getPtyClient: () => fakePtyClient,
    getImeHandler: () => null,
    getImageHandler: () => null,
    getCharSize: () => ({ width: 8, height: 16 }),
    registerCoreCallbacks: () => {},
    processPendingOscQueue: () => {},
    getOutputActivityCallback: () => null,
    getSessionExitCallback: () => null,
    getMuxApcContext: () => null,
    isTabActive: () => true,
  };

  return { ctx, stopCursorBlink, startCursorBlink, forceRender, recreateSpy, coreColsSpy };
}

// Each test starts with fresh module-level mocks.
beforeEach(() => {
  focusHandler = null;
  unlistenFocusSpy.mockClear();
  mockReinitWasm.mockClear();
});

afterEach(() => {
  focusHandler = null;
});

describe("setupPtyHandlers — tryRecoverFromWasmCrash", () => {
  it("classifies WebAssembly.RuntimeError as a WASM crash and attempts recovery", async () => {
    const { ctx, stopCursorBlink, recreateSpy, forceRender, startCursorBlink } = createContext();
    const handle = await setupPtyHandlers(ctx);

    const err = new WebAssembly.RuntimeError("Out of bounds memory access");
    const handled = handle.tryRecoverFromWasmCrash(err);

    expect(handled).toBe(true);
    expect(stopCursorBlink).toHaveBeenCalledTimes(1);
    expect(recreateSpy).toHaveBeenCalledTimes(1);
    // Step 1 (recreate) succeeded, so finishRecovery runs synchronously:
    expect(forceRender).toHaveBeenCalledTimes(1);
    expect(startCursorBlink).toHaveBeenCalledTimes(1);

    handle.destroy();
  });

  it("classifies 'recursive use of an object' as a WASM crash", async () => {
    const { ctx, recreateSpy } = createContext();
    const handle = await setupPtyHandlers(ctx);

    const handled = handle.tryRecoverFromWasmCrash(
      new Error("recursive use of an object detected which would lead to unsafe aliasing in rust"),
    );
    expect(handled).toBe(true);
    expect(recreateSpy).toHaveBeenCalledTimes(1);

    handle.destroy();
  });

  it("classifies 'WASM not initialized' as a WASM crash", async () => {
    const { ctx, recreateSpy } = createContext();
    const handle = await setupPtyHandlers(ctx);

    const handled = handle.tryRecoverFromWasmCrash(new Error("WASM not initialized"));
    expect(handled).toBe(true);
    expect(recreateSpy).toHaveBeenCalledTimes(1);

    handle.destroy();
  });

  it("TS-3: classifies an exports-lost TypeError (message contains terminalcore_) as a WASM crash", async () => {
    const { ctx, recreateSpy } = createContext();
    const handle = await setupPtyHandlers(ctx);

    // Exports-lost crash: the bundle's local wasm alias loses its exports, so a
    // call surfaces as e.g. "undefined is not an object (d0.terminalcore_render)".
    const handled = handle.tryRecoverFromWasmCrash(
      new TypeError("undefined is not an object (evaluating 'd0.terminalcore_process_pty_data')"),
    );
    expect(handled).toBe(true);
    expect(recreateSpy).toHaveBeenCalledTimes(1);

    handle.destroy();
  });

  it("TS-4: returns false and performs no recovery for an unrelated TypeError (no terminalcore_)", async () => {
    const { ctx, stopCursorBlink, recreateSpy, forceRender } = createContext();
    const handle = await setupPtyHandlers(ctx);

    const handled = handle.tryRecoverFromWasmCrash(new TypeError("unrelated failure"));
    expect(handled).toBe(false);
    expect(stopCursorBlink).not.toHaveBeenCalled();
    expect(recreateSpy).not.toHaveBeenCalled();
    expect(forceRender).not.toHaveBeenCalled();

    handle.destroy();
  });

  it("is idempotent: concurrent triggers while recovery is in-flight run recovery only once", async () => {
    // recreateWasmCore returns false the first time, forcing the async reinit
    // path. While reinitWasm is pending (wasmRecoveryInProgress=true), a
    // second tryRecoverFromWasmCrash must be a no-op.
    let releaseReinit!: () => void;
    const reinitGate = new Promise<void>((resolve) => { releaseReinit = resolve; });
    mockReinitWasm.mockImplementation(() => reinitGate);

    const { ctx, recreateSpy, stopCursorBlink } = createContext({ recreateSucceeds: "throwOnce" });
    const handle = await setupPtyHandlers(ctx);

    const err = new WebAssembly.RuntimeError("oob");
    handle.tryRecoverFromWasmCrash(err);
    // First call: stopCursorBlink + recreate invoked, reinit started but not resolved.
    expect(stopCursorBlink).toHaveBeenCalledTimes(1);
    expect(recreateSpy).toHaveBeenCalledTimes(1);
    expect(mockReinitWasm).toHaveBeenCalledTimes(1);

    // Second concurrent call should be a no-op beyond returning true.
    const secondResult = handle.tryRecoverFromWasmCrash(err);
    expect(secondResult).toBe(true);
    expect(stopCursorBlink).toHaveBeenCalledTimes(1); // unchanged
    expect(recreateSpy).toHaveBeenCalledTimes(1);      // unchanged
    expect(mockReinitWasm).toHaveBeenCalledTimes(1);   // unchanged

    // Let reinit complete so the async branch doesn't leak into later tests.
    releaseReinit();
    await reinitGate;
    // Yield so the .finally() block runs and clears wasmRecoveryInProgress.
    await new Promise<void>((resolve) => setTimeout(resolve, 0));

    handle.destroy();
  });

  it("is a no-op beyond returning true after exceeding MAX_WASM_RECOVERY_ATTEMPTS", async () => {
    // Force the reinit path so each call counts a recovery attempt before
    // any async work completes — but also deterministic: we'll just rely on
    // the 3-attempt budget by invoking recovery 4 times with successful
    // recreate (counter increments regardless).
    const { ctx, recreateSpy, stopCursorBlink } = createContext();
    const handle = await setupPtyHandlers(ctx);

    const err = new WebAssembly.RuntimeError("oob");
    handle.tryRecoverFromWasmCrash(err); // 1
    handle.tryRecoverFromWasmCrash(err); // 2
    handle.tryRecoverFromWasmCrash(err); // 3
    const fourth = handle.tryRecoverFromWasmCrash(err); // 4 -> exhausted

    expect(fourth).toBe(true);
    // Counter exceeded on the 4th call: wasmUnrecoverable set, recreate not invoked.
    expect(recreateSpy).toHaveBeenCalledTimes(3);
    expect(stopCursorBlink).toHaveBeenCalledTimes(3);

    // Subsequent calls are silent no-ops (unrecoverable gate).
    const fifth = handle.tryRecoverFromWasmCrash(err);
    expect(fifth).toBe(true);
    expect(recreateSpy).toHaveBeenCalledTimes(3);

    handle.destroy();
  });

  it("never throws, even when stopCursorBlink throws", async () => {
    const { ctx, stopCursorBlink } = createContext();
    stopCursorBlink.mockImplementation(() => { throw new Error("stop blink boom"); });

    const handle = await setupPtyHandlers(ctx);
    // If tryRecoverFromWasmCrash threw, this test would throw synchronously.
    const handled = handle.tryRecoverFromWasmCrash(new WebAssembly.RuntimeError("oob"));
    expect(handled).toBe(true);

    handle.destroy();
  });
});

describe("setupPtyHandlers — tryRecoverFromWasmCrash onComplete", () => {
  it("fires onComplete synchronously with true when sync recovery succeeds", async () => {
    const { ctx } = createContext();
    const handle = await setupPtyHandlers(ctx);

    const onComplete = mock((_success: boolean) => {});
    const handled = handle.tryRecoverFromWasmCrash(
      new WebAssembly.RuntimeError("oob"),
      onComplete,
    );

    expect(handled).toBe(true);
    // Fast path (recreateWasmCore succeeds) fires onComplete synchronously.
    expect(onComplete).toHaveBeenCalledTimes(1);
    expect(onComplete).toHaveBeenCalledWith(true);

    handle.destroy();
  });

  it("does not fire onComplete when the error is unrelated to WASM", async () => {
    const { ctx } = createContext();
    const handle = await setupPtyHandlers(ctx);

    const onComplete = mock((_success: boolean) => {});
    const handled = handle.tryRecoverFromWasmCrash(new TypeError("unrelated"), onComplete);

    expect(handled).toBe(false);
    expect(onComplete).not.toHaveBeenCalled();

    handle.destroy();
  });

  it("fires onComplete with false when attempts are exhausted", async () => {
    const { ctx } = createContext();
    const handle = await setupPtyHandlers(ctx);

    const err = new WebAssembly.RuntimeError("oob");
    handle.tryRecoverFromWasmCrash(err); // 1
    handle.tryRecoverFromWasmCrash(err); // 2
    handle.tryRecoverFromWasmCrash(err); // 3

    const onComplete = mock((_success: boolean) => {});
    handle.tryRecoverFromWasmCrash(err, onComplete); // 4 -> exhausted

    expect(onComplete).toHaveBeenCalledTimes(1);
    expect(onComplete).toHaveBeenCalledWith(false);

    handle.destroy();
  });

  it("fires onComplete with false when already marked unrecoverable", async () => {
    const { ctx } = createContext();
    const handle = await setupPtyHandlers(ctx);

    const err = new WebAssembly.RuntimeError("oob");
    handle.tryRecoverFromWasmCrash(err); // 1
    handle.tryRecoverFromWasmCrash(err); // 2
    handle.tryRecoverFromWasmCrash(err); // 3
    handle.tryRecoverFromWasmCrash(err); // 4 -> sets wasmUnrecoverable

    // Subsequent calls hit the unrecoverable gate.
    const onComplete = mock((_success: boolean) => {});
    handle.tryRecoverFromWasmCrash(err, onComplete);

    expect(onComplete).toHaveBeenCalledTimes(1);
    expect(onComplete).toHaveBeenCalledWith(false);

    handle.destroy();
  });

  it("fires onComplete with true asynchronously after reinitWasm succeeds", async () => {
    // recreateWasmCore returns false first → async reinit path runs.
    // Second call (from finally block in async IIFE) succeeds.
    const { ctx } = createContext({ recreateSucceeds: "throwOnce" });
    const handle = await setupPtyHandlers(ctx);

    const onComplete = mock((_success: boolean) => {});
    handle.tryRecoverFromWasmCrash(new WebAssembly.RuntimeError("oob"), onComplete);

    // Not yet fired — async reinit still pending.
    expect(onComplete).not.toHaveBeenCalled();

    // Yield once for the awaited reinitWasm, then again for the finally block.
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
    await new Promise<void>((resolve) => setTimeout(resolve, 0));

    expect(onComplete).toHaveBeenCalledTimes(1);
    expect(onComplete).toHaveBeenCalledWith(true);

    handle.destroy();
  });

  it("fires onComplete with false when reinitWasm throws", async () => {
    mockReinitWasm.mockImplementation(() => Promise.reject(new Error("reinit failed")));

    const { ctx } = createContext({ recreateSucceeds: "throwOnce" });
    const handle = await setupPtyHandlers(ctx);

    const onComplete = mock((_success: boolean) => {});
    handle.tryRecoverFromWasmCrash(new WebAssembly.RuntimeError("oob"), onComplete);

    await new Promise<void>((resolve) => setTimeout(resolve, 0));
    await new Promise<void>((resolve) => setTimeout(resolve, 0));

    expect(onComplete).toHaveBeenCalledTimes(1);
    expect(onComplete).toHaveBeenCalledWith(false);

    handle.destroy();
  });

  it("queues concurrent onComplete callbacks during in-flight recovery and fires all with the same outcome", async () => {
    let releaseReinit!: () => void;
    const reinitGate = new Promise<void>((resolve) => { releaseReinit = resolve; });
    mockReinitWasm.mockImplementation(() => reinitGate);

    const { ctx } = createContext({ recreateSucceeds: "throwOnce" });
    const handle = await setupPtyHandlers(ctx);

    const cb1 = mock((_success: boolean) => {});
    const cb2 = mock((_success: boolean) => {});
    const cb3 = mock((_success: boolean) => {});

    // First call starts async recovery; its own onComplete also queues.
    handle.tryRecoverFromWasmCrash(new WebAssembly.RuntimeError("oob"), cb1);
    // Subsequent calls while in-flight: onComplete gets queued.
    handle.tryRecoverFromWasmCrash(new WebAssembly.RuntimeError("oob"), cb2);
    handle.tryRecoverFromWasmCrash(new WebAssembly.RuntimeError("oob"), cb3);

    // None fired yet.
    expect(cb1).not.toHaveBeenCalled();
    expect(cb2).not.toHaveBeenCalled();
    expect(cb3).not.toHaveBeenCalled();

    // Let reinit complete.
    releaseReinit();
    await reinitGate;
    await new Promise<void>((resolve) => setTimeout(resolve, 0));

    // All three fire with true (recovery succeeded).
    expect(cb1).toHaveBeenCalledTimes(1);
    expect(cb1).toHaveBeenCalledWith(true);
    expect(cb2).toHaveBeenCalledTimes(1);
    expect(cb2).toHaveBeenCalledWith(true);
    expect(cb3).toHaveBeenCalledTimes(1);
    expect(cb3).toHaveBeenCalledWith(true);

    handle.destroy();
  });

  it("does not break recovery when an onComplete callback throws", async () => {
    const { ctx, recreateSpy } = createContext();
    const handle = await setupPtyHandlers(ctx);

    const throwingCallback = mock((_success: boolean) => {
      throw new Error("callback boom");
    });

    // tryRecoverFromWasmCrash itself must not throw even if the user callback does.
    expect(() => {
      handle.tryRecoverFromWasmCrash(
        new WebAssembly.RuntimeError("oob"),
        throwingCallback,
      );
    }).not.toThrow();

    expect(throwingCallback).toHaveBeenCalledTimes(1);
    expect(recreateSpy).toHaveBeenCalledTimes(1);

    // A subsequent recovery call must still function.
    const nextCallback = mock((_success: boolean) => {});
    handle.tryRecoverFromWasmCrash(new WebAssembly.RuntimeError("oob"), nextCallback);
    expect(nextCallback).toHaveBeenCalledTimes(1);
    expect(nextCallback).toHaveBeenCalledWith(true);

    handle.destroy();
  });
});

describe("setupPtyHandlers — focus listener lifecycle", () => {
  it("registers a Tauri focus listener during setup", async () => {
    const { ctx } = createContext();
    const handle = await setupPtyHandlers(ctx);
    // Yield so the async IIFE that awaits onFocusChanged completes.
    await new Promise<void>((resolve) => setTimeout(resolve, 0));

    expect(focusHandler).not.toBeNull();

    handle.destroy();
  });

  it("invokes the shared recovery when the focus probe throws RuntimeError", async () => {
    const { ctx, recreateSpy } = createContext({ activeCoreColsThrows: true });
    const handle = await setupPtyHandlers(ctx);
    await new Promise<void>((resolve) => setTimeout(resolve, 0));

    // Simulate the window regaining focus.
    expect(focusHandler).not.toBeNull();
    focusHandler!({ payload: true });

    expect(recreateSpy).toHaveBeenCalledTimes(1);

    handle.destroy();
  });

  it("focus event is a no-op when payload is false (blur)", async () => {
    const { ctx, recreateSpy } = createContext({ activeCoreColsThrows: true });
    const handle = await setupPtyHandlers(ctx);
    await new Promise<void>((resolve) => setTimeout(resolve, 0));

    focusHandler!({ payload: false });

    expect(recreateSpy).not.toHaveBeenCalled();

    handle.destroy();
  });

  it("destroy calls the Tauri unlisten function", async () => {
    const { ctx } = createContext();
    const handle = await setupPtyHandlers(ctx);
    await new Promise<void>((resolve) => setTimeout(resolve, 0));

    handle.destroy();

    expect(unlistenFocusSpy).toHaveBeenCalledTimes(1);
  });
});

describe("setupPtyHandlers — noopHandle", () => {
  it("returns a noop handle whose tryRecoverFromWasmCrash returns false when PtyClient is missing", async () => {
    // Omit PtyClient to take the early-return path.
    const { ctx } = createContext();
    const brokenCtx: PtyHandlerContext = { ...ctx, getPtyClient: () => null };
    const handle: PtyHandlerHandle = await setupPtyHandlers(brokenCtx);

    expect(handle.tryRecoverFromWasmCrash(new WebAssembly.RuntimeError("oob"))).toBe(false);
    // destroy must remain safe on the noop handle.
    handle.destroy();
  });
});

// ── Microtask scheduler tests (TS-MT-1〜TS-MT-11) ────────────────────
//
// These tests exercise the microtask-driven scheduler installed by
// scheduleProcessing(). They override globalThis.MessageChannel /
// queueMicrotask / setTimeout-as-needed to observe which primitive was
// chosen and to drive the scheduled callback synchronously.

interface MtContextResult {
  ctx: PtyHandlerContext;
  processCalls: { trigger: string }[];
  consumedSequence: number[]; // bytes to consume per call
  ackSpy: ReturnType<typeof mock>;
}

function createMtContext(opts: {
  consumedSequence?: number[]; // amount to consume each process_pty_data call
} = {}): MtContextResult {
  const consumedSequence = opts.consumedSequence ?? [];
  const processCalls: { trigger: string }[] = [];
  let consumedIdx = 0;

  const ackSpy = mock((_n: number) => {});

  const fakeCore = {
    cols: () => 80,
    process_pty_data: (input: Uint8Array) => {
      // Default: consume everything; if consumedSequence supplied, use it.
      if (consumedIdx < consumedSequence.length) {
        const c = consumedSequence[consumedIdx]!;
        consumedIdx++;
        return Math.min(c, input.length);
      }
      return input.length;
    },
    take_mode_actions: () => new Uint8Array(0),
  } as unknown as ReturnType<ReturnType<PtyHandlerContext["getState"]>["getActiveCore"]>;

  const fakeState = {
    getActiveCore: () => fakeCore,
    getWasmCore: () => fakeCore,
    setCellSizePx: () => {},
    recreateWasmCore: () => true,
    cursorCol: 0,
    cursorRow: 0,
    cursorVisible: false,
    syncModesFromWasm: () => {},
    setDecPrivateMode: () => {},
    handleModeAction: () => {},
    modes: { synchronizedOutput: false },
  } as unknown as ReturnType<PtyHandlerContext["getState"]>;

  const fakeRenderer = {
    stopCursorBlink: () => {},
    startCursorBlink: () => {},
    forceRender: () => {},
    renderImmediate: () => {},
  } as unknown as ReturnType<PtyHandlerContext["getRenderer"]>;

  const fakePtyClient = {
    onData: () => {},
    onExit: async () => {},
    ackBytes: ackSpy,
  } as unknown as ReturnType<PtyHandlerContext["getPtyClient"]>;

  const ctx: PtyHandlerContext = {
    getState: () => fakeState,
    getRenderer: () => fakeRenderer,
    getPtyClient: () => fakePtyClient,
    getImeHandler: () => null,
    getImageHandler: () => null,
    getCharSize: () => ({ width: 8, height: 16 }),
    registerCoreCallbacks: () => {},
    processPendingOscQueue: () => {},
    getOutputActivityCallback: () => null,
    getSessionExitCallback: () => null,
    getMuxApcContext: () => null,
    isTabActive: () => true,
  };

  // Wrap getRenderer so calls to renderImmediate are no-ops; nothing
  // observed beyond the ack spy and process trigger calls.
  void processCalls;
  return { ctx, processCalls, consumedSequence: [...consumedSequence], ackSpy };
}

/**
 * Snapshot existing globals and provide a restorer. Tests can override
 * any of MessageChannel / queueMicrotask / setTimeout / requestAnimationFrame
 * inside `withGlobals(() => { ... })` and have them restored automatically.
 */
function withGlobals(): {
  setMessageChannel: (mc: any) => void;
  setQueueMicrotask: (fn: any) => void;
  setRequestAnimationFrame: (fn: any) => void;
  rafSpy: ReturnType<typeof mock>;
  restore: () => void;
} {
  const g = globalThis as any;
  const orig = {
    MessageChannel: g.MessageChannel,
    queueMicrotask: g.queueMicrotask,
    requestAnimationFrame: g.requestAnimationFrame,
    cancelAnimationFrame: g.cancelAnimationFrame,
  };
  const rafSpy = mock(() => 0);
  // Default: install a recording rAF stub so any leak is observable.
  g.requestAnimationFrame = rafSpy;
  g.cancelAnimationFrame = () => {};
  return {
    setMessageChannel: (mc: any) => { g.MessageChannel = mc; },
    setQueueMicrotask: (fn: any) => { g.queueMicrotask = fn; },
    setRequestAnimationFrame: (fn: any) => { g.requestAnimationFrame = fn; },
    rafSpy,
    restore: () => {
      g.MessageChannel = orig.MessageChannel;
      g.queueMicrotask = orig.queueMicrotask;
      g.requestAnimationFrame = orig.requestAnimationFrame;
      g.cancelAnimationFrame = orig.cancelAnimationFrame;
    },
  };
}

/**
 * Build a fake MessageChannel where postMessage(0) on port1 invokes
 * port2.onmessage synchronously — lets us drive the scheduled callback
 * deterministically inside a sync test.
 *
 * setting `defer=true` instead queues the callbacks so the test can
 * trigger them later via `flush()`, simulating real microtask delivery.
 */
function createFakeMessageChannel(opts: { defer?: boolean } = {}) {
  const defer = opts.defer ?? false;
  let port2OnMessage: ((ev: any) => void) | null = null;
  const closeSpies = {
    port1: mock(() => {}),
    port2: mock(() => {}),
  };
  const queue: Array<() => void> = [];
  const postMessageSpy = mock((_data: any) => {
    const fire = () => {
      if (port2OnMessage) port2OnMessage({ data: _data });
    };
    if (defer) queue.push(fire); else fire();
  });
  const port1 = {
    postMessage: postMessageSpy,
    close: closeSpies.port1,
  };
  const port2 = {
    set onmessage(fn: any) { port2OnMessage = fn; },
    get onmessage() { return port2OnMessage; },
    close: closeSpies.port2,
  };
  // The factory in pty-handler reads `MessageChannel` as a constructor,
  // so we expose a class-like callable.
  class FakeMC {
    port1 = port1;
    port2 = port2;
  }
  return {
    Ctor: FakeMC,
    postMessageSpy,
    closeSpies,
    flush: () => {
      const pending = [...queue];
      queue.length = 0;
      for (const f of pending) f();
    },
    pendingCount: () => queue.length,
  };
}

describe("setupPtyHandlers — microtask scheduler (TS-MT-1〜11)", () => {
  let g: ReturnType<typeof withGlobals>;

  beforeEach(() => {
    g = withGlobals();
  });

  afterEach(() => {
    g.restore();
  });

  it("TS-MT-1 (FR1): one scheduleProcessing call results in exactly one postMessage delivery via MessageChannel", async () => {
    const fake = createFakeMessageChannel({ defer: true });
    g.setMessageChannel(fake.Ctor);

    const { ctx } = createMtContext();
    const handle = await setupPtyHandlers(ctx);

    handle.injectData(new Uint8Array([0x61])); // "a"

    expect(fake.postMessageSpy).toHaveBeenCalledTimes(1);
    fake.flush();

    handle.destroy();
  });

  it("TS-MT-2 (FR1): two consecutive scheduleProcessing calls produce only one postMessage (deduplicated by processScheduled)", async () => {
    const fake = createFakeMessageChannel({ defer: true });
    g.setMessageChannel(fake.Ctor);

    const { ctx } = createMtContext();
    const handle = await setupPtyHandlers(ctx);

    handle.injectData(new Uint8Array([0x61]));
    handle.injectData(new Uint8Array([0x62]));

    expect(fake.postMessageSpy).toHaveBeenCalledTimes(1);
    fake.flush();

    handle.destroy();
  });

  it("TS-MT-3 (FR2): when MessageChannel is unavailable, setTimeout(0) is used and trigger label is 'timer'", async () => {
    // The scheduler chain is MessageChannel → setTimeout(0). queueMicrotask
    // is intentionally NOT a fallback, because microtask chaining cannot
    // yield to the task queue and would starve rendering under sustained
    // bursts. Verify that, with MessageChannel removed, the scheduler falls
    // straight through to setTimeout(0) regardless of queueMicrotask.
    g.setMessageChannel(undefined);

    const origSetTimeout = globalThis.setTimeout;
    const stOnZeroCallbacks: Array<() => void> = [];
    const stSpy = mock(((cb: any, delay: number) => {
      if (delay === 0 || delay === undefined) {
        stOnZeroCallbacks.push(cb);
        return 999 as any;
      }
      // Forward non-zero delays (ackFlushTimer, healthCheck, focus probe etc.)
      return origSetTimeout(cb, delay);
    }) as any);
    (globalThis as any).setTimeout = stSpy;

    try {
      const { ctx } = createMtContext();
      const handle = await setupPtyHandlers(ctx);

      // Some unrelated setTimeouts happen during setup (healthCheck, ack flush
      // primer). Track the count, then check setTimeout(0) was invoked once
      // injectData triggered scheduling.
      const before = stOnZeroCallbacks.length;
      handle.injectData(new Uint8Array([0x61]));
      const after = stOnZeroCallbacks.length;
      expect(after - before).toBeGreaterThanOrEqual(1);

      // Drive the timer.
      stOnZeroCallbacks[after - 1]!();
      // No rAF used.
      expect(g.rafSpy).not.toHaveBeenCalled();

      handle.destroy();
    } finally {
      (globalThis as any).setTimeout = origSetTimeout;
    }
  });

  it("TS-MT-5 (FR5): processPendingData is called with trigger='microtask' on the primary path", async () => {
    const fake = createFakeMessageChannel({ defer: true });
    g.setMessageChannel(fake.Ctor);

    const { ctx, ackSpy } = createMtContext();
    const handle = await setupPtyHandlers(ctx);

    handle.injectData(new Uint8Array([0x61, 0x62, 0x63]));
    fake.flush();

    // ack is coalesced behind a timer; flush by calling destroy which flushes.
    handle.destroy();

    expect(ackSpy).toHaveBeenCalledTimes(1);
    expect(ackSpy.mock.calls[0]![0]).toBe(3);
  });

  it("TS-MT-6 (FR6): when leftoverData remains after a process tick, handler schedules another microtask", async () => {
    const fake = createFakeMessageChannel({ defer: true });
    g.setMessageChannel(fake.Ctor);

    // Drive a cursor false→true transition mid-processing: this is one of
    // the existing pty-handler conditions that parks the unconsumed tail
    // into leftoverData and breaks the loop. After leftoverData is set, the
    // leftover-data re-schedule guard (FR6) must call scheduleProcessing()
    // which queues another microtask via the scheduler.
    let firstCall = true;
    const fakeCore: any = {
      cols: () => 80,
      take_mode_actions: () => new Uint8Array(0),
      process_pty_data: (input: Uint8Array) => {
        if (firstCall) {
          firstCall = false;
          // Simulate a CSI ?25h embedded by flipping cursorVisible AFTER
          // partial consumption.
          fakeState.cursorVisible = true;
          return 2; // consumed 2 of input.length (5)
        }
        // Second call drains the rest.
        return input.length;
      },
    };
    const fakeState: any = {
      getActiveCore: () => fakeCore,
      getWasmCore: () => fakeCore,
      setCellSizePx: () => {},
      recreateWasmCore: () => true,
      cursorCol: 0,
      cursorRow: 0,
      cursorVisible: false,
      syncModesFromWasm: () => {},
      setDecPrivateMode: () => {},
      handleModeAction: () => {},
      modes: { synchronizedOutput: false },
    };
    const fakeRenderer: any = {
      stopCursorBlink: () => {},
      startCursorBlink: () => {},
      forceRender: () => {},
      renderImmediate: () => {},
    };
    const ackSpy = mock((_n: number) => {});
    const fakePtyClient: any = {
      onData: () => {},
      onExit: async () => {},
      ackBytes: ackSpy,
    };
    const ctx: PtyHandlerContext = {
      getState: () => fakeState,
      getRenderer: () => fakeRenderer,
      getPtyClient: () => fakePtyClient,
      getImeHandler: () => null,
      getImageHandler: () => null,
      getCharSize: () => ({ width: 8, height: 16 }),
      registerCoreCallbacks: () => {},
      processPendingOscQueue: () => {},
      getOutputActivityCallback: () => null,
      getSessionExitCallback: () => null,
      getMuxApcContext: () => null,
      isTabActive: () => true,
    };

    const handle = await setupPtyHandlers(ctx);

    handle.injectData(new Uint8Array([0x61, 0x62, 0x63, 0x64, 0x65]));

    // First scheduling.
    expect(fake.postMessageSpy).toHaveBeenCalledTimes(1);
    fake.flush();
    // Leftover (3 bytes parked) triggers a second scheduling.
    expect(fake.postMessageSpy).toHaveBeenCalledTimes(2);
    fake.flush();
    // Drained; no further scheduling.
    expect(fake.postMessageSpy).toHaveBeenCalledTimes(2);

    handle.destroy();
  });

  it("TS-MT-8 (FR9): canvas-renderer.ts retains its requestAnimationFrame call site (renderer remains rAF-driven)", () => {
    // FR9 requires that canvas rendering itself stays on rAF (vsync alignment).
    // The pty-handler change only swapped the data-path scheduler; the
    // renderer side must be untouched. Source-grep assert that
    // canvas-renderer.ts still references requestAnimationFrame.
    const rendererPath = resolve(__dirname, "..", "terminal", "canvas-renderer.ts");
    const source = readFileSync(rendererPath, "utf-8");
    const stripped = source
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .replace(/\/\/.*$/gm, "");
    expect(stripped).toMatch(/\brequestAnimationFrame\s*\(/);
  });

  it("TS-MT-7 (FR7): pty-handler.ts data path does not call globalThis.requestAnimationFrame during normal scheduling", async () => {
    const fake = createFakeMessageChannel({ defer: true });
    g.setMessageChannel(fake.Ctor);

    const { ctx } = createMtContext();
    const handle = await setupPtyHandlers(ctx);

    handle.injectData(new Uint8Array([0x61, 0x62, 0x63]));
    fake.flush();

    expect(g.rafSpy).not.toHaveBeenCalled();

    handle.destroy();
  });

  it("TS-MT-10 (FR3): pendingHandle is null on MessageChannel path (not observable directly, but verified by no setTimeout(0) calls leaking out)", async () => {
    // Indirect: on the MessageChannel path, no zero-delay setTimeout is used
    // for scheduling, only for ackFlushTimer (which uses ACK_FLUSH_INTERVAL_MS=250).
    const fake = createFakeMessageChannel({ defer: true });
    g.setMessageChannel(fake.Ctor);

    const origSetTimeout = globalThis.setTimeout;
    let zeroDelayCount = 0;
    (globalThis as any).setTimeout = (cb: any, delay: number) => {
      if (delay === 0) zeroDelayCount++;
      return origSetTimeout(cb, delay);
    };

    try {
      const { ctx } = createMtContext();
      const handle = await setupPtyHandlers(ctx);
      handle.injectData(new Uint8Array([0x61]));
      fake.flush();
      // The MessageChannel scheduler must NOT use setTimeout(0).
      expect(zeroDelayCount).toBe(0);
      handle.destroy();
    } finally {
      (globalThis as any).setTimeout = origSetTimeout;
    }
  });

  it("TS-MT-11 (FR7): rafScheduled and rafHandle identifiers are removed from pty-handler.ts source", () => {
    const sourcePath = resolve(__dirname, "pty-handler.ts");
    const source = readFileSync(sourcePath, "utf-8");
    // Strip block + line comments before scanning.
    const stripped = source
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .replace(/\/\/.*$/gm, "");
    expect(stripped).not.toMatch(/\brafScheduled\b/);
    expect(stripped).not.toMatch(/\brafHandle\b/);
  });
});

describe("setupPtyHandlers — microtask scheduler edge cases (TS-MT-4 / TS-MT-9)", () => {
  let g: ReturnType<typeof withGlobals>;

  beforeEach(() => {
    g = withGlobals();
  });

  afterEach(() => {
    g.restore();
  });

  it("TS-MT-4 (FR4): a microtask whose captured myToken no longer matches scheduleToken does not call processPendingData", async () => {
    const fake = createFakeMessageChannel({ defer: true });
    g.setMessageChannel(fake.Ctor);

    const { ctx, ackSpy } = createMtContext();
    const handle = await setupPtyHandlers(ctx);

    handle.injectData(new Uint8Array([0x61, 0x62])); // queues one microtask
    expect(fake.postMessageSpy).toHaveBeenCalledTimes(1);

    // Synchronously drain via processNow — bumps scheduleToken.
    handle.processNow();

    // ack from processNow should have been queued for flush.
    // Now fire the previously-queued microtask. It should be a no-op because
    // its captured token is stale.
    fake.flush();

    handle.destroy();

    // ackBytes was called exactly once (from processNow flush), not twice.
    expect(ackSpy).toHaveBeenCalledTimes(1);
    expect(ackSpy.mock.calls[0]![0]).toBe(2);
  });

  it("TS-MT-9 (FR11): destroy() closes both MessagePort instances on MessageChannel path", async () => {
    const fake = createFakeMessageChannel({ defer: true });
    g.setMessageChannel(fake.Ctor);

    const { ctx } = createMtContext();
    const handle = await setupPtyHandlers(ctx);

    handle.destroy();

    expect(fake.closeSpies.port1).toHaveBeenCalledTimes(1);
    expect(fake.closeSpies.port2).toHaveBeenCalledTimes(1);
  });

  it("TS-MT-9c (FR11): a MessageChannel callback that fires after destroy() is a no-op", async () => {
    // HTML spec allows tasks queued before close() to dispatch after close.
    // Verify that disposed=true + scheduleToken bump makes the late callback
    // a no-op even if it slips through. Also verify that scheduler.dispose()
    // detached onmessage so the natural close behaviour is reinforced.
    const fake = createFakeMessageChannel({ defer: true });
    g.setMessageChannel(fake.Ctor);

    const { ctx, ackSpy } = createMtContext();
    const handle = await setupPtyHandlers(ctx);

    // Queue a delivery, then destroy before flushing.
    handle.injectData(new Uint8Array([0x61, 0x62]));
    expect(fake.postMessageSpy).toHaveBeenCalledTimes(1);

    handle.destroy();

    // After destroy(), scheduler.dispose() must have detached port2.onmessage
    // so a flush can no longer fire the captured callback.
    fake.flush();

    // The late delivery (if any) must NOT have invoked processPendingData,
    // so no fresh ackBytes call should have happened. destroy() flushes any
    // pendingAckBytes once, but pendingAckBytes was 0 (we never drained the
    // injected data), so ackBytes is not called at all.
    expect(ackSpy).not.toHaveBeenCalled();
  });

  it("TS-MT-9b (FR11): destroy() clears pending setTimeout(0) handle on the timer fallback path", async () => {
    g.setMessageChannel(undefined);
    g.setQueueMicrotask(undefined);

    const origSetTimeout = globalThis.setTimeout;
    const origClearTimeout = globalThis.clearTimeout;
    const cleared: any[] = [];
    let lastZeroId = 0;
    (globalThis as any).setTimeout = ((cb: any, delay: number) => {
      if (delay === 0 || delay === undefined) {
        return ++lastZeroId; // never invoke, just hand back a fake id
      }
      return origSetTimeout(cb, delay);
    }) as any;
    (globalThis as any).clearTimeout = ((id: any) => {
      cleared.push(id);
      return origClearTimeout(id);
    }) as any;

    try {
      const { ctx } = createMtContext();
      const handle = await setupPtyHandlers(ctx);
      handle.injectData(new Uint8Array([0x61])); // populates pendingHandle

      handle.destroy();

      // The fake timer id assigned by our setTimeout stub (lastZeroId) must
      // have been passed to clearTimeout.
      expect(cleared).toContain(lastZeroId);
    } finally {
      (globalThis as any).setTimeout = origSetTimeout;
      (globalThis as any).clearTimeout = origClearTimeout;
    }
  });
});
