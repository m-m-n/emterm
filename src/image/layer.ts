/**
 * Image rendering layer for terminal.
 *
 * Renders images on a Canvas element positioned behind the terminal text.
 * Supports both static images and animations.
 *
 * Phase 4 additions:
 * - WebGL-accelerated rendering with Canvas 2D fallback
 * - Bitmap caching for scaled images
 * - Debounced resize handling
 * - Performance monitoring
 * - Progressive loading for large images
 *
 * @module image/layer
 */

import type {
  DecodedImage,
  ImagePlacement,
  ImageDeleteTarget,
  StoredImage,
  ActivePlacement,
  AnimationEvent,
  ImageLayerOptions,
  RenderBackend,
  RenderStats,
  ProgressiveImage,
} from "./types.ts";
import { AnimationController } from "./animation.ts";
import { WebGLLayer, isWebGLSupported } from "./webgl-layer.ts";
import { BitmapCache } from "./cache.ts";
import { ResizeHandler } from "./resize-handler.ts";
import { PerformanceMonitor } from "./performance.ts";

/**
 * Default configuration values.
 */
const DEFAULT_OPTIONS: Required<ImageLayerOptions> = {
  preferredBackend: "webgl",
  enableCache: true,
  maxCacheEntries: 100,
  maxCacheMemoryBytes: 50 * 1024 * 1024,
  resizeDebounceMs: 100,
  enablePerformanceMonitoring: true,
  progressiveLoadingThreshold: 1024 * 1024, // 1MB
};

/**
 * Image layer for rendering inline images in terminal.
 *
 * Creates and manages a Canvas element that renders images behind
 * the terminal text layer. Supports WebGL acceleration with Canvas 2D fallback.
 */
export class ImageLayer {
  /** Canvas element for image rendering (Canvas 2D mode). */
  private canvas: HTMLCanvasElement | null = null;

  /** 2D rendering context (Canvas 2D mode). */
  private ctx: CanvasRenderingContext2D | null = null;

  /** WebGL layer (WebGL mode). */
  private webglLayer: WebGLLayer | null = null;

  /** Active render backend. */
  private activeBackend: RenderBackend;

  /** Whether WebGL is available. */
  private webglAvailable: boolean;

  /** Stored images by ID. */
  private images: Map<number, StoredImage> = new Map();

  /** Active placements by unique key (imageId:placementId). */
  private placements: Map<string, ActivePlacement> = new Map();

  /** Animation controller for animated images. */
  private animationController: AnimationController;

  /** Map of image IDs to current animation bitmap (for rendering). */
  private animationBitmaps: Map<number, ImageBitmap> = new Map();

  /** Bitmap cache for scaled images. */
  private cache: BitmapCache | null = null;

  /** Resize handler with debounce. */
  private resizeHandler: ResizeHandler;

  /** Performance monitor. */
  private performanceMonitor: PerformanceMonitor;

  /** Progressive loading state by image ID. */
  private progressiveImages: Map<number, ProgressiveImage> = new Map();

  /** Progressive loading threshold in bytes. */
  private progressiveLoadingThreshold: number;

  /** Container element. */
  private container: HTMLElement;

  /** Character cell dimensions. */
  private charWidth: number = 8;
  private charHeight: number = 16;

  /** Terminal dimensions in cells. */
  private cols: number = 80;
  private rows: number = 24;

  /** Pixel offset for terminal padding. */
  private paddingX: number = 8;
  private paddingY: number = 8;

  /** Current canvas dimensions. */
  private canvasWidth: number = 0;
  private canvasHeight: number = 0;

  /**
   * Create a new image layer.
   *
   * @param container - Parent element to attach canvas to
   * @param options - Layer configuration options
   */
  constructor(container: HTMLElement, options: ImageLayerOptions = {}) {
    const opts = { ...DEFAULT_OPTIONS, ...options };

    this.container = container;
    this.progressiveLoadingThreshold = opts.progressiveLoadingThreshold;

    // Check WebGL availability
    this.webglAvailable = isWebGLSupported();

    // Determine backend
    if (opts.preferredBackend === "webgl" && this.webglAvailable) {
      this.activeBackend = "webgl";
      this.initWebGLBackend();
    } else {
      this.activeBackend = "canvas2d";
      this.initCanvas2DBackend();
    }

    // Initialize cache if enabled
    if (opts.enableCache) {
      this.cache = new BitmapCache({
        maxEntries: opts.maxCacheEntries,
        maxMemoryBytes: opts.maxCacheMemoryBytes,
      });
    }

    // Initialize resize handler
    this.resizeHandler = new ResizeHandler({
      debounceMs: opts.resizeDebounceMs,
    });
    this.resizeHandler.onResize(({ width, height }) => {
      this.handleResizeComplete(width, height);
    });

    // Initialize performance monitor
    this.performanceMonitor = new PerformanceMonitor({
      enabled: opts.enablePerformanceMonitoring,
    });
    this.performanceMonitor.setThreshold("frameTime", 16);

    // Initialize animation controller
    this.animationController = new AnimationController();
    this.animationController.setFrameUpdateCallback((imageId, bitmap) => {
      this.animationBitmaps.set(imageId, bitmap);
      this.render();
    });
    this.animationController.setAnimationCompleteCallback((imageId) => {
      console.debug(`Animation ${imageId} completed`);
    });

    // Ensure container has relative positioning
    const containerStyle = getComputedStyle(container);
    if (containerStyle.position === "static") {
      container.style.position = "relative";
    }

    this.updateCanvasSize();
  }

  /**
   * Initialize Canvas 2D backend.
   */
  private initCanvas2DBackend(): void {
    this.canvas = document.createElement("canvas");
    this.canvas.className = "terminal-image-layer";
    this.canvas.style.cssText = `
      position: absolute;
      top: 0;
      left: 0;
      pointer-events: none;
      z-index: -1;
    `;

    const ctx = this.canvas.getContext("2d");
    if (!ctx) {
      throw new Error("Failed to get 2D context");
    }
    this.ctx = ctx;

    // Insert canvas
    if (this.container.firstChild) {
      this.container.insertBefore(this.canvas, this.container.firstChild);
    } else {
      this.container.appendChild(this.canvas);
    }
  }

  /**
   * Initialize WebGL backend.
   */
  private initWebGLBackend(): void {
    try {
      this.webglLayer = new WebGLLayer(this.container);
      if (!this.webglLayer.isWebGLActive()) {
        // Fall back to Canvas 2D
        this.webglLayer.dispose();
        this.webglLayer = null;
        this.activeBackend = "canvas2d";
        this.initCanvas2DBackend();
      }
    } catch (e) {
      console.warn("WebGL initialization failed, falling back to Canvas 2D:", e);
      this.activeBackend = "canvas2d";
      this.initCanvas2DBackend();
    }
  }

  /**
   * Get the active render backend.
   *
   * @returns Active backend type
   */
  getActiveBackend(): RenderBackend {
    return this.activeBackend;
  }

  /**
   * Check if WebGL is available.
   *
   * @returns True if WebGL is supported
   */
  isWebGLAvailable(): boolean {
    return this.webglAvailable;
  }

  /**
   * Update character cell size.
   *
   * @param width - Character width in pixels
   * @param height - Character height in pixels
   */
  setCharSize(width: number, height: number): void {
    this.charWidth = width;
    this.charHeight = height;
    this.handleResize();
  }

  /**
   * Update terminal dimensions.
   *
   * @param cols - Number of columns
   * @param rows - Number of rows
   */
  setDimensions(cols: number, rows: number): void {
    this.cols = cols;
    this.rows = rows;
    this.handleResize();
  }

  /**
   * Update padding offset.
   *
   * @param x - Horizontal padding in pixels
   * @param y - Vertical padding in pixels
   */
  setPadding(x: number, y: number): void {
    this.paddingX = x;
    this.paddingY = y;
    this.render();
  }

  /**
   * Handle resize with debounce.
   */
  private handleResize(): void {
    const width = this.cols * this.charWidth + this.paddingX * 2;
    const height = this.rows * this.charHeight + this.paddingY * 2;
    this.resizeHandler.handleResize({ width, height });
  }

  /**
   * Handle resize complete (after debounce).
   */
  private handleResizeComplete(width: number, height: number): void {
    this.performanceMonitor.startMeasure("resize");

    this.canvasWidth = width;
    this.canvasHeight = height;
    this.updateCanvasSize();

    // Clear scaled bitmap cache on resize
    if (this.cache) {
      this.cache.clear();
    }

    this.render();

    this.performanceMonitor.endMeasure("resize");
  }

  /**
   * Update canvas size to match terminal.
   */
  private updateCanvasSize(): void {
    const width = this.cols * this.charWidth + this.paddingX * 2;
    const height = this.rows * this.charHeight + this.paddingY * 2;

    if (this.activeBackend === "webgl" && this.webglLayer) {
      this.webglLayer.setCanvasSize(width, height);
    } else if (this.canvas && this.ctx) {
      const dpr = window.devicePixelRatio || 1;

      this.canvas.width = width * dpr;
      this.canvas.height = height * dpr;
      this.canvas.style.width = `${width}px`;
      this.canvas.style.height = `${height}px`;

      this.ctx.scale(dpr, dpr);
    }
  }

  /**
   * Store a new image.
   *
   * @param image - Decoded image from backend
   */
  async addImage(image: DecodedImage): Promise<void> {
    this.performanceMonitor.startMeasure("decode");

    // Calculate data size
    const dataSize = image.rgba_base64.length * 0.75; // Approximate decoded size

    // Check if progressive loading is needed
    const useProgressiveLoading = dataSize > this.progressiveLoadingThreshold;

    // Decode base64 RGBA data
    const binaryString = atob(image.rgba_base64);
    const bytes = new Uint8ClampedArray(binaryString.length);
    for (let i = 0; i < binaryString.length; i++) {
      bytes[i] = binaryString.charCodeAt(i);
    }

    // Create ImageData
    const imageData = new ImageData(bytes, image.width, image.height);

    // Handle progressive loading for large images
    if (useProgressiveLoading) {
      await this.handleProgressiveLoading(image, imageData, dataSize);
    } else {
      // Create ImageBitmap for efficient rendering
      let bitmap: ImageBitmap | null = null;
      try {
        bitmap = await createImageBitmap(imageData);
      } catch (e) {
        console.warn("Failed to create ImageBitmap:", e);
      }

      this.images.set(image.id, {
        data: image,
        bitmap,
      });

      // Upload to WebGL if active
      if (this.activeBackend === "webgl" && this.webglLayer) {
        this.webglLayer.uploadTexture(image.id, bytes, image.width, image.height);
      }
    }

    this.performanceMonitor.endMeasure("decode");
  }

  /**
   * Handle progressive loading for large images.
   */
  private async handleProgressiveLoading(
    image: DecodedImage,
    fullImageData: ImageData,
    dataSize: number
  ): Promise<void> {
    // Create low-resolution preview (1/4 size)
    const previewWidth = Math.max(1, Math.floor(image.width / 4));
    const previewHeight = Math.max(1, Math.floor(image.height / 4));

    // Initialize progressive state
    const progressive: ProgressiveImage = {
      id: image.id,
      state: "pending",
      lowResPreview: null,
      fullResolution: null,
      width: image.width,
      height: image.height,
      dataSize,
    };
    this.progressiveImages.set(image.id, progressive);

    // Create low-res preview first (if image is large enough)
    if (image.width > 8 && image.height > 8) {
      try {
        // Create scaled preview using createImageBitmap with resize option
        const lowResBitmap = await createImageBitmap(fullImageData, {
          resizeWidth: previewWidth,
          resizeHeight: previewHeight,
          resizeQuality: "low",
        });
        progressive.lowResPreview = lowResBitmap;
        progressive.state = "low-resolution";

        // Store preview for immediate display
        this.images.set(image.id, {
          data: image,
          bitmap: lowResBitmap,
        });

        // Trigger render to show preview
        this.render();
      } catch (e) {
        console.warn("Failed to create low-res preview:", e);
      }
    }

    // Create full resolution bitmap (async)
    try {
      const fullBitmap = await createImageBitmap(fullImageData);
      progressive.fullResolution = fullBitmap;
      progressive.state = "complete";

      // Update stored image with full resolution
      this.images.set(image.id, {
        data: image,
        bitmap: fullBitmap,
      });

      // Close low-res preview if we have full resolution
      if (progressive.lowResPreview && progressive.lowResPreview !== fullBitmap) {
        progressive.lowResPreview.close();
        progressive.lowResPreview = null;
      }

      // Upload full resolution to WebGL
      if (this.activeBackend === "webgl" && this.webglLayer) {
        const binaryString = atob(image.rgba_base64);
        const bytes = new Uint8ClampedArray(binaryString.length);
        for (let i = 0; i < binaryString.length; i++) {
          bytes[i] = binaryString.charCodeAt(i);
        }
        this.webglLayer.uploadTexture(image.id, bytes, image.width, image.height);
      }

      // Trigger final render
      this.render();
    } catch (e) {
      console.warn("Failed to create full resolution bitmap:", e);
      progressive.state = "low-resolution"; // Fallback to low-res
    }
  }

  /**
   * Place an image at a position.
   *
   * @param placement - Placement specification
   */
  placeImage(placement: ImagePlacement): void {
    const stored = this.images.get(placement.image_id);
    if (!stored) {
      console.warn(`Image ${placement.image_id} not found for placement`);
      return;
    }

    // Calculate pixel position
    const x = this.paddingX + placement.col * this.charWidth + placement.x_offset;
    const y = this.paddingY + placement.row * this.charHeight + placement.y_offset;

    // Calculate display size
    let displayWidth: number;
    let displayHeight: number;

    if (placement.columns > 0 && placement.rows > 0) {
      displayWidth = placement.columns * this.charWidth;
      displayHeight = placement.rows * this.charHeight;
    } else if (placement.columns > 0) {
      displayWidth = placement.columns * this.charWidth;
      displayHeight = (stored.data.height / stored.data.width) * displayWidth;
    } else if (placement.rows > 0) {
      displayHeight = placement.rows * this.charHeight;
      displayWidth = (stored.data.width / stored.data.height) * displayHeight;
    } else {
      displayWidth = stored.data.width;
      displayHeight = stored.data.height;
    }

    const key = `${placement.image_id}:${placement.placement_id}`;
    const activePlacement: ActivePlacement = {
      placement,
      x,
      y,
      displayWidth,
      displayHeight,
    };
    this.placements.set(key, activePlacement);

    // Add placement to WebGL layer if active
    if (this.activeBackend === "webgl" && this.webglLayer) {
      this.webglLayer.addPlacement({
        textureId: placement.image_id,
        x,
        y,
        width: displayWidth,
        height: displayHeight,
        zIndex: placement.z_index,
        key,
      });
    }

    this.render();
  }

  /**
   * Delete images matching the target.
   *
   * @param target - Deletion target
   */
  deleteImages(target: ImageDeleteTarget): void {
    const deletedImageIds: number[] = [];

    switch (target.type) {
      case "All":
        this.placements.clear();
        if (this.webglLayer) this.webglLayer.clearPlacements();
        break;

      case "AllIncludingHidden":
        for (const imageId of this.images.keys()) {
          deletedImageIds.push(imageId);
        }
        this.placements.clear();
        this.images.clear();
        if (this.webglLayer) {
          this.webglLayer.clearPlacements();
          for (const id of deletedImageIds) {
            this.webglLayer.deleteTexture(id);
          }
        }
        break;

      case "ById":
        for (const [key, active] of this.placements) {
          if (active.placement.image_id === target.id) {
            this.placements.delete(key);
            if (this.webglLayer) this.webglLayer.removePlacement(key);
          }
        }
        this.images.delete(target.id);
        if (this.webglLayer) this.webglLayer.deleteTexture(target.id);
        deletedImageIds.push(target.id);
        break;

      case "ByPlacement":
        {
          const key = `${target.image_id}:${target.placement_id}`;
          this.placements.delete(key);
          if (this.webglLayer) this.webglLayer.removePlacement(key);
        }
        break;

      case "AtCursor":
        for (const [key, active] of this.placements) {
          if (
            active.placement.row === target.row &&
            active.placement.col === target.col
          ) {
            this.placements.delete(key);
            if (this.webglLayer) this.webglLayer.removePlacement(key);
          }
        }
        break;

      case "ByZIndex":
        for (const [key, active] of this.placements) {
          if (active.placement.z_index === target.z_index) {
            this.placements.delete(key);
            if (this.webglLayer) this.webglLayer.removePlacement(key);
          }
        }
        break;

      case "ByRow":
        for (const [key, active] of this.placements) {
          if (active.placement.row === target.row) {
            this.placements.delete(key);
            if (this.webglLayer) this.webglLayer.removePlacement(key);
          }
        }
        break;

      case "ByColumn":
        for (const [key, active] of this.placements) {
          if (active.placement.col === target.col) {
            this.placements.delete(key);
            if (this.webglLayer) this.webglLayer.removePlacement(key);
          }
        }
        break;
    }

    // Clear cache for deleted images
    if (this.cache) {
      for (const id of deletedImageIds) {
        this.cache.deleteByImageId(id);
      }
    }

    // Clean up progressive loading state
    for (const id of deletedImageIds) {
      this.progressiveImages.delete(id);
    }

    this.render();
  }

  /**
   * Render all images.
   */
  render(): void {
    this.performanceMonitor.startMeasure("frameTime");

    if (this.activeBackend === "webgl" && this.webglLayer) {
      this.renderWebGL();
    } else {
      this.renderCanvas2D();
    }

    this.performanceMonitor.endMeasure("frameTime");
  }

  /**
   * Render using WebGL.
   */
  private renderWebGL(): void {
    if (!this.webglLayer) return;
    this.webglLayer.render();
  }

  /**
   * Render using Canvas 2D.
   */
  private renderCanvas2D(): void {
    if (!this.canvas || !this.ctx) return;

    // Clear canvas
    const dpr = window.devicePixelRatio || 1;
    this.ctx.clearRect(
      0,
      0,
      this.canvas.width / dpr,
      this.canvas.height / dpr
    );

    // Sort placements by z-index
    const sorted = Array.from(this.placements.values()).sort(
      (a, b) => a.placement.z_index - b.placement.z_index
    );

    // Draw each placement
    for (const active of sorted) {
      const imageId = active.placement.image_id;

      // Check for animation bitmap first
      const animBitmap = this.animationBitmaps.get(imageId);
      if (animBitmap) {
        this.ctx.drawImage(
          animBitmap,
          active.x,
          active.y,
          active.displayWidth,
          active.displayHeight
        );
        continue;
      }

      // Fall back to static image
      const stored = this.images.get(imageId);
      if (!stored) continue;

      // Try to get cached scaled bitmap
      const cachedBitmap = this.getCachedBitmap(
        imageId,
        active.displayWidth,
        active.displayHeight
      );

      if (cachedBitmap) {
        this.ctx.drawImage(cachedBitmap, active.x, active.y);
      } else if (stored.bitmap) {
        this.ctx.drawImage(
          stored.bitmap,
          active.x,
          active.y,
          active.displayWidth,
          active.displayHeight
        );
        // Cache the scaled version
        this.cacheScaledBitmap(imageId, stored.bitmap, active.displayWidth, active.displayHeight);
      } else {
        this.drawImageData(stored.data, active);
      }
    }
  }

  /**
   * Get cached bitmap for a specific scale.
   */
  private getCachedBitmap(
    imageId: number,
    width: number,
    height: number
  ): ImageBitmap | undefined {
    if (!this.cache) return undefined;

    const key = this.cache.generateKey(imageId, Math.round(width), Math.round(height));
    return this.cache.get(key);
  }

  /**
   * Cache a scaled bitmap.
   */
  private async cacheScaledBitmap(
    imageId: number,
    source: ImageBitmap,
    width: number,
    height: number
  ): Promise<void> {
    if (!this.cache) return;

    const roundedWidth = Math.round(width);
    const roundedHeight = Math.round(height);

    // Only cache if significantly different from original
    if (
      Math.abs(source.width - roundedWidth) < 10 &&
      Math.abs(source.height - roundedHeight) < 10
    ) {
      return;
    }

    try {
      const scaledBitmap = await createImageBitmap(source, {
        resizeWidth: roundedWidth,
        resizeHeight: roundedHeight,
        resizeQuality: "high",
      });

      const key = this.cache.generateKey(imageId, roundedWidth, roundedHeight);
      this.cache.set(key, scaledBitmap, roundedWidth, roundedHeight);
    } catch (e) {
      console.warn("Failed to cache scaled bitmap:", e);
    }
  }

  /**
   * Fallback rendering using ImageData.
   */
  private drawImageData(
    image: DecodedImage,
    active: ActivePlacement
  ): void {
    if (!this.ctx) return;

    const tempCanvas = document.createElement("canvas");
    tempCanvas.width = image.width;
    tempCanvas.height = image.height;
    const tempCtx = tempCanvas.getContext("2d");
    if (!tempCtx) return;

    const binaryString = atob(image.rgba_base64);
    const bytes = new Uint8ClampedArray(binaryString.length);
    for (let i = 0; i < binaryString.length; i++) {
      bytes[i] = binaryString.charCodeAt(i);
    }

    const imageData = new ImageData(bytes, image.width, image.height);
    tempCtx.putImageData(imageData, 0, 0);

    this.ctx.drawImage(
      tempCanvas,
      active.x,
      active.y,
      active.displayWidth,
      active.displayHeight
    );
  }

  /**
   * Handle scroll offset change.
   *
   * @param scrollTop - Scroll offset in pixels
   */
  setScrollOffset(scrollTop: number): void {
    if (this.canvas) {
      this.canvas.style.top = `${-scrollTop}px`;
    }
    if (this.webglLayer) {
      this.webglLayer.getCanvas().style.top = `${-scrollTop}px`;
    }
  }

  /**
   * Adjust placements after scroll (line-based scroll).
   *
   * @param delta - Number of lines scrolled (positive = down, negative = up)
   */
  scrollPlacements(delta: number): void {
    const keysToDelete: string[] = [];

    for (const [key, active] of this.placements) {
      const newRow = active.placement.row + delta;
      if (newRow < 0) {
        keysToDelete.push(key);
      } else {
        active.placement.row = newRow;
        active.y = this.paddingY + newRow * this.charHeight + active.placement.y_offset;
      }
    }

    for (const key of keysToDelete) {
      this.placements.delete(key);
      if (this.webglLayer) this.webglLayer.removePlacement(key);
    }

    // Update WebGL placements
    if (this.webglLayer) {
      this.webglLayer.clearPlacements();
      for (const [key, active] of this.placements) {
        this.webglLayer.addPlacement({
          textureId: active.placement.image_id,
          x: active.x,
          y: active.y,
          width: active.displayWidth,
          height: active.displayHeight,
          zIndex: active.placement.z_index,
          key,
        });
      }
    }

    this.render();
  }

  /**
   * Get placements at a specific cell position.
   */
  getPlacementsAtPosition(row: number, col: number): ActivePlacement[] {
    const result: ActivePlacement[] = [];
    for (const active of this.placements.values()) {
      if (active.placement.row === row && active.placement.col === col) {
        result.push(active);
      }
    }
    return result;
  }

  /**
   * Get all placements for an image.
   */
  getPlacementsForImage(imageId: number): ActivePlacement[] {
    const result: ActivePlacement[] = [];
    for (const active of this.placements.values()) {
      if (active.placement.image_id === imageId) {
        result.push(active);
      }
    }
    return result;
  }

  /**
   * Check if an image is stored.
   */
  hasImage(imageId: number): boolean {
    return this.images.has(imageId);
  }

  /**
   * Get image count.
   */
  getImageCount(): number {
    return this.images.size;
  }

  /**
   * Get placement count.
   */
  getPlacementCount(): number {
    return this.placements.size;
  }

  /**
   * Clear all images and placements.
   */
  clear(): void {
    this.placements.clear();
    this.images.clear();
    this.animationController.clear();
    this.animationBitmaps.clear();
    this.progressiveImages.clear();

    if (this.cache) {
      this.cache.clear();
    }

    if (this.webglLayer) {
      this.webglLayer.clearPlacements();
    }

    this.render();
  }

  /**
   * Handle animation event from backend.
   */
  async handleAnimationEvent(event: AnimationEvent): Promise<void> {
    await this.animationController.handleEvent(event);
  }

  /**
   * Set visibility for animation.
   */
  setAnimationVisibility(imageId: number, isVisible: boolean): void {
    this.animationController.setVisibility(imageId, isVisible);
  }

  /**
   * Set visibility for all animations.
   */
  setAllAnimationsVisibility(isVisible: boolean): void {
    this.animationController.setAllVisibility(isVisible);
  }

  /**
   * Check if an image has animation.
   */
  hasAnimation(imageId: number): boolean {
    return this.animationController.hasAnimation(imageId);
  }

  /**
   * Get the animation controller.
   */
  getAnimationController(): AnimationController {
    return this.animationController;
  }

  /**
   * Get the performance monitor.
   */
  getPerformanceMonitor(): PerformanceMonitor {
    return this.performanceMonitor;
  }

  /**
   * Get render statistics.
   */
  getRenderStats(): RenderStats {
    const cacheStats = this.cache?.getStats();
    const frameMetrics = this.performanceMonitor.getMetrics("frameTime");

    return {
      backend: this.activeBackend,
      imageCount: this.images.size,
      placementCount: this.placements.size,
      cacheHitRate: cacheStats?.hitRate ?? 0,
      avgFrameTime: frameMetrics.average,
      lastFrameTime: frameMetrics.last,
      memoryUsage: cacheStats?.memoryBytes ?? 0,
      webglAvailable: this.webglAvailable,
      webglActive: this.activeBackend === "webgl" && this.webglLayer?.isWebGLActive() === true,
    };
  }

  /**
   * Get debug information.
   */
  getDebugInfo(): string {
    const stats = this.getRenderStats();
    const perfInfo = this.performanceMonitor.getDebugInfo();

    return [
      `Render Backend: ${stats.backend}`,
      `WebGL Available: ${stats.webglAvailable}`,
      `WebGL Active: ${stats.webglActive}`,
      `Images: ${stats.imageCount}`,
      `Placements: ${stats.placementCount}`,
      `Cache Hit Rate: ${(stats.cacheHitRate * 100).toFixed(1)}%`,
      `Memory Usage: ${(stats.memoryUsage / 1024 / 1024).toFixed(2)} MB`,
      `Avg Frame Time: ${stats.avgFrameTime.toFixed(2)} ms`,
      `Last Frame Time: ${stats.lastFrameTime.toFixed(2)} ms`,
      "",
      perfInfo,
    ].join("\n");
  }

  /**
   * Dispose of the image layer.
   */
  dispose(): void {
    // Remove canvas/WebGL layer
    if (this.canvas) {
      this.canvas.remove();
      this.canvas = null;
      this.ctx = null;
    }
    if (this.webglLayer) {
      this.webglLayer.dispose();
      this.webglLayer = null;
    }

    this.placements.clear();

    // Dispose animation controller
    this.animationController.dispose();
    this.animationBitmaps.clear();

    // Clear progressive loading state
    for (const progressive of this.progressiveImages.values()) {
      if (progressive.lowResPreview) {
        progressive.lowResPreview.close();
      }
      if (progressive.fullResolution) {
        progressive.fullResolution.close();
      }
    }
    this.progressiveImages.clear();

    // Release ImageBitmaps
    for (const stored of this.images.values()) {
      if (stored.bitmap) {
        stored.bitmap.close();
      }
    }
    this.images.clear();

    // Dispose cache
    if (this.cache) {
      this.cache.dispose();
      this.cache = null;
    }

    // Dispose resize handler
    this.resizeHandler.dispose();

    // Dispose performance monitor
    this.performanceMonitor.dispose();
  }
}
