/**
 * Image display type definitions.
 *
 * @module image/types
 */

/**
 * Decoded image data from backend.
 */
export interface DecodedImage {
  /** Unique image ID. */
  id: number;

  /** Image width in pixels. */
  width: number;

  /** Image height in pixels. */
  height: number;

  /** Base64-encoded RGBA pixel data. */
  rgba_base64: string;
}

/**
 * Image placement specification.
 */
export interface ImagePlacement {
  /** Image ID to display. */
  image_id: number;

  /** Placement ID (for multiple placements of same image). */
  placement_id: number;

  /** Display position: row (0-based). */
  row: number;

  /** Display position: column (0-based). */
  col: number;

  /** Display width in terminal columns (0 = auto). */
  columns: number;

  /** Display height in terminal rows (0 = auto). */
  rows: number;

  /** X offset within cell in pixels. */
  x_offset: number;

  /** Y offset within cell in pixels. */
  y_offset: number;

  /** Z-index for layering (negative = behind text). */
  z_index: number;
}

/**
 * Image deletion target.
 */
export type ImageDeleteTarget =
  | { type: "All" }
  | { type: "AllIncludingHidden" }
  | { type: "ById"; id: number }
  | { type: "ByPlacement"; image_id: number; placement_id: number }
  | { type: "AtCursor"; row: number; col: number }
  | { type: "ByZIndex"; z_index: number }
  | { type: "ByRow"; row: number }
  | { type: "ByColumn"; col: number };

/**
 * Animation playback state.
 */
export type AnimationState = "Stopped" | "Loading" | "Playing" | "Paused";

/**
 * Animation event types from backend.
 */
export type AnimationEvent =
  | {
      type: "FrameReady";
      image_id: number;
      frame_number: number;
      delay_ms: number;
      rgba_base64: string;
      width: number;
      height: number;
    }
  | {
      type: "StateChanged";
      image_id: number;
      state: AnimationState;
    }
  | {
      type: "Completed";
      image_id: number;
    };

/**
 * Image event types from backend.
 */
export type ImageEvent =
  | { type: "ImageReady"; image: DecodedImage }
  | { type: "Place"; placement: ImagePlacement }
  | { type: "Delete"; target: ImageDeleteTarget }
  | { type: "QueryResponse"; supported: boolean }
  | { type: "Response"; data: string }
  | { type: "Animation"; data: AnimationEvent };

/**
 * Stored image with cached ImageBitmap.
 */
export interface StoredImage {
  /** Original decoded image data. */
  data: DecodedImage;

  /** Cached ImageBitmap for rendering. */
  bitmap: ImageBitmap | null;
}

/**
 * Active image placement on screen.
 */
export interface ActivePlacement {
  /** Placement specification. */
  placement: ImagePlacement;

  /** Calculated pixel position: x. */
  x: number;

  /** Calculated pixel position: y. */
  y: number;

  /** Calculated display width in pixels. */
  displayWidth: number;

  /** Calculated display height in pixels. */
  displayHeight: number;
}

/**
 * Animation frame data.
 */
export interface AnimationFrameData {
  /** Frame number (1-based). */
  frameNumber: number;

  /** Frame delay in milliseconds. */
  delayMs: number;

  /** Frame width. */
  width: number;

  /** Frame height. */
  height: number;

  /** Cached ImageBitmap for this frame. */
  bitmap: ImageBitmap | null;
}

/**
 * Active animation state for an image.
 */
export interface ActiveAnimation {
  /** Image ID. */
  imageId: number;

  /** Animation frames by frame number. */
  frames: Map<number, AnimationFrameData>;

  /** Current frame number. */
  currentFrame: number;

  /** Animation state. */
  state: AnimationState;

  /** Loop count (0 = infinite). */
  loopCount: number;

  /** Current loop iteration. */
  currentLoop: number;

  /** Timer ID for animation playback. */
  timerId: number | null;

  /** Whether animation is visible (for visibility-based pause). */
  isVisible: boolean;
}

// ============================================================================
// Phase 4: Optimization Types
// ============================================================================

/**
 * Render backend type.
 */
export type RenderBackend = "webgl" | "canvas2d";

/**
 * Image layer configuration options.
 */
export interface ImageLayerOptions {
  /** Preferred render backend. */
  preferredBackend?: RenderBackend;

  /** Enable bitmap caching. */
  enableCache?: boolean;

  /** Maximum cache entries. */
  maxCacheEntries?: number;

  /** Maximum cache memory in bytes. */
  maxCacheMemoryBytes?: number;

  /** Resize debounce time in milliseconds. */
  resizeDebounceMs?: number;

  /** Enable performance monitoring. */
  enablePerformanceMonitoring?: boolean;

  /** Progressive loading threshold in bytes. */
  progressiveLoadingThreshold?: number;
}

/**
 * Progressive loading state.
 */
export type ProgressiveLoadingState =
  | "pending"
  | "low-resolution"
  | "high-resolution"
  | "complete";

/**
 * Progressive image data.
 */
export interface ProgressiveImage {
  /** Image ID. */
  id: number;

  /** Loading state. */
  state: ProgressiveLoadingState;

  /** Low-resolution preview (if available). */
  lowResPreview: ImageBitmap | null;

  /** Full resolution image (when loaded). */
  fullResolution: ImageBitmap | null;

  /** Original width. */
  width: number;

  /** Original height. */
  height: number;

  /** Data size in bytes. */
  dataSize: number;
}

/**
 * Render statistics for debugging.
 */
export interface RenderStats {
  /** Active render backend. */
  backend: RenderBackend;

  /** Number of stored images. */
  imageCount: number;

  /** Number of active placements. */
  placementCount: number;

  /** Cache hit rate (0-1). */
  cacheHitRate: number;

  /** Average frame time in milliseconds. */
  avgFrameTime: number;

  /** Last frame time in milliseconds. */
  lastFrameTime: number;

  /** Memory usage in bytes. */
  memoryUsage: number;

  /** Whether WebGL is available. */
  webglAvailable: boolean;

  /** Whether WebGL is active. */
  webglActive: boolean;
}
