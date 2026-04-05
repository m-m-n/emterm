/**
 * IME (Input Method Editor) handler for terminal application
 * Supports both EditContext API (Chromium/WebView2) and textarea fallback
 */

import type { PtyClient } from "../../pty/client";
import type { TerminalState } from "../../terminal/state";
import type { CharSize } from "../types";
import { IME_DEBUG, IME_COMPOSITION_CLASS } from "../config";
import { isModalOverlayVisible } from "../../shared/dom-utils";

/**
 * EditContext API type definitions (experimental Chromium feature)
 */
interface EditContextInit {
	text?: string;
	selectionStart?: number;
	selectionEnd?: number;
}

interface EditContext extends EventTarget {
	text: string;
	selectionStart: number;
	selectionEnd: number;
	updateText(start: number, end: number, text: string): void;
	updateSelection(start: number, end: number): void;
	updateControlBounds(bounds: DOMRect): void;
	updateSelectionBounds(bounds: DOMRect): void;
	updateCharacterBounds(start: number, bounds: DOMRect[]): void;
	addEventListener(type: string, listener: (event: any) => void): void;
	removeEventListener(type: string, listener: (event: any) => void): void;
}

interface EditContextConstructor {
	new (init?: EditContextInit): EditContext;
}

/**
 * Options for ImeHandler initialization
 */
export interface ImeHandlerOptions {
	container: HTMLElement;
	ptyClient: PtyClient;
	getState: () => TerminalState;
	charSize: CharSize;
	/** Check if this terminal tab is active (for multi-tab support) */
	isActiveTab?: () => boolean;
	/** Unique identifier for debugging */
	debugId?: string;
	/** Callback to exit scrollback mode (scroll to bottom) */
	onExitScrollback?: () => void;
}

/**
 * ImeHandler manages IME input for the terminal
 * Uses EditContext API when available (Chromium/WebView2), falls back to textarea
 */
export class ImeHandler {
	private container: HTMLElement;
	private ptyClient: PtyClient;
	private getState: () => TerminalState;
	private charSize: CharSize;
	private useEditContext: boolean;
	private imeInput: HTMLTextAreaElement | null = null;
	private compositionView: HTMLDivElement | null = null;
	private editContext: EditContext | null = null;
	private editContextCleanup: (() => void) | null = null;
	private terminalClickHandler: ((e: MouseEvent) => void) | null = null;

	/** Focus trap: recapture focus after WebKitGTK's native Tab traversal */
	private focusTrapFocusinHandler: ((e: FocusEvent) => void) | null = null;
	private focusTrapPointerDownHandler: (() => void) | null = null;
	private focusTrapPointerUpHandler: (() => void) | null = null;
	private pointerActive = false;
	private pointerActiveRafId: number | null = null;

	/** Back-tab escape sequence (ESC [ Z) - shared constant to avoid per-event allocation */
	private static readonly BACK_TAB_SEQUENCE = new Uint8Array([0x1b, 0x5b, 0x5a]);

	/** Check if this terminal tab is active */
	private isActiveTab: () => boolean;
	/** Unique identifier for debugging */
	private debugId: string;
	/** Callback to exit scrollback mode */
	private onExitScrollback: (() => void) | null;

	constructor(options: ImeHandlerOptions) {
		this.container = options.container;
		this.ptyClient = options.ptyClient;
		this.getState = options.getState;
		this.charSize = options.charSize;
		this.isActiveTab = options.isActiveTab || (() => true);
		this.debugId = options.debugId || `ime-${Date.now()}`;
		this.onExitScrollback = options.onExitScrollback || null;

		// Check if EditContext API is available
		this.useEditContext = typeof (window as any).EditContext !== "undefined";

		if (IME_DEBUG) {
			console.log("[ImeHandler] Initialization:", {
				useEditContext: this.useEditContext,
				charSize: this.charSize,
			});
		}
	}

	/**
	 * Initialize IME handler (EditContext or textarea fallback)
	 */
	init(): void {
		// Create composition view (used by both modes)
		this.compositionView = document.createElement("div");
		this.compositionView.id = "ime-composition-view";
		this.compositionView.className = IME_COMPOSITION_CLASS;
		document.body.appendChild(this.compositionView);

		if (this.useEditContext) {
			this.setupEditContextIME();
		} else {
			this.setupTextareaFallback();
		}

		if (IME_DEBUG) {
			console.log("[ImeHandler] Initialized with mode:", {
				mode: this.useEditContext ? "EditContext" : "Textarea",
			});
		}

		// Focus trap: WebKitGTK handles Shift+Tab at the native level without
		// dispatching a JavaScript keydown event. We identify Shift+Tab by three
		// conditions that are ONLY true for native Tab traversal:
		//   1. No pointer interaction (not a click/touch/pen)
		//   2. No JS keydown was dispatched (WebKitGTK swallows Shift+Tab keydown)
		//   3. Focus departed from the IME target (relatedTarget check)
		// Any programmatic focus change (search bar, settings, etc.) is preceded
		// by a keydown event (the shortcut that triggered it), so condition 2 filters it.
		this.focusTrapPointerDownHandler = () => { this.pointerActive = true; };
		this.focusTrapPointerUpHandler = () => {
			this.pointerActiveRafId = requestAnimationFrame(() => {
				this.pointerActive = false;
				this.pointerActiveRafId = null;
			});
		};
		document.addEventListener("pointerdown", this.focusTrapPointerDownHandler, { capture: true });
		document.addEventListener("pointerup", this.focusTrapPointerUpHandler, { capture: true });

		this.focusTrapFocusinHandler = (e: FocusEvent) => {
			// Ignore focus changes from pointer interactions (mouse/touch/pen)
			if (this.pointerActive) return;
			if (!this.isActiveTab()) return;
			if (isModalOverlayVisible()) return;

			const focusTarget = this.useEditContext ? this.container : this.imeInput;
			// Only act when focus DEPARTED from the IME target (relatedTarget)
			if (e.relatedTarget === focusTarget && e.target !== focusTarget) {
				// Allow focus to child UI elements (search bar, etc.)
				// but not the container itself (which is an intermediate
				// step in WebKitGTK's native Tab traversal)
				if (e.target !== this.container && this.container.contains(e.target as Node)) return;

				this.focus();
				this.ptyClient.write(ImeHandler.BACK_TAB_SEQUENCE).catch((error) => {
					console.error("Failed to write Shift+Tab to PTY:", error);
				});
			}
		};
		document.addEventListener("focusin", this.focusTrapFocusinHandler);
	}

	/**
	 * Clean up IME resources
	 */
	dispose(): void {
		// Clean up focus trap
		if (this.focusTrapPointerDownHandler) {
			document.removeEventListener("pointerdown", this.focusTrapPointerDownHandler, { capture: true });
			this.focusTrapPointerDownHandler = null;
		}
		if (this.focusTrapPointerUpHandler) {
			document.removeEventListener("pointerup", this.focusTrapPointerUpHandler, { capture: true });
			this.focusTrapPointerUpHandler = null;
		}
		if (this.pointerActiveRafId !== null) {
			cancelAnimationFrame(this.pointerActiveRafId);
			this.pointerActiveRafId = null;
		}
		this.pointerActive = false;
		if (this.focusTrapFocusinHandler) {
			document.removeEventListener("focusin", this.focusTrapFocusinHandler);
			this.focusTrapFocusinHandler = null;
		}

		// Clean up EditContext
		if (this.editContextCleanup) {
			this.editContextCleanup();
			this.editContextCleanup = null;
		}

		// Remove terminal click handler
		if (this.terminalClickHandler) {
			this.container.removeEventListener("click", this.terminalClickHandler);
			this.terminalClickHandler = null;
		}

		// Remove textarea
		if (this.imeInput) {
			this.imeInput.remove();
			this.imeInput = null;
		}

		// Remove composition view
		if (this.compositionView) {
			this.compositionView.remove();
			this.compositionView = null;
		}

		// Clear EditContext reference
		this.editContext = null;
	}

	/**
	 * Update IME position based on cursor position
	 */
	updatePosition(): void {
		// Handle EditContext mode (Windows/WebView2)
		if (this.useEditContext && this.editContext) {
			this.updateEditContextBounds();
			return;
		}

		if (!this.imeInput) {
			return;
		}

		const terminalState = this.getState();
		if (!terminalState) {
			return;
		}

		const cursorCol = terminalState.cursorCol;
		const cursorRow = terminalState.cursorRow;

		if (IME_DEBUG) {
			const visible = terminalState.cursorVisible;
			console.log(`[IME Debug] updatePosition: cursor=(${cursorCol},${cursorRow}) visible=${visible}`);
		}

		const rect = this.container.getBoundingClientRect();

		// Get computed styles for accurate padding
		const styles = getComputedStyle(this.container);
		const paddingLeft = parseFloat(styles.paddingLeft) || 0;
		const paddingTop = parseFloat(styles.paddingTop) || 0;

		let x: number;
		let y: number;

		if (terminalState.cursorVisible === false) {
			// TUI mode: position at last row of terminal grid
			// Use rows from terminal state to calculate exact grid bottom
			const lastRow = terminalState.rows - 1;
			x = paddingLeft;
			y = lastRow * this.charSize.height + paddingTop;
		} else {
			// Normal mode: position at cursor location
			// Get scroll offset if available
			const scrollOffset = (terminalState as any).getScrollOffset?.() ?? 0;
			x = cursorCol * this.charSize.width + paddingLeft;
			y = cursorRow * this.charSize.height + paddingTop - scrollOffset;
		}

		// Position textarea at calculated location for IME candidate window placement
		this.imeInput.style.left = `${rect.left + x}px`;
		this.imeInput.style.top = `${rect.top + y}px`;
		this.imeInput.style.width = `${this.charSize.width}px`;
		this.imeInput.style.height = `${this.charSize.height}px`;
	}

	/**
	 * Update character size (called when font size changes)
	 */
	updateCharSize(width: number, height: number): void {
		this.charSize = { width, height };
		this.updatePosition();
	}

	/**
	 * Focus the IME input
	 */
	focus(): void {
		if (this.useEditContext) {
			this.container.focus();
		} else if (this.imeInput) {
			this.imeInput.focus();
		}
	}

	/**
	 * Blur (unfocus) the IME input to prevent key interception.
	 * Called when a modal overlay (image viewer, markdown fullscreen) opens.
	 */
	blur(): void {
		if (this.useEditContext) {
			this.container.blur();
		} else if (this.imeInput) {
			this.imeInput.blur();
		}
	}

	/**
	 * Check if EditContext API is being used
	 */
	isEditContextActive(): boolean {
		return this.useEditContext && this.editContext !== null;
	}

	/**
	 * Check if IME input textarea has focus
	 */
	isImeInputFocused(): boolean {
		return this.imeInput !== null && document.activeElement === this.imeInput;
	}

	/**
	 * Set up IME using EditContext API (Chromium/WebView2 only)
	 */
	private setupEditContextIME(): void {
		const EditContextClass = (window as any)
			.EditContext as EditContextConstructor;
		this.editContext = new EditContextClass();

		// Make terminal editable with EditContext
		(this.container as any).editContext = this.editContext;
		this.container.tabIndex = 0;

		let compositionText = "";
		let isComposing = false;

		// Handle text updates (both direct input and composition)
		const onTextUpdate = (event: any) => {
			if (IME_DEBUG) {
				console.log("[EditContext] textupdate:", {
					text: event.text,
					selectionStart: event.selectionStart,
					selectionEnd: event.selectionEnd,
					compositionStart: event.compositionStart,
					compositionEnd: event.compositionEnd,
				});
			}

			// Block input when this tab is not active (for multi-tab support)
			if (!this.isActiveTab()) {
				if (IME_DEBUG) console.log("[EditContext] textupdate: blocked by inactive tab");
				if (this.editContext) {
					this.editContext.updateText(0, this.editContext.text.length, "");
					this.editContext.updateSelection(0, 0);
				}
				this.updateCompositionView("");
				return;
			}

			// Block input while modal overlay is visible (image viewer, markdown fullscreen)
			if (isModalOverlayVisible()) {
				if (IME_DEBUG) console.log("[EditContext] textupdate: blocked by modal overlay");
				// Reset EditContext text
				if (this.editContext) {
					this.editContext.updateText(0, this.editContext.text.length, "");
					this.editContext.updateSelection(0, 0);
				}
				this.updateCompositionView("");
				return;
			}

			const text = event.text;

			if (isComposing) {
				// Update composition view
				compositionText = text;
				this.updateCompositionView(text);
			} else {
				// Direct input - send to PTY
				if (text) {
					this.onExitScrollback?.();
					const bytes = new TextEncoder().encode(text);
					this.ptyClient.write(bytes).catch((error: unknown) => {
						console.error("Failed to write to PTY:", error);
					});
				}
				// Reset EditContext buffer after direct input to prevent accumulation.
				// Without this, the browser's text input system may intercept subsequent
				// special keys (e.g., Shift+Enter) to "commit" the accumulated buffer,
				// calling preventDefault() on the keydown event and causing key loss.
				if (this.editContext && this.editContext.text.length > 0) {
					this.editContext.updateText(0, this.editContext.text.length, "");
					this.editContext.updateSelection(0, 0);
				}
			}

			// Update EditContext's text bounds for IME positioning
			if (this.editContext) {
				this.updateEditContextBounds();
			}
		};

		// Handle composition start
		const onCompositionStart = (event: any) => {
			if (IME_DEBUG) console.log("[EditContext] compositionstart");
			isComposing = true;
			compositionText = "";
		};

		// Handle composition end
		const onCompositionEnd = (event: any) => {
			if (IME_DEBUG) console.log("[EditContext] compositionend");
			isComposing = false;

			// Block input when this tab is not active (for multi-tab support)
			if (!this.isActiveTab()) {
				if (IME_DEBUG) console.log("[EditContext] compositionend: blocked by inactive tab");
				compositionText = "";
				this.updateCompositionView("");
				if (this.editContext) {
					this.editContext.updateText(0, this.editContext.text.length, "");
					this.editContext.updateSelection(0, 0);
				}
				return;
			}

			// Block input while modal overlay is visible (image viewer, markdown fullscreen)
			if (isModalOverlayVisible()) {
				if (IME_DEBUG) console.log("[EditContext] compositionend: blocked by modal overlay");
				compositionText = "";
				this.updateCompositionView("");
				if (this.editContext) {
					this.editContext.updateText(0, this.editContext.text.length, "");
					this.editContext.updateSelection(0, 0);
				}
				return;
			}

			// Send the final composition text to PTY
			if (compositionText) {
				this.onExitScrollback?.();
				const bytes = new TextEncoder().encode(compositionText);
				this.ptyClient.write(bytes).catch((error) => {
					console.error("Failed to write to PTY:", error);
				});
			}

			// Clear composition view
			compositionText = "";
			this.updateCompositionView("");

			// Reset EditContext text
			if (this.editContext) {
				this.editContext.updateText(0, this.editContext.text.length, "");
				this.editContext.updateSelection(0, 0);
			}
		};

		// Handle character bounds request (for IME candidate window positioning)
		const onCharacterBoundsUpdate = (event: any) => {
			if (IME_DEBUG)
				console.log("[EditContext] characterboundsupdate:", event);
			if (this.editContext) {
				this.updateEditContextBounds();
			}
		};

		// Focus terminal to activate EditContext
		const onTerminalClick = () => {
			this.container.focus();
		};

		// Add event listeners
		this.editContext.addEventListener("textupdate", onTextUpdate);
		this.editContext.addEventListener("compositionstart", onCompositionStart);
		this.editContext.addEventListener("compositionend", onCompositionEnd);
		this.editContext.addEventListener(
			"characterboundsupdate",
			onCharacterBoundsUpdate,
		);
		this.container.addEventListener("click", onTerminalClick);

		// Store cleanup function
		this.editContextCleanup = () => {
			if (this.editContext) {
				this.editContext.removeEventListener("textupdate", onTextUpdate);
				this.editContext.removeEventListener(
					"compositionstart",
					onCompositionStart,
				);
				this.editContext.removeEventListener("compositionend", onCompositionEnd);
				this.editContext.removeEventListener(
					"characterboundsupdate",
					onCharacterBoundsUpdate,
				);
			}
			this.container.removeEventListener("click", onTerminalClick);
		};

		// Initial focus
		this.container.focus();
	}

	/**
	 * Set up IME using textarea fallback (for non-Chromium browsers)
	 */
	private setupTextareaFallback(): void {
		// Create IME input element with unique ID per instance
		const uniqueId = `ime-input-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
		this.imeInput = document.createElement("textarea");
		this.imeInput.id = uniqueId;
		this.imeInput.className = "ime-input";
		this.imeInput.style.position = "fixed";
		this.imeInput.style.opacity = "0";
		this.imeInput.style.pointerEvents = "none";
		this.imeInput.setAttribute("autocomplete", "off");
		this.imeInput.setAttribute("autocorrect", "off");
		this.imeInput.setAttribute("autocapitalize", "off");
		this.imeInput.setAttribute("spellcheck", "false");
		document.body.appendChild(this.imeInput);

		// Set up IME event handlers
		if (this.compositionView) {
			this.setupIMEHandlers(this.imeInput, this.compositionView);
		}

		// Update position
		this.updatePosition();
	}

	/**
	 * Update EditContext bounds for IME positioning
	 */
	private updateEditContextBounds(): void {
		if (!this.editContext) return;

		const terminalState = this.getState();
		if (!terminalState) return;

		const cursorCol = terminalState.cursorCol;
		const cursorRow = terminalState.cursorRow;

		if (IME_DEBUG) {
			const visible = terminalState.cursorVisible;
			console.log(`[IME Debug] updateEditContextBounds: cursor=(${cursorCol},${cursorRow}) visible=${visible}`);
		}

		const rect = this.container.getBoundingClientRect();

		// Get computed styles for accurate padding
		const styles = getComputedStyle(this.container);
		const paddingLeft = parseFloat(styles.paddingLeft) || 0;
		const paddingTop = parseFloat(styles.paddingTop) || 0;

		let x: number;
		let y: number;

		if (terminalState.cursorVisible === false) {
			// TUI mode: position at last row of terminal grid
			const lastRow = terminalState.rows - 1;
			x = rect.left + paddingLeft;
			y = rect.top + lastRow * this.charSize.height + paddingTop;
		} else {
			// Normal mode: position at cursor location
			// Get scroll offset if available
			const scrollOffset = (terminalState as any).getScrollOffset?.() ?? 0;
			x = rect.left + cursorCol * this.charSize.width + paddingLeft;
			y = rect.top + cursorRow * this.charSize.height + paddingTop - scrollOffset;
		}

		// Set control bounds (the editable area)
		this.editContext.updateControlBounds(
			new DOMRect(rect.left, rect.top, rect.width, rect.height),
		);

		// Set selection bounds (cursor position)
		this.editContext.updateSelectionBounds(
			new DOMRect(x, y, this.charSize.width, this.charSize.height),
		);

		// Set character bounds for composition text
		const textLength = this.editContext.text?.length || 0;
		if (textLength > 0) {
			const bounds: DOMRect[] = [];
			for (let i = 0; i < textLength; i++) {
				bounds.push(
					new DOMRect(
						x + i * this.charSize.width,
						y,
						this.charSize.width,
						this.charSize.height,
					),
				);
			}
			this.editContext.updateCharacterBounds(0, bounds);
		}
	}

	/**
	 * Update composition view position and content
	 */
	private updateCompositionView(text: string): void {
		if (!this.compositionView) return;

		if (IME_DEBUG) {
			console.log("[IME Debug] updateCompositionView:", {
				text,
				viewId: this.compositionView.id,
			});
		}

		const terminalState = this.getState();
		if (!terminalState) return;

		if (!text) {
			this.compositionView.style.display = "none";
			this.compositionView.textContent = "";
			return;
		}

		const rect = this.container.getBoundingClientRect();
		const cursorCol = terminalState.cursorCol;
		const cursorRow = terminalState.cursorRow;

		// Get computed styles for accurate padding
		const styles = getComputedStyle(this.container);
		const paddingLeft = parseFloat(styles.paddingLeft) || 0;
		const paddingTop = parseFloat(styles.paddingTop) || 0;

		let x: number;
		let y: number;

		if (terminalState.cursorVisible === false) {
			// TUI mode: position at last row of terminal grid
			const lastRow = terminalState.rows - 1;
			x = rect.left + paddingLeft;
			y = rect.top + lastRow * this.charSize.height + paddingTop;
		} else {
			// Normal mode: position at cursor location
			// Get scroll offset if available
			const scrollOffset = (terminalState as any).getScrollOffset?.() ?? 0;
			x = rect.left + cursorCol * this.charSize.width + paddingLeft;
			y = rect.top + cursorRow * this.charSize.height + paddingTop - scrollOffset;
		}

		if (IME_DEBUG) {
			console.log("[IME Debug] positioning compositionView at:", {
				x,
				y,
				cursorCol,
				cursorRow,
				cursorVisible: terminalState.cursorVisible,
				rectLeft: rect.left,
				rectTop: rect.top,
				paddingLeft,
				paddingTop,
			});
		}

		this.compositionView.style.left = `${x}px`;
		this.compositionView.style.top = `${y}px`;
		this.compositionView.style.display = "block";
		this.compositionView.textContent = text;
	}

	/**
	 * Set up IME event handlers for textarea mode
	 */
	private setupIMEHandlers(
		input: HTMLTextAreaElement,
		view: HTMLDivElement,
	): void {
		// Track if we're in composition mode
		let isComposing = false;
		// One-shot token: set by compositionend after sending, consumed by the
		// immediately-following input event. Prevents double-send of the same
		// composition commit without blocking key-repeat throughput.
		let compositionJustCommitted = false;

		// Focus debugging
		input.addEventListener("focus", () => {
			if (IME_DEBUG) console.log("[IME Debug] textarea focus gained");
		});
		input.addEventListener("blur", () => {
			if (IME_DEBUG)
				console.log(
					"[IME Debug] textarea focus lost, activeElement:",
					document.activeElement?.tagName,
				);
		});

		// Handle compositionstart to reset flags
		input.addEventListener("compositionstart", (event) => {
			if (IME_DEBUG) {
				console.log("[IME Debug] compositionstart:", {
					data: (event as CompositionEvent).data,
					inputValue: input.value,
				});
			}
			isComposing = true;
		});

		// Handle compositionupdate - show current composition
		input.addEventListener("compositionupdate", (event) => {
			const ce = event as CompositionEvent;
			if (IME_DEBUG) {
				console.log("[IME Debug] compositionupdate:", {
					data: ce.data,
					inputValue: input.value,
					viewDisplay: view.style.display,
				});
			}
			// Show composition text in view (use event.data if input.value is empty)
			const displayText = input.value || ce.data || "";
			if (displayText) {
				this.updateCompositionView(displayText);
			}
		});

		// Handle beforeinput for debugging
		input.addEventListener("beforeinput", (event) => {
			if (IME_DEBUG) {
				console.log("[IME Debug] beforeinput:", {
					inputType: event.inputType,
					data: event.data,
					isComposing: event.isComposing,
				});
			}
		});

		// Handle compositioncancel to cleanup
		input.addEventListener("compositioncancel", () => {
			if (IME_DEBUG) console.log("[IME Debug] compositioncancel");
			isComposing = false;
			input.value = "";
			this.updateCompositionView("");
		});

		// Handle input event (primary handler)
		input.addEventListener("input", (event: Event) => {
			const inputEvent = event as InputEvent;
			const value = input.value;
			const isActive = this.isActiveTab();

			// Skip if this tab is not active (for multi-tab support)
			if (!isActive) {
				input.value = "";
				this.updateCompositionView("");
				return;
			}

			// Block input while modal overlay is visible (image viewer, markdown fullscreen)
			if (isModalOverlayVisible()) {
				if (IME_DEBUG) console.log("[IME Debug] input: blocked by modal overlay");
				input.value = "";
				this.updateCompositionView("");
				return;
			}

			if (IME_DEBUG) {
				console.log("[IME Debug] input event:", {
					value,
					isComposing: inputEvent.isComposing,
					localIsComposing: isComposing,
					inputType: inputEvent.inputType,
					data: inputEvent.data,
					});
			}

			// If composing, show in composition view
			if (inputEvent.isComposing || isComposing) {
				if (IME_DEBUG)
					console.log("[IME Debug] input: composing, updating view");
				this.updateCompositionView(value);
				return;
			}

			// Not composing - this is final input, send to PTY
			if (!value) {
				return;
			}

			// One-shot guard: if compositionend just committed this exact value,
			// skip this input event (it is the paired double-fire, not a new keystroke).
			if (compositionJustCommitted) {
				compositionJustCommitted = false;
				if (IME_DEBUG) console.log("[IME Debug] input: post-composition double-fire, skipping");
				input.value = "";
				this.updateCompositionView("");
				return;
			}

			if (IME_DEBUG) console.log("[IME Debug] input: sending value:", value);
			this.onExitScrollback?.();
			const bytes = new TextEncoder().encode(value);
			this.ptyClient.write(bytes).catch((error) => {
				console.error("Failed to write IME input to PTY:", error);
			});
			input.value = "";
			this.updateCompositionView("");
		});

		// Handle compositionend (fallback for standard IME)
		input.addEventListener("compositionend", (event) => {
			if (IME_DEBUG) {
				console.log("[IME Debug] compositionend:", {
					data: (event as CompositionEvent).data,
					inputValue: input.value,
				});
			}

			// Mark composition as ended
			isComposing = false;

			// Block input when this tab is not active (for multi-tab support)
			if (!this.isActiveTab()) {
				if (IME_DEBUG) console.log("[IME Debug] compositionend: blocked by inactive tab");
				input.value = "";
				this.updateCompositionView("");
				return;
			}

			// Block input while modal overlay is visible (image viewer, markdown fullscreen)
			if (isModalOverlayVisible()) {
				if (IME_DEBUG) console.log("[IME Debug] compositionend: blocked by modal overlay");
				input.value = "";
				this.updateCompositionView("");
				return;
			}

			const value = input.value;
			if (!value) {
				if (IME_DEBUG)
					console.log("[IME Debug] compositionend: no value, returning");
				this.updateCompositionView("");
				return;
			}

			if (IME_DEBUG)
				console.log("[IME Debug] compositionend: sending value:", value);
			this.onExitScrollback?.();
			const bytes = new TextEncoder().encode(value);
			// Set one-shot flag BEFORE async write to prevent race condition
			// where the paired input event fires before write() resolves.
			compositionJustCommitted = true;
			this.ptyClient.write(bytes).catch((error) => {
				compositionJustCommitted = false;
				console.error("Failed to write IME composition to PTY:", error);
			});
			if (IME_DEBUG)
				console.log("[IME Debug] compositionend: sent successfully");
			input.value = "";
			this.updateCompositionView("");
		});
	}
}
