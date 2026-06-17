/**
 * Word and line boundary detection for selection.
 */

import type { SelectionRange } from "./types";

/**
 * Word boundary detector.
 *
 * Detects word and line boundaries for double-click and triple-click selection.
 *
 * @example
 * ```ts
 * const boundary = new WordBoundary((row) => terminal.getLineText(row));
 *
 * // Get word at position (for double-click)
 * const wordRange = boundary.getWordAt(5, 10);
 *
 * // Get line at position (for triple-click)
 * const lineRange = boundary.getLineAt(10, 80);
 * ```
 */
export class WordBoundary {
	/**
	 * Pattern for word separator characters.
	 * Matches spaces, tabs, newlines, and common punctuation.
	 */
	private static readonly WORD_SEPARATORS =
		/[\s\t\n\r!"#$%&'()*+,\-./:;<=>?@[\\\]^_`{|}~]/;

	/**
	 * Characters that should be included in words.
	 * Used for languages like Japanese where word boundaries are unclear.
	 */
	private static readonly CJK_RANGE =
		/[\u4e00-\u9fff\u3040-\u309f\u30a0-\u30ff\uff00-\uffef]/;

	private getLine: (row: number) => string;
	private cols: number;

	/**
	 * Create a new WordBoundary detector.
	 *
	 * @param getLine - Function to get line text at a given row
	 * @param cols - Number of columns in the terminal
	 */
	constructor(getLine: (row: number) => string, cols: number) {
		this.getLine = getLine;
		this.cols = cols;
	}

	/**
	 * Update the number of columns.
	 *
	 * @param cols - New column count
	 */
	updateCols(cols: number): void {
		this.cols = cols;
	}

	/**
	 * Check if a character is a word separator.
	 *
	 * @param char - Character to check
	 * @returns True if the character is a word separator
	 */
	isWordSeparator(char: string): boolean {
		if (char.length === 0) {
			return true;
		}
		return WordBoundary.WORD_SEPARATORS.test(char);
	}

	/**
	 * Check if a character is CJK (Chinese/Japanese/Korean).
	 *
	 * @param char - Character to check
	 * @returns True if the character is CJK
	 */
	private isCjk(char: string): boolean {
		return WordBoundary.CJK_RANGE.test(char);
	}

	/**
	 * Get the word range at a given position.
	 *
	 * For double-click selection. Finds the word boundaries around the position.
	 *
	 * @param col - Column position
	 * @param row - Row position
	 * @returns Selection range covering the word
	 */
	getWordAt(col: number, row: number): SelectionRange {
		const line = this.getLine(row);

		// Empty line or position beyond line
		if (!line || col >= line.length) {
			return {
				start: { col, row },
				end: { col, row },
			};
		}

		const char = line.charAt(col);

		// If on a separator, select just that character
		if (this.isWordSeparator(char)) {
			// Special case: expand to adjacent separators of same type
			let startCol = col;
			let endCol = col;

			// For spaces, select contiguous spaces
			if (/\s/.test(char)) {
				while (startCol > 0 && /\s/.test(line.charAt(startCol - 1))) {
					startCol--;
				}
				while (endCol < line.length - 1 && /\s/.test(line.charAt(endCol + 1))) {
					endCol++;
				}
			}

			return {
				start: { col: startCol, row },
				end: { col: endCol, row },
			};
		}

		// For CJK characters, select just that character
		// (Word boundaries in CJK are complex and context-dependent)
		if (this.isCjk(char)) {
			return {
				start: { col, row },
				end: { col, row },
			};
		}

		// Find word boundaries
		let startCol = col;
		let endCol = col;

		// Expand left
		while (startCol > 0) {
			const prevChar = line.charAt(startCol - 1);
			if (this.isWordSeparator(prevChar) || this.isCjk(prevChar)) {
				break;
			}
			startCol--;
		}

		// Expand right
		while (endCol < line.length - 1) {
			const nextChar = line.charAt(endCol + 1);
			if (this.isWordSeparator(nextChar) || this.isCjk(nextChar)) {
				break;
			}
			endCol++;
		}

		return {
			start: { col: startCol, row },
			end: { col: endCol, row },
		};
	}

	/**
	 * Get the line range at a given row.
	 *
	 * For triple-click selection. Selects the entire line.
	 *
	 * @param row - Row position
	 * @returns Selection range covering the entire line
	 */
	getLineAt(row: number): SelectionRange {
		const line = this.getLine(row);
		const endCol = line.length > 0 ? line.length - 1 : 0;

		return {
			start: { col: 0, row },
			end: { col: Math.min(endCol, this.cols - 1), row },
		};
	}

	/**
	 * Expand a word selection as the mouse moves.
	 *
	 * Ensures selection stays at word boundaries.
	 *
	 * @param anchorWord - Original word range (from double-click)
	 * @param currentPos - Current mouse position
	 * @returns Expanded selection range
	 */
	expandWordSelection(
		anchorWord: SelectionRange,
		currentPos: { col: number; row: number },
	): SelectionRange {
		// Get word at current position
		const currentWord = this.getWordAt(currentPos.col, currentPos.row);

		// Determine direction and expand appropriately
		if (
			currentPos.row < anchorWord.start.row ||
			(currentPos.row === anchorWord.start.row &&
				currentPos.col < anchorWord.start.col)
		) {
			// Selecting backwards
			return {
				start: currentWord.start,
				end: anchorWord.end,
			};
		} else {
			// Selecting forwards
			return {
				start: anchorWord.start,
				end: currentWord.end,
			};
		}
	}

	/**
	 * Expand a line selection as the mouse moves.
	 *
	 * Ensures selection stays at line boundaries.
	 *
	 * @param anchorRow - Original row (from triple-click)
	 * @param currentRow - Current mouse row
	 * @returns Expanded selection range
	 */
	expandLineSelection(anchorRow: number, currentRow: number): SelectionRange {
		const startRow = Math.min(anchorRow, currentRow);
		const endRow = Math.max(anchorRow, currentRow);

		const startLine = this.getLineAt(startRow);
		const endLine = this.getLineAt(endRow);

		return {
			start: startLine.start,
			end: endLine.end,
		};
	}
}
