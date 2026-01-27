/**
 * Settings Service
 *
 * Service layer for loading and saving settings via Tauri commands.
 */

import { invoke } from "@tauri-apps/api/core";
import type { AppSettings } from "./types";

/**
 * SettingsService - Encapsulates Tauri invoke calls for settings.
 *
 * All methods are static for simplicity.
 */
export class SettingsService {
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
  }
}
