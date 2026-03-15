/**
 * Resizable panel drag handle.
 *
 * Creates a vertical drag handle for resizing a panel in a flexbox layout.
 * Supports min/max constraints and localStorage persistence.
 *
 * @module ui/resize-handle
 */

export interface ResizeHandleOptions {
  /** Minimum width in pixels */
  minWidth: number;
  /** Maximum width in pixels (defaults to 50% of viewport) */
  maxWidth?: number;
  /** localStorage key for persisting width */
  storageKey?: string;
}

/**
 * Create a vertical drag handle for resizing a panel.
 *
 * @param panel - The panel element whose width will be adjusted
 * @param options - Configuration for min/max width and persistence
 * @returns The handle element to insert between panels
 */
export function createResizeHandle(
  panel: HTMLElement,
  options: ResizeHandleOptions,
): HTMLElement {
  const handle = document.createElement("div");
  handle.className = "resize-handle";

  // Restore saved width
  if (options.storageKey) {
    try {
      const saved = localStorage.getItem(options.storageKey);
      if (saved) {
        const w = Number.parseInt(saved, 10);
        if (!isNaN(w) && w >= options.minWidth) {
          panel.style.width = `${w}px`;
        }
      }
    } catch {
      // localStorage may not be available
    }
  }

  let startX = 0;
  let startWidth = 0;

  const onMouseMove = (e: MouseEvent) => {
    const delta = e.clientX - startX;
    let newWidth = startWidth + delta;
    newWidth = Math.max(options.minWidth, newWidth);
    const maxW = options.maxWidth || window.innerWidth * 0.5;
    newWidth = Math.min(maxW, newWidth);
    panel.style.width = `${newWidth}px`;
  };

  const onMouseUp = () => {
    document.removeEventListener("mousemove", onMouseMove);
    document.removeEventListener("mouseup", onMouseUp);
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
    handle.classList.remove("resize-handle-active");

    if (options.storageKey) {
      try {
        localStorage.setItem(
          options.storageKey,
          panel.style.width.replace("px", ""),
        );
      } catch {
        // localStorage may not be available
      }
    }
  };

  handle.addEventListener("mousedown", (e: MouseEvent) => {
    e.preventDefault();
    startX = e.clientX;
    startWidth = panel.getBoundingClientRect().width;
    document.addEventListener("mousemove", onMouseMove);
    document.addEventListener("mouseup", onMouseUp);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    handle.classList.add("resize-handle-active");
  });

  return handle;
}
