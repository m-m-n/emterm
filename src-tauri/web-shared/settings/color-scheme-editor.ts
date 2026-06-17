/**
 * Color Scheme Editor
 *
 * CRUD operations and utilities for user-defined terminal color schemes,
 * plus the UI rendering function for the inline palette editor.
 */

import type { AppSettings, UserColorScheme } from "./types";
import {
  COLOR_SCHEME_PRESETS,
  getColorSchemePreset,
  rgbToHex,
  validateHexColor,
  type ColorSchemePreset,
} from "../terminal/colors";
import { applyTerminalColorScheme } from "./settings-applier";
import type { AddListenerFn } from "./settings-components";
import { t } from "../i18n/index.ts";
import { createMd3Select } from "../components/md3-select";

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

// ============================================================
// UI Rendering - Context Interface
// ============================================================

/** Context required by the color scheme editor UI */
export interface ColorSchemeEditorContext {
  currentSettings: AppSettings;
  addContentListener: AddListenerFn;
  saveSetting: <K extends keyof AppSettings>(
    key: K,
    value: AppSettings[K],
  ) => void;
}

// ============================================================
// UI Rendering - Color Scheme Editor
// ============================================================

/** Debounce timer for color changes */
let colorSaveTimer: ReturnType<typeof setTimeout> | null = null;

export function renderColorSchemeEditor(
  panel: HTMLElement,
  ctx: ColorSchemeEditorContext,
): void {
  const settings = ctx.currentSettings;
  const currentScheme = settings.terminal_color_scheme || "emterm";

  // Container
  const container = document.createElement("div");
  container.className = "settings-row";
  container.dataset.key = "terminal-color-scheme-editor";
  panel.appendChild(container);

  // Label
  const label = document.createElement("label");
  label.className = "settings-label";
  label.textContent = t("settings.appearance.colorScheme");
  container.appendChild(label);

  // Description
  const desc = document.createElement("span");
  desc.className = "settings-description";
  desc.textContent = t("settings.appearance.colorSchemeDesc");
  container.appendChild(desc);

  // Control area
  const controlDiv = document.createElement("div");
  controlDiv.className = "settings-control color-scheme-editor";
  container.appendChild(controlDiv);

  // Select box (MD3 custom dropdown)
  const selectOptions = buildSelectOptions(settings.custom_color_schemes);
  const md3Select = createMd3Select({
    id: "settings-terminal-color-scheme",
    options: selectOptions,
    value: currentScheme,
    onChange: () => handleSelectChange(),
  });
  controlDiv.appendChild(md3Select.element);

  // Proxy object for compatibility with existing code
  const select = {
    get value() { return md3Select.getValue(); },
    set value(v: string) { md3Select.setValue(v); },
  };

  // Action buttons container
  const actionsDiv = document.createElement("div");
  actionsDiv.className = "color-scheme-actions";
  controlDiv.appendChild(actionsDiv);

  // Palette container
  const paletteDiv = document.createElement("div");
  paletteDiv.className = "color-palette-editor";
  controlDiv.appendChild(paletteDiv);

  // Render functions
  const renderPalette = () => {
    paletteDiv.innerHTML = "";
    const schemeColors = getSchemeColors(
      select.value,
      settings.custom_color_schemes,
    );
    if (!schemeColors) return;

    // Special colors section (8-column grid, 4 items occupy first 4 columns)
    const specialDiv = document.createElement("div");
    specialDiv.className = "color-palette-grid";
    paletteDiv.appendChild(specialDiv);

    renderColorInput(
      specialDiv,
      t("settings.appearance.colorSchemeForeground"),
      schemeColors.foreground,
      "foreground",
    );
    renderColorInput(
      specialDiv,
      t("settings.appearance.colorSchemeBackground"),
      schemeColors.background,
      "background",
    );
    renderColorInput(
      specialDiv,
      t("settings.appearance.colorSchemeCursor"),
      schemeColors.cursor,
      "cursor",
    );
    renderColorInput(
      specialDiv,
      t("settings.appearance.colorSchemeSelection"),
      schemeColors.selection,
      "selection",
    );

    // ANSI colors section
    const ansiLabel = document.createElement("div");
    ansiLabel.className = "color-palette-label";
    ansiLabel.textContent = t("settings.appearance.colorSchemeStandardColors");
    paletteDiv.appendChild(ansiLabel);

    const standardGrid = document.createElement("div");
    standardGrid.className = "color-palette-grid";
    paletteDiv.appendChild(standardGrid);
    for (let i = 0; i < 8; i++) {
      const color = schemeColors.ansi_colors[i] || "#000000";
      renderColorInput(
        standardGrid,
        String(i),
        color,
        `ansi_${i}` as ColorKey,
      );
    }

    const brightLabel = document.createElement("div");
    brightLabel.className = "color-palette-label";
    brightLabel.textContent = t("settings.appearance.colorSchemeBrightColors");
    paletteDiv.appendChild(brightLabel);

    const brightGrid = document.createElement("div");
    brightGrid.className = "color-palette-grid";
    paletteDiv.appendChild(brightGrid);
    for (let i = 8; i < 16; i++) {
      const color = schemeColors.ansi_colors[i] || "#000000";
      renderColorInput(
        brightGrid,
        String(i),
        color,
        `ansi_${i}` as ColorKey,
      );
    }
  };

  const renderActions = () => {
    actionsDiv.innerHTML = "";
    const isUser = isUserScheme(select.value, settings.custom_color_schemes);

    // Duplicate button (always visible)
    const dupBtn = document.createElement("button");
    dupBtn.className = "settings-button settings-button-secondary";
    dupBtn.textContent = t("settings.appearance.colorSchemeDuplicate");
    dupBtn.onclick = () => handleDuplicate();
    actionsDiv.appendChild(dupBtn);

    if (isUser) {
      // Delete button (user schemes only)
      const delBtn = document.createElement("button");
      delBtn.className = "settings-button settings-button-danger";
      delBtn.textContent = t("settings.appearance.colorSchemeDelete");
      delBtn.onclick = () => handleDelete();
      actionsDiv.appendChild(delBtn);

      // Rename field (user schemes only)
      const renameDiv = document.createElement("div");
      renameDiv.className = "color-scheme-rename";
      actionsDiv.appendChild(renameDiv);

      const renameLabel = document.createElement("span");
      renameLabel.textContent =
        t("settings.appearance.colorSchemeRename") + ":";
      renameDiv.appendChild(renameLabel);

      const renameInput = document.createElement("input");
      renameInput.type = "text";
      renameInput.className = "settings-input";
      renameInput.value = select.value;
      renameDiv.appendChild(renameInput);

      renameInput.onblur = () => handleRename(renameInput.value);
      renameInput.onkeydown = (e) => {
        if (e.key === "Enter") {
          renameInput.blur();
        }
      };
    }
  };

  const renderColorInput = (
    parent: HTMLElement,
    label: string,
    value: string,
    colorKey: ColorKey,
  ) => {
    const row = document.createElement("div");
    row.className = "color-input-compact";

    const labelEl = document.createElement("span");
    labelEl.className = "color-input-label";
    labelEl.textContent = label;
    row.appendChild(labelEl);

    const inputGroup = document.createElement("div");
    inputGroup.className = "color-input-group";
    row.appendChild(inputGroup);

    const colorPicker = document.createElement("input");
    colorPicker.type = "color";
    colorPicker.className = "color-picker";
    colorPicker.value = value;
    colorPicker.title = "";
    inputGroup.appendChild(colorPicker);

    const hexInput = document.createElement("input");
    hexInput.type = "text";
    hexInput.className = "color-hex-input";
    hexInput.value = value;
    hexInput.maxLength = 7;
    inputGroup.appendChild(hexInput);

    // Store original value for change detection
    let originalValue = value.toLowerCase();

    // Use 'change' event instead of 'input' - fires only on commit (not during drag)
    colorPicker.onchange = () => {
      const newValue = colorPicker.value.toLowerCase();
      hexInput.value = newValue;
      if (newValue !== originalValue) {
        handleColorChange(colorKey, newValue);
        originalValue = newValue; // Update after successful change
      }
    };

    // Sync hex input -> color picker (on blur)
    hexInput.onblur = () => {
      const newValue = hexInput.value.toLowerCase();
      if (validateHexColor(newValue)) {
        colorPicker.value = newValue;
        if (newValue !== originalValue) {
          handleColorChange(colorKey, newValue);
          originalValue = newValue; // Update after successful change
        }
      } else {
        // Revert to picker value
        hexInput.value = colorPicker.value;
      }
    };

    parent.appendChild(row);
  };

  // Event handlers
  const handleSelectChange = () => {
    const newScheme = select.value === "emterm" ? "" : select.value;
    applyTerminalColorScheme(newScheme, settings.custom_color_schemes);
    ctx.saveSetting("terminal_color_scheme", newScheme);
    settings.terminal_color_scheme = newScheme;
    renderActions();
    renderPalette();
  };

  const handleColorChange = (colorKey: ColorKey, newValue: string) => {
    const currentSchemeName = select.value;
    const isUser = isUserScheme(
      currentSchemeName,
      settings.custom_color_schemes,
    );

    if (!isUser) {
      // Auto-copy: create user scheme from preset
      const newScheme = createUserSchemeFromPreset(
        currentSchemeName,
        settings.custom_color_schemes,
      );
      if (newScheme) {
        // Apply the color change to the new scheme
        const updated = updateUserSchemeColor(newScheme, colorKey, newValue);
        settings.custom_color_schemes = [
          ...settings.custom_color_schemes,
          updated,
        ];
        settings.terminal_color_scheme = updated.name;

        // Update select box
        refreshSelectOptions();
        select.value = updated.name;

        // Save
        debouncedSave();
        applyTerminalColorScheme(updated.name, settings.custom_color_schemes);
        renderActions();
      }
    } else {
      // Update existing user scheme
      const schemeIndex = settings.custom_color_schemes.findIndex(
        (s) => s.name === currentSchemeName,
      );
      if (schemeIndex >= 0) {
        const existingScheme = settings.custom_color_schemes[schemeIndex];
        if (existingScheme) {
          const updated = updateUserSchemeColor(
            existingScheme,
            colorKey,
            newValue,
          );
          settings.custom_color_schemes[schemeIndex] = updated;
          debouncedSave();
          applyTerminalColorScheme(
            currentSchemeName,
            settings.custom_color_schemes,
          );
        }
      }
    }
  };

  const handleDuplicate = () => {
    const newScheme = duplicateScheme(
      select.value,
      settings.custom_color_schemes,
    );
    if (newScheme) {
      settings.custom_color_schemes = [
        ...settings.custom_color_schemes,
        newScheme,
      ];
      settings.terminal_color_scheme = newScheme.name;
      refreshSelectOptions();
      select.value = newScheme.name;
      ctx.saveSetting("custom_color_schemes", settings.custom_color_schemes);
      ctx.saveSetting("terminal_color_scheme", newScheme.name);
      applyTerminalColorScheme(newScheme.name, settings.custom_color_schemes);
      renderActions();
      renderPalette();
    }
  };

  const handleDelete = () => {
    const schemeName = select.value;
    settings.custom_color_schemes = deleteUserScheme(
      settings.custom_color_schemes,
      schemeName,
    );
    settings.terminal_color_scheme = "";
    refreshSelectOptions();
    select.value = "emterm";
    ctx.saveSetting("custom_color_schemes", settings.custom_color_schemes);
    ctx.saveSetting("terminal_color_scheme", "");
    applyTerminalColorScheme("", settings.custom_color_schemes);
    renderActions();
    renderPalette();
  };

  const handleRename = (newName: string) => {
    const oldName = select.value;
    const result = renameUserScheme(
      settings.custom_color_schemes,
      oldName,
      newName,
    );
    if (result.success && result.schemes) {
      settings.custom_color_schemes = result.schemes;
      if (settings.terminal_color_scheme === oldName) {
        settings.terminal_color_scheme = newName.trim();
      }
      refreshSelectOptions();
      select.value = newName.trim();
      ctx.saveSetting("custom_color_schemes", settings.custom_color_schemes);
      ctx.saveSetting("terminal_color_scheme", settings.terminal_color_scheme);
    }
  };

  const refreshSelectOptions = () => {
    const options = buildSelectOptions(settings.custom_color_schemes);
    md3Select.updateOptions(options);
  };

  const debouncedSave = () => {
    if (colorSaveTimer) {
      clearTimeout(colorSaveTimer);
    }
    colorSaveTimer = setTimeout(() => {
      ctx.saveSetting("custom_color_schemes", settings.custom_color_schemes);
      ctx.saveSetting("terminal_color_scheme", settings.terminal_color_scheme);
    }, 300);
  };

  // Initial render
  renderActions();
  renderPalette();
}
