/**
 * Clipboard operations bridge.
 *
 * Handles copying to and pasting from the system clipboard.
 */

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
			await navigator.clipboard.writeText(text);
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
			return await navigator.clipboard.readText();
		} catch (error) {
			console.error("Failed to read from clipboard:", error);
			return "";
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
