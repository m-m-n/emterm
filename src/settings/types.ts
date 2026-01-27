/**
 * Settings Types
 *
 * TypeScript type definitions matching Rust AppSettings struct.
 */

/**
 * Application settings structure.
 * Matches Rust AppSettings exactly for JSON serialization.
 *
 * Note: font_size is always a valid number (never null) because
 * the backend applies defaults before returning settings.
 */
export interface AppSettings {
  /** Font size in points (8-32) */
  font_size: number;
}

/**
 * Minimum allowed font size.
 * Must match MIN_FONT_SIZE in Rust.
 */
export const MIN_FONT_SIZE = 8;

/**
 * Maximum allowed font size.
 * Must match MAX_FONT_SIZE in Rust.
 */
export const MAX_FONT_SIZE = 32;
