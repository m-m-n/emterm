/**
 * Settings Module
 *
 * Provides settings panel functionality for eMterm.
 */

export { SettingsPanel } from "./settings-panel";
export type { SettingsPanelOptions } from "./settings-panel";
export { SettingsService } from "./settings-service";
export { applySettings, applySettingsToCSS, applyTerminalColorScheme } from "./settings-applier";
export type { AppSettings, KeybindSettings, UiTheme, CursorStyle, BellAction, ScrollbarMode } from "./types";
export {
  MIN_FONT_SIZE, MAX_FONT_SIZE,
  MIN_PADDING, MAX_PADDING,
  MIN_SCROLLBACK_LINES, MAX_SCROLLBACK_LINES,
  MIN_SCROLL_SPEED, MAX_SCROLL_SPEED,
} from "./types";
