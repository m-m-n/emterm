/**
 * Tests for Font Service
 */

import { afterEach, describe, test, expect, beforeEach, mock } from "bun:test";
import type { FontListResponse } from "./types";

const mockFontList: FontListResponse = {
  monospace_fonts: ["Courier New", "Fira Code", "JetBrains Mono"],
  all_fonts: ["Arial", "Courier New", "Fira Code", "JetBrains Mono", "Noto Sans JP"],
  emoji_fonts: ["Noto Color Emoji"],
};

let invokeCallCount = 0;

// Mock @tauri-apps/api/core before importing
mock.module("@tauri-apps/api/core", () => ({
  invoke: async (cmd: string) => {
    if (cmd === "list_fonts") {
      invokeCallCount++;
      return mockFontList;
    }
    throw new Error(`Unknown command: ${cmd}`);
  },
}));

// Import after mock setup
const { FontService } = await import("./font-service");

describe("FontService", () => {
  beforeEach(() => {
    invokeCallCount = 0;
    FontService.resetCache();
  });

  test("list() calls invoke('list_fonts') on first call", async () => {
    const result = await FontService.list();
    expect(invokeCallCount).toBe(1);
    expect(result).toEqual(mockFontList);
  });

  test("list() returns cached result on second call without invoking again", async () => {
    await FontService.list();
    expect(invokeCallCount).toBe(1);

    const result = await FontService.list();
    expect(invokeCallCount).toBe(1);
    expect(result).toEqual(mockFontList);
  });

  test("list() returns correct structure with three arrays", async () => {
    const result = await FontService.list();
    expect(Array.isArray(result.monospace_fonts)).toBe(true);
    expect(Array.isArray(result.all_fonts)).toBe(true);
    expect(Array.isArray(result.emoji_fonts)).toBe(true);
  });

  test("resetCache() clears the cache so next list() calls invoke again", async () => {
    await FontService.list();
    expect(invokeCallCount).toBe(1);

    FontService.resetCache();
    await FontService.list();
    expect(invokeCallCount).toBe(2);
  });
});
