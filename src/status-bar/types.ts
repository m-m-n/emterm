/**
 * Status Bar Types
 *
 * Type definitions for the status bar component.
 */

/**
 * Layer identifiers in the status bar.
 * - osc: Content injected via OSC 777;statusbar protocol (hidden when empty)
 * - app-line1: Application layer line 1 (default display)
 * - app-line2: Application layer line 2 (hidden when empty)
 */
export type StatusBarLayer = "osc" | "app-line1" | "app-line2";

/**
 * Section within a layer (left or right).
 */
export type StatusBarSection = "left" | "right";

/**
 * Configuration for status bar appearance.
 */
export interface StatusBarConfig {
  enabled: boolean;
  appLine1Left: string;
  appLine1Right: string;
  appLine2Left: string;
  appLine2Right: string;
  timeFormat: string;
  fontSize: number | null;
  bgColor: string;
  fgColor: string;
  opacity: number;
}
