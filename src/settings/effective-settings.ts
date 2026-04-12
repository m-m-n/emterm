/**
 * Effective-settings accessors.
 *
 * Some settings have a fixed policy value on specific platforms that cannot
 * be changed via `settings.json`. This module centralizes the overrides so
 * call sites don't scatter platform checks through the code, and so the
 * policy is unit-testable in isolation.
 *
 * Linux policy (FR5 of `linux-primary-selection`):
 * - `copy_on_select` is always `false` (PRIMARY selection handles this natively)
 * - `middle_click_paste` is always `true` (native Linux UX)
 *
 * Windows: the raw `settings.json` value is used, preserving existing behavior.
 */
import { isLinux } from "../platform";
import type { AppSettings } from "./types";

/**
 * Return the effective value of `copy_on_select` for the current platform.
 *
 * Linux: always `false` (PRIMARY selection is the native equivalent).
 * Other: the raw value from settings, defaulting to `false`.
 */
export function effectiveCopyOnSelect(
  settings: AppSettings | null | undefined,
): boolean {
  if (isLinux()) return false;
  return settings?.copy_on_select ?? false;
}

/**
 * Return the effective value of `middle_click_paste` for the current platform.
 *
 * Linux: always `true` (native Linux middle-click paste behavior).
 * Other: the raw value from settings, defaulting to `true` to preserve the
 *        existing Windows default where middle-click paste is enabled unless
 *        explicitly disabled.
 */
export function effectiveMiddleClickPaste(
  settings: AppSettings | null | undefined,
): boolean {
  if (isLinux()) return true;
  return settings?.middle_click_paste !== false;
}
