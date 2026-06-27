/**
 * UI Theme Presets
 *
 * Defines Material Design 3 color tokens for each theme preset (Purple, Blue, Green, Orange).
 * Each preset has both dark and light variants.
 */

import type { UiThemePreset } from "./types";

// ============================================================
// Types
// ============================================================

export interface ThemeColors {
  primary: string;
  onPrimary: string;
  primaryContainer: string;
  onPrimaryContainer: string;
  secondary: string;
  onSecondary: string;
  secondaryContainer: string;
  onSecondaryContainer: string;
  surface: string;
  surfaceContainer: string;
  surfaceContainerLow: string;
  surfaceContainerHigh: string;
  surfaceContainerHighest: string;
  onSurface: string;
  onSurfaceVariant: string;
  surfaceVariant: string;
  outline: string;
  outlineVariant: string;
  error: string;
  onError: string;
  errorContainer: string;
  onErrorContainer: string;
}

export interface PresetDefinition {
  dark: ThemeColors;
  light: ThemeColors;
}

// ============================================================
// Preset Definitions
// ============================================================

export const UI_THEME_PRESETS: Record<UiThemePreset, PresetDefinition> = {
  purple: {
    dark: {
      primary: "#D0BCFF",
      onPrimary: "#381E72",
      primaryContainer: "#4F378B",
      onPrimaryContainer: "#EADDFF",
      secondary: "#CCC2DC",
      onSecondary: "#332D41",
      secondaryContainer: "#4A4458",
      onSecondaryContainer: "#E8DEF8",
      surface: "#141218",
      surfaceContainer: "#211F26",
      surfaceContainerLow: "#1D1B20",
      surfaceContainerHigh: "#2B2930",
      surfaceContainerHighest: "#36343B",
      onSurface: "#E6E0E9",
      onSurfaceVariant: "#CAC4D0",
      surfaceVariant: "#49454F",
      outline: "#938F99",
      outlineVariant: "#49454F",
      error: "#F2B8B5",
      onError: "#601410",
      errorContainer: "#8C1D18",
      onErrorContainer: "#F9DEDC",
    },
    light: {
      primary: "#6750A4",
      onPrimary: "#FFFFFF",
      primaryContainer: "#EADDFF",
      onPrimaryContainer: "#21005D",
      secondary: "#625B71",
      onSecondary: "#FFFFFF",
      secondaryContainer: "#E8DEF8",
      onSecondaryContainer: "#1D192B",
      surface: "#FEF7FF",
      surfaceContainer: "#F3EDF7",
      surfaceContainerLow: "#F7F2FA",
      surfaceContainerHigh: "#ECE6F0",
      surfaceContainerHighest: "#E6E0E9",
      onSurface: "#1D1B20",
      onSurfaceVariant: "#49454F",
      surfaceVariant: "#E7E0EC",
      outline: "#79747E",
      outlineVariant: "#CAC4D0",
      error: "#B3261E",
      onError: "#FFFFFF",
      errorContainer: "#F9DEDC",
      onErrorContainer: "#410E0B",
    },
  },
  blue: {
    dark: {
      primary: "#A8C7FA",
      onPrimary: "#062E6F",
      primaryContainer: "#0842A0",
      onPrimaryContainer: "#D3E3FD",
      secondary: "#C2C6DC",
      onSecondary: "#2C3041",
      secondaryContainer: "#434659",
      onSecondaryContainer: "#DEE2F9",
      surface: "#111318",
      surfaceContainer: "#1F2126",
      surfaceContainerLow: "#1A1C20",
      surfaceContainerHigh: "#292B30",
      surfaceContainerHighest: "#34363B",
      onSurface: "#E2E2E9",
      onSurfaceVariant: "#C4C6D0",
      surfaceVariant: "#44464F",
      outline: "#8E909A",
      outlineVariant: "#44464F",
      error: "#F2B8B5",
      onError: "#601410",
      errorContainer: "#8C1D18",
      onErrorContainer: "#F9DEDC",
    },
    light: {
      primary: "#0B57D0",
      onPrimary: "#FFFFFF",
      primaryContainer: "#D3E3FD",
      onPrimaryContainer: "#041E49",
      secondary: "#5A5E71",
      onSecondary: "#FFFFFF",
      secondaryContainer: "#DEE2F9",
      onSecondaryContainer: "#171B2C",
      surface: "#F9F9FF",
      surfaceContainer: "#EFF0F6",
      surfaceContainerLow: "#F3F3FA",
      surfaceContainerHigh: "#E8E9EF",
      surfaceContainerHighest: "#E2E2E9",
      onSurface: "#1A1C20",
      onSurfaceVariant: "#44464F",
      surfaceVariant: "#E1E2EC",
      outline: "#75767F",
      outlineVariant: "#C4C6D0",
      error: "#B3261E",
      onError: "#FFFFFF",
      errorContainer: "#F9DEDC",
      onErrorContainer: "#410E0B",
    },
  },
  green: {
    dark: {
      primary: "#7DD3A8",
      onPrimary: "#003823",
      primaryContainer: "#005234",
      onPrimaryContainer: "#98F0C3",
      secondary: "#B4CCB8",
      onSecondary: "#213528",
      secondaryContainer: "#374B3E",
      onSecondaryContainer: "#D0E8D4",
      surface: "#101412",
      surfaceContainer: "#1C201E",
      surfaceContainerLow: "#181C1A",
      surfaceContainerHigh: "#262B28",
      surfaceContainerHighest: "#313633",
      onSurface: "#DEE4DF",
      onSurfaceVariant: "#BFC9C1",
      surfaceVariant: "#404943",
      outline: "#8A938C",
      outlineVariant: "#404943",
      error: "#F2B8B5",
      onError: "#601410",
      errorContainer: "#8C1D18",
      onErrorContainer: "#F9DEDC",
    },
    light: {
      primary: "#006D3E",
      onPrimary: "#FFFFFF",
      primaryContainer: "#98F0C3",
      onPrimaryContainer: "#002110",
      secondary: "#4E6354",
      onSecondary: "#FFFFFF",
      secondaryContainer: "#D0E8D4",
      onSecondaryContainer: "#0B1F13",
      surface: "#F5FBF5",
      surfaceContainer: "#EBF1EB",
      surfaceContainerLow: "#EFF5EF",
      surfaceContainerHigh: "#E5EBE5",
      surfaceContainerHighest: "#DEE4DF",
      onSurface: "#181C1A",
      onSurfaceVariant: "#404943",
      surfaceVariant: "#DBE5DD",
      outline: "#717972",
      outlineVariant: "#BFC9C1",
      error: "#B3261E",
      onError: "#FFFFFF",
      errorContainer: "#F9DEDC",
      onErrorContainer: "#410E0B",
    },
  },
  orange: {
    dark: {
      primary: "#FFB877",
      onPrimary: "#4C2700",
      primaryContainer: "#6C3A00",
      onPrimaryContainer: "#FFDCBE",
      secondary: "#DDC2A1",
      onSecondary: "#3E2D16",
      secondaryContainer: "#56432B",
      onSecondaryContainer: "#FADEBB",
      surface: "#18120B",
      surfaceContainer: "#261F18",
      surfaceContainerLow: "#211A13",
      surfaceContainerHigh: "#302922",
      surfaceContainerHighest: "#3B342D",
      onSurface: "#EFE0CF",
      onSurfaceVariant: "#D4C4B1",
      surfaceVariant: "#524436",
      outline: "#9D8E7D",
      outlineVariant: "#524436",
      error: "#F2B8B5",
      onError: "#601410",
      errorContainer: "#8C1D18",
      onErrorContainer: "#F9DEDC",
    },
    light: {
      primary: "#8B5000",
      onPrimary: "#FFFFFF",
      primaryContainer: "#FFDCBE",
      onPrimaryContainer: "#2D1600",
      secondary: "#6F5B40",
      onSecondary: "#FFFFFF",
      secondaryContainer: "#FADEBB",
      onSecondaryContainer: "#271904",
      surface: "#FFF8F4",
      surfaceContainer: "#F5EDEA",
      surfaceContainerLow: "#FAF2EE",
      surfaceContainerHigh: "#EEE6E3",
      surfaceContainerHighest: "#E9E1DD",
      onSurface: "#211A13",
      onSurfaceVariant: "#524436",
      surfaceVariant: "#F0E0CD",
      outline: "#847465",
      outlineVariant: "#D4C4B1",
      error: "#B3261E",
      onError: "#FFFFFF",
      errorContainer: "#F9DEDC",
      onErrorContainer: "#410E0B",
    },
  },
  pink: {
    dark: {
      primary: "#FFB1C8",
      onPrimary: "#5E1133",
      primaryContainer: "#7B2949",
      onPrimaryContainer: "#FFD9E3",
      secondary: "#E3BDC6",
      onSecondary: "#422931",
      secondaryContainer: "#5B3F47",
      onSecondaryContainer: "#FFD9E2",
      surface: "#1A1114",
      surfaceContainer: "#271D21",
      surfaceContainerLow: "#221820",
      surfaceContainerHigh: "#322830",
      surfaceContainerHighest: "#3D333A",
      onSurface: "#F0DEE2",
      onSurfaceVariant: "#D4BFC5",
      surfaceVariant: "#514349",
      outline: "#9D8A90",
      outlineVariant: "#514349",
      error: "#F2B8B5",
      onError: "#601410",
      errorContainer: "#8C1D18",
      onErrorContainer: "#F9DEDC",
    },
    light: {
      primary: "#984061",
      onPrimary: "#FFFFFF",
      primaryContainer: "#FFD9E3",
      onPrimaryContainer: "#3E001D",
      secondary: "#74565F",
      onSecondary: "#FFFFFF",
      secondaryContainer: "#FFD9E2",
      onSecondaryContainer: "#2B151C",
      surface: "#FFF8F8",
      surfaceContainer: "#FAECEF",
      surfaceContainerLow: "#FDF0F2",
      surfaceContainerHigh: "#F2E4E8",
      surfaceContainerHighest: "#EBDEE2",
      onSurface: "#22191C",
      onSurfaceVariant: "#514349",
      surfaceVariant: "#F0DBE1",
      outline: "#837379",
      outlineVariant: "#D4BFC5",
      error: "#B3261E",
      onError: "#FFFFFF",
      errorContainer: "#F9DEDC",
      onErrorContainer: "#410E0B",
    },
  },
};

// ============================================================
// CSS Variable Application
// ============================================================

/** Maps ThemeColors property names to CSS variable names */
const COLOR_TO_CSS_VAR: Record<keyof ThemeColors, string> = {
  primary: "--md-sys-color-primary",
  onPrimary: "--md-sys-color-on-primary",
  primaryContainer: "--md-sys-color-primary-container",
  onPrimaryContainer: "--md-sys-color-on-primary-container",
  secondary: "--md-sys-color-secondary",
  onSecondary: "--md-sys-color-on-secondary",
  secondaryContainer: "--md-sys-color-secondary-container",
  onSecondaryContainer: "--md-sys-color-on-secondary-container",
  surface: "--md-sys-color-surface",
  surfaceContainer: "--md-sys-color-surface-container",
  surfaceContainerLow: "--md-sys-color-surface-container-low",
  surfaceContainerHigh: "--md-sys-color-surface-container-high",
  surfaceContainerHighest: "--md-sys-color-surface-container-highest",
  onSurface: "--md-sys-color-on-surface",
  onSurfaceVariant: "--md-sys-color-on-surface-variant",
  surfaceVariant: "--md-sys-color-surface-variant",
  outline: "--md-sys-color-outline",
  outlineVariant: "--md-sys-color-outline-variant",
  error: "--md-sys-color-error",
  onError: "--md-sys-color-on-error",
  errorContainer: "--md-sys-color-error-container",
  onErrorContainer: "--md-sys-color-on-error-container",
};

/**
 * Apply preset colors as CSS variables on :root.
 * Sets all 20 MD3 color tokens.
 */
export function applyPresetColors(colors: ThemeColors): void {
  const root = document.documentElement;
  for (const [key, cssVar] of Object.entries(COLOR_TO_CSS_VAR)) {
    root.style.setProperty(cssVar, colors[key as keyof ThemeColors]);
  }
}
