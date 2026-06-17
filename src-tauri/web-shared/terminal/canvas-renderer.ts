/**
 * Canvas 2D Renderer for terminal output.
 *
 * Renders terminal state to a Canvas element using the 2D API.
 * Optimized for high-performance scrolling with High DPI support.
 */

import type { LineAccessor } from "./grid.ts";
import {
	DEFAULT_BACKGROUND,
	DEFAULT_FOREGROUND,
	PALETTE_16,
	PALETTE_256,
	type Rgb,
	rgbToCSS,
} from "./colors.ts";
import type { UserColorScheme } from "../settings/types";
import type { CursorStyle } from "./cursor.ts";
import {
	checkFrameBudget,
	getPerformanceMonitor,
	RenderTimer,
} from "./performance.ts";
import type { ITerminalRenderer } from "./renderer-interface.ts";
import type { RendererSettings } from "../settings/settings-applier";
import type { TerminalState } from "./state.ts";
import type { SearchMatch } from "./search/search-state.ts";
import { computeAdjustedScrollOffset } from "./scroll-pin.ts";
import { getLogicalLine } from "./url-detector.ts";
import {
	type TextSpan,
	groupPackedCellsIntoSpans,
	getVisibleLines,
} from "./renderer-utils.ts";
import type { LineRenderContext } from "./renderer-line.ts";
import {
	renderLineBackground as renderLineBackgroundImpl,
	renderLineText as renderLineTextImpl,
	renderLineBackgroundFromSpans as renderLineBackgroundFromSpansImpl,
	renderLineTextFromSpans as renderLineTextFromSpansImpl,
} from "./renderer-line.ts";
import type { DecorationRenderContext, DetectionCacheEntry } from "./renderer-decorations.ts";
import {
	renderDetectionUnderlinesFromSpans as renderDetectionUnderlinesFromSpansImpl,
	renderDetectionUnderlinesLogical as renderDetectionUnderlinesLogicalImpl,
} from "./renderer-decorations.ts";
import type { CursorRenderContext } from "./renderer-cursor.ts";
import {
	renderCursor as renderCursorImpl,
	renderCursorArea as renderCursorAreaImpl,
	startCursorBlink as startCursorBlinkImpl,
	stopCursorBlink as stopCursorBlinkImpl,
} from "./renderer-cursor.ts";
import {
	renderSelection as renderSelectionImpl,
	clearSelectionOverlays as clearSelectionOverlaysImpl,
	clearSelectionHighlight as clearSelectionHighlightImpl,
	type SelectionOverlayState,
} from "./renderer-selection.ts";
import type { FoldRenderContext } from "./renderer-fold.ts";
import {
	getVisibleRowsPacked as getVisibleRowsPackedImpl,
	getVisibleLinesWithFolding as getVisibleLinesWithFoldingImpl,
	renderFoldSummaryLines as renderFoldSummaryLinesImpl,
} from "./renderer-fold.ts";
import type { ColorState, SettingsCallbacks } from "./renderer-settings.ts";
import {
	setFontSize as setFontSizeImpl,
	getFontSizePt,
	setFontFamily as setFontFamilyImpl,
	setCursorStyle as setCursorStyleImpl,
	setCursorBlink as setCursorBlinkImpl,
	setColorScheme as setColorSchemeImpl,
	setUserColorScheme as setUserColorSchemeImpl,
	setBoldBrightensAnsiColors as setBoldBrightensAnsiColorsImpl,
} from "./renderer-settings.ts";

// Re-export utilities for backward compatibility
export {
	type TextSpan,
	type TextAttributeStyles,
	type SelectionRange,
	groupCellsIntoSpans,
	packedAttrsEqual,
	unpackAttrsFromBinary,
	groupPackedCellsIntoSpans,
	getVisibleLines,
	calculateScrollPosition,
	buildFontString,
	applyTextAttributes,
	normalizeSelection,
} from "./renderer-utils.ts";

/**
 * Canvas 2D terminal renderer.
 */
export class CanvasRenderer implements ITerminalRenderer {
	/** Container element. */
	private container: HTMLElement;

	/** Canvas element. */
	private canvas: HTMLCanvasElement;

	/** 2D rendering context. */
	private ctx: CanvasRenderingContext2D;

	/** Font family. */
	private fontFamily: string;

	/** Font size in pixels. */
	private fontSize: number;

	/** Number of columns. */
	private cols: number = 80;

	/** Number of rows. */
	private rows: number = 24;

	/** Character width in pixels. */
	private charWidth: number = 0;

	/** Character height in pixels. */
	private charHeight: number = 0;

	/** Font ascent in pixels (for baseline positioning). */
	private fontAscent: number = 0;

	/** Font descent in pixels (for baseline positioning). */
	private fontDescent: number = 0;

	/** Current device pixel ratio. */
	private dpr: number = 1;

	/** Pending render flag. */
	private renderPending: boolean = false;

	/** Current state to render. */
	private pendingState: TerminalState | null = null;

	/** Performance timer. */
	private renderTimer: RenderTimer = new RenderTimer();

	/** Media query for DPR changes. */
	private dprMediaQuery: MediaQueryList | null = null;

	/** DPR change handler. */
	private dprChangeHandler: (() => void) | null = null;

	/** Selection overlay state. */
	private selectionState: SelectionOverlayState = {
		selectionContainer: null,
		selectionOverlays: [],
	};

	/** Cursor blink timer ID. */
	private cursorBlinkTimer: ReturnType<typeof setInterval> | null = null;

	/** Cursor visible state for blinking. */
	private cursorBlinkVisible: boolean = true;

	/** Blink text timer ID. */
	private blinkTextTimer: ReturnType<typeof setInterval> | null = null;

	/** Blink text visible state. */
	private blinkTextVisible: boolean = true;

	/** Previous cursor position for clearing. */
	private prevCursorCol: number = -1;

	/** Previous cursor row for clearing. */
	private prevCursorRow: number = -1;

	/** Previous cursor visible state for detecting visibility changes. */
	private prevCursorVisible: boolean = true;

	/** Current foreground color (can be changed by color scheme). */
	private currentForeground: Rgb = DEFAULT_FOREGROUND;

	/** Current background color (can be changed by color scheme). */
	private currentBackground: Rgb = DEFAULT_BACKGROUND;

	/** Current cursor color (can be changed by color scheme). */
	private currentCursorColor: Rgb = { r: 0, g: 128, b: 0 };

	/** Current 16-color palette (can be changed by color scheme). */
	private currentPalette16: readonly Rgb[] = PALETTE_16;

	/** Current full 256-color palette (first 16 entries from currentPalette16, rest from static). */
	private currentPalette256: readonly Rgb[] = PALETTE_256;

	/** Whether bold attribute brightens standard ANSI colors (0-7 -> 8-15). */
	private boldBrightensAnsiColors: boolean = true;

	/** Diagnostic: bypass differential rendering, always forceRender.
	 *  Set via EMTERM_FORCE_FULL_RENDER=1 environment variable. */
	private forceFullRender: boolean = false;

	/** Glyph width cache: outer key = ctx.font string, inner key = character. */
	private glyphWidthCache: Map<string, Map<string, number>> = new Map();

	/**
	 * Diagnostic-only: counters from the most recent forceRender call.
	 * Surfaced via structural cast in mux-window-manager.ts so that we can
	 * tell whether forceRender did real drawing work (>0 fills + text rows)
	 * or short-circuited internally (the canvasHash-unchanged freeze
	 * symptom). Reset at the top of each forceRender.
	 */
	private _lastForceRenderDiag: {
		visibleLines: number;
		emptyRows: number;
		bgFillCount: number;
		textPassRows: number;
		cursorRendered: boolean;
		probeR: number;
		probeG: number;
		probeB: number;
		probeSxDev: number;
		probeSyDev: number;
		probeWDev: number;
		probeHDev: number;
		ctxFillStyle: string;
		ctxFont: string;
		ctxAlpha: number;
		ctxComposite: string;
		ctxTxA: number;
		ctxTxD: number;
		ctxTxE: number;
		ctxTxF: number;
	} | null = null;

	/** Monotonic counter feeding the diagnostic probe RGB. Using performance.now()
	 *  caused false-positive `match=true` because two forceRender calls within
	 *  the same millisecond produced identical probe colors — when the
	 *  freeze-symptom case (fillRect did not commit) leaves the previous probe
	 *  pixel on the surface, the readback equals the new expected color and
	 *  the freeze hides itself. A per-call counter guarantees uniqueness. */
	private _probeSeq: number = 0;

	/** Context-reset bookkeeping for the mux race detector. Mux switch (or any
	 *  caller that swaps the active TerminalCore beneath the renderer) calls
	 *  notifyContextReset(label, expectedRef). Subsequent render()/forceRender()
	 *  within `_raceWindowMs` compare state.getActiveCore().__wbg_ptr to
	 *  expectedRef; mismatch warns once per reset event so we can tell whether a
	 *  draw landed on the wrong canvas/core. */
	private _lastCtxReset: { at: number; label: string; expectedRef: number; activePaneId: number } | null = null;
	private _raceWarnedForReset: number = -1;
	private _raceWindowMs: number = 500;

	/** rAF interval tracking for heartbeat. _maxRafGapMs is the largest gap
	 *  observed since the last heartbeat reset; _lastRafAt is the previous
	 *  rAF callback timestamp. Updated on every render() entry. The heartbeat
	 *  consumer reads and resets via getAndResetMaxRafGap(). */
	private _maxRafGapMs: number = 0;
	private _lastRafAt: number = 0;

	/** Current scroll offset (number of lines scrolled back from bottom). */
	private scrollOffset: number = 0;

	/** Last observed `state.getScrollbackLength()`, used to pin the viewport
	 *  when scrollback grows beneath a user that has scrolled up. See
	 *  doc/tasks/pin-viewport-when-scrolled-up/SPEC.md (FR1〜FR6). */
	private prevScrollbackLength: number = 0;

	/** Visible lines resolved for the current render pass (scroll-aware). */
	private renderVisibleLines: (LineAccessor | null)[] | null = null;

	/** Hover position for link underline (display row). -1 = no hover. */
	private hoverRow: number = -1;

	/** Hover position for link underline (display col). -1 = no hover. */
	private hoverCol: number = -1;

	/** Previous hover position for differential redraw. */
	private prevHoverRow: number = -1;
	private prevHoverCol: number = -1;

	/** Search matches to highlight (set externally). */
	private searchMatches: SearchMatch[] = [];

	/** Current search match index (-1 if none). */
	private searchCurrentIndex: number = -1;

	/** Per-frame cache for logical line detection (keyed by startRow). */
	private detectionCache: Map<number, DetectionCacheEntry> = new Map();

	/**
	 * Optional WASM crash recovery callback. When a render path (rAF render,
	 * synchronous renderImmediate, or cursor blink) catches an error, it is
	 * routed here so the PTY handler can trigger shared recovery instead of
	 * leaving the terminal silently broken.
	 *
	 * Returns `true` if the error was a WASM crash and recovery was attempted.
	 */
	private wasmRecoveryCallback: ((error: unknown) => boolean) | null = null;

	/**
	 * Create a new canvas renderer.
	 */
	constructor(container: HTMLElement, fontFamily: string, fontSize: number) {
		this.container = container;
		this.fontFamily = fontFamily;
		this.fontSize = fontSize;

		// Create canvas element
		this.canvas = document.createElement("canvas");
		this.canvas.style.display = "block";
		this.container.appendChild(this.canvas);

		const ctx = this.canvas.getContext("2d");
		if (!ctx) {
			throw new Error("Failed to get 2D rendering context");
		}
		this.ctx = ctx;

		// Initialize canvas with DPR support
		this.setupCanvas();

		// Measure character dimensions
		this.measureCharacterSize();

		// Watch for DPR changes
		this.watchDPRChanges();

		// Start cursor blink timer
		this.startCursorBlink();
	}

	// ── Context builders ──────────────────────────────────────

	/** Build LineRenderContext from current state. */
	private getLineRenderContext(): LineRenderContext {
		return {
			ctx: this.ctx,
			charWidth: this.charWidth,
			charHeight: this.charHeight,
			fontAscent: this.fontAscent,
			fontDescent: this.fontDescent,
			fontSize: this.fontSize,
			fontFamily: this.fontFamily,
			dpr: this.dpr,
			canvas: this.canvas,
			currentForeground: this.currentForeground,
			currentBackground: this.currentBackground,
			currentPalette256: this.currentPalette256,
			boldBrightensAnsiColors: this.boldBrightensAnsiColors,
			glyphWidthCache: this.glyphWidthCache,
			cols: this.cols,
			renderDetectionUnderlines: (rowIndex: number) => {
				this.renderDetectionUnderlines(rowIndex);
			},
			renderDetectionUnderlinesFromSpans: (rowIndex: number, spans: TextSpan[]) => {
				this.renderDetectionUnderlinesFromSpans(rowIndex, spans);
			},
		};
	}

	/** Build DecorationRenderContext from current state. */
	private getDecorationRenderContext(): DecorationRenderContext {
		return {
			ctx: this.ctx,
			charWidth: this.charWidth,
			charHeight: this.charHeight,
			cols: this.cols,
			rows: this.rows,
			currentForeground: this.currentForeground,
			currentBackground: this.currentBackground,
			currentPalette256: this.currentPalette256,
			boldBrightensAnsiColors: this.boldBrightensAnsiColors,
			hoverRow: this.hoverRow,
			hoverCol: this.hoverCol,
			renderVisibleLines: this.renderVisibleLines,
			detectionCache: this.detectionCache,
			getBufferLine: (r: number) => {
				if (r < 0 || r >= this.rows) return null;
				try {
					return this.pendingState!.getActiveBuffer().getLine(r);
				} catch (e) {
					console.warn("[WARN][FRONTEND] Unexpected getLine error at row", r, e);
					return null;
				}
			},
		};
	}

	/** Build CursorRenderContext from current state. */
	private getCursorRenderContext(): CursorRenderContext {
		return {
			...this.getLineRenderContext(),
			currentCursorColor: this.currentCursorColor,
			scrollOffset: this.scrollOffset,
			cursorBlinkVisible: this.cursorBlinkVisible,
		};
	}

	/** Build FoldRenderContext from current state. */
	private getFoldRenderContext(): FoldRenderContext {
		return {
			ctx: this.ctx,
			charWidth: this.charWidth,
			charHeight: this.charHeight,
			fontSize: this.fontSize,
			fontFamily: this.fontFamily,
			cols: this.cols,
			scrollOffset: this.scrollOffset,
		};
	}

	/** Build ColorState from current state. */
	private getColorState(): ColorState {
		return {
			currentForeground: this.currentForeground,
			currentBackground: this.currentBackground,
			currentCursorColor: this.currentCursorColor,
			currentPalette16: this.currentPalette16,
			currentPalette256: this.currentPalette256,
			boldBrightensAnsiColors: this.boldBrightensAnsiColors,
		};
	}

	/** Apply ColorState back to this instance. */
	private applyColorState(state: ColorState): void {
		this.currentForeground = state.currentForeground;
		this.currentBackground = state.currentBackground;
		this.currentCursorColor = state.currentCursorColor;
		this.currentPalette16 = state.currentPalette16;
		this.currentPalette256 = state.currentPalette256;
		this.boldBrightensAnsiColors = state.boldBrightensAnsiColors;
	}

	/** Build SettingsCallbacks for settings functions. */
	private getSettingsCallbacks(): SettingsCallbacks {
		return {
			measureCharacterSize: () => this.measureCharacterSize(),
			forceRender: (state: TerminalState) => this.forceRender(state),
			startCursorBlink: () => this.startCursorBlink(),
			stopCursorBlink: () => this.stopCursorBlink(),
			getPendingState: () => this.pendingState,
		};
	}

	// ── Canvas setup ──────────────────────────────────────────

	/**
	 * Set up canvas with High DPI support.
	 */
	private setupCanvas(): void {
		this.dpr = window.devicePixelRatio || 1;

		const rect = this.container.getBoundingClientRect();
		const width = rect.width || 800;
		const height = rect.height || 600;

		const pxWidth = Math.floor(width * this.dpr);
		const pxHeight = Math.floor(height * this.dpr);

		this.canvas.width = pxWidth;
		this.canvas.height = pxHeight;
		this.canvas.style.width = `${width}px`;
		this.canvas.style.height = `${height}px`;

		// Apply DPR scaling (setTransform resets any prior transform)
		this.ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);

		this.ctx.textBaseline = "alphabetic";
		this.ctx.font = `${this.fontSize}px ${this.fontFamily}`;
	}

	/**
	 * Measure character dimensions using the canvas context.
	 */
	private measureCharacterSize(): void {
		this.ctx.font = `${this.fontSize}px ${this.fontFamily}`;

		const metrics = this.ctx.measureText("M");
		this.charWidth = metrics.width;

		const ascent = metrics.fontBoundingBoxAscent ?? this.fontSize * 0.8;
		const descent = metrics.fontBoundingBoxDescent ?? this.fontSize * 0.2;
		this.fontAscent = ascent;
		this.fontDescent = descent;

		this.charHeight = Math.ceil(ascent + descent);

		this.glyphWidthCache.clear();
	}

	/**
	 * Watch for devicePixelRatio changes.
	 */
	private watchDPRChanges(): void {
		const updateDPR = () => {
			const newDpr = window.devicePixelRatio || 1;
			if (newDpr !== this.dpr) {
				this.setupCanvas();
				this.measureCharacterSize();
				if (this.pendingState) {
					this.forceRender(this.pendingState);
				}
			}
			this.registerDPRListener();
		};

		this.dprChangeHandler = updateDPR;
		this.registerDPRListener();
	}

	/**
	 * Register DPR change listener.
	 */
	private registerDPRListener(): void {
		if (this.dprMediaQuery && this.dprChangeHandler) {
			this.dprMediaQuery.removeEventListener("change", this.dprChangeHandler);
		}

		this.dprMediaQuery = window.matchMedia(`(resolution: ${this.dpr}dppx)`);
		if (this.dprChangeHandler) {
			this.dprMediaQuery.addEventListener("change", this.dprChangeHandler);
		}
	}

	// ── Render scheduling ─────────────────────────────────────

	/**
	 * Register a WASM crash recovery callback. Invoked from render and cursor
	 * blink error paths so the PTY handler's shared recovery can run instead
	 * of the error being silently swallowed.
	 */
	setWasmRecoveryCallback(cb: ((error: unknown) => boolean) | null): void {
		this.wasmRecoveryCallback = cb;
	}

	/** Mux switch (or any active-core swap) calls this with the new core's
	 *  __wbg_ptr so the race detector can flag any subsequent render that
	 *  lands on a different core within the race window. label is "muxSwitch"
	 *  / "muxCreate" / "muxRestore" / etc. for log triage. */
	notifyContextReset(label: string, expectedRef: number, activePaneId: number): void {
		this._lastCtxReset = {
			at: performance.now(),
			label,
			expectedRef,
			activePaneId,
		};
		this._raceWarnedForReset = -1;
	}

	/** Returns the largest rAF interval observed since the previous call and
	 *  resets both the max counter and the per-frame anchor. Heartbeat reads
	 *  this every 5 s — a value much greater than 16 ms is a sign the WebKit
	 *  compositor / main thread is stalling. Resetting `_lastRafAt = 0`
	 *  prevents the next render after a long idle period from producing a
	 *  spurious huge gap (the renderer is event-driven; long gaps during
	 *  user idle are not stalls). */
	getAndResetMaxRafGap(): number {
		const v = this._maxRafGapMs;
		this._maxRafGapMs = 0;
		this._lastRafAt = 0;
		return v;
	}

	/** Compare the active core to the one notified at last context reset.
	 *  Warns at most once per reset event (debounced via _raceWarnedForReset)
	 *  so a freeze that pegs render() at 60 fps does not flood the log. kind
	 *  identifies the call site for triage. */
	private checkRaceWithLastReset(state: TerminalState, kind: string): void {
		const reset = this._lastCtxReset;
		if (!reset) return;
		const now = performance.now();
		const sinceMs = now - reset.at;
		if (sinceMs > this._raceWindowMs) return;
		if (this._raceWarnedForReset === reset.at) return;
		try {
			const core = state.getActiveCore() as unknown as { __wbg_ptr?: number };
			const ref = core.__wbg_ptr ?? 0;
			if (ref !== reset.expectedRef) {
				console.warn(
					`[DIAG-MUX-RACE] ${kind} on stale core` +
					` | sinceReset=${sinceMs.toFixed(0)}ms` +
					` | label=${reset.label}` +
					` | expectedRef=${reset.expectedRef} gotRef=${ref}` +
					` | resetPaneId=${reset.activePaneId}`,
				);
				this._raceWarnedForReset = reset.at;
			}
		} catch (err) {
			console.warn(`[DIAG-MUX-RACE] checkRace threw: ${err instanceof Error ? err.message : String(err)}`);
			this._raceWarnedForReset = reset.at;
		}
	}

	/**
	 * Schedule a render of the terminal state.
	 */
	scheduleRender(state: TerminalState): void {
		this.pendingState = state;

		if (!this.renderPending) {
			this.renderPending = true;
			requestAnimationFrame(() => {
				try {
					this.render();
				} catch (error) {
					console.error("[ERROR][FRONTEND] Render failed:", error);
					this.detectionCache.clear();
					this.wasmRecoveryCallback?.(error);
				} finally {
					this.renderPending = false;
				}
			});
		}
	}

	/**
	 * Pin the viewport on PTY-driven scrollback growth.
	 *
	 * Observes the delta between the previously seen scrollback length and
	 * the current one; if scrollOffset > 0 and scrollback grew, scrollOffset
	 * is increased by the delta (clamped to the current scrollback length)
	 * so that the topmost visible absolute row stays put. See
	 * doc/tasks/pin-viewport-when-scrolled-up/SPEC.md.
	 *
	 * Idempotent within the same frame: a second call observes Δ === 0
	 * because the first call wrote `prevScrollbackLength` back to the
	 * current value.
	 */
	private adjustScrollOffsetForGrowth(state: TerminalState): void {
		const sbLen = state.getScrollbackLength();
		const { nextScrollOffset, nextPrevSbLen } = computeAdjustedScrollOffset(
			this.prevScrollbackLength,
			sbLen,
			this.scrollOffset,
		);
		this.scrollOffset = nextScrollOffset;
		this.prevScrollbackLength = nextPrevSbLen;
	}

	/**
	 * Render immediately (synchronously) using dirty-row differential path.
	 */
	renderImmediate(state: TerminalState): void {
		this.pendingState = state;
		try {
			this.render();
		} catch (error) {
			console.error("[ERROR][FRONTEND] Render failed:", error);
			this.detectionCache.clear();
			this.wasmRecoveryCallback?.(error);
		}
		this.renderPending = false;
	}

	// ── Main render ───────────────────────────────────────────

	/**
	 * Perform the actual render.
	 */
	private render(): void {
		if (!this.pendingState || !this.pendingState.isReady()) {
			return;
		}

		// Pin viewport to current absolute rows when scrollback grew beneath
		// a scrolled-up user. Must run before any scrollOffset reads below.
		this.adjustScrollOffsetForGrowth(this.pendingState);

		// Track rAF interval for the heartbeat: a gap >> 16 ms during an
		// active rendering sequence means the main thread or compositor was
		// blocked. Heartbeat reads max gap every 5 s and resets _lastRafAt
		// so the next idle→active transition does not record a huge gap as
		// a spurious stall. Cap at 2 s defensively in case some other path
		// resets state between renders without going through the heartbeat.
		const renderEntryAt = performance.now();
		if (this._lastRafAt > 0) {
			const gap = Math.min(renderEntryAt - this._lastRafAt, 2000);
			if (gap > this._maxRafGapMs) this._maxRafGapMs = gap;
		}
		this._lastRafAt = renderEntryAt;

		this.checkRaceWithLastReset(this.pendingState, "diff-render");

		this.renderTimer.start();

		const state = this.pendingState;

		if (this.forceFullRender) {
			this.forceRender(state);
			const duration = this.renderTimer.end();
			const monitor = getPerformanceMonitor();
			if (monitor.isEnabled()) {
				monitor.recordRender(duration);
			}
			return;
		}

		if (this.scrollOffset > 0) {
			this.forceRender(state);
			const duration = this.renderTimer.end();
			const monitor = getPerformanceMonitor();
			if (monitor.isEnabled()) {
				monitor.recordRender(duration);
			}
			return;
		}

		const scrollDir = state.getScrollEventDirection();
		if (scrollDir === 1) {
			const scrollCount = state.getScrollEventCount();
			state.clearScrollEvent();
			const shiftPx = scrollCount * this.charHeight;
			const canvasW = this.canvas.width / this.dpr;
			const canvasH = this.canvas.height / this.dpr;
			if (shiftPx > 0 && shiftPx < canvasH) {
				const srcOffsetPx = Math.round(shiftPx * this.dpr);
				this.ctx.drawImage(
					this.canvas,
					0, srcOffsetPx,
					this.canvas.width, this.canvas.height - srcOffsetPx,
					0, 0,
					canvasW, canvasH - shiftPx,
				);
				this.ctx.fillStyle = rgbToCSS(this.currentBackground);
				this.ctx.fillRect(0, canvasH - shiftPx, canvasW, shiftPx);
				// Adjust previous cursor row to match shifted pixel position
				this.prevCursorRow -= scrollCount;
			}
		}

		const buffer = state.getActiveBuffer();
		const dirtyRows = state.getDirtyRows();
		const bufferRows = buffer.rows;

		if (dirtyRows.length > bufferRows * 0.4) {
			this.forceRender(state);
			const duration = this.renderTimer.end();
			const monitor = getPerformanceMonitor();
			if (monitor.isEnabled()) {
				monitor.recordRender(duration);
			}
			return;
		}

		const rctx = this.getLineRenderContext();

		// Pre-parse packed data for dirty rows
		const parsedRows: { rowIndex: number; spans: TextSpan[] | null; line: LineAccessor | null }[] = [];
		for (const rowIndex of dirtyRows) {
			if (rowIndex < 0 || rowIndex >= bufferRows) {
				console.warn(`[WARN][RENDERER] Dirty row ${rowIndex} out of bounds (buffer rows=${bufferRows}), skipping`);
				continue;
			}
			const packed = state.getRowPacked(rowIndex);
			if (packed && packed.length > 0) {
				const spans = groupPackedCellsIntoSpans(packed, this.cols);
				if (spans.length === 0) {
					console.warn(`[WARN][RENDERER] Empty spans from non-empty packed data for row ${rowIndex}, packed.length=${packed.length}`);
				}
				parsedRows.push({ rowIndex, spans, line: null });
			} else {
				if (packed && packed.length === 0) {
					console.warn(`[WARN][RENDERER] Empty packed data for dirty row ${rowIndex}, falling back to LineAccessor`);
				}
				parsedRows.push({ rowIndex, spans: null, line: buffer.getLine(rowIndex) });
			}
		}

		// Two-pass rendering to prevent descender clipping
		// Pass 1: backgrounds
		for (const { rowIndex, spans, line } of parsedRows) {
			if (spans) {
				renderLineBackgroundFromSpansImpl(rctx, rowIndex, spans);
			} else if (line) {
				renderLineBackgroundImpl(rctx, rowIndex, line);
			}
		}
		// Pass 2: text
		for (const { rowIndex, spans, line } of parsedRows) {
			if (spans) {
				renderLineTextFromSpansImpl(rctx, rowIndex, spans);
			} else if (line) {
				renderLineTextImpl(rctx, rowIndex, line);
			}
		}

		// Clear dirty flags
		state.clearDirty();

		// Clear previous cursor position if it moved or became invisible
		const cursorMoved =
			this.prevCursorCol !== state.cursorCol ||
			this.prevCursorRow !== state.cursorRow;
		const cursorBecameInvisible =
			this.prevCursorVisible && !state.cursorVisible;
		const prevRowNeedsRedraw =
			this.prevCursorRow >= 0 &&
			(cursorMoved || cursorBecameInvisible) &&
			!dirtyRows.includes(this.prevCursorRow);

		if (prevRowNeedsRedraw) {
			const prevPacked = state.getRowPacked(this.prevCursorRow);
			if (prevPacked && prevPacked.length > 0) {
				const prevSpans = groupPackedCellsIntoSpans(prevPacked, this.cols);
				renderLineBackgroundFromSpansImpl(rctx, this.prevCursorRow, prevSpans);
				renderLineTextFromSpansImpl(rctx, this.prevCursorRow, prevSpans);
			} else {
				const prevLine = buffer.getLine(this.prevCursorRow);
				renderLineBackgroundImpl(rctx, this.prevCursorRow, prevLine);
				renderLineTextImpl(rctx, this.prevCursorRow, prevLine);
			}
		}

		// Update cursor
		this.renderCursor(
			state.cursorCol,
			state.cursorRow,
			state.cursorVisible,
			state.cursorStyle,
			state.cursorBlink,
			state,
		);

		// Save current cursor position for next render
		this.prevCursorCol = state.cursorCol;
		this.prevCursorRow = state.cursorRow;
		this.prevCursorVisible = state.cursorVisible;

		// Hover underline pass
		const hoverChanged = this.hoverRow !== this.prevHoverRow || this.hoverCol !== this.prevHoverCol;
		if (hoverChanged) {
			const hoverRedrawRows = new Set<number>();
			const getLineForHover = (r: number): LineAccessor | null =>
				(r >= 0 && r < this.rows) ? buffer.getLine(r) : null;

			for (const seedRow of [this.prevHoverRow, this.hoverRow]) {
				if (seedRow < 0 || seedRow >= this.rows) continue;
				const logical = getLogicalLine(getLineForHover, seedRow, this.rows);
				for (let r = logical.startRow; r < logical.startRow + logical.rowCount; r++) {
					hoverRedrawRows.add(r);
				}
			}

			for (const row of hoverRedrawRows) {
				if (dirtyRows.includes(row)) continue;
				const packed = state.getRowPacked(row);
				if (packed && packed.length > 0) {
					const spans = groupPackedCellsIntoSpans(packed, this.cols);
					renderLineBackgroundFromSpansImpl(rctx, row, spans);
					renderLineTextFromSpansImpl(rctx, row, spans);
				} else {
					const line = buffer.getLine(row);
					renderLineBackgroundImpl(rctx, row, line);
					renderLineTextImpl(rctx, row, line);
				}
			}
			this.prevHoverRow = this.hoverRow;
			this.prevHoverCol = this.hoverCol;
		}

		// Clear per-frame detection cache
		this.detectionCache.clear();

		// Record performance metrics
		const duration = this.renderTimer.end();
		const monitor = getPerformanceMonitor();
		if (monitor.isEnabled()) {
			monitor.recordRender(duration);
			if (duration > 16) {
				checkFrameBudget(duration, `canvas render ${dirtyRows.length} rows`);
			}
		}
	}

	// ── Decoration rendering (delegated) ──────────────────────

	private renderDetectionUnderlinesFromSpans(rowIndex: number, spans: TextSpan[]): void {
		renderDetectionUnderlinesFromSpansImpl(this.getDecorationRenderContext(), rowIndex, spans);
	}

	private renderDetectionUnderlines(rowIndex: number): void {
		renderDetectionUnderlinesLogicalImpl(this.getDecorationRenderContext(), rowIndex);
	}

	// ── Cursor rendering (delegated) ──────────────────────────

	private renderCursor(
		col: number,
		row: number,
		visible: boolean,
		style: CursorStyle,
		blink: boolean = true,
		state?: TerminalState,
	): void {
		renderCursorImpl(this.getCursorRenderContext(), col, row, visible, style, blink, state);
	}

	private renderCursorArea(state: TerminalState): void {
		renderCursorAreaImpl(this.getCursorRenderContext(), state);
	}

	startCursorBlink(): void {
		const blinkState = { cursorBlinkTimer: this.cursorBlinkTimer, cursorBlinkVisible: this.cursorBlinkVisible };
		startCursorBlinkImpl(blinkState, () => {
			this.cursorBlinkVisible = blinkState.cursorBlinkVisible;
			if (this.pendingState) {
				try {
					this.renderCursorArea(this.pendingState);
				} catch (error) {
					// WASM may be in recovery — silently skip this blink frame
					if (error instanceof WebAssembly.RuntimeError) {
						// DIAG-IDLE: capture visibility context so we can tell
						// whether WebKitGTK flips document.hidden during PC lock.
						const vs = typeof document !== "undefined" ? document.visibilityState : "n/a";
						const hidden = typeof document !== "undefined" ? document.hidden : "n/a";
						console.warn(
							`[WARN][FRONTEND] cursor blink skipped — WASM unavailable | visibilityState=${vs} hidden=${hidden}`,
						);
					}
					// Route into shared recovery so the terminal can self-heal even
					// when no PTY data is arriving (idle terminal after suspend).
					this.wasmRecoveryCallback?.(error);
				}
			}
		});
		this.cursorBlinkTimer = blinkState.cursorBlinkTimer;
		this.cursorBlinkVisible = blinkState.cursorBlinkVisible;
	}

	stopCursorBlink(): void {
		const blinkState = { cursorBlinkTimer: this.cursorBlinkTimer, cursorBlinkVisible: this.cursorBlinkVisible };
		stopCursorBlinkImpl(blinkState);
		this.cursorBlinkTimer = blinkState.cursorBlinkTimer;
		this.cursorBlinkVisible = blinkState.cursorBlinkVisible;
	}

	// ── Selection rendering (delegated) ───────────────────────

	renderSelection(selection: {
		start: { col: number; row: number };
		end: { col: number; row: number };
	}): void {
		renderSelectionImpl(
			{ container: this.container, charWidth: this.charWidth, charHeight: this.charHeight, cols: this.cols },
			this.selectionState,
			selection,
		);
	}

	private clearSelectionOverlays(): void {
		clearSelectionOverlaysImpl(this.selectionState);
	}

	clearSelectionHighlight(): void {
		clearSelectionHighlightImpl(this.selectionState);
	}

	// ── Fold rendering (delegated) ────────────────────────────

	private getVisibleRowsPacked(
		state: TerminalState,
		scrollOffset: number,
		count: number,
	): (Uint8Array | null)[] {
		return getVisibleRowsPackedImpl(state, scrollOffset, count);
	}

	private getVisibleLinesWithFolding(
		state: TerminalState,
		foldManager: ReturnType<TerminalState["getFoldManager"]>,
	): (LineAccessor | null)[] {
		return getVisibleLinesWithFoldingImpl(state, foldManager, this.scrollOffset);
	}

	private renderFoldSummaryLines(
		state: TerminalState,
		visibleLines: (LineAccessor | null)[],
		foldManager: ReturnType<TerminalState["getFoldManager"]>,
	): void {
		renderFoldSummaryLinesImpl(this.getFoldRenderContext(), state, visibleLines, foldManager);
	}

	// ── Force render ──────────────────────────────────────────

	/**
	 * Force a full re-render.
	 */
	forceRender(state: TerminalState): void {
		this.pendingState = state;
		this.checkRaceWithLastReset(state, "forceRender");
		// Pin viewport when scrollback grew beneath a scrolled-up user.
		// Same-frame double calls (render()→forceRender()) are no-ops because
		// the first call rebases prevScrollbackLength to currSbLen.
		this.adjustScrollOffsetForGrowth(state);
		// Diagnostic counters reset at the top so every forceRender call
		// produces a fresh snapshot. Fields are mutated below in the bg/text
		// passes and read by mux-window-manager.ts on freeze investigation.
		const diag = {
			visibleLines: 0,
			emptyRows: 0,
			bgFillCount: 0,
			textPassRows: 0,
			cursorRendered: false,
			probeR: 0,
			probeG: 0,
			probeB: 0,
			probeSxDev: 0,
			probeSyDev: 0,
			probeWDev: 0,
			probeHDev: 0,
			ctxFillStyle: "",
			ctxFont: "",
			ctxAlpha: 1,
			ctxComposite: "",
			ctxTxA: 1,
			ctxTxD: 1,
			ctxTxE: 0,
			ctxTxF: 0,
		};
		this._lastForceRenderDiag = diag;

		const foldManager = state.getFoldManager();
		const collapsedRegions = foldManager.getCollapsedRegions();
		const hasFolds = collapsedRegions.length > 0;

		const visibleLines = hasFolds
			? this.getVisibleLinesWithFolding(state, foldManager)
			: getVisibleLines(state, this.scrollOffset);

		diag.visibleLines = visibleLines.length;
		this.renderVisibleLines = visibleLines;

		const rctx = this.getLineRenderContext();
		const canvasWidth = this.canvas.width / this.dpr;
		const canvasHeight = this.canvas.height / this.dpr;
		const bgCSS = rgbToCSS(this.currentBackground);

		// Pre-parse packed data
		const packedSpans: (TextSpan[] | null)[] = new Array(visibleLines.length).fill(null);
		if (!hasFolds) {
			const packedRows = this.getVisibleRowsPacked(state, this.scrollOffset, visibleLines.length);
			for (let row = 0; row < visibleLines.length; row++) {
				const packed = packedRows[row];
				if (packed) {
					packedSpans[row] = groupPackedCellsIntoSpans(packed, this.cols);
				}
			}
		}

		// Two-pass rendering (no full-canvas clear to avoid flicker)
		// Pass 1: backgrounds — each row fills its full width, overwriting old content.
		// Null/undefined rows get a default background fill to prevent stale content.
		for (let row = 0; row < visibleLines.length; row++) {
			const line = visibleLines[row];
			if (line === null || !line) {
				// Null (fold summary placeholder) or undefined: fill with default background
				const y = row * this.charHeight;
				const fillY = Math.floor(y);
				const fillNextY = Math.ceil((row + 1) * this.charHeight);
				this.ctx.fillStyle = bgCSS;
				this.ctx.fillRect(0, fillY, canvasWidth, fillNextY - fillY);
				diag.emptyRows++;
				diag.bgFillCount++;
			} else {
				const spans = packedSpans[row];
				if (spans) {
					renderLineBackgroundFromSpansImpl(rctx, row, spans);
				} else {
					renderLineBackgroundImpl(rctx, row, line);
				}
				diag.bgFillCount++;
			}
		}

		// Clear area below the last visible row
		const lastRowBottom = Math.ceil(visibleLines.length * this.charHeight);
		if (lastRowBottom < canvasHeight) {
			this.ctx.fillStyle = bgCSS;
			this.ctx.fillRect(0, lastRowBottom, canvasWidth, canvasHeight - lastRowBottom);
		}

		// Pass 2: text
		for (let row = 0; row < visibleLines.length; row++) {
			const line = visibleLines[row];
			if (line === null) {
				// Summary line placeholder
			} else if (line) {
				const spans = packedSpans[row];
				if (spans) {
					renderLineTextFromSpansImpl(rctx, row, spans);
				} else {
					renderLineTextImpl(rctx, row, line);
				}
				diag.textPassRows++;
			}
		}

		// Fold summary line pass
		if (hasFolds) {
			this.renderFoldSummaryLines(state, visibleLines, foldManager);
		}

		// Search highlights
		if (this.searchMatches.length > 0) {
			this.renderSearchHighlights(state);
		}

		// Clear render-pass state
		this.renderVisibleLines = null;
		this.detectionCache.clear();

		state.clearDirty();

		if (this.scrollOffset === 0) {
			this.renderCursor(
				state.cursorCol,
				state.cursorRow,
				state.cursorVisible,
				state.cursorStyle,
				state.cursorBlink,
				state,
			);

			this.prevCursorCol = state.cursorCol;
			this.prevCursorRow = state.cursorRow;
			this.prevCursorVisible = state.cursorVisible;
			// Only count as 'rendered' when the cursor is visible. The
			// branch may execute with cursorVisible=false (renderCursor
			// then no-ops), and we don't want to report a draw that
			// didn't happen during freeze triage.
			diag.cursorRendered = state.cursorVisible;
		}

		// Diagnostic probe: paint a small region in the bottom-right corner
		// with a monotonically-changing color, then snapshot the 2D context
		// state. mux-window-manager.ts reads the painted pixels back via an
		// off-screen drawImage and compares to (probeR,probeG,probeB) to tell
		// whether the Canvas2D API actually committed pixels (vs the WASM
		// grid being correct but the canvas surface being stuck — the
		// "screen frozen but keys go through" symptom).
		//
		// Visible side-effect: a small colored square flickers at the bottom-
		// right corner. Acceptable for the freeze-investigation build.
		//
		// Coords are computed in device pixels and the transform is reset to
		// identity around the paint, so the probe always lands on integer
		// device-pixel boundaries even with fractional DPR. Otherwise a half-
		// pixel left edge would be alpha-blended and the readback would never
		// equal the expected RGB.
		try {
			this._probeSeq = (this._probeSeq + 1) & 0xffffff;
			const seq = this._probeSeq;
			const pr = seq & 0xff;
			const pg = (seq >> 8) & 0xff;
			const pb = (seq >> 16) & 0xff;
			const wDev = Math.max(1, Math.floor(4 * this.dpr));
			const hDev = wDev;
			const sxDev = Math.max(0, this.canvas.width - wDev);
			const syDev = Math.max(0, this.canvas.height - hDev);
			const prevTx = this.ctx.getTransform();
			this.ctx.setTransform(1, 0, 0, 1, 0, 0);
			this.ctx.fillStyle = `rgb(${pr},${pg},${pb})`;
			this.ctx.fillRect(sxDev, syDev, wDev, hDev);
			diag.ctxFillStyle = String(this.ctx.fillStyle);
			diag.ctxFont = this.ctx.font;
			diag.ctxAlpha = this.ctx.globalAlpha;
			diag.ctxComposite = this.ctx.globalCompositeOperation;
			diag.ctxTxA = prevTx.a;
			diag.ctxTxD = prevTx.d;
			diag.ctxTxE = prevTx.e;
			diag.ctxTxF = prevTx.f;
			this.ctx.setTransform(prevTx);
			diag.probeR = pr;
			diag.probeG = pg;
			diag.probeB = pb;
			diag.probeSxDev = sxDev;
			diag.probeSyDev = syDev;
			diag.probeWDev = wDev;
			diag.probeHDev = hDev;
		} catch {
			// Probe paint must never block real rendering — swallow.
		}
	}

	// ── Resize ────────────────────────────────────────────────

	resize(cols: number, rows: number): void {
		this.cols = cols;
		this.rows = rows;

		this.setupCanvas();

		if (this.pendingState) {
			this.forceRender(this.pendingState);
		}
	}

	// ── Settings (delegated) ──────────────────────────────────

	getFontFamily(): string {
		return this.fontFamily;
	}

	getFontSize(): number {
		return getFontSizePt(this.fontSize);
	}

	setFontSize(fontSize: number): void {
		this.fontSize = setFontSizeImpl(fontSize, this.getSettingsCallbacks());
		this.measureCharacterSize();
		if (this.pendingState) this.forceRender(this.pendingState);
	}

	setFontFamily(fontFamily: string): void {
		this.fontFamily = setFontFamilyImpl(fontFamily, this.getSettingsCallbacks());
		this.measureCharacterSize();
		if (this.pendingState) this.forceRender(this.pendingState);
	}

	setCursorStyle(style: CursorStyle): void {
		setCursorStyleImpl(style, this.getSettingsCallbacks());
	}

	setCursorBlink(blink: boolean): void {
		setCursorBlinkImpl(blink, this.getSettingsCallbacks());
	}

	setColorScheme(schemeName: string): void {
		const colorState = this.getColorState();
		setColorSchemeImpl(schemeName, colorState);
		this.applyColorState(colorState);
		if (this.pendingState) this.forceRender(this.pendingState);
	}

	setUserColorScheme(scheme: UserColorScheme): void {
		const colorState = this.getColorState();
		setUserColorSchemeImpl(scheme, colorState);
		this.applyColorState(colorState);
		if (this.pendingState) this.forceRender(this.pendingState);
	}

	setBoldBrightensAnsiColors(enabled: boolean): void {
		const colorState = this.getColorState();
		setBoldBrightensAnsiColorsImpl(enabled, colorState);
		this.applyColorState(colorState);
		if (this.pendingState) this.forceRender(this.pendingState);
	}

	applySetting<K extends keyof RendererSettings>(
		setting: K,
		value: RendererSettings[K],
	): void {
		switch (setting) {
			case "fontSize":
				this.setFontSize(value as number);
				break;
			case "fontFamily":
				this.setFontFamily(value as string);
				break;
			case "cursorStyle":
				this.setCursorStyle(value as CursorStyle);
				break;
			case "cursorBlink":
				this.setCursorBlink(value as boolean);
				break;
			case "colorScheme":
				this.setColorScheme(value as string);
				break;
			case "userColorScheme":
				if (value) {
					this.setUserColorScheme(value as UserColorScheme);
				}
				break;
			case "boldBrightensAnsiColors":
				this.setBoldBrightensAnsiColors(value as boolean);
				break;
		}
	}

	// ── Getters ───────────────────────────────────────────────

	getCharWidth(): number {
		return this.charWidth;
	}

	getCharHeight(): number {
		return this.charHeight;
	}

	// ── Scroll ────────────────────────────────────────────────

	scrollUp(lines: number): void {
		if (!this.pendingState) return;
		const maxOffset = this.pendingState.getScrollbackLength();
		this.scrollOffset = Math.min(this.scrollOffset + lines, maxOffset);
	}

	scrollDown(lines: number): void {
		this.scrollOffset = Math.max(this.scrollOffset - lines, 0);
	}

	getScrollOffset(): number {
		return this.scrollOffset;
	}

	setScrollOffset(offset: number): void {
		if (!this.pendingState) return;
		const maxOffset = this.pendingState.getScrollbackLength();
		this.scrollOffset = Math.max(0, Math.min(offset, maxOffset));
	}

	/**
	 * Get the scroll-pin baseline (last observed scrollback length).
	 * Exposed so the mux window manager can save/restore it per pane: the
	 * baseline is what scroll-pin correction measures growth against, and
	 * without per-pane save/restore a background pane's growth would be
	 * compared to the wrong pane's baseline. See doc/tasks/
	 * mux-per-pane-scroll-position (FR2).
	 */
	getScrollPinBaseline(): number {
		return this.prevScrollbackLength;
	}

	/**
	 * Set the scroll-pin baseline directly. Used by mux pane restore so the
	 * next adjustScrollOffsetForGrowth measures growth against this pane's own
	 * baseline rather than the previously active pane's.
	 */
	setScrollPinBaseline(baseline: number): void {
		this.prevScrollbackLength = Math.max(0, baseline);
	}

	// ── Search highlights ─────────────────────────────────────

	setSearchHighlights(matches: SearchMatch[], currentIndex: number): void {
		this.searchMatches = matches;
		this.searchCurrentIndex = currentIndex;
	}

	clearSearchHighlights(): void {
		this.searchMatches = [];
		this.searchCurrentIndex = -1;
	}

	/**
	 * Render search match highlights on the canvas.
	 */
	private renderSearchHighlights(state: TerminalState): void {
		const scrollbackLength = state.getScrollbackLength();
		const foldManager = state.getFoldManager();
		const hasFolds = foldManager.getCollapsedRegions().length > 0;

		const totalActualLines = scrollbackLength + state.rows;
		const totalDisplayLines = hasFolds
			? foldManager.getTotalDisplayLines(totalActualLines)
			: totalActualLines;
		const displayStart = Math.max(0, totalDisplayLines - state.rows - this.scrollOffset);
		const displayEnd = displayStart + state.rows;

		for (let i = 0; i < this.searchMatches.length; i++) {
			const match = this.searchMatches[i];
			if (!match) continue;

			if (hasFolds) {
				const region = foldManager.getRegionAtLine(match.lineIndex);
				if (region && region.collapsed) continue;
			}

			const displayLine = hasFolds
				? foldManager.actualLineToDisplay(match.lineIndex)
				: match.lineIndex;

			if (displayLine < displayStart || displayLine >= displayEnd) {
				continue;
			}

			const screenRow = displayLine - displayStart;

			const x = match.startCol * this.charWidth;
			const y = Math.floor(screenRow * this.charHeight);
			const width = (match.endCol - match.startCol) * this.charWidth;
			const height = Math.ceil(this.charHeight);

			if (i === this.searchCurrentIndex) {
				this.ctx.fillStyle = "rgba(230, 150, 30, 0.45)";
			} else {
				this.ctx.fillStyle = "rgba(230, 230, 50, 0.3)";
			}
			this.ctx.fillRect(x, y, width, height);
		}
	}

	// ── Hover ─────────────────────────────────────────────────

	setHoverPosition(row: number, col: number): void {
		if (row === this.hoverRow && col === this.hoverCol) return;
		this.hoverRow = row;
		this.hoverCol = col;
		if (this.pendingState) {
			this.scheduleRender(this.pendingState);
		}
	}

	setDiagnosticFlags(flags: { forceFullRender?: boolean }): void {
		if (flags.forceFullRender !== undefined) {
			this.forceFullRender = flags.forceFullRender;
			if (flags.forceFullRender) {
				console.info("[INFO][RENDERER] Diagnostic: forceFullRender enabled (EMTERM_FORCE_FULL_RENDER=1)");
			}
		}
	}

	// ── Dispose ───────────────────────────────────────────────

	startCompositorKeepAlive(): void {
		// No-op: removed.
	}

	stopCompositorKeepAlive(): void {
		// No-op: removed.
	}

	dispose(): void {
		this.stopCursorBlink();

		if (this.blinkTextTimer !== null) {
			clearInterval(this.blinkTextTimer);
			this.blinkTextTimer = null;
		}

		if (this.dprMediaQuery && this.dprChangeHandler) {
			this.dprMediaQuery.removeEventListener("change", this.dprChangeHandler);
		}

		if (this.canvas.parentNode) {
			this.canvas.parentNode.removeChild(this.canvas);
		}

		// Release GPU memory
		this.canvas.width = 0;
		this.canvas.height = 0;

		if (this.selectionState.selectionContainer?.parentNode) {
			this.selectionState.selectionContainer.parentNode.removeChild(this.selectionState.selectionContainer);
		}
	}
}
