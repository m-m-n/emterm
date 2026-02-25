/**
 * Tests for UI Theme Presets
 */

import { describe, test, expect, beforeEach, afterEach } from "bun:test";
import { UI_THEME_PRESETS, applyPresetColors } from "./ui-theme-presets";
import type { ThemeColors } from "./ui-theme-presets";
import type { UiThemePreset } from "./types";

// ============================================================
// Preset Data Tests
// ============================================================

describe("UI_THEME_PRESETS", () => {
  test("should contain 5 presets", () => {
    const presetKeys = Object.keys(UI_THEME_PRESETS);
    expect(presetKeys).toHaveLength(5);
    expect(presetKeys).toEqual(["purple", "blue", "green", "orange", "pink"]);
  });

  test("each preset should have dark and light variants", () => {
    const presets: UiThemePreset[] = ["purple", "blue", "green", "orange", "pink"];
    for (const name of presets) {
      const preset = UI_THEME_PRESETS[name];
      expect(preset.dark).toBeDefined();
      expect(preset.light).toBeDefined();
    }
  });

  test("each color definition should have all 19 MD3 tokens", () => {
    const expectedKeys: (keyof ThemeColors)[] = [
      "primary", "onPrimary",
      "primaryContainer", "onPrimaryContainer",
      "secondary", "onSecondary",
      "secondaryContainer", "onSecondaryContainer",
      "surface", "surfaceContainer", "surfaceContainerLow",
      "surfaceContainerHigh", "surfaceContainerHighest",
      "onSurface", "onSurfaceVariant",
      "outline", "outlineVariant",
      "error", "onError",
    ];

    const presets: UiThemePreset[] = ["purple", "blue", "green", "orange", "pink"];
    for (const name of presets) {
      const preset = UI_THEME_PRESETS[name];
      for (const variant of [preset.dark, preset.light]) {
        for (const key of expectedKeys) {
          expect(variant[key]).toBeDefined();
          expect(typeof variant[key]).toBe("string");
          // All values should be hex colors
          expect(variant[key]).toMatch(/^#[0-9A-Fa-f]{6}$/);
        }
      }
    }
  });

  test("purple dark should match known values", () => {
    const pd = UI_THEME_PRESETS.purple.dark;
    expect(pd.primary).toBe("#D0BCFF");
    expect(pd.surface).toBe("#141218");
  });

  test("purple light should match known values", () => {
    const pl = UI_THEME_PRESETS.purple.light;
    expect(pl.primary).toBe("#6750A4");
    expect(pl.surface).toBe("#FEF7FF");
  });
});

// ============================================================
// applyPresetColors Tests
// ============================================================

describe("applyPresetColors", () => {
  const mockStyle = {
    setProperty: (name: string, value: string) => {
      mockStyle.properties[name] = value;
    },
    removeProperty: (name: string) => {
      delete mockStyle.properties[name];
    },
    properties: {} as Record<string, string>,
  };

  const savedDocument = globalThis.document;

  beforeEach(() => {
    mockStyle.properties = {};
    // @ts-expect-error - Mock for testing
    globalThis.document = {
      documentElement: {
        style: mockStyle,
        setAttribute: () => {},
        getAttribute: () => null,
        removeAttribute: () => {},
      },
    };
  });

  afterEach(() => {
    globalThis.document = savedDocument;
  });

  test("should set all 19 CSS variables", () => {
    applyPresetColors(UI_THEME_PRESETS.purple.dark);
    expect(Object.keys(mockStyle.properties)).toHaveLength(19);
  });

  test("should set correct CSS variable names", () => {
    applyPresetColors(UI_THEME_PRESETS.blue.dark);
    expect(mockStyle.properties["--md-sys-color-primary"]).toBe("#A8C7FA");
    expect(mockStyle.properties["--md-sys-color-on-primary"]).toBe("#062E6F");
    expect(mockStyle.properties["--md-sys-color-surface"]).toBe("#111318");
    expect(mockStyle.properties["--md-sys-color-on-surface"]).toBe("#E2E2E9");
    expect(mockStyle.properties["--md-sys-color-error"]).toBe("#F2B8B5");
  });

  test("should overwrite previously set values", () => {
    applyPresetColors(UI_THEME_PRESETS.purple.dark);
    expect(mockStyle.properties["--md-sys-color-primary"]).toBe("#D0BCFF");

    applyPresetColors(UI_THEME_PRESETS.green.light);
    expect(mockStyle.properties["--md-sys-color-primary"]).toBe("#006D3E");
  });
});
