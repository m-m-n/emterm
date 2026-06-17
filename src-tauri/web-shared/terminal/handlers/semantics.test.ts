/**
 * Tests for ANSI sequence semantics utilities.
 */

import { describe, expect, test } from "bun:test";
import {
  CSI_DEFAULTS,
  toZeroIndexed,
  clampPosition,
} from "./semantics.ts";

describe("CSI_DEFAULTS", () => {
  test("should have default value 1 for cursor movement actions", () => {
    expect(CSI_DEFAULTS.CursorUp).toBe(1);
    expect(CSI_DEFAULTS.CursorDown).toBe(1);
    expect(CSI_DEFAULTS.CursorForward).toBe(1);
    expect(CSI_DEFAULTS.CursorBack).toBe(1);
    expect(CSI_DEFAULTS.CursorNextLine).toBe(1);
    expect(CSI_DEFAULTS.CursorPreviousLine).toBe(1);
  });

  test("should have default value 1 for absolute positioning", () => {
    expect(CSI_DEFAULTS.CursorHorizontalAbsolute).toBe(1);
    expect(CSI_DEFAULTS.CursorVerticalAbsolute).toBe(1);
  });

  test("should have default value 1 for erase and edit operations", () => {
    expect(CSI_DEFAULTS.EraseCharacters).toBe(1);
    expect(CSI_DEFAULTS.InsertLines).toBe(1);
    expect(CSI_DEFAULTS.DeleteLines).toBe(1);
    expect(CSI_DEFAULTS.InsertCharacters).toBe(1);
    expect(CSI_DEFAULTS.DeleteCharacters).toBe(1);
  });

  test("should have default value 1 for scroll operations", () => {
    expect(CSI_DEFAULTS.ScrollUp).toBe(1);
    expect(CSI_DEFAULTS.ScrollDown).toBe(1);
  });
});

describe("toZeroIndexed", () => {
  test("should convert 1-indexed ANSI value to 0-indexed", () => {
    expect(toZeroIndexed(1)).toBe(0);
    expect(toZeroIndexed(5)).toBe(4);
    expect(toZeroIndexed(80)).toBe(79);
  });

  test("should treat 0 as 1 (default ANSI behavior)", () => {
    // ANSI treats 0 the same as 1 for many commands
    expect(toZeroIndexed(0)).toBe(0);
  });

  test("should handle undefined as 1", () => {
    expect(toZeroIndexed(undefined)).toBe(0);
  });

  test("should handle negative values by clamping to 0", () => {
    expect(toZeroIndexed(-1)).toBe(0);
    expect(toZeroIndexed(-100)).toBe(0);
  });
});

describe("clampPosition", () => {
  test("should clamp column within bounds", () => {
    const result = clampPosition(100, 5, 80, 24);
    expect(result.col).toBe(79); // 0-indexed, max is cols-1
    expect(result.row).toBe(5);
  });

  test("should clamp row within bounds", () => {
    const result = clampPosition(5, 100, 80, 24);
    expect(result.col).toBe(5);
    expect(result.row).toBe(23); // 0-indexed, max is rows-1
  });

  test("should clamp negative values to 0", () => {
    const result = clampPosition(-5, -10, 80, 24);
    expect(result.col).toBe(0);
    expect(result.row).toBe(0);
  });

  test("should not modify values within bounds", () => {
    const result = clampPosition(40, 12, 80, 24);
    expect(result.col).toBe(40);
    expect(result.row).toBe(12);
  });

  test("should handle boundary values correctly", () => {
    // At max allowed position
    const result = clampPosition(79, 23, 80, 24);
    expect(result.col).toBe(79);
    expect(result.row).toBe(23);
  });
});
