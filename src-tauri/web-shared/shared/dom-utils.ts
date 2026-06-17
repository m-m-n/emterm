/**
 * Shared DOM utility functions.
 *
 * @module shared/dom-utils
 */

/**
 * Checks if any ancestor element has display:none.
 * Used to detect when a tab container is hidden during tab switch,
 * where the element may have an "active" class but is not actually visible.
 *
 * @param element - Element to check
 * @returns True if any ancestor has display:none
 */
export function isAncestorHidden(element: HTMLElement): boolean {
	let current: HTMLElement | null = element.parentElement;
	while (current) {
		if (current.style.display === "none") {
			return true;
		}
		current = current.parentElement;
	}
	return false;
}

/**
 * Checks if a modal overlay (image viewer or markdown fullscreen) is currently visible.
 * Takes into account multi-tab scenarios where the overlay may have the "visible" class
 * but be in a hidden tab (ancestor has display:none).
 *
 * @returns True if a modal overlay is visible in the active tab
 */
export function isModalOverlayVisible(): boolean {
	const imageOverlay = document.querySelector(
		".image-viewer-overlay.visible",
	) as HTMLElement | null;
	if (imageOverlay && !isAncestorHidden(imageOverlay)) return true;

	const markdownOverlay = document.querySelector(
		".markdown-fullscreen-overlay.visible",
	) as HTMLElement | null;
	if (markdownOverlay && !isAncestorHidden(markdownOverlay)) return true;

	const dataViewerOverlay = document.querySelector(
		".dv-fullscreen-overlay",
	) as HTMLElement | null;
	if (dataViewerOverlay && !isAncestorHidden(dataViewerOverlay)) return true;

	return false;
}
