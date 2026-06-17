/**
 * Fullscreen Image Viewer.
 *
 * Displays images in a fullscreen overlay with two display modes:
 * - Pixel Perfect (100%): Shows image at original size
 * - Fit to Window: Scales image to fit within viewport
 *
 * @module image-viewer/index
 */

import type {
  DecodedImage,
  AnimationEvent,
  AnimationState,
} from "../image/types.ts";
import { decodeBase64ToBytes } from "../image/utils.ts";
import { DisplayModeController } from "./display-mode.ts";
import type { DisplayModeState } from "./display-mode.ts";
import { PanController } from "./pan-controller.ts";
import { t } from "../i18n/index.ts";

/**
 * Viewport padding factor (95% of viewport).
 * Exported for backward compatibility with existing tests.
 */
const VIEWPORT_PADDING = 0.95;

/**
 * Default zoom constraints.
 * Kept for backward compatibility.
 */
const DEFAULT_MIN_ZOOM = 25;

/**
 * Calculates the fit level to display an image within a viewport.
 * Exported for backward compatibility with existing tests.
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
 * Note: position: absolute is used to render within the container (overlay-root)
 * instead of covering the entire viewport. This allows the tab bar to remain
 * accessible during viewer display.
 */
const STYLES = `
.image-viewer-overlay {
  position: absolute;
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
  z-index: 1000;
  outline: none;
}

.image-viewer-overlay.visible {
  opacity: 1;
  visibility: visible;
}

.image-viewer-canvas {
  /* Use browser default interpolation (bilinear/bicubic).
     DPR-scaled canvas buffer provides pixel-perfect rendering without
     needing image-rendering: pixelated, which causes nearest-neighbor
     artifacts when combined with CSS transform compositor layers. */
  /* Use transform for zoom - smoother and better cross-browser support */
  transition: transform 0.1s ease;
  /* Prevent flexbox from shrinking canvas when larger than viewport */
  flex-shrink: 0;
  /* Override external CSS max-width/max-height constraints.
     Sizing is handled entirely via CSS transform by DisplayModeController. */
  max-width: none;
  max-height: none;
}

.image-viewer-info {
  position: absolute;
  bottom: 20px;
  left: 50%;
  transform: translateX(-50%);
  color: rgba(255, 255, 255, 0.7);
  font-family: monospace, 'Apple Color Emoji', 'Segoe UI Emoji', 'Segoe UI Symbol', 'Noto Color Emoji';
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
 *
 * Features:
 * - Two display modes: Pixel Perfect (100%) and Fit to Window
 * - Keyboard shortcuts: 'f' toggle, Escape close
 * - Drag pan and wheel scroll for large images in pixel mode
 * - Animated image support (GIF/APNG)
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

  // Original image dimensions
  private originalWidth = 0;
  private originalHeight = 0;

  // Current scale (for transform-based display)
  // Using separate X/Y scales to correct aspect ratio distortion from flexbox
  private currentScaleX = 1;
  private currentScaleY = 1;

  // Current pan offset (for combined transform)
  private panOffsetX = 0;
  private panOffsetY = 0;

  // Constrained base size (actual rendered size before transform)
  // This may differ from originalWidth/Height due to flexbox constraints
  private constrainedBaseWidth = 0;
  private constrainedBaseHeight = 0;

  // Animation state
  private animationFrames: Map<number, AnimationFrame> = new Map();
  private currentFrameIndex = 0;
  private animationTimerId: ReturnType<typeof setTimeout> | null = null;
  private animationState: AnimationState = "Stopped";

  // Bound event handlers for cleanup
  private boundHandleResize: () => void;
  private boundHandleWheel: (e: WheelEvent) => void;

  // Resize throttling
  private resizeThrottleTimer: ReturnType<typeof setTimeout> | null = null;
  private readonly RESIZE_THROTTLE_MS = 100;

  // Display mode controller (replaces ZoomController for ImageViewer)
  private displayModeController: DisplayModeController | null = null;

  // Pan controller
  private panController: PanController | null = null;

  // Callback invoked after the viewer is hidden
  private onHideCallback: (() => void) | null = null;

  // Callback invoked before the viewer is shown
  private onShowCallback: (() => void) | null = null;

  // Generation counter to cancel stale async show() operations
  private showGeneration = 0;

  /**
   * Creates a new ImageViewer instance.
   *
   * @param container - Parent element (overlay-root) to append the overlay to.
   *                    This should be the overlay-root container within each tab,
   *                    which is separate from terminal-root to prevent viewer
   *                    operations from affecting terminal content.
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
    this.overlay.setAttribute("aria-label", t("imageViewer.label"));
    this.overlay.tabIndex = -1; // Allow programmatic focus

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

    // Append to container (overlay-root) instead of document.body
    // The container separation (terminal-root vs overlay-root) prevents viewer
    // operations from destroying terminal content during forceRender()
    this.container.appendChild(this.overlay);

    // Bind event handlers
    this.boundHandleResize = this.handleResize.bind(this);
    this.boundHandleWheel = this.handleWheel.bind(this);
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
    // Increment generation to cancel any in-flight show() operations
    const generation = ++this.showGeneration;

    this.currentImage = image;

    // Store original dimensions
    this.originalWidth = image.width;
    this.originalHeight = image.height;

    // Clear animation state
    this.stopAnimation();
    this.animationFrames.clear();
    this.currentFrameIndex = 0;

    // Release previous bitmap to prevent resource leak
    if (this.currentBitmap) {
      this.currentBitmap.close();
      this.currentBitmap = null;
    }

    // Decode base64 RGBA to ImageBitmap
    await this.decodeAndRender(image);

    // If hide() was called or another show() started during decode, abort
    if (this.showGeneration !== generation) {
      return;
    }

    // Show overlay first so we can get viewport dimensions
    this.overlay.classList.add("visible");

    // Notify show callback (e.g., blur IME input to prevent key interception)
    this.onShowCallback?.();

    // Focus the overlay to ensure keydown events have the correct event target.
    // This guarantees capture-phase handlers on document fire before bubble-phase handlers,
    // allowing DisplayModeController to intercept keys before KeyboardHandler.
    this.overlay.focus();

    // Measure constrained base size (actual rendered size before transform)
    // This captures any flexbox constraints applied by the browser
    this.measureConstrainedBaseSize();

    // Initialize display mode controller with pixel mode (100%)
    this.displayModeController = new DisplayModeController({
      imageWidth: image.width,
      imageHeight: image.height,
      viewportWidth: this.overlay.clientWidth,
      viewportHeight: this.overlay.clientHeight,
      overlay: this.overlay,
      initialMode: "pixel",
      onModeChange: (state) => this.handleModeChange(state),
      onClose: () => this.hide(),
      onScroll: (deltaY) => {
        if (!this.panController?.canPan()) return;
        const offset = this.panController.getOffset();
        this.panController.setOffset(offset.x, offset.y - deltaY);
      },
    });

    // Apply initial pixel mode (100%)
    this.applyScale(1.0);
    this.updateInfoDisplay("pixel");

    // Initialize pan controller
    this.panController = new PanController({
      canvas: this.canvas,
      overlay: this.overlay,
      onOffsetChange: (x, y) => {
        this.panOffsetX = x;
        this.panOffsetY = y;
        this.applyTransform();
      },
    });

    // Update pan state based on initial mode
    this.updatePanState();

    // Setup wheel scroll handler
    this.overlay.addEventListener("wheel", this.boundHandleWheel, { passive: false });

    // Setup resize handler to recalculate fit scale on window resize
    this.setupResizeHandler();
  }

  /**
   * Handles mode changes from DisplayModeController.
   *
   * @param state - New display mode state
   */
  private handleModeChange(state: DisplayModeState): void {
    // Apply the new scale
    this.applyScale(state.scale);

    // Update info display
    this.updateInfoDisplay(state.mode);

    // Reset pan offset when mode changes
    this.panOffsetX = 0;
    this.panOffsetY = 0;
    this.panController?.reset();

    // Update pan state based on new mode
    this.updatePanState();
  }

  /**
   * Updates the pan controller state based on current mode.
   * Pan is enabled only in pixel mode when image exceeds viewport.
   */
  private updatePanState(): void {
    if (!this.panController || !this.displayModeController) return;

    const state = this.displayModeController.getState();

    // Calculate display size
    const displayWidth = Math.round(this.originalWidth * state.scale);
    const displayHeight = Math.round(this.originalHeight * state.scale);

    // Update pan controller with current display size
    this.panController.updateCanvasSize(displayWidth, displayHeight);
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

    // Update display mode controller with new viewport dimensions
    if (this.displayModeController) {
      this.displayModeController.updateViewport(
        this.overlay.clientWidth,
        this.overlay.clientHeight,
      );
    }

    // Update pan bounds
    this.updatePanState();
  }

  /**
   * Measures the constrained base size of the canvas.
   * This captures the actual rendered size after flexbox constraints are applied.
   * Must be called when canvas is visible but before any transform is applied.
   */
  private measureConstrainedBaseSize(): void {
    // Temporarily remove any transform to get the base size
    const savedTransform = this.canvas.style.transform;
    this.canvas.style.transform = "";

    const rect = this.canvas.getBoundingClientRect();
    this.constrainedBaseWidth = rect.width;
    this.constrainedBaseHeight = rect.height;

    // Restore transform
    this.canvas.style.transform = savedTransform;
  }

  /**
   * Applies the combined transform (translate + scale) to the canvas.
   * Uses separate X/Y scale factors to correct aspect ratio distortion.
   */
  private applyTransform(): void {
    this.canvas.style.transform =
      `translate(${this.panOffsetX}px, ${this.panOffsetY}px) ` +
      `scale(${this.currentScaleX}, ${this.currentScaleY})`;
  }

  /**
   * Applies scale using CSS transform.
   * Uses separate X/Y correction factors to fix aspect ratio distortion from flexbox.
   *
   * @param scale - Scale value (1.0 = 100%)
   */
  private applyScale(scale: number): void {
    // Guard against invalid scale
    if (scale <= 0 || !Number.isFinite(scale)) {
      return;
    }

    // Guard against invalid base size
    if (
      this.constrainedBaseWidth <= 0 ||
      this.constrainedBaseHeight <= 0 ||
      this.originalWidth <= 0 ||
      this.originalHeight <= 0
    ) {
      this.currentScaleX = scale;
      this.currentScaleY = scale;
      this.applyTransform();
      return;
    }

    // Calculate separate correction factors for X and Y
    // This fixes aspect ratio distortion caused by flexbox constraints
    const correctionX = this.originalWidth / this.constrainedBaseWidth;
    const correctionY = this.originalHeight / this.constrainedBaseHeight;

    // Use uniform correction to maintain aspect ratio
    const correction = Math.max(correctionX, correctionY);

    // Apply correction with scale
    this.currentScaleX = correction * scale;
    this.currentScaleY = correction * scale;

    // Apply combined transform (scale + any existing pan offset)
    this.applyTransform();
  }

  /**
   * Updates the info display with current mode.
   *
   * @param mode - Current display mode ('pixel' or 'fit')
   */
  private updateInfoDisplay(mode: string): void {
    const modeText = mode === "pixel" ? "100%" : t("imageViewer.modeFit");
    const helpText = t("imageViewer.helpText");
    this.infoElement.textContent = t("imageViewer.info", {
      width: this.originalWidth,
      height: this.originalHeight,
      mode: modeText,
      help: helpText,
    });
  }

  /**
   * Decodes base64 RGBA data and renders to canvas.
   *
   * @param image - Decoded image with base64 RGBA data
   */
  private async decodeAndRender(image: DecodedImage): Promise<void> {
    try {
      const expectedSize = image.width * image.height * 4;

      // Validate base64 data
      if (!image.rgba_base64 || image.rgba_base64.length === 0) {
        throw new Error("No rgba_base64 data received");
      }

      // Decode base64 to binary
      const bytes = decodeBase64ToBytes(image.rgba_base64);

      if (bytes.length !== expectedSize) {
        throw new Error(
          `Size mismatch: decoded ${bytes.length} bytes, expected ${expectedSize}`,
        );
      }

      // Create ImageData from RGBA bytes
      const imageData = new ImageData(
        bytes,
        image.width,
        image.height,
      );

      // Create ImageBitmap for efficient rendering
      this.currentBitmap = await createImageBitmap(imageData);

      // Set canvas buffer size with DPR scaling for crisp HiDPI rendering
      const dpr = window.devicePixelRatio || 1;
      this.canvas.width = Math.round(image.width * dpr);
      this.canvas.height = Math.round(image.height * dpr);

      // Set canvas CSS size to match logical image dimensions
      // This is the base size that transform: scale() will operate on
      this.canvas.style.width = `${image.width}px`;
      this.canvas.style.height = `${image.height}px`;

      // Draw the image at DPR-scaled resolution
      // Use actual buffer/logical ratio to avoid fractional DPR rounding mismatch
      const scaleX = this.canvas.width / image.width;
      const scaleY = this.canvas.height / image.height;
      this.ctx.save();
      this.ctx.setTransform(scaleX, 0, 0, scaleY, 0, 0);
      this.ctx.clearRect(0, 0, image.width, image.height);
      this.ctx.drawImage(this.currentBitmap, 0, 0);
      this.ctx.restore();
    } catch (error) {
      console.error("[ERROR][FRONTEND] Failed to decode image:", error);
      // Show error state
      this.ctx.fillStyle = "#ff0000";
      this.ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);
      this.ctx.fillStyle = "#ffffff";
      this.ctx.font = "14px monospace";
      this.ctx.fillText(t("imageViewer.decodeError"), 10, 30);
    }
  }

  /**
   * Hides the viewer.
   */
  hide(): void {
    // Invalidate any in-flight async show() operations
    this.showGeneration++;

    // Remove event handlers
    this.removeResizeHandler();
    this.overlay.removeEventListener("wheel", this.boundHandleWheel);

    // Clear resize throttle timer
    if (this.resizeThrottleTimer !== null) {
      clearTimeout(this.resizeThrottleTimer);
      this.resizeThrottleTimer = null;
    }

    // Dispose display mode controller
    if (this.displayModeController) {
      this.displayModeController.dispose();
      this.displayModeController = null;
    }

    // Dispose pan controller
    if (this.panController) {
      this.panController.dispose();
      this.panController = null;
    }

    // Reset transform state
    this.currentScaleX = 1;
    this.currentScaleY = 1;
    this.panOffsetX = 0;
    this.panOffsetY = 0;
    this.constrainedBaseWidth = 0;
    this.constrainedBaseHeight = 0;

    // Reset canvas transform
    this.canvas.style.transform = "";

    this.overlay.classList.remove("visible");
    this.stopAnimation();
    this.currentImage = null;

    // Release bitmap to prevent resource leak
    if (this.currentBitmap) {
      this.currentBitmap.close();
      this.currentBitmap = null;
    }

    this.onHideCallback?.();
  }

  /**
   * Sets a callback to be invoked before the viewer is shown.
   */
  onShow(callback: () => void): void {
    this.onShowCallback = callback;
  }

  /**
   * Sets a callback to be invoked after the viewer is hidden.
   */
  onHide(callback: () => void): void {
    this.onHideCallback = callback;
  }

  /**
   * Returns whether the viewer is currently visible.
   */
  isVisible(): boolean {
    return this.overlay.classList.contains("visible");
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
      const bytes = decodeBase64ToBytes(event.rgba_base64);

      const imageData = new ImageData(
        bytes,
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

    // Cache DPR at animation start to avoid per-frame property access
    const dpr = window.devicePixelRatio || 1;

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
        // Resize canvas if needed (with DPR scaling)
        const bufferWidth = Math.round(frame.bitmap.width * dpr);
        const bufferHeight = Math.round(frame.bitmap.height * dpr);
        if (
          this.canvas.width !== bufferWidth ||
          this.canvas.height !== bufferHeight
        ) {
          this.canvas.width = bufferWidth;
          this.canvas.height = bufferHeight;
          this.canvas.style.width = `${frame.bitmap.width}px`;
          this.canvas.style.height = `${frame.bitmap.height}px`;
        }

        // Draw frame at DPR-scaled resolution
        // Use actual buffer/logical ratio to avoid fractional DPR rounding mismatch
        const scaleX = this.canvas.width / frame.bitmap.width;
        const scaleY = this.canvas.height / frame.bitmap.height;
        this.ctx.save();
        this.ctx.setTransform(scaleX, 0, 0, scaleY, 0, 0);
        this.ctx.clearRect(0, 0, frame.bitmap.width, frame.bitmap.height);
        this.ctx.drawImage(frame.bitmap, 0, 0);
        this.ctx.restore();

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
   * Handles wheel events for scrolling large images.
   */
  private handleWheel(e: WheelEvent): void {
    e.preventDefault();

    // Ctrl+Wheel: block browser zoom only (reserved for future zoom)
    if (e.ctrlKey) return;

    // Ignore if panning is not possible (fit mode or image within viewport)
    if (!this.panController?.canPan()) return;

    const offset = this.panController.getOffset();

    if (e.shiftKey) {
      // Shift+Wheel: horizontal scroll
      // Some OSes put wheel value in deltaX when shiftKey is true
      const delta = e.deltaX !== 0 ? e.deltaX : e.deltaY;
      this.panController.setOffset(offset.x - delta, offset.y);
    } else {
      // Normal: vertical scroll
      this.panController.setOffset(offset.x, offset.y - e.deltaY);
    }
  }

  /**
   * Disposes the viewer and releases resources.
   */
  dispose(): void {
    // Remove event handlers
    this.removeResizeHandler();
    this.overlay.removeEventListener("wheel", this.boundHandleWheel);

    // Clear resize throttle timer
    if (this.resizeThrottleTimer !== null) {
      clearTimeout(this.resizeThrottleTimer);
      this.resizeThrottleTimer = null;
    }

    // Dispose display mode controller
    if (this.displayModeController) {
      this.displayModeController.dispose();
      this.displayModeController = null;
    }

    // Dispose pan controller
    if (this.panController) {
      this.panController.dispose();
      this.panController = null;
    }

    // Stop animation
    this.stopAnimation();

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
  }
}
