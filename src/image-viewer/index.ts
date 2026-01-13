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
  max-width: 95%;
  max-height: 95%;
  object-fit: contain;
  image-rendering: pixelated;
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

  // Animation state
  private animationFrames: Map<number, AnimationFrame> = new Map();
  private currentFrameIndex = 0;
  private animationTimerId: ReturnType<typeof setTimeout> | null = null;
  private animationState: AnimationState = "Stopped";

  // Bound event handlers for cleanup
  private boundHandleKeydown: (e: KeyboardEvent) => void;

  /**
   * Creates a new ImageViewer instance.
   *
   * @param container - Parent element to attach the viewer to
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

    // Append to container
    this.container.appendChild(this.overlay);

    // Bind event handlers
    this.boundHandleKeydown = this.handleKeydown.bind(this);
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

    // Clear animation state
    this.stopAnimation();
    this.animationFrames.clear();
    this.currentFrameIndex = 0;

    // Decode base64 RGBA to ImageBitmap
    await this.decodeAndRender(image);

    // Update info display
    this.infoElement.textContent = `${image.width} x ${image.height} | Press Escape to close`;

    // Show overlay
    this.overlay.classList.add("visible");

    console.log(
      `[LOG][FRONTEND] Image viewer opened: id=${image.id}, ${image.width}x${image.height}`,
    );
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
    e.preventDefault();

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
