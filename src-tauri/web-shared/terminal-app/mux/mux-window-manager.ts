/**
 * Mux Window Manager functions extracted from TerminalApp.
 * Handles mux window/pane lifecycle: creation, switching, resizing, and cleanup.
 */

import { WasmGrid } from "../../terminal/wasm/terminal-core";
import { muxLog } from "../../terminal/mux/mux-logger";
import { MuxMessageType } from "../../terminal/mux/mux-client";
import type { MuxClient, MuxWindowInfo } from "../../terminal/mux/mux-client";
import type { TerminalState, MuxPaneGridState } from "../../terminal/state";
import type { ITerminalRenderer } from "../../terminal";
import {
  applyScrollState,
  resetScrollState,
  type ScrollStateTarget,
} from "../../terminal/state-mux-pane-scroll";
import type { KeyboardHandler } from "../handlers/keyboard";
import { SettingsService } from "../../settings/settings-service";
import { recordEvent } from "../diagnostics-history";

/**
 * Context needed by mux window manager functions.
 * Provides access to the subset of TerminalApp state these functions need.
 */
export interface MuxWindowManagerContext {
  getState: () => TerminalState | null;
  getRenderer: () => ITerminalRenderer | null;
  getMuxClient: () => MuxClient | null;
  getKeyboardHandler: () => KeyboardHandler | null;
  getInMuxMode: () => boolean;

  // Mux window/pane state accessors
  getMuxWindows: () => { id: number; name: string }[];
  getActiveMuxWindowIndex: () => number;
  setActiveMuxWindowIndex: (index: number) => void;
  getMuxPaneIds: () => number[];
  getMuxPaneGrids: () => Map<number, MuxPaneGridState>;
  getMuxDetachedGrids: () => Map<string, Uint8Array>;
  getMuxPendingWindowCount: () => number;
  setMuxPendingWindowCount: (count: number) => void;
  getMuxIsReattaching: () => boolean;
  setMuxIsReattaching: (value: boolean) => void;
  getMuxLastActiveIndex: () => number;
  getMuxReattachWindows: () => MuxWindowInfo[];

  // Callbacks
  onMuxStateChange: ((info: {
    windowCount: number;
    activeWindow: number;
    windowNames: string[];
  }) => void) | null;

  // PTY handler
  flushPtyPendingData: () => void;
  processPtyPendingDataNow: () => void;

  // Delegate methods that remain on TerminalApp
  registerCoreCallbacks: (core: ReturnType<TerminalState["getActiveCore"]>) => void;
  sendMuxControl: (msgType: number, paneId: number, payload?: Uint8Array) => void;
  exitMuxMode: () => void;
  enterMuxMode: (socketPath: string, sessionId: number) => Promise<void>;
  /** Sync the TerminalApp's title dedup cache to match state._title. Called
   *  after we swap in a different pane's saved title so the next OSC event
   *  isn't suppressed (and the parent tab title immediately reflects the
   *  new active window). */
  syncWindowTitleFromState: () => void;
}

/** Create a fresh WASM grid for a new mux pane and swap it in. */
export function createFreshMuxGrid(ctx: MuxWindowManagerContext): void {
  const state = ctx.getState();
  if (!state) return;
  // Discard any buffered PTY data from the previous pane to prevent
  // stale output (e.g. TUI app frames) from bleeding into the new grid.
  ctx.flushPtyPendingData();
  const cols = state.getWasmCore().cols();
  const rows = state.getWasmCore().rows();
  const oldPtr = corePtrOf(state.getActiveCore());
  const newGrid = new WasmGrid(cols, rows, 10000);
  const newPtr = corePtrOf(newGrid.core);
  state.swapPrimaryGrid(newGrid);
  const postSwapPtr = corePtrOf(state.getActiveCore());
  console.warn(
    `[DIAG-MUX-GRID] createFresh` +
    ` | oldActivePtr=${fmtPtr(oldPtr)}` +
    ` | newGridPtr=${fmtPtr(newPtr)}` +
    ` | postSwapActivePtr=${fmtPtr(postSwapPtr)}` +
    ` | cols=${cols} rows=${rows}` +
    ` | reused=${oldPtr === newPtr}`,
  );
  ctx.registerCoreCallbacks(state.getActiveCore());
  const renderer = ctx.getRenderer();
  if (renderer) {
    // Fresh pane starts at the bottom: reset the shared renderer's scroll
    // position and scroll-pin baseline so the previous pane's values are not
    // carried over (FR1/FR2). mux pane scroll baseline is outside the
    // ITerminalRenderer contract; the CanvasRenderer instance structurally
    // satisfies ScrollStateTarget.
    resetScrollState(renderer as unknown as ScrollStateTarget);
    renderer.forceRender(state);
  }
}

/** Read the wasm-bindgen pointer for an opaque core handle. -1 if missing.
 *  Identifies a TerminalCore instance for the bind-watch diagnostic so we
 *  can tell when mux switch swapped to a new core vs left the renderer
 *  pointing at the old one. */
function corePtrOf(core: unknown): number {
  return (core as { __wbg_ptr?: number } | null)?.__wbg_ptr ?? -1;
}

/** Format a pointer (as returned by corePtrOf) for logging. */
function fmtPtr(ptr: number): string {
  return `0x${(ptr >>> 0).toString(16)}`;
}

/** Log a MuxPaneGridState save/restore so we can confirm whether multiple
 *  panes are unexpectedly sharing the same WasmGrid instance after a WASM
 *  recovery. Emits primary + alternate __wbg_ptr alongside the paneId and
 *  call-site so the resulting log trail can be diffed across panes. */
function logGridSnapshot(
  action: "save" | "restore",
  paneId: number,
  snapshot: MuxPaneGridState,
  callsite: string,
): void {
  const primaryPtr = corePtrOf(snapshot.primaryGrid.core);
  const altPtr = snapshot.alternateGrid ? corePtrOf(snapshot.alternateGrid.core) : -1;
  console.warn(
    `[DIAG-MUX-GRID] ${action} | paneId=${paneId}` +
    ` | primaryPtr=${fmtPtr(primaryPtr)}` +
    ` | altPtr=${fmtPtr(altPtr)}` +
    ` | useAlt=${snapshot.useAlternate}` +
    ` | callsite=${callsite}`,
  );
}

/** Wall-clock (perf clock) timestamp of the last completed mux switch. Read
 *  by the heartbeat to print `lastSwitchAgoMs`. 0 means no switch has
 *  happened in this session. Updated at the very end of switchMuxWindow. */
let _lastMuxSwitchAt: number = 0;
export function getLastMuxSwitchAt(): number {
  return _lastMuxSwitchAt;
}

/** Sliding-window log of recent mux switch timestamps (perf clock).
 *  Pushed at the start of switchMuxWindow; consumed by the heartbeat to
 *  surface "switch storm" patterns (e.g. 8 switches in 4 minutes correlated
 *  with a renderer hang). Trimmed lazily in getMuxSwitchCountWithin. */
const _muxSwitchTimestamps: number[] = [];
export function getMuxSwitchCountWithin(windowMs: number): number {
  const cutoff = performance.now() - windowMs;
  while (_muxSwitchTimestamps.length > 0 && _muxSwitchTimestamps[0]! < cutoff) {
    _muxSwitchTimestamps.shift();
  }
  return _muxSwitchTimestamps.length;
}

/** Throttle window for `emitMuxStateChange` calls during a mux reattach burst.
 *  The original coalescing emitted 11 times for an 11-pane reattach; the
 *  per-pane skip introduced for FR7 dropped that to 1 (final emit). However,
 *  if the daemon delivers fewer PaneCreated messages than expected (transport
 *  drop, daemon-side error, partial reply), the final-pane condition never
 *  fires and the tab bar is never updated for the panes that DID arrive.
 *  This throttle adds a safety net: during reattach, emit at most once per
 *  REATTACH_EMIT_THROTTLE_MS so the UI surfaces partial progress without
 *  reintroducing the per-pane storm. The first PaneCreated of a fresh reattach
 *  always emits because the initial 0 (or stale timestamp from a long-ago
 *  reattach) makes `now - _lastReattachEmitAt >= THROTTLE_MS` trivially true. */
const REATTACH_EMIT_THROTTLE_MS = 150;
let _lastReattachEmitAt = 0;

/** Switch to the current activeMuxWindowIndex: swap WASM grids and update UI. */
export function switchMuxWindow(ctx: MuxWindowManagerContext, previousIndex?: number): void {
  // Record switch *attempt* timestamp eagerly so the heartbeat's
  // `lastSwitchAgoMs` reflects "user just tried to switch" even if we
  // bail out below or hit an exception in the middle of the swap. The
  // alternative (record only on success) would mask freezes that abort
  // mid-switch — the very pattern we are hunting.
  _lastMuxSwitchAt = performance.now();
  _muxSwitchTimestamps.push(_lastMuxSwitchAt);
  if (_muxSwitchTimestamps.length > 1024) _muxSwitchTimestamps.shift();
  const state = ctx.getState();
  if (!state) return;
  const muxPaneIds = ctx.getMuxPaneIds();
  const muxPaneGrids = ctx.getMuxPaneGrids();
  // Capture pre-switch core identity so the bind-watch log can show whether
  // the active TerminalCore actually changed across the swap. If two
  // consecutive switches both report the same corePtr, save/restore is not
  // doing what we think it is.
  const prePaneId = previousIndex != null ? muxPaneIds[previousIndex] : -1;
  const preCorePtr = corePtrOf(state.getActiveCore());

  // Capture current terminal dimensions from the active (prev) pane before
  // save/restore. muxPaneGrids entries are only resized when their pane is
  // active, so a pane that was inactive during a terminal resize (e.g., mux
  // status bar appeared after reattach) holds a grid at stale dimensions.
  // We re-apply the current size after restoreMuxPaneState so forceRender
  // paints the correct area and sendMuxPaneResize sends the correct size to
  // the daemon.
  const targetCols = state.cols;
  const targetRows = state.rows;

  // Save current pane's full state (primary + alternate)
  if (previousIndex != null) {
    const prevPaneId = muxPaneIds[previousIndex];
    if (prevPaneId != null) {
      // mux pane scroll baseline is outside the ITerminalRenderer contract;
      // the CanvasRenderer instance structurally satisfies ScrollStateTarget.
      const prevRenderer = ctx.getRenderer() as unknown as ScrollStateTarget | null;
      // saveMuxPaneState records the (mux-shared) renderer's scroll position into
      // the snapshot so the outgoing pane's scroll-up position is restored later
      // (FR1/FR2). Falls back to 0 when no renderer is available.
      const snapshot = state.saveMuxPaneState(prevRenderer ?? undefined);
      muxPaneGrids.set(prevPaneId, snapshot);
      logGridSnapshot("save", prevPaneId, snapshot, "switchMuxWindow");
      // Clear callbacks on saved grids to prevent OSC events from inactive panes
      // leaking into the shared pendingOscQueue and polluting the active window's title
      snapshot.primaryGrid.core.clear_callbacks();
      snapshot.alternateGrid?.core.clear_callbacks();
    }
  }

  // Discard any buffered PTY data from the previous pane
  ctx.flushPtyPendingData();

  // Restore the target pane's state
  const newPaneId = muxPaneIds[ctx.getActiveMuxWindowIndex()];
  if (newPaneId != null) {
    const savedState = muxPaneGrids.get(newPaneId);
    if (savedState) {
      muxPaneGrids.delete(newPaneId);
      logGridSnapshot("restore", newPaneId, savedState, "switchMuxWindow");
      state.restoreMuxPaneState(savedState);
      ctx.registerCoreCallbacks(state.getActiveCore());
      // Restore the renderer's scroll position to this pane's saved value.
      // Applied after restoreMuxPaneState so setScrollOffset clamps against
      // the restored buffer's scrollback length (FR1/FR2).
      // mux pane scroll baseline is outside the ITerminalRenderer contract;
      // the CanvasRenderer instance structurally satisfies ScrollStateTarget.
      const restoreRenderer = ctx.getRenderer() as unknown as ScrollStateTarget | null;
      if (restoreRenderer) applyScrollState(savedState, restoreRenderer);
    } else {
      // No saved state (first visit, e.g. after reattach). Seed the title
      // from the daemon-provided window name so syncWindowTitleFromState
      // does not overwrite muxWindows[i].name with the "Terminal" fallback.
      const branchActivePtr = corePtrOf(state.getActiveCore());
      console.warn(
        `[DIAG-MUX-GRID] freshBranch` +
        ` | paneId=${newPaneId}` +
        ` | activeCorePtrBefore=${fmtPtr(branchActivePtr)}` +
        ` | callsite=switchMuxWindow` +
        ` | reason=noSavedState`,
      );
      // Allocate a fresh per-pane WasmGrid. Reusing the shared active core
      // via .reset() would let subsequent saveMuxPaneState() calls capture
      // the SAME WasmGrid instance across panes, surfacing as cross-pane
      // content bleed and eventual WASM heap corruption.
      createFreshMuxGrid(ctx);
      const windows = ctx.getMuxWindows();
      state._title = windows[ctx.getActiveMuxWindowIndex()]?.name ?? "";
      state._iconName = "";
    }
  }

  // Reconcile restored grid dimensions with the current terminal size.
  reconcileActivePaneSize(state, targetCols, targetRows);

  // Sync the title dedup cache and parent tab title to the restored pane.
  ctx.syncWindowTitleFromState();

  // Notify daemon of active pane change for status bar cwd tracking
  const activePaneId = muxPaneIds[ctx.getActiveMuxWindowIndex()];
  if (activePaneId != null) {
    ctx.sendMuxControl(MuxMessageType.SwitchWindow, activePaneId);
    // Reconcile daemon-side PTY dimensions with the current terminal size.
    // The newly activated pane may have stale dimensions (e.g., initialized
    // during reattach before the status bar was restored), which would cause
    // `stty size` to report the wrong row count.
    sendMuxPaneResize(ctx, activePaneId);
    // Request an on-demand screen snapshot so the displayed grid is
    // reconciled with the daemon shadow_parser authoritative state. Without
    // this, any bytes that arrived in the gap between the click and
    // flushPtyPendingData (or any ambiguity in the inactive-pane processing
    // path) can leave the target pane's saved grid stale.
    requestPaneSnapshot(ctx, activePaneId);
  }

  const renderer = ctx.getRenderer();
  // Log the bind-watch line BEFORE diagTraceMuxRender so the race detector,
  // armed by notifyContextReset, can flag the very forceRender that
  // diagTrace triggers (if it ever winds up running on the wrong core).
  const postPaneId = muxPaneIds[ctx.getActiveMuxWindowIndex()] ?? -1;
  const postCorePtr = corePtrOf(state.getActiveCore());
  console.warn(
    `[DIAG-MUX-BIND] switchMuxWindow` +
    ` | paneId=${prePaneId}→${postPaneId}` +
    ` | corePtr=0x${(preCorePtr >>> 0).toString(16)}→0x${(postCorePtr >>> 0).toString(16)}` +
    ` | ptrChanged=${preCorePtr !== postCorePtr}`,
  );
  try {
    recordEvent("mux-switch", `paneId=${prePaneId}→${postPaneId}`);
  } catch { /* never let diagnostics break mux switching */ }
  if (renderer) {
    // Arm the race detector with the new core's identity. Any subsequent
    // render() / forceRender() within the race window that lands on a
    // different core (i.e. wrong active pane) will produce a [DIAG-MUX-RACE]
    // warn from inside the renderer.
    (renderer as unknown as {
      notifyContextReset?: (label: string, expectedRef: number, activePaneId: number) => void;
    }).notifyContextReset?.("muxSwitch", postCorePtr, postPaneId);
    diagTraceMuxRender(state, renderer, "switchMuxWindow");
  }
  emitMuxStateChange(ctx);
}

/** Send RequestPaneSnapshot to the daemon for the given pane. The daemon
 *  replies with a PtyOutput frame containing a screen reset + shadow parser
 *  replay, which the normal PTY pipeline applies to the active WASM grid. */
function requestPaneSnapshot(ctx: MuxWindowManagerContext, paneId: number): void {
  const client = ctx.getMuxClient();
  if (!client) return;
  client.sendRequestPaneSnapshot(paneId).catch((err: unknown) => {
    muxLog.warn(
      `sendRequestPaneSnapshot failed for pane ${paneId}: ${
        err instanceof Error ? err.message : String(err)
      }`,
    );
  });
}

/** Resize the currently-active grid to match the given dimensions if they
 *  differ. Used after restoreMuxPaneState to compensate for saved grids that
 *  went stale while their pane was inactive (saved grids are not touched by
 *  ResizeObserver). */
function reconcileActivePaneSize(state: TerminalState, cols: number, rows: number): void {
  if (state.cols === cols && state.rows === rows) return;
  state.resize(cols, rows);
}

/** Sample a 64x64 region in the center of `canvas` and return a stable FNV-1a
 *  hash of the RGB bytes (alpha skipped). Used to detect "forceRender ran but
 *  no pixels changed" — the symptom users describe as "screen frozen but keys
 *  go through". Returns "n/a" / "no-ctx" / "err" sentinels on failure so the
 *  diagnostic never throws.
 *
 *  Implementation note: we explicitly DO NOT call getContext on the production
 *  canvas — that would (a) lock its context type permanently (breaks future
 *  WebGL/WebGPU migration) and (b) potentially demote its backing store from
 *  GPU to CPU on first getImageData (WebKit/Chromium heuristic), permanently
 *  slowing every subsequent render of that pane. Instead we draw the sampled
 *  region into a throwaway off-screen canvas (created with
 *  `willReadFrequently:true` so the readback path is fast) and getImageData
 *  from the copy. drawImage stays GPU→GPU and the production canvas's
 *  context attributes are untouched. */
/** Stable per-canvas-instance ID assigned on first sighting. Used by mux
 *  switch diagnostics to detect whether a switch swaps to a different canvas
 *  element (legitimate — each pane owns its own) or stays on the same one
 *  (the case where a canvas surface is stuck). Never grows beyond the number
 *  of canvases the user creates in a session. */
const _canvasIdMap: WeakMap<HTMLCanvasElement, number> = new WeakMap();
let _canvasIdCounter = 0;
function canvasIdOf(c: HTMLCanvasElement | undefined): number {
  if (!c) return -1;
  let id = _canvasIdMap.get(c);
  if (id === undefined) {
    _canvasIdCounter++;
    id = _canvasIdCounter;
    _canvasIdMap.set(c, id);
  }
  return id;
}

/** Read the 4x4 probe region painted by CanvasRenderer.forceRender at the
 *  bottom-right corner of `canvas` and return the actual RGB of the
 *  top-left probe pixel. Compared against the (probeR,probeG,probeB) the
 *  renderer recorded into _lastForceRenderDiag — mismatch means the
 *  Canvas2D fillRect committed in TS-land but the surface didn't update,
 *  which is the smoking gun for "renderer thinks it drew but the GPU
 *  texture is stuck". Like sampleCanvasHash, isolated via off-screen
 *  drawImage so the production canvas isn't demoted to CPU. */
function sampleProbePixel(
  canvas: HTMLCanvasElement | undefined,
  sxDev: number,
  syDev: number,
  wDev: number,
  hDev: number,
): { r: number; g: number; b: number; ok: boolean } {
  if (!canvas) return { r: 0, g: 0, b: 0, ok: false };
  try {
    if (wDev <= 0 || hDev <= 0) return { r: 0, g: 0, b: 0, ok: false };
    if (sxDev + wDev > canvas.width || syDev + hDev > canvas.height) {
      return { r: 0, g: 0, b: 0, ok: false };
    }
    const off = document.createElement("canvas");
    off.width = wDev;
    off.height = hDev;
    const offCtx = off.getContext("2d", { willReadFrequently: true });
    if (!offCtx) return { r: 0, g: 0, b: 0, ok: false };
    offCtx.drawImage(canvas, sxDev, syDev, wDev, hDev, 0, 0, wDev, hDev);
    const data = offCtx.getImageData(0, 0, wDev, hDev).data;
    return { r: data[0] ?? 0, g: data[1] ?? 0, b: data[2] ?? 0, ok: true };
  } catch {
    return { r: 0, g: 0, b: 0, ok: false };
  }
}

function sampleCanvasHash(canvas: HTMLCanvasElement | undefined): string {
  if (!canvas) return "n/a";
  try {
    const w = Math.min(64, canvas.width);
    const h = Math.min(64, canvas.height);
    if (w <= 0 || h <= 0) return "0";
    const x = Math.max(0, Math.floor((canvas.width - w) / 2));
    const y = Math.max(0, Math.floor((canvas.height - h) / 2));

    const off = document.createElement("canvas");
    off.width = w;
    off.height = h;
    const offCtx = off.getContext("2d", { willReadFrequently: true });
    if (!offCtx) return "no-ctx";
    offCtx.drawImage(canvas, x, y, w, h, 0, 0, w, h);
    const data = offCtx.getImageData(0, 0, w, h).data;
    let h32 = 2166136261;
    const len = data.length;
    for (let i = 0; i < len; i += 4) {
      h32 = Math.imul(h32 ^ (data[i] ?? 0), 16777619);
      h32 = Math.imul(h32 ^ (data[i + 1] ?? 0), 16777619);
      h32 = Math.imul(h32 ^ (data[i + 2] ?? 0), 16777619);
    }
    return "0x" + (h32 >>> 0).toString(16).padStart(8, "0");
  } catch {
    return "err";
  }
}

/** Diagnostic trace for forceRender on mux switch paths. Logs canvas state,
 *  grid dimensions, parent visibility, and samples again on next rAF to see
 *  whether anything overwrites the canvas after forceRender. */
function diagTraceMuxRender(state: TerminalState, renderer: ITerminalRenderer, callsite: string): void {
  try {
    const activeCore = state.getActiveCore();
    const rows = activeCore.rows();
    const cols = activeCore.cols();
    const dirtyBefore = state.getDirtyRows().length;
    const rend = renderer as unknown as { canvas?: HTMLCanvasElement; cols?: number; rows?: number; scrollOffset?: number; dpr?: number };
    const cw = rend.canvas?.width ?? -1;
    const ch = rend.canvas?.height ?? -1;
    const cssW = rend.canvas?.clientWidth ?? -1;
    const cssH = rend.canvas?.clientHeight ?? -1;
    const parent = rend.canvas?.parentElement;
    const parentDisplay = parent ? getComputedStyle(parent).display : "n/a";
    const parentVis = parent ? getComputedStyle(parent).visibility : "n/a";
    const isReady = state.isReady?.() ?? "n/a";
    console.warn(
      `[DIAG-MUX-RENDER][${callsite}] pre-forceRender` +
      ` | gridCols=${cols} gridRows=${rows}` +
      ` | rendCols=${rend.cols} rendRows=${rend.rows}` +
      ` | scrollOffset=${rend.scrollOffset}` +
      ` | canvasPx=${cw}x${ch} cssPx=${cssW}x${cssH} dpr=${rend.dpr}` +
      ` | parentDisplay=${parentDisplay} parentVis=${parentVis}` +
      ` | dirtyRows=${dirtyBefore}` +
      ` | isReady=${isReady}`,
    );
    const t0 = performance.now();
    renderer.forceRender(state);
    const t1 = performance.now();
    const dirtyAfter = state.getDirtyRows().length;
    // Capture the canvas reference at sample time. If the renderer swaps
    // the underlying canvas between now and the next rAF (e.g. a resize
    // recreates it), comparing rafHash against postHash from a different
    // canvas would be a meaningless signal during freeze triage. We
    // compare canvas0 === rend.canvas later to gate the equality check.
    const canvas0 = rend.canvas;
    const postHash = sampleCanvasHash(canvas0);
    // DOM connectivity probe: distinguishes "canvas in DOM, getting
    // pixels" (real freeze) from "canvas detached or zero-sized" (DOM
    // wiring problem). isConnected=false / rect 0x0 / parentDisplay=none
    // would all indicate the user can't possibly be seeing this canvas.
    const isConn = canvas0?.isConnected ?? false;
    const inDoc = canvas0 ? document.contains(canvas0) : false;
    const rect = canvas0?.getBoundingClientRect();
    const visW = rect?.width.toFixed(0) ?? "n/a";
    const visH = rect?.height.toFixed(0) ?? "n/a";
    // Snapshot the renderer's internal forceRender counters (set on the
    // CanvasRenderer's _lastForceRenderDiag private field). Tells us
    // whether forceRender actually iterated visible lines and called
    // fillRect / textRender, vs short-circuiting somehow.
    const fdiag = (renderer as unknown as {
      _lastForceRenderDiag?: {
        visibleLines: number;
        emptyRows: number;
        bgFillCount: number;
        textPassRows: number;
        cursorRendered: boolean;
        probeR: number;
        probeG: number;
        probeB: number;
        probeSxDev: number;
        probeSyDev: number;
        probeWDev: number;
        probeHDev: number;
        ctxFillStyle: string;
        ctxFont: string;
        ctxAlpha: number;
        ctxComposite: string;
        ctxTxA: number;
        ctxTxD: number;
        ctxTxE: number;
        ctxTxF: number;
      };
    })._lastForceRenderDiag;
    const fdiagStr = fdiag
      ? `vl=${fdiag.visibleLines} empty=${fdiag.emptyRows} bgFill=${fdiag.bgFillCount} text=${fdiag.textPassRows} cur=${fdiag.cursorRendered}`
      : "n/a";
    // Read back the probe pixel that CanvasRenderer painted at the bottom-
    // right. match=true means Canvas2D actually committed pixels; false
    // means the API succeeded in TS-land but the surface is stuck (the
    // freeze fingerprint we're hunting). Coords come back from the renderer
    // in device pixels, computed at probe-paint time, so a DPR drift
    // between paint and readback cannot misalign the sample.
    const probeRead = fdiag
      ? sampleProbePixel(canvas0, fdiag.probeSxDev, fdiag.probeSyDev, fdiag.probeWDev, fdiag.probeHDev)
      : null;
    const probeStr = fdiag && probeRead
      ? probeRead.ok
        ? `wantRGB=${fdiag.probeR},${fdiag.probeG},${fdiag.probeB}` +
          ` gotRGB=${probeRead.r},${probeRead.g},${probeRead.b}` +
          ` match=${probeRead.r === fdiag.probeR && probeRead.g === fdiag.probeG && probeRead.b === fdiag.probeB}`
        : `wantRGB=${fdiag.probeR},${fdiag.probeG},${fdiag.probeB} gotRGB=READ-FAIL match=n/a`
      : "n/a";
    // 2D context state. globalAlpha=0, transform a/d=0, or a destination-out
    // composite mode would all silently nullify fillRect. fillStyle/font are
    // captured at probe-paint time so they reflect the very last fillStyle
    // the renderer set (the probe color), proving the value actually stuck
    // on the context object.
    const ctxStr = fdiag
      ? `fill=${fdiag.ctxFillStyle.substring(0, 20)} font=${fdiag.ctxFont.substring(0, 24)}` +
        ` alpha=${fdiag.ctxAlpha} comp=${fdiag.ctxComposite}` +
        ` tx=${fdiag.ctxTxA.toFixed(2)},${fdiag.ctxTxD.toFixed(2)},${fdiag.ctxTxE.toFixed(1)},${fdiag.ctxTxF.toFixed(1)}`
      : "n/a";
    // CSS state on the canvas itself. opacity:0, visibility:hidden, a
    // filter:blur, or mix-blend-mode:multiply with a black overlay would
    // all keep the surface "alive" while making it invisible to the user.
    const gcs = canvas0 ? getComputedStyle(canvas0) : null;
    const cssStr = gcs
      ? `op=${gcs.opacity} vis=${gcs.visibility} disp=${gcs.display}` +
        ` fil=${gcs.filter} mix=${gcs.mixBlendMode} iso=${gcs.isolation}`
      : "n/a";
    const canvasId = canvasIdOf(canvas0);
    // FNV-1a hash of the entire viewport grid content (codepoint bytes,
    // width, fg/bg/flags). Logged alongside canvasHash so a human reading
    // the log can do the cross-comparison:
    //   - gridHash differs but canvasHash same → render path failed to commit
    //   - gridHash same and canvasHash same     → no actual change to draw
    //   - gridHash same but canvasHash differs  → renderer touched pixels
    //     unrelated to grid (e.g. stale overlay)
    // Limitation: overflow cells (codepoint stored in side table when
    // char_len == 0xFF) only contribute their attributes to the hash, not
    // the actual grapheme bytes — different overflow strings with the
    // same width/colors hash identically. Acceptable for freeze triage.
    const postGridHash = `0x${(activeCore.grid_content_hash() >>> 0).toString(16).padStart(8, "0")}`;
    console.warn(
      `[DIAG-MUX-RENDER][${callsite}] post-forceRender` +
      ` | elapsed=${(t1 - t0).toFixed(2)}ms` +
      ` | dirtyAfter=${dirtyAfter}` +
      ` | canvasPx=${canvas0?.width}x${canvas0?.height}` +
      ` | canvasId=${canvasId}` +
      ` | canvasHash=${postHash}` +
      ` | gridHash=${postGridHash}` +
      ` | dom=conn:${isConn},inDoc:${inDoc},rect:${visW}x${visH}` +
      ` | fdiag=${fdiagStr}` +
      ` | probe=${probeStr}` +
      ` | ctx=${ctxStr}` +
      ` | css=${cssStr}`,
    );
    requestAnimationFrame(() => {
      const canvas1 = rend.canvas;
      const cw2 = canvas1?.width ?? -1;
      const ch2 = canvas1?.height ?? -1;
      const rafHash = sampleCanvasHash(canvas1);
      const sizeChanged = cw !== cw2 || ch !== ch2;
      const sameCanvas = canvas0 !== undefined && canvas0 === canvas1;
      const resizeNote = sizeChanged
        ? (() => {
            const parentDisplay2 = parent ? getComputedStyle(parent).display : "n/a";
            const parentVis2 = parent ? getComputedStyle(parent).visibility : "n/a";
            return ` | canvas-resized from=${cw}x${ch} to=${cw2}x${ch2}` +
                   ` parentDisplay=${parentDisplay2} parentVis=${parentVis2}`;
          })()
        : "";
      // postHashEq is only meaningful when both samples returned a real hex
      // digest AND were taken on the same canvas instance. Sentinel-vs-
      // sentinel collisions and canvas-identity drift both yield the
      // 'n/a-*' branch so neither gets misread as 'forceRender produced no
      // pixel change' during freeze triage.
      const bothSampled = postHash.startsWith("0x") && rafHash.startsWith("0x");
      const postHashEq = !sameCanvas
        ? "n/a-canvas-changed"
        : bothSampled
          ? String(rafHash === postHash)
          : "n/a";
      console.warn(
        `[DIAG-MUX-RENDER][${callsite}] next-rAF` +
        ` | canvasHash=${rafHash}` +
        ` | postHashEq=${postHashEq}` +
        resizeNote,
      );
    });
  } catch (err) {
    console.warn(`[DIAG-MUX-RENDER][${callsite}] diag error: ${err instanceof Error ? err.message : String(err)}`);
    renderer.forceRender(state);
  }
}

/** Handle PaneCreated from daemon — register actual pane ID and update UI. */
export function handleMuxPaneCreated(ctx: MuxWindowManagerContext, paneId: number): void {
  if (ctx.getMuxPendingWindowCount() <= 0) return;
  ctx.setMuxPendingWindowCount(ctx.getMuxPendingWindowCount() - 1);

  // Snapshot reattach state at function entry per SPEC FR9. The finalize block
  // below flips setMuxIsReattaching(false) mid-function, so subsequent
  // FR6 / FR7 / FR8 decisions MUST use this captured value rather than
  // re-querying ctx.getMuxIsReattaching(). Capturing here also fixes the prior
  // location-violation flagged by the spec reviewer (the snapshot was
  // previously taken further down, after sendMuxPaneResize).
  const wasReattachingThisCall = ctx.getMuxIsReattaching();

  const state = ctx.getState();
  const muxPaneIds = ctx.getMuxPaneIds();
  const muxPaneGrids = ctx.getMuxPaneGrids();
  const muxWindows = ctx.getMuxWindows();
  const muxDetachedGrids = ctx.getMuxDetachedGrids();

  // Save current pane's full state (primary + alternate) before switching.
  // During reattach, process pending PTY data synchronously first so the
  // alternate screen state from the daemon's replay is captured in the save.
  const previousIndex = ctx.getActiveMuxWindowIndex();
  const prevPaneId = muxPaneIds[previousIndex];
  const hadPrevPane = prevPaneId != null && state != null;
  if (hadPrevPane) {
    if (wasReattachingThisCall) {
      ctx.processPtyPendingDataNow();
    }
    // mux pane scroll baseline is outside the ITerminalRenderer contract;
    // the CanvasRenderer instance structurally satisfies ScrollStateTarget.
    const prevRenderer = ctx.getRenderer() as unknown as ScrollStateTarget | null;
    const snapshot = state!.saveMuxPaneState(prevRenderer ?? undefined);
    muxPaneGrids.set(prevPaneId, snapshot);
    logGridSnapshot("save", prevPaneId, snapshot, "handleMuxPaneCreated");
    // Clear callbacks on saved grids to prevent OSC leaking from inactive panes
    snapshot.primaryGrid.core.clear_callbacks();
    snapshot.alternateGrid?.core.clear_callbacks();
    // Reset shared title state so the NEW window doesn't inherit the
    // previous pane's title via initialName / dedup. The previous pane's
    // title is preserved inside its saved MuxPaneGridState.
    state!._title = "";
    state!._iconName = "";
  }

  const newIdx = muxWindows.length;
  // Determine initial window name and daemon window ID:
  // During reattach, use daemon-provided window info (matched by index position,
  // since PaneCreated messages arrive in the same order as the windows array).
  let initialName = hadPrevPane ? "Terminal" : (state?.title || "Terminal");
  let daemonWindowId = newIdx; // fallback: use frontend index
  if (wasReattachingThisCall) {
    const reattachWindows = ctx.getMuxReattachWindows();
    const winInfo = reattachWindows[newIdx];
    if (winInfo) {
      daemonWindowId = winInfo.id;
      if (winInfo.name) {
        initialName = winInfo.name;
      }
    }
  }
  muxWindows.push({ id: daemonWindowId, name: initialName });
  muxPaneIds.push(paneId);
  ctx.setActiveMuxWindowIndex(newIdx);

  console.warn(
    `[DIAG-MUX-ATTACH] push window newIdx=${newIdx} daemonId=${daemonWindowId} paneId=${paneId} reattach=${wasReattachingThisCall}`,
  );

  muxLog.info(`Mux pane created: id=${paneId}, window=${newIdx}`);

  // Try to restore from detached snapshot, otherwise create fresh grid
  const detachedKey = `pane-${paneId}`;
  const detachedSnapshot = muxDetachedGrids.get(detachedKey);
  // Shadow parser on the daemon side now handles screen restoration via PtyOutput.
  // Skip slow frontend snapshot restore (WASM deserialization takes ~3s in debug).
  // Just create a fresh grid — the daemon's screen data will populate it.
  createFreshMuxGrid(ctx);
  muxDetachedGrids.delete(detachedKey);

  // Sync title dedup / parent tab to the fresh pane (empty title) so that
  // the previous pane's title doesn't linger on the parent tab until the
  // new pane emits its own OSC.
  // Skip during reattach — initialName was just set from daemon-provided
  // window name, and syncWindowTitleFromState would overwrite it with the
  // empty state._title (which would also clobber daemon-side via RenameWindow).
  if (hadPrevPane && !wasReattachingThisCall) {
    ctx.syncWindowTitleFromState();
  }

  // Send initial resize so daemon PTY matches actual terminal dimensions
  sendMuxPaneResize(ctx, paneId);

  // Ensure canvas reflects restored/fresh grid (without this, canvas stays blank).
  // Skip during reattach: the final switchMuxWindow below performs a forceRender
  // on the restored active pane, and intermediate forceRender calls on
  // not-yet-active panes contribute to the 1-2s reattach storm (slow-render
  // 40-85ms × N panes, plus event-loop hangs >500ms) without visible benefit.
  const renderer = ctx.getRenderer();
  if (renderer && state && !wasReattachingThisCall) {
    renderer.forceRender(state);
  }

  // After all pending windows are received during reattach, switch to first window.
  // Wrap the swap/render path in try/finally so that a thrown exception in
  // switchMuxWindow or renderer.forceRender (e.g. WASM RuntimeError after
  // suspend/resume) does NOT leak isReattaching=true permanently. The flag
  // gates several downstream paths (wasReattachingThisCall snapshot in
  // future calls, syncWindowTitleFromState skip, processPtyPendingDataNow,
  // and the emit gate below); leaving it stuck silently degrades all of them.
  if (wasReattachingThisCall && ctx.getMuxPendingWindowCount() === 0) {
    try {
      // Process any pending output for the last pane before switching,
      // so its screen data and OSC title are captured in the saved state.
      ctx.processPtyPendingDataNow();

      // Restore the active window from before detach (clamped to valid range)
      const targetIndex = Math.min(ctx.getMuxLastActiveIndex(), muxWindows.length - 1);
      if (targetIndex !== ctx.getActiveMuxWindowIndex()) {
        const prev = ctx.getActiveMuxWindowIndex();
        ctx.setActiveMuxWindowIndex(targetIndex);
        switchMuxWindow(ctx, prev);
      } else if (renderer && state) {
        // Edge case (FR10): the pre-detach active window is the last-attached
        // one, so switchMuxWindow above does not fire. With per-pane
        // forceRender skipped during reattach, no canvas paint would occur
        // unless we force one here. The current `state` already corresponds
        // to the active pane's grid (it was just registered as
        // newIdx === targetIndex), so a single render is sufficient.
        renderer.forceRender(state);
      }
    } finally {
      ctx.setMuxIsReattaching(false);

      // Request status bar content from daemon after reattach completes.
      // Run in finally so a render failure above does not skip the request.
      ctx.getMuxClient()?.sendRequestStatusUpdate().catch(() => {});
    }
  }

  // Mux-state-change emits during reattach were originally fired per pane (11
  // emits for an 11-pane reattach, each triggering a tab-bar repaint and
  // contributing to the slow-render / event-loop-hang storm). The
  // happy-path final emit (pendingCount===0) handles a clean reattach in a
  // single emit. To avoid the bad-path failure mode where the daemon delivers
  // fewer PaneCreated messages than expected — pendingCount never reaches 0
  // and the tab bar is never updated for panes that DID arrive — also emit
  // during the burst at most once per REATTACH_EMIT_THROTTLE_MS so partial
  // progress is still visible to the user.
  if (!wasReattachingThisCall || ctx.getMuxPendingWindowCount() === 0) {
    emitMuxStateChange(ctx);
  } else {
    const now = performance.now();
    if (now - _lastReattachEmitAt >= REATTACH_EMIT_THROTTLE_MS) {
      _lastReattachEmitAt = now;
      emitMuxStateChange(ctx);
    }
  }
}

/** Send a Resize message to the daemon for a single pane using current terminal dimensions.
 *  Forces SIGWINCH by first sending a slightly smaller size, then the actual size.
 *  This ensures the shell redraws even if the PTY was already at the same dimensions. */
export function sendMuxPaneResize(ctx: MuxWindowManagerContext, paneId: number): void {
  const state = ctx.getState();
  const muxClient = ctx.getMuxClient();
  if (!state || !muxClient) return;
  const cols = state.getWasmCore().cols();
  const rows = state.getWasmCore().rows();

  // Send a slightly different size first to guarantee SIGWINCH
  const kickCols = Math.max(1, cols - 1);
  const kickPayload = new Uint8Array(4);
  kickPayload[0] = kickCols & 0xFF;
  kickPayload[1] = (kickCols >> 8) & 0xFF;
  kickPayload[2] = rows & 0xFF;
  kickPayload[3] = (rows >> 8) & 0xFF;
  ctx.sendMuxControl(MuxMessageType.Resize, paneId, kickPayload);

  // Then send the actual size to restore correct dimensions
  const payload = new Uint8Array(4);
  payload[0] = cols & 0xFF;
  payload[1] = (cols >> 8) & 0xFF;
  payload[2] = rows & 0xFF;
  payload[3] = (rows >> 8) & 0xFF;
  ctx.sendMuxControl(MuxMessageType.Resize, paneId, payload);
}

/** Handle a mux pane exiting (shell closed). Remove the window and switch if needed. */
export function handleMuxPaneExited(ctx: MuxWindowManagerContext, paneId: number): void {
  // Notify daemon to clean up the exited pane (cascade: pane->window->session)
  ctx.sendMuxControl(MuxMessageType.DestroyPane, paneId);

  const muxPaneIds = ctx.getMuxPaneIds();
  const muxWindows = ctx.getMuxWindows();
  const muxPaneGrids = ctx.getMuxPaneGrids();

  const windowIdx = muxPaneIds.indexOf(paneId);
  if (windowIdx === -1) return;

  muxLog.info(`Mux pane ${paneId} exited (window ${windowIdx})`);

  // Clean up snapshot for the exited pane — dispose WASM grids to free memory
  const savedState = muxPaneGrids.get(paneId);
  if (savedState) {
    savedState.primaryGrid.dispose();
    savedState.alternateGrid?.dispose();
    muxPaneGrids.delete(paneId);
  }

  // If the exited pane is NOT the active one, save current pane's snapshot
  // before the index adjustment that follows
  const wasActive = windowIdx === ctx.getActiveMuxWindowIndex();

  // Remove the window
  muxWindows.splice(windowIdx, 1);
  muxPaneIds.splice(windowIdx, 1);

  // If no windows left, exit mux mode
  if (muxWindows.length === 0) {
    ctx.exitMuxMode();
    return;
  }

  // Adjust active window index
  if (ctx.getActiveMuxWindowIndex() >= muxWindows.length) {
    ctx.setActiveMuxWindowIndex(muxWindows.length - 1);
  }

  // Only switch if the active pane was the one that exited
  if (wasActive) {
    switchMuxWindow(ctx);
  } else {
    emitMuxStateChange(ctx);
  }
}

/**
 * Reorder `muxWindows` / `muxPaneIds` atomically, adjusting
 * `activeMuxWindowIndex` to follow the movement, and emit a state-change.
 *
 * Semantics (remove-then-insert, matching `MuxSession::move_window` on the
 * daemon side): the element at `fromIndex` is removed, then re-inserted at
 * `toIndex`. Both indices are 0-based.
 *
 * `activeMuxWindowIndex` adjustment:
 * - if the active window is the one being moved, the active index follows
 *   the element to its new position (`toIndex`);
 * - otherwise, the active index is shifted one step when the moved window
 *   crosses it (from ≤ active < to  => active - 1; to ≤ active < from  =>
 *   active + 1); otherwise unchanged.
 *
 * Returns `true` iff the order was actually changed.
 */
export function reorderMuxWindows(
  ctx: MuxWindowManagerContext | MuxReorderContext,
  fromIndex: number,
  toIndex: number,
): boolean {
  const muxWindows = ctx.getMuxWindows();
  const muxPaneIds = ctx.getMuxPaneIds();
  const len = muxWindows.length;
  if (len === 0) return false;
  if (
    !Number.isInteger(fromIndex) ||
    !Number.isInteger(toIndex) ||
    fromIndex < 0 ||
    fromIndex >= len ||
    toIndex < 0 ||
    toIndex >= len
  ) {
    return false;
  }
  if (fromIndex === toIndex) return false;

  const winItem = muxWindows.splice(fromIndex, 1)[0]!;
  muxWindows.splice(toIndex, 0, winItem);
  const paneItem = muxPaneIds.splice(fromIndex, 1)[0]!;
  muxPaneIds.splice(toIndex, 0, paneItem);

  const active = ctx.getActiveMuxWindowIndex();
  let newActive = active;
  if (active === fromIndex) {
    newActive = toIndex;
  } else if (fromIndex < active && active <= toIndex) {
    newActive = active - 1;
  } else if (toIndex <= active && active < fromIndex) {
    newActive = active + 1;
  }
  if (newActive !== active) {
    ctx.setActiveMuxWindowIndex(newActive);
  }

  // Emit via whichever API the context exposes.
  if ("emitMuxStateChange" in ctx && typeof ctx.emitMuxStateChange === "function") {
    ctx.emitMuxStateChange();
  } else {
    emitMuxStateChange(ctx as MuxWindowManagerContext);
  }
  return true;
}

/**
 * Narrow context accepted by `reorderMuxWindows`. Declared separately so
 * both `MuxWindowManagerContext` and `MuxActionContext` (which both expose
 * these accessors) can be passed without requiring a shared base class.
 */
export interface MuxReorderContext {
  getMuxWindows: () => { id: number; name: string }[];
  getMuxPaneIds: () => number[];
  getActiveMuxWindowIndex: () => number;
  setActiveMuxWindowIndex: (index: number) => void;
  emitMuxStateChange?: () => void;
  onMuxStateChange?: ((info: {
    windowCount: number;
    activeWindow: number;
    windowNames: string[];
  }) => void) | null;
}

/** Notify listeners of mux window state changes. */
export function emitMuxStateChange(ctx: MuxWindowManagerContext): void {
  const muxWindows = ctx.getMuxWindows();
  const activeIdx = ctx.getActiveMuxWindowIndex();
  const activeId = muxWindows[activeIdx]?.id ?? -1;
  console.warn(
    `[DIAG-MUX-STATE] emit windowCount=${muxWindows.length} active=${activeIdx} activeId=${activeId}`,
  );
  ctx.onMuxStateChange?.({
    windowCount: muxWindows.length,
    activeWindow: activeIdx,
    windowNames: muxWindows.map((w) => w.name),
  });
}

/** Re-apply mux keybind settings (call when settings change at runtime). */
export function reloadMuxSettings(ctx: MuxWindowManagerContext): void {
  if (!ctx.getInMuxMode() || !ctx.getKeyboardHandler()) return;
  const muxSettings = SettingsService.getCached()?.mux;
  if (muxSettings) {
    ctx.getKeyboardHandler()!.updateMuxSettings(
      muxSettings.prefix ?? "Ctrl+B",
      muxSettings.keybinds ?? {},
    );
  }
}

/** Handle a remote SwitchWindow notification (e.g., from CLI `emterm mux switch-window`).
 *  Finds the window containing the given paneId and switches to it.
 *  Does NOT send SwitchWindow back to daemon (unlike switchMuxWindow) to avoid feedback loops. */
export function handleRemoteSwitchWindow(ctx: MuxWindowManagerContext, paneId: number): void {
  const state = ctx.getState();
  if (!state) return;

  const muxPaneIds = ctx.getMuxPaneIds();
  const muxPaneGrids = ctx.getMuxPaneGrids();
  const targetIndex = muxPaneIds.indexOf(paneId);
  if (targetIndex === -1) {
    muxLog.warn(`Remote SwitchWindow: pane ${paneId} not found in local windows`);
    return;
  }
  if (targetIndex === ctx.getActiveMuxWindowIndex()) {
    muxLog.debug(`Remote SwitchWindow: already on window ${targetIndex}`);
    return;
  }

  muxLog.info(`Remote SwitchWindow: switching to window ${targetIndex} (pane ${paneId})`);

  // Capture current terminal dimensions before save/restore — see
  // switchMuxWindow for rationale.
  const targetCols = state.cols;
  const targetRows = state.rows;

  // Save current pane's full state
  const previousIndex = ctx.getActiveMuxWindowIndex();
  const prevPaneId = muxPaneIds[previousIndex];
  if (prevPaneId != null) {
    // mux pane scroll baseline is outside the ITerminalRenderer contract;
    // the CanvasRenderer instance structurally satisfies ScrollStateTarget.
    const prevRenderer = ctx.getRenderer() as unknown as ScrollStateTarget | null;
    const snapshot = state.saveMuxPaneState(prevRenderer ?? undefined);
    muxPaneGrids.set(prevPaneId, snapshot);
    logGridSnapshot("save", prevPaneId, snapshot, "handleRemoteSwitchWindow");
    // Clear callbacks on saved grids to prevent OSC leaking from inactive panes
    snapshot.primaryGrid.core.clear_callbacks();
    snapshot.alternateGrid?.core.clear_callbacks();
  }

  // Discard any buffered PTY data from the previous pane
  ctx.flushPtyPendingData();

  ctx.setActiveMuxWindowIndex(targetIndex);

  // Restore the target pane's state
  const savedState = muxPaneGrids.get(paneId);
  if (savedState) {
    muxPaneGrids.delete(paneId);
    logGridSnapshot("restore", paneId, savedState, "handleRemoteSwitchWindow");
    state.restoreMuxPaneState(savedState);
    ctx.registerCoreCallbacks(state.getActiveCore());
    // mux pane scroll baseline is outside the ITerminalRenderer contract;
    // the CanvasRenderer instance structurally satisfies ScrollStateTarget.
    const restoreRenderer = ctx.getRenderer() as unknown as ScrollStateTarget | null;
    if (restoreRenderer) applyScrollState(savedState, restoreRenderer);
  } else {
    const branchActivePtr = corePtrOf(state.getActiveCore());
    console.warn(
      `[DIAG-MUX-GRID] freshBranch` +
      ` | paneId=${paneId}` +
      ` | activeCorePtrBefore=${fmtPtr(branchActivePtr)}` +
      ` | callsite=handleRemoteSwitchWindow` +
      ` | reason=noSavedState`,
    );
    // Allocate a fresh per-pane WasmGrid (see switchMuxWindow's matching
    // branch for the rationale — .reset() shares the core across panes).
    createFreshMuxGrid(ctx);
    const windows = ctx.getMuxWindows();
    state._title = windows[ctx.getActiveMuxWindowIndex()]?.name ?? "";
    state._iconName = "";
  }

  // Reconcile restored grid dimensions with the current terminal size.
  reconcileActivePaneSize(state, targetCols, targetRows);

  // Sync the title dedup cache and parent tab title to the restored pane.
  ctx.syncWindowTitleFromState();

  // Skip sendMuxControl(SwitchWindow) — the daemon already knows

  // Reconcile daemon-side PTY dimensions with the current terminal size.
  // Same rationale as switchMuxWindow: the target pane may have stale
  // dimensions if reattach initialized it before the status bar was restored.
  sendMuxPaneResize(ctx, paneId);

  // Request on-demand screen snapshot — same rationale as switchMuxWindow.
  requestPaneSnapshot(ctx, paneId);

  const renderer = ctx.getRenderer();
  if (renderer) {
    renderer.forceRender(state);
  }
  emitMuxStateChange(ctx);
}

/** Start or attach to mux session via inband protocol.
 *  Launches bridge process in the PTY and communicates via APC. */
export async function startMuxDirect(ctx: MuxWindowManagerContext): Promise<void> {
  if (ctx.getInMuxMode()) return;
  // enterMuxMode launches the bridge process and waits for Welcome APC
  await ctx.enterMuxMode("", 0);
}
