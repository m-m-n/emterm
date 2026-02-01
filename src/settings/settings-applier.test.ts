/**
 * Tests for Settings Applier
 */

import { afterEach, describe, test, expect, beforeEach } from "bun:test";
import {
  applySettings,
  applySettingsToCSS,
  applyFontSize,
  applyFontFamily,
  applyLineHeight,
  applyUiTheme,
  applyTerminalColorScheme,
  applyCursorStyle,
  applyCursorBlink,
  applyPadding,
  applyScrollbar,
  applyOpacity,
} from "./settings-applier";
import type { AppSettings, KeybindSettings } from "./types";

// Mock document.documentElement
const mockStyle = {
  setProperty: (name: string, value: string) => {
    mockStyle.properties[name] = value;
  },
  removeProperty: (name: string) => {
    delete mockStyle.properties[name];
  },
  properties: {} as Record<string, string>,
};

let mockDataTheme = "";
const mockAttributes: Record<string, string> = {};

// Mock tabManager for renderer notifications
let mockRendererCalls: Array<{ setting: string; value: any }> = [];
const mockTabManager = {
  updateAllTerminalsSetting: (setting: string, value: any) => {
    mockRendererCalls.push({ setting, value });
  },
};

// Mock matchMedia
let mockMatchesDark = false;
let mockMediaChangeHandler: ((e: any) => void) | null = null;

const mockMediaQueryList = {
  matches: false,
  addEventListener: (_type: string, handler: (e: any) => void) => {
    mockMediaChangeHandler = handler;
  },
  removeEventListener: () => {
    mockMediaChangeHandler = null;
  },
};

// Save original globals
const savedDocument = globalThis.document;
const savedWindow = globalThis.window;

// Setup mock before each test
beforeEach(() => {
  mockStyle.properties = {};
  mockDataTheme = "";
  // Clear mockAttributes (keep the same object reference)
  for (const key of Object.keys(mockAttributes)) {
    delete mockAttributes[key];
  }
  mockMatchesDark = false;
  mockMediaChangeHandler = null;
  mockRendererCalls = [];

  // @ts-expect-error - Mock for testing
  globalThis.document = {
    documentElement: {
      style: mockStyle,
      setAttribute: (name: string, value: string) => {
        mockAttributes[name] = value;
        if (name === "data-theme") mockDataTheme = value;
      },
      getAttribute: (name: string) => mockAttributes[name] || null,
      removeAttribute: (name: string) => {
        delete mockAttributes[name];
      },
    },
  };

  // @ts-expect-error - Mock for testing
  globalThis.window = {
    matchMedia: () => {
      mockMediaQueryList.matches = mockMatchesDark;
      return mockMediaQueryList;
    },
    tabManager: mockTabManager,
  };
});

// Restore original globals after each test
afterEach(() => {
  globalThis.document = savedDocument;
  globalThis.window = savedWindow;
});

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
    font_family: "",
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
    keybinds: defaultKeybinds,
    ...overrides,
  };
}

describe("applyFontSize", () => {
  test("should update --terminal-font-size CSS variable", () => {
    applyFontSize(16);
    expect(mockStyle.properties["--terminal-font-size"]).toBe("16pt");
  });

  test("should handle minimum font size", () => {
    applyFontSize(8);
    expect(mockStyle.properties["--terminal-font-size"]).toBe("8pt");
  });

  test("should handle maximum font size", () => {
    applyFontSize(32);
    expect(mockStyle.properties["--terminal-font-size"]).toBe("32pt");
  });
});

describe("applyFontFamily", () => {
  test("should set --terminal-font-family for non-empty value", () => {
    applyFontFamily("Fira Code");
    expect(mockStyle.properties["--terminal-font-family"]).toBe("Fira Code");
  });

  test("should remove --terminal-font-family for empty string", () => {
    // First set it
    mockStyle.properties["--terminal-font-family"] = "Fira Code";
    applyFontFamily("");
    expect(mockStyle.properties["--terminal-font-family"]).toBeUndefined();
  });

  test("should notify renderers with fontFamily", () => {
    applyFontFamily("JetBrains Mono");
    expect(mockRendererCalls).toContainEqual({
      setting: "fontFamily",
      value: "JetBrains Mono",
    });
  });

  test("should notify renderers with empty string for default", () => {
    applyFontFamily("");
    expect(mockRendererCalls).toContainEqual({
      setting: "fontFamily",
      value: "",
    });
  });
});

describe("applyLineHeight", () => {
  test("should set --terminal-line-height CSS variable", () => {
    applyLineHeight(1.5);
    expect(mockStyle.properties["--terminal-line-height"]).toBe("1.5");
  });

  test("should set default line height", () => {
    applyLineHeight(1.2);
    expect(mockStyle.properties["--terminal-line-height"]).toBe("1.2");
  });

  test("should notify renderers with lineHeight", () => {
    applyLineHeight(1.5);
    expect(mockRendererCalls).toContainEqual({
      setting: "lineHeight",
      value: 1.5,
    });
  });
});

describe("applyUiTheme", () => {
  test("should set data-theme=light for light theme", () => {
    applyUiTheme("light");
    expect(mockDataTheme).toBe("light");
  });

  test("should set data-theme=dark for dark theme", () => {
    applyUiTheme("dark");
    expect(mockDataTheme).toBe("dark");
  });

  test("should resolve system theme to dark when prefers-color-scheme is dark", () => {
    mockMatchesDark = true;
    applyUiTheme("system");
    expect(mockDataTheme).toBe("dark");
  });

  test("should resolve system theme to light when prefers-color-scheme is light", () => {
    mockMatchesDark = false;
    applyUiTheme("system");
    expect(mockDataTheme).toBe("light");
  });

  test("should register media change listener for system theme", () => {
    applyUiTheme("system");
    expect(mockMediaChangeHandler).not.toBeNull();
  });

  test("should clean up previous media listener when switching themes", () => {
    applyUiTheme("system");
    expect(mockMediaChangeHandler).not.toBeNull();

    applyUiTheme("dark");
    expect(mockMediaChangeHandler).toBeNull();
  });
});

describe("applyPadding", () => {
  test("should set --terminal-padding CSS variable", () => {
    applyPadding(8);
    expect(mockStyle.properties["--terminal-padding"]).toBe("8px");
  });

  test("should set padding to 0", () => {
    applyPadding(0);
    expect(mockStyle.properties["--terminal-padding"]).toBe("0px");
  });
});

describe("applyScrollbar", () => {
  test("should set --terminal-scrollbar-mode to auto", () => {
    applyScrollbar("auto");
    expect(mockStyle.properties["--terminal-scrollbar-mode"]).toBe("auto");
  });

  test("should set --terminal-scrollbar-mode to always", () => {
    applyScrollbar("always");
    expect(mockStyle.properties["--terminal-scrollbar-mode"]).toBe("always");
  });

  test("should set --terminal-scrollbar-mode to never", () => {
    applyScrollbar("never");
    expect(mockStyle.properties["--terminal-scrollbar-mode"]).toBe("never");
  });

  test("should map 'always' to overflow 'scroll'", () => {
    applyScrollbar("always");
    expect(mockStyle.properties["--terminal-scrollbar-overflow"]).toBe("scroll");
  });

  test("should map 'never' to overflow 'hidden'", () => {
    applyScrollbar("never");
    expect(mockStyle.properties["--terminal-scrollbar-overflow"]).toBe("hidden");
  });

  test("should map 'auto' to overflow 'auto'", () => {
    applyScrollbar("auto");
    expect(mockStyle.properties["--terminal-scrollbar-overflow"]).toBe("auto");
  });
});

describe("applyOpacity", () => {
  test("should set --terminal-opacity CSS variable", () => {
    applyOpacity(0.8);
    expect(mockStyle.properties["--terminal-opacity"]).toBe("0.8");
  });

  test("should set full opacity", () => {
    applyOpacity(1.0);
    expect(mockStyle.properties["--terminal-opacity"]).toBe("1");
  });

  test("should notify renderers with opacity", () => {
    applyOpacity(0.5);
    expect(mockRendererCalls).toContainEqual({
      setting: "opacity",
      value: 0.5,
    });
  });
});

describe("applySettings (full)", () => {
  test("should apply all settings", () => {
    const settings = makeSettings({
      font_size: 16,
      font_family: "JetBrains Mono",
      line_height: 1.4,
      ui_theme: "dark",
      padding: 8,
      show_scrollbar: "always",
      opacity: 0.9,
    });

    applySettings(settings);

    expect(mockStyle.properties["--terminal-font-size"]).toBe("16pt");
    expect(mockStyle.properties["--terminal-font-family"]).toBe(
      "JetBrains Mono",
    );
    expect(mockStyle.properties["--terminal-line-height"]).toBe("1.4");
    expect(mockDataTheme).toBe("dark");
    expect(mockStyle.properties["--terminal-padding"]).toBe("8px");
    expect(mockStyle.properties["--terminal-scrollbar-mode"]).toBe("always");
    expect(mockStyle.properties["--terminal-opacity"]).toBe("0.9");
  });
});

describe("applyCursorStyle", () => {
  test("should notify renderers with cursor style", () => {
    applyCursorStyle("underline");
    expect(mockRendererCalls).toContainEqual({
      setting: "cursorStyle",
      value: "underline",
    });
  });

  test("should notify renderers with block style", () => {
    applyCursorStyle("block");
    expect(mockRendererCalls).toContainEqual({
      setting: "cursorStyle",
      value: "block",
    });
  });

  test("should notify renderers with bar style", () => {
    applyCursorStyle("bar");
    expect(mockRendererCalls).toContainEqual({
      setting: "cursorStyle",
      value: "bar",
    });
  });
});

describe("applyCursorBlink", () => {
  test("should notify renderers with blink enabled", () => {
    applyCursorBlink(true);
    expect(mockRendererCalls).toContainEqual({
      setting: "cursorBlink",
      value: true,
    });
  });

  test("should notify renderers with blink disabled", () => {
    applyCursorBlink(false);
    expect(mockRendererCalls).toContainEqual({
      setting: "cursorBlink",
      value: false,
    });
  });
});

describe("applyTerminalColorScheme", () => {
  test("should remove custom CSS variables for 'default' scheme", () => {
    // First set some terminal color variables
    mockStyle.properties["--terminal-foreground"] = "#fff";
    mockStyle.properties["--terminal-background"] = "#000";
    mockStyle.properties["--terminal-color-0"] = "#111";

    applyTerminalColorScheme("default");

    expect(mockStyle.properties["--terminal-foreground"]).toBeUndefined();
    expect(mockStyle.properties["--terminal-background"]).toBeUndefined();
    expect(mockStyle.properties["--terminal-color-0"]).toBeUndefined();
  });

  test("should remove custom CSS variables for empty string", () => {
    mockStyle.properties["--terminal-foreground"] = "#fff";
    mockStyle.properties["--terminal-cursor-color"] = "#0f0";

    applyTerminalColorScheme("");

    expect(mockStyle.properties["--terminal-foreground"]).toBeUndefined();
    expect(mockStyle.properties["--terminal-cursor-color"]).toBeUndefined();
  });

  test("should remove custom CSS variables for 'emterm' scheme", () => {
    mockStyle.properties["--terminal-foreground"] = "#fff";
    mockStyle.properties["--terminal-background"] = "#000";

    applyTerminalColorScheme("emterm");

    expect(mockStyle.properties["--terminal-foreground"]).toBeUndefined();
    expect(mockStyle.properties["--terminal-background"]).toBeUndefined();
  });

  test("should notify renderers with 'emterm' for default scheme", () => {
    applyTerminalColorScheme("default");

    expect(mockRendererCalls).toContainEqual({
      setting: "colorScheme",
      value: "emterm",
    });
  });

  test("should notify renderers with 'emterm' for empty string", () => {
    applyTerminalColorScheme("");

    expect(mockRendererCalls).toContainEqual({
      setting: "colorScheme",
      value: "emterm",
    });
  });

  test("should notify renderers with 'emterm' for emterm scheme", () => {
    applyTerminalColorScheme("emterm");

    expect(mockRendererCalls).toContainEqual({
      setting: "colorScheme",
      value: "emterm",
    });
  });

  test("should set data attribute for named scheme", () => {
    applyTerminalColorScheme("solarized-dark");

    expect(mockAttributes["data-terminal-color-scheme"]).toBe(
      "solarized-dark",
    );
  });

  test("should notify renderers with scheme name for named scheme", () => {
    applyTerminalColorScheme("solarized-dark");

    expect(mockRendererCalls).toContainEqual({
      setting: "colorScheme",
      value: "solarized-dark",
    });
  });

  test("should remove data attribute when switching to default scheme", () => {
    // First set a named scheme
    applyTerminalColorScheme("solarized-dark");
    expect(mockAttributes["data-terminal-color-scheme"]).toBe("solarized-dark");

    // Switch to default
    applyTerminalColorScheme("default");
    expect(mockAttributes["data-terminal-color-scheme"]).toBeUndefined();
  });
});

describe("applySettingsToCSS (legacy)", () => {
  test("should call applySettings", () => {
    const settings = makeSettings({ font_size: 20 });

    applySettingsToCSS(settings);

    expect(mockStyle.properties["--terminal-font-size"]).toBe("20pt");
  });
});
