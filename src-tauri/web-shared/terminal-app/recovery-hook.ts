/**
 * Post-recovery coordination after a WASM crash recovery.
 *
 * Two responsibilities, both run after the PTY handler finishes its
 * recovery-or-reinit cycle:
 *
 * 1. **Restore visible content** that a fresh empty WASM grid cannot
 *    reproduce on its own:
 *    - viaReinit=true: drop all saved mux pane/grid snapshots — they
 *      reference the dead module's memory.
 *    - In any mux mode: ask the daemon to resend the active pane's
 *      shadow screen so the user sees real content instead of a blank
 *      buffer. Non-mux relies on the shell to redraw on next keypress.
 *
 * 2. **Probe mux IPC liveness** (only after a viaReinit recovery in mux
 *    mode). Sends `RequestStatusUpdate` and waits for the matching
 *    `StatusUpdate` reply with race + grace semantics that tolerate
 *    in-flight replies that lose to a tight setTimeout (the original
 *    bug: a 2 ms-late alive reply was misclassified as a dead bridge).
 *    On confirmed timeout we exit mux mode so the user can relaunch
 *    `emterm mux`.
 *
 * Extracted from TerminalApp. Uses an explicit context object rather
 * than `this` so the recovery state can later migrate into a dedicated
 * controller without rewriting these functions.
 */

import type { TerminalState } from "../terminal/state";
import type { MuxClient } from "../terminal/mux/mux-client";

/**
 * State + accessors the recovery hook needs from TerminalApp.
 *
 * Reads (`get*`) and writes (`set*`) are split so the hook never holds
 * a reference to the host object — it only sees the live values it
 * asked for.
 */
export interface RecoveryHookContext {
  // Mux state
  getState(): TerminalState | null;
  getInMuxMode(): boolean;
  getMuxClient(): MuxClient | null;
  getActiveMuxPaneId(): number | null;
  getActiveMuxWindowIndex(): number;
  getMuxPaneIds(): readonly number[];
  /** Snapshots saved per-pane (cleared on viaReinit). */
  getMuxPaneGrids(): { clear(): void; size: number };
  /** Snapshots saved across detach/reattach (cleared on viaReinit). */
  getMuxDetachedGrids(): { clear(): void; size: number };

  // Snapshot-wait observability (used by mux-session.ts to log the
  // arrival of the snapshot reply on the PtyOutput callback).
  getSnapshotWaitPaneId(): number | null;
  setSnapshotWaitPaneId(paneId: number | null): void;
  getSnapshotWaitSetAt(): number;
  setSnapshotWaitSetAt(perfNow: number): void;

  // Post-recovery watch window (used by mux-session.ts to count
  // PtyOutput chunks during the watch).
  getPostRecoveryWatchUntil(): number;
  setPostRecoveryWatchUntil(deadlineMs: number): void;
  resetPostRecoveryCounters(): void;
  getPostRecoveryPtyOutputChunks(): number;
  getPostRecoveryPtyOutputBytes(): number;

  // Status update wrapper (so we can detect alive replies).
  getMuxStatusUpdateCallback(): ((msg: { left: string; right: string }) => void) | null;
  setMuxStatusUpdateCallback(cb: ((msg: { left: string; right: string }) => void) | null): void;

  // Mux teardown — invoked when the IPC probe times out.
  exitMuxMode(): void;
}

/**
 * Post-recovery hook: restore visible content that a fresh empty WASM grid
 * cannot reproduce on its own.
 *
 * - `viaReinit=true`: the WASM module was replaced, so every saved
 *   `MuxPaneGridState` / detached snapshot references dead memory. Drop
 *   them so subsequent `switchMuxWindow` takes the "no saved state"
 *   branch (fresh grid + daemon snapshot) instead of crashing during
 *   `restoreMuxPaneState`.
 * - In any mux mode, ask the daemon to resend the active pane's shadow
 *   screen so the user sees real content instead of a blank buffer.
 * - Non-mux has no backend buffer — the shell redraws on the next keypress.
 */
export function onWasmRecovered(ctx: RecoveryHookContext, viaReinit: boolean): void {
  const inMux = ctx.getInMuxMode();
  const muxClient = ctx.getMuxClient();
  const activePaneIdPre = inMux ? ctx.getActiveMuxPaneId() : null;
  console.warn(
    `[WARN][FRONTEND] [DIAG-RECOVERY] onWasmRecovered entry | viaReinit=${viaReinit} inMuxMode=${inMux} muxClient=${!!muxClient} activePaneId=${activePaneIdPre} activeMuxWindowIndex=${ctx.getActiveMuxWindowIndex()} muxPaneGrids=${ctx.getMuxPaneGrids().size} muxDetached=${ctx.getMuxDetachedGrids().size}`,
  );
  if (viaReinit) {
    ctx.getMuxPaneGrids().clear();
    ctx.getMuxDetachedGrids().clear();
    console.warn(
      `[WARN][FRONTEND] [DIAG-RECOVERY] onWasmRecovered cleared stale refs | muxPaneGrids=0 muxDetached=0`,
    );
  }
  if (!inMux || !muxClient) {
    console.warn(
      `[WARN][FRONTEND] [DIAG-RECOVERY] onWasmRecovered skip snapshot — inMuxMode=${inMux} muxClient=${!!muxClient}`,
    );
    return;
  }
  const paneId = ctx.getActiveMuxPaneId();
  if (paneId == null) {
    console.warn(
      `[WARN][FRONTEND] [DIAG-RECOVERY] onWasmRecovered skip snapshot — activePaneId=null activeMuxWindowIndex=${ctx.getActiveMuxWindowIndex()} muxPaneIds=[${ctx.getMuxPaneIds().join(",")}]`,
    );
    return;
  }
  console.warn(
    `[WARN][FRONTEND] [DIAG-RECOVERY] onWasmRecovered sending RequestPaneSnapshot | paneId=${paneId}`,
  );
  // Arm the snapshot-reply observer so the next PtyOutput chunk for this
  // pane gets logged once. Cleared in mux-session.ts on first match.
  const prevWaitPaneId = ctx.getSnapshotWaitPaneId();
  if (prevWaitPaneId != null) {
    console.warn(
      `[WARN][FRONTEND] [DIAG-RECOVERY] previous snapshot wait abandoned | prevPaneId=${prevWaitPaneId} newPaneId=${paneId} elapsedMs=${(performance.now() - ctx.getSnapshotWaitSetAt()).toFixed(0)}`,
    );
  }
  ctx.setSnapshotWaitPaneId(paneId);
  ctx.setSnapshotWaitSetAt(performance.now());
  muxClient.sendRequestPaneSnapshot(paneId).then(() => {
    console.warn(
      `[WARN][FRONTEND] [DIAG-RECOVERY] RequestPaneSnapshot sent | paneId=${paneId}`,
    );
    // After a WASM module reinit, the daemon snapshot reply is the only
    // way to repaint. Run a lightweight IPC health check to detect a dead
    // bridge socket (e.g. from a PC suspend that left the Unix socket in a
    // half-open state) and surface it instead of leaving a blank screen.
    if (viaReinit) {
      runPostRecoveryIpcHealthCheck(ctx).catch((healthErr: unknown) => {
        console.error(
          `[ERROR][FRONTEND] runPostRecoveryIpcHealthCheck threw: ${
            healthErr instanceof Error ? healthErr.message : String(healthErr)
          }`,
        );
      });
    }
  }).catch((err: unknown) => {
    console.warn(
      `[WARN][FRONTEND] sendRequestPaneSnapshot after WASM recovery failed: ${
        err instanceof Error ? err.message : String(err)
      }`,
    );
    // Disarm the wait observer on send failure so a stray PtyOutput chunk
    // for this pane doesn't get falsely attributed to a snapshot reply
    // that will never arrive.
    if (ctx.getSnapshotWaitPaneId() === paneId) {
      ctx.setSnapshotWaitPaneId(null);
      ctx.setSnapshotWaitSetAt(0);
    }
  });
}

/**
 * Post-recovery mux IPC health probe.
 *
 * Runs only after a viaReinit WASM recovery in mux mode. Sends a
 * `RequestStatusUpdate` and waits up to `HEALTH_CHECK_TIMEOUT_MS` for the
 * matching `StatusUpdate` reply. If nothing arrives, the bridge↔daemon
 * socket is presumably dead (typical after a long PC suspend on Linux),
 * so we exit mux mode to expose the host shell prompt — the user can
 * type `emterm mux` to relaunch the bridge and rebuild the connection.
 *
 * Implementation notes:
 * - Uses `Promise.race` between StatusUpdate arrival and timeout so
 *   in-flight replies short-circuit the wait instead of being judged
 *   against a 2 ms-precision setTimeout boundary (this caused a
 *   false-positive exit in production: alive reply arrived 2 ms after
 *   the 3 s timer fired).
 * - After timeout, applies a small grace window for late arrivals that
 *   raced the timer fire — recovers the session without exiting mux.
 * - Also opens an observability window during which incoming mux APC
 *   traffic is logged at warn level.
 */
export async function runPostRecoveryIpcHealthCheck(
  ctx: RecoveryHookContext,
): Promise<void> {
  const muxClient = ctx.getMuxClient();
  if (!muxClient || !ctx.getInMuxMode()) return;
  // Single-flight: if a check is already in flight, skip. Avoids
  // wrapper-chain corruption from overlapping recoveries.
  if (ctx.getPostRecoveryWatchUntil() > 0) return;

  const sessionMuxClient = muxClient;
  // 10s tolerates slow daemon replies after heavy WASM reinit work
  // (snapshot replay, large grid resize) which previously squeezed
  // the alive reply past the old 3 s threshold.
  const HEALTH_CHECK_TIMEOUT_MS = 10_000;
  // Grace window for replies that lost the race against the timeout
  // fire. Originally 200 ms (chosen to cover a 2 ms-class race), but
  // production logs showed a reply arriving 224 ms after the timeout
  // fire after the daemon had been silent for 4.5 h — heavy reinit
  // work plus suspended-bridge wake-up can stack hundreds of ms on top
  // of the 10 s timeout. 2000 ms keeps the user-visible exit path open
  // for the genuinely-dead case while absorbing realistic post-recovery
  // latency without false-positive mux teardown.
  const LATE_ARRIVAL_GRACE_MS = 2_000;
  // Watch window equals the maximum wait — `finally` clears
  // `postRecoveryWatchUntil` immediately when the await chain ends,
  // so any tail beyond timeout+grace is unobservable.
  const WATCH_WINDOW_MS = HEALTH_CHECK_TIMEOUT_MS + LATE_ARRIVAL_GRACE_MS;

  const watchOpenedAt = Date.now();
  ctx.setPostRecoveryWatchUntil(watchOpenedAt + WATCH_WINDOW_MS);
  ctx.resetPostRecoveryCounters();
  console.warn(
    `[WARN][FRONTEND] [DIAG-RECOVERY] post-recovery watch opened | windowMs=${WATCH_WINDOW_MS} timeoutMs=${HEALTH_CHECK_TIMEOUT_MS} graceMs=${LATE_ARRIVAL_GRACE_MS}`,
  );

  let statusReceived = false;
  let statusResolve: (() => void) | null = null;
  const statusPromise = new Promise<void>((resolve) => {
    statusResolve = resolve;
  });
  const originalCallback = ctx.getMuxStatusUpdateCallback();
  const wrapper = (msg: { left: string; right: string }) => {
    // If a session swap happened mid-flight (exit + re-enter), the new
    // session's StatusUpdate must NOT mark this probe as alive — that
    // would let a half-dead original session escape detection.
    if (ctx.getMuxClient() !== sessionMuxClient) {
      originalCallback?.(msg);
      return;
    }
    if (!statusReceived) {
      statusReceived = true;
      const elapsedMs = Date.now() - watchOpenedAt;
      console.warn(
        `[WARN][FRONTEND] [DIAG-RECOVERY] mux IPC alive — StatusUpdate received post-recovery | elapsedMs=${elapsedMs}`,
      );
      statusResolve?.();
    }
    originalCallback?.(msg);
  };
  ctx.setMuxStatusUpdateCallback(wrapper);

  // Restore the callback only if our wrapper is still the current one —
  // otherwise some other code (concurrent run, exit, external rewire)
  // has taken over the slot and we must not stomp on it.
  const restoreCallback = () => {
    if (ctx.getMuxStatusUpdateCallback() === wrapper) {
      ctx.setMuxStatusUpdateCallback(originalCallback);
    }
  };

  try {
    try {
      await muxClient.sendRequestStatusUpdate();
    } catch (err) {
      console.error(
        `[ERROR][FRONTEND] [DIAG-RECOVERY] post-recovery sendRequestStatusUpdate failed: ${
          err instanceof Error ? err.message : String(err)
        }`,
      );
      return;
    }

    // Race: alive reply short-circuits the timeout. This avoids the
    // 2 ms false-positive class entirely.
    await Promise.race([
      statusPromise,
      new Promise<void>((resolve) =>
        setTimeout(resolve, HEALTH_CHECK_TIMEOUT_MS),
      ),
    ]);

    // Bail out if mux mode was torn down or replaced during the wait —
    // a late StatusUpdate from a stale session must not trigger exit on
    // a freshly-attached session.
    if (!ctx.getInMuxMode() || ctx.getMuxClient() !== sessionMuxClient) return;

    if (statusReceived) return;

    // Timeout fired without a reply. Wait briefly for late arrivals
    // that raced the timer — exiting mux mode is destructive (forces
    // the user to manually `emterm mux`), so a small grace is well
    // worth a 200 ms delay.
    console.warn(
      `[WARN][FRONTEND] [DIAG-RECOVERY] no StatusUpdate within ${HEALTH_CHECK_TIMEOUT_MS}ms — entering ${LATE_ARRIVAL_GRACE_MS}ms grace window before exiting mux mode`,
    );

    await Promise.race([
      statusPromise,
      new Promise<void>((resolve) =>
        setTimeout(resolve, LATE_ARRIVAL_GRACE_MS),
      ),
    ]);

    if (!ctx.getInMuxMode() || ctx.getMuxClient() !== sessionMuxClient) return;

    if (statusReceived) {
      console.warn(
        `[WARN][FRONTEND] [DIAG-RECOVERY] mux IPC alive (late arrival in grace window) — keeping mux mode`,
      );
      return;
    }

    console.error(
      `[ERROR][FRONTEND] [DIAG-RECOVERY] mux IPC dead — no StatusUpdate within ${HEALTH_CHECK_TIMEOUT_MS}ms + ${LATE_ARRIVAL_GRACE_MS}ms grace after WASM recovery. Exiting mux mode so the user can relaunch the bridge.`,
    );
    ctx.exitMuxMode();
  } finally {
    restoreCallback();
    console.warn(
      `[WARN][FRONTEND] [DIAG-RECOVERY] post-recovery watch closed | chunks=${ctx.getPostRecoveryPtyOutputChunks()} bytes=${ctx.getPostRecoveryPtyOutputBytes()} statusReceived=${statusReceived}`,
    );
    ctx.setPostRecoveryWatchUntil(0);
  }
}
