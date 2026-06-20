/**
 * Settings Service
 *
 * Service layer for loading and saving settings via Tauri commands.
 */

import { invoke } from "@tauri-apps/api/core";
import type { AppSettings, MuxActionDefault } from "./types";

/**
 * SettingsService - Encapsulates Tauri invoke calls for settings.
 *
 * All methods are static for simplicity.
 */
export class SettingsService {
  /** Cached settings from the last load/save operation */
  private static cachedSettings: AppSettings | null = null;

  /**
   * Loads settings from the backend.
   *
   * Returns AppSettings with valid font_size (never null).
   * The backend always returns fully populated settings with defaults applied.
   *
   * @throws Error if loading fails (should not happen in normal operation)
   */
  static async load(): Promise<AppSettings> {
    const settings = await invoke<AppSettings>("load_settings");
    SettingsService.cachedSettings = structuredClone(settings);
    return settings;
  }

  /**
   * Saves settings to the backend.
   *
   * @param settings The settings to save
   * @throws Error if save fails (e.g., invalid font_size range)
   */
  static async save(settings: AppSettings): Promise<void> {
    await invoke("save_settings", { settings });
    SettingsService.cachedSettings = structuredClone(settings);
  }

  /**
   * Returns the cached settings from the last load/save.
   * Returns null if settings have not been loaded yet.
   */
  static getCached(): AppSettings | null {
    return SettingsService.cachedSettings;
  }

  /**
   * Loads the default mux action bindings from the backend SSOT
   * (`crate::mux::prefix::DEFAULT_ACTION_BINDINGS`). The settings panel uses
   * these to enumerate the mux actions and show each action's default chord
   * when the user has not customized it — instead of duplicating the table
   * in TypeScript. Returned in the backend's declaration (display) order.
   */
  static async loadMuxActionDefaults(): Promise<MuxActionDefault[]> {
    return await invoke<MuxActionDefault[]>("get_mux_action_defaults");
  }
}
