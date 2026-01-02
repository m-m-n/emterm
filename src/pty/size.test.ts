/**
 * Tests for terminal size calculation utilities.
 */

import { describe, it, expect, beforeEach, afterEach, mock } from "bun:test";
import { calculateTerminalSize, measureCharacterSize } from "./size";

// Mock getComputedStyle
const originalGetComputedStyle = globalThis.getComputedStyle;

describe("calculateTerminalSize", () => {
  beforeEach(() => {
    // Mock getComputedStyle to return padding values
    globalThis.getComputedStyle = mock(() => ({
      paddingLeft: "10px",
      paddingRight: "10px",
      paddingTop: "5px",
      paddingBottom: "5px",
    })) as unknown as typeof getComputedStyle;
  });

  afterEach(() => {
    globalThis.getComputedStyle = originalGetComputedStyle;
  });

  it("should calculate correct columns and rows", () => {
    const container = {
      clientWidth: 820, // 820 - 20 (padding) = 800 available
      clientHeight: 510, // 510 - 10 (padding) = 500 available
    } as HTMLElement;

    const result = calculateTerminalSize(container, 10, 20);

    // 800 / 10 = 80 cols, 500 / 20 = 25 rows
    expect(result.cols).toBe(80);
    expect(result.rows).toBe(25);
  });

  it("should return at least 1 column and 1 row", () => {
    const container = {
      clientWidth: 15, // Less than padding + one char
      clientHeight: 8,
    } as HTMLElement;

    const result = calculateTerminalSize(container, 10, 20);

    expect(result.cols).toBe(1);
    expect(result.rows).toBe(1);
  });

  it("should floor fractional values", () => {
    const container = {
      clientWidth: 135, // 135 - 20 = 115 -> 115 / 10 = 11.5 -> 11
      clientHeight: 77, // 77 - 10 = 67 -> 67 / 20 = 3.35 -> 3
    } as HTMLElement;

    const result = calculateTerminalSize(container, 10, 20);

    expect(result.cols).toBe(11);
    expect(result.rows).toBe(3);
  });

  it("should handle zero padding", () => {
    globalThis.getComputedStyle = mock(() => ({
      paddingLeft: "0px",
      paddingRight: "0px",
      paddingTop: "0px",
      paddingBottom: "0px",
    })) as unknown as typeof getComputedStyle;

    const container = {
      clientWidth: 800,
      clientHeight: 400,
    } as HTMLElement;

    const result = calculateTerminalSize(container, 8, 16);

    expect(result.cols).toBe(100); // 800 / 8
    expect(result.rows).toBe(25); // 400 / 16
  });
});

describe("measureCharacterSize", () => {
  it("should return width and height", () => {
    const result = measureCharacterSize("monospace", 14);

    // Check that we get reasonable values
    expect(result.width).toBeGreaterThan(0);
    expect(result.height).toBeGreaterThan(0);
  });

  it("should calculate height as 1.2x font size", () => {
    const result = measureCharacterSize("monospace", 20);

    // Height should be fontSize * 1.2 = 24
    expect(result.height).toBe(24);
  });

  it("should work with different font families", () => {
    const mono = measureCharacterSize("monospace", 14);
    const consolas = measureCharacterSize("Consolas, monospace", 14);

    // Both should return valid dimensions
    expect(mono.width).toBeGreaterThan(0);
    expect(consolas.width).toBeGreaterThan(0);
  });
});
