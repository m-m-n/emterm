/**
 * Shared ZoomController for image and Markdown viewers.
 *
 * Provides zoom functionality with mouse wheel, keyboard, and UI button controls.
 *
 * @module shared/zoom-controller
 */

import { ZOOM_CONTROLLER_STYLES } from "./zoom-styles.ts";

/**
 * Module-level reference count for shared styles.
 * Ensures styles are only removed when the last controller is disposed.
 */
let styleRefCount = 0;

/**
 * Zoom state management.
 */
interface ZoomState {
  /** Current zoom level as percentage (25-400) */
  level: number;
  /** Transform origin X coordinate (for mouse-based zoom) */
  originX: number;
  /** Transform origin Y coordinate (for mouse-based zoom) */
  originY: number;
}

/**
 * Options for ZoomController initialization.
 */
export interface ZoomControllerOptions {
  /** Element to apply zoom transform to */
  container: HTMLElement;
  /** Parent overlay element for fixed UI components */
  overlay: HTMLElement;
  /** Minimum zoom level (default: 25) */
  minZoom?: number;
  /** Maximum zoom level (default: 400) */
  maxZoom?: number;
  /** Zoom step per operation (default: 10) */
  zoomStep?: number;
  /** Callback when close button is clicked */
  onClose?: () => void;
  /**
   * Callback when zoom level changes.
   * When provided, the controller will NOT apply default transform.
   * The callback is responsible for handling the zoom display.
   */
  onZoomChange?: (level: number) => void;
  /** Callback when reset is triggered (clicking zoom percentage) */
  onReset?: () => void;
  /** Initial zoom level (default: 100). Will be clamped to min/max range. */
  initialLevel?: number;
}

/**
 * Default option values.
 */
const DEFAULT_OPTIONS = {
  minZoom: 25,
  maxZoom: 400,
  zoomStep: 10,
};

/**
 * ZoomController class.
 *
 * Manages zoom state and provides zoom operations for viewer components.
 */
export class ZoomController {
  private state: ZoomState;
  private options: Required<
    Omit<
      ZoomControllerOptions,
      "onClose" | "onZoomChange" | "onReset" | "initialLevel"
    >
  > & {
    onClose?: () => void;
    onZoomChange?: (level: number) => void;
    onReset?: () => void;
  };

  /** The initial zoom level to reset to */
  private initialLevel: number;

  private closeButton: HTMLElement | null = null;
  private zoomBar: HTMLElement | null = null;
  private zoomLevelDisplay: HTMLElement | null = null;
  private styleElement: HTMLStyleElement | null = null;

  // Bound event handlers for cleanup
  private boundHandleWheel: (e: WheelEvent) => void;
  private boundHandleKeydown: (e: KeyboardEvent) => void;

  // Throttle for wheel events
  private lastWheelTime = 0;
  private readonly WHEEL_THROTTLE = 16; // ms (60fps)

  // Original transform values for restoration on dispose
  private originalTransform: string = "";
  private originalTransformOrigin: string = "";

  /**
   * Creates a new ZoomController.
   *
   * @param options - Controller options
   */
  constructor(options: ZoomControllerOptions) {
    this.options = {
      ...DEFAULT_OPTIONS,
      ...options,
    };

    // Determine initial level, clamped to valid range
    const requestedInitial = options.initialLevel ?? 100;
    this.initialLevel = Math.max(
      this.options.minZoom,
      Math.min(requestedInitial, this.options.maxZoom),
    );

    // Initialize state at initial level, centered origin
    this.state = {
      level: this.initialLevel,
      originX: 50,
      originY: 50,
    };

    // Bind event handlers
    this.boundHandleWheel = this.handleWheel.bind(this);
    this.boundHandleKeydown = this.handleKeydown.bind(this);

    // Save original transform values for restoration on dispose
    this.originalTransform = this.options.container.style.transform;
    this.originalTransformOrigin = this.options.container.style.transformOrigin;

    // Initialize
    this.injectStyles();
    this.createUI();
    this.updateZoomDisplay(); // Update display with initial level
    this.setupEventListeners();
    this.applyZoom();
  }

  /**
   * Increases zoom level by one step.
   */
  zoomIn(): void {
    const newLevel = Math.min(
      this.state.level + this.options.zoomStep,
      this.options.maxZoom,
    );
    this.state.level = newLevel;
    this.applyZoom();
    this.updateZoomDisplay();
  }

  /**
   * Decreases zoom level by one step.
   */
  zoomOut(): void {
    const newLevel = Math.max(
      this.state.level - this.options.zoomStep,
      this.options.minZoom,
    );
    this.state.level = newLevel;
    this.applyZoom();
    this.updateZoomDisplay();
  }

  /**
   * Sets zoom level to a specific value.
   *
   * @param level - Target zoom level (will be clamped to valid range)
   */
  zoomTo(level: number): void {
    const clampedLevel = Math.max(
      this.options.minZoom,
      Math.min(level, this.options.maxZoom),
    );
    this.state.level = clampedLevel;
    this.applyZoom();
    this.updateZoomDisplay();
  }

  /**
   * Resets zoom level to initial level (or 100% if not set).
   */
  resetZoom(): void {
    this.state.level = this.initialLevel;
    this.state.originX = 50;
    this.state.originY = 50;
    this.applyZoom();
    this.updateZoomDisplay();

    // Call onReset callback if provided
    this.options.onReset?.();
  }

  /**
   * Returns the current zoom level.
   */
  getZoomLevel(): number {
    return this.state.level;
  }

  /**
   * Sets the zoom level programmatically.
   *
   * @param level - New zoom level percentage
   */
  setZoomLevel(level: number): void {
    const clampedLevel = Math.max(
      this.options.minZoom,
      Math.min(level, this.options.maxZoom),
    );
    this.state.level = clampedLevel;
    this.applyZoom();
    this.updateZoomDisplay();
  }

  /**
   * Disposes the controller and releases resources.
   */
  dispose(): void {
    this.removeEventListeners();

    // Remove UI elements
    if (this.closeButton) {
      this.closeButton.remove();
      this.closeButton = null;
    }
    if (this.zoomBar) {
      this.zoomBar.remove();
      this.zoomBar = null;
    }
    // Decrement style reference count and remove if last instance
    styleRefCount--;
    if (styleRefCount <= 0) {
      if (this.styleElement) {
        this.styleElement.remove();
        this.styleElement = null;
      }
      styleRefCount = 0; // Ensure non-negative
    }

    // Restore original transform values
    this.options.container.style.transform = this.originalTransform;
    this.options.container.style.transformOrigin = this.originalTransformOrigin;
  }

  /**
   * Injects CSS styles into the document.
   * Uses reference counting to manage shared styles across instances.
   */
  private injectStyles(): void {
    // Check if styles already exist
    const existingStyle = document.getElementById("zoom-controller-styles");
    if (existingStyle) {
      // Styles already exist, just increment ref count
      this.styleElement = existingStyle as HTMLStyleElement;
      styleRefCount++;
      return;
    }

    this.styleElement = document.createElement("style");
    this.styleElement.id = "zoom-controller-styles";
    this.styleElement.textContent = ZOOM_CONTROLLER_STYLES;
    document.head.appendChild(this.styleElement);
    styleRefCount++;
  }

  /**
   * Creates UI elements (close button and zoom bar).
   */
  private createUI(): void {
    this.createCloseButton();
    this.createZoomBar();
  }

  /**
   * Creates the close button.
   */
  private createCloseButton(): void {
    this.closeButton = document.createElement("button");
    this.closeButton.className = "viewer-close-button";
    this.closeButton.setAttribute("type", "button");
    this.closeButton.setAttribute("aria-label", "Close viewer");
    this.closeButton.innerHTML = "\u00D7"; // multiplication sign (x)
    this.closeButton.addEventListener("click", () => {
      this.options.onClose?.();
    });
    this.options.overlay.appendChild(this.closeButton);
  }

  /**
   * Creates the zoom control bar.
   */
  private createZoomBar(): void {
    this.zoomBar = document.createElement("div");
    this.zoomBar.className = "viewer-zoom-bar";

    // Zoom out button
    const zoomOutBtn = document.createElement("button");
    zoomOutBtn.className = "viewer-zoom-button";
    zoomOutBtn.setAttribute("type", "button");
    zoomOutBtn.setAttribute("aria-label", "Zoom out");
    zoomOutBtn.textContent = "\u2212"; // minus sign
    zoomOutBtn.addEventListener("click", () => this.zoomOut());

    // Zoom level display
    this.zoomLevelDisplay = document.createElement("span");
    this.zoomLevelDisplay.className = "viewer-zoom-level";
    this.zoomLevelDisplay.setAttribute("role", "button");
    this.zoomLevelDisplay.setAttribute("aria-label", "Reset zoom to 100%");
    this.zoomLevelDisplay.textContent = "100%";
    this.zoomLevelDisplay.addEventListener("click", () => this.resetZoom());

    // Zoom in button
    const zoomInBtn = document.createElement("button");
    zoomInBtn.className = "viewer-zoom-button";
    zoomInBtn.setAttribute("type", "button");
    zoomInBtn.setAttribute("aria-label", "Zoom in");
    zoomInBtn.textContent = "+";
    zoomInBtn.addEventListener("click", () => this.zoomIn());

    this.zoomBar.appendChild(zoomOutBtn);
    this.zoomBar.appendChild(this.zoomLevelDisplay);
    this.zoomBar.appendChild(zoomInBtn);
    this.options.overlay.appendChild(this.zoomBar);
  }

  /**
   * Updates the zoom level display.
   */
  private updateZoomDisplay(): void {
    if (this.zoomLevelDisplay) {
      this.zoomLevelDisplay.textContent = `${this.state.level}%`;
    }
  }

  /**
   * Applies the current zoom level to the container.
   * If onZoomChange callback is provided, calls it instead of applying transform.
   */
  private applyZoom(): void {
    // If callback is provided, delegate zoom handling to the consumer
    if (this.options.onZoomChange) {
      this.options.onZoomChange(this.state.level);
      return;
    }

    // Default behavior: apply CSS transform
    const scale = this.state.level / 100;
    this.options.container.style.transformOrigin = `${this.state.originX}% ${this.state.originY}%`;
    this.options.container.style.transform = `scale(${scale})`;
  }

  /**
   * Sets up event listeners.
   */
  private setupEventListeners(): void {
    this.options.overlay.addEventListener("wheel", this.boundHandleWheel, {
      passive: false,
    });
    document.addEventListener("keydown", this.boundHandleKeydown, {
      capture: true,
    });
  }

  /**
   * Removes event listeners.
   */
  private removeEventListeners(): void {
    this.options.overlay.removeEventListener("wheel", this.boundHandleWheel);
    document.removeEventListener("keydown", this.boundHandleKeydown, {
      capture: true,
    });
  }

  /**
   * Handles mouse wheel events for zooming.
   */
  private handleWheel(e: WheelEvent): void {
    // Only zoom with Ctrl key
    if (!e.ctrlKey) return;

    e.preventDefault();

    // Throttle wheel events
    const now = performance.now();
    if (now - this.lastWheelTime < this.WHEEL_THROTTLE) return;
    this.lastWheelTime = now;

    // Calculate mouse position relative to container for transform-origin
    const rect = this.options.container.getBoundingClientRect();

    // Guard against zero width/height to prevent NaN values
    if (rect.width > 0 && rect.height > 0) {
      const x = ((e.clientX - rect.left) / rect.width) * 100;
      const y = ((e.clientY - rect.top) / rect.height) * 100;

      // Update origin to mouse position (clamped to 0-100%)
      this.state.originX = Math.max(0, Math.min(100, x));
      this.state.originY = Math.max(0, Math.min(100, y));
    }
    // If container has zero size, keep existing origin (center by default)

    // Zoom in or out based on wheel direction
    if (e.deltaY < 0) {
      this.zoomIn();
    } else {
      this.zoomOut();
    }
  }

  /**
   * Handles keyboard events for zooming.
   */
  private handleKeydown(e: KeyboardEvent): void {
    // Only handle zoom keys, let other keys pass through
    switch (e.key) {
      case "+":
      case "=":
        e.preventDefault();
        e.stopPropagation();
        // Reset origin to center for keyboard zoom
        this.state.originX = 50;
        this.state.originY = 50;
        this.zoomIn();
        break;

      case "-":
        e.preventDefault();
        e.stopPropagation();
        // Reset origin to center for keyboard zoom
        this.state.originX = 50;
        this.state.originY = 50;
        this.zoomOut();
        break;

      case "0":
        e.preventDefault();
        e.stopPropagation();
        this.resetZoom();
        break;

      case "1":
        e.preventDefault();
        e.stopPropagation();
        // Jump to 100% (pixel-perfect)
        this.state.originX = 50;
        this.state.originY = 50;
        this.zoomTo(100);
        break;

      // Let other keys pass through (Escape, arrows, etc.)
    }
  }
}
