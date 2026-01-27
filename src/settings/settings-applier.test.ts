/**
 * Tests for Settings Applier
 */

import { describe, test, expect, beforeEach } from "bun:test";
import { applySettingsToCSS } from "./settings-applier";
import type { AppSettings } from "./types";

// Mock document.documentElement
const mockStyle = {
  setProperty: (name: string, value: string) => {
    mockStyle.properties[name] = value;
  },
  properties: {} as Record<string, string>,
};

// Setup mock before each test
beforeEach(() => {
  mockStyle.properties = {};
  // @ts-expect-error - Mock for testing
  globalThis.document = {
    documentElement: {
      style: mockStyle,
    },
  };
});

describe("applySettingsToCSS", () => {
  test("should update --terminal-font-size CSS variable", () => {
    const settings: AppSettings = { font_size: 16 };

    applySettingsToCSS(settings);

    expect(mockStyle.properties["--terminal-font-size"]).toBe("16pt");
  });

  test("should update --terminal-line-height CSS variable", () => {
    const settings: AppSettings = { font_size: 16 };

    applySettingsToCSS(settings);

    expect(mockStyle.properties["--terminal-line-height"]).toBe("18pt");
  });

  test("should calculate line height as font_size + 2", () => {
    const settings: AppSettings = { font_size: 20 };

    applySettingsToCSS(settings);

    expect(mockStyle.properties["--terminal-font-size"]).toBe("20pt");
    expect(mockStyle.properties["--terminal-line-height"]).toBe("22pt");
  });

  test("should handle minimum font size", () => {
    const settings: AppSettings = { font_size: 8 };

    applySettingsToCSS(settings);

    expect(mockStyle.properties["--terminal-font-size"]).toBe("8pt");
    expect(mockStyle.properties["--terminal-line-height"]).toBe("10pt");
  });

  test("should handle maximum font size", () => {
    const settings: AppSettings = { font_size: 32 };

    applySettingsToCSS(settings);

    expect(mockStyle.properties["--terminal-font-size"]).toBe("32pt");
    expect(mockStyle.properties["--terminal-line-height"]).toBe("34pt");
  });
});
