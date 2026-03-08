/**
 * Upload Confirmation Dialog
 *
 * Modal dialog shown when files are dropped on an SSH tab.
 * Displays the file list, destination path input, and confirm/cancel buttons.
 */

import { t } from "../i18n/index.ts";
import type { FileDropInfo } from "./file-drop-handler";

/**
 * Options for the upload dialog.
 */
export interface UploadDialogOptions {
  /** Files/directories to upload */
  files: FileDropInfo[];
  /** Default destination path (from OSC 7 or empty for home) */
  defaultDestination: string;
}

/**
 * Result from the upload dialog.
 */
export interface UploadDialogResult {
  /** Whether the user confirmed the upload */
  confirmed: boolean;
  /** Destination path on the remote host */
  destination: string;
}

/**
 * Show the upload confirmation dialog.
 */
export function showUploadDialog(
  options: UploadDialogOptions,
): Promise<UploadDialogResult> {
  return new Promise((resolve) => {
    const previouslyFocused = document.activeElement as HTMLElement | null;

    // Create overlay
    const overlay = document.createElement("div");
    overlay.className = "sftp-dialog-overlay";

    // Create dialog
    const dialog = document.createElement("div");
    dialog.className = "sftp-dialog";

    // Title
    const title = document.createElement("h3");
    title.className = "sftp-dialog-title";
    title.textContent = t("sftp.uploadDialog.title");
    dialog.appendChild(title);

    // File list
    const fileListLabel = document.createElement("p");
    fileListLabel.className = "sftp-dialog-label";
    fileListLabel.textContent = t("sftp.uploadDialog.fileList", {
      count: options.files.length,
    });
    dialog.appendChild(fileListLabel);

    const fileList = document.createElement("ul");
    fileList.className = "sftp-dialog-file-list";
    for (const file of options.files) {
      const li = document.createElement("li");
      li.className = "sftp-dialog-file-item";
      const icon = file.isDirectory ? "\u{1F4C1}" : "\u{1F4C4}";
      li.textContent = `${icon} ${file.name}`;
      fileList.appendChild(li);
    }
    dialog.appendChild(fileList);

    // Destination path input
    const destLabel = document.createElement("label");
    destLabel.className = "sftp-dialog-label";
    destLabel.textContent = t("sftp.uploadDialog.destination");
    dialog.appendChild(destLabel);

    const destInput = document.createElement("input");
    destInput.type = "text";
    destInput.className = "sftp-dialog-input";
    destInput.value = options.defaultDestination;
    destInput.placeholder = t("sftp.uploadDialog.destinationPlaceholder");
    dialog.appendChild(destInput);

    // Button row
    const buttonRow = document.createElement("div");
    buttonRow.className = "sftp-dialog-buttons";

    const cancelBtn = document.createElement("button");
    cancelBtn.className = "sftp-dialog-btn sftp-dialog-btn-cancel";
    cancelBtn.textContent = t("sftp.uploadDialog.cancel");
    cancelBtn.addEventListener("click", () => {
      cleanup();
      resolve({ confirmed: false, destination: "" });
    });

    const confirmBtn = document.createElement("button");
    confirmBtn.className = "sftp-dialog-btn sftp-dialog-btn-confirm";
    confirmBtn.textContent = t("sftp.uploadDialog.confirm");
    confirmBtn.addEventListener("click", () => {
      const destination = destInput.value.trim();
      cleanup();
      resolve({ confirmed: true, destination });
    });

    buttonRow.appendChild(cancelBtn);
    buttonRow.appendChild(confirmBtn);
    dialog.appendChild(buttonRow);

    overlay.appendChild(dialog);
    document.body.appendChild(overlay);

    // Focus destination input
    destInput.focus();
    destInput.select();

    // Handle keyboard events - stop propagation to prevent terminal from consuming them
    const handleKeyDown = (e: KeyboardEvent) => {
      e.stopPropagation();
      if (e.key === "Escape") {
        cleanup();
        resolve({ confirmed: false, destination: "" });
      } else if (e.key === "Enter") {
        const destination = destInput.value.trim();
        cleanup();
        resolve({ confirmed: true, destination });
      }
    };
    overlay.addEventListener("keydown", handleKeyDown, true);

    function cleanup() {
      overlay.remove();
      previouslyFocused?.focus();
    }
  });
}
