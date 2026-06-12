/**
 * Standalone settings window entry (native-poc child window).
 *
 * Reuses the WebView settings panel (`src/settings/`) unchanged inside the
 * Wry/WebKitGTK child window. The panel's Tauri `invoke()` calls are routed
 * to the Rust host through the bridge installed by `ipc-bridge.ts`; the
 * host implements `load_settings` / `save_settings` / `list_fonts` / … and
 * notifies the parent terminal process after every save so changes apply
 * live.
 *
 * @module native-poc/settings/entry
 */

import { installTauriInvokeBridge } from "./ipc-bridge.ts";

// The bridge must exist before any imported module can call `invoke()`
// (calls only happen after `boot()` below, but installing first keeps the
// ordering trivially safe).
installTauriInvokeBridge();

import { SettingsPanel } from "../../../src/settings/settings-panel.ts";
import { SettingsService } from "../../../src/settings/settings-service.ts";
import { applyUiTheme } from "../../../src/settings/settings-applier.ts";
import type { AppSettings } from "../../../src/settings/types.ts";
import { initI18n, resolveLocale } from "../../../src/i18n/index.ts";
import { initPlatform } from "../../../src/platform.ts";

import "./settings.css";

/** Apply the window-level appearance driven by the current settings. */
function applyWindowAppearance(settings: AppSettings): void {
	applyUiTheme(settings.ui_theme, settings.ui_theme_preset);
}

async function boot(): Promise<void> {
	const root = document.getElementById("settings-root");
	if (!root) {
		console.error("[ERROR][FRONTEND] settings: #settings-root missing");
		return;
	}

	// Platform + locale before the first render so `isLinux()` and `t()`
	// resolve correctly inside the panel.
	await initPlatform();
	const settings = await SettingsService.load();
	initI18n(resolveLocale(settings.language));
	applyWindowAppearance(settings);

	const panel = new SettingsPanel({ container: root });
	await panel.init();

	// The panel dispatches this after every successful save; re-resolve the
	// window's own theme (the parent terminal applies the rest on its side).
	window.addEventListener("emterm-settings-changed", () => {
		SettingsService.load()
			.then(applyWindowAppearance)
			.catch((err) => {
				console.warn("[WARN][FRONTEND] settings: theme refresh failed:", err);
			});
	});
}

// Only auto-boot in a real document (skipped under unit tests that import
// this module without a DOM).
if (typeof document !== "undefined" && document.getElementById("settings-root")) {
	void boot();
}
