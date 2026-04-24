/**
 * Move Window Dialog
 *
 * Modal dialog asking the user for a 1-origin target position to reorder
 * the currently active mux window. Reuses the `sftp-dialog-*` styling so
 * the appearance stays consistent with other custom dialogs across
 * Linux/Windows WebView differences.
 *
 * Contract:
 * - Input validation:
 *     empty / non-integer / `< 1` / `> windowCount`  => `{ confirmed: false }`
 *     integer in `[1, windowCount]`                  => `{ confirmed: true, value }`
 * - "Equal to current index" detection is intentionally out of scope here.
 *   The caller (mux-action-handler) already knows `activeIndex` and decides
 *   whether to send the IPC message, so the dialog stays reusable.
 */

import { t } from "../../i18n/index.ts";

export interface MoveWindowDialogOptions {
  /** 1-origin current index. Currently informational only. */
  currentIndex: number;
  /** Inclusive upper bound for valid input (lower bound is always 1). */
  windowCount: number;
}

export interface MoveWindowDialogResult {
  confirmed: boolean;
  /** 1-origin target position. Undefined when not confirmed. */
  value?: number;
}

export function showMoveWindowDialog(
  options: MoveWindowDialogOptions,
): Promise<MoveWindowDialogResult> {
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
    title.textContent = t("mux.moveDialog.title");
    dialog.appendChild(title);

    const label = document.createElement("label");
    label.className = "sftp-dialog-label";
    label.textContent = t("mux.moveDialog.label");
    dialog.appendChild(label);

    const input = document.createElement("input");
    input.type = "text";
    input.inputMode = "numeric";
    input.setAttribute("pattern", "[0-9]*");
    input.className = "sftp-dialog-input";
    input.value = "";
    input.maxLength = 4;
    dialog.appendChild(input);

    const buttonRow = document.createElement("div");
    buttonRow.className = "sftp-dialog-buttons";

    const cancelBtn = document.createElement("button");
    cancelBtn.className = "sftp-dialog-btn sftp-dialog-btn-cancel";
    cancelBtn.textContent = t("mux.moveDialog.cancel");
    cancelBtn.addEventListener("click", () => {
      cleanup();
      resolve({ confirmed: false });
    });

    const confirmBtn = document.createElement("button");
    confirmBtn.className = "sftp-dialog-btn sftp-dialog-btn-confirm";
    confirmBtn.textContent = t("mux.moveDialog.confirm");
    confirmBtn.addEventListener("click", () => {
      const parsed = parseInput(input.value, options.windowCount);
      cleanup();
      if (parsed === null) {
        resolve({ confirmed: false });
      } else {
        resolve({ confirmed: true, value: parsed });
      }
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
        resolve({ confirmed: false });
      } else if (e.key === "Enter") {
        const parsed = parseInput(input.value, options.windowCount);
        cleanup();
        if (parsed === null) {
          resolve({ confirmed: false });
        } else {
          resolve({ confirmed: true, value: parsed });
        }
      }
    };
    overlay.addEventListener("keydown", handleKeyDown, true);

    function cleanup() {
      overlay.remove();
      previouslyFocused?.focus();
    }
  });
}

/**
 * Parse and validate the dialog input.
 * Returns the 1-origin integer on success, or `null` when the input is
 * empty, non-integer, or out of `[1, windowCount]`.
 */
function parseInput(raw: string, windowCount: number): number | null {
  const trimmed = raw.trim();
  if (trimmed === "") return null;
  // Reject non-integer characters (including floats, signs, whitespace in
  // the middle). `/^\d+$/` keeps the check simple and matches `pattern`.
  if (!/^\d+$/.test(trimmed)) return null;
  const n = Number.parseInt(trimmed, 10);
  if (!Number.isFinite(n)) return null;
  if (n < 1 || n > windowCount) return null;
  return n;
}
