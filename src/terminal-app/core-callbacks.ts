/**
 * Register WASM-side parser callbacks (OSC, APC, DCS, BEL, device-response)
 * on a TerminalCore instance.
 *
 * Called once for the primary core during init and again when the alternate
 * core becomes active in mux mode (so each core gets its own callbacks
 * pointing to the live host fields).
 *
 * The callback bodies must NOT access the core they were registered on:
 * doing so re-enters WASM during `process_pty_data` and causes a recursive
 * borrow error. OSC and APC/DCS payloads are therefore queued on the host
 * for processing once `process_pty_data` returns and the borrow is released.
 *
 * Extracted from TerminalApp so the core-callback wiring lives next to the
 * other parser-related handlers rather than buried in the orchestrator.
 */

import type { TerminalState } from "../terminal/state";
import type { PtyClient } from "../pty/client";
import type { ImageHandler } from "./handlers/image";

type Core = ReturnType<TerminalState["getActiveCore"]>;

/**
 * Read-only access the callbacks need from the host TerminalApp.
 *
 * Everything is a getter so the callback always reads the live value
 * (e.g. `imageHandler` may be null briefly during init).
 */
export interface CoreCallbacksContext {
  getState(): TerminalState | null;
  getPtyClient(): PtyClient | null;
  getImageHandler(): ImageHandler | null;
  /** Append an OSC event for deferred processing after process_pty_data returns. */
  enqueueOsc(actionType: number, data: string): void;
}

/**
 * Wire the OSC, APC, DCS, BEL, and device-response callbacks for `core`.
 */
export function registerCoreCallbacks(core: Core, ctx: CoreCallbacksContext): void {
  core.set_osc_callback((actionType: number, data: string) => {
    // Queue data - do NOT access core here (recursive borrow error)
    // OSC 133 (SemanticPrompt) and OSC 777 (EmtermExtension) call
    // getScrollbackLength() which re-enters WASM during process_pty_data.
    ctx.enqueueOsc(actionType, data);
  });

  core.set_apc_callback((data: Uint8Array) => {
    // Queue data - do NOT access core here (recursive borrow error)
    ctx.getImageHandler()?.queueApc(data);
  });

  core.set_dcs_callback((data: Uint8Array) => {
    // Queue data - do NOT access core here (recursive borrow error)
    ctx.getImageHandler()?.queueDcs(data);
  });

  core.set_bell_callback(() => {
    ctx.getState()?.onBell?.();
  });

  core.set_device_response_callback((data: Uint8Array) => {
    // Skip Kitty Graphics Protocol APC responses (ESC _ G ...).
    // These are handled by the PTY reader thread's KittyScanner which
    // writes directly to the master fd for zero-latency delivery.
    if (data.length >= 3 && data[0] === 0x1b && data[1] === 0x5f && data[2] === 0x47) {
      return;
    }
    ctx.getPtyClient()?.write(data);
  });
}
