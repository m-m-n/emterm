/**
 * Prefix key state machine for mux operations.
 *
 * States: idle → waiting (prefix pressed) → dispatch (action key pressed)
 *
 * Default prefix: Ctrl+b (tmux compatible)
 */

/** Mux action dispatched after prefix + key. */
export type MuxAction =
  | { type: "split-vertical" }
  | { type: "split-horizontal" }
  | { type: "next-pane" }
  | { type: "prev-pane" }
  | { type: "close-pane" }
  | { type: "zoom-toggle" }
  | { type: "detach" }
  | { type: "new-window" }
  | { type: "next-window" }
  | { type: "prev-window" }
  | { type: "rename-window" }
  | { type: "copy-mode" }
  | { type: "paste" }
  | { type: "prefix-passthrough" }; // Send prefix key itself to PTY

/** Prefix key handler state. */
export type PrefixKeyState = "idle" | "waiting";

/** Default key bindings (tmux-compatible). */
const DEFAULT_BINDINGS: Record<string, MuxAction> = {
  "%": { type: "split-vertical" },
  '"': { type: "split-horizontal" },
  o: { type: "next-pane" },
  ";": { type: "prev-pane" },
  x: { type: "close-pane" },
  z: { type: "zoom-toggle" },
  d: { type: "detach" },
  c: { type: "new-window" },
  n: { type: "next-window" },
  p: { type: "prev-window" },
  ",": { type: "rename-window" },
  "[": { type: "copy-mode" },
  "]": { type: "paste" },
};

/**
 * Prefix key handler for mux mode.
 */
export class PrefixKeyHandler {
  private _state: PrefixKeyState = "idle";
  private prefixKey: string;
  private prefixCtrl: boolean;
  private bindings: Record<string, MuxAction>;
  private onAction: ((action: MuxAction) => void) | null = null;
  private timeoutId: ReturnType<typeof setTimeout> | null = null;

  constructor(prefix = "b", prefixCtrl = true, customBindings?: Record<string, MuxAction>) {
    this.prefixKey = prefix;
    this.prefixCtrl = prefixCtrl;
    this.bindings = { ...DEFAULT_BINDINGS, ...customBindings };
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
      // Check for prefix key
      if (this.isPrefix(event)) {
        this._state = "waiting";
        // Auto-reset after 2 seconds if no action key pressed
        this.timeoutId = setTimeout(() => {
          this._state = "idle";
        }, 2000);
        return true; // Consume prefix key
      }
      return false; // Not consumed
    }

    // State: waiting for action key
    this.clearTimeout();
    this._state = "idle";

    // Check if it's the prefix key again (send prefix to PTY)
    if (this.isPrefix(event)) {
      this.onAction?.({ type: "prefix-passthrough" });
      return true;
    }

    // Look up binding
    const action = this.bindings[event.key];
    if (action) {
      this.onAction?.(action);
      return true;
    }

    // Unknown key after prefix — ignore
    return true;
  }

  /** Reset state to idle. */
  reset(): void {
    this.clearTimeout();
    this._state = "idle";
  }

  private isPrefix(event: KeyboardEvent): boolean {
    if (this.prefixCtrl && !event.ctrlKey) return false;
    if (!this.prefixCtrl && event.ctrlKey) return false;
    return event.key === this.prefixKey;
  }

  private clearTimeout(): void {
    if (this.timeoutId !== null) {
      clearTimeout(this.timeoutId);
      this.timeoutId = null;
    }
  }
}
