/**
 * Tests for platform detection helper.
 */

import { describe, test, expect, beforeEach } from "bun:test";
import {
  isLinux,
  isWindows,
  _resetPlatformCacheForTests,
  _setPlatformCacheForTests,
} from "./platform";

describe("platform detection", () => {
  beforeEach(() => {
    _resetPlatformCacheForTests();
  });

  describe("isLinux", () => {
    test("returns false before the cache is populated", () => {
      expect(isLinux()).toBe(false);
    });

    test("returns true when the cached platform is 'linux'", () => {
      _setPlatformCacheForTests("linux");
      expect(isLinux()).toBe(true);
    });

    test("returns false when the cached platform is 'windows'", () => {
      _setPlatformCacheForTests("windows");
      expect(isLinux()).toBe(false);
    });

    test("returns false when the cache is the empty string (resolution failed)", () => {
      _setPlatformCacheForTests("");
      expect(isLinux()).toBe(false);
    });
  });

  describe("isWindows", () => {
    test("returns false before the cache is populated", () => {
      expect(isWindows()).toBe(false);
    });

    test("returns true when the cached platform is 'windows'", () => {
      _setPlatformCacheForTests("windows");
      expect(isWindows()).toBe(true);
    });

    test("returns false when the cached platform is 'linux'", () => {
      _setPlatformCacheForTests("linux");
      expect(isWindows()).toBe(false);
    });
  });

  describe("isLinux / isWindows mutual exclusion", () => {
    test("isLinux and isWindows are never both true", () => {
      _setPlatformCacheForTests("linux");
      expect(isLinux() && isWindows()).toBe(false);
      _setPlatformCacheForTests("windows");
      expect(isLinux() && isWindows()).toBe(false);
      _setPlatformCacheForTests("macos");
      expect(isLinux() && isWindows()).toBe(false);
    });
  });
});
