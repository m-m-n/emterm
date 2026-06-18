/**
 * Tauri-invoke → wry IPC bridge (native-poc child settings window).
 *
 * The reused settings panel (`src/settings/`) talks to its backend through
 * `invoke()` from `@tauri-apps/api/core`, which at runtime is just
 * `window.__TAURI_INTERNALS__.invoke(cmd, args)`. The Wry child window has
 * no Tauri runtime, so this module installs a minimal `__TAURI_INTERNALS__`
 * whose `invoke` forwards each call over wry's IPC channel
 * (`window.ipc.postMessage`) and resolves when the Rust host replies via
 * `window.__EMTERM_SETTINGS_IPC__.resolve(id, ok, payload)`.
 *
 * Wire format (JS → Rust, one JSON object per postMessage):
 *   { id: number, cmd: string, args: object }
 * Reply (Rust → JS, via evaluate_script):
 *   window.__EMTERM_SETTINGS_IPC__.resolve(id, ok, payload)
 *
 * @module native-poc/settings/ipc-bridge
 */

interface PendingCall {
  resolve: (value: unknown) => void;
  reject: (reason: unknown) => void;
}

declare global {
  interface Window {
    /** wry-provided IPC channel (present inside the Wry WebView). */
    ipc?: { postMessage(message: string): void };
    /** Minimal stand-in for the Tauri runtime internals. */
    __TAURI_INTERNALS__?: {
      invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown>;
    };
    /** Reply entry point evaluated by the Rust host. */
    __EMTERM_SETTINGS_IPC__?: {
      resolve(id: number, ok: boolean, payload: unknown): void;
    };
  }
}

const pending = new Map<number, PendingCall>();
let nextId = 1;

/**
 * Install the bridge. Must run before the first `invoke()` call (i.e.
 * before the settings panel boots). Idempotent.
 */
export function installTauriInvokeBridge(): void {
  if (window.__EMTERM_SETTINGS_IPC__) return;

  window.__EMTERM_SETTINGS_IPC__ = {
    resolve(id: number, ok: boolean, payload: unknown): void {
      const call = pending.get(id);
      if (!call) {
        console.warn(
          `[WARN][FRONTEND] settings ipc: reply for unknown id ${id}`,
        );
        return;
      }
      pending.delete(id);
      if (ok) {
        call.resolve(payload);
      } else {
        call.reject(payload);
      }
    },
  };

  window.__TAURI_INTERNALS__ = {
    invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
      return new Promise((resolve, reject) => {
        const channel = window.ipc;
        if (!channel) {
          reject(new Error("settings ipc: window.ipc is unavailable"));
          return;
        }
        const id = nextId++;
        pending.set(id, { resolve, reject });
        channel.postMessage(JSON.stringify({ id, cmd, args: args ?? {} }));
      });
    },
  };
}

/** Number of in-flight calls (test hook). */
export function pendingCallCount(): number {
  return pending.size;
}
