/**
 * Image display module.
 *
 * Provides inline image rendering for Kitty Graphics Protocol and SIXEL.
 *
 * Phase 4 additions:
 * - WebGL-accelerated rendering
 * - Bitmap caching
 * - Debounced resize handling
 * - Performance monitoring
 * - Progressive loading
 *
 * @module image
 */

// Core types
export type {
  DecodedImage,
  ImagePlacement,
  ImageDeleteTarget,
  ImageEvent,
  StoredImage,
  ActivePlacement,
  AnimationState,
  AnimationEvent,
  AnimationFrameData,
  ActiveAnimation,
  // Phase 4 types
  RenderBackend,
  ImageLayerOptions,
  ProgressiveLoadingState,
  ProgressiveImage,
  RenderStats,
} from "./types.ts";

// Core components
export { ImageLayer } from "./layer.ts";
export { AnimationController } from "./animation.ts";

// Phase 4 components
export { WebGLLayer, isWebGLSupported } from "./webgl-layer.ts";
export { BitmapCache } from "./cache.ts";
export type { CacheKey, CacheOptions, CacheStats } from "./cache.ts";
export { ResizeHandler } from "./resize-handler.ts";
export type { ResizeEvent, ResizeCallback, ResizeHandlerOptions } from "./resize-handler.ts";
export { PerformanceMonitor } from "./performance.ts";
export type { MetricType, PerformanceMetrics, PerformanceMonitorOptions } from "./performance.ts";
