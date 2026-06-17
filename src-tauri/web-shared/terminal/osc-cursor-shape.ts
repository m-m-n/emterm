/**
 * OSC 22 mouse cursor shape handler with push/pop stack.
 *
 * Supports:
 * - `OSC 22 ; shape ST` - Set cursor shape directly
 * - `OSC 22 ; >shape ST` - Push current cursor and set new shape
 * - `OSC 22 ; < ST` - Pop and restore previous cursor shape
 * - `OSC 22 ; ST` (empty) - Reset to default cursor
 */

/**
 * Valid CSS cursor shape names accepted by OSC 22.
 */
export const VALID_CURSOR_SHAPES = [
	"default",
	"none",
	"context-menu",
	"help",
	"pointer",
	"progress",
	"wait",
	"cell",
	"crosshair",
	"text",
	"vertical-text",
	"alias",
	"copy",
	"move",
	"no-drop",
	"not-allowed",
	"grab",
	"grabbing",
	"all-scroll",
	"col-resize",
	"row-resize",
	"n-resize",
	"e-resize",
	"s-resize",
	"w-resize",
	"ne-resize",
	"nw-resize",
	"se-resize",
	"sw-resize",
	"ew-resize",
	"ns-resize",
	"nesw-resize",
	"nwse-resize",
	"zoom-in",
	"zoom-out",
] as const;

const validShapeSet = new Set<string>(VALID_CURSOR_SHAPES);

export type CursorShape = (typeof VALID_CURSOR_SHAPES)[number];

/** Set cursor shape directly. */
export interface Osc22Set {
	type: "set";
	shape: CursorShape;
}

/** Push current cursor and set new shape. */
export interface Osc22Push {
	type: "push";
	shape: CursorShape;
}

/** Pop and restore previous cursor shape. */
export interface Osc22Pop {
	type: "pop";
}

export type Osc22Action = Osc22Set | Osc22Push | Osc22Pop;

/**
 * Parse OSC 22 data string into an action.
 *
 * @param data - The OSC data after "22;"
 * @returns Parsed action, or null if cursor shape is unknown
 */
export function parseOsc22(data: string): Osc22Action | null {
	// Empty -> reset to default
	if (data === "") {
		return { type: "set", shape: "default" };
	}

	// Pop: "<"
	if (data === "<") {
		return { type: "pop" };
	}

	// Push: ">shape"
	if (data.startsWith(">")) {
		const shape = data.slice(1);
		if (!validShapeSet.has(shape)) return null;
		return { type: "push", shape: shape as CursorShape };
	}

	// Set: "shape"
	if (!validShapeSet.has(data)) return null;
	return { type: "set", shape: data as CursorShape };
}

/**
 * Cursor shape stack with bounded depth.
 * Supports push/pop operations for OSC 22.
 */
export class CursorShapeStack {
	private static readonly MAX_DEPTH = 10;
	private stack: CursorShape[] = [];
	private currentShape: CursorShape = "default";

	/** Get the current cursor shape. */
	current(): CursorShape {
		return this.currentShape;
	}

	/** Get the current stack depth. */
	depth(): number {
		return this.stack.length;
	}

	/** Set cursor shape directly (does not affect stack). */
	set(shape: CursorShape): void {
		this.currentShape = shape;
	}

	/** Push current cursor onto stack and set new shape. */
	push(shape: CursorShape): void {
		if (this.stack.length >= CursorShapeStack.MAX_DEPTH) {
			// Drop oldest entry
			this.stack.shift();
		}
		this.stack.push(this.currentShape);
		this.currentShape = shape;
	}

	/** Pop and restore previous cursor shape. No-op if stack is empty. */
	pop(): void {
		if (this.stack.length > 0) {
			this.currentShape = this.stack.pop()!;
		}
	}

	/** Reset stack and cursor to default. */
	reset(): void {
		this.stack = [];
		this.currentShape = "default";
	}
}
