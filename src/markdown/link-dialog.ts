/**
 * Link confirmation dialog.
 *
 * Provides a confirmation dialog for opening external links.
 *
 * @module markdown/link-dialog
 */

import { t } from "../i18n/index.ts";

/**
 * Confirmation dialog for external links.
 */
export class LinkConfirmDialog {
	/** Dialog element */
	private dialog: HTMLElement | null = null;

	/** Container element (overlay-root) for rendering */
	private container: HTMLElement | null = null;

	/** Promise resolver */
	private resolvePromise: ((value: boolean) => void) | null = null;

	/** Bound keyboard handler */
	private boundHandleKeydown: (e: KeyboardEvent) => void;

	/**
	 * Create a new dialog.
	 */
	constructor() {
		this.boundHandleKeydown = this.handleKeydown.bind(this);
	}

	/**
	 * Set container for dialog rendering.
	 * @param container - Container element (overlay-root) to append dialog to
	 */
	setContainer(container: HTMLElement): void {
		this.container = container;
	}

	/**
	 * Show confirmation dialog for URL.
	 *
	 * @param url - URL to confirm
	 * @returns Promise resolving to true if user confirms, false if cancelled
	 */
	confirm(url: string): Promise<boolean> {
		return new Promise((resolve) => {
			this.resolvePromise = resolve;

			// Create dialog
			this.dialog = document.createElement("div");
			this.dialog.className = "link-confirm-dialog-overlay";
			this.dialog.innerHTML = `
				<div class="link-confirm-dialog" role="alertdialog" aria-modal="true">
					<h3 class="link-confirm-title">${this.escapeHtml(t("link.title"))}</h3>
					<p class="link-confirm-url">${this.escapeHtml(url)}</p>
					<div class="link-confirm-buttons">
						<button class="link-confirm-cancel" type="button">${this.escapeHtml(t("link.cancel"))}</button>
						<button class="link-confirm-open" type="button">${this.escapeHtml(t("link.open"))}</button>
					</div>
				</div>
			`;

			// Event listeners
			const openBtn = this.dialog.querySelector(".link-confirm-open");
			const cancelBtn = this.dialog.querySelector(".link-confirm-cancel");

			openBtn?.addEventListener("click", () => this.handleConfirm(true));
			cancelBtn?.addEventListener("click", () => this.handleConfirm(false));
			this.dialog.addEventListener("click", (e) => {
				if (e.target === this.dialog) {
					this.handleConfirm(false);
				}
			});

			document.addEventListener("keydown", this.boundHandleKeydown);

			// Append to container if set, otherwise fall back to document.body
			if (this.container) {
				this.container.appendChild(this.dialog);
			} else {
				document.body.appendChild(this.dialog);
			}

			// Focus open button
			(openBtn as HTMLElement)?.focus();
		});
	}

	/**
	 * Handle dialog confirmation.
	 */
	private handleConfirm(confirmed: boolean): void {
		if (this.resolvePromise) {
			this.resolvePromise(confirmed);
			this.resolvePromise = null;
		}
		this.close();
	}

	/**
	 * Handle keyboard events.
	 * Note: stopPropagation prevents parent (FullscreenMarkdownView) from handling these events.
	 */
	private handleKeydown(e: KeyboardEvent): void {
		// Always stop propagation to prevent parent overlay from handling
		e.stopPropagation();

		if (e.key === "Escape") {
			e.preventDefault();
			this.handleConfirm(false);
		} else if (e.key === "Enter") {
			e.preventDefault();
			this.handleConfirm(true);
		}
	}

	/**
	 * Close dialog without action.
	 * Resolves pending promise with false if not yet resolved.
	 */
	close(): void {
		// Resolve pending promise with false
		if (this.resolvePromise) {
			this.resolvePromise(false);
			this.resolvePromise = null;
		}

		// Guard for test environment where document may not have removeEventListener
		if (typeof document !== "undefined" && document.removeEventListener) {
			document.removeEventListener("keydown", this.boundHandleKeydown);
		}
		if (this.dialog) {
			// Guard for test environment where remove may not be available
			if (typeof this.dialog.remove === "function") {
				this.dialog.remove();
			} else if (this.dialog.parentNode) {
				this.dialog.parentNode.removeChild(this.dialog);
			}
			this.dialog = null;
		}
	}

	/**
	 * Check if dialog is currently shown.
	 */
	isShown(): boolean {
		return this.dialog !== null;
	}

	/**
	 * Escape HTML for safe display.
	 */
	private escapeHtml(text: string): string {
		return text
			.replace(/&/g, "&amp;")
			.replace(/</g, "&lt;")
			.replace(/>/g, "&gt;")
			.replace(/"/g, "&quot;")
			.replace(/'/g, "&#039;");
	}

	/**
	 * Dispose dialog.
	 */
	dispose(): void {
		this.close();
	}
}
