/**
 * Terminal size calculation utilities.
 *
 * Provides functions for calculating terminal dimensions based on
 * container size and character metrics.
 */

/**
 * Terminal size in columns and rows.
 */
export interface TerminalSize {
  /** Number of character columns */
  cols: number;
  /** Number of character rows */
  rows: number;
}

/**
 * Character dimensions in pixels.
 */
export interface CharacterSize {
  /** Width of a single character in pixels */
  width: number;
  /** Height of a single character (line height) in pixels */
  height: number;
}

/**
 * Calculates the terminal size (columns and rows) that fits within
 * a container element.
 *
 * @param container - The HTML element that contains the terminal
 * @param charWidth - Width of a single character in pixels
 * @param charHeight - Height of a single character (line height) in pixels
 * @returns The calculated terminal size
 *
 * @example
 * ```typescript
 * const container = document.getElementById('terminal');
 * const charSize = measureCharacterSize('monospace', 14);
 * const size = calculateTerminalSize(container, charSize.width, charSize.height);
 * console.log(`Terminal: ${size.cols}x${size.rows}`);
 * ```
 */
export function calculateTerminalSize(
  container: HTMLElement,
  charWidth: number,
  charHeight: number
): TerminalSize {
  const { clientWidth, clientHeight } = container;

  // Account for padding/margins
  const style = getComputedStyle(container);
  const paddingX =
    parseFloat(style.paddingLeft) + parseFloat(style.paddingRight);
  const paddingY =
    parseFloat(style.paddingTop) + parseFloat(style.paddingBottom);

  const availableWidth = clientWidth - paddingX;
  const availableHeight = clientHeight - paddingY;

  // Calculate columns and rows, ensuring at least 1 of each
  const cols = Math.max(1, Math.floor(availableWidth / charWidth));
  const rows = Math.max(1, Math.floor(availableHeight / charHeight));

  return { cols, rows };
}

/**
 * Measures the dimensions of a single character for a given font.
 *
 * Uses the canvas API to measure the width of a character and
 * calculates height based on font size with typical line height ratio.
 *
 * @param fontFamily - CSS font-family string (e.g., "monospace", "Consolas")
 * @param fontSize - Font size in pixels
 * @returns The measured character dimensions
 *
 * @example
 * ```typescript
 * const charSize = measureCharacterSize('Consolas, monospace', 14);
 * console.log(`Character size: ${charSize.width}x${charSize.height}px`);
 * ```
 */
export function measureCharacterSize(
  fontFamily: string,
  fontSize: number
): CharacterSize {
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d");

  if (!ctx) {
    // Fallback values if canvas is not available
    return {
      width: fontSize * 0.6,
      height: fontSize * 1.2,
    };
  }

  ctx.font = `${fontSize}px ${fontFamily}`;

  // Measure 'M' as a representative character for monospace fonts
  const metrics = ctx.measureText("M");

  return {
    width: metrics.width,
    // Approximate line height (typically 1.2x font size)
    height: fontSize * 1.2,
  };
}

/**
 * Creates a ResizeObserver that automatically updates terminal size
 * when the container is resized.
 *
 * @param container - The HTML element to observe
 * @param charWidth - Width of a single character in pixels
 * @param charHeight - Height of a single character in pixels
 * @param onResize - Callback function called with new dimensions
 * @returns A function to disconnect the observer
 *
 * @example
 * ```typescript
 * const disconnect = observeContainerResize(
 *   container,
 *   charSize.width,
 *   charSize.height,
 *   (cols, rows) => {
 *     ptyClient.resize(cols, rows);
 *   }
 * );
 *
 * // Later, to stop observing:
 * disconnect();
 * ```
 */
export function observeContainerResize(
  container: HTMLElement,
  charWidth: number,
  charHeight: number,
  onResize: (cols: number, rows: number) => void
): () => void {
  let lastCols = 0;
  let lastRows = 0;

  const observer = new ResizeObserver(() => {
    const { cols, rows } = calculateTerminalSize(
      container,
      charWidth,
      charHeight
    );

    // Only call callback if dimensions actually changed
    if (cols !== lastCols || rows !== lastRows) {
      lastCols = cols;
      lastRows = rows;
      onResize(cols, rows);
    }
  });

  observer.observe(container);

  // Return disconnect function
  return () => observer.disconnect();
}
