/**
 * DisplayModeController for ImageViewer.
 *
 * Manages two display modes: "pixel" (100%) and "fit" (fit to window).
 * Creates simplified UI with mode toggle instead of incremental zoom controls.
 *
 * @module image-viewer/display-mode
 */

import { DISPLAY_MODE_STYLES } from "./display-mode-styles.ts";
import { isAncestorHidden } from "../shared/dom-utils.ts";

/**
 * Viewport padding factor (95% of viewport).
 */
const VIEWPORT_PADDING = 0.95;

/**
 * Minimum scale value (25%).
 */
const MIN_SCALE = 0.25;

/**
 * Module-level reference count for shared styles.
 * Ensures styles are only removed when the last controller is disposed.
 */
let styleRefCount = 0;

/**
 * Display mode type.
 */
export type DisplayMode = "pixel" | "fit";

/**
 * Display mode state.
 */
export interface DisplayModeState {
  /** Current display mode */
  mode: DisplayMode;
  /** Current scale value (1.0 = 100%) */
  scale: number;
  /** Pre-calculated fit scale */
  fitScale: number;
}

/**
 * Options for DisplayModeController initialization.
 */
export interface DisplayModeControllerOptions {
  /** Natural image width in pixels */
  imageWidth: number;
  /** Natural image height in pixels */
  imageHeight: number;
  /** Available viewport width */
  viewportWidth: number;
  /** Available viewport height */
  viewportHeight: number;
  /** Initial display mode (default: 'pixel') */
  initialMode?: DisplayMode;
  /** Callback when mode changes */
  onModeChange?: (state: DisplayModeState) => void;
  /** Callback when close button is clicked */
  onClose?: () => void;
  /** Callback for keyboard-initiated scroll (delta in pixels, positive = down) */
  onScroll?: (deltaY: number) => void;
  /** Overlay element for UI */
  overlay?: HTMLElement;
}

/**
 * Calculates the fit scale to display an image within a viewport.
 *
 * @param imageWidth - Natural image width in pixels
 * @param imageHeight - Natural image height in pixels
 * @param viewportWidth - Viewport width in pixels
 * @param viewportHeight - Viewport height in pixels
 * @returns Fit scale value (1.0 = 100%, never exceeds 1.0)
 */
export function calculateFitScale(
  imageWidth: number,
  imageHeight: number,
  viewportWidth: number,
  viewportHeight: number,
): number {
  // Guard against invalid image dimensions
  if (imageWidth <= 0 || imageHeight <= 0) {
    return 1.0; // Return 100% for invalid image dimensions
  }

  // Guard against invalid viewport dimensions
  if (viewportWidth <= 0 || viewportHeight <= 0) {
    return MIN_SCALE; // Return minimum scale for invalid viewport
  }

  // Apply viewport padding
  const effectiveWidth = viewportWidth * VIEWPORT_PADDING;
  const effectiveHeight = viewportHeight * VIEWPORT_PADDING;

  // Calculate scale factors
  const scaleX = effectiveWidth / imageWidth;
  const scaleY = effectiveHeight / imageHeight;

  // Use smaller scale to ensure image fits
  let fitScale = Math.min(scaleX, scaleY);

  // Don't upscale small images beyond 100%
  if (fitScale > 1.0) {
    fitScale = 1.0;
  }

  // Clamp to minimum scale
  if (fitScale < MIN_SCALE) {
    fitScale = MIN_SCALE;
  }

  return fitScale;
}

/**
 * DisplayModeController class.
 *
 * Manages display mode state and scale calculation for ImageViewer.
 * Provides simple two-mode display: "pixel" (100%) and "fit" (fit to window).
 */
export class DisplayModeController {
  private mode: DisplayMode;
  private scale: number;
  private fitScale: number;

  private imageWidth: number;
  private imageHeight: number;
  private viewportWidth: number;
  private viewportHeight: number;

  private onModeChange?: (state: DisplayModeState) => void;
  private onClose?: () => void;
  private onScroll?: (deltaY: number) => void;
  private overlay?: HTMLElement;

  // UI elements
  private styleElement: HTMLStyleElement | null = null;
  private closeButton: HTMLElement | null = null;
  private modeBar: HTMLElement | null = null;
  private modeToggle: HTMLElement | null = null;

  // Bound event handlers for cleanup
  private boundHandleKeydown: ((e: KeyboardEvent) => void) | null = null;

  /**
   * Creates a new DisplayModeController.
   *
   * @param options - Controller options
   */
  constructor(options: DisplayModeControllerOptions) {
    this.imageWidth = options.imageWidth;
    this.imageHeight = options.imageHeight;
    this.viewportWidth = options.viewportWidth;
    this.viewportHeight = options.viewportHeight;
    this.onModeChange = options.onModeChange;
    this.onClose = options.onClose;
    this.onScroll = options.onScroll;
    this.overlay = options.overlay;

    // Calculate fit scale
    this.fitScale = calculateFitScale(
      this.imageWidth,
      this.imageHeight,
      this.viewportWidth,
      this.viewportHeight,
    );

    // Initialize mode
    this.mode = options.initialMode ?? "pixel";

    // Set initial scale based on mode
    this.scale = this.mode === "pixel" ? 1.0 : this.fitScale;

    // Create UI if overlay is provided
    if (this.overlay) {
      this.injectStyles();
      this.createUI();
      this.setupKeyboardListener();
    }
  }

  /**
   * Toggles between pixel and fit modes.
   */
  toggle(): void {
    const newMode: DisplayMode = this.mode === "pixel" ? "fit" : "pixel";
    this.setMode(newMode);
  }

  /**
   * Sets a specific display mode.
   *
   * @param mode - Target display mode
   */
  setMode(mode: DisplayMode): void {
    // Skip if already in this mode
    if (this.mode === mode) {
      return;
    }

    this.mode = mode;
    this.scale = mode === "pixel" ? 1.0 : this.fitScale;

    // Update UI
    this.updateModeDisplay();

    // Notify callback
    this.onModeChange?.(this.getState());
  }

  /**
   * Returns the current state.
   */
  getState(): DisplayModeState {
    return {
      mode: this.mode,
      scale: this.scale,
      fitScale: this.fitScale,
    };
  }

  /**
   * Updates viewport dimensions.
   * Recalculates fit scale and updates current scale if in fit mode.
   *
   * @param width - New viewport width
   * @param height - New viewport height
   */
  updateViewport(width: number, height: number): void {
    this.viewportWidth = width;
    this.viewportHeight = height;

    // Recalculate fit scale
    this.fitScale = calculateFitScale(
      this.imageWidth,
      this.imageHeight,
      this.viewportWidth,
      this.viewportHeight,
    );

    // If in fit mode, update scale and notify
    if (this.mode === "fit") {
      this.scale = this.fitScale;
      this.onModeChange?.(this.getState());
    }
  }

  /**
   * Disposes the controller and releases resources.
   */
  dispose(): void {
    // Remove keyboard listener
    if (this.boundHandleKeydown) {
      document.removeEventListener("keydown", this.boundHandleKeydown, {
        capture: true,
      });
      this.boundHandleKeydown = null;
    }

    // Remove UI elements
    if (this.closeButton) {
      this.closeButton.remove();
      this.closeButton = null;
    }
    if (this.modeBar) {
      this.modeBar.remove();
      this.modeBar = null;
    }
    this.modeToggle = null;

    // Decrement style reference count and remove if last instance
    styleRefCount--;
    if (styleRefCount <= 0) {
      if (this.styleElement) {
        this.styleElement.remove();
        this.styleElement = null;
      }
      styleRefCount = 0; // Ensure non-negative
    }
  }

  /**
   * Injects CSS styles into the document.
   * Uses reference counting to manage shared styles across instances.
   */
  private injectStyles(): void {
    // Check if styles already exist
    const existingStyle = document.getElementById("display-mode-styles");
    if (existingStyle) {
      // Styles already exist, just increment ref count
      this.styleElement = existingStyle as HTMLStyleElement;
      styleRefCount++;
      return;
    }

    this.styleElement = document.createElement("style");
    this.styleElement.id = "display-mode-styles";
    this.styleElement.textContent = DISPLAY_MODE_STYLES;
    document.head.appendChild(this.styleElement);
    styleRefCount++;
  }

  /**
   * Creates UI elements (close button and mode bar).
   */
  private createUI(): void {
    if (!this.overlay) return;

    this.createCloseButton();
    this.createModeBar();
  }

  /**
   * Creates the close button.
   */
  private createCloseButton(): void {
    if (!this.overlay) return;

    this.closeButton = document.createElement("button");
    this.closeButton.className = "viewer-close-button";
    this.closeButton.setAttribute("type", "button");
    this.closeButton.setAttribute("aria-label", "Close viewer");
    this.closeButton.innerHTML = "\u00D7"; // multiplication sign (x)
    this.closeButton.addEventListener("click", () => {
      this.onClose?.();
    });
    this.overlay.appendChild(this.closeButton);
  }

  /**
   * Creates the mode toggle bar.
   */
  private createModeBar(): void {
    if (!this.overlay) return;

    this.modeBar = document.createElement("div");
    this.modeBar.className = "viewer-mode-bar";

    // Mode toggle button
    this.modeToggle = document.createElement("button");
    this.modeToggle.className = "viewer-mode-toggle";
    this.modeToggle.setAttribute("type", "button");
    this.modeToggle.setAttribute("aria-label", "Toggle display mode");
    this.updateModeDisplay();
    this.modeToggle.addEventListener("click", () => this.toggle());

    this.modeBar.appendChild(this.modeToggle);
    this.overlay.appendChild(this.modeBar);
  }

  /**
   * Updates the mode display text.
   */
  private updateModeDisplay(): void {
    if (!this.modeToggle) return;

    this.modeToggle.textContent = this.mode === "pixel" ? "100%" : "Fit";
  }

  /**
   * Sets up keyboard event listener.
   */
  private setupKeyboardListener(): void {
    this.boundHandleKeydown = this.handleKeydown.bind(this);
    document.addEventListener("keydown", this.boundHandleKeydown, {
      capture: true,
    });
  }

  /**
   * Handles keyboard events for mode switching.
   */
  private handleKeydown(e: KeyboardEvent): void {
    // Only handle keys when the viewer is visible
    if (!this.overlay?.classList.contains("visible")) {
      return;
    }

    // Additional check: is the overlay actually visible in the DOM?
    // When tab is switched, the tab container becomes display:none
    // but the visible class remains on the overlay.
    // Check if any ancestor has display:none by walking up the DOM tree.
    if (isAncestorHidden(this.overlay)) {
      return;
    }

    switch (e.key) {
      case "f":
        e.preventDefault();
        e.stopPropagation();
        this.toggle();
        break;

      case "Escape":
        e.preventDefault();
        e.stopPropagation();
        this.onClose?.();
        break;

      case " ": {
        e.preventDefault();
        e.stopPropagation();
        const viewportHeight = this.overlay?.clientHeight || 0;
        const delta = viewportHeight * 0.85;
        if (e.shiftKey) {
          this.onScroll?.(-delta);
        } else {
          this.onScroll?.(delta);
        }
        break;
      }

      // Block all other keys from reaching the shell
      default:
        e.preventDefault();
        e.stopPropagation();
        break;
    }
  }

}
