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

export { AnimationController } from "./animation.ts";
export type { CacheKey, CacheOptions, CacheStats } from "./cache.ts";
export { BitmapCache } from "./cache.ts";
// Core components
export { ImageLayer } from "./layer.ts";
export type {
	MetricType,
	PerformanceMetrics,
	PerformanceMonitorOptions,
} from "./performance.ts";
export { PerformanceMonitor } from "./performance.ts";
export type {
	ResizeCallback,
	ResizeEvent,
	ResizeHandlerOptions,
} from "./resize-handler.ts";
export { ResizeHandler } from "./resize-handler.ts";
// Core types
export type {
	ActiveAnimation,
	ActivePlacement,
	AnimationEvent,
	AnimationFrameData,
	AnimationState,
	DecodedImage,
	ImageDeleteTarget,
	ImageEvent,
	ImageLayerOptions,
	ImagePlacement,
	ProgressiveImage,
	ProgressiveLoadingState,
	// Phase 4 types
	RenderBackend,
	RenderStats,
	StoredImage,
} from "./types.ts";
// Phase 4 components
export { isWebGLSupported, WebGLLayer } from "./webgl-layer.ts";
