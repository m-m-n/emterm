/**
 * Apply cached settings to a freshly-created renderer/state pair.
 *
 * Run once during `TerminalApp.init()` immediately after `createRendererAsync`.
 * The cached settings are normally applied to all renderers via the
 * tab-manager broadcast, but the broadcast happens before this tab's
 * renderer exists, so its notifications are dropped — we replay them here
 * to bring the new renderer up to the user's current preferences before the
 * first frame is drawn.
 *
 * Extracted from TerminalApp to keep `init()` readable. No behavioural
 * change versus the inline version it replaces.
 */

import type { TerminalState } from "../terminal/state";
import type { ITerminalRenderer } from "../terminal";
import { SettingsService } from "../settings/settings-service";
import { buildFontFamilyChain } from "../settings/settings-applier";

/**
 * Reads the current cached settings (if any) and applies the visual /
 * fold-related subset to the given renderer + state. No-op if no settings
 * have been cached yet.
 */
export function applyInitialCachedSettings(
  state: TerminalState,
  renderer: ITerminalRenderer,
): void {
  const cachedSettings = SettingsService.getCached();
  if (!cachedSettings) return;

  if (cachedSettings.terminal_color_scheme) {
    // Check if it's a user-defined color scheme
    const userScheme = cachedSettings.custom_color_schemes?.find(
      (s) => s.name === cachedSettings.terminal_color_scheme,
    );
    if (userScheme) {
      // Apply user-defined color scheme directly
      renderer.setUserColorScheme(userScheme);
    } else {
      // Apply preset color scheme
      renderer.applySetting("colorScheme", cachedSettings.terminal_color_scheme);
    }
  }
  if (cachedSettings.cursor_style) {
    renderer.applySetting("cursorStyle", cachedSettings.cursor_style);
  }
  if (cachedSettings.cursor_blink !== undefined) {
    renderer.applySetting("cursorBlink", cachedSettings.cursor_blink);
  }
  if (cachedSettings.fold_enabled !== undefined) {
    state.getFoldManager().setEnabled(cachedSettings.fold_enabled);
  }
  if (cachedSettings.bold_brightens_ansi_colors !== undefined) {
    renderer.applySetting(
      "boldBrightensAnsiColors",
      cachedSettings.bold_brightens_ansi_colors,
    );
  }
  const fontChain = buildFontFamilyChain(
    cachedSettings.font_family_primary || "",
    cachedSettings.font_family_emoji || "",
    cachedSettings.font_family_secondary || "",
  );
  if (fontChain) {
    renderer.applySetting("fontFamily", fontChain);
  }
}
