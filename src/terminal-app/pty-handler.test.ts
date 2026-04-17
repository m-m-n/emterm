/**
 * Tests for setupPtyHandlers — focused on the shared WASM crash recovery entry
 * point and focus-listener lifecycle introduced by visibility-render-recovery.
 *
 * The full PTY pipeline is mocked; these tests only exercise:
 *   - tryRecoverFromWasmCrash classification + idempotency + unrecoverable gate
 *   - destroy() unlistening the Tauri focus callback
 */

import { beforeEach, afterEach, describe, expect, it, mock } from "bun:test";

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

  it("returns false and performs no recovery for unrelated errors", async () => {
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
