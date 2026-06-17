/**
 * Mouse event handling for terminal.
 *
 * Encodes mouse events into escape sequences for PTY.
 */

import type { MouseEncoding, MouseTrackingMode } from "./modes.ts";

/**
 * Mouse button types.
 */
export type MouseButton =
	| "left"
	| "middle"
	| "right"
	| "release"
	| "wheelUp"
	| "wheelDown";

/**
 * Mouse event data.
 */
export interface MouseEvent {
	button: MouseButton;
	col: number; // 1-based column
	row: number; // 1-based row
	modifiers: {
		shift: boolean;
		meta: boolean;
		ctrl: boolean;
	};
	motion?: boolean; // true if this is a motion event (drag)
}

/**
 * Get the button code for mouse encoding.
 */
function getButtonCode(button: MouseButton, motion: boolean = false): number {
	let code: number;

	switch (button) {
		case "left":
			code = 0;
			break;
		case "middle":
			code = 1;
			break;
		case "right":
			code = 2;
			break;
		case "release":
			code = 3;
			break;
		case "wheelUp":
			code = 64;
			break;
		case "wheelDown":
			code = 65;
			break;
		default:
			code = 0;
	}

	// Add motion bit if this is a drag event
	if (motion) {
		code |= 32;
	}

	return code;
}

/**
 * Add modifier bits to button code.
 */
function addModifiers(
	code: number,
	modifiers: { shift: boolean; meta: boolean; ctrl: boolean },
): number {
	if (modifiers.shift) code |= 4;
	if (modifiers.meta) code |= 8;
	if (modifiers.ctrl) code |= 16;
	return code;
}

/**
 * Encode a mouse event using default X10 encoding.
 *
 * Format: ESC [ M Cb Cx Cy
 * Where Cb = button + 32, Cx = col + 32, Cy = row + 32
 */
function encodeX10(event: MouseEvent): Uint8Array | null {
	const col = Math.min(Math.max(event.col, 1), 223); // Max 223 for X10
	const row = Math.min(Math.max(event.row, 1), 223);

	let code = getButtonCode(event.button, event.motion);
	code = addModifiers(code, event.modifiers);

	return new Uint8Array([
		0x1b, // ESC
		0x5b, // [
		0x4d, // M
		code + 32,
		col + 32,
		row + 32,
	]);
}

/**
 * Encode a mouse event using UTF-8 encoding (mode 1005).
 *
 * Similar to X10 but uses UTF-8 for coordinates > 95.
 */
function encodeUTF8(event: MouseEvent): Uint8Array | null {
	let code = getButtonCode(event.button, event.motion);
	code = addModifiers(code, event.modifiers);

	const col = Math.max(event.col, 1);
	const row = Math.max(event.row, 1);

	// Build the sequence
	const bytes: number[] = [0x1b, 0x5b, 0x4d]; // ESC [ M

	// Button + 32
	bytes.push(code + 32);

	// Column (UTF-8 encoded if > 127)
	const colChar = col + 32;
	if (colChar < 128) {
		bytes.push(colChar);
	} else {
		// UTF-8 encode
		bytes.push(0xc0 | ((colChar >> 6) & 0x1f));
		bytes.push(0x80 | (colChar & 0x3f));
	}

	// Row (UTF-8 encoded if > 127)
	const rowChar = row + 32;
	if (rowChar < 128) {
		bytes.push(rowChar);
	} else {
		bytes.push(0xc0 | ((rowChar >> 6) & 0x1f));
		bytes.push(0x80 | (rowChar & 0x3f));
	}

	return new Uint8Array(bytes);
}

/**
 * Encode a mouse event using SGR encoding (mode 1006).
 *
 * Format: ESC [ < Cb ; Cx ; Cy M/m
 * Where M = press, m = release
 */
function encodeSGR(event: MouseEvent): Uint8Array | null {
	let code = getButtonCode(event.button, event.motion);
	code = addModifiers(code, event.modifiers);

	// For release in SGR mode, we need to report the actual button
	// that was released, not code 3
	if (event.button === "release") {
		// We don't know which button was released, assume left
		code = addModifiers(0, event.modifiers);
	}

	const col = Math.max(event.col, 1);
	const row = Math.max(event.row, 1);

	// Build the sequence string
	const isRelease = event.button === "release";
	const sequence = `\x1b[<${code};${col};${row}${isRelease ? "m" : "M"}`;

	return new TextEncoder().encode(sequence);
}

/**
 * Encode a mouse event into an escape sequence.
 *
 * @param event - The mouse event to encode
 * @param trackingMode - Current mouse tracking mode
 * @param encoding - Current mouse encoding format
 * @returns Escape sequence bytes, or null if event should not be reported
 */
export function encodeMouseEvent(
	event: MouseEvent,
	trackingMode: MouseTrackingMode,
	encoding: MouseEncoding,
): Uint8Array | null {
	// Check if we should report this event
	if (trackingMode === "none") {
		return null;
	}

	// X10 mode only reports button press, not release or motion
	if (trackingMode === "x10") {
		if (event.button === "release" || event.motion) {
			return null;
		}
	}

	// Button mode reports press, release, and motion while button held
	if (trackingMode === "button") {
		// Motion is only reported if button is held (motion === true with actual button)
		// We can't easily check this here, so we report all motion if mode is button
	}

	// Any-event mode reports everything including motion without button held

	// Encode based on format
	switch (encoding) {
		case "sgr":
			return encodeSGR(event);
		case "utf8":
			return encodeUTF8(event);
		default:
			return encodeX10(event);
	}
}

/**
 * Convert a DOM MouseEvent to our MouseEvent format.
 *
 * @param domEvent - The DOM mouse event
 * @param cellWidth - Width of a terminal cell in pixels
 * @param cellHeight - Height of a terminal cell in pixels
 * @param containerRect - Bounding rect of the terminal container
 * @param type - Event type: "down", "up", "move", or "wheel"
 * @returns MouseEvent or null if invalid
 */
export function domEventToMouseEvent(
	domEvent: globalThis.MouseEvent | WheelEvent,
	cellWidth: number,
	cellHeight: number,
	containerRect: DOMRect,
	type: "down" | "up" | "move" | "wheel",
): MouseEvent | null {
	// Calculate cell position (1-based)
	const x = domEvent.clientX - containerRect.left;
	const y = domEvent.clientY - containerRect.top;

	if (x < 0 || y < 0) {
		return null;
	}

	const col = Math.floor(x / cellWidth) + 1;
	const row = Math.floor(y / cellHeight) + 1;

	if (col < 1 || row < 1) {
		return null;
	}

	// Determine button
	let button: MouseButton;
	const motion = type === "move";

	if (type === "wheel") {
		const wheelEvent = domEvent as WheelEvent;
		button = wheelEvent.deltaY < 0 ? "wheelUp" : "wheelDown";
	} else if (type === "up") {
		button = "release";
	} else {
		switch (domEvent.button) {
			case 0:
				button = "left";
				break;
			case 1:
				button = "middle";
				break;
			case 2:
				button = "right";
				break;
			default:
				return null;
		}
	}

	return {
		button,
		col,
		row,
		modifiers: {
			shift: domEvent.shiftKey,
			meta: domEvent.metaKey,
			ctrl: domEvent.ctrlKey,
		},
		motion,
	};
}

/**
 * Check if mouse tracking should be enabled based on terminal modes.
 */
export function isMouseTrackingEnabled(
	trackingMode: MouseTrackingMode,
): boolean {
	return trackingMode !== "none";
}
