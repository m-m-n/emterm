/**
 * Mouse event handler for terminal PTY mouse tracking.
 *
 * Handles mouse events for PTY applications that request mouse tracking.
 * Selection is handled by SelectionController, not this handler.
 */

import type { PtyClient } from "../../pty/client";
import type { TerminalState } from "../../terminal/state";
import type { CharSize } from "../types";
import {
  domEventToMouseEvent,
  encodeMouseEvent,
  isMouseTrackingEnabled,
} from "../../terminal/mouse";

/**
 * Mouse handler options (kept for backwards compatibility)
 */
export interface MouseHandlerOptions {
  // Reserved for future use
}

/**
 * Handles mouse events for PTY mouse tracking.
 *
 * This handler ONLY sends mouse events to the PTY when mouse tracking is enabled.
 * Text selection is handled separately by SelectionController.
 */
export class MouseHandler {
  private container: HTMLElement;
  private ptyClient: PtyClient;
  private getState: () => TerminalState;
  private charSize: CharSize;
  private cleanupFunctions: (() => void)[] = [];

  /**
   * Creates a new MouseHandler instance
   * @param container - Terminal container element
   * @param ptyClient - PTY client for sending mouse events
   * @param getState - Function to get current terminal state
   * @param charSize - Character cell dimensions
   * @param _options - Reserved for future use
   */
  constructor(
    container: HTMLElement,
    ptyClient: PtyClient,
    getState: () => TerminalState,
    charSize: CharSize,
    _options?: MouseHandlerOptions,
  ) {
    this.container = container;
    this.ptyClient = ptyClient;
    this.getState = getState;
    this.charSize = charSize;
  }

  /**
   * Attaches mouse event listeners
   */
  attach(): void {
    const onMouseDown = this.onMouseDown.bind(this);
    const onMouseUp = this.onMouseUp.bind(this);
    const onMouseMove = this.onMouseMove.bind(this);
    const onWheel = this.onWheel.bind(this);
    const onContextMenu = this.onContextMenu.bind(this);

    this.container.addEventListener("mousedown", onMouseDown);
    this.container.addEventListener("mouseup", onMouseUp);
    this.container.addEventListener("mousemove", onMouseMove);
    this.container.addEventListener("wheel", onWheel, { passive: false });
    this.container.addEventListener("contextmenu", onContextMenu);

    // Store cleanup functions
    this.cleanupFunctions = [
      () => this.container.removeEventListener("mousedown", onMouseDown),
      () => this.container.removeEventListener("mouseup", onMouseUp),
      () => this.container.removeEventListener("mousemove", onMouseMove),
      () => this.container.removeEventListener("wheel", onWheel),
      () => this.container.removeEventListener("contextmenu", onContextMenu),
    ];
  }

  /**
   * Detaches mouse event listeners
   */
  detach(): void {
    for (const cleanup of this.cleanupFunctions) {
      cleanup();
    }
    this.cleanupFunctions = [];
  }

  /**
   * Updates the character cell dimensions
   * @param width - New character width in pixels
   * @param height - New character height in pixels
   */
  updateCharSize(width: number, height: number): void {
    this.charSize = { width, height };
  }

  /**
   * Check if this event should be handled for PTY mouse tracking.
   * Returns false if selection should be handled instead.
   */
  private shouldHandleForPty(event: MouseEvent): boolean {
    const state = this.getState();
    if (!state) return false;

    const modes = state.getModes();
    if (!isMouseTrackingEnabled(modes.mouseTracking)) {
      return false;
    }

    // Shift key forces selection mode - don't send to PTY
    if (event.shiftKey) {
      return false;
    }

    return true;
  }

  /**
   * Handles mouse events and sends them to PTY if mouse tracking is enabled
   */
  private async handleMouseEvent(
    event: MouseEvent | WheelEvent,
    type: "down" | "up" | "move" | "wheel",
  ): Promise<void> {
    const state = this.getState();
    if (!state) return;

    const modes = state.getModes();
    if (!isMouseTrackingEnabled(modes.mouseTracking)) return;

    const rect = this.container.getBoundingClientRect();
    const mouseEvent = domEventToMouseEvent(
      event,
      this.charSize.width,
      this.charSize.height,
      rect,
      type,
    );

    if (!mouseEvent) return;

    const encoded = encodeMouseEvent(
      mouseEvent,
      modes.mouseTracking,
      modes.mouseEncoding,
    );
    if (encoded) {
      event.preventDefault();
      try {
        await this.ptyClient.write(encoded);
      } catch (error) {
        console.error("Failed to send mouse event:", error);
      }
    }
  }

  /**
   * Handles mouse down events
   */
  private onMouseDown(e: MouseEvent): void {
    if (this.shouldHandleForPty(e)) {
      this.handleMouseEvent(e, "down");
    }
    // If not handling for PTY, let SelectionController handle it
  }

  /**
   * Handles mouse up events
   */
  private onMouseUp(e: MouseEvent): void {
    if (this.shouldHandleForPty(e)) {
      this.handleMouseEvent(e, "up");
    }
  }

  /**
   * Handles mouse move events
   */
  private onMouseMove(e: MouseEvent): void {
    const state = this.getState();
    if (!state) return;

    const modes = state.getModes();

    // Only send to PTY if tracking and no shift
    if (isMouseTrackingEnabled(modes.mouseTracking) && !e.shiftKey) {
      // Only track motion if a button is pressed or any-event mode
      if (modes.mouseTracking === "any" || e.buttons !== 0) {
        this.handleMouseEvent(e, "move");
      }
    }
  }

  /**
   * Handles wheel events
   */
  private onWheel(e: WheelEvent): void {
    const state = this.getState();
    if (!state) return;

    const modes = state.getModes();
    if (isMouseTrackingEnabled(modes.mouseTracking)) {
      this.handleMouseEvent(e, "wheel");
    }
  }

  /**
   * Handles context menu events.
   * Always allows the event to propagate so the terminal's
   * contextmenu handler can show the native context menu,
   * even when PTY mouse tracking is active (FR12).
   */
  private onContextMenu(_e: MouseEvent): void {
    // Do not preventDefault — let the contextmenu event
    // propagate to the terminal root handler.
  }
}
