/**
 * Tests for effective-settings accessors.
 */

import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import {
  effectiveCopyOnSelect,
  effectiveMiddleClickPaste,
} from "./effective-settings";
import {
  _resetPlatformCacheForTests,
  _setPlatformCacheForTests,
} from "../platform";
import type { AppSettings } from "./types";

// Minimal AppSettings factory — only the fields under test matter; the rest
// are type-filled with reasonable defaults to satisfy the type checker.
function makeSettings(
  overrides: Partial<Pick<AppSettings, "copy_on_select" | "middle_click_paste">>,
): AppSettings {
  return {
    copy_on_select: false,
    middle_click_paste: true,
    ...overrides,
  } as AppSettings;
}

describe("effectiveCopyOnSelect", () => {
  afterEach(() => {
    _resetPlatformCacheForTests();
  });

  describe("Linux", () => {
    beforeEach(() => {
      _setPlatformCacheForTests("linux");
    });

    test("returns false when raw value is true", () => {
      expect(effectiveCopyOnSelect(makeSettings({ copy_on_select: true }))).toBe(false);
    });

    test("returns false when raw value is false", () => {
      expect(effectiveCopyOnSelect(makeSettings({ copy_on_select: false }))).toBe(false);
    });

    test("returns false when settings is null", () => {
      expect(effectiveCopyOnSelect(null)).toBe(false);
    });

    test("returns false when settings is undefined", () => {
      expect(effectiveCopyOnSelect(undefined)).toBe(false);
    });
  });

  describe("Windows", () => {
    beforeEach(() => {
      _setPlatformCacheForTests("windows");
    });

    test("returns true when raw value is true", () => {
      expect(effectiveCopyOnSelect(makeSettings({ copy_on_select: true }))).toBe(true);
    });

    test("returns false when raw value is false", () => {
      expect(effectiveCopyOnSelect(makeSettings({ copy_on_select: false }))).toBe(false);
    });

    test("returns false when settings is null (existing default)", () => {
      expect(effectiveCopyOnSelect(null)).toBe(false);
    });
  });

  describe("Unknown / unresolved platform", () => {
    test("falls through to raw value when cache is empty", () => {
      _resetPlatformCacheForTests();
      expect(effectiveCopyOnSelect(makeSettings({ copy_on_select: true }))).toBe(true);
      expect(effectiveCopyOnSelect(makeSettings({ copy_on_select: false }))).toBe(false);
    });
  });
});

describe("effectiveMiddleClickPaste", () => {
  afterEach(() => {
    _resetPlatformCacheForTests();
  });

  describe("Linux", () => {
    beforeEach(() => {
      _setPlatformCacheForTests("linux");
    });

    test("returns true when raw value is true", () => {
      expect(
        effectiveMiddleClickPaste(makeSettings({ middle_click_paste: true })),
      ).toBe(true);
    });

    test("returns true when raw value is false", () => {
      expect(
        effectiveMiddleClickPaste(makeSettings({ middle_click_paste: false })),
      ).toBe(true);
    });

    test("returns true when settings is null", () => {
      expect(effectiveMiddleClickPaste(null)).toBe(true);
    });

    test("returns true when settings is undefined", () => {
      expect(effectiveMiddleClickPaste(undefined)).toBe(true);
    });
  });

  describe("Windows", () => {
    beforeEach(() => {
      _setPlatformCacheForTests("windows");
    });

    test("returns true when raw value is true", () => {
      expect(
        effectiveMiddleClickPaste(makeSettings({ middle_click_paste: true })),
      ).toBe(true);
    });

    test("returns false when raw value is false", () => {
      expect(
        effectiveMiddleClickPaste(makeSettings({ middle_click_paste: false })),
      ).toBe(false);
    });

    test("returns true when settings is null (existing default)", () => {
      expect(effectiveMiddleClickPaste(null)).toBe(true);
    });

    test("returns true when settings is undefined (existing default)", () => {
      expect(effectiveMiddleClickPaste(undefined)).toBe(true);
    });
  });
});
