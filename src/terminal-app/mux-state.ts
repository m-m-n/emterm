/**
 * Mux state container + context builders for TerminalApp.
 *
 * The mux subsystem is implemented across three sibling modules
 * (`mux/mux-window-manager.ts`, `mux/mux-action-handler.ts`,
 * `mux/mux-session.ts`) which all consume their own context-object
 * shape. TerminalApp used to hand-build each context inline, dragging
 * a dozen mux-only fields into the orchestrator class.
 *
 * This module:
 * - Defines `MuxState`, the bag of fields that live on TerminalApp but
 *   are only used by mux helpers, with sensible defaults for fresh
 *   instances.
 * - Provides three pure builders that take a `MuxStateAccess` interface
 *   (the host's mutable accessors) plus a small set of TerminalApp
 *   collaborators and return the existing context shapes unchanged.
 *
 * Behaviour is unchanged versus the inline builders. The only purpose
 * of this extraction is to slim down `index.ts` and group related
 * fields/builders so future mux work has one place to look.
 */

import type { TerminalState } from "../terminal/state";
import type { ITerminalRenderer } from "../terminal";
import type { PtyClient } from "../pty";
import type { WasmGrid } from "../terminal/wasm/terminal-core";
import type { KeyboardHandler } from "./handlers/keyboard";
import type { ImageHandler } from "./handlers/image";
import type { MuxClient, MuxWindowInfo } from "../terminal/mux/mux-client";
import type { MuxAction } from "../terminal/mux/prefix-key";
import type { MuxPaneGridState } from "../terminal/state";
import type { PtyHandlerHandle } from "./pty-handler";
import type { MuxWindowManagerContext } from "./mux/mux-window-manager";
import type { MuxActionContext } from "./mux/mux-action-handler";
import type { MuxSessionContext, EnterMuxOptions } from "./mux/mux-session";

/**
 * The mux-only fields TerminalApp owns. Bundled into one object so
 * future work can promote it to a dedicated `MuxStateController`
 * without touching every accessor.
 */
export interface MuxState {
  inMuxMode: boolean;
  muxWindows: { id: number; name: string }[];
  activeMuxWindowIndex: number;
  muxPaneIds: number[];
  muxPendingWindowCount: number;
  muxIsReattaching: boolean;
  muxReattachWindows: MuxWindowInfo[];
  muxPaneGrids: Map<number, MuxPaneGridState>;
  muxOriginalGrid: WasmGrid | null;
  muxDetachedGrids: Map<string, Uint8Array>;
  muxLastActiveIndex: number;

  // Post-recovery observability — read by mux-session.ts.
  postRecoveryWatchUntil: number;
  postRecoveryPtyOutputChunks: number;
  postRecoveryPtyOutputBytes: number;
  snapshotWaitPaneId: number | null;
  snapshotWaitSetAt: number;

  muxClient: MuxClient | null;
}

/** Initial values for a freshly constructed TerminalApp. */
export function createInitialMuxState(): MuxState {
  return {
    inMuxMode: false,
    muxWindows: [],
    activeMuxWindowIndex: 0,
    muxPaneIds: [],
    muxPendingWindowCount: 0,
    muxIsReattaching: false,
    muxReattachWindows: [],
    muxPaneGrids: new Map(),
    muxOriginalGrid: null,
    muxDetachedGrids: new Map(),
    muxLastActiveIndex: 0,
    postRecoveryWatchUntil: 0,
    postRecoveryPtyOutputChunks: 0,
    postRecoveryPtyOutputBytes: 0,
    snapshotWaitPaneId: null,
    snapshotWaitSetAt: 0,
    muxClient: null,
  };
}

/**
 * Mutable access into TerminalApp's mux state, plus the small set of
 * cross-system collaborators (state/renderer/ptyClient/...) the mux
 * helpers also need. Implemented as getters/setters so context objects
 * always read live values even after recovery rotates fields.
 */
export interface MuxStateAccess {
  // Cross-system collaborators
  getState(): TerminalState | null;
  getRenderer(): ITerminalRenderer | null;
  getPtyClient(): PtyClient | null;
  getKeyboardHandler(): KeyboardHandler | null;
  getImageHandler(): ImageHandler | null;
  getPtyHandlerHandle(): PtyHandlerHandle | null;

  // Mux-state field accessors
  getInMuxMode(): boolean;
  setInMuxMode(value: boolean): void;
  getMuxClient(): MuxClient | null;
  setMuxClient(client: MuxClient | null): void;
  getMuxWindows(): { id: number; name: string }[];
  setMuxWindows(windows: { id: number; name: string }[]): void;
  getActiveMuxWindowIndex(): number;
  setActiveMuxWindowIndex(index: number): void;
  getMuxPaneIds(): number[];
  setMuxPaneIds(ids: number[]): void;
  getMuxPendingWindowCount(): number;
  setMuxPendingWindowCount(count: number): void;
  getMuxIsReattaching(): boolean;
  setMuxIsReattaching(value: boolean): void;
  getMuxOriginalGrid(): WasmGrid | null;
  setMuxOriginalGrid(grid: WasmGrid | null): void;
  getMuxPaneGrids(): Map<number, MuxPaneGridState>;
  getMuxDetachedGrids(): Map<string, Uint8Array>;
  getMuxLastActiveIndex(): number;
  setMuxLastActiveIndex(index: number): void;
  setMuxReattachWindows(windows: MuxWindowInfo[]): void;
  getMuxReattachWindows(): MuxWindowInfo[];

  // Post-recovery observability
  getPostRecoveryWatchUntil(): number;
  countPostRecoveryPtyOutput(bytes: number): void;
  getSnapshotWaitPaneId(): number | null;
  setSnapshotWaitPaneId(paneId: number | null): void;
  getSnapshotWaitSetAt(): number;

  // Status callback (used by recovery probe wrapping)
  onStatusUpdate(msg: { left: string; right: string }): void;

  // Cross-controller hooks
  registerCoreCallbacks(core: ReturnType<TerminalState["getActiveCore"]>): void;
  handleMuxPaneCreated(paneId: number): void;
  handleMuxPaneExited(paneId: number): void;
  handleRemoteSwitchWindow(paneId: number): void;
  handleMuxAction(action: MuxAction): void;
  sendMuxControl(msgType: number, paneId: number, payload?: Uint8Array): void;
  getActiveMuxPaneId(): number | null;
  emitMuxStateChange(): void;
  switchMuxWindow(previousIndex?: number): void;
  exitMuxMode(): void;
  enterMuxMode(socketPath: string, sessionId: number, options?: EnterMuxOptions): Promise<void>;
  onMuxModeExited(): void;
  syncWindowTitleFromState(): void;

  // mux-window-manager observation hook
  getOnMuxStateChange(): ((info: {
    windowCount: number;
    activeWindow: number;
    windowNames: string[];
  }) => void) | null;
}

export function buildMuxSessionContext(access: MuxStateAccess): MuxSessionContext {
  return {
    getState: () => access.getState(),
    getRenderer: () => access.getRenderer(),
    getPtyClient: () => access.getPtyClient(),
    getKeyboardHandler: () => access.getKeyboardHandler(),
    getPtyHandlerHandle: () => access.getPtyHandlerHandle(),
    getInMuxMode: () => access.getInMuxMode(),
    setInMuxMode: (value) => access.setInMuxMode(value),
    getMuxClient: () => access.getMuxClient(),
    setMuxClient: (client) => access.setMuxClient(client),
    getMuxWindows: () => access.getMuxWindows(),
    setMuxWindows: (windows) => access.setMuxWindows(windows),
    getActiveMuxWindowIndex: () => access.getActiveMuxWindowIndex(),
    setActiveMuxWindowIndex: (index) => access.setActiveMuxWindowIndex(index),
    getMuxPaneIds: () => access.getMuxPaneIds(),
    setMuxPaneIds: (ids) => access.setMuxPaneIds(ids),
    getMuxPendingWindowCount: () => access.getMuxPendingWindowCount(),
    setMuxPendingWindowCount: (count) => access.setMuxPendingWindowCount(count),
    getMuxIsReattaching: () => access.getMuxIsReattaching(),
    setMuxIsReattaching: (value) => access.setMuxIsReattaching(value),
    getMuxOriginalGrid: () => access.getMuxOriginalGrid(),
    setMuxOriginalGrid: (grid) => access.setMuxOriginalGrid(grid),
    getMuxPaneGrids: () => access.getMuxPaneGrids(),
    getMuxLastActiveIndex: () => access.getMuxLastActiveIndex(),
    setMuxLastActiveIndex: (index) => access.setMuxLastActiveIndex(index),
    setMuxReattachWindows: (windows) => access.setMuxReattachWindows(windows),
    setMuxApcContext: (ctx) => access.getImageHandler()?.setMuxApcContext(ctx),
    registerCoreCallbacks: (core) => access.registerCoreCallbacks(core),
    handleMuxPaneCreated: (paneId) => access.handleMuxPaneCreated(paneId),
    handleMuxPaneExited: (paneId) => access.handleMuxPaneExited(paneId),
    handleRemoteSwitchWindow: (paneId) => access.handleRemoteSwitchWindow(paneId),
    handleMuxAction: (action) => access.handleMuxAction(action),
    sendMuxControl: (msgType, paneId, payload) => access.sendMuxControl(msgType, paneId, payload),
    getActiveMuxPaneId: () => access.getActiveMuxPaneId(),
    emitMuxStateChange: () => access.emitMuxStateChange(),
    onMuxModeExited: () => access.onMuxModeExited(),
    onStatusUpdate: (msg) => access.onStatusUpdate(msg),
    getPostRecoveryWatchUntil: () => access.getPostRecoveryWatchUntil(),
    countPostRecoveryPtyOutput: (bytes) => access.countPostRecoveryPtyOutput(bytes),
    getSnapshotWaitPaneId: () => access.getSnapshotWaitPaneId(),
    setSnapshotWaitPaneId: (paneId) => access.setSnapshotWaitPaneId(paneId),
    getSnapshotWaitSetAt: () => access.getSnapshotWaitSetAt(),
  };
}

export function buildMuxWindowManagerContext(
  access: MuxStateAccess,
): MuxWindowManagerContext {
  return {
    getState: () => access.getState(),
    getRenderer: () => access.getRenderer(),
    getMuxClient: () => access.getMuxClient(),
    getKeyboardHandler: () => access.getKeyboardHandler(),
    getInMuxMode: () => access.getInMuxMode(),
    getMuxWindows: () => access.getMuxWindows(),
    getActiveMuxWindowIndex: () => access.getActiveMuxWindowIndex(),
    setActiveMuxWindowIndex: (index) => access.setActiveMuxWindowIndex(index),
    getMuxPaneIds: () => access.getMuxPaneIds(),
    getMuxPaneGrids: () => access.getMuxPaneGrids(),
    getMuxDetachedGrids: () => access.getMuxDetachedGrids(),
    getMuxPendingWindowCount: () => access.getMuxPendingWindowCount(),
    setMuxPendingWindowCount: (count) => access.setMuxPendingWindowCount(count),
    getMuxIsReattaching: () => access.getMuxIsReattaching(),
    setMuxIsReattaching: (value) => access.setMuxIsReattaching(value),
    getMuxLastActiveIndex: () => access.getMuxLastActiveIndex(),
    getMuxReattachWindows: () => access.getMuxReattachWindows(),
    get onMuxStateChange() { return access.getOnMuxStateChange(); },
    flushPtyPendingData: () => { access.getPtyHandlerHandle()?.flushPendingData(); },
    processPtyPendingDataNow: () => { access.getPtyHandlerHandle()?.processNow(); },
    registerCoreCallbacks: (core) => access.registerCoreCallbacks(core),
    sendMuxControl: (msgType, paneId, payload) => access.sendMuxControl(msgType, paneId, payload),
    exitMuxMode: () => access.exitMuxMode(),
    enterMuxMode: (socketPath, sessionId) => access.enterMuxMode(socketPath, sessionId),
    syncWindowTitleFromState: () => access.syncWindowTitleFromState(),
  };
}

export function buildMuxActionContext(access: MuxStateAccess): MuxActionContext {
  return {
    getMuxClient: () => access.getMuxClient(),
    getPtyClient: () => access.getPtyClient(),
    getMuxWindows: () => access.getMuxWindows(),
    getActiveMuxWindowIndex: () => access.getActiveMuxWindowIndex(),
    setActiveMuxWindowIndex: (index) => access.setActiveMuxWindowIndex(index),
    getMuxPaneIds: () => access.getMuxPaneIds(),
    getMuxPendingWindowCount: () => access.getMuxPendingWindowCount(),
    setMuxPendingWindowCount: (count) => access.setMuxPendingWindowCount(count),
    switchMuxWindow: (previousIndex?) => access.switchMuxWindow(previousIndex),
    emitMuxStateChange: () => access.emitMuxStateChange(),
    exitMuxMode: () => access.exitMuxMode(),
  };
}
