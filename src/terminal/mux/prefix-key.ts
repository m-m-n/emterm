/**
 * Prefix key state machine for mux operations.
 *
 * States: idle -> waiting (prefix pressed) -> dispatch (action key pressed)
 *
 * Default prefix: Ctrl+B (tmux compatible)
 */

import { matchKeybindStr, parseKeybind } from "../../keybind/matcher";

/** Mux action dispatched after prefix + key. */
export type MuxAction =
  | { type: "detach" }
  | { type: "new-window" }
  | { type: "next-window" }
  | { type: "prev-window" }
  | { type: "rename-window" }
  | { type: "prefix-passthrough" }; // Send prefix key itself to PTY

/** Prefix key handler state. */
export type PrefixKeyState = "idle" | "waiting";

/** Default action bindings (action -> keybind string, tmux-compatible). */
export const DEFAULT_ACTION_BINDINGS: Record<string, string> = {
  "detach": "d",
  "new-window": "c",
  "next-window": "n",
  "prev-window": "p",
  "rename-window": ",",
};

/**
 * Check if a keybind string contains modifiers (Ctrl, Shift, Alt, Meta).
 * Single characters like "c" or "%" have no modifiers.
 */
function hasModifiers(keybindStr: string): boolean {
  const parsed = parseKeybind(keybindStr);
  return parsed.ctrlKey || parsed.shiftKey || parsed.altKey || parsed.metaKey;
}

/**
 * Prefix key handler for mux mode.
 *
 * Accepts keybind strings in settings format (e.g., "Ctrl+B", "Ctrl+Z").
 * Action bindings can be single chars ("c", "%") or key combos ("Ctrl+N").
 */
export class PrefixKeyHandler {
  private _state: PrefixKeyState = "idle";
  private prefixKeybind: string;
  private actionBindings: Record<string, string>;
  private onAction: ((action: MuxAction) => void) | null = null;
  private timeoutId: ReturnType<typeof setTimeout> | null = null;

  constructor(
    prefixKeybind = "Ctrl+B",
    customBindings?: Record<string, string>,
  ) {
    this.prefixKeybind = prefixKeybind;
    this.actionBindings = { ...DEFAULT_ACTION_BINDINGS };
    if (customBindings) {
      for (const [action, key] of Object.entries(customBindings)) {
        if (key) this.actionBindings[action] = key;
      }
    }
  }

  get state(): PrefixKeyState {
    return this._state;
  }

  /** Set action callback. */
  setOnAction(callback: (action: MuxAction) => void): void {
    this.onAction = callback;
  }

  /**
   * Handle a keyboard event. Returns true if the event was consumed
   * (should not be forwarded to PTY).
   */
  handleKeyEvent(event: KeyboardEvent): boolean {
    if (this._state === "idle") {
      if (matchKeybindStr(event, this.prefixKeybind)) {
        this._state = "waiting";
        // Auto-reset after 2 seconds if no action key pressed
        this.timeoutId = setTimeout(() => {
          this._state = "idle";
        }, 2000);
        return true;
      }
      return false;
    }

    // State: waiting for action key
    this.clearTimeout();
    this._state = "idle";

    // Check if it's the prefix key again (send prefix to PTY)
    if (matchKeybindStr(event, this.prefixKeybind)) {
      this.onAction?.({ type: "prefix-passthrough" });
      return true;
    }

    // Look up binding by iterating action bindings
    const action = this.matchActionBinding(event);
    if (action) {
      this.onAction?.({ type: action } as MuxAction);
      return true;
    }

    // Unknown key after prefix -- ignore
    return true;
  }

  /** Reset state to idle. */
  reset(): void {
    this.clearTimeout();
    this._state = "idle";
  }

  /**
   * Match an event against action bindings.
   * For single-char bindings (no modifiers), match event.key directly.
   * For modifier bindings (e.g., "Ctrl+N"), use matchKeybindStr.
   */
  private matchActionBinding(event: KeyboardEvent): string | null {
    for (const [action, binding] of Object.entries(this.actionBindings)) {
      if (hasModifiers(binding)) {
        if (matchKeybindStr(event, binding)) {
          return action;
        }
      } else {
        // Single-char binding: match event.key directly (no modifier check)
        const parsed = parseKeybind(binding);
        if (event.key === parsed.key) {
          return action;
        }
      }
    }
    return null;
  }

  private clearTimeout(): void {
    if (this.timeoutId !== null) {
      clearTimeout(this.timeoutId);
      this.timeoutId = null;
    }
  }
}
