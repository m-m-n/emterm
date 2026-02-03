/**
 * Color Scheme Editor
 *
 * CRUD operations and utilities for user-defined terminal color schemes.
 * Phase 2: Logic layer (no DOM).
 * Phase 4: Will add UI rendering functions.
 */

import type { UserColorScheme } from "./types";
import {
  COLOR_SCHEME_PRESETS,
  getColorSchemePreset,
  rgbToHex,
  type ColorSchemePreset,
} from "../terminal/colors";

// ============================================================
// Types
// ============================================================

/** Select box option for color scheme dropdown */
export interface ColorSchemeSelectOption {
  value: string;
  label: string;
  isUser: boolean;
}

/** Result of rename operation */
export interface RenameResult {
  success: boolean;
  schemes?: UserColorScheme[];
  error?: string;
}

/** Color key for special colors */
export type SpecialColorKey = "foreground" | "background" | "cursor" | "selection";

/** Color key including ANSI colors */
export type ColorKey = SpecialColorKey | `ansi_${number}`;

// ============================================================
// Naming Utilities
// ============================================================

/**
 * Generate a unique copy name for a scheme.
 * Format: {baseName}_copy_N where N is the lowest available integer.
 *
 * @param baseName - The base name to copy from
 * @param existingNames - List of existing scheme names
 * @returns A unique name in format {baseName}_copy_N
 */
export function generateCopyName(baseName: string, existingNames: string[]): string {
  let n = 1;
  while (existingNames.includes(`${baseName}_copy_${n}`)) {
    n++;
  }
  return `${baseName}_copy_${n}`;
}

// ============================================================
// Scheme Lookup
// ============================================================

/**
 * Check if a scheme name belongs to a user-defined scheme.
 *
 * @param name - Scheme name to check
 * @param userSchemes - Array of user schemes
 * @returns True if the name matches a user scheme
 */
export function isUserScheme(name: string, userSchemes: UserColorScheme[]): boolean {
  return userSchemes.some((s) => s.name === name);
}

/**
 * Check if a name is a preset name.
 *
 * @param name - Name to check
 * @returns True if the name matches a preset
 */
export function isPresetName(name: string): boolean {
  return COLOR_SCHEME_PRESETS.some((p) => p.name === name);
}

// ============================================================
// CRUD Operations
// ============================================================

/**
 * Convert a ColorSchemePreset to UserColorScheme format.
 * Converts Rgb values to hex strings.
 */
function presetToUserScheme(preset: ColorSchemePreset, name: string): UserColorScheme {
  return {
    name,
    foreground: rgbToHex(preset.foreground),
    background: rgbToHex(preset.background),
    cursor: rgbToHex(preset.cursor),
    selection: rgbToHex(preset.selection),
    ansi_colors: preset.ansiColors.map((rgb) => rgbToHex(rgb)),
  };
}

/**
 * Create a new user scheme from a preset.
 * The new scheme gets an auto-generated name: {presetName}_copy_N.
 *
 * @param presetName - Name of the preset to copy
 * @param userSchemes - Existing user schemes (for name generation)
 * @returns New UserColorScheme or null if preset not found
 */
export function createUserSchemeFromPreset(
  presetName: string,
  userSchemes: UserColorScheme[]
): UserColorScheme | null {
  const preset = getColorSchemePreset(presetName);
  if (!preset) {
    return null;
  }

  const existingNames = userSchemes.map((s) => s.name);
  const newName = generateCopyName(presetName, existingNames);

  return presetToUserScheme(preset, newName);
}

/**
 * Update a specific color in a user scheme.
 * Returns a new scheme object (immutable update).
 *
 * @param scheme - The scheme to update
 * @param colorKey - Which color to update (e.g., "foreground", "ansi_0")
 * @param newValue - New hex color value
 * @returns Updated scheme
 */
export function updateUserSchemeColor(
  scheme: UserColorScheme,
  colorKey: ColorKey,
  newValue: string
): UserColorScheme {
  // Handle ANSI colors
  if (colorKey.startsWith("ansi_")) {
    const index = parseInt(colorKey.slice(5), 10);
    if (index >= 0 && index < 16) {
      const newAnsiColors = [...scheme.ansi_colors];
      newAnsiColors[index] = newValue;
      return { ...scheme, ansi_colors: newAnsiColors };
    }
    return scheme;
  }

  // Handle special colors
  const key = colorKey as SpecialColorKey;
  return { ...scheme, [key]: newValue };
}

/**
 * Delete a user scheme from the array.
 *
 * @param schemes - Array of user schemes
 * @param name - Name of scheme to delete
 * @returns New array without the deleted scheme
 */
export function deleteUserScheme(
  schemes: UserColorScheme[],
  name: string
): UserColorScheme[] {
  return schemes.filter((s) => s.name !== name);
}

/**
 * Duplicate a scheme (preset or user) as a new user scheme.
 *
 * @param sourceName - Name of the scheme to duplicate
 * @param userSchemes - Existing user schemes
 * @returns New UserColorScheme or null if source not found
 */
export function duplicateScheme(
  sourceName: string,
  userSchemes: UserColorScheme[]
): UserColorScheme | null {
  // Check if it's a user scheme first
  const userScheme = userSchemes.find((s) => s.name === sourceName);
  if (userScheme) {
    const existingNames = userSchemes.map((s) => s.name);
    const newName = generateCopyName(sourceName, existingNames);
    return { ...userScheme, name: newName };
  }

  // Try as preset
  return createUserSchemeFromPreset(sourceName, userSchemes);
}

/**
 * Rename a user scheme.
 * Validates that:
 * - New name is not empty
 * - New name is not a duplicate of another user scheme
 * - New name is not a preset name
 *
 * @param schemes - Array of user schemes
 * @param oldName - Current name of the scheme
 * @param newName - New name to set
 * @returns RenameResult with success status and updated schemes or error
 */
export function renameUserScheme(
  schemes: UserColorScheme[],
  oldName: string,
  newName: string
): RenameResult {
  const trimmedName = newName.trim();

  // Validate: not empty
  if (!trimmedName) {
    return { success: false, error: "Name cannot be empty" };
  }

  // Allow same name (no-op)
  if (trimmedName === oldName) {
    return { success: true, schemes };
  }

  // Validate: not a preset name
  if (isPresetName(trimmedName)) {
    return { success: false, error: "Name conflicts with a preset" };
  }

  // Validate: not a duplicate user scheme name
  if (schemes.some((s) => s.name === trimmedName && s.name !== oldName)) {
    return { success: false, error: "Name already exists" };
  }

  // Find and update the scheme
  const index = schemes.findIndex((s) => s.name === oldName);
  if (index === -1) {
    return { success: false, error: "Scheme not found" };
  }

  const newSchemes = [...schemes];
  const existingScheme = schemes[index];
  // existingScheme is guaranteed to exist since index !== -1
  newSchemes[index] = {
    name: trimmedName,
    foreground: existingScheme!.foreground,
    background: existingScheme!.background,
    cursor: existingScheme!.cursor,
    selection: existingScheme!.selection,
    ansi_colors: existingScheme!.ansi_colors,
  };

  return { success: true, schemes: newSchemes };
}

// ============================================================
// Select Box Options
// ============================================================

/**
 * Build select options for the color scheme dropdown.
 * Presets are listed first (in fixed order), then user schemes.
 *
 * @param userSchemes - Array of user schemes
 * @returns Array of select options
 */
export function buildSelectOptions(userSchemes: UserColorScheme[]): ColorSchemeSelectOption[] {
  const options: ColorSchemeSelectOption[] = [];

  // Add presets first (fixed order from COLOR_SCHEME_PRESETS)
  for (const preset of COLOR_SCHEME_PRESETS) {
    options.push({
      value: preset.name,
      label: formatPresetLabel(preset.name),
      isUser: false,
    });
  }

  // Add user schemes
  for (const scheme of userSchemes) {
    options.push({
      value: scheme.name,
      label: `${scheme.name} [User]`,
      isUser: true,
    });
  }

  return options;
}

/**
 * Format a preset name as a display label.
 * Capitalizes and formats the name nicely.
 */
function formatPresetLabel(name: string): string {
  // Special cases
  const labelMap: Record<string, string> = {
    emterm: "eMterm",
    "solarized-dark": "Solarized Dark",
    "solarized-light": "Solarized Light",
    monokai: "Monokai",
    dracula: "Dracula",
    nord: "Nord",
  };
  return labelMap[name] || name;
}

// ============================================================
// Get Current Scheme Colors (for palette display)
// ============================================================

/**
 * Get all colors from a scheme (preset or user) as hex strings.
 */
export function getSchemeColors(schemeName: string, userSchemes: UserColorScheme[]): UserColorScheme | null {
  // Check user schemes first
  const userScheme = userSchemes.find((s) => s.name === schemeName);
  if (userScheme) {
    return userScheme;
  }

  // Fall back to preset
  const preset = getColorSchemePreset(schemeName);
  if (preset) {
    return presetToUserScheme(preset, schemeName);
  }

  // Default to emterm
  const emterm = getColorSchemePreset("emterm");
  if (emterm) {
    return presetToUserScheme(emterm, "emterm");
  }

  return null;
}
