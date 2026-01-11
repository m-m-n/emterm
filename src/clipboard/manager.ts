/**
 * Clipboard operations manager.
 * Handles copying to and pasting from the system clipboard.
 */

/**
 * Manages clipboard operations using the Clipboard API.
 *
 * Responsibilities:
 * - Copy text to system clipboard
 * - Paste text from system clipboard
 * - Detect multi-line content
 * - Count lines in text
 *
 * @example
 * ```ts
 * const manager = new ClipboardManager();
 * await manager.copyToClipboard("Hello, World!");
 * const text = await manager.pasteFromClipboard();
 * ```
 */
export class ClipboardManager {
	/**
	 * Copy text to the system clipboard.
	 *
	 * @param text - Text to copy
	 * @returns Promise resolving to true on success, false on failure
	 *
	 * @example
	 * ```ts
	 * const success = await manager.copyToClipboard("Hello");
	 * if (success) {
	 *   console.log("Text copied");
	 * }
	 * ```
	 */
	async copyToClipboard(text: string): Promise<boolean> {
		try {
			await navigator.clipboard.writeText(text);
			return true;
		} catch (error) {
			console.error("Failed to copy to clipboard:", error);
			return false;
		}
	}

	/**
	 * Paste text from the system clipboard.
	 *
	 * @returns Promise resolving to clipboard text, or empty string on failure
	 *
	 * @example
	 * ```ts
	 * const text = await manager.pasteFromClipboard();
	 * console.log("Pasted:", text);
	 * ```
	 */
	async pasteFromClipboard(): Promise<string> {
		try {
			return await navigator.clipboard.readText();
		} catch (error) {
			console.error("Failed to read from clipboard:", error);
			return "";
		}
	}

	/**
	 * Check if text contains newline characters.
	 *
	 * Detects LF (\n), CR (\r), or CRLF (\r\n).
	 *
	 * @param text - Text to check
	 * @returns True if text contains newlines
	 *
	 * @example
	 * ```ts
	 * manager.hasNewlines("Single line"); // false
	 * manager.hasNewlines("Line 1\nLine 2"); // true
	 * ```
	 */
	hasNewlines(text: string): boolean {
		return /[\r\n]/.test(text);
	}

	/**
	 * Count the number of lines in text.
	 *
	 * Lines are separated by LF (\n), CR (\r), or CRLF (\r\n).
	 * Empty string is considered 1 line.
	 *
	 * @param text - Text to count
	 * @returns Number of lines
	 *
	 * @example
	 * ```ts
	 * manager.countLines("Single line"); // 1
	 * manager.countLines("Line 1\nLine 2"); // 2
	 * manager.countLines(""); // 1
	 * ```
	 */
	countLines(text: string): number {
		if (text === "") {
			return 1;
		}

		// Split by any newline type and count resulting array length
		return text.split(/\r\n|\r|\n/).length;
	}
}
