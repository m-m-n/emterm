/**
 * Settings functions for the terminal canvas renderer.
 *
 * Extracted from CanvasRenderer: functions for applying font,
 * cursor, and color scheme settings.
 *
 * NOTE: These functions return new values but do NOT trigger
 * measureCharacterSize() or forceRender(). The caller
 * (CanvasRenderer) is responsible for applying the returned
 * value first, then measuring/rendering in the correct order.
 */

import type { Rgb } from "./colors.ts";
import {
	buildPalette256,
	DEFAULT_BACKGROUND,
	DEFAULT_FOREGROUND,
	getColorSchemePreset,
	hexToRgb,
	PALETTE_16,
} from "./colors.ts";
import type { UserColorScheme } from "../settings/types";
import type { CursorStyle } from "./cursor.ts";
import type { TerminalState } from "./state.ts";

/**
 * Mutable color state managed by the renderer.
 */
export interface ColorState {
	currentForeground: Rgb;
	currentBackground: Rgb;
	currentCursorColor: Rgb;
	currentPalette16: readonly Rgb[];
	currentPalette256: readonly Rgb[];
	boldBrightensAnsiColors: boolean;
}

/**
 * Callbacks for triggering re-measurement and re-rendering.
 */
export interface SettingsCallbacks {
	measureCharacterSize: () => void;
	forceRender: (state: TerminalState) => void;
	startCursorBlink: () => void;
	stopCursorBlink: () => void;
	getPendingState: () => TerminalState | null;
}

/**
 * Convert font size from points to pixels.
 * @param fontSizePt - Font size in points
 * @returns Font size in pixels
 */
export function setFontSize(
	fontSizePt: number,
	_callbacks: SettingsCallbacks,
): number {
	return fontSizePt * (96 / 72);
}

/**
 * Get the font size in points from pixels.
 */
export function getFontSizePt(fontSizePx: number): number {
	return fontSizePx * (72 / 96);
}

/**
 * Resolve font family string.
 * @returns Resolved font family string
 */
export function setFontFamily(
	fontFamily: string,
	_callbacks: SettingsCallbacks,
): string {
	return fontFamily || "monospace";
}

/**
 * Set the cursor style.
 */
export function setCursorStyle(
	style: CursorStyle,
	callbacks: SettingsCallbacks,
): void {
	const state = callbacks.getPendingState();
	if (state) {
		state.cursor.style = style;
		callbacks.forceRender(state);
	}
}

/**
 * Set cursor blink mode.
 */
export function setCursorBlink(
	blink: boolean,
	callbacks: SettingsCallbacks,
): void {
	const state = callbacks.getPendingState();
	if (state) {
		state.modes.cursorBlink = blink;
	}
	if (blink) {
		callbacks.startCursorBlink();
	} else {
		callbacks.stopCursorBlink();
		if (state) {
			callbacks.forceRender(state);
		}
	}
}

/**
 * Set the color scheme. Mutates the provided colorState.
 * Caller is responsible for applying colorState and rendering.
 */
export function setColorScheme(
	schemeName: string,
	colorState: ColorState,
): void {
	const preset = getColorSchemePreset(schemeName);

	if (!preset || schemeName === "emterm") {
		colorState.currentForeground = DEFAULT_FOREGROUND;
		colorState.currentBackground = DEFAULT_BACKGROUND;
		colorState.currentCursorColor = { r: 0, g: 128, b: 0 };
		colorState.currentPalette16 = PALETTE_16;
	} else {
		colorState.currentForeground = preset.foreground;
		colorState.currentBackground = preset.background;
		colorState.currentCursorColor = preset.cursor;
		colorState.currentPalette16 = preset.ansiColors;
	}

	colorState.currentPalette256 = buildPalette256(colorState.currentPalette16);
}

/**
 * Set a user-defined color scheme. Mutates the provided colorState.
 * Caller is responsible for applying colorState and rendering.
 */
export function setUserColorScheme(
	scheme: UserColorScheme,
	colorState: ColorState,
): void {
	const fg = hexToRgb(scheme.foreground);
	const bg = hexToRgb(scheme.background);
	const cursor = hexToRgb(scheme.cursor);

	if (fg) colorState.currentForeground = fg;
	if (bg) colorState.currentBackground = bg;
	if (cursor) colorState.currentCursorColor = cursor;

	const ansiColors: Rgb[] = [];
	for (const hex of scheme.ansi_colors) {
		const rgb = hexToRgb(hex);
		if (rgb) {
			ansiColors.push(rgb);
		}
	}
	if (ansiColors.length === 16) {
		colorState.currentPalette16 = ansiColors;
	}

	colorState.currentPalette256 = buildPalette256(colorState.currentPalette16);
}

/**
 * Set bold-brightens ANSI colors behavior. Mutates the provided colorState.
 * Caller is responsible for applying colorState and rendering.
 */
export function setBoldBrightensAnsiColors(
	enabled: boolean,
	colorState: ColorState,
): void {
	colorState.boldBrightensAnsiColors = enabled;
}
