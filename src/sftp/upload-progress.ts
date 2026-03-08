/**
 * Upload Progress UI
 *
 * Displays upload progress in the top-right corner as non-blocking toasts.
 * Each upload gets its own progress item showing filename, progress bar, and status.
 */

import { t } from "../i18n/index.ts";

/**
 * Progress status for a single upload.
 */
export interface UploadProgressInfo {
  sessionId: string;
  fileName: string;
  bytesTransferred: number;
  totalBytes: number;
  status: "preparing" | "uploading" | "completed" | "failed" | "cancelled";
  errorMessage?: string;
}

/**
 * Manages the progress display container and individual progress items.
 */
export class UploadProgressDisplay {
  private container: HTMLElement | null = null;
  private items: Map<string, HTMLElement> = new Map();
  private onCancelCallback: ((sessionId: string) => void) | null = null;

  /**
   * Set the callback for cancel button clicks.
   */
  onCancel(callback: (sessionId: string) => void): void {
    this.onCancelCallback = callback;
  }

  /**
   * Update the progress display for a specific upload session.
   */
  update(info: UploadProgressInfo): void {
    this.ensureContainer();

    let item = this.items.get(info.sessionId);
    if (!item) {
      item = this.createItem(info.sessionId, info.fileName);
      this.items.set(info.sessionId, item);
      this.container!.appendChild(item);
    }

    this.updateItem(item, info);

    // Remove completed/failed/cancelled items after a delay
    if (
      info.status === "completed" ||
      info.status === "failed" ||
      info.status === "cancelled"
    ) {
      setTimeout(() => {
        item!.classList.add("sftp-progress-item-done");
        setTimeout(() => {
          item!.remove();
          this.items.delete(info.sessionId);
          this.cleanupContainerIfEmpty();
        }, 300);
      }, 2000);
    }
  }

  /**
   * Dispose the progress display.
   */
  dispose(): void {
    if (this.container) {
      this.container.remove();
      this.container = null;
    }
    this.items.clear();
  }

  private ensureContainer(): void {
    if (this.container) return;
    this.container = document.createElement("div");
    this.container.className = "sftp-progress-container";
    document.body.appendChild(this.container);
  }

  private createItem(sessionId: string, fileName: string): HTMLElement {
    const item = document.createElement("div");
    item.className = "sftp-progress-item";
    item.dataset.sessionId = sessionId;

    // Header row with filename and cancel button
    const header = document.createElement("div");
    header.style.display = "flex";
    header.style.alignItems = "center";
    header.style.justifyContent = "space-between";

    const fileNameEl = document.createElement("div");
    fileNameEl.className = "sftp-progress-filename";
    fileNameEl.textContent = fileName;
    header.appendChild(fileNameEl);

    const cancelBtn = document.createElement("button");
    cancelBtn.className = "sftp-progress-cancel-btn";
    cancelBtn.textContent = t("sftp.progress.cancel");
    cancelBtn.addEventListener("click", () => {
      this.onCancelCallback?.(sessionId);
    });
    header.appendChild(cancelBtn);

    item.appendChild(header);

    // Status row with spinner and text
    const statusRow = document.createElement("div");
    statusRow.className = "sftp-progress-status-row";

    const spinner = document.createElement("div");
    spinner.className = "sftp-progress-spinner";
    statusRow.appendChild(spinner);

    const status = document.createElement("div");
    status.className = "sftp-progress-status";
    status.textContent = t("sftp.progress.preparing");
    statusRow.appendChild(status);

    item.appendChild(statusRow);

    return item;
  }

  private updateItem(item: HTMLElement, info: UploadProgressInfo): void {
    const spinner = item.querySelector(
      ".sftp-progress-spinner",
    ) as HTMLElement | null;
    const status = item.querySelector(
      ".sftp-progress-status",
    ) as HTMLElement | null;

    const isTerminal =
      info.status === "completed" ||
      info.status === "failed" ||
      info.status === "cancelled";

    // Update spinner visibility and style
    if (spinner) {
      if (isTerminal) {
        spinner.style.display = "none";
      } else {
        spinner.style.display = "";
        spinner.className =
          info.status === "preparing"
            ? "sftp-progress-spinner sftp-progress-spinner-preparing"
            : "sftp-progress-spinner sftp-progress-spinner-uploading";
      }
    }

    // Update status text
    if (status) {
      switch (info.status) {
        case "preparing":
          status.textContent = t("sftp.progress.preparing");
          status.className = "sftp-progress-status";
          break;
        case "uploading":
          status.textContent = t("sftp.progress.uploading");
          status.className = "sftp-progress-status";
          break;
        case "completed":
          status.textContent = t("sftp.progress.completed");
          status.className = "sftp-progress-status sftp-progress-status-success";
          break;
        case "failed":
          status.textContent = info.errorMessage || t("sftp.progress.failed");
          status.className = "sftp-progress-status sftp-progress-status-error";
          break;
        case "cancelled":
          status.textContent = t("sftp.progress.cancelled");
          status.className = "sftp-progress-status";
          break;
      }
    }

    // Hide cancel button for terminal states
    if (isTerminal) {
      const cancelBtn = item.querySelector(
        ".sftp-progress-cancel-btn",
      ) as HTMLElement | null;
      if (cancelBtn) cancelBtn.style.display = "none";
    }
  }

  private cleanupContainerIfEmpty(): void {
    if (this.items.size === 0 && this.container) {
      this.container.remove();
      this.container = null;
    }
  }
}
