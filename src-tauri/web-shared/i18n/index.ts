/**
 * Internationalization (i18n) Module
 *
 * Lightweight i18n implementation for the eMterm frontend.
 * Uses JSON translation files with dot-separated key access and {param} placeholder replacement.
 */

import enTranslations from "./locales/en.json";
import jaTranslations from "./locales/ja.json";

// ============================================================
// Types
// ============================================================

type TranslationData = Record<string, unknown>;

// ============================================================
// Constants
// ============================================================

/**
 * Supported locale codes.
 */
export const SUPPORTED_LOCALES = ["en", "ja"] as const;

/**
 * Fallback locale when a key is not found in the current locale.
 */
const FALLBACK_LOCALE = "en";

// ============================================================
// Module State
// ============================================================

/**
 * Map of locale code to translation data.
 */
const translations: Record<string, TranslationData> = {
  en: enTranslations as TranslationData,
  ja: jaTranslations as TranslationData,
};

/**
 * Current active locale.
 */
let currentLocale = "en";

// ============================================================
// Public API
// ============================================================

/**
 * Initializes the i18n module with the given locale.
 * Loads the translation files and sets the active locale.
 *
 * @param locale - Language code ("en", "ja")
 */
export function initI18n(locale: string): void {
  currentLocale = locale;
}

/**
 * Returns the translated string for the given key.
 *
 * Lookup order:
 * 1. Current locale translations
 * 2. English (fallback) translations
 * 3. Return the key string as-is
 *
 * @param key - Dot-separated key (e.g., "settings.appearance.fontSize")
 * @param params - Optional parameter map for placeholder replacement
 * @returns Translated string, or the key itself if not found
 */
export function t(key: string, params?: Record<string, string | number>): string {
  // Try current locale
  let value = resolveKey(translations[currentLocale], key);

  // Fallback to English
  if (value === undefined && currentLocale !== FALLBACK_LOCALE) {
    value = resolveKey(translations[FALLBACK_LOCALE], key);
  }

  // Return key as-is if not found
  if (value === undefined) {
    return key;
  }

  // Replace {paramName} placeholders
  if (params) {
    return replacePlaceholders(value, params);
  }

  return value;
}

/**
 * Changes the active locale at runtime.
 *
 * @param locale - Language code ("en", "ja")
 */
export function setLocale(locale: string): void {
  currentLocale = locale;
}

/**
 * Returns the current active locale code.
 *
 * @returns Language code (e.g., "en", "ja")
 */
export function getLocale(): string {
  return currentLocale;
}

/**
 * Resolves "auto" to a concrete locale code using navigator.language.
 * Returns "en" if the detected language is not supported.
 *
 * @param locale - "auto" or a specific language code
 * @returns Resolved language code
 */
export function resolveLocale(locale: string): string {
  if (locale !== "auto") {
    return locale;
  }

  // Read navigator.language (e.g., "ja-JP", "en-US")
  const browserLang =
    typeof navigator !== "undefined" && navigator.language
      ? navigator.language
      : "en";

  // Extract base tag: "ja-JP" -> "ja"
  const baseTag = browserLang.split("-")[0] ?? "en";

  // Check if supported
  if ((SUPPORTED_LOCALES as readonly string[]).includes(baseTag)) {
    return baseTag;
  }

  return "en";
}

// ============================================================
// Internal Helpers
// ============================================================

/**
 * Resolves a dot-separated key in a nested translation object.
 *
 * @param data - Translation data object
 * @param key - Dot-separated key (e.g., "settings.appearance.fontSize")
 * @returns The string value, or undefined if not found
 */
function resolveKey(data: TranslationData | undefined, key: string): string | undefined {
  if (!data) return undefined;

  const parts = key.split(".");
  let current: unknown = data;

  for (const part of parts) {
    if (current === null || current === undefined || typeof current !== "object") {
      return undefined;
    }
    current = (current as Record<string, unknown>)[part];
  }

  if (typeof current === "string") {
    return current;
  }

  return undefined;
}

/**
 * Replaces {paramName} placeholders in a string with parameter values.
 *
 * @param template - String containing {paramName} placeholders
 * @param params - Parameter map
 * @returns String with placeholders replaced
 */
function replacePlaceholders(
  template: string,
  params: Record<string, string | number>,
): string {
  return template.replace(/\{(\w+)\}/g, (match, paramName: string) => {
    const value = params[paramName];
    if (value !== undefined) {
      return String(value);
    }
    return match;
  });
}
