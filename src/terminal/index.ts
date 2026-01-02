/**
 * Terminal module exports.
 *
 * This module provides the terminal state management and rendering system.
 */

// Core types and state
export { TerminalState } from "./state.ts";
export { ScreenBuffer } from "./buffer.ts";
export { CursorState } from "./cursor.ts";
export type { CursorStyle } from "./cursor.ts";
export { TerminalRenderer } from "./renderer.ts";

// Modes
export {
  createDefaultModes,
  cloneModes,
  setDecPrivateMode,
  DECPrivateMode,
} from "./modes.ts";
export type {
  TerminalModes,
  MouseTrackingMode,
  MouseEncoding,
  CursorKeysMode,
} from "./modes.ts";

// Grid structures
export { Line, createCell, createEmptyCell, cloneCell } from "./grid.ts";
export type { Cell } from "./grid.ts";

// Attributes
export {
  createDefaultAttributes,
  attributesEqual,
  cloneAttributes,
  applySgrAttr,
  applySgrAttrs,
  getEffectiveForeground,
  getEffectiveBackground,
  DEFAULT_ATTRIBUTES,
} from "./attributes.ts";
export type { CellAttributes, Color, SgrAttr } from "./attributes.ts";

// Colors
export {
  PALETTE_16,
  PALETTE_256,
  indexToRgb,
  standardColorToRgb,
  brightColorToRgb,
  rgbToCSS,
  indexToCSS,
  sgrColorToRgb,
  DEFAULT_FOREGROUND,
  DEFAULT_BACKGROUND,
} from "./colors.ts";
export type { Rgb, SgrColor } from "./colors.ts";

// SGR parsing
export { parseSgrParams } from "./sgr.ts";

// Unicode utilities
export { charWidth, isWideChar, stringWidth } from "./unicode.ts";

// Performance utilities
export {
  PerformanceMonitor,
  RenderTimer,
  ThroughputMeter,
  getPerformanceMonitor,
  checkFrameBudget,
} from "./performance.ts";
export type { PerformanceMetrics } from "./performance.ts";

// Style cache (for advanced usage)
export { StyleCache, getStyleCache, resetStyleCache } from "./style-cache.ts";
export type { StyleCacheMetrics } from "./style-cache.ts";

// Mouse event handling
export {
  encodeMouseEvent,
  domEventToMouseEvent,
  isMouseTrackingEnabled,
} from "./mouse.ts";
export type { MouseEvent, MouseButton } from "./mouse.ts";
