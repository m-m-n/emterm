/**
 * Terminal module exports.
 *
 * This module provides the terminal state management and rendering system.
 */

export type { CellAttributes, Color, SgrAttr } from "./attributes.ts";
// Attributes
export {
	applySgrAttr,
	applySgrAttrs,
	attributesEqual,
	cloneAttributes,
	createDefaultAttributes,
	DEFAULT_ATTRIBUTES,
	getEffectiveBackground,
	getEffectiveForeground,
} from "./attributes.ts";
export { ScreenBuffer } from "./buffer.ts";
export type { Rgb, SgrColor } from "./colors.ts";
// Colors
export {
	brightColorToRgb,
	DEFAULT_BACKGROUND,
	DEFAULT_FOREGROUND,
	indexToCSS,
	indexToRgb,
	PALETTE_16,
	PALETTE_256,
	rgbToCSS,
	sgrColorToRgb,
	standardColorToRgb,
} from "./colors.ts";
export type { CursorStyle } from "./cursor.ts";
export { CursorState } from "./cursor.ts";
export type { Cell } from "./grid.ts";
// Grid structures
export { cloneCell, createCell, createEmptyCell, Line } from "./grid.ts";
export type {
	CursorKeysMode,
	MouseEncoding,
	MouseTrackingMode,
	TerminalModes,
} from "./modes.ts";
// Modes
export {
	cloneModes,
	createDefaultModes,
	DECPrivateMode,
	setDecPrivateMode,
} from "./modes.ts";
export type { MouseButton, MouseEvent } from "./mouse.ts";
// Mouse event handling
export {
	domEventToMouseEvent,
	encodeMouseEvent,
	isMouseTrackingEnabled,
} from "./mouse.ts";
export type { PerformanceMetrics } from "./performance.ts";
// Performance utilities
export {
	checkFrameBudget,
	getPerformanceMonitor,
	PerformanceMonitor,
	RenderTimer,
	ThroughputMeter,
} from "./performance.ts";
// Canvas renderer and factory
export { CanvasRenderer } from "./canvas-renderer.ts";
export type { ITerminalRenderer, RendererType } from "./renderer-interface.ts";
export { createRenderer, createRendererAsync } from "./renderer-factory.ts";
// SGR parsing
export { parseSgrParams } from "./sgr.ts";
// Core types and state
export { TerminalState } from "./state.ts";
export type { StyleCacheMetrics } from "./style-cache.ts";
// Style cache (for advanced usage)
export { getStyleCache, resetStyleCache, StyleCache } from "./style-cache.ts";
// Unicode utilities
export { charWidth, isWideChar, stringWidth } from "./unicode.ts";
