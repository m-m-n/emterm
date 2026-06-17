/**
 * Font Service
 *
 * Provides system font enumeration with frontend-side caching.
 * Invokes the Rust `list_fonts` Tauri command on first call,
 * then returns cached data on subsequent calls.
 */

import { invoke } from "@tauri-apps/api/core";
import type { FontListResponse } from "./types";

export class FontService {
  private static cachedFonts: FontListResponse | null = null;

  static async list(): Promise<FontListResponse> {
    if (FontService.cachedFonts) {
      return FontService.cachedFonts;
    }
    const fonts = await invoke<FontListResponse>("list_fonts");
    FontService.cachedFonts = fonts;
    return fonts;
  }

  /** Reset cache (for testing) */
  static resetCache(): void {
    FontService.cachedFonts = null;
  }
}
