/**
 * Clipboard module - System clipboard operations.
 */

export { ClipboardManager } from "./manager";
export type {
	PasteDialogOptions,
	PasteDialogResult,
} from "./dialog";
export { showPasteDialog } from "./dialog";
export { sendTextInChunks } from "./paste";
