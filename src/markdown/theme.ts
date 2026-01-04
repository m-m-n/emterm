/**
 * Markdown theme management.
 *
 * Provides theme generation and application for Markdown blocks
 * to match the terminal's color scheme.
 *
 * @module markdown/theme
 */

/**
 * Markdown theme colors.
 */
export interface MarkdownTheme {
  /** Background color */
  bg: string;
  /** Foreground/text color */
  fg: string;
  /** Heading color */
  heading: string;
  /** Link color */
  link: string;
  /** Border color */
  border: string;
  /** Muted text color */
  muted: string;
  /** Inline code background */
  codeBg: string;
  /** Code block background */
  preBg: string;
  /** Code text color */
  codeFg: string;
  /** Table background (transparent by default) */
  tableBg: string;
  /** Table stripe background */
  tableStripe: string;
}

/**
 * Default dark theme.
 */
const DARK_THEME: MarkdownTheme = {
  bg: "#1e1e1e",
  fg: "#e0e0e0",
  heading: "#ffffff",
  link: "#58a6ff",
  border: "#30363d",
  muted: "#8b949e",
  codeBg: "rgba(110, 118, 129, 0.4)",
  preBg: "#161b22",
  codeFg: "#e0e0e0",
  tableBg: "transparent",
  tableStripe: "rgba(110, 118, 129, 0.1)",
};

/**
 * Default light theme.
 */
const LIGHT_THEME: MarkdownTheme = {
  bg: "#ffffff",
  fg: "#24292f",
  heading: "#1f2328",
  link: "#0969da",
  border: "#d0d7de",
  muted: "#656d76",
  codeBg: "rgba(175, 184, 193, 0.2)",
  preBg: "#f6f8fa",
  codeFg: "#1f2328",
  tableBg: "transparent",
  tableStripe: "rgba(175, 184, 193, 0.2)",
};

/**
 * Generate a Markdown theme from terminal colors.
 *
 * @param terminalBg - Terminal background color (CSS color string)
 * @param terminalFg - Terminal foreground color (CSS color string)
 * @returns Generated Markdown theme
 */
export function generateMarkdownTheme(
  terminalBg: string,
  terminalFg: string,
): MarkdownTheme {
  // Determine if dark or light mode based on background luminance
  const isDark = isColorDark(terminalBg);

  // Use default theme as base
  const baseTheme = isDark ? DARK_THEME : LIGHT_THEME;

  // Adjust colors based on terminal colors
  return {
    ...baseTheme,
    // Slightly adjust background to differentiate from terminal
    bg: adjustBrightness(terminalBg, isDark ? 0.1 : -0.05),
    fg: terminalFg,
    // Keep heading slightly brighter than foreground
    heading: isDark ? "#ffffff" : "#1f2328",
  };
}

/**
 * Apply a Markdown theme to the document.
 *
 * This sets CSS custom properties on the document root element.
 *
 * @param theme - Theme to apply
 * @param container - Optional container element (defaults to document root)
 */
export function applyMarkdownTheme(
  theme: MarkdownTheme,
  container?: HTMLElement,
): void {
  const target = container || document.documentElement;

  target.style.setProperty("--markdown-bg", theme.bg);
  target.style.setProperty("--markdown-fg", theme.fg);
  target.style.setProperty("--markdown-heading", theme.heading);
  target.style.setProperty("--markdown-link", theme.link);
  target.style.setProperty("--markdown-border", theme.border);
  target.style.setProperty("--markdown-muted", theme.muted);
  target.style.setProperty("--markdown-code-bg", theme.codeBg);
  target.style.setProperty("--markdown-pre-bg", theme.preBg);
  target.style.setProperty("--markdown-code-fg", theme.codeFg);
  target.style.setProperty("--markdown-table-bg", theme.tableBg);
  target.style.setProperty("--markdown-table-stripe", theme.tableStripe);
}

/**
 * Get the default dark theme.
 */
export function getDarkTheme(): MarkdownTheme {
  return { ...DARK_THEME };
}

/**
 * Get the default light theme.
 */
export function getLightTheme(): MarkdownTheme {
  return { ...LIGHT_THEME };
}

/**
 * Check if a color is dark based on luminance.
 *
 * @param color - CSS color string
 * @returns true if the color is dark
 */
function isColorDark(color: string): boolean {
  const rgb = parseColor(color);
  if (!rgb) return true; // Default to dark

  // Calculate relative luminance using sRGB formula
  const luminance = 0.299 * rgb.r + 0.587 * rgb.g + 0.114 * rgb.b;
  return luminance < 128;
}

/**
 * Parse a CSS color string to RGB.
 *
 * @param color - CSS color string (hex, rgb, or rgba)
 * @returns RGB values or null if parsing fails
 */
function parseColor(color: string): { r: number; g: number; b: number } | null {
  // Handle hex colors
  const hexMatch = color.match(/^#([0-9a-f]{3,8})$/i);
  if (hexMatch) {
    const hex = hexMatch[1]!;
    if (hex.length === 3) {
      return {
        r: parseInt(hex[0]! + hex[0]!, 16),
        g: parseInt(hex[1]! + hex[1]!, 16),
        b: parseInt(hex[2]! + hex[2]!, 16),
      };
    } else if (hex.length >= 6) {
      return {
        r: parseInt(hex.slice(0, 2), 16),
        g: parseInt(hex.slice(2, 4), 16),
        b: parseInt(hex.slice(4, 6), 16),
      };
    }
  }

  // Handle rgb/rgba colors
  const rgbMatch = color.match(/rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/);
  if (rgbMatch) {
    return {
      r: parseInt(rgbMatch[1]!, 10),
      g: parseInt(rgbMatch[2]!, 10),
      b: parseInt(rgbMatch[3]!, 10),
    };
  }

  return null;
}

/**
 * Adjust brightness of a color.
 *
 * @param color - CSS color string
 * @param amount - Amount to adjust (-1 to 1, positive = brighter)
 * @returns Adjusted color as hex string
 */
function adjustBrightness(color: string, amount: number): string {
  const rgb = parseColor(color);
  if (!rgb) return color;

  const adjust = (value: number): number => {
    if (amount > 0) {
      // Brighten: move toward 255
      return Math.round(value + (255 - value) * amount);
    } else {
      // Darken: move toward 0
      return Math.round(value * (1 + amount));
    }
  };

  const r = Math.min(255, Math.max(0, adjust(rgb.r)));
  const g = Math.min(255, Math.max(0, adjust(rgb.g)));
  const b = Math.min(255, Math.max(0, adjust(rgb.b)));

  return `#${r.toString(16).padStart(2, "0")}${g.toString(16).padStart(2, "0")}${b.toString(16).padStart(2, "0")}`;
}
