/**
 * Terminal application configuration and constants
 */

/**
 * Default terminal dimensions
 */
export const DEFAULT_COLS = 80;
export const DEFAULT_ROWS = 24;

/**
 * Default font settings
 */
export const DEFAULT_FONT_FAMILY = 'monospace';
export const DEFAULT_FONT_SIZE = 16;

/**
 * IME configuration
 */
export const IME_INPUT_ID = 'ime-input';
export const IME_COMPOSITION_CLASS = 'ime-composition';
export const IME_DEBUG = false; // Set to true for IME debug logging

/**
 * CSS class names
 */
export const CSS_CLASSES = {
  TERMINAL_CONTAINER: 'terminal-container',
  IME_INPUT: 'ime-input',
  IME_COMPOSITION: 'ime-composition',
} as const;
