/**
 * Selection system v2.
 *
 * Provides text selection, clipboard operations, and selection rendering
 * for the terminal emulator.
 *
 * @module selection-v2
 */

// Types
export type {
	GridPosition,
	SelectionEvent,
	SelectionEventListener,
	SelectionEventType,
	SelectionMode,
	SelectionRange,
	SelectionState,
} from "./types";
export { isPositionInRange, normalizeRange } from "./types";

// Core components
export { ClipboardBridge } from "./ClipboardBridge";
export { SelectionController } from "./SelectionController";
export type { SelectionControllerOptions } from "./SelectionController";
export { SelectionModel } from "./SelectionModel";
export { SelectionRenderer } from "./SelectionRenderer";
export { WordBoundary } from "./WordBoundary";
