/**
 * UI handler functions extracted from TerminalApp.
 * Handles bell, mouse wheel scrollback, and middle-click paste.
 */

import type { TerminalState } from "../terminal/state";
import type { ITerminalRenderer } from "../terminal";
import type { PtyClient } from "../pty/client";
import type { ImeHandler } from "./handlers/ime";
import type { SelectionController } from "../selection-v2";
import type { CharSize } from "./types";
import { SettingsService } from "../settings/settings-service";
import { showPasteDialog, sendTextInChunks } from "../clipboard";

/**
 * Handle BEL character based on bell_action setting.
 */
export function handleBell(
  terminalRoot: HTMLElement | null,
  bellActivityCallback: (() => void) | null,
): void {
  const cachedSettings = SettingsService.getCached();
  const bellAction = cachedSettings?.bell_action ?? "visual";

  switch (bellAction) {
    case "visual": {
      const container = terminalRoot;
      if (container) {
        container.classList.add("terminal-bell-flash");
        setTimeout(() => container.classList.remove("terminal-bell-flash"), 150);
      }
      break;
    }
    case "sound": {
      try {
        const ctx = new AudioContext();
        const oscillator = ctx.createOscillator();
        const gain = ctx.createGain();
        oscillator.connect(gain);
        gain.connect(ctx.destination);
        oscillator.frequency.value = 800;
        gain.gain.value = 0.1;
        oscillator.start();
        oscillator.stop(ctx.currentTime + 0.1);
      } catch {
        // Audio not available
      }
      break;
    }
    case "none":
      break;
  }

  // Notify activity tracker
  bellActivityCallback?.();
}

/**
 * Handle mouse wheel events for scrollback.
 */
export function handleWheel(
  e: WheelEvent,
  renderer: ITerminalRenderer | null,
  state: TerminalState | null,
  charSize: CharSize,
): void {
  e.preventDefault();

  if (!renderer || !state) return;

  // Get scroll speed multiplier from settings (default: 3)
  const cachedSettings = SettingsService.getCached();
  const scrollSpeed = cachedSettings?.scroll_speed ?? 3;

  // Calculate number of lines to scroll based on wheel delta and speed
  const lines = Math.ceil(Math.abs(e.deltaY) / charSize.height * scrollSpeed);

  if (e.deltaY < 0) {
    // Scroll up (toward past)
    renderer.scrollUp(lines);
  } else {
    // Scroll down (toward present)
    renderer.scrollDown(lines);
  }

  // Force re-render with new scroll offset
  renderer.forceRender(state);
}

/**
 * Handle middle-click paste from clipboard.
 */
export async function handleMiddleClickPaste(
  selectionController: SelectionController | null,
  ptyClient: PtyClient | null,
  imeHandler: ImeHandler | null,
  exitScrollback: () => void,
): Promise<void> {
  if (!selectionController || !ptyClient) return;

  try {
    const text = await selectionController.paste();
    if (!text) return;

    // Auto-scroll to bottom when user pastes during scrollback
    exitScrollback();

    if (selectionController.isMultiLinePaste(text)) {
      const lineCount = selectionController.countPasteLines(text);
      const result = await showPasteDialog({ text, lineCount });
      if (result.confirmed) {
        await sendTextInChunks(text, (data: Uint8Array) =>
          ptyClient.write(data),
        );
      }
    } else {
      const bytes = new TextEncoder().encode(text);
      await ptyClient.write(bytes);
    }
  } catch (error) {
    console.error("Failed to paste from clipboard (middle-click):", error);
  } finally {
    imeHandler?.focus();
  }
}
