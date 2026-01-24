/**
 * Keyboard input handler for terminal emulator.
 *
 * Converts DOM KeyboardEvents to byte sequences suitable for
 * sending to a PTY (pseudo-terminal).
 */

import type { CursorKeysMode } from "../terminal/modes";

/**
 * Mapping definition for special key sequences.
 */
export interface KeyMapping {
	key: string;
	ctrl?: boolean;
	alt?: boolean;
	shift?: boolean;
	sequence: number[];
}

/**
 * Application cursor key sequences (when DECCKM mode is set).
 *
 * In Application mode, arrow keys send SS3 (ESC O) instead of CSI (ESC [).
 * This is used by applications like less, vim, htop, etc.
 *
 * VT100 Reference: https://vt100.net/docs/vt510-rm/DECCKM.html
 */
const APPLICATION_CURSOR_KEYS: Record<string, number[]> = {
	ArrowUp: [0x1b, 0x4f, 0x41], // ESC O A
	ArrowDown: [0x1b, 0x4f, 0x42], // ESC O B
	ArrowRight: [0x1b, 0x4f, 0x43], // ESC O C
	ArrowLeft: [0x1b, 0x4f, 0x44], // ESC O D
};

/**
 * Special key mappings for terminal emulation.
 *
 * These follow the ANSI/VT100 terminal escape sequence conventions.
 */
const SPECIAL_KEYS: KeyMapping[] = [
	// Control characters
	{ key: "c", ctrl: true, sequence: [0x03] }, // ETX (Ctrl+C - interrupt)
	{ key: "d", ctrl: true, sequence: [0x04] }, // EOT (Ctrl+D - EOF)
	{ key: "z", ctrl: true, sequence: [0x1a] }, // SUB (Ctrl+Z - suspend)
	{ key: "l", ctrl: true, sequence: [0x0c] }, // FF  (Ctrl+L - clear screen)
	{ key: "a", ctrl: true, sequence: [0x01] }, // SOH (Ctrl+A - line start)
	{ key: "e", ctrl: true, sequence: [0x05] }, // ENQ (Ctrl+E - line end)
	{ key: "k", ctrl: true, sequence: [0x0b] }, // VT  (Ctrl+K - kill to end)
	{ key: "u", ctrl: true, sequence: [0x15] }, // NAK (Ctrl+U - kill line)
	{ key: "w", ctrl: true, sequence: [0x17] }, // ETB (Ctrl+W - delete word)

	// Arrow keys (CSI sequences)
	{ key: "ArrowUp", sequence: [0x1b, 0x5b, 0x41] }, // ESC [ A
	{ key: "ArrowDown", sequence: [0x1b, 0x5b, 0x42] }, // ESC [ B
	{ key: "ArrowRight", sequence: [0x1b, 0x5b, 0x43] }, // ESC [ C
	{ key: "ArrowLeft", sequence: [0x1b, 0x5b, 0x44] }, // ESC [ D

	// Navigation keys
	{ key: "Home", sequence: [0x1b, 0x5b, 0x48] }, // ESC [ H
	{ key: "End", sequence: [0x1b, 0x5b, 0x46] }, // ESC [ F
	{ key: "PageUp", sequence: [0x1b, 0x5b, 0x35, 0x7e] }, // ESC [ 5 ~
	{ key: "PageDown", sequence: [0x1b, 0x5b, 0x36, 0x7e] }, // ESC [ 6 ~
	{ key: "Insert", sequence: [0x1b, 0x5b, 0x32, 0x7e] }, // ESC [ 2 ~
	{ key: "Delete", sequence: [0x1b, 0x5b, 0x33, 0x7e] }, // ESC [ 3 ~

	// Function keys (F1-F4 use SS3, F5-F12 use CSI)
	{ key: "F1", sequence: [0x1b, 0x4f, 0x50] }, // ESC O P
	{ key: "F2", sequence: [0x1b, 0x4f, 0x51] }, // ESC O Q
	{ key: "F3", sequence: [0x1b, 0x4f, 0x52] }, // ESC O R
	{ key: "F4", sequence: [0x1b, 0x4f, 0x53] }, // ESC O S
	{ key: "F5", sequence: [0x1b, 0x5b, 0x31, 0x35, 0x7e] }, // ESC [ 15 ~
	{ key: "F6", sequence: [0x1b, 0x5b, 0x31, 0x37, 0x7e] }, // ESC [ 17 ~
	{ key: "F7", sequence: [0x1b, 0x5b, 0x31, 0x38, 0x7e] }, // ESC [ 18 ~
	{ key: "F8", sequence: [0x1b, 0x5b, 0x31, 0x39, 0x7e] }, // ESC [ 19 ~
	{ key: "F9", sequence: [0x1b, 0x5b, 0x32, 0x30, 0x7e] }, // ESC [ 20 ~
	{ key: "F10", sequence: [0x1b, 0x5b, 0x32, 0x31, 0x7e] }, // ESC [ 21 ~
	{ key: "F11", sequence: [0x1b, 0x5b, 0x32, 0x33, 0x7e] }, // ESC [ 23 ~
	{ key: "F12", sequence: [0x1b, 0x5b, 0x32, 0x34, 0x7e] }, // ESC [ 24 ~

	// Special keys
	{ key: "Enter", sequence: [0x0d] }, // CR (Carriage Return)
	{ key: "Tab", sequence: [0x09] }, // HT (Horizontal Tab)
	{ key: "Backspace", sequence: [0x7f] }, // DEL
	{ key: "Escape", sequence: [0x1b] }, // ESC
];

/**
 * Converts a DOM KeyboardEvent to a byte sequence for PTY input.
 *
 * Handles:
 * - Special key mappings (arrows, function keys, etc.)
 * - Control character combinations (Ctrl+C, Ctrl+D, etc.)
 * - Alt key combinations (sends ESC prefix)
 * - Regular printable characters
 * - DECCKM mode (Application Cursor Keys) for arrow keys
 *
 * @param event - The DOM KeyboardEvent to convert
 * @param cursorKeysMode - Cursor keys mode (normal or application)
 * @returns Byte array for the key, or null if the key should be ignored
 *
 * @example
 * ```typescript
 * document.addEventListener('keydown', (event) => {
 *   const cursorMode = terminalState.getModes().cursorKeys;
 *   const bytes = keyEventToBytes(event, cursorMode);
 *   if (bytes) {
 *     event.preventDefault();
 *     ptyClient.write(bytes);
 *   }
 * });
 * ```
 */
export function keyEventToBytes(
	event: KeyboardEvent,
	cursorKeysMode: CursorKeysMode = "normal",
): Uint8Array | null {
	// Handle Application Cursor Keys mode (DECCKM)
	// Arrow keys send ESC O instead of ESC [ when DECCKM is set
	if (
		cursorKeysMode === "application" &&
		!event.ctrlKey &&
		!event.altKey &&
		!event.shiftKey
	) {
		const appCursorSequence = APPLICATION_CURSOR_KEYS[event.key];
		if (appCursorSequence) {
			return new Uint8Array(appCursorSequence);
		}
	}

	// Check for special key mappings first
	for (const mapping of SPECIAL_KEYS) {
		if (
			event.key === mapping.key &&
			!!event.ctrlKey === !!mapping.ctrl &&
			!!event.altKey === !!mapping.alt &&
			!!event.shiftKey === !!mapping.shift
		) {
			return new Uint8Array(mapping.sequence);
		}
	}

	// Ctrl + letter (a-z) -> control characters (0x01-0x1a)
	if (event.ctrlKey && !event.altKey && event.key.length === 1) {
		const char = event.key.toLowerCase();
		if (char >= "a" && char <= "z") {
			// Ctrl+A = 0x01, Ctrl+B = 0x02, ..., Ctrl+Z = 0x1a
			return new Uint8Array([char.charCodeAt(0) - 96]);
		}
	}

	// Alt + key -> ESC prefix followed by the key
	if (event.altKey && !event.ctrlKey && event.key.length === 1) {
		const bytes = new TextEncoder().encode(event.key);
		const result = new Uint8Array(bytes.length + 1);
		result[0] = 0x1b; // ESC
		result.set(bytes, 1);
		return result;
	}

	// Regular printable character (no modifiers except Shift)
	if (event.key.length === 1 && !event.ctrlKey && !event.altKey) {
		return new TextEncoder().encode(event.key);
	}

	// Key should be ignored (e.g., modifier keys alone, unhandled special keys)
	return null;
}

/**
 * Checks if a keyboard event should be passed to the terminal.
 *
 * Use this to determine if preventDefault should be called.
 *
 * @param event - The DOM KeyboardEvent to check
 * @returns true if the event should be handled by the terminal
 */
export function shouldHandleKey(event: KeyboardEvent): boolean {
	// Ignore standalone modifier keys
	if (
		event.key === "Control" ||
		event.key === "Alt" ||
		event.key === "Shift" ||
		event.key === "Meta"
	) {
		return false;
	}

	// Handle most other keys
	return true;
}
