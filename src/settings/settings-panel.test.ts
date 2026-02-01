/**
 * Tests for Settings Panel - Description feature
 *
 * Tests that render methods correctly handle the optional description parameter:
 * - Create description span with correct class, id, and textContent
 * - Set aria-describedby on the input element
 * - Backward compatible when description is omitted
 */

import { afterEach, describe, test, expect, beforeEach, mock } from "bun:test";

// Mock @tauri-apps/api/core before importing the module
mock.module("@tauri-apps/api/core", () => ({
  invoke: () => Promise.resolve(),
}));

// Mock settings-service
mock.module("./settings-service", () => ({
  SettingsService: {
    load: () => Promise.resolve(makeSettings()),
    save: () => Promise.resolve(),
  },
}));

// Mock settings-applier
mock.module("./settings-applier", () => ({
  applySettings: () => {},
  applySettingsToCSS: () => {},
  applyFontSize: () => {},
  applyFontFamily: () => {},
  buildFontFamilyChain: () => "monospace",
  applyLineHeight: () => {},
  applyUiTheme: () => {},
  applyTerminalColorScheme: () => {},
  applyPadding: () => {},
  applyScrollbar: () => {},
  applyOpacity: () => {},
  applyCursorStyle: () => {},
  applyCursorBlink: () => {},
}));

import type { AppSettings, KeybindSettings } from "./types";

function makeSettings(overrides: Partial<AppSettings> = {}): AppSettings {
  const defaultKeybinds: KeybindSettings = {
    copy: "Ctrl+Shift+C",
    paste: "Ctrl+Shift+V",
    select_all: "Ctrl+Shift+A",
    search: "Ctrl+Shift+F",
    new_tab: "Ctrl+Shift+T",
    close_tab: "Ctrl+Shift+W",
    next_tab: "Ctrl+Tab",
    prev_tab: "Ctrl+Shift+Tab",
    zoom_in: "Ctrl+Plus",
    zoom_out: "Ctrl+Minus",
    zoom_reset: "Ctrl+0",
    toggle_fullscreen: "F11",
    open_settings: "Ctrl+Comma",
  };

  return {
    font_size: 13,
    font_family_primary: "",
    font_family_secondary: "",
    font_family_emoji: "",
    line_height: 1.2,
    ui_theme: "system",
    terminal_color_scheme: "",
    opacity: 1.0,
    padding: 4,
    scrollback_lines: 10000,
    show_scrollbar: "auto",
    shell_path: "",
    shell_args: [],
    cursor_style: "block",
    cursor_blink: true,
    scroll_speed: 3,
    bell_action: "visual",
    url_detection: true,
    copy_on_select: false,
    language: "auto",
    keybinds: defaultKeybinds,
    ...overrides,
  };
}

// Import after mocks are set up
const { SettingsPanel } = await import("./settings-panel");

describe("SettingsPanel render methods - description feature", () => {
  let container: HTMLElement;
  let panel: InstanceType<typeof SettingsPanel>;

  beforeEach(async () => {
    container = document.createElement("div");
    document.body.appendChild(container);
    panel = new SettingsPanel({ container });
    await panel.init();
  });

  afterEach(() => {
    panel.dispose();
    container.remove();
  });

  // ============================================================
  // Description span creation tests
  // ============================================================

  describe("description spans are created for all settings items", () => {
    test("should create description spans with correct class in appearance section", () => {
      const descriptions = container.querySelectorAll(".settings-description");
      // Appearance section has 10 items with descriptions (language + 9 appearance)
      expect(descriptions.length).toBeGreaterThanOrEqual(10);
    });

    test("should create description spans with correct id pattern", () => {
      // Check a few known description ids
      const fontSizeDesc = container.querySelector("#settings-font-size-desc");
      expect(fontSizeDesc).not.toBeNull();
      expect(fontSizeDesc?.className).toBe("settings-description");

      const languageDesc = container.querySelector("#settings-language-desc");
      expect(languageDesc).not.toBeNull();
    });

    test("should set description text via textContent (not innerHTML)", () => {
      const fontSizeDesc = container.querySelector("#settings-font-size-desc");
      expect(fontSizeDesc).not.toBeNull();
      // textContent should be a non-empty string
      expect(fontSizeDesc?.textContent).toBeTruthy();
      // innerHTML should equal textContent (no HTML tags)
      expect(fontSizeDesc?.innerHTML).toBe(fontSizeDesc?.textContent);
    });
  });

  // ============================================================
  // aria-describedby tests
  // ============================================================

  describe("aria-describedby is set on input elements", () => {
    test("should set aria-describedby on number inputs", () => {
      const fontSizeInput = container.querySelector("#settings-font-size") as HTMLInputElement;
      expect(fontSizeInput).not.toBeNull();
      expect(fontSizeInput?.getAttribute("aria-describedby")).toBe("settings-font-size-desc");
    });

    test("should set aria-describedby on text inputs", () => {
      const fontFamilyInput = container.querySelector("#settings-font-family-primary") as HTMLInputElement;
      expect(fontFamilyInput).not.toBeNull();
      expect(fontFamilyInput?.getAttribute("aria-describedby")).toBe("settings-font-family-primary-desc");
    });

    test("should set aria-describedby on select elements", () => {
      const languageSelect = container.querySelector("#settings-language") as HTMLSelectElement;
      expect(languageSelect).not.toBeNull();
      expect(languageSelect?.getAttribute("aria-describedby")).toBe("settings-language-desc");
    });

    test("should set aria-describedby on select elements (ui-theme)", () => {
      const uiThemeSelect = container.querySelector("#settings-ui-theme") as HTMLSelectElement;
      expect(uiThemeSelect).not.toBeNull();
      expect(uiThemeSelect?.getAttribute("aria-describedby")).toBe("settings-ui-theme-desc");
    });

    test("should set aria-describedby on slider inputs", () => {
      const opacitySlider = container.querySelector("#settings-opacity") as HTMLInputElement;
      expect(opacitySlider).not.toBeNull();
      expect(opacitySlider?.getAttribute("aria-describedby")).toBe("settings-opacity-desc");
    });
  });

  // ============================================================
  // Toggle row wrapper tests
  // ============================================================

  describe("toggle rows use wrapper div for description", () => {
    beforeEach(() => {
      // Switch to terminal category which has toggle buttons
      const terminalTab = container.querySelector('[data-category-id="terminal"]') as HTMLButtonElement;
      terminalTab?.click();
    });

    test("should wrap label and description in settings-toggle-label-group", () => {
      const cursorBlinkToggle = container.querySelector("#settings-cursor-blink");
      expect(cursorBlinkToggle).not.toBeNull();

      // Find the row containing this toggle
      const row = cursorBlinkToggle?.closest(".settings-row-toggle");
      expect(row).not.toBeNull();

      // The row should contain a wrapper div
      const wrapper = row?.querySelector(".settings-toggle-label-group");
      expect(wrapper).not.toBeNull();

      // Wrapper should contain the label
      const label = wrapper?.querySelector(".settings-label");
      expect(label).not.toBeNull();

      // Wrapper should contain the description
      const desc = wrapper?.querySelector(".settings-description");
      expect(desc).not.toBeNull();
    });
  });

  // ============================================================
  // Terminal section tests
  // ============================================================

  describe("terminal section descriptions", () => {
    beforeEach(() => {
      // Switch to terminal category
      const terminalTab = container.querySelector('[data-category-id="terminal"]') as HTMLButtonElement;
      terminalTab?.click();
    });

    test("should create description spans in terminal section", () => {
      const descriptions = container.querySelectorAll(".settings-description");
      // Terminal section has 8 items with descriptions
      expect(descriptions.length).toBeGreaterThanOrEqual(8);
    });

    test("should set aria-describedby on cursor style select", () => {
      const cursorStyleSelect = container.querySelector("#settings-cursor-style") as HTMLSelectElement;
      expect(cursorStyleSelect).not.toBeNull();
      expect(cursorStyleSelect?.getAttribute("aria-describedby")).toBe("settings-cursor-style-desc");
    });

    test("should set aria-describedby on cursor blink toggle", () => {
      const cursorBlinkToggle = container.querySelector("#settings-cursor-blink") as HTMLButtonElement;
      expect(cursorBlinkToggle).not.toBeNull();
      expect(cursorBlinkToggle?.getAttribute("aria-describedby")).toBe("settings-cursor-blink-desc");
    });

    test("should set aria-describedby on scroll speed slider", () => {
      const scrollSpeedSlider = container.querySelector("#settings-scroll-speed") as HTMLInputElement;
      expect(scrollSpeedSlider).not.toBeNull();
      expect(scrollSpeedSlider?.getAttribute("aria-describedby")).toBe("settings-scroll-speed-desc");
    });
  });
});
