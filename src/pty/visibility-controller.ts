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
}

export class VisibilityController {
  private deps: VisibilityControllerDeps;
  private setTimeoutFn: typeof setTimeout;
  private clearTimeoutFn: typeof clearTimeout;
  private setIntervalFn: typeof setInterval;
  private clearIntervalFn: typeof clearInterval;
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
      this.resendCurrent();
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
  }

  /** Test helper: returns the last confirmed state pushed to the backend. */
  getLastNotified(): boolean | null {
    return this.lastNotified;
  }

  private currentEffective(): boolean {
    return this.deps.getDocumentVisible() && this.focused;
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
  }

  private logTransition(visible: boolean): void {
    const nowIso = new Date().toISOString();
    const perfNow = typeof performance !== "undefined" ? performance.now() : Date.now();
    if (visible) {
      const hiddenForMs =
        this.hiddenSincePerfMs !== null
          ? Math.round(perfNow - this.hiddenSincePerfMs)
          : -1;
      this.hiddenSincePerfMs = null;
      console.warn(
        `[WARN][FRONTEND] [DIAG-IDLE] visibility→visible at ${nowIso} | hiddenForMs=${hiddenForMs}`,
      );
    } else {
      this.hiddenSincePerfMs = perfNow;
      console.warn(`[WARN][FRONTEND] [DIAG-IDLE] visibility→hidden at ${nowIso}`);
    }
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
