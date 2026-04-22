/**
 * Rename Window Dialog
 *
 * Modal dialog for renaming a mux window. Reuses the `sftp-dialog-*`
 * styling so the appearance is consistent with other custom dialogs
 * across platforms (Linux/Windows WebView differences).
 */

import { t } from "../../i18n/index.ts";

export interface RenameWindowDialogOptions {
  currentName: string;
}

export interface RenameWindowDialogResult {
  confirmed: boolean;
  name: string;
}

export function showRenameWindowDialog(
  options: RenameWindowDialogOptions,
): Promise<RenameWindowDialogResult> {
  return new Promise((resolve) => {
    const previouslyFocused = document.activeElement as HTMLElement | null;

    const overlay = document.createElement("div");
    overlay.className = "sftp-dialog-overlay";

    const dialog = document.createElement("div");
    dialog.className = "sftp-dialog";
    dialog.setAttribute("role", "dialog");
    dialog.setAttribute("aria-modal", "true");

    const title = document.createElement("h3");
    title.className = "sftp-dialog-title";
    title.textContent = t("mux.renameDialog.title");
    dialog.appendChild(title);

    const label = document.createElement("label");
    label.className = "sftp-dialog-label";
    label.textContent = t("mux.renameDialog.label");
    dialog.appendChild(label);

    const input = document.createElement("input");
    input.type = "text";
    input.className = "sftp-dialog-input";
    input.value = options.currentName;
    input.maxLength = 256;
    dialog.appendChild(input);

    const buttonRow = document.createElement("div");
    buttonRow.className = "sftp-dialog-buttons";

    const cancelBtn = document.createElement("button");
    cancelBtn.className = "sftp-dialog-btn sftp-dialog-btn-cancel";
    cancelBtn.textContent = t("mux.renameDialog.cancel");
    cancelBtn.addEventListener("click", () => {
      cleanup();
      resolve({ confirmed: false, name: "" });
    });

    const confirmBtn = document.createElement("button");
    confirmBtn.className = "sftp-dialog-btn sftp-dialog-btn-confirm";
    confirmBtn.textContent = t("mux.renameDialog.confirm");
    confirmBtn.addEventListener("click", () => {
      const name = input.value.trim();
      cleanup();
      resolve({ confirmed: true, name });
    });

    buttonRow.appendChild(cancelBtn);
    buttonRow.appendChild(confirmBtn);
    dialog.appendChild(buttonRow);

    overlay.appendChild(dialog);
    document.body.appendChild(overlay);

    input.focus();
    input.select();

    const handleKeyDown = (e: KeyboardEvent) => {
      e.stopPropagation();
      // IME composition: let the input commit the composed text without
      // closing the dialog on the commit-Enter keydown.
      if (e.isComposing || e.keyCode === 229) return;
      if (e.key === "Escape") {
        cleanup();
        resolve({ confirmed: false, name: "" });
      } else if (e.key === "Enter") {
        const name = input.value.trim();
        cleanup();
        resolve({ confirmed: true, name });
      }
    };
    overlay.addEventListener("keydown", handleKeyDown, true);

    function cleanup() {
      overlay.remove();
      previouslyFocused?.focus();
    }
  });
}
