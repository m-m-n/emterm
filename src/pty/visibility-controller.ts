/**
 * Visibility-aware PTY streaming controller.
 *
 * Watches `document.visibilityState` and Tauri `onFocusChanged` to compute
 * an effective-visible state. Hide is debounced so brief blur/focus
 * cycles do not pause/resume the backend stream. Show is immediate.
 *
 * The controller forwards confirmed state changes to:
 *   - PtyClient.setVisibility (always, for the local Tauri PTY backend)
 *   - MuxClient.sendSetVisibility (when in mux mode)
 *
 * A 10-second health-check resends the current confirmed state to recover
 * from any dropped notification.
 */

import type { PtyClient } from "./client";
import type { MuxClient } from "../terminal/mux/mux-client";

/** Debounce window for visible -> hidden. Visible is immediate. */
export const HIDE_DEBOUNCE_MS = 1000;

/** Idempotent re-send interval (NFR5). */
export const HEALTH_CHECK_MS = 10_000;

/**
 * Threshold above which the rAF heartbeat is considered dead.
 * If the elapsed time since the last rAF callback exceeds this
 * value at a health-check tick, `rafAlive` flips to false.
 */
export const RAF_DEAD_THRESHOLD_MS = 5_000;

/**
 * Tick gap above which a system suspend is suspected.
 * When two consecutive health-check ticks are separated by
 * more than this, dead detection is skipped for that tick and
 * `lastRafPerfMs` is reset to the current `now`.
 */
export const SUSPEND_GAP_MS = 30_000;

/** Tauri webview focus subscription type (subset we need). */
export type FocusUnsubscribe = () => void;
export type FocusSubscribe = (
  cb: (focused: boolean) => void,
) => Promise<FocusUnsubscribe>;

export interface VisibilityControllerDeps {
  /** Returns the current PtyClient (may be null between sessions). */
  getPtyClient: () => PtyClient | null;
  /** Returns the current MuxClient if attached, null otherwise. */
  getMuxClient: () => MuxClient | null;
  /** Returns `document.visibilityState === "visible"`. Injectable for tests. */
  getDocumentVisible: () => boolean;
  /** Subscribe to Tauri webview focus changes. Returns the unsubscribe fn. */
  subscribeFocus: FocusSubscribe;
  /** DOM target for `visibilitychange` listener (default: document). */
  visibilityTarget?: { addEventListener: typeof document.addEventListener; removeEventListener: typeof document.removeEventListener };
  /** Timer factories — overridable for unit tests with fake timers. */
  setTimeoutFn?: typeof setTimeout;
  clearTimeoutFn?: typeof clearTimeout;
  setIntervalFn?: typeof setInterval;
  clearIntervalFn?: typeof clearInterval;
  /**
   * Test injectable. Defaults to a lazy wrapper that resolves
   * `globalThis.requestAnimationFrame` per call so post-construction
   * monkey-patching (used by E2E) is observed.
   */
  requestAnimationFrameFn?: typeof requestAnimationFrame;
  /** Test injectable. Defaults to a lazy `globalThis.cancelAnimationFrame` wrapper. */
  cancelAnimationFrameFn?: typeof cancelAnimationFrame;
  /**
   * Test injectable. Returns monotonic ms; defaults to a lazy wrapper that
   * prefers `performance.now()` and falls back to `Date.now()`.
   */
  nowFn?: () => number;
}

export class VisibilityController {
  private deps: VisibilityControllerDeps;
  private setTimeoutFn: typeof setTimeout;
  private clearTimeoutFn: typeof clearTimeout;
  private setIntervalFn: typeof setInterval;
  private clearIntervalFn: typeof clearInterval;
  private requestAnimationFrameFn: typeof requestAnimationFrame;
  private cancelAnimationFrameFn: typeof cancelAnimationFrame;
  private nowFn: () => number;
  private visibilityTarget: NonNullable<VisibilityControllerDeps["visibilityTarget"]>;

  /** Last confirmed-and-notified effective state. null until first notify. */
  private lastNotified: boolean | null = null;
  /** Last assumed-by-listeners focus state. */
  private focused = true;
  /** Pending hide-debounce timer handle. */
  private hideTimer: ReturnType<typeof setTimeout> | null = null;
  private healthTimer: ReturnType<typeof setInterval> | null = null;
  private visibilityListener: (() => void) | null = null;
  private focusUnsubscribe: FocusUnsubscribe | null = null;
  private started = false;
  private destroyed = false;
  /** `performance.now()` timestamp of the most recent visible -> hidden notify. */
  private hiddenSincePerfMs: number | null = null;

  /**
   * rAF heartbeat liveness. Initial value true so the first effective
   * computation does not falsely classify the controller as hidden before
   * any rAF callback has fired.
   */
  private rafAlive = true;
  /** Monotonic ms of the most recent rAF callback; null until the first one fires. */
  private lastRafPerfMs: number | null = null;
  /** Monotonic ms of the most recent health-check tick; null until first tick. */
  private lastHealthTickPerfMs: number | null = null;
  /** Pending rAF request id (null when no rAF is in flight). */
  private rafHandle: number | null = null;

  constructor(deps: VisibilityControllerDeps) {
    this.deps = deps;
    // WebKit (WebKitGTK) refuses to call window timer methods when `this`
    // is not the Window itself, so when we fall back to the globals we
    // must bind them to globalThis. Test injectors are passed through
    // as-is.
    this.setTimeoutFn = deps.setTimeoutFn ?? setTimeout.bind(globalThis);
    this.clearTimeoutFn = deps.clearTimeoutFn ?? clearTimeout.bind(globalThis);
    this.setIntervalFn = deps.setIntervalFn ?? setInterval.bind(globalThis);
    this.clearIntervalFn = deps.clearIntervalFn ?? clearInterval.bind(globalThis);
    this.visibilityTarget = deps.visibilityTarget ?? document;

    // Lazy default wrappers for rAF / cancelAF / now. Unlike the timer
    // defaults above which bind once, these resolve `globalThis.<api>`
    // per call so E2E specs (and future runtime polyfills) that
    // monkey-patch the global property after construction are observed.
    this.requestAnimationFrameFn =
      deps.requestAnimationFrameFn ??
      (((cb: FrameRequestCallback) => {
        const fn = (globalThis as { requestAnimationFrame?: typeof requestAnimationFrame })
          .requestAnimationFrame;
        if (typeof fn !== "function") {
          // Degraded: rAF unavailable in this environment. Return a sentinel
          // handle; cancelAnimationFrame wrapper below is also a no-op.
          return 0 as number;
        }
        return fn(cb);
      }) as typeof requestAnimationFrame);

    this.cancelAnimationFrameFn =
      deps.cancelAnimationFrameFn ??
      (((handle: number) => {
        const fn = (globalThis as { cancelAnimationFrame?: typeof cancelAnimationFrame })
          .cancelAnimationFrame;
        if (typeof fn !== "function") return;
        fn(handle);
      }) as typeof cancelAnimationFrame);

    this.nowFn =
      deps.nowFn ??
      (() => {
        const perf = (globalThis as { performance?: { now?: () => number } }).performance;
        if (perf && typeof perf.now === "function") return perf.now();
        return Date.now();
      });
  }

  /** Begin observing visibility. Idempotent. */
  async start(): Promise<void> {
    if (this.started || this.destroyed) return;
    this.started = true;

    this.visibilityListener = () => this.onSignalChanged();
    this.visibilityTarget.addEventListener("visibilitychange", this.visibilityListener);

    try {
      const unsub = await this.deps.subscribeFocus((focused) => {
        if (this.destroyed) return;
        this.focused = focused;
        this.onSignalChanged();
      });
      // RACE GUARD: stop() may have run while subscribeFocus was awaiting.
      // In that case unsubscribe immediately and skip the post-await setup so
      // the health timer is never created against a torn-down controller.
      if (this.destroyed) {
        try {
          unsub();
        } catch {
          /* ignore */
        }
        return;
      }
      this.focusUnsubscribe = unsub;
    } catch (err) {
      console.warn("[WARN][FRONTEND] VisibilityController: focus subscribe failed:", err);
      if (this.destroyed) return;
    }

    if (this.destroyed) return;

    // Push the current effective state once so the backend is in sync from the
    // start (rather than implicitly assuming visible).
    this.evaluate();

    this.healthTimer = this.setIntervalFn(() => {
      this.healthTick();
    }, HEALTH_CHECK_MS);
  }

  /** Stop observing. Safe to call multiple times. */
  stop(): void {
    // Set destroyed first so any in-flight start() awaits exit early before
    // creating timers or storing the focus unsubscribe handle.
    this.destroyed = true;
    this.started = false;
    if (this.visibilityListener) {
      this.visibilityTarget.removeEventListener("visibilitychange", this.visibilityListener);
      this.visibilityListener = null;
    }
    if (this.focusUnsubscribe) {
      try {
        this.focusUnsubscribe();
      } catch {
        /* ignore */
      }
      this.focusUnsubscribe = null;
    }
    if (this.hideTimer !== null) {
      this.clearTimeoutFn(this.hideTimer);
      this.hideTimer = null;
    }
    if (this.healthTimer !== null) {
      this.clearIntervalFn(this.healthTimer);
      this.healthTimer = null;
    }
    this.cancelRaf();
  }

  /** Test helper: returns the last confirmed state pushed to the backend. */
  getLastNotified(): boolean | null {
    return this.lastNotified;
  }

  private currentEffective(): boolean {
    return this.deps.getDocumentVisible() && this.focused && this.rafAlive;
  }

  private onSignalChanged(): void {
    this.evaluate();
  }

  private evaluate(): void {
    const next = this.currentEffective();
    if (next) {
      // Cancel any pending hide debounce; show is immediate.
      if (this.hideTimer !== null) {
        this.clearTimeoutFn(this.hideTimer);
        this.hideTimer = null;
      }
      this.notify(true);
    } else {
      // Hidden candidate. If a hide timer is already in flight, leave it.
      if (this.hideTimer !== null) return;
      this.hideTimer = this.setTimeoutFn(() => {
        this.hideTimer = null;
        // Re-check: maybe focus came back during the debounce.
        if (!this.currentEffective()) {
          this.notify(false);
        }
      }, HIDE_DEBOUNCE_MS);
    }
  }

  private notify(visible: boolean): void {
    if (this.lastNotified === visible) return;
    this.lastNotified = visible;
    this.logTransition(visible);
    this.dispatch(visible);
    if (visible) {
      this.scheduleRaf();
    } else {
      this.cancelRaf();
    }
  }

  /**
   * Schedule the next rAF callback. Two scheduling modes:
   *   (a) Hot loop: `lastNotified === true` — the normal effective-visible
   *       self-loop that refreshes `lastRafPerfMs` every frame.
   *   (b) Recovery probe: `lastNotified === false` AND `!rafAlive` AND
   *       document/focus indicate visible-eligible. Driven by `healthTick`,
   *       this single-shot probe detects rAF resumption WITHOUT depending
   *       on `cancelAnimationFrame` failing to cancel a queued callback
   *       (which is non-portable browser behavior).
   * No-op if a request is already in flight or the controller is torn down.
   */
  private scheduleRaf(): void {
    if (this.destroyed) return;
    if (this.rafHandle !== null) return;
    const inHotLoop = this.lastNotified === true;
    const inRecoveryProbe =
      this.lastNotified === false &&
      !this.rafAlive &&
      this.deps.getDocumentVisible() &&
      this.focused;
    if (!inHotLoop && !inRecoveryProbe) return;

    let handle: number;
    try {
      handle = this.requestAnimationFrameFn(() => {
        // Defense against late-delivered queued callbacks after stop().
        // WebKit may deliver queued rAF cbs even after cancelAnimationFrame
        // — without this guard a stale cb could mutate state and dispatch
        // a phantom notify(true) on an already-destroyed controller.
        if (this.destroyed) return;
        this.rafHandle = null;
        const now = this.nowFn();
        this.lastRafPerfMs = now;
        if (!this.rafAlive) {
          this.rafAlive = true;
          this.evaluate();
        }
        // Re-schedule only while still effective-visible and alive.
        if (this.lastNotified === true && !this.destroyed) {
          this.scheduleRaf();
        }
      });
    } catch (err) {
      console.warn("[WARN][FRONTEND] VisibilityController: requestAnimationFrame failed:", err);
      return;
    }
    this.rafHandle = handle;
  }

  /** Cancel any in-flight rAF request. Best-effort; swallows errors. */
  private cancelRaf(): void {
    if (this.rafHandle === null) return;
    const handle = this.rafHandle;
    this.rafHandle = null;
    try {
      this.cancelAnimationFrameFn(handle);
    } catch {
      /* ignore */
    }
  }

  /**
   * Health-check tick body: resend current state, then suspend-gap and
   * rAF-dead detection. Skipped early stages: first tick (records
   * baseline), suspend gap (>30 s, resets baseline), grace period
   * (`lastRafPerfMs===null`).
   */
  private healthTick(): void {
    this.resendCurrent();

    const now = this.nowFn();
    if (this.lastHealthTickPerfMs !== null) {
      const gap = now - this.lastHealthTickPerfMs;
      if (gap > SUSPEND_GAP_MS) {
        // Suspend suspected: reset baseline, skip detection this tick.
        this.lastHealthTickPerfMs = now;
        this.lastRafPerfMs = now;
        return;
      }
    }
    this.lastHealthTickPerfMs = now;

    // Recovery probe: when hidden purely due to rAF stall (document and
    // focus indicate visible-eligible), schedule a one-shot rAF so we can
    // detect rAF resumption. This path is independent of
    // `cancelAnimationFrame` semantics — it never relies on a canceled
    // callback being delivered later. scheduleRaf's own guards keep the
    // probe single-flight (rafHandle !== null short-circuits).
    if (
      this.lastNotified === false &&
      !this.rafAlive &&
      this.deps.getDocumentVisible() &&
      this.focused
    ) {
      this.scheduleRaf();
      return;
    }

    // Only meaningful while controller has dispatched a visible state.
    if (this.lastNotified !== true) return;
    // Grace period: no rAF callback has ever fired yet.
    if (this.lastRafPerfMs === null) return;

    const sinceRaf = now - this.lastRafPerfMs;
    if (sinceRaf > RAF_DEAD_THRESHOLD_MS) {
      if (this.rafAlive) {
        this.rafAlive = false;
        this.evaluate();
      }
    }
  }

  private logTransition(visible: boolean): void {
    const nowIso = new Date().toISOString();
    const perfNow = typeof performance !== "undefined" ? performance.now() : Date.now();
    if (visible) {
      if (this.hiddenSincePerfMs !== null) {
        const hiddenForMs = Math.round(perfNow - this.hiddenSincePerfMs);
        this.hiddenSincePerfMs = null;
        console.warn(
          `[WARN][FRONTEND] [DIAG-IDLE] visibility→visible at ${nowIso} | hiddenForMs=${hiddenForMs}`,
        );
      } else {
        // No prior hidden was recorded — initial dispatch right after start(),
        // typically during fresh app startup. Emit explicit (initial) marker
        // instead of the previous misleading hiddenForMs=-1 sentinel.
        console.warn(
          `[WARN][FRONTEND] [DIAG-IDLE] visibility→visible (initial) at ${nowIso}`,
        );
      }
    } else {
      this.hiddenSincePerfMs = perfNow;
      console.warn(
        `[WARN][FRONTEND] [DIAG-IDLE] visibility→hidden at ${nowIso} | reason=${this.hiddenReason()}`,
      );
    }
  }

  /**
   * Compose a human-readable cause string for the most recent
   * hidden notification. Joins all currently-active causes with `+`.
   * Returns `unknown` defensively if no signal is currently false.
   */
  private hiddenReason(): string {
    const causes: string[] = [];
    if (!this.deps.getDocumentVisible()) causes.push("document");
    if (!this.focused) causes.push("focus");
    if (!this.rafAlive) causes.push("raf-stall");
    return causes.length === 0 ? "unknown" : causes.join("+");
  }

  private resendCurrent(): void {
    if (this.lastNotified === null) return;
    this.dispatch(this.lastNotified);
  }

  private dispatch(visible: boolean): void {
    const pty = this.deps.getPtyClient();
    if (pty) {
      pty.setVisibility(visible).catch((err: unknown) => {
        console.warn("[WARN][FRONTEND] VisibilityController: pty setVisibility failed:", err);
      });
    }
    const mux = this.deps.getMuxClient();
    if (mux && mux.isConnected) {
      mux.sendSetVisibility(visible).catch((err: unknown) => {
        console.warn("[WARN][FRONTEND] VisibilityController: mux sendSetVisibility failed:", err);
      });
    }
  }
}
