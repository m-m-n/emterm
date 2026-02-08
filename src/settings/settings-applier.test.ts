/**
 * Tests for Settings Applier
 */

import { afterEach, describe, test, expect, beforeEach } from "bun:test";
import {
  applySettings,
  applySettingsToCSS,
  applyFontSize,
  applyFontFamily,
  buildFontFamilyChain,
  applyLineHeight,
  applyUiTheme,
  applyTerminalColorScheme,
  applyCursorStyle,
  applyCursorBlink,
  applyPadding,
  applyScrollbar,
  applyMarkdownSettings,
  applyMarkdownColorTheme,
  type MarkdownColorThemeOptions,
} from "./settings-applier";
import type { AppSettings, KeybindSettings, UserColorScheme } from "./types";

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
    next_tab: "Ctrl+PageDown",
    prev_tab: "Ctrl+PageUp",
    zoom_in: "Ctrl+Plus",
    zoom_out: "Ctrl+Minus",
    zoom_reset: "Ctrl+0",
    toggle_fullscreen: "F11",
    open_settings: "Ctrl+,",
    toggle_tab_bar: "Ctrl+Shift+B",
    jump_to_prev_prompt: "Ctrl+Shift+ArrowUp",
    jump_to_next_prompt: "Ctrl+Shift+ArrowDown",
  };

  return {
    font_size: 13,
    font_family_primary: "",
    font_family_secondary: "",
    font_family_emoji: "",
    line_height: 1.2,
    ui_theme: "system",
    ui_theme_preset: "purple",
    terminal_color_scheme: "",
    padding: 4,
    scrollback_lines: 10000,
    show_scrollbar: "auto",
    show_tab_bar: true,
    shell_path: "",
    shell_args: [],
    cursor_style: "block",
    cursor_blink: true,
    scroll_speed: 3,
    bell_action: "visual",
    url_detection: true,
    copy_on_select: false,
    keybinds: defaultKeybinds,
    language: "auto",
    ui_font_family: "",
    custom_color_schemes: [],
    markdown_theme_follow_ui: true,
    markdown_theme: "system",
    markdown_theme_preset: "purple",
    markdown_body_font_family: "",
    markdown_code_font_family: "",
    markdown_font_size: 14,
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

describe("buildFontFamilyChain", () => {
  test("all empty -> monospace", () => {
    expect(buildFontFamilyChain("", "", "")).toBe("monospace");
  });

  test("primary only", () => {
    expect(buildFontFamilyChain("Fira Code", "", "")).toBe("Fira Code, monospace");
  });

  test("primary + secondary", () => {
    expect(buildFontFamilyChain("Fira Code", "", "Noto Sans JP")).toBe("Fira Code, Noto Sans JP, monospace");
  });

  test("all three filled", () => {
    expect(buildFontFamilyChain("JetBrains Mono", "Noto Color Emoji", "Noto Sans JP")).toBe(
      "JetBrains Mono, Noto Color Emoji, Noto Sans JP, monospace",
    );
  });

  test("emoji + secondary (no primary)", () => {
    expect(buildFontFamilyChain("", "Noto Color Emoji", "Noto Sans JP")).toBe(
      "Noto Color Emoji, Noto Sans JP, monospace",
    );
  });

  test("secondary only", () => {
    expect(buildFontFamilyChain("", "", "Noto Sans JP")).toBe("Noto Sans JP, monospace");
  });

  test("emoji only", () => {
    expect(buildFontFamilyChain("", "Noto Color Emoji", "")).toBe("Noto Color Emoji, monospace");
  });
});

describe("applyFontFamily", () => {
  test("should set --terminal-font-family for non-empty primary", () => {
    applyFontFamily("Fira Code", "", "");
    expect(mockStyle.properties["--terminal-font-family"]).toBe("Fira Code, monospace");
  });

  test("should remove --terminal-font-family when all empty", () => {
    mockStyle.properties["--terminal-font-family"] = "Fira Code, monospace";
    applyFontFamily("", "", "");
    expect(mockStyle.properties["--terminal-font-family"]).toBeUndefined();
  });

  test("should notify renderers with chain string", () => {
    applyFontFamily("JetBrains Mono", "", "");
    expect(mockRendererCalls).toContainEqual({
      setting: "fontFamily",
      value: "JetBrains Mono, monospace",
    });
  });

  test("should notify renderers with monospace when all empty", () => {
    applyFontFamily("", "", "");
    expect(mockRendererCalls).toContainEqual({
      setting: "fontFamily",
      value: "monospace",
    });
  });

  test("should build full chain with all three fonts", () => {
    applyFontFamily("Fira Code", "Noto Color Emoji", "Noto Sans JP");
    expect(mockStyle.properties["--terminal-font-family"]).toBe(
      "Fira Code, Noto Color Emoji, Noto Sans JP, monospace",
    );
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

  test("should apply purple dark preset colors for dark theme by default", () => {
    applyUiTheme("dark");
    expect(mockStyle.properties["--md-sys-color-primary"]).toBe("#D0BCFF");
    expect(mockStyle.properties["--md-sys-color-surface"]).toBe("#141218");
  });

  test("should apply blue dark preset colors for dark + blue", () => {
    applyUiTheme("dark", "blue");
    expect(mockStyle.properties["--md-sys-color-primary"]).toBe("#A8C7FA");
    expect(mockStyle.properties["--md-sys-color-surface"]).toBe("#111318");
  });

  test("should apply green light preset colors for light + green", () => {
    applyUiTheme("light", "green");
    expect(mockStyle.properties["--md-sys-color-primary"]).toBe("#006D3E");
    expect(mockStyle.properties["--md-sys-color-surface"]).toBe("#F5FBF5");
  });

  test("should apply orange preset with system theme (dark)", () => {
    mockMatchesDark = true;
    applyUiTheme("system", "orange");
    expect(mockStyle.properties["--md-sys-color-primary"]).toBe("#FFB877");
    expect(mockStyle.properties["--md-sys-color-surface"]).toBe("#18120B");
  });

  test("should apply orange preset with system theme (light)", () => {
    mockMatchesDark = false;
    applyUiTheme("system", "orange");
    expect(mockStyle.properties["--md-sys-color-primary"]).toBe("#8B5000");
    expect(mockStyle.properties["--md-sys-color-surface"]).toBe("#FFF8F4");
  });

  test("system theme listener should re-apply preset colors on change", () => {
    mockMatchesDark = false;
    applyUiTheme("system", "blue");
    expect(mockStyle.properties["--md-sys-color-primary"]).toBe("#0B57D0"); // blue light

    // Simulate system theme change to dark
    if (mockMediaChangeHandler) {
      mockMediaChangeHandler({ matches: true } as any);
    }
    expect(mockDataTheme).toBe("dark");
    expect(mockStyle.properties["--md-sys-color-primary"]).toBe("#A8C7FA"); // blue dark
  });

  test("should fallback to purple when preset is invalid", () => {
    applyUiTheme("dark", "invalid" as any);
    expect(mockStyle.properties["--md-sys-color-primary"]).toBe("#D0BCFF"); // purple dark
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

describe("applySettings (full)", () => {
  test("should apply all settings", () => {
    const settings = makeSettings({
      font_size: 16,
      font_family_primary: "JetBrains Mono",
      line_height: 1.4,
      ui_theme: "dark",
      padding: 8,
      show_scrollbar: "always",
    });

    applySettings(settings);

    expect(mockStyle.properties["--terminal-font-size"]).toBe("16pt");
    expect(mockStyle.properties["--terminal-font-family"]).toBe(
      "JetBrains Mono, monospace",
    );
    expect(mockStyle.properties["--terminal-line-height"]).toBe("1.4");
    expect(mockDataTheme).toBe("dark");
    expect(mockStyle.properties["--terminal-padding"]).toBe("8px");
    expect(mockStyle.properties["--terminal-scrollbar-mode"]).toBe("always");
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

describe("startup race condition: color scheme applied before tabManager exists", () => {
  test("notifyRenderers is silently dropped when tabManager is null", () => {
    // Simulate startup state: tabManager does not exist yet
    // @ts-expect-error - Mock for testing
    globalThis.window = {
      matchMedia: () => {
        mockMediaQueryList.matches = false;
        return mockMediaQueryList;
      },
      tabManager: null, // Not yet created
    };

    mockRendererCalls = [];

    // Apply a non-default color scheme (e.g., solarized-light with light background)
    applyTerminalColorScheme("solarized-light");

    // CSS variable IS set (padding area will show theme background)
    expect(mockStyle.properties["--terminal-background"]).toBe(
      "rgb(253, 246, 227)",
    );

    // But renderer notification is LOST - no renderer gets the color scheme
    // This is the root cause: the canvas renderer starts with DEFAULT_BACKGROUND (black)
    // while the CSS padding area already shows the theme's background color
    expect(mockRendererCalls).toEqual([]);
  });

  test("notifyRenderers succeeds when tabManager exists", () => {
    // Simulate normal state: tabManager exists
    // @ts-expect-error - Mock for testing
    globalThis.window = {
      matchMedia: () => {
        mockMediaQueryList.matches = false;
        return mockMediaQueryList;
      },
      tabManager: mockTabManager,
    };

    mockRendererCalls = [];

    applyTerminalColorScheme("solarized-light");

    // Renderer notification IS delivered
    expect(mockRendererCalls).toContainEqual({
      setting: "colorScheme",
      value: "solarized-light",
    });
  });

  test("CSS variable and renderer notification are in sync when tabManager exists", () => {
    // @ts-expect-error - Mock for testing
    globalThis.window = {
      matchMedia: () => {
        mockMediaQueryList.matches = false;
        return mockMediaQueryList;
      },
      tabManager: mockTabManager,
    };

    mockRendererCalls = [];

    applyTerminalColorScheme("dracula");

    // Both CSS variable and renderer are updated
    expect(mockStyle.properties["--terminal-background"]).toBe(
      "rgb(40, 42, 54)",
    );
    expect(mockRendererCalls).toContainEqual({
      setting: "colorScheme",
      value: "dracula",
    });
  });

  test("full startup sequence: applySettings before tabManager causes desync", () => {
    // Step 1: Simulate main.ts startup - applySettings called before tabManager
    // @ts-expect-error - Mock for testing
    globalThis.window = {
      matchMedia: () => {
        mockMediaQueryList.matches = false;
        return mockMediaQueryList;
      },
      tabManager: null, // Not yet created at this point in main()
    };

    mockRendererCalls = [];

    const settings = makeSettings({
      terminal_color_scheme: "solarized-light",
    });

    applySettings(settings);

    // CSS variable is set (padding/body shows theme background)
    expect(mockStyle.properties["--terminal-background"]).toBe(
      "rgb(253, 246, 227)",
    );

    // But NO renderer notification was delivered
    // The colorScheme notification was lost because tabManager was null
    const colorSchemeNotifications = mockRendererCalls.filter(
      (c) => c.setting === "colorScheme",
    );
    expect(colorSchemeNotifications).toEqual([]);

    // This means: after CanvasRenderer is created, it will use DEFAULT_BACKGROUND (black)
    // while the CSS padding area shows rgb(253, 246, 227) - a visible mismatch
  });
});

// ============================================================
// User Color Scheme Tests (Phase 3)
// ============================================================

function createMockUserScheme(name: string): UserColorScheme {
  return {
    name,
    foreground: "#f8f8f2",
    background: "#282a36",
    cursor: "#f8f8f2",
    selection: "#44475a",
    ansi_colors: [
      "#21222c", "#ff5555", "#50fa7b", "#f1fa8c",
      "#bd93f9", "#ff79c6", "#8be9fd", "#f8f8f2",
      "#6272a4", "#ff6e6e", "#69ff94", "#ffffa5",
      "#d6acff", "#ff92df", "#a4ffff", "#ffffff",
    ],
  };
}

describe("applyTerminalColorScheme with user schemes", () => {
  test("should apply user scheme colors as CSS variables", () => {
    const userSchemes = [createMockUserScheme("my_theme")];
    applyTerminalColorScheme("my_theme", userSchemes);

    expect(mockStyle.properties["--terminal-foreground"]).toBe("#f8f8f2");
    expect(mockStyle.properties["--terminal-background"]).toBe("#282a36");
    expect(mockStyle.properties["--terminal-cursor-color"]).toBe("#f8f8f2");
    expect(mockStyle.properties["--terminal-selection-bg"]).toBe("#44475a");
  });

  test("should apply all 16 ANSI colors from user scheme", () => {
    const userSchemes = [createMockUserScheme("my_theme")];
    applyTerminalColorScheme("my_theme", userSchemes);

    expect(mockStyle.properties["--terminal-color-0"]).toBe("#21222c");
    expect(mockStyle.properties["--terminal-color-1"]).toBe("#ff5555");
    expect(mockStyle.properties["--terminal-color-7"]).toBe("#f8f8f2");
    expect(mockStyle.properties["--terminal-color-8"]).toBe("#6272a4");
    expect(mockStyle.properties["--terminal-color-15"]).toBe("#ffffff");
  });

  test("should set data attribute for user scheme", () => {
    const userSchemes = [createMockUserScheme("my_theme")];
    applyTerminalColorScheme("my_theme", userSchemes);

    expect(mockAttributes["data-terminal-color-scheme"]).toBe("my_theme");
  });

  test("should notify renderers with user scheme object", () => {
    const userScheme = createMockUserScheme("my_theme");
    const userSchemes = [userScheme];
    applyTerminalColorScheme("my_theme", userSchemes);

    expect(mockRendererCalls).toContainEqual({
      setting: "userColorScheme",
      value: userScheme,
    });
  });

  test("should fall back to preset lookup when user scheme not found", () => {
    const userSchemes: UserColorScheme[] = [];
    applyTerminalColorScheme("dracula", userSchemes);

    // Should use preset's background color
    expect(mockStyle.properties["--terminal-background"]).toBe("rgb(40, 42, 54)");
    expect(mockAttributes["data-terminal-color-scheme"]).toBe("dracula");
  });

  test("should fall back to preset when empty userSchemes array provided", () => {
    applyTerminalColorScheme("solarized-dark", []);

    expect(mockAttributes["data-terminal-color-scheme"]).toBe("solarized-dark");
    expect(mockRendererCalls).toContainEqual({
      setting: "colorScheme",
      value: "solarized-dark",
    });
  });

  test("should apply emterm/default and clear CSS vars even with user schemes", () => {
    const userSchemes = [createMockUserScheme("my_theme")];

    // First apply user scheme
    applyTerminalColorScheme("my_theme", userSchemes);
    expect(mockStyle.properties["--terminal-foreground"]).toBe("#f8f8f2");

    // Then apply emterm
    applyTerminalColorScheme("emterm", userSchemes);
    expect(mockStyle.properties["--terminal-foreground"]).toBeUndefined();
    expect(mockAttributes["data-terminal-color-scheme"]).toBeUndefined();
  });

  test("should handle undefined userSchemes (backward compatibility)", () => {
    // @ts-expect-error - Testing backward compatibility
    applyTerminalColorScheme("dracula", undefined);

    expect(mockAttributes["data-terminal-color-scheme"]).toBe("dracula");
  });
});

describe("applyMarkdownSettings", () => {
  test("should set all three CSS variables when fonts are non-empty", () => {
    applyMarkdownSettings("Georgia", "Fira Code", 16);
    expect(mockStyle.properties["--markdown-body-font-family"]).toBe("Georgia");
    expect(mockStyle.properties["--markdown-code-font-family"]).toBe("Fira Code");
    expect(mockStyle.properties["--markdown-body-font-size"]).toBe("16pt");
  });

  test("should remove body font CSS variable when empty string", () => {
    // First set it
    applyMarkdownSettings("Georgia", "Fira Code", 14);
    expect(mockStyle.properties["--markdown-body-font-family"]).toBe("Georgia");

    // Then clear it
    applyMarkdownSettings("", "Fira Code", 14);
    expect(mockStyle.properties["--markdown-body-font-family"]).toBeUndefined();
    expect(mockStyle.properties["--markdown-code-font-family"]).toBe("Fira Code");
  });

  test("should remove code font CSS variable when empty string", () => {
    applyMarkdownSettings("Georgia", "", 14);
    expect(mockStyle.properties["--markdown-body-font-family"]).toBe("Georgia");
    expect(mockStyle.properties["--markdown-code-font-family"]).toBeUndefined();
  });

  test("should remove font CSS variables for whitespace-only strings", () => {
    applyMarkdownSettings("  ", "  \t  ", 14);
    expect(mockStyle.properties["--markdown-body-font-family"]).toBeUndefined();
    expect(mockStyle.properties["--markdown-code-font-family"]).toBeUndefined();
  });

  test("should always set font size with pt unit", () => {
    applyMarkdownSettings("", "", 8);
    expect(mockStyle.properties["--markdown-body-font-size"]).toBe("8pt");

    applyMarkdownSettings("", "", 32);
    expect(mockStyle.properties["--markdown-body-font-size"]).toBe("32pt");
  });

  test("should trim font names before setting", () => {
    applyMarkdownSettings("  Georgia  ", "  Fira Code  ", 14);
    expect(mockStyle.properties["--markdown-body-font-family"]).toBe("Georgia");
    expect(mockStyle.properties["--markdown-code-font-family"]).toBe("Fira Code");
  });
});

// ============================================================
// applyMarkdownColorTheme
// ============================================================

describe("applyMarkdownColorTheme", () => {
  test("with followUi=true uses UI theme and preset", () => {
    // followUi=true: should use uiTheme (dark) + uiPreset (blue), ignoring md values
    applyMarkdownColorTheme({ followUi: true, mdTheme: "light", mdPreset: "green", uiTheme: "dark", uiPreset: "blue" });
    // Verify blue/dark palette colors
    expect(mockStyle.properties["--markdown-bg"]).toBe("#111318");
    expect(mockStyle.properties["--markdown-link"]).toBe("#A8C7FA");
  });

  test("with followUi=false uses markdown theme and preset", () => {
    // followUi=false: should use mdTheme (light) + mdPreset (green), ignoring ui values
    applyMarkdownColorTheme({ followUi: false, mdTheme: "light", mdPreset: "green", uiTheme: "dark", uiPreset: "blue" });
    // Verify green/light palette colors
    expect(mockStyle.properties["--markdown-bg"]).toBe("#F5FBF5");
    expect(mockStyle.properties["--markdown-link"]).toBe("#006D3E");
  });

  test("sets all 11 --markdown-* color CSS variables", () => {
    applyMarkdownColorTheme({ followUi: false, mdTheme: "dark", mdPreset: "purple", uiTheme: "system", uiPreset: "purple" });
    const expectedVars = [
      "--markdown-bg",
      "--markdown-fg",
      "--markdown-heading",
      "--markdown-link",
      "--markdown-border",
      "--markdown-blockquote",
      "--markdown-code-bg",
      "--markdown-code-fg",
      "--markdown-pre-bg",
      "--markdown-table-bg",
      "--markdown-table-stripe",
    ];
    for (const v of expectedVars) {
      expect(mockStyle.properties[v]).toBeDefined();
      expect(mockStyle.properties[v].length).toBeGreaterThan(0);
    }
  });

  test("system theme resolves to dark when prefers-color-scheme is dark", () => {
    mockMatchesDark = true;
    mockMediaQueryList.matches = true;
    applyMarkdownColorTheme({ followUi: false, mdTheme: "system", mdPreset: "orange", uiTheme: "system", uiPreset: "purple" });
    // Should use orange/dark palette
    expect(mockStyle.properties["--markdown-bg"]).toBe("#18120B");
    expect(mockStyle.properties["--markdown-link"]).toBe("#FFB877");
  });

  test("system theme resolves to light when prefers-color-scheme is light", () => {
    mockMatchesDark = false;
    mockMediaQueryList.matches = false;
    applyMarkdownColorTheme({ followUi: false, mdTheme: "system", mdPreset: "orange", uiTheme: "system", uiPreset: "purple" });
    // Should use orange/light palette
    expect(mockStyle.properties["--markdown-bg"]).toBe("#FFF8F4");
    expect(mockStyle.properties["--markdown-link"]).toBe("#8B5000");
  });

  test("followUi=true with system UI theme resolves correctly", () => {
    mockMatchesDark = true;
    mockMediaQueryList.matches = true;
    applyMarkdownColorTheme({ followUi: true, mdTheme: "light", mdPreset: "green", uiTheme: "system", uiPreset: "purple" });
    // Should use purple/dark palette (UI is system->dark, purple preset)
    expect(mockStyle.properties["--markdown-bg"]).toBe("#141218");
    expect(mockStyle.properties["--markdown-link"]).toBe("#D0BCFF");
  });

  test("should clean up previous media listener when switching themes", () => {
    applyMarkdownColorTheme({ followUi: false, mdTheme: "system", mdPreset: "purple", uiTheme: "system", uiPreset: "purple" });
    expect(mockMediaChangeHandler).not.toBeNull();
    applyMarkdownColorTheme({ followUi: false, mdTheme: "dark", mdPreset: "purple", uiTheme: "system", uiPreset: "purple" });
    expect(mockMediaChangeHandler).toBeNull();
  });
});
