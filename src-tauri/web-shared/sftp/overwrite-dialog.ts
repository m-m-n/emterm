/**
 * Overwrite Confirmation Dialog
 *
 * Modal dialog shown when uploaded files already exist on the remote host.
 * Displays the list of conflicting file names with bulk approve/cancel.
 */

import { t } from "../i18n/index.ts";

/**
 * Options for the overwrite dialog.
 */
export interface OverwriteDialogOptions {
  /** File names that already exist on the remote host */
  conflictingFiles: string[];
}

/**
 * Result from the overwrite dialog.
 */
export interface OverwriteDialogResult {
  /** Whether the user approved overwriting */
  approved: boolean;
}

/**
 * Show the overwrite confirmation dialog.
 */
export function showOverwriteDialog(
  options: OverwriteDialogOptions,
): Promise<OverwriteDialogResult> {
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
    title.textContent = t("sftp.overwriteDialog.title");
    dialog.appendChild(title);

    // Message
    const message = document.createElement("p");
    message.className = "sftp-dialog-label";
    message.textContent = t("sftp.overwriteDialog.message", {
      count: options.conflictingFiles.length,
    });
    dialog.appendChild(message);

    // Conflicting file list
    const fileList = document.createElement("ul");
    fileList.className = "sftp-dialog-file-list";
    for (const fileName of options.conflictingFiles) {
      const li = document.createElement("li");
      li.className = "sftp-dialog-file-item sftp-dialog-file-conflict";
      li.textContent = fileName;
      fileList.appendChild(li);
    }
    dialog.appendChild(fileList);

    // Button row
    const buttonRow = document.createElement("div");
    buttonRow.className = "sftp-dialog-buttons";

    const cancelBtn = document.createElement("button");
    cancelBtn.className = "sftp-dialog-btn sftp-dialog-btn-cancel";
    cancelBtn.textContent = t("sftp.overwriteDialog.cancel");
    cancelBtn.addEventListener("click", () => {
      cleanup();
      resolve({ approved: false });
    });

    const approveBtn = document.createElement("button");
    approveBtn.className = "sftp-dialog-btn sftp-dialog-btn-danger";
    approveBtn.textContent = t("sftp.overwriteDialog.overwrite");
    approveBtn.addEventListener("click", () => {
      cleanup();
      resolve({ approved: true });
    });

    buttonRow.appendChild(cancelBtn);
    buttonRow.appendChild(approveBtn);
    dialog.appendChild(buttonRow);

    overlay.appendChild(dialog);
    document.body.appendChild(overlay);

    // Focus cancel button (safer default)
    cancelBtn.focus();

    // Handle Escape key
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        cleanup();
        resolve({ approved: false });
      }
    };
    overlay.addEventListener("keydown", handleKeyDown);

    function cleanup() {
      overlay.remove();
      previouslyFocused?.focus();
    }
  });
}
