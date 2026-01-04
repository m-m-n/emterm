/**
 * Tests for Markdown theme management.
 */
import { describe, test, expect, beforeEach } from "bun:test";
import {
  generateMarkdownTheme,
  applyMarkdownTheme,
  getDarkTheme,
  getLightTheme,
  type MarkdownTheme,
} from "./theme.ts";

describe("Markdown Theme", () => {
  describe("generateMarkdownTheme", () => {
    test("should generate dark theme for dark background", () => {
      const theme = generateMarkdownTheme("#1e1e1e", "#e0e0e0");
      expect(theme.heading).toBe("#ffffff");
      // Background should be slightly adjusted
      expect(theme.bg).not.toBe("#1e1e1e");
    });

    test("should generate light theme for light background", () => {
      const theme = generateMarkdownTheme("#ffffff", "#24292f");
      expect(theme.heading).toBe("#1f2328");
    });

    test("should use terminal foreground color", () => {
      const theme = generateMarkdownTheme("#1e1e1e", "#00ff00");
      expect(theme.fg).toBe("#00ff00");
    });

    test("should handle hex shorthand colors", () => {
      const theme = generateMarkdownTheme("#000", "#fff");
      expect(theme.fg).toBe("#fff");
    });

    test("should handle rgb colors", () => {
      const theme = generateMarkdownTheme(
        "rgb(30, 30, 30)",
        "rgb(224, 224, 224)",
      );
      expect(theme.fg).toBe("rgb(224, 224, 224)");
    });

    test("should handle rgba colors", () => {
      const theme = generateMarkdownTheme(
        "rgba(30, 30, 30, 1)",
        "rgba(224, 224, 224, 1)",
      );
      expect(theme.fg).toBe("rgba(224, 224, 224, 1)");
    });
  });

  describe("getDarkTheme", () => {
    test("should return dark theme defaults", () => {
      const theme = getDarkTheme();
      expect(theme.bg).toBe("#1e1e1e");
      expect(theme.fg).toBe("#e0e0e0");
      expect(theme.heading).toBe("#ffffff");
      expect(theme.link).toBe("#58a6ff");
    });

    test("should return a copy, not the original", () => {
      const theme1 = getDarkTheme();
      const theme2 = getDarkTheme();
      theme1.bg = "#000000";
      expect(theme2.bg).toBe("#1e1e1e");
    });
  });

  describe("getLightTheme", () => {
    test("should return light theme defaults", () => {
      const theme = getLightTheme();
      expect(theme.bg).toBe("#ffffff");
      expect(theme.fg).toBe("#24292f");
      expect(theme.heading).toBe("#1f2328");
      expect(theme.link).toBe("#0969da");
    });

    test("should return a copy, not the original", () => {
      const theme1 = getLightTheme();
      const theme2 = getLightTheme();
      theme1.bg = "#ffffff";
      expect(theme2.bg).toBe("#ffffff");
    });
  });

  describe("applyMarkdownTheme", () => {
    test("should set CSS custom properties on document root", () => {
      const theme = getDarkTheme();
      applyMarkdownTheme(theme);

      const root = document.documentElement;
      expect(root.style.getPropertyValue("--markdown-bg")).toBe("#1e1e1e");
      expect(root.style.getPropertyValue("--markdown-fg")).toBe("#e0e0e0");
      expect(root.style.getPropertyValue("--markdown-heading")).toBe("#ffffff");
      expect(root.style.getPropertyValue("--markdown-link")).toBe("#58a6ff");
    });

    test("should set CSS custom properties on specified container", () => {
      const container = document.createElement("div");
      const theme = getLightTheme();
      applyMarkdownTheme(theme, container);

      expect(container.style.getPropertyValue("--markdown-bg")).toBe("#ffffff");
      expect(container.style.getPropertyValue("--markdown-fg")).toBe("#24292f");
    });
  });

  describe("theme completeness", () => {
    test("dark theme should have all required properties", () => {
      const theme = getDarkTheme();
      const requiredProps: (keyof MarkdownTheme)[] = [
        "bg",
        "fg",
        "heading",
        "link",
        "border",
        "muted",
        "codeBg",
        "preBg",
        "codeFg",
        "tableBg",
        "tableStripe",
      ];

      for (const prop of requiredProps) {
        expect(theme[prop]).toBeDefined();
        expect(typeof theme[prop]).toBe("string");
        expect(theme[prop].length).toBeGreaterThan(0);
      }
    });

    test("light theme should have all required properties", () => {
      const theme = getLightTheme();
      const requiredProps: (keyof MarkdownTheme)[] = [
        "bg",
        "fg",
        "heading",
        "link",
        "border",
        "muted",
        "codeBg",
        "preBg",
        "codeFg",
        "tableBg",
        "tableStripe",
      ];

      for (const prop of requiredProps) {
        expect(theme[prop]).toBeDefined();
        expect(typeof theme[prop]).toBe("string");
        expect(theme[prop].length).toBeGreaterThan(0);
      }
    });
  });
});
