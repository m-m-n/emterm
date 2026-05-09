/**
 * Pointer event wiring for the terminal container.
 *
 * Centralises the four DOM event listeners that `init()` used to attach
 * inline:
 *
 * - **mousedown (middle button)** on the terminal root: clears the
 *   selection, optionally triggers paste-on-middle-click, and suppresses
 *   the matching mouseup so PTY mouse tracking does not see an orphaned
 *   button-3 release. The capture-phase suppressor is one-shot and self-
 *   removing, matching the pre-extraction behaviour exactly.
 * - **contextmenu** on the *tab content* container (not the terminal
 *   root) so the menu also opens when the user right-clicks the padding
 *   around the canvas.
 * - **wheel** on the terminal root for scrollback navigation.
 * - **click** on the terminal root for fold toggle (plain click) and
 *   URL open (Ctrl/Meta + click).
 *
 * Extracted from TerminalApp purely to slim `init()`. Behaviour and
 * registration order are preserved 1:1.
 */

import { SettingsService } from "../settings/settings-service";
import { effectiveMiddleClickPaste } from "../settings/effective-settings";
import { showTerminalContextMenu } from "../context-menu";
import type { SelectionController } from "../selection-v2";
import type { LinkHandler, FoldHandler } from "./handlers";

/**
 * Hooks the wiring needs from the host TerminalApp.
 *
 * `app` is required because `showTerminalContextMenu` takes the whole
 * TerminalApp instance (it uses several of its public methods to build
 * the menu). `unknown` here is fine — context-menu treats it as opaque.
 */
export interface InputWiringContext {
  /** Tab content container — receives the contextmenu listener. */
  container: HTMLElement;
  /** Terminal root — receives mousedown/wheel/click listeners. */
  terminalContainer: HTMLElement;
  getSelectionController(): SelectionController | null;
  getLinkHandler(): LinkHandler | null;
  getFoldHandler(): FoldHandler | null;
  /** TerminalApp instance for context menu. */
  app: unknown;
  /** Invoked on confirmed middle-click paste. */
  onMiddleClickPaste(): void;
  /** Invoked on wheel events. */
  onWheel(e: WheelEvent): void;
}

/**
 * Attach the four pointer-event listeners on the appropriate containers.
 * No detach handle is returned — the listeners are bound for the
 * lifetime of the container, which is removed in `dispose()` together
 * with the listeners.
 */
export function wireInputEvents(ctx: InputWiringContext): void {
  // Add middle-click paste handler (registered before MouseHandler so stopImmediatePropagation
  // prevents PTY mouse tracking from seeing middle button events when paste is enabled)
  ctx.terminalContainer.addEventListener('mousedown', (e) => {
    if (e.button === 1) {
      // Clear selection on middle click
      ctx.getSelectionController()?.clearSelection();

      const settings = SettingsService.getCached();
      if (effectiveMiddleClickPaste(settings)) {
        e.preventDefault();
        e.stopImmediatePropagation();
        // Suppress the matching mouseup to prevent an orphaned release event reaching the PTY.
        // Uses capture phase so it fires before MouseHandler's bubble-phase listener.
        const suppressMouseUp = (ev: MouseEvent) => {
          if (ev.button === 1) {
            ev.stopPropagation();
            ev.preventDefault();
            ctx.terminalContainer.removeEventListener('mouseup', suppressMouseUp, true);
          }
        };
        ctx.terminalContainer.addEventListener('mouseup', suppressMouseUp, true);
        ctx.onMiddleClickPaste();
      }
    }
  });

  // Add context menu handler for terminal right-click
  // Use this.container (.tab-content) instead of terminalContainer (.terminal-root)
  // so the handler also covers the padding area around the terminal
  ctx.container.addEventListener('contextmenu', (e) => {
    showTerminalContextMenu(e, { app: ctx.app as never });
  });

  // Add mouse wheel handler for scrollback
  ctx.terminalContainer.addEventListener('wheel', (e) => ctx.onWheel(e));

  // Add click handler for fold toggle (plain click) and URL opening (Ctrl+click)
  ctx.terminalContainer.addEventListener('click', (e) => {
    if (e.ctrlKey || e.metaKey) {
      ctx.getLinkHandler()?.handleUrlClick(e);
    } else {
      ctx.getFoldHandler()?.handleFoldClick(e);
    }
  });
}
