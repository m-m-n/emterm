/**
 * Clipboard operations bridge.
 *
 * Handles copying to and pasting from the system clipboard.
 */

import {
	readText,
	writeText,
} from "@tauri-apps/plugin-clipboard-manager";
import { invoke } from "@tauri-apps/api/core";
import { isLinux } from "../platform";

/**
 * Clipboard bridge.
 *
 * Provides clipboard read/write operations with error handling.
 *
 * @example
 * ```ts
 * const clipboard = new ClipboardBridge();
 *
 * // Copy text
 * const success = await clipboard.write("Hello, World!");
 *
 * // Paste text
 * const text = await clipboard.read();
 *
 * // Check for multi-line content
 * if (clipboard.isMultiLine(text)) {
 *   // Show confirmation dialog
 * }
 * ```
 */
export class ClipboardBridge {
	/**
	 * Write text to the system clipboard.
	 *
	 * @param text - Text to copy
	 * @returns True on success, false on failure
	 */
	async write(text: string): Promise<boolean> {
		try {
			await writeText(text);
			return true;
		} catch (error) {
			console.error("Failed to write to clipboard:", error);
			return false;
		}
	}

	/**
	 * Read text from the system clipboard.
	 *
	 * @returns Clipboard text or empty string on failure
	 */
	async read(): Promise<string> {
		try {
			return await readText();
		} catch (error) {
			console.error("Failed to read from clipboard:", error);
			return "";
		}
	}

	/**
	 * Write text to the Linux PRIMARY selection (select-to-copy buffer).
	 *
	 * On non-Linux platforms this is a no-op that returns `false` without
	 * invoking any Tauri command. On Linux, failures are caught and logged
	 * via `console.warn` — callers treat the return value only as an
	 * informational flag and should never change control flow on failure.
	 *
	 * @param text - Text to write
	 * @returns `true` on a successful Linux write, `false` otherwise
	 */
	async writePrimary(text: string): Promise<boolean> {
		if (!isLinux()) return false;
		try {
			await invoke("clipboard_write_primary", { text });
			return true;
		} catch (error) {
			console.warn("[WARN][FRONTEND] Failed to write PRIMARY:", error);
			return false;
		}
	}

	/**
	 * Read the current Linux PRIMARY selection.
	 *
	 * Returns:
	 * - `""` (empty string) when PRIMARY is genuinely empty on Linux, or on
	 *   non-Linux platforms (no IPC call is dispatched).
	 * - A non-empty string when PRIMARY contains text.
	 * - `null` on Linux when the read failed (backend init error, mutex
	 *   poisoning, etc.). The error is logged via `console.warn`.
	 *
	 * The `null` return value lets callers distinguish a real failure from
	 * an empty PRIMARY: a CLIPBOARD fallback is only safe when PRIMARY is
	 * genuinely empty, not when the read errored — falling back on error
	 * would defeat the privacy goal of keeping PRIMARY and CLIPBOARD
	 * separate.
	 *
	 * @returns PRIMARY text, `""` if empty/non-Linux, `null` on read error
	 */
	async readPrimary(): Promise<string | null> {
		if (!isLinux()) return "";
		try {
			return await invoke<string>("clipboard_read_primary");
		} catch (error) {
			console.warn("[WARN][FRONTEND] Failed to read PRIMARY:", error);
			return null;
		}
	}

	/**
	 * Check if text contains multiple lines.
	 *
	 * @param text - Text to check
	 * @returns True if text contains newline characters
	 */
	isMultiLine(text: string): boolean {
		return /[\r\n]/.test(text);
	}

	/**
	 * Count the number of lines in text.
	 *
	 * @param text - Text to count
	 * @returns Number of lines (empty string counts as 1)
	 */
	countLines(text: string): number {
		if (text === "") {
			return 1;
		}
		return text.split(/\r\n|\r|\n/).length;
	}
}
