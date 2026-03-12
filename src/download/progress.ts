/**
 * Download progress display.
 *
 * Shows a toast-style progress indicator during file downloads.
 *
 * @module download/progress
 */

export class DownloadProgressDisplay {
	private container: HTMLElement | null = null;
	private toast: HTMLElement | null = null;
	private autoDismissTimer: ReturnType<typeof setTimeout> | null = null;

	setContainer(container: HTMLElement): void {
		this.container = container;
	}

	show(filename: string, progress: number): void {
		if (!this.container) return;

		if (!this.toast) {
			this.toast = document.createElement("div");
			this.toast.className = "download-toast";
			this.container.appendChild(this.toast);
		}

		const pct = Math.round(progress);
		this.toast.innerHTML = `
			<div class="download-toast__content">
				<span class="download-toast__icon">&#x2B07;</span>
				<span class="download-toast__filename">${this.escapeHtml(filename)}</span>
				<span class="download-toast__progress">${pct}%</span>
			</div>
			<div class="download-toast__bar">
				<div class="download-toast__bar-fill" style="width: ${pct}%"></div>
			</div>
		`;
		this.toast.style.display = "";
		this.clearAutoDismiss();
	}

	showCompleted(filename: string): void {
		if (!this.toast) return;

		this.toast.innerHTML = `
			<div class="download-toast__content">
				<span class="download-toast__icon">&#x2705;</span>
				<span class="download-toast__filename">${this.escapeHtml(filename)}</span>
				<span class="download-toast__progress">Done</span>
			</div>
			<div class="download-toast__bar">
				<div class="download-toast__bar-fill download-toast__bar-fill--done" style="width: 100%"></div>
			</div>
		`;
		this.autoDismiss(3000);
	}

	showCancelled(): void {
		if (!this.toast) return;

		this.toast.innerHTML = `
			<div class="download-toast__content">
				<span class="download-toast__icon">&#x274C;</span>
				<span class="download-toast__filename">Cancelled</span>
			</div>
		`;
		this.autoDismiss(2000);
	}

	hide(): void {
		this.clearAutoDismiss();
		if (this.toast) {
			this.toast.remove();
			this.toast = null;
		}
	}

	dispose(): void {
		this.hide();
	}

	private autoDismiss(ms: number): void {
		this.clearAutoDismiss();
		this.autoDismissTimer = setTimeout(() => {
			this.hide();
		}, ms);
	}

	private clearAutoDismiss(): void {
		if (this.autoDismissTimer !== null) {
			clearTimeout(this.autoDismissTimer);
			this.autoDismissTimer = null;
		}
	}

	private escapeHtml(text: string): string {
		const div = document.createElement("div");
		div.textContent = text;
		return div.innerHTML;
	}
}
