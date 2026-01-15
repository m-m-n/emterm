/**
 * Fullscreen Markdown view.
 *
 * Manages fullscreen Markdown display as an overlay.
 *
 * @module markdown/fullscreen
 */

import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { open as shellOpen } from "@tauri-apps/plugin-shell";
import { LinkConfirmDialog } from "./link-dialog.ts";
import { ZoomController } from "../shared/zoom-controller.ts";
import type { FullscreenConfig, FullscreenState, MarkdownBlock } from "./types.ts";

/**
 * Default fullscreen configuration.
 */
const DEFAULT_CONFIG: FullscreenConfig = {
	showCloseButton: false,
	alwaysShowScrollbar: true,
	showCopyButtons: true,
	linkBehavior: "confirm",
};

/**
 * Manages fullscreen Markdown display.
 */
export class FullscreenMarkdownView {
	/** Overlay element */
	private overlay: HTMLElement | null = null;

	/** Content container element */
	private content: HTMLElement | null = null;

	/** Current state */
	private state: FullscreenState = {
		isActive: false,
	};

	/** Current configuration */
	private config: FullscreenConfig = DEFAULT_CONFIG;

	/** Bound event handlers for cleanup */
	private boundHandleKeydown: (e: KeyboardEvent) => void;
	private boundHandleCopyClick: (e: MouseEvent) => void;
	private boundHandleLinkClick: (e: MouseEvent) => void;

	/** Element that had focus before fullscreen was opened */
	private previouslyFocusedElement: HTMLElement | null = null;

	/** Link confirmation dialog */
	private linkDialog: LinkConfirmDialog;

	/** Zoom controller */
	private zoomController: ZoomController | null = null;

	/**
	 * Create a new fullscreen view.
	 */
	constructor() {
		this.boundHandleKeydown = this.handleKeydown.bind(this);
		this.boundHandleCopyClick = this.handleCopyClick.bind(this);
		this.boundHandleLinkClick = this.handleLinkClick.bind(this);
		this.linkDialog = new LinkConfirmDialog();
	}

	/**
	 * Show Markdown content in fullscreen mode.
	 *
	 * @param block - Rendered Markdown block
	 * @param config - Display configuration
	 */
	show(block: MarkdownBlock, config?: Partial<FullscreenConfig>): void {
		// Close existing if any
		if (this.state.isActive) {
			this.close();
		}

		// Save currently focused element for restoration on close
		this.previouslyFocusedElement = document.activeElement as HTMLElement | null;

		this.config = { ...DEFAULT_CONFIG, ...config };

		// Create overlay
		this.overlay = document.createElement("div");
		this.overlay.className = "markdown-fullscreen-overlay";
		this.overlay.setAttribute("role", "dialog");
		this.overlay.setAttribute("aria-modal", "true");
		this.overlay.setAttribute("aria-label", "Markdown Document");

		// Create content container
		this.content = document.createElement("div");
		this.content.className = "markdown-fullscreen-content";
		this.content.innerHTML = block.html;

		// Add copy buttons to code blocks
		if (this.config.showCopyButtons) {
			this.addCopyButtons();
		}

		// Configure scrollbar - auto shows when content overflows
		this.content.style.overflowY = this.config.alwaysShowScrollbar ? "scroll" : "auto";

		// Assemble and insert
		this.overlay.appendChild(this.content);
		document.body.appendChild(this.overlay);

		// Set up event listeners
		// Note: Use capture phase to intercept keyboard events before terminal KeyboardHandler
		document.addEventListener("keydown", this.boundHandleKeydown, { capture: true });
		this.content.addEventListener("click", this.boundHandleCopyClick);
		this.content.addEventListener("click", this.boundHandleLinkClick);

		// Update state
		this.state.isActive = true;

		// Focus for keyboard navigation
		this.content.setAttribute("tabindex", "-1");
		this.content.focus();

		// Initialize zoom controller
		this.zoomController = new ZoomController({
			container: this.content,
			overlay: this.overlay,
			onClose: () => this.close(),
		});

		console.log(`[LOG][FRONTEND] Fullscreen markdown view opened: ${block.id}`);
	}

	/**
	 * Close fullscreen view and cleanup.
	 */
	close(): void {
		if (!this.state.isActive) return;

		// Dispose zoom controller
		if (this.zoomController) {
			this.zoomController.dispose();
			this.zoomController = null;
		}

		// Remove event listeners (with guard for test environment)
		// Note: Must match the capture phase used in addEventListener
		if (typeof document !== "undefined" && document.removeEventListener) {
			document.removeEventListener("keydown", this.boundHandleKeydown, { capture: true });
		}
		if (this.content) {
			this.content.removeEventListener("click", this.boundHandleCopyClick);
			this.content.removeEventListener("click", this.boundHandleLinkClick);
		}

		// Close link dialog if open
		this.linkDialog.close();

		// Remove from DOM (with guard for test environment)
		if (this.overlay) {
			if (typeof this.overlay.remove === "function") {
				this.overlay.remove();
			} else if (this.overlay.parentNode) {
				this.overlay.parentNode.removeChild(this.overlay);
			}
			this.overlay = null;
			this.content = null;
		}

		// Restore focus to previously focused element
		if (
			this.previouslyFocusedElement &&
			typeof this.previouslyFocusedElement.focus === "function"
		) {
			this.previouslyFocusedElement.focus();
		}
		this.previouslyFocusedElement = null;

		// Reset state
		this.state = {
			isActive: false,
		};

		console.log("[LOG][FRONTEND] Fullscreen markdown view closed");
	}

	/**
	 * Check if fullscreen view is currently active.
	 */
	isActive(): boolean {
		return this.state.isActive;
	}

	/**
	 * Scroll to position.
	 *
	 * @param position - Scroll position or "top" | "bottom"
	 */
	scrollTo(position: number | "top" | "bottom"): void {
		if (!this.content) return;

		if (position === "top") {
			this.content.scrollTop = 0;
		} else if (position === "bottom") {
			this.content.scrollTop = this.content.scrollHeight;
		} else {
			this.content.scrollTop = position;
		}
	}

	/**
	 * Scroll by amount.
	 *
	 * @param delta - Scroll delta (positive = down, negative = up)
	 */
	scrollBy(delta: number): void {
		if (!this.content) return;
		this.content.scrollBy({ top: delta, behavior: "smooth" });
	}

	/**
	 * Handle keyboard events.
	 * Note: If link dialog is shown, it handles its own keyboard events.
	 * All keyboard input is blocked while fullscreen is active to prevent
	 * keys from being sent to the underlying shell.
	 */
	private handleKeydown(e: KeyboardEvent): void {
		if (!this.state.isActive) return;

		// When link dialog is shown, let it handle keyboard events
		if (this.linkDialog.isShown()) {
			// Still prevent default to block shell input
			e.preventDefault();
			return;
		}

		// Block all keyboard input from reaching the shell while fullscreen is active
		e.preventDefault();

		switch (e.key) {
			case "Escape":
				this.close();
				break;

			case "ArrowUp":
				this.scrollBy(-40); // ~1 line
				break;

			case "ArrowDown":
				this.scrollBy(40);
				break;

			case "PageUp":
				this.scrollBy(-(this.content?.clientHeight || 400));
				break;

			case "PageDown":
				this.scrollBy(this.content?.clientHeight || 400);
				break;

			case "Home":
				this.scrollTo("top");
				break;

			case "End":
				this.scrollTo("bottom");
				break;

			case "Tab":
				this.handleTabKey(e);
				break;

			// All other keys are blocked (preventDefault already called above)
		}
	}

	/**
	 * Handle Tab key for focus trap within fullscreen overlay.
	 * Cycles focus among focusable elements (links, buttons).
	 */
	private handleTabKey(e: KeyboardEvent): void {
		if (!this.content) return;

		const focusableElements = this.content.querySelectorAll<HTMLElement>(
			'a[href], button, [tabindex]:not([tabindex="-1"])',
		);
		if (focusableElements.length === 0) return;

		const focusableArray = Array.from(focusableElements);
		const first = focusableArray[0];
		const last = focusableArray[focusableArray.length - 1];

		// Safety check (should never happen since we check length above)
		if (!first || !last) return;

		if (e.shiftKey) {
			// Shift+Tab: move backward
			if (
				document.activeElement === first ||
				document.activeElement === this.content
			) {
				e.preventDefault();
				last.focus();
			}
		} else {
			// Tab: move forward
			if (document.activeElement === last) {
				e.preventDefault();
				first.focus();
			}
		}
	}

	/**
	 * Add copy buttons to code blocks.
	 */
	private addCopyButtons(): void {
		if (!this.content) return;

		const codeBlocks = this.content.querySelectorAll("pre > code");
		for (const code of codeBlocks) {
			const pre = code.parentElement;
			if (!pre) continue;

			// Wrap in container for positioning
			pre.style.position = "relative";

			const button = document.createElement("button");
			button.className = "copy-code-button";
			button.setAttribute("type", "button");
			button.setAttribute("aria-label", "Copy code");
			button.innerHTML = '<span class="copy-icon">Copy</span>';

			pre.appendChild(button);
		}
	}

	/**
	 * Handle copy button clicks.
	 */
	private async handleCopyClick(e: MouseEvent): Promise<void> {
		const target = e.target as HTMLElement;
		const button = target.closest(".copy-code-button");
		if (!button) return;

		e.preventDefault();
		e.stopPropagation();

		const pre = button.closest("pre");
		const code = pre?.querySelector("code");
		if (!code) return;

		const text = code.textContent || "";

		try {
			await writeText(text);
			this.showCopyFeedback(button as HTMLElement, true);
			console.log("[LOG][FRONTEND] Code copied to clipboard");
		} catch (err) {
			this.showCopyFeedback(button as HTMLElement, false);
			console.error("[ERROR][FRONTEND] Failed to copy code", err);
		}
	}

	/**
	 * Show copy feedback on button.
	 */
	private showCopyFeedback(button: HTMLElement, success: boolean): void {
		const originalText = button.innerHTML;
		button.innerHTML = success
			? '<span class="copy-icon">Copied!</span>'
			: '<span class="copy-icon">Failed</span>';
		button.classList.add(success ? "copy-success" : "copy-error");

		setTimeout(() => {
			button.innerHTML = originalText;
			button.classList.remove("copy-success", "copy-error");
		}, 2000);
	}

	/**
	 * Handle link clicks.
	 */
	private async handleLinkClick(e: MouseEvent): Promise<void> {
		const target = e.target as HTMLElement;
		const link = target.closest("a");
		if (!link) return;

		e.preventDefault();

		const href = link.getAttribute("href");
		if (!href) return;

		// Skip non-http(s) links
		if (!href.startsWith("http://") && !href.startsWith("https://")) {
			return;
		}

		// Ctrl+Click or Cmd+Click bypasses confirmation
		const bypassConfirm = e.ctrlKey || e.metaKey;

		if (bypassConfirm || this.config.linkBehavior === "direct") {
			await this.openLink(href);
		} else if (this.config.linkBehavior === "confirm") {
			const confirmed = await this.linkDialog.confirm(href);
			if (confirmed) {
				await this.openLink(href);
			}
		}
		// linkBehavior === "disabled": do nothing
	}

	/**
	 * Open link in external browser.
	 */
	private async openLink(url: string): Promise<void> {
		try {
			await shellOpen(url);
			console.log(`[LOG][FRONTEND] Opened external link: ${url}`);
		} catch (err) {
			console.error(`[ERROR][FRONTEND] Failed to open link: ${url}`, err);
		}
	}

	/**
	 * Dispose view and release resources.
	 */
	dispose(): void {
		this.close();
		this.linkDialog.dispose();
	}
}
