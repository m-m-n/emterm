/**
 * Paste confirmation dialog component.
 *
 * Shows a dialog to confirm pasting multi-line text into the terminal.
 */

/**
 * Options for the paste confirmation dialog.
 */
export interface PasteDialogOptions {
	/** Text to be pasted */
	text: string;
	/** Number of lines in the text */
	lineCount: number;
}

/**
 * Result returned from the paste confirmation dialog.
 */
export interface PasteDialogResult {
	/** True if user confirmed the paste, false if cancelled */
	confirmed: boolean;
}

/**
 * Show a confirmation dialog for pasting multi-line content.
 *
 * @param options - Dialog options including text and line count
 * @returns Promise resolving to dialog result
 *
 * @example
 * ```ts
 * const result = await showPasteDialog({
 *   text: "Line 1\nLine 2\nLine 3",
 *   lineCount: 3
 * });
 *
 * if (result.confirmed) {
 *   // Proceed with paste
 * }
 * ```
 */
export function showPasteDialog(
	options: PasteDialogOptions,
): Promise<PasteDialogResult> {
	return new Promise((resolve) => {
		// Create dialog overlay
		const overlay = document.createElement("div");
		overlay.className = "paste-dialog-overlay";
		overlay.style.cssText = `
      position: fixed;
      top: 0;
      left: 0;
      right: 0;
      bottom: 0;
      background: rgba(0, 0, 0, 0.5);
      display: flex;
      align-items: center;
      justify-content: center;
      z-index: 10000;
    `;

		// Create dialog container
		const dialog = document.createElement("div");
		dialog.className = "paste-dialog";
		dialog.style.cssText = `
      background: #2d2d2d;
      color: #d4d4d4;
      border: 1px solid #555;
      border-radius: 6px;
      padding: 24px;
      min-width: 400px;
      max-width: 600px;
      box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif;
    `;

		// Create dialog title
		const title = document.createElement("h3");
		title.textContent = "Confirm Paste";
		title.style.cssText = `
      margin: 0 0 16px 0;
      font-size: 18px;
      font-weight: 600;
      color: #ffffff;
    `;

		// Create dialog message
		const message = document.createElement("p");
		message.textContent = `You are about to paste ${options.lineCount} line${options.lineCount === 1 ? "" : "s"} of text into the terminal.`;
		message.style.cssText = `
      margin: 0 0 20px 0;
      font-size: 14px;
      line-height: 1.5;
      color: #d4d4d4;
    `;

		// Create preview (first 5 lines)
		const preview = document.createElement("pre");
		const lines = options.text.split(/\r\n|\r|\n/);
		const previewLines = lines.slice(0, 5);
		const previewText = previewLines.join("\n");
		const moreLines =
			lines.length > 5
				? `\n... and ${lines.length - 5} more line${lines.length - 5 === 1 ? "" : "s"}`
				: "";
		preview.textContent = previewText + moreLines;
		preview.style.cssText = `
      background: #1e1e1e;
      color: #d4d4d4;
      padding: 12px;
      border-radius: 4px;
      font-size: 13px;
      font-family: "Inconsolata", "Noto Sans JP", "Noto Color Emoji", monospace;
      overflow-x: auto;
      max-height: 200px;
      margin: 0 0 20px 0;
      border: 1px solid #444;
    `;

		// Create button container
		const buttonContainer = document.createElement("div");
		buttonContainer.style.cssText = `
      display: flex;
      justify-content: flex-end;
      gap: 12px;
    `;

		// Create Cancel button
		const cancelButton = document.createElement("button");
		cancelButton.textContent = "Cancel";
		cancelButton.style.cssText = `
      padding: 8px 16px;
      background: #3a3a3a;
      color: #d4d4d4;
      border: 1px solid #555;
      border-radius: 4px;
      font-size: 14px;
      cursor: pointer;
      transition: background 0.2s;
    `;
		cancelButton.onmouseenter = () => {
			cancelButton.style.background = "#4a4a4a";
		};
		cancelButton.onmouseleave = () => {
			cancelButton.style.background = "#3a3a3a";
		};

		// Create Paste button
		const pasteButton = document.createElement("button");
		pasteButton.textContent = "Paste";
		pasteButton.style.cssText = `
      padding: 8px 16px;
      background: #0e639c;
      color: #ffffff;
      border: 1px solid #0e639c;
      border-radius: 4px;
      font-size: 14px;
      cursor: pointer;
      transition: background 0.2s;
    `;
		pasteButton.onmouseenter = () => {
			pasteButton.style.background = "#1177bb";
		};
		pasteButton.onmouseleave = () => {
			pasteButton.style.background = "#0e639c";
		};

		// Close dialog function
		const closeDialog = (confirmed: boolean) => {
			// Clean up event listener before removing dialog (must match options)
			document.removeEventListener("keydown", handleKeyDown, { capture: true });
			document.body.removeChild(overlay);
			resolve({ confirmed });
		};

		// Escape key handler
		const handleKeyDown = (e: KeyboardEvent) => {
			if (e.key === "Escape") {
				e.preventDefault();
				e.stopPropagation();
				closeDialog(false);
			} else if (e.key === "Enter") {
				e.preventDefault();
				e.stopPropagation();
				closeDialog(true);
			}
		};
		// Use capture phase to prevent key events from reaching other handlers
		document.addEventListener("keydown", handleKeyDown, { capture: true });

		// Button event handlers
		cancelButton.onclick = () => closeDialog(false);
		pasteButton.onclick = () => closeDialog(true);

		// Overlay click handler (click outside to cancel)
		overlay.onclick = (e) => {
			if (e.target === overlay) {
				closeDialog(false);
			}
		};

		// Prevent dialog clicks from closing
		dialog.onclick = (e) => {
			e.stopPropagation();
		};

		// Assemble dialog
		buttonContainer.appendChild(cancelButton);
		buttonContainer.appendChild(pasteButton);
		dialog.appendChild(title);
		dialog.appendChild(message);
		dialog.appendChild(preview);
		dialog.appendChild(buttonContainer);
		overlay.appendChild(dialog);

		// Show dialog
		document.body.appendChild(overlay);

		// Focus paste button
		pasteButton.focus();
	});
}
