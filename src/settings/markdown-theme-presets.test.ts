import { describe, expect, test } from "bun:test";
import {
  MARKDOWN_THEME_PRESETS,
  MARKDOWN_COLOR_TO_CSS_VAR,
  type MarkdownThemeColors,
} from "./markdown-theme-presets";

// ============================================================
// Helpers
// ============================================================

const PRESETS = ["purple", "blue", "green", "orange"] as const;
const MODES = ["dark", "light"] as const;

const REQUIRED_KEYS: (keyof MarkdownThemeColors)[] = [
  "bg",
  "fg",
  "heading",
  "link",
  "border",
  "blockquote",
  "codeBg",
  "codeFg",
  "preBg",
  "tableBg",
  "tableStripe",
];

/** Matches #RGB, #RRGGBB, #RRGGBBAA, or rgba(...) */
const CSS_COLOR_PATTERN = /^(#[0-9a-fA-F]{3,8}|rgba?\(\s*\d+\s*,\s*\d+\s*,\s*\d+\s*(,\s*[\d.]+\s*)?\))$/;

// ============================================================
// Tests
// ============================================================

describe("MARKDOWN_THEME_PRESETS", () => {
  test("has all 4 presets", () => {
    for (const preset of PRESETS) {
      expect(MARKDOWN_THEME_PRESETS[preset]).toBeDefined();
    }
  });

  test("each preset has both dark and light variants", () => {
    for (const preset of PRESETS) {
      expect(MARKDOWN_THEME_PRESETS[preset].dark).toBeDefined();
      expect(MARKDOWN_THEME_PRESETS[preset].light).toBeDefined();
    }
  });

  test("each variant has all 11 required color properties", () => {
    for (const preset of PRESETS) {
      for (const mode of MODES) {
        const palette = MARKDOWN_THEME_PRESETS[preset][mode];
        for (const key of REQUIRED_KEYS) {
          expect(palette[key]).toBeDefined();
          expect(typeof palette[key]).toBe("string");
          expect(palette[key].length).toBeGreaterThan(0);
        }
      }
    }
  });

  test("all color values are valid CSS color strings", () => {
    for (const preset of PRESETS) {
      for (const mode of MODES) {
        const palette = MARKDOWN_THEME_PRESETS[preset][mode];
        for (const key of REQUIRED_KEYS) {
          const value = palette[key];
          expect(value).toMatch(CSS_COLOR_PATTERN);
        }
      }
    }
  });

  test("dark variants have dark backgrounds", () => {
    for (const preset of PRESETS) {
      const bg = MARKDOWN_THEME_PRESETS[preset].dark.bg;
      // Dark backgrounds should have low RGB values
      const r = Number.parseInt(bg.slice(1, 3), 16);
      const g = Number.parseInt(bg.slice(3, 5), 16);
      const b = Number.parseInt(bg.slice(5, 7), 16);
      const brightness = (r + g + b) / 3;
      expect(brightness).toBeLessThan(80);
    }
  });

  test("light variants have light backgrounds", () => {
    for (const preset of PRESETS) {
      const bg = MARKDOWN_THEME_PRESETS[preset].light.bg;
      const r = Number.parseInt(bg.slice(1, 3), 16);
      const g = Number.parseInt(bg.slice(3, 5), 16);
      const b = Number.parseInt(bg.slice(5, 7), 16);
      const brightness = (r + g + b) / 3;
      expect(brightness).toBeGreaterThan(200);
    }
  });
});

describe("MARKDOWN_COLOR_TO_CSS_VAR", () => {
  test("maps all 11 color properties to CSS variables", () => {
    for (const key of REQUIRED_KEYS) {
      expect(MARKDOWN_COLOR_TO_CSS_VAR[key]).toBeDefined();
      expect(typeof MARKDOWN_COLOR_TO_CSS_VAR[key]).toBe("string");
    }
  });

  test("all CSS variable names use --markdown- prefix", () => {
    for (const key of REQUIRED_KEYS) {
      expect(MARKDOWN_COLOR_TO_CSS_VAR[key]).toMatch(/^--markdown-/);
    }
  });

  test("has exactly 11 entries", () => {
    expect(Object.keys(MARKDOWN_COLOR_TO_CSS_VAR).length).toBe(11);
  });
});
