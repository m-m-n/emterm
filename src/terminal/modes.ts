/**
 * Terminal mode management.
 *
 * Handles DEC private modes and terminal settings.
 */

/**
 * Mouse tracking mode.
 */
export type MouseTrackingMode =
	| "none"
	| "x10" // Mode 1000: X10 mouse reporting
	| "button" // Mode 1002: Button-event tracking
	| "any"; // Mode 1003: Any-event tracking

/**
 * Mouse encoding format.
 */
export type MouseEncoding =
	| "default" // Normal X10-style encoding
	| "utf8" // Mode 1005: UTF-8 encoding
	| "sgr"; // Mode 1006: SGR encoding

/**
 * Cursor keys application mode.
 */
export type CursorKeysMode = "normal" | "application";

/**
 * Terminal modes state.
 *
 * Contains all DEC private modes and other terminal settings.
 */
export interface TerminalModes {
	// DECCKM (1): Cursor keys mode
	cursorKeys: CursorKeysMode;

	// DECCOLM (3): Column mode (132/80)
	// Not typically implemented in modern terminals, track for compatibility
	column132: boolean;

	// DECSCNM (5): Screen mode (reverse video)
	reverseScreen: boolean;

	// DECOM (6): Origin mode
	// When set, cursor addressing is relative to scroll region
	originMode: boolean;

	// DECAWM (7): Auto wrap mode
	autoWrap: boolean;

	// ATT160 (12): Cursor blink
	cursorBlink: boolean;

	// DECTCEM (25): Cursor visibility
	cursorVisible: boolean;

	// Mouse tracking modes
	mouseTracking: MouseTrackingMode;
	mouseEncoding: MouseEncoding;

	// Focus tracking (1004)
	focusTracking: boolean;

	// Bracketed paste (2004)
	bracketedPaste: boolean;

	// Synchronized output (2026)
	synchronizedOutput: boolean;
}

/**
 * Create default terminal modes.
 */
export function createDefaultModes(): TerminalModes {
	return {
		cursorKeys: "normal",
		column132: false,
		reverseScreen: false,
		originMode: false,
		autoWrap: true,
		cursorBlink: true,
		cursorVisible: true,
		mouseTracking: "none",
		mouseEncoding: "default",
		focusTracking: false,
		bracketedPaste: false,
		synchronizedOutput: false,
	};
}

/**
 * Clone terminal modes.
 */
export function cloneModes(modes: TerminalModes): TerminalModes {
	return { ...modes };
}

/**
 * DEC Private mode numbers.
 */
export const DECPrivateMode = {
	DECCKM: 1, // Cursor keys mode
	DECCOLM: 3, // Column mode
	DECSCNM: 5, // Screen mode (reverse)
	DECOM: 6, // Origin mode
	DECAWM: 7, // Auto wrap mode
	ATT160: 12, // Cursor blink
	DECTCEM: 25, // Cursor visibility
	XTERM_ALTBUF_47: 47, // Alternate buffer
	X10_MOUSE: 1000, // X10 mouse reporting
	BTN_EVENT_MOUSE: 1002, // Button-event mouse tracking
	ANY_EVENT_MOUSE: 1003, // Any-event mouse tracking
	FOCUS_TRACKING: 1004, // Focus tracking
	UTF8_MOUSE: 1005, // UTF-8 mouse mode
	SGR_MOUSE: 1006, // SGR mouse mode
	XTERM_ALTBUF_1047: 1047, // Alternate buffer
	XTERM_SAVE: 1048, // Save cursor
	XTERM_ALTBUF_1049: 1049, // Alternate buffer + save cursor
	BRACKETED_PASTE: 2004, // Bracketed paste mode
	SYNCHRONIZED_OUTPUT: 2026, // Synchronized output
} as const;

/**
 * Set a DEC private mode.
 *
 * @param modes - Terminal modes to modify
 * @param mode - Mode number to set
 * @param value - true to enable, false to disable
 * @returns Object with mode changed flag and any special action required
 */
export function setDecPrivateMode(
	modes: TerminalModes,
	mode: number,
	value: boolean,
): {
	changed: boolean;
	action?:
		| "saveAndSwitchToAlt"
		| "switchToAlt"
		| "switchToMain"
		| "saveCursor"
		| "restoreCursor";
} {
	let changed = false;
	let action:
		| "saveAndSwitchToAlt"
		| "switchToAlt"
		| "switchToMain"
		| "saveCursor"
		| "restoreCursor"
		| undefined;

	switch (mode) {
		case DECPrivateMode.DECCKM:
			changed = modes.cursorKeys !== (value ? "application" : "normal");
			modes.cursorKeys = value ? "application" : "normal";
			break;

		case DECPrivateMode.DECCOLM:
			changed = modes.column132 !== value;
			modes.column132 = value;
			break;

		case DECPrivateMode.DECSCNM:
			changed = modes.reverseScreen !== value;
			modes.reverseScreen = value;
			break;

		case DECPrivateMode.DECOM:
			changed = modes.originMode !== value;
			modes.originMode = value;
			break;

		case DECPrivateMode.DECAWM:
			changed = modes.autoWrap !== value;
			modes.autoWrap = value;
			break;

		case DECPrivateMode.ATT160:
			changed = modes.cursorBlink !== value;
			modes.cursorBlink = value;
			break;

		case DECPrivateMode.DECTCEM:
			changed = modes.cursorVisible !== value;
			modes.cursorVisible = value;
			break;

		case DECPrivateMode.XTERM_ALTBUF_47:
		case DECPrivateMode.XTERM_ALTBUF_1047:
			action = value ? "switchToAlt" : "switchToMain";
			changed = true;
			break;

		case DECPrivateMode.XTERM_SAVE:
			action = value ? "saveCursor" : "restoreCursor";
			changed = true;
			break;

		case DECPrivateMode.XTERM_ALTBUF_1049:
			if (value) {
				action = "saveAndSwitchToAlt";
			} else {
				action = "switchToMain";
			}
			changed = true;
			break;

		case DECPrivateMode.X10_MOUSE:
			if (value) {
				changed = modes.mouseTracking !== "x10";
				modes.mouseTracking = "x10";
			} else if (modes.mouseTracking === "x10") {
				changed = true;
				modes.mouseTracking = "none";
			}
			break;

		case DECPrivateMode.BTN_EVENT_MOUSE:
			if (value) {
				changed = modes.mouseTracking !== "button";
				modes.mouseTracking = "button";
			} else if (modes.mouseTracking === "button") {
				changed = true;
				modes.mouseTracking = "none";
			}
			break;

		case DECPrivateMode.ANY_EVENT_MOUSE:
			if (value) {
				changed = modes.mouseTracking !== "any";
				modes.mouseTracking = "any";
			} else if (modes.mouseTracking === "any") {
				changed = true;
				modes.mouseTracking = "none";
			}
			break;

		case DECPrivateMode.FOCUS_TRACKING:
			changed = modes.focusTracking !== value;
			modes.focusTracking = value;
			break;

		case DECPrivateMode.UTF8_MOUSE:
			if (value) {
				changed = modes.mouseEncoding !== "utf8";
				modes.mouseEncoding = "utf8";
			} else if (modes.mouseEncoding === "utf8") {
				changed = true;
				modes.mouseEncoding = "default";
			}
			break;

		case DECPrivateMode.SGR_MOUSE:
			if (value) {
				changed = modes.mouseEncoding !== "sgr";
				modes.mouseEncoding = "sgr";
			} else if (modes.mouseEncoding === "sgr") {
				changed = true;
				modes.mouseEncoding = "default";
			}
			break;

		case DECPrivateMode.BRACKETED_PASTE:
			changed = modes.bracketedPaste !== value;
			modes.bracketedPaste = value;
			break;

		case DECPrivateMode.SYNCHRONIZED_OUTPUT:
			changed = modes.synchronizedOutput !== value;
			modes.synchronizedOutput = value;
			break;
	}

	return { changed, action };
}

// ── WASM mode bit mapping ────────────────────────────────

/**
 * WASM mode bit constants matching Rust MODE_* constants.
 * Only boolean modes are mapped (multi-valued modes like
 * mouseTracking/mouseEncoding/cursorKeys stay in JS).
 */
export const WASM_MODE_BITS = {
	autoWrap: 0,
	originMode: 1,
	cursorVisible: 2,
	cursorBlink: 3,
	reverseScreen: 4,
	bracketedPaste: 5,
	focusTracking: 6,
	column132: 7,
	synchronizedOutput: 8,
} as const;

/**
 * WASM-backed mode storage interface.
 * Structurally compatible with TerminalCore from wasm_bindgen.
 */
export interface WasmModeCore {
	get_mode(bit: number): boolean;
	set_mode(bit: number, value: boolean): void;
}

/**
 * Sync all boolean mode flags from JS TerminalModes to WASM bitfield.
 * Call after mode changes to keep WASM state in sync.
 */
export function syncModesToWasm(modes: TerminalModes, core: WasmModeCore): void {
	core.set_mode(WASM_MODE_BITS.autoWrap, modes.autoWrap);
	core.set_mode(WASM_MODE_BITS.originMode, modes.originMode);
	core.set_mode(WASM_MODE_BITS.cursorVisible, modes.cursorVisible);
	core.set_mode(WASM_MODE_BITS.cursorBlink, modes.cursorBlink);
	core.set_mode(WASM_MODE_BITS.reverseScreen, modes.reverseScreen);
	core.set_mode(WASM_MODE_BITS.bracketedPaste, modes.bracketedPaste);
	core.set_mode(WASM_MODE_BITS.focusTracking, modes.focusTracking);
	core.set_mode(WASM_MODE_BITS.column132, modes.column132);
	core.set_mode(WASM_MODE_BITS.synchronizedOutput, modes.synchronizedOutput);
}

/**
 * Sync all boolean mode flags from WASM bitfield to JS TerminalModes.
 * Call after buffer switches to restore WASM-stored mode state.
 */
export function syncModesFromWasm(modes: TerminalModes, core: WasmModeCore): void {
	modes.autoWrap = core.get_mode(WASM_MODE_BITS.autoWrap);
	modes.originMode = core.get_mode(WASM_MODE_BITS.originMode);
	modes.cursorVisible = core.get_mode(WASM_MODE_BITS.cursorVisible);
	modes.cursorBlink = core.get_mode(WASM_MODE_BITS.cursorBlink);
	modes.reverseScreen = core.get_mode(WASM_MODE_BITS.reverseScreen);
	modes.bracketedPaste = core.get_mode(WASM_MODE_BITS.bracketedPaste);
	modes.focusTracking = core.get_mode(WASM_MODE_BITS.focusTracking);
	modes.column132 = core.get_mode(WASM_MODE_BITS.column132);
	modes.synchronizedOutput = core.get_mode(WASM_MODE_BITS.synchronizedOutput);
}
