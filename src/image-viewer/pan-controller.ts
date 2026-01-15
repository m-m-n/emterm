/**
 * Pan controller for image viewer.
 *
 * Handles mouse drag functionality for panning images that exceed the viewport.
 *
 * @module image-viewer/pan-controller
 */

/**
 * Pan bounds defining the allowed range of offsets.
 */
export interface PanBounds {
  minX: number;
  maxX: number;
  minY: number;
  maxY: number;
}

/**
 * Offset position.
 */
export interface PanOffset {
  x: number;
  y: number;
}

/**
 * Options for PanController initialization.
 */
export interface PanControllerOptions {
  /** Canvas element to pan */
  canvas: HTMLCanvasElement;
  /** Overlay container element */
  overlay: HTMLElement;
  /** Callback when offset changes */
  onOffsetChange?: (x: number, y: number) => void;
  /** Callback when dragging state changes */
  onDragStateChange?: (isDragging: boolean) => void;
}

/**
 * PanController class.
 *
 * Manages pan state and handles mouse drag events for image panning.
 */
export class PanController {
  private canvas: HTMLCanvasElement;
  private overlay: HTMLElement;
  private onOffsetChange?: (x: number, y: number) => void;
  private onDragStateChange?: (isDragging: boolean) => void;

  private offset: PanOffset = { x: 0, y: 0 };
  private bounds: PanBounds = { minX: 0, maxX: 0, minY: 0, maxY: 0 };
  private dragging = false;
  private startPosition = { x: 0, y: 0 };
  private startOffset = { x: 0, y: 0 };

  // Track canvas size separately for bounds calculation
  private canvasWidth: number;
  private canvasHeight: number;

  // Bound event handlers for cleanup
  private boundMouseDown: (e: MouseEvent) => void;
  private boundMouseMove: (e: MouseEvent) => void;
  private boundMouseUp: (e: MouseEvent) => void;

  /**
   * Creates a new PanController.
   *
   * @param options - Controller options
   */
  constructor(options: PanControllerOptions) {
    this.canvas = options.canvas;
    this.overlay = options.overlay;
    this.onOffsetChange = options.onOffsetChange;
    this.onDragStateChange = options.onDragStateChange;

    this.canvasWidth = this.canvas.width;
    this.canvasHeight = this.canvas.height;

    // Bind event handlers
    this.boundMouseDown = this.handleMouseDown.bind(this);
    this.boundMouseMove = this.handleMouseMove.bind(this);
    this.boundMouseUp = this.handleMouseUp.bind(this);

    // Calculate initial bounds
    this.calculateBounds();

    // Set up event listeners
    this.setupEventListeners();

    // Update cursor
    this.updateCursor();
  }

  /**
   * Checks if panning is currently possible.
   * Pan is possible when canvas dimensions exceed viewport.
   */
  canPan(): boolean {
    const viewportWidth = this.overlay.clientWidth;
    const viewportHeight = this.overlay.clientHeight;

    return (
      this.canvasWidth > viewportWidth || this.canvasHeight > viewportHeight
    );
  }

  /**
   * Returns the current offset.
   */
  getOffset(): PanOffset {
    return { ...this.offset };
  }

  /**
   * Returns the current bounds.
   */
  getBounds(): PanBounds {
    return { ...this.bounds };
  }

  /**
   * Sets the offset, clamping to bounds.
   *
   * @param x - X offset
   * @param y - Y offset
   */
  setOffset(x: number, y: number): void {
    const clampedX = Math.max(this.bounds.minX, Math.min(x, this.bounds.maxX));
    const clampedY = Math.max(this.bounds.minY, Math.min(y, this.bounds.maxY));

    this.offset.x = clampedX;
    this.offset.y = clampedY;

    this.onOffsetChange?.(clampedX, clampedY);
  }

  /**
   * Resets the offset to zero.
   */
  reset(): void {
    this.setOffset(0, 0);
  }

  /**
   * Updates the canvas size and recalculates bounds.
   *
   * @param width - New canvas width
   * @param height - New canvas height
   */
  updateCanvasSize(width: number, height: number): void {
    // Guard against invalid dimensions
    if (width <= 0 || height <= 0) {
      return;
    }

    this.canvasWidth = width;
    this.canvasHeight = height;
    this.calculateBounds();

    // If panning is no longer possible, reset offset
    if (!this.canPan()) {
      this.reset();
    } else {
      // Re-clamp current offset to new bounds
      this.setOffset(this.offset.x, this.offset.y);
    }

    this.updateCursor();
  }

  /**
   * Returns whether currently dragging.
   */
  isDragging(): boolean {
    return this.dragging;
  }

  /**
   * Disposes the controller and releases resources.
   */
  dispose(): void {
    this.removeEventListeners();
  }

  /**
   * Calculates the pan bounds based on canvas and viewport sizes.
   */
  private calculateBounds(): void {
    const viewportWidth = this.overlay.clientWidth;
    const viewportHeight = this.overlay.clientHeight;

    // Calculate how much the canvas exceeds the viewport
    const excessWidth = Math.max(0, this.canvasWidth - viewportWidth);
    const excessHeight = Math.max(0, this.canvasHeight - viewportHeight);

    // Max pan is half the excess (centered image can move in both directions)
    // Use || 0 to convert -0 to 0
    this.bounds = {
      minX: -excessWidth / 2 || 0,
      maxX: excessWidth / 2 || 0,
      minY: -excessHeight / 2 || 0,
      maxY: excessHeight / 2 || 0,
    };
  }

  /**
   * Sets up event listeners for mouse drag.
   */
  private setupEventListeners(): void {
    this.canvas.addEventListener("mousedown", this.boundMouseDown);
    document.addEventListener("mousemove", this.boundMouseMove);
    document.addEventListener("mouseup", this.boundMouseUp);
  }

  /**
   * Removes event listeners.
   */
  private removeEventListeners(): void {
    this.canvas.removeEventListener("mousedown", this.boundMouseDown);
    document.removeEventListener("mousemove", this.boundMouseMove);
    document.removeEventListener("mouseup", this.boundMouseUp);
  }

  /**
   * Handles mouse down event to start dragging.
   */
  private handleMouseDown(e: MouseEvent): void {
    if (!this.canPan()) return;

    e.preventDefault();
    this.dragging = true;
    this.startPosition = { x: e.clientX, y: e.clientY };
    this.startOffset = { ...this.offset };
    this.updateCursor();
    this.onDragStateChange?.(true);
  }

  /**
   * Handles mouse move event during dragging.
   */
  private handleMouseMove(e: MouseEvent): void {
    if (!this.dragging) return;

    const deltaX = e.clientX - this.startPosition.x;
    const deltaY = e.clientY - this.startPosition.y;

    this.setOffset(this.startOffset.x + deltaX, this.startOffset.y + deltaY);
  }

  /**
   * Handles mouse up event to stop dragging.
   */
  private handleMouseUp(): void {
    if (!this.dragging) return;

    this.dragging = false;
    this.updateCursor();
    this.onDragStateChange?.(false);
  }

  /**
   * Updates the cursor style based on pan state.
   */
  private updateCursor(): void {
    if (this.canPan()) {
      this.canvas.style.cursor = this.dragging ? "grabbing" : "grab";
    } else {
      this.canvas.style.cursor = "default";
    }
  }
}
