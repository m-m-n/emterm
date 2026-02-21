/**
 * Keyboard input handler for terminal emulator.
 *
 * Converts DOM KeyboardEvents to byte sequences suitable for
 * sending to a PTY (pseudo-terminal).
 */

import type { CursorKeysMode } from "../terminal/modes";

/** Reusable TextEncoder instance to avoid per-keystroke allocation. */
const textEncoder = new TextEncoder();

/**
 * Options for keyEventToBytes conversion.
 */
export interface KeyboardOptions {
	cursorKeysMode?: CursorKeysMode;
	shiftEnterAsAltEnter?: boolean;
}

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
	{ key: "Tab", shift: true, sequence: [0x1b, 0x5b, 0x5a] }, // ESC [ Z (back-tab)
	{ key: "Enter", sequence: [0x0d] }, // CR (Carriage Return)
	{ key: "Enter", shift: true, sequence: [0x0d] }, // Shift+Enter -> CR (default; overridden by shiftEnterAsAltEnter option)
	{ key: "Enter", alt: true, sequence: [0x1b, 0x0d] }, // Alt+Enter -> ESC + CR
	{ key: "Tab", sequence: [0x09] }, // HT (Horizontal Tab)
	{ key: "Backspace", sequence: [0x7f] }, // DEL
	{ key: "Backspace", shift: true, sequence: [0x7f] }, // Shift+Backspace -> same as Backspace
	{ key: "Escape", sequence: [0x1b] }, // ESC
	{ key: "Escape", shift: true, sequence: [0x1b] }, // Shift+Escape -> same as Escape
	{ key: "Escape", ctrl: true, sequence: [0x1b] }, // Ctrl+[ (browser reports as Escape with ctrlKey)
];

/**
 * Computes the xterm modifier parameter from modifier key states.
 *
 * The formula is: 1 + shift_bit + alt_bit*2 + ctrl_bit*4
 * Returns 0 when no modifiers are pressed.
 *
 * @param shift - Shift key pressed
 * @param alt - Alt key pressed
 * @param ctrl - Ctrl key pressed
 * @returns Modifier parameter (0 for none, 2-8 for combinations)
 */
export function calcModifierParam(
	shift: boolean,
	alt: boolean,
	ctrl: boolean,
): number {
	const bits =
		(shift ? 1 : 0) + (alt ? 2 : 0) + (ctrl ? 4 : 0);
	return bits === 0 ? 0 : bits + 1;
}

/** Arrow key -> suffix letter */
const ARROW_KEY_LETTERS: Record<string, number> = {
	ArrowUp: 0x41, // A
	ArrowDown: 0x42, // B
	ArrowRight: 0x43, // C
	ArrowLeft: 0x44, // D
};

/** Navigation key -> suffix letter (Home/End) */
const NAV_KEY_LETTERS: Record<string, number> = {
	Home: 0x48, // H
	End: 0x46, // F
};

/** Tilde-style key -> CSI number string */
const TILDE_KEY_NUMBERS: Record<string, string> = {
	Insert: "2",
	Delete: "3",
	PageUp: "5",
	PageDown: "6",
};

/** F1-F4 -> suffix letter */
const FKEY_LETTERS: Record<string, number> = {
	F1: 0x50, // P
	F2: 0x51, // Q
	F3: 0x52, // R
	F4: 0x53, // S
};

/** F5-F12 -> CSI number string */
const FKEY_TILDE_NUMBERS: Record<string, string> = {
	F5: "15",
	F6: "17",
	F7: "18",
	F8: "19",
	F9: "20",
	F10: "21",
	F11: "23",
	F12: "24",
};

/**
 * Encodes a letter-style modified sequence: ESC [ {prefix} ; {mod} {letter}
 */
function encodeModifiedLetterSeq(
	prefix: string,
	mod: number,
	letter: number,
): Uint8Array {
	const seq = `\x1b[${prefix};${mod}`;
	const bytes = new Uint8Array(seq.length + 1);
	for (let i = 0; i < seq.length; i++) {
		bytes[i] = seq.charCodeAt(i);
	}
	bytes[seq.length] = letter;
	return bytes;
}

/**
 * Encodes a tilde-style modified sequence: ESC [ {num} ; {mod} ~
 */
function encodeModifiedTildeSeq(num: string, mod: number): Uint8Array {
	const seq = `\x1b[${num};${mod}~`;
	const bytes = new Uint8Array(seq.length);
	for (let i = 0; i < seq.length; i++) {
		bytes[i] = seq.charCodeAt(i);
	}
	return bytes;
}

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
	cursorKeysModeOrOptions?: CursorKeysMode | KeyboardOptions,
): Uint8Array | null {
	// Normalize arguments: support both legacy positional and new options form
	const options: KeyboardOptions =
		typeof cursorKeysModeOrOptions === "object" && cursorKeysModeOrOptions !== null
			? cursorKeysModeOrOptions
			: { cursorKeysMode: cursorKeysModeOrOptions ?? "normal" };
	const cursorKeysMode = options.cursorKeysMode ?? "normal";

	// Handle Shift+Enter → Alt+Enter remapping (when option is enabled)
	if (
		options.shiftEnterAsAltEnter &&
		event.key === "Enter" &&
		event.shiftKey &&
		!event.ctrlKey &&
		!event.altKey
	) {
		return new Uint8Array([0x1b, 0x0d]); // ESC + CR (same as Alt+Enter)
	}

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

		// Ctrl + symbol (@[\]^_) -> control characters via bitwise AND
		const code = event.key.charCodeAt(0);
		if (code >= 0x40 && code <= 0x5f) {
			return new Uint8Array([code & 0x1f]);
		}

		// Ctrl + Space -> NUL (0x00)
		if (event.key === " ") {
			return new Uint8Array([0x00]);
		}
	}

	// Modified special keys (Ctrl/Shift/Alt + Arrow/Home/End/Delete/PageUp/PageDown/F-keys)
	{
		const mod = calcModifierParam(event.shiftKey, event.altKey, event.ctrlKey);
		if (mod > 0) {
			// Arrow keys -> ESC [1;{mod}{letter}
			const arrowLetter = ARROW_KEY_LETTERS[event.key];
			if (arrowLetter !== undefined) {
				return encodeModifiedLetterSeq("1", mod, arrowLetter);
			}

			// Home/End -> ESC [1;{mod}{letter}
			const navLetter = NAV_KEY_LETTERS[event.key];
			if (navLetter !== undefined) {
				return encodeModifiedLetterSeq("1", mod, navLetter);
			}

			// Delete/Insert/PageUp/PageDown -> ESC [{num};{mod}~
			const tildeNum = TILDE_KEY_NUMBERS[event.key];
			if (tildeNum !== undefined) {
				return encodeModifiedTildeSeq(tildeNum, mod);
			}

			// F1-F4 -> ESC [1;{mod}{letter}
			const fkeyLetter = FKEY_LETTERS[event.key];
			if (fkeyLetter !== undefined) {
				return encodeModifiedLetterSeq("1", mod, fkeyLetter);
			}

			// F5-F12 -> ESC [{num};{mod}~
			const fkeyTildeNum = FKEY_TILDE_NUMBERS[event.key];
			if (fkeyTildeNum !== undefined) {
				return encodeModifiedTildeSeq(fkeyTildeNum, mod);
			}
		}
	}

	// Ctrl+Alt + letter -> ESC + control character
	if (event.altKey && event.ctrlKey && event.key.length === 1) {
		const char = event.key.toLowerCase();
		if (char >= "a" && char <= "z") {
			return new Uint8Array([0x1b, char.charCodeAt(0) - 96]);
		}
	}

	// Alt + key -> ESC prefix followed by the key
	if (event.altKey && !event.ctrlKey && event.key.length === 1) {
		const bytes = textEncoder.encode(event.key);
		const result = new Uint8Array(bytes.length + 1);
		result[0] = 0x1b; // ESC
		result.set(bytes, 1);
		return result;
	}

	// Regular printable character (no modifiers except Shift)
	if (event.key.length === 1 && !event.ctrlKey && !event.altKey) {
		return textEncoder.encode(event.key);
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
