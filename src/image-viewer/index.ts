/**
 * Fullscreen Image Viewer.
 *
 * Displays images in a fullscreen overlay, similar to the MarkdownViewer pattern.
 *
 * @module image-viewer/index
 */

import type {
  DecodedImage,
  AnimationEvent,
  AnimationState,
} from "../image/types.ts";
import { ZoomController } from "../shared/zoom-controller.ts";
import { PanController } from "./pan-controller.ts";

/**
 * Viewport padding factor (95% of viewport).
 */
const VIEWPORT_PADDING = 0.95;

/**
 * Default zoom constraints.
 */
const DEFAULT_MIN_ZOOM = 25;
const DEFAULT_MAX_ZOOM = 400;

/**
 * Maximum safe canvas dimension (to avoid browser limits).
 */
const MAX_CANVAS_DIMENSION = 16384;

/**
 * Calculates the fit level to display an image within a viewport.
 *
 * @param imageWidth - Original image width
 * @param imageHeight - Original image height
 * @param viewportWidth - Viewport width
 * @param viewportHeight - Viewport height
 * @param minZoom - Minimum zoom level (default: 25)
 * @returns Fit level as integer percentage (e.g., 35 for 35%)
 */
export function calculateFitLevel(
  imageWidth: number,
  imageHeight: number,
  viewportWidth: number,
  viewportHeight: number,
  minZoom: number = DEFAULT_MIN_ZOOM,
): number {
  // Guard against invalid dimensions
  if (imageWidth <= 0 || imageHeight <= 0) {
    return 100; // Return 100% for invalid image dimensions
  }
  if (viewportWidth <= 0 || viewportHeight <= 0) {
    return minZoom; // Return minimum zoom for invalid viewport
  }

  // Apply viewport padding
  const effectiveWidth = viewportWidth * VIEWPORT_PADDING;
  const effectiveHeight = viewportHeight * VIEWPORT_PADDING;

  // Calculate scale factors
  const scaleX = effectiveWidth / imageWidth;
  const scaleY = effectiveHeight / imageHeight;

  // Use smaller scale to ensure image fits
  const scale = Math.min(scaleX, scaleY);

  // Convert to percentage and round down
  let fitLevel = Math.floor(scale * 100);

  // Don't upscale small images beyond 100%
  if (fitLevel > 100) {
    fitLevel = 100;
  }

  // Clamp to minimum zoom
  if (fitLevel < minZoom) {
    fitLevel = minZoom;
  }

  return fitLevel;
}

/**
 * CSS styles for the image viewer overlay.
 */
const STYLES = `
.image-viewer-overlay {
  position: fixed;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  background: rgba(0, 0, 0, 0.95);
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  opacity: 0;
  visibility: hidden;
  transition: opacity 0.15s ease, visibility 0.15s ease;
  z-index: 10000;
}

.image-viewer-overlay.visible {
  opacity: 1;
  visibility: visible;
}

.image-viewer-canvas {
  /* Remove max-width/height to allow size-based zoom */
  /* Size is controlled via width/height style attributes */
  image-rendering: pixelated;
  /* Transition for smooth zoom changes */
  transition: width 0.1s ease, height 0.1s ease;
}

.image-viewer-info {
  position: absolute;
  bottom: 20px;
  left: 50%;
  transform: translateX(-50%);
  color: rgba(255, 255, 255, 0.7);
  font-family: monospace;
  font-size: 12px;
  user-select: none;
}
`;

/**
 * Animation frame data for GIF/APNG playback.
 */
interface AnimationFrame {
  bitmap: ImageBitmap | null;
  delayMs: number;
}

/**
 * Fullscreen image viewer component.
 */
export class ImageViewer {
  private container: HTMLElement;
  private overlay: HTMLElement;
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;
  private infoElement: HTMLElement;
  private styleElement: HTMLStyleElement | null = null;

  private currentImage: DecodedImage | null = null;
  private currentBitmap: ImageBitmap | null = null;

  // Original image dimensions (for zoom calculations)
  private originalWidth = 0;
  private originalHeight = 0;

  // Current fit level (initial zoom to fit in viewport)
  private fitLevel = 100;

  // Animation state
  private animationFrames: Map<number, AnimationFrame> = new Map();
  private currentFrameIndex = 0;
  private animationTimerId: ReturnType<typeof setTimeout> | null = null;
  private animationState: AnimationState = "Stopped";

  // Bound event handlers for cleanup
  private boundHandleKeydown: (e: KeyboardEvent) => void;
  private boundHandleResize: () => void;

  // Resize throttling
  private resizeThrottleTimer: ReturnType<typeof setTimeout> | null = null;
  private readonly RESIZE_THROTTLE_MS = 100;

  // Zoom controller
  private zoomController: ZoomController | null = null;

  // Pan controller
  private panController: PanController | null = null;

  /**
   * Creates a new ImageViewer instance.
   *
   * @param container - Parent element (stored for reference, but overlay is appended to body)
   */
  constructor(container: HTMLElement) {
    this.container = container;

    // Inject styles
    this.injectStyles();

    // Create overlay structure
    this.overlay = document.createElement("div");
    this.overlay.className = "image-viewer-overlay";
    this.overlay.setAttribute("role", "dialog");
    this.overlay.setAttribute("aria-modal", "true");
    this.overlay.setAttribute("aria-label", "Image Viewer");

    // Create canvas
    this.canvas = document.createElement("canvas");
    this.canvas.className = "image-viewer-canvas";
    this.overlay.appendChild(this.canvas);

    const ctx = this.canvas.getContext("2d");
    if (!ctx) {
      throw new Error("Failed to get 2D context for image viewer canvas");
    }
    this.ctx = ctx;

    // Create info display
    this.infoElement = document.createElement("div");
    this.infoElement.className = "image-viewer-info";
    this.overlay.appendChild(this.infoElement);

    // Append to document.body (not container) to avoid being destroyed by terminal re-render
    // The terminal's forceRender() clears container.innerHTML, which would destroy the overlay
    document.body.appendChild(this.overlay);

    // Bind event handlers
    this.boundHandleKeydown = this.handleKeydown.bind(this);
    this.boundHandleResize = this.handleResize.bind(this);
    document.addEventListener("keydown", this.boundHandleKeydown, {
      capture: true,
    });
  }

  /**
   * Injects CSS styles into the document.
   */
  private injectStyles(): void {
    // Check if styles already exist
    const existingStyle = document.getElementById("image-viewer-styles");
    if (existingStyle) return;

    this.styleElement = document.createElement("style");
    this.styleElement.id = "image-viewer-styles";
    this.styleElement.textContent = STYLES;
    document.head.appendChild(this.styleElement);
  }

  /**
   * Shows an image in the fullscreen viewer.
   *
   * @param image - Decoded image to display
   */
  async show(image: DecodedImage): Promise<void> {
    this.currentImage = image;

    // Store original dimensions
    this.originalWidth = image.width;
    this.originalHeight = image.height;

    // Clear animation state
    this.stopAnimation();
    this.animationFrames.clear();
    this.currentFrameIndex = 0;

    // Decode base64 RGBA to ImageBitmap
    await this.decodeAndRender(image);

    // Show overlay first so we can get viewport dimensions
    this.overlay.classList.add("visible");

    // Calculate fit level based on viewport
    this.fitLevel = calculateFitLevel(
      image.width,
      image.height,
      this.overlay.clientWidth,
      this.overlay.clientHeight,
    );

    // Apply initial fit zoom
    this.applyImageZoom(this.fitLevel);

    // Update info display with fit percentage
    this.updateInfoDisplay(this.fitLevel);

    // Initialize pan controller
    this.panController = new PanController({
      canvas: this.canvas,
      overlay: this.overlay,
      onOffsetChange: (x, y) => {
        this.canvas.style.transform = `translate(${x}px, ${y}px)`;
      },
    });

    // Initialize zoom controller with custom callbacks
    this.zoomController = new ZoomController({
      container: this.canvas,
      overlay: this.overlay,
      initialLevel: this.fitLevel,
      onClose: () => this.hide(),
      onZoomChange: (level) => this.handleZoomChange(level),
      onReset: () => this.handleZoomReset(),
    });

    // Setup resize handler to recalculate fit level on window resize
    this.setupResizeHandler();

    console.log(
      `[LOG][FRONTEND] Image viewer opened: id=${image.id}, ${image.width}x${image.height}, fit=${this.fitLevel}%`,
    );
  }

  /**
   * Handles zoom level changes from ZoomController.
   *
   * @param level - New zoom level percentage
   */
  private handleZoomChange(level: number): void {
    this.applyImageZoom(level);
    this.updateInfoDisplay(level);

    // Reset pan offset when zoom changes
    this.panController?.reset();

    // Update pan controller with new canvas size
    const displayWidth = Math.round((this.originalWidth * level) / 100);
    const displayHeight = Math.round((this.originalHeight * level) / 100);
    this.panController?.updateCanvasSize(displayWidth, displayHeight);
  }

  /**
   * Sets up the window resize handler.
   */
  private setupResizeHandler(): void {
    window.addEventListener("resize", this.boundHandleResize);
  }

  /**
   * Removes the window resize handler.
   */
  private removeResizeHandler(): void {
    window.removeEventListener("resize", this.boundHandleResize);
  }

  /**
   * Handles window resize events.
   * Uses throttling to prevent performance issues during rapid resizing.
   */
  private handleResize(): void {
    if (!this.isVisible() || !this.currentImage) return;

    // Throttle resize handling
    if (this.resizeThrottleTimer !== null) {
      return;
    }

    this.resizeThrottleTimer = setTimeout(() => {
      this.resizeThrottleTimer = null;
      this.performResizeUpdate();
    }, this.RESIZE_THROTTLE_MS);
  }

  /**
   * Performs the actual resize update.
   * Called after throttle delay.
   */
  private performResizeUpdate(): void {
    if (!this.isVisible() || !this.currentImage) return;

    // Recalculate fit level based on new viewport size
    const newFitLevel = calculateFitLevel(
      this.originalWidth,
      this.originalHeight,
      this.overlay.clientWidth,
      this.overlay.clientHeight,
    );

    // Update fit level
    this.fitLevel = newFitLevel;

    // Update zoom controller's initial level for reset behavior
    if (this.zoomController) {
      // Get current zoom level from zoom controller
      const currentLevel = this.zoomController.getZoomLevel();

      // If current zoom is at or below the old fit level, adjust to new fit level
      // This ensures the image still fits after resize
      if (currentLevel <= newFitLevel) {
        this.zoomController.setZoomLevel(newFitLevel);
      }
    }

    // Update pan controller bounds with current canvas size
    if (this.panController) {
      const currentLevel = this.zoomController?.getZoomLevel() ?? this.fitLevel;
      const displayWidth = Math.round((this.originalWidth * currentLevel) / 100);
      const displayHeight = Math.round((this.originalHeight * currentLevel) / 100);
      this.panController.updateCanvasSize(displayWidth, displayHeight);
    }
  }

  /**
   * Handles zoom reset from ZoomController.
   * Called after ZoomController has already applied its initialLevel.
   * We override with current fitLevel to handle window resize correctly.
   */
  private handleZoomReset(): void {
    // Update ZoomController's level to current fitLevel if different
    // This handles the case where fitLevel changed due to window resize
    if (this.zoomController) {
      const currentLevel = this.zoomController.getZoomLevel();
      if (currentLevel !== this.fitLevel) {
        // Note: setZoomLevel will trigger onZoomChange which handles display
        this.zoomController.setZoomLevel(this.fitLevel);
        return; // onZoomChange already handled the display update
      }
    }

    // If already at fitLevel, just ensure pan is reset
    this.panController?.reset();
  }

  /**
   * Applies zoom level by setting canvas display dimensions.
   *
   * @param level - Zoom level percentage (100 = original size)
   */
  private applyImageZoom(level: number): void {
    // Guard against invalid zoom level
    if (level <= 0 || !Number.isFinite(level)) {
      return;
    }

    // Calculate display dimensions based on original size
    let displayWidth = Math.round((this.originalWidth * level) / 100);
    let displayHeight = Math.round((this.originalHeight * level) / 100);

    // Guard against invalid dimensions (e.g., if originalWidth/Height is 0)
    if (displayWidth <= 0 || displayHeight <= 0) {
      return;
    }

    // Clamp to safe canvas dimensions
    if (displayWidth > MAX_CANVAS_DIMENSION) {
      const ratio = MAX_CANVAS_DIMENSION / displayWidth;
      displayWidth = MAX_CANVAS_DIMENSION;
      displayHeight = Math.round(displayHeight * ratio);
    }
    if (displayHeight > MAX_CANVAS_DIMENSION) {
      const ratio = MAX_CANVAS_DIMENSION / displayHeight;
      displayHeight = MAX_CANVAS_DIMENSION;
      displayWidth = Math.round(displayWidth * ratio);
    }

    // Set display size via CSS (keeps canvas internal resolution)
    this.canvas.style.width = `${displayWidth}px`;
    this.canvas.style.height = `${displayHeight}px`;
  }

  /**
   * Updates the info display with current zoom level.
   *
   * @param level - Current zoom level percentage
   */
  private updateInfoDisplay(level: number): void {
    this.infoElement.textContent = `${this.originalWidth} x ${this.originalHeight} | ${level}% | Press Escape to close`;
  }

  /**
   * Decodes base64 RGBA data and renders to canvas.
   *
   * @param image - Decoded image with base64 RGBA data
   */
  private async decodeAndRender(image: DecodedImage): Promise<void> {
    try {
      // Decode base64 to binary
      const binaryString = atob(image.rgba_base64);
      const bytes = new Uint8Array(binaryString.length);
      for (let i = 0; i < binaryString.length; i++) {
        bytes[i] = binaryString.charCodeAt(i);
      }

      // Create ImageData from RGBA bytes
      const imageData = new ImageData(
        new Uint8ClampedArray(bytes.buffer),
        image.width,
        image.height,
      );

      // Create ImageBitmap for efficient rendering
      this.currentBitmap = await createImageBitmap(imageData);

      // Set canvas size to match image
      this.canvas.width = image.width;
      this.canvas.height = image.height;

      // Draw the image
      this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
      this.ctx.drawImage(this.currentBitmap, 0, 0);
    } catch (error) {
      console.error("[ERROR][FRONTEND] Failed to decode image:", error);
      // Show error state
      this.ctx.fillStyle = "#ff0000";
      this.ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);
      this.ctx.fillStyle = "#ffffff";
      this.ctx.font = "14px monospace";
      this.ctx.fillText("Failed to decode image", 10, 30);
    }
  }

  /**
   * Hides the viewer.
   */
  hide(): void {
    // Remove resize handler
    this.removeResizeHandler();

    // Clear resize throttle timer
    if (this.resizeThrottleTimer !== null) {
      clearTimeout(this.resizeThrottleTimer);
      this.resizeThrottleTimer = null;
    }

    // Dispose zoom controller
    if (this.zoomController) {
      this.zoomController.dispose();
      this.zoomController = null;
    }

    // Dispose pan controller
    if (this.panController) {
      this.panController.dispose();
      this.panController = null;
    }

    // Reset canvas transform
    this.canvas.style.transform = "";
    this.canvas.style.width = "";
    this.canvas.style.height = "";

    this.overlay.classList.remove("visible");
    this.stopAnimation();
    this.currentImage = null;

    console.log("[LOG][FRONTEND] Image viewer closed");
  }

  /**
   * Returns whether the viewer is currently visible.
   */
  isVisible(): boolean {
    return this.overlay.classList.contains("visible");
  }

  /**
   * Handles keyboard events.
   */
  private handleKeydown(e: KeyboardEvent): void {
    if (!this.isVisible()) return;

    // Block all keyboard input from reaching the shell while viewer is active
    // Both preventDefault() and stopPropagation() are needed:
    // - preventDefault() prevents default browser behavior
    // - stopPropagation() prevents other listeners from receiving the event
    e.preventDefault();
    e.stopPropagation();

    switch (e.key) {
      case "Escape":
        this.hide();
        break;
      // Add more keys if needed (e.g., arrow keys for navigation)
    }
  }

  /**
   * Handles animation events from the backend.
   *
   * @param event - Animation event
   */
  handleAnimationEvent(event: AnimationEvent): void {
    switch (event.type) {
      case "FrameReady":
        this.handleFrameReady(event);
        break;
      case "StateChanged":
        this.handleStateChanged(event);
        break;
      case "Completed":
        this.stopAnimation();
        break;
    }
  }

  /**
   * Handles a frame ready event.
   */
  private async handleFrameReady(
    event: Extract<AnimationEvent, { type: "FrameReady" }>,
  ): Promise<void> {
    // Check if this frame is for the current image
    if (!this.currentImage || this.currentImage.id !== event.image_id) {
      return;
    }

    try {
      // Decode frame
      const binaryString = atob(event.rgba_base64);
      const bytes = new Uint8Array(binaryString.length);
      for (let i = 0; i < binaryString.length; i++) {
        bytes[i] = binaryString.charCodeAt(i);
      }

      const imageData = new ImageData(
        new Uint8ClampedArray(bytes.buffer),
        event.width,
        event.height,
      );

      const bitmap = await createImageBitmap(imageData);

      // Store frame
      this.animationFrames.set(event.frame_number, {
        bitmap,
        delayMs: event.delay_ms,
      });

      // Start animation if playing and this is the first frame
      if (this.animationState === "Playing" && this.animationTimerId === null) {
        this.playAnimation();
      }
    } catch (error) {
      console.error(
        "[ERROR][FRONTEND] Failed to decode animation frame:",
        error,
      );
    }
  }

  /**
   * Handles animation state change event.
   */
  private handleStateChanged(
    event: Extract<AnimationEvent, { type: "StateChanged" }>,
  ): void {
    this.animationState = event.state;

    switch (event.state) {
      case "Playing":
        if (this.animationFrames.size > 0) {
          this.playAnimation();
        }
        break;
      case "Stopped":
      case "Paused":
        this.stopAnimation();
        break;
    }
  }

  /**
   * Starts animation playback.
   */
  private playAnimation(): void {
    if (this.animationTimerId !== null) return;
    if (this.animationFrames.size === 0) return;

    const frameNumbers = Array.from(this.animationFrames.keys()).sort(
      (a, b) => a - b,
    );

    const renderNextFrame = (): void => {
      if (!this.isVisible() || this.animationState !== "Playing") {
        this.stopAnimation();
        return;
      }

      const frameNumber = frameNumbers[this.currentFrameIndex];
      if (frameNumber === undefined) {
        this.stopAnimation();
        return;
      }
      const frame = this.animationFrames.get(frameNumber);

      if (frame?.bitmap) {
        // Resize canvas if needed
        if (
          this.canvas.width !== frame.bitmap.width ||
          this.canvas.height !== frame.bitmap.height
        ) {
          this.canvas.width = frame.bitmap.width;
          this.canvas.height = frame.bitmap.height;
        }

        // Draw frame
        this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
        this.ctx.drawImage(frame.bitmap, 0, 0);

        // Schedule next frame
        this.currentFrameIndex =
          (this.currentFrameIndex + 1) % frameNumbers.length;
        this.animationTimerId = setTimeout(renderNextFrame, frame.delayMs);
      }
    };

    renderNextFrame();
  }

  /**
   * Stops animation playback.
   */
  private stopAnimation(): void {
    if (this.animationTimerId !== null) {
      clearTimeout(this.animationTimerId);
      this.animationTimerId = null;
    }
  }

  /**
   * Disposes the viewer and releases resources.
   */
  dispose(): void {
    // Remove resize handler
    this.removeResizeHandler();

    // Clear resize throttle timer
    if (this.resizeThrottleTimer !== null) {
      clearTimeout(this.resizeThrottleTimer);
      this.resizeThrottleTimer = null;
    }

    // Dispose zoom controller
    if (this.zoomController) {
      this.zoomController.dispose();
      this.zoomController = null;
    }

    // Dispose pan controller
    if (this.panController) {
      this.panController.dispose();
      this.panController = null;
    }

    // Stop animation
    this.stopAnimation();

    // Remove event listeners
    document.removeEventListener("keydown", this.boundHandleKeydown, {
      capture: true,
    });

    // Clean up ImageBitmaps
    if (this.currentBitmap) {
      this.currentBitmap.close();
      this.currentBitmap = null;
    }

    for (const frame of this.animationFrames.values()) {
      if (frame.bitmap) {
        frame.bitmap.close();
      }
    }
    this.animationFrames.clear();

    // Remove from DOM
    this.overlay.remove();

    // Remove styles if we created them
    if (this.styleElement) {
      this.styleElement.remove();
      this.styleElement = null;
    }

    console.log("[LOG][FRONTEND] Image viewer disposed");
  }
}
