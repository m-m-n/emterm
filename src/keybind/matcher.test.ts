/**
 * Tests for keybind matcher
 */

import { describe, test, expect } from "bun:test";
import { parseKeybind, matchKeybind, matchKeybindStr } from "./matcher";

describe("parseKeybind", () => {
  test("should parse Ctrl+T", () => {
    const result = parseKeybind("Ctrl+T");
    expect(result.ctrlKey).toBe(true);
    expect(result.shiftKey).toBe(false);
    expect(result.altKey).toBe(false);
    expect(result.metaKey).toBe(false);
    expect(result.key).toBe("T");
  });

  test("should parse Ctrl+Shift+T", () => {
    const result = parseKeybind("Ctrl+Shift+T");
    expect(result.ctrlKey).toBe(true);
    expect(result.shiftKey).toBe(true);
    expect(result.key).toBe("T");
  });

  test("should parse single key F11", () => {
    const result = parseKeybind("F11");
    expect(result.ctrlKey).toBe(false);
    expect(result.shiftKey).toBe(false);
    expect(result.key).toBe("F11");
  });

  test("should parse Ctrl+Plus", () => {
    const result = parseKeybind("Ctrl+Plus");
    expect(result.ctrlKey).toBe(true);
    expect(result.key).toBe("+");
  });

  test("should parse Ctrl+Minus", () => {
    const result = parseKeybind("Ctrl+Minus");
    expect(result.ctrlKey).toBe(true);
    expect(result.key).toBe("-");
  });

  test("should parse Ctrl+0", () => {
    const result = parseKeybind("Ctrl+0");
    expect(result.ctrlKey).toBe(true);
    expect(result.key).toBe("0");
  });

  test("should parse Ctrl+, (comma key)", () => {
    const result = parseKeybind("Ctrl+,");
    expect(result.ctrlKey).toBe(true);
    expect(result.key).toBe(",");
  });

  test("should parse Ctrl+Comma (legacy format)", () => {
    const result = parseKeybind("Ctrl+Comma");
    expect(result.ctrlKey).toBe(true);
    expect(result.key).toBe(",");
  });

  test("should parse Ctrl+Tab", () => {
    const result = parseKeybind("Ctrl+Tab");
    expect(result.ctrlKey).toBe(true);
    expect(result.key).toBe("Tab");
  });

  test("should parse Ctrl+Shift+Tab", () => {
    const result = parseKeybind("Ctrl+Shift+Tab");
    expect(result.ctrlKey).toBe(true);
    expect(result.shiftKey).toBe(true);
    expect(result.key).toBe("Tab");
  });
});

function makeKeyEvent(overrides: Partial<KeyboardEvent>): KeyboardEvent {
  return {
    key: "",
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    metaKey: false,
    ...overrides,
  } as KeyboardEvent;
}

describe("matchKeybind", () => {
  test("should match Ctrl+Shift+C", () => {
    const keybind = parseKeybind("Ctrl+Shift+C");
    const event = makeKeyEvent({
      key: "c",
      ctrlKey: true,
      shiftKey: true,
    });
    expect(matchKeybind(event, keybind)).toBe(true);
  });

  test("should not match when modifier differs", () => {
    const keybind = parseKeybind("Ctrl+Shift+C");
    const event = makeKeyEvent({
      key: "c",
      ctrlKey: true,
      shiftKey: false,
    });
    expect(matchKeybind(event, keybind)).toBe(false);
  });

  test("should match F11 without modifiers", () => {
    const keybind = parseKeybind("F11");
    const event = makeKeyEvent({ key: "F11" });
    expect(matchKeybind(event, keybind)).toBe(true);
  });

  test("should not match F11 when Ctrl is pressed", () => {
    const keybind = parseKeybind("F11");
    const event = makeKeyEvent({ key: "F11", ctrlKey: true });
    expect(matchKeybind(event, keybind)).toBe(false);
  });

  test("should be case-insensitive for key matching", () => {
    const keybind = parseKeybind("Ctrl+Shift+T");
    const event = makeKeyEvent({
      key: "t",
      ctrlKey: true,
      shiftKey: true,
    });
    expect(matchKeybind(event, keybind)).toBe(true);
  });
});

describe("matchKeybindStr", () => {
  test("should match keybind string directly", () => {
    const event = makeKeyEvent({
      key: "v",
      ctrlKey: true,
      shiftKey: true,
    });
    expect(matchKeybindStr(event, "Ctrl+Shift+V")).toBe(true);
  });

  test("should not match different keybind", () => {
    const event = makeKeyEvent({
      key: "v",
      ctrlKey: true,
      shiftKey: true,
    });
    expect(matchKeybindStr(event, "Ctrl+V")).toBe(false);
  });
});
