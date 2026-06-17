/**
 * Tests for Color Scheme Editor - CRUD logic and utilities.
 */
import { describe, test, expect, beforeEach } from "bun:test";
import {
  generateCopyName,
  createUserSchemeFromPreset,
  updateUserSchemeColor,
  deleteUserScheme,
  duplicateScheme,
  renameUserScheme,
  isUserScheme,
  buildSelectOptions,
} from "./color-scheme-editor";
import type { UserColorScheme, AppSettings } from "./types";
import { COLOR_SCHEME_PRESETS } from "../terminal/colors";

// Helper to create a mock UserColorScheme
function createMockUserScheme(name: string): UserColorScheme {
  return {
    name,
    foreground: "#ffffff",
    background: "#000000",
    cursor: "#ffffff",
    selection: "#333333",
    ansi_colors: Array(16).fill("#808080"),
  };
}

describe("generateCopyName", () => {
  test("returns {name}_copy_1 when no copies exist", () => {
    const result = generateCopyName("dracula", []);
    expect(result).toBe("dracula_copy_1");
  });

  test("increments N when copies exist", () => {
    const existing = ["dracula_copy_1"];
    const result = generateCopyName("dracula", existing);
    expect(result).toBe("dracula_copy_2");
  });

  test("fills gaps in copy numbers", () => {
    const existing = ["dracula_copy_1", "dracula_copy_3"];
    const result = generateCopyName("dracula", existing);
    expect(result).toBe("dracula_copy_2");
  });

  test("handles multiple copies correctly", () => {
    const existing = ["dracula_copy_1", "dracula_copy_2", "dracula_copy_3"];
    const result = generateCopyName("dracula", existing);
    expect(result).toBe("dracula_copy_4");
  });

  test("ignores unrelated names", () => {
    const existing = ["monokai_copy_1", "nord_copy_2"];
    const result = generateCopyName("dracula", existing);
    expect(result).toBe("dracula_copy_1");
  });
});

describe("createUserSchemeFromPreset", () => {
  test("creates user scheme with dracula colors", () => {
    const userSchemes: UserColorScheme[] = [];
    const result = createUserSchemeFromPreset("dracula", userSchemes);

    expect(result).not.toBeNull();
    expect(result!.name).toBe("dracula_copy_1");
    expect(result!.ansi_colors.length).toBe(16);
  });

  test("creates user scheme with auto-incremented name", () => {
    const userSchemes = [createMockUserScheme("dracula_copy_1")];
    const result = createUserSchemeFromPreset("dracula", userSchemes);

    expect(result!.name).toBe("dracula_copy_2");
  });

  test("returns null for unknown preset", () => {
    const result = createUserSchemeFromPreset("unknown-preset", []);
    expect(result).toBeNull();
  });

  test("copies all color fields from preset", () => {
    const result = createUserSchemeFromPreset("dracula", []);
    const preset = COLOR_SCHEME_PRESETS.find((p) => p.name === "dracula")!;

    expect(result).not.toBeNull();
    // Verify colors are hex strings
    expect(result!.foreground).toMatch(/^#[0-9a-f]{6}$/);
    expect(result!.background).toMatch(/^#[0-9a-f]{6}$/);
    expect(result!.cursor).toMatch(/^#[0-9a-f]{6}$/);
    expect(result!.selection).toMatch(/^#[0-9a-f]{6}$/);
    expect(result!.ansi_colors.every((c) => /^#[0-9a-f]{6}$/.test(c))).toBe(true);
  });
});

describe("updateUserSchemeColor", () => {
  test("updates foreground color", () => {
    const scheme = createMockUserScheme("test");
    const result = updateUserSchemeColor(scheme, "foreground", "#ff0000");

    expect(result.foreground).toBe("#ff0000");
    expect(result.background).toBe("#000000"); // unchanged
  });

  test("updates background color", () => {
    const scheme = createMockUserScheme("test");
    const result = updateUserSchemeColor(scheme, "background", "#00ff00");

    expect(result.background).toBe("#00ff00");
  });

  test("updates cursor color", () => {
    const scheme = createMockUserScheme("test");
    const result = updateUserSchemeColor(scheme, "cursor", "#0000ff");

    expect(result.cursor).toBe("#0000ff");
  });

  test("updates selection color", () => {
    const scheme = createMockUserScheme("test");
    const result = updateUserSchemeColor(scheme, "selection", "#ffff00");

    expect(result.selection).toBe("#ffff00");
  });

  test("updates ANSI color by index", () => {
    const scheme = createMockUserScheme("test");
    const result = updateUserSchemeColor(scheme, "ansi_0", "#123456");

    expect(result.ansi_colors[0]).toBe("#123456");
    expect(result.ansi_colors[1]).toBe("#808080"); // unchanged
  });

  test("updates all ANSI colors 0-15", () => {
    let scheme = createMockUserScheme("test");
    for (let i = 0; i < 16; i++) {
      scheme = updateUserSchemeColor(scheme, `ansi_${i}` as any, `#00${i.toString(16).padStart(2, "0")}00`);
    }

    for (let i = 0; i < 16; i++) {
      expect(scheme.ansi_colors[i]).toBe(`#00${i.toString(16).padStart(2, "0")}00`);
    }
  });
});

describe("deleteUserScheme", () => {
  test("removes scheme from array", () => {
    const schemes = [
      createMockUserScheme("theme1"),
      createMockUserScheme("theme2"),
      createMockUserScheme("theme3"),
    ];
    const result = deleteUserScheme(schemes, "theme2");

    expect(result.length).toBe(2);
    expect(result.find((s) => s.name === "theme2")).toBeUndefined();
    expect(result.find((s) => s.name === "theme1")).toBeDefined();
    expect(result.find((s) => s.name === "theme3")).toBeDefined();
  });

  test("returns unchanged array if scheme not found", () => {
    const schemes = [createMockUserScheme("theme1")];
    const result = deleteUserScheme(schemes, "nonexistent");

    expect(result.length).toBe(1);
    expect(result[0].name).toBe("theme1");
  });

  test("handles empty array", () => {
    const result = deleteUserScheme([], "any");
    expect(result.length).toBe(0);
  });
});

describe("duplicateScheme", () => {
  test("duplicates preset as new user scheme", () => {
    const userSchemes: UserColorScheme[] = [];
    const result = duplicateScheme("dracula", userSchemes);

    expect(result).not.toBeNull();
    expect(result!.name).toBe("dracula_copy_1");
  });

  test("duplicates user scheme with new name", () => {
    const source = createMockUserScheme("my_theme");
    source.foreground = "#aabbcc";
    const userSchemes = [source];

    const result = duplicateScheme("my_theme", userSchemes);

    expect(result).not.toBeNull();
    expect(result!.name).toBe("my_theme_copy_1");
    expect(result!.foreground).toBe("#aabbcc");
  });

  test("increments copy number for duplicate of duplicate", () => {
    const userSchemes = [
      createMockUserScheme("dracula_copy_1"),
    ];
    const result = duplicateScheme("dracula", userSchemes);

    expect(result!.name).toBe("dracula_copy_2");
  });
});

describe("renameUserScheme", () => {
  test("renames scheme to valid name", () => {
    const schemes = [createMockUserScheme("old_name")];
    const result = renameUserScheme(schemes, "old_name", "new_name");

    expect(result.success).toBe(true);
    expect(result.schemes![0].name).toBe("new_name");
  });

  test("rejects empty string", () => {
    const schemes = [createMockUserScheme("theme")];
    const result = renameUserScheme(schemes, "theme", "");

    expect(result.success).toBe(false);
    expect(result.error).toBeDefined();
  });

  test("rejects whitespace-only name", () => {
    const schemes = [createMockUserScheme("theme")];
    const result = renameUserScheme(schemes, "theme", "   ");

    expect(result.success).toBe(false);
  });

  test("rejects duplicate name", () => {
    const schemes = [
      createMockUserScheme("theme1"),
      createMockUserScheme("theme2"),
    ];
    const result = renameUserScheme(schemes, "theme1", "theme2");

    expect(result.success).toBe(false);
  });

  test("rejects rename to preset name", () => {
    const schemes = [createMockUserScheme("my_theme")];
    const result = renameUserScheme(schemes, "my_theme", "dracula");

    expect(result.success).toBe(false);
  });

  test("allows rename to same name (no-op)", () => {
    const schemes = [createMockUserScheme("theme")];
    const result = renameUserScheme(schemes, "theme", "theme");

    expect(result.success).toBe(true);
  });
});

describe("isUserScheme", () => {
  test("returns true for user scheme name", () => {
    const schemes = [createMockUserScheme("my_theme")];
    expect(isUserScheme("my_theme", schemes)).toBe(true);
  });

  test("returns false for preset name", () => {
    const schemes: UserColorScheme[] = [];
    expect(isUserScheme("dracula", schemes)).toBe(false);
  });

  test("returns false for unknown name", () => {
    const schemes = [createMockUserScheme("my_theme")];
    expect(isUserScheme("unknown", schemes)).toBe(false);
  });
});

describe("buildSelectOptions", () => {
  test("orders presets first, user schemes second", () => {
    const userSchemes = [
      createMockUserScheme("my_theme"),
      createMockUserScheme("another_theme"),
    ];
    const options = buildSelectOptions(userSchemes);

    // Find indices
    const emtermIndex = options.findIndex((o) => o.value === "emterm");
    const draculaIndex = options.findIndex((o) => o.value === "dracula");
    const myThemeIndex = options.findIndex((o) => o.value === "my_theme");

    expect(emtermIndex).toBeLessThan(myThemeIndex);
    expect(draculaIndex).toBeLessThan(myThemeIndex);
  });

  test("adds [User] suffix to user scheme labels", () => {
    const userSchemes = [createMockUserScheme("my_theme")];
    const options = buildSelectOptions(userSchemes);

    const userOption = options.find((o) => o.value === "my_theme");
    expect(userOption?.label).toContain("[User]");
  });

  test("presets do not have [User] suffix", () => {
    const options = buildSelectOptions([]);

    const presetOption = options.find((o) => o.value === "dracula");
    expect(presetOption?.label).not.toContain("[User]");
  });

  test("includes all presets", () => {
    const options = buildSelectOptions([]);
    const presetNames = COLOR_SCHEME_PRESETS.map((p) => p.name);

    for (const name of presetNames) {
      expect(options.find((o) => o.value === name)).toBeDefined();
    }
  });

  test("preserves preset order", () => {
    const options = buildSelectOptions([]);
    const presetValues = options
      .filter((o) => !o.label.includes("[User]"))
      .map((o) => o.value);

    expect(presetValues[0]).toBe("emterm");
    // Order: emterm, solarized-dark, solarized-light, monokai, dracula, nord
  });
});
