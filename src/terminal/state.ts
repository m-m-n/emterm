/**
 * Terminal state management.
 *
 * Processes terminal actions and maintains screen state.
 */

import { MarkdownSessionManager } from "../markdown/session.ts";
import type { MarkdownBlock } from "../markdown/types.ts";
import type {
	ApcAction,
	CharSet,
	CsiAction,
	DcsAction,
	EscAction,
	OscAction,
	TerminalAction,
} from "../types/terminal.ts";
import { C0 } from "../types/terminal.ts";
import { applySgrAttr, type SgrAttr } from "./attributes.ts";
import { ScreenBuffer } from "./buffer.ts";
import { CursorState } from "./cursor.ts";
import { createAsciiCell, createCell } from "./grid.ts";
import {
	cloneModes,
	createDefaultModes,
	setDecPrivateMode,
	type TerminalModes,
} from "./modes.ts";
import { parseSgrParams } from "./sgr.ts";
import { charWidth } from "./unicode.ts";

/**
 * Active character set (G0 or G1).
 */
type ActiveCharSet = "G0" | "G1";

/**
 * Terminal state manager.
 *
 * Receives parsed terminal actions and updates the screen buffer.
 */
export class TerminalState {
	/** Primary screen buffer. */
	private primaryBuffer: ScreenBuffer;

	/** Alternate screen buffer. */
	private alternateBuffer: ScreenBuffer | null = null;

	/** Whether alternate buffer is active. */
	private useAlternate: boolean = false;

	/** Cursor state for primary buffer. */
	private primaryCursor: CursorState;

	/** Cursor state for alternate buffer (saved when switching). */
	private alternateCursor: CursorState | null = null;

	/** Current cursor (points to active buffer's cursor). */
	private cursor: CursorState;

	/** Terminal modes. */
	private modes: TerminalModes;

	/** Pending wrap flag - next character will wrap. */
	private wrapPending: boolean = false;

	/** Tab stops (column indices where tab stops are set). */
	private tabStops: Set<number>;

	/** G0 character set. */
	private g0CharSet: CharSet = "Ascii";

	/** G1 character set. */
	private g1CharSet: CharSet = "Ascii";

	/** Active character set (G0 or G1). */
	private activeCharSet: ActiveCharSet = "G0";

	/** Saved cursor for alternate buffer switch (1049). */
	private savedCursorForAlt: CursorState | null = null;

	/** Window title. */
	private _title: string = "";

	/** Window icon name. */
	private _iconName: string = "";

	/** Current working directory (from OSC 7). */
	private _workingDirectory: string = "";

	/** Pending response bytes to write back to PTY (buffered to handle multiple DSRs). */
	private _pendingResponses: Uint8Array[] = [];

	/** Active hyperlink (from OSC 8). */
	private _activeHyperlink: { params: string; uri: string } | null = null;

	/** Markdown session manager. */
	private markdownManager: MarkdownSessionManager;

	/** Pending markdown blocks to be rendered. */
	private _pendingMarkdownBlocks: MarkdownBlock[] = [];

	/**
	 * Create a new terminal state.
	 *
	 * @param cols - Number of columns
	 * @param rows - Number of rows
	 */
	constructor(cols: number, rows: number) {
		this.primaryBuffer = new ScreenBuffer(cols, rows);
		this.primaryCursor = new CursorState(cols, rows);
		this.cursor = this.primaryCursor;
		this.modes = createDefaultModes();
		this.tabStops = this.createDefaultTabStops(cols);
		this.markdownManager = new MarkdownSessionManager();
	}

	/**
	 * Create default tab stops (every 8 columns).
	 */
	private createDefaultTabStops(cols: number): Set<number> {
		const stops = new Set<number>();
		for (let i = 8; i < cols; i += 8) {
			stops.add(i);
		}
		return stops;
	}

	/** Get number of columns. */
	get cols(): number {
		return this.cursor.cols;
	}

	/** Get number of rows. */
	get rows(): number {
		return this.cursor.rows;
	}

	/** Get current cursor column. */
	get cursorCol(): number {
		return this.cursor.col;
	}

	/** Get current cursor row. */
	get cursorRow(): number {
		return this.cursor.row;
	}

	/** Get cursor visibility. */
	get cursorVisible(): boolean {
		return this.modes.cursorVisible;
	}

	/** Get cursor blink mode. */
	get cursorBlink(): boolean {
		return this.modes.cursorBlink;
	}

	/** Get cursor style. */
	get cursorStyle(): "block" | "underline" | "bar" {
		return this.cursor.style;
	}

	/** Get terminal modes. */
	getModes(): Readonly<TerminalModes> {
		return this.modes;
	}

	/** Check if using alternate buffer. */
	get isAlternateBuffer(): boolean {
		return this.useAlternate;
	}

	/** Get window title. */
	get title(): string {
		return this._title;
	}

	/** Get icon name. */
	get iconName(): string {
		return this._iconName;
	}

	/** Get working directory. */
	get workingDirectory(): string {
		return this._workingDirectory;
	}

	/** Get active hyperlink. */
	get activeHyperlink(): { params: string; uri: string } | null {
		return this._activeHyperlink;
	}

	/**
	 * Get and clear pending response bytes.
	 * Call this after processing actions to get data that should be written back to PTY.
	 *
	 * Returns all buffered responses concatenated together to handle multiple DSRs.
	 */
	takePendingResponse(): Uint8Array | null {
		if (this._pendingResponses.length === 0) {
			return null;
		}

		// Concatenate all pending responses
		const totalLength = this._pendingResponses.reduce(
			(sum, r) => sum + r.length,
			0,
		);
		const combined = new Uint8Array(totalLength);
		let offset = 0;
		for (const response of this._pendingResponses) {
			combined.set(response, offset);
			offset += response.length;
		}

		// Clear the buffer
		this._pendingResponses = [];
		return combined;
	}

	/**
	 * Get the active screen buffer.
	 */
	getActiveBuffer(): ScreenBuffer {
		return this.useAlternate && this.alternateBuffer
			? this.alternateBuffer
			: this.primaryBuffer;
	}

	/**
	 * Switch to alternate screen buffer.
	 *
	 * @param saveCursor - Whether to save cursor before switching
	 *
	 * Ensures consistent state:
	 * - Cursor is saved before switching if requested
	 * - Alternate buffer is cleared on each switch
	 * - Cursor is reset to home position (0, 0)
	 */
	private switchToAlternateBuffer(saveCursor: boolean = false): void {
		if (this.useAlternate) return;

		if (saveCursor) {
			// Save primary cursor for 1049 mode
			this.savedCursorForAlt = this.primaryCursor.clone();
		}

		// Create or reset alternate buffer
		if (!this.alternateBuffer) {
			this.alternateBuffer = new ScreenBuffer(this.cols, this.rows);
			this.alternateCursor = new CursorState(this.cols, this.rows);
		} else {
			// Clear alternate buffer on switch
			this.alternateBuffer.clearAll();
			// Reset alternate cursor to home position
			if (!this.alternateCursor) {
				this.alternateCursor = new CursorState(this.cols, this.rows);
			} else {
				this.alternateCursor.moveTo(0, 0);
			}
		}

		// Switch to alternate buffer
		this.useAlternate = true;
		this.cursor = this.alternateCursor!;
		this.wrapPending = false;

		// Mark all lines as dirty to force redraw
		for (let row = 0; row < this.rows; row++) {
			this.alternateBuffer.getLine(row).dirty = true;
		}
	}

	/**
	 * Switch to primary screen buffer.
	 *
	 * @param restoreCursor - Whether to restore cursor after switching
	 *
	 * Ensures consistent state:
	 * - Cursor is restored if requested (mode 1049)
	 * - All lines marked dirty for redraw
	 * - Wrap state is cleared
	 */
	private switchToPrimaryBuffer(restoreCursor: boolean = false): void {
		if (!this.useAlternate) return;

		// Switch to primary buffer
		this.useAlternate = false;
		this.cursor = this.primaryCursor;

		// Restore cursor if requested (for mode 1049)
		if (restoreCursor && this.savedCursorForAlt) {
			this.primaryCursor.restoreFrom(this.savedCursorForAlt);
			this.savedCursorForAlt = null;
		}

		this.wrapPending = false;

		// Mark all lines as dirty to force redraw
		const buffer = this.getActiveBuffer();
		for (let row = 0; row < this.rows; row++) {
			buffer.getLine(row).dirty = true;
		}
	}

	/**
	 * Process a terminal action.
	 *
	 * @param action - The action to process
	 */
	processAction(action: TerminalAction): void {
		switch (action.type) {
			case "Print":
				this.handlePrint(action.value);
				break;
			case "Execute":
				this.handleExecute(action.value);
				break;
			case "Csi":
				this.handleCsi(action.value);
				break;
			case "Esc":
				this.handleEsc(action.value);
				break;
			case "Osc":
				this.handleOsc(action.value);
				break;
			case "Apc":
				this.handleApc(action.value);
				break;
			case "Dcs":
				this.handleDcs(action.value);
				break;
		}
	}

	/**
	 * Handle printable character.
	 */
	private handlePrint(char: string): void {
		// Fast path for ASCII characters without line drawing and without wrap pending
		const code = char.charCodeAt(0);
		if (
			code >= 0x20 &&
			code < 0x7f &&
			!this.wrapPending &&
			this.activeCharSet === "G0" &&
			this.g0CharSet === "Ascii"
		) {
			const buffer = this.getActiveBuffer();
			const newCol = this.cursor.col + 1;
			if (newCol < this.cols) {
				// Simple case: ASCII char, not at end of line
				// Use createAsciiCell for optimized cell creation (skips charWidth)
				const cell = createAsciiCell(char, this.cursor.attrs);
				buffer.setCell(this.cursor.col, this.cursor.row, cell);
				this.cursor.col = newCol;
				return;
			} else if (this.modes.autoWrap) {
				// At end of line with autoWrap
				const cell = createAsciiCell(char, this.cursor.attrs);
				buffer.setCell(this.cursor.col, this.cursor.row, cell);
				this.cursor.col = this.cols - 1;
				this.wrapPending = true;
				return;
			}
		}

		// Slow path for complex cases
		this.handlePrintSlow(char);
	}

	/**
	 * Handle printable character - slow path for complex cases.
	 */
	private handlePrintSlow(char: string): void {
		const buffer = this.getActiveBuffer();
		const width = charWidth(char);

		// Apply character set translation if needed
		const translatedChar = this.translateCharacter(char);

		// Handle wrap pending (cursor was at end of line)
		if (this.wrapPending) {
			this.wrapPending = false;
			this.cursor.carriageReturn();
			if (this.cursor.lineFeed()) {
				buffer.scrollUp();
			}
		}

		// Check if we need to wrap before printing wide character
		if (width === 2 && this.cursor.col >= this.cols - 1) {
			if (this.modes.autoWrap) {
				this.cursor.carriageReturn();
				if (this.cursor.lineFeed()) {
					buffer.scrollUp();
				}
			}
		}

		// Create cell with current attributes
		const cell = createCell(translatedChar, this.cursor.attrs);
		buffer.setCell(this.cursor.col, this.cursor.row, cell);

		// For wide characters, set a placeholder in the next cell
		if (width === 2 && this.cursor.col < this.cols - 1) {
			const placeholder = createCell("", this.cursor.attrs);
			placeholder.width = 0;
			buffer.setCell(this.cursor.col + 1, this.cursor.row, placeholder);
		}

		// Advance cursor
		const newCol = this.cursor.col + width;
		if (newCol >= this.cols) {
			if (this.modes.autoWrap) {
				// Set wrap pending - next character will wrap
				this.cursor.col = this.cols - 1;
				this.wrapPending = true;
			}
		} else {
			this.cursor.col = newCol;
		}
	}

	/**
	 * Translate a character using the active character set.
	 */
	private translateCharacter(char: string): string {
		const charSet =
			this.activeCharSet === "G0" ? this.g0CharSet : this.g1CharSet;

		// Only translate for DEC Line Drawing character set
		if (charSet === "DecLineDrawing") {
			return this.translateLineDrawing(char);
		}

		return char;
	}

	/**
	 * Translate a character using DEC Line Drawing character set.
	 */
	private translateLineDrawing(char: string): string {
		// DEC Special Graphics / Line Drawing character set
		// Maps 0x5F-0x7E to box drawing characters
		const translations: Record<string, string> = {
			_: " ", // Blank
			"`": "\u25C6", // Diamond
			a: "\u2592", // Checkerboard
			b: "\u2409", // HT
			c: "\u240C", // FF
			d: "\u240D", // CR
			e: "\u240A", // LF
			f: "\u00B0", // Degree
			g: "\u00B1", // Plus/minus
			h: "\u2424", // NL
			i: "\u240B", // VT
			j: "\u2518", // Lower right corner
			k: "\u2510", // Upper right corner
			l: "\u250C", // Upper left corner
			m: "\u2514", // Lower left corner
			n: "\u253C", // Crossing lines
			o: "\u23BA", // Horizontal line - scan 1
			p: "\u23BB", // Horizontal line - scan 3
			q: "\u2500", // Horizontal line - scan 5
			r: "\u23BC", // Horizontal line - scan 7
			s: "\u23BD", // Horizontal line - scan 9
			t: "\u251C", // Left tee
			u: "\u2524", // Right tee
			v: "\u2534", // Bottom tee
			w: "\u252C", // Top tee
			x: "\u2502", // Vertical line
			y: "\u2264", // Less than or equal
			z: "\u2265", // Greater than or equal
			"{": "\u03C0", // Pi
			"|": "\u2260", // Not equal
			"}": "\u00A3", // UK pound
			"~": "\u00B7", // Bullet
		};

		return translations[char] ?? char;
	}

	/**
	 * Handle C0 control character.
	 */
	private handleExecute(code: number): void {
		const buffer = this.getActiveBuffer();

		switch (code) {
			case C0.BEL:
				// Bell - could emit event, for now do nothing
				break;

			case C0.BS:
				this.cursor.backspace();
				this.wrapPending = false;
				break;

			case C0.HT:
				this.handleTab();
				this.wrapPending = false;
				break;

			case C0.LF:
			case C0.VT:
			case C0.FF:
				// Line feed, vertical tab, form feed - all treated as newline
				if (this.cursor.lineFeed()) {
					buffer.scrollUp();
				}
				this.wrapPending = false;
				break;

			case C0.CR:
				this.cursor.carriageReturn();
				this.wrapPending = false;
				break;

			case C0.SO:
				// Shift Out - switch to G1 character set
				this.activeCharSet = "G1";
				break;

			case C0.SI:
				// Shift In - switch to G0 character set
				this.activeCharSet = "G0";
				break;

			default:
				// Ignore other control characters
				break;
		}
	}

	/**
	 * Handle horizontal tab.
	 */
	private handleTab(): void {
		// Find next tab stop
		const currentCol = this.cursor.col;
		const sortedStops = Array.from(this.tabStops).sort((a, b) => a - b);

		for (const stop of sortedStops) {
			if (stop > currentCol) {
				this.cursor.col = Math.min(stop, this.cols - 1);
				return;
			}
		}

		// No more tab stops, move to end of line
		this.cursor.col = this.cols - 1;
	}

	/**
	 * Set a tab stop at the current column.
	 */
	private setTabStop(): void {
		this.tabStops.add(this.cursor.col);
	}

	/**
	 * Clear tab stop at current column.
	 */
	private clearTabStop(): void {
		this.tabStops.delete(this.cursor.col);
	}

	/**
	 * Clear all tab stops.
	 */
	private clearAllTabStops(): void {
		this.tabStops.clear();
	}

	/**
	 * Handle CSI sequence.
	 */
	private handleCsi(action: CsiAction): void {
		const buffer = this.getActiveBuffer();

		switch (action.action) {
			// Cursor movement - relative
			case "CursorUp":
				this.cursor.moveUp(action.data || 1);
				this.wrapPending = false;
				break;

			case "CursorDown":
				this.cursor.moveDown(action.data || 1);
				this.wrapPending = false;
				break;

			case "CursorForward":
				this.cursor.moveRight(action.data || 1);
				this.wrapPending = false;
				break;

			case "CursorBack":
				this.cursor.moveLeft(action.data || 1);
				this.wrapPending = false;
				break;

			case "CursorNextLine":
				// Move down N lines and to column 1
				this.cursor.moveDown(action.data || 1);
				this.cursor.carriageReturn();
				this.wrapPending = false;
				break;

			case "CursorPreviousLine":
				// Move up N lines and to column 1
				this.cursor.moveUp(action.data || 1);
				this.cursor.carriageReturn();
				this.wrapPending = false;
				break;

			// Cursor movement - absolute
			case "CursorHorizontalAbsolute": {
				// CSI Ps G - Move cursor to column Ps (1-indexed)
				const col = Math.max(0, (action.data || 1) - 1);
				this.cursor.setColumn(col);
				this.wrapPending = false;
				break;
			}

			case "CursorVerticalAbsolute": {
				// CSI Ps d - Move cursor to row Ps (1-indexed)
				const row = Math.max(0, (action.data || 1) - 1);
				this.cursor.setRow(row);
				this.wrapPending = false;
				break;
			}

			case "CursorPosition": {
				// CSI row ; col H (1-indexed in ANSI, convert to 0-indexed)
				const row = Math.max(0, (action.data.row || 1) - 1);
				const col = Math.max(0, (action.data.col || 1) - 1);
				this.cursor.moveTo(col, row);
				this.wrapPending = false;
				break;
			}

			// Erase operations
			case "EraseInDisplay":
				switch (action.data) {
					case "Below":
						buffer.clearBelow(this.cursor.col, this.cursor.row);
						break;
					case "Above":
						buffer.clearAbove(this.cursor.col, this.cursor.row);
						break;
					case "All":
						buffer.clearAll();
						break;
					case "Scrollback":
						// Clear scrollback - not implemented yet
						buffer.clearAll();
						break;
				}
				break;

			case "EraseInLine":
				switch (action.data) {
					case "Below":
						buffer.clearLineFromCursor(this.cursor.row, this.cursor.col);
						break;
					case "Above":
						buffer.clearLineToCursor(this.cursor.row, this.cursor.col);
						break;
					case "All":
						buffer.clearLine(this.cursor.row);
						break;
				}
				break;

			case "EraseCharacters":
				buffer.eraseCharacters(
					this.cursor.row,
					this.cursor.col,
					action.data || 1,
				);
				break;

			// Insert/delete operations
			case "InsertLines":
				buffer.insertLines(this.cursor.row, action.data || 1);
				break;

			case "DeleteLines":
				buffer.deleteLines(this.cursor.row, action.data || 1);
				break;

			case "InsertCharacters":
				buffer.insertCharacters(
					this.cursor.row,
					this.cursor.col,
					action.data || 1,
				);
				break;

			case "DeleteCharacters":
				buffer.deleteCharacters(
					this.cursor.row,
					this.cursor.col,
					action.data || 1,
				);
				break;

			// Scroll operations
			case "ScrollUp":
				buffer.scrollUp(action.data || 1);
				break;

			case "ScrollDown":
				buffer.scrollDown(action.data || 1);
				break;

			case "SetScrollRegion": {
				// CSI top ; bottom r (1-indexed, convert to 0-indexed)
				const top = Math.max(0, (action.data.top || 1) - 1);
				// bottom 0 means use screen height
				const bottom =
					action.data.bottom === 0
						? this.rows - 1
						: Math.max(0, action.data.bottom - 1);
				buffer.setScrollRegion(top, bottom);
				// DECSTBM moves cursor to home position
				this.cursor.moveTo(0, 0);
				this.wrapPending = false;
				break;
			}

			// Style attributes
			case "Sgr": {
				// Parse SGR parameters and apply to cursor attributes
				const sgrAttrs = parseSgrParams(action.data);
				for (const sgrAttr of sgrAttrs) {
					applySgrAttr(this.cursor.attrs, sgrAttr);
				}
				break;
			}

			// Mode handling
			case "SetMode":
				this.handleSetMode(action.data, true);
				break;

			case "ResetMode":
				this.handleSetMode(action.data, false);
				break;

			// Device status
			case "DeviceStatusReport":
				this.handleDeviceStatusReport(action.data);
				break;

			// Device Attributes
			case "PrimaryDeviceAttributes":
				this.handlePrimaryDeviceAttributes();
				break;

			case "SecondaryDeviceAttributes":
				this.handleSecondaryDeviceAttributes();
				break;

			case "TertiaryDeviceAttributes":
				// Tertiary DA is rarely used, respond with empty DCS
				// For now, ignore
				break;

			case "Unknown":
				// Log unknown sequences for debugging
				// console.debug("Unknown CSI:", action.data);
				break;
		}
	}

	/**
	 * Handle Device Status Report (DSR).
	 *
	 * Buffers responses to handle multiple DSR requests in a single batch.
	 */
	private handleDeviceStatusReport(ps: number): void {
		let response: Uint8Array | null = null;

		switch (ps) {
			case 5:
				// Device Status Report - respond with OK
				// CSI 0 n
				response = new Uint8Array([0x1b, 0x5b, 0x30, 0x6e]); // ESC [ 0 n
				break;

			case 6: {
				// Cursor Position Report - respond with CSI row ; col R
				// Note: ANSI positions are 1-indexed
				const row = this.cursor.row + 1;
				const col = this.cursor.col + 1;
				const responseStr = `\x1b[${row};${col}R`;
				response = new TextEncoder().encode(responseStr);
				break;
			}

			default:
				// Unknown DSR, ignore
				break;
		}

		// Add to response buffer if we generated a response
		if (response) {
			this._pendingResponses.push(response);
		}
	}

	/**
	 * Handle Primary Device Attributes (DA1).
	 * Response: CSI ? 64 ; 1 ; 2 ; 6 ; 22 c
	 * Indicates VT420 with various capabilities.
	 */
	private handlePrimaryDeviceAttributes(): void {
		// Report as VT420 with:
		// 64 = VT420
		// 1 = 132 columns
		// 2 = Printer port
		// 6 = Selective erase
		// 22 = ANSI color
		const response = "\x1b[?64;1;2;6;22c";
		this._pendingResponses.push(new TextEncoder().encode(response));
	}

	/**
	 * Handle Secondary Device Attributes (DA2).
	 * Response: CSI > Pp ; Pv ; Pc c
	 * Pp = Terminal type (41 = VT420)
	 * Pv = Firmware version
	 * Pc = ROM cartridge registration number
	 */
	private handleSecondaryDeviceAttributes(): void {
		// Report as VT420 (41), version 1, no ROM cartridge
		const response = "\x1b[>41;1;0c";
		this._pendingResponses.push(new TextEncoder().encode(response));
	}

	/**
	 * Handle ESC sequence.
	 */
	private handleEsc(action: EscAction): void {
		const buffer = this.getActiveBuffer();

		switch (action.action) {
			case "SaveCursor":
				this.cursor.save();
				break;

			case "RestoreCursor":
				this.cursor.restore();
				this.wrapPending = false;
				break;

			case "Index":
				// Move cursor down, scroll if at bottom
				if (this.cursor.lineFeed()) {
					buffer.scrollUp();
				}
				break;

			case "NextLine":
				// Move to column 0 of next line, scroll if needed
				this.cursor.carriageReturn();
				if (this.cursor.lineFeed()) {
					buffer.scrollUp();
				}
				break;

			case "ReverseIndex":
				// Move cursor up, scroll if at top
				if (this.cursor.row === 0) {
					buffer.scrollDown();
				} else {
					this.cursor.moveUp();
				}
				break;

			case "HorizontalTabSet":
				this.setTabStop();
				break;

			case "ResetToInitialState":
				this.reset();
				break;

			case "SetG0CharSet":
				this.g0CharSet = action.data as CharSet;
				break;

			case "SetG1CharSet":
				this.g1CharSet = action.data as CharSet;
				break;

			case "Unknown":
				// Log unknown sequences for debugging
				// console.debug("Unknown ESC:", action.data);
				break;
		}
	}

	/**
	 * Handle OSC sequence.
	 */
	private handleOsc(action: OscAction): void {
		switch (action.action) {
			case "SetTitle":
				this._title = action.data;
				break;

			case "SetIconName":
				this._iconName = action.data;
				break;

			case "SetTitleAndIcon":
				this._title = action.data;
				this._iconName = action.data;
				break;

			case "SetColorPalette":
				// Color palette customization
				// Could update colors.ts palette, but for now just log
				// console.debug(`Set color ${action.index} to ${action.color}`);
				break;

			case "SetWorkingDirectory":
				this._workingDirectory = action.data;
				break;

			case "Hyperlink":
				// Handle hyperlink start/end
				if (action.uri) {
					// Start hyperlink
					this._activeHyperlink = { params: action.params, uri: action.uri };
				} else {
					// End hyperlink (empty URI)
					this._activeHyperlink = null;
				}
				break;

			case "SetForegroundColor":
				// Could update default foreground color
				// console.debug(`Set foreground color to ${action.data}`);
				break;

			case "SetBackgroundColor":
				// Could update default background color
				// console.debug(`Set background color to ${action.data}`);
				break;

			case "EmtermExtension":
				this.handleEmtermExtension(action.verb, action.params);
				break;

			case "Unknown":
				// Unknown OSC sequences are ignored
				break;
		}
	}

	/**
	 * Handle SetMode/ResetMode CSI sequences.
	 *
	 * @param modes - Array of mode numbers
	 * @param enable - true for SetMode, false for ResetMode
	 *
	 * Modes are processed in order, but special combinations are handled atomically:
	 * - Mode 1049 (save cursor + switch to alt) is atomic
	 * - Buffer switches complete before processing subsequent modes
	 */
	private handleSetMode(modes: number[], enable: boolean): void {
		// Collect actions to execute after mode state updates
		const actions: Array<() => void> = [];

		// Update mode state for all modes first
		for (const mode of modes) {
			const result = setDecPrivateMode(this.modes, mode, enable);

			if (result.action) {
				// Queue action for execution after all mode updates
				const action = result.action;
				actions.push(() => {
					switch (action) {
						case "saveAndSwitchToAlt":
							this.switchToAlternateBuffer(true);
							break;
						case "switchToAlt":
							this.switchToAlternateBuffer(false);
							break;
						case "switchToMain":
							this.switchToPrimaryBuffer(true);
							break;
						case "saveCursor":
							this.cursor.save();
							break;
						case "restoreCursor":
							this.cursor.restore();
							break;
					}
				});
			}
		}

		// Execute actions in order after all mode state is updated
		for (const action of actions) {
			action();
		}
	}

	/**
	 * Get indices of dirty rows.
	 */
	getDirtyRows(): number[] {
		return this.getActiveBuffer().getDirtyRows();
	}

	/**
	 * Clear all dirty flags.
	 */
	clearDirty(): void {
		this.getActiveBuffer().clearAllDirty();
	}

	/**
	 * Resize the terminal.
	 *
	 * @param cols - New number of columns
	 * @param rows - New number of rows
	 */
	resize(cols: number, rows: number): void {
		this.primaryBuffer.resize(cols, rows);
		if (this.alternateBuffer) {
			this.alternateBuffer.resize(cols, rows);
		}
		this.primaryCursor.resize(cols, rows);
		if (this.alternateCursor) {
			this.alternateCursor.resize(cols, rows);
		}
		this.tabStops = this.createDefaultTabStops(cols);
		this.wrapPending = false;
	}

	/**
	 * Handle eMterm extension commands.
	 *
	 * @param verb - The command verb (e.g., "emterm")
	 * @param params - Command parameters
	 */
	private handleEmtermExtension(verb: string, params: string[]): void {
		// Route to markdown manager
		const block = this.markdownManager.handleCommand(verb, params);

		if (block) {
			// Set block position based on current cursor
			block.startRow = this.cursor.row;
			this._pendingMarkdownBlocks.push(block);
		}
	}

	/**
	 * Get pending Markdown blocks for rendering.
	 *
	 * Call this after processing actions to get blocks that should be rendered.
	 * The returned blocks are removed from the pending queue.
	 *
	 * @returns Array of pending Markdown blocks
	 */
	takePendingMarkdownBlocks(): MarkdownBlock[] {
		const blocks = this._pendingMarkdownBlocks;
		this._pendingMarkdownBlocks = [];
		return blocks;
	}

	/**
	 * Get the markdown session manager.
	 *
	 * @returns The markdown session manager instance
	 */
	getMarkdownManager(): MarkdownSessionManager {
		return this.markdownManager;
	}

	/**
	 * Handle APC (Application Program Command) action.
	 * Used for Kitty Graphics Protocol.
	 */
	private handleApc(action: ApcAction): void {
		switch (action.action) {
			case "KittyGraphics":
				// Store image action for frontend processing
				// The ImageProcessor on the backend will handle actual decoding
				// Frontend receives this for display coordination
				// console.debug("Kitty Graphics command:", action.data.action);
				break;

			case "Unknown":
				// Unknown APC sequences are ignored
				break;
		}
	}

	/**
	 * Handle DCS (Device Control String) action.
	 * Used for SIXEL graphics.
	 */
	private handleDcs(action: DcsAction): void {
		switch (action.action) {
			case "Sixel":
				// Store SIXEL action for frontend processing
				// The backend decodes SIXEL to RGBA and sends via image event
				// console.debug("SIXEL data received");
				break;

			case "Unknown":
				// Unknown DCS sequences are ignored
				break;
		}
	}

	/**
	 * Reset terminal to initial state.
	 */
	reset(): void {
		const cols = this.cols;
		const rows = this.rows;

		// Reset buffers
		this.primaryBuffer = new ScreenBuffer(cols, rows);
		this.alternateBuffer = null;
		this.useAlternate = false;

		// Reset cursors
		this.primaryCursor = new CursorState(cols, rows);
		this.alternateCursor = null;
		this.cursor = this.primaryCursor;
		this.savedCursorForAlt = null;

		// Reset modes
		this.modes = createDefaultModes();

		// Reset other state
		this.wrapPending = false;
		this.tabStops = this.createDefaultTabStops(cols);
		this.g0CharSet = "Ascii";
		this.g1CharSet = "Ascii";
		this.activeCharSet = "G0";

		// Reset OSC state
		this._title = "";
		this._iconName = "";
		this._workingDirectory = "";
		this._pendingResponses = [];
		this._activeHyperlink = null;

		// Reset markdown state
		this.markdownManager.dispose();
		this.markdownManager = new MarkdownSessionManager();
		this._pendingMarkdownBlocks = [];
	}

	/**
	 * Extract plain text from a grid range for copy operations.
	 *
	 * Coordinates are automatically normalized (start comes before end).
	 * Trailing spaces on each line are removed.
	 * Lines are joined with '\n'.
	 *
	 * @param startCol - Start column (0-indexed)
	 * @param startRow - Start row (0-indexed)
	 * @param endCol - End column (0-indexed, inclusive)
	 * @param endRow - End row (0-indexed, inclusive)
	 * @returns Extracted text with newlines between rows
	 *
	 * @example
	 * ```ts
	 * // Extract "Hello" from cells (0,0) to (4,0)
	 * const text = state.extractText(0, 0, 4, 0);
	 * ```
	 */
	extractText(
		startCol: number,
		startRow: number,
		endCol: number,
		endRow: number,
	): string {
		// Normalize coordinates (ensure start comes before end)
		if (startRow > endRow || (startRow === endRow && startCol > endCol)) {
			[startCol, startRow, endCol, endRow] = [
				endCol,
				endRow,
				startCol,
				startRow,
			];
		}

		const buffer = this.getActiveBuffer();
		const lines: string[] = [];

		// Extract text row by row
		for (let row = startRow; row <= endRow; row++) {
			const line = buffer.getLine(row);
			const lineLength = line.length;

			let rowStartCol: number;
			let rowEndCol: number;

			if (row === startRow && row === endRow) {
				// Single line selection
				rowStartCol = startCol;
				rowEndCol = endCol;
			} else if (row === startRow) {
				// First line of multi-line selection
				rowStartCol = startCol;
				rowEndCol = lineLength - 1;
			} else if (row === endRow) {
				// Last line of multi-line selection
				rowStartCol = 0;
				rowEndCol = endCol;
			} else {
				// Middle line of multi-line selection
				rowStartCol = 0;
				rowEndCol = lineLength - 1;
			}

			// Extract characters from this row
			let rowText = "";
			for (let col = rowStartCol; col <= rowEndCol && col < lineLength; col++) {
				const cell = line.getCell(col);
				rowText += cell.char;
			}

			// Remove trailing spaces
			rowText = rowText.replace(/\s+$/, "");

			lines.push(rowText);
		}

		// Join lines with newline
		return lines.join("\n");
	}
}
