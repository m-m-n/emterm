import { describe, test, expect } from "bun:test";
import {
  parseColorSpec,
  formatColorResponse,
  OscColorHandler,
  type ColorSpecResult,
} from "./osc-colors.ts";

// ── parseColorSpec tests ────────────────────────────────

describe("parseColorSpec", () => {
  test("should parse query token", () => {
    expect(parseColorSpec("?")).toEqual({ type: "query" });
  });

  test("should parse rgb:r/g/b with 2-digit components", () => {
    expect(parseColorSpec("rgb:ff/00/80")).toEqual({
      type: "color",
      r: 0xff,
      g: 0x00,
      b: 0x80,
    });
  });

  test("should parse rgb:r/g/b with 4-digit components (downscale)", () => {
    expect(parseColorSpec("rgb:ffff/0000/8080")).toEqual({
      type: "color",
      r: 0xff,
      g: 0x00,
      b: 0x80,
    });
  });

  test("should parse rgb:r/g/b with 1-digit components", () => {
    expect(parseColorSpec("rgb:f/0/8")).toEqual({
      type: "color",
      r: 0xff,
      g: 0x00,
      b: 0x88,
    });
  });

  test("should parse #RGB format", () => {
    expect(parseColorSpec("#F08")).toEqual({
      type: "color",
      r: 0xff,
      g: 0x00,
      b: 0x88,
    });
  });

  test("should parse #RRGGBB format", () => {
    expect(parseColorSpec("#ff0080")).toEqual({
      type: "color",
      r: 0xff,
      g: 0x00,
      b: 0x80,
    });
  });

  test("should parse #RRRRGGGGBBBB format", () => {
    expect(parseColorSpec("#ffff00008080")).toEqual({
      type: "color",
      r: 0xff,
      g: 0x00,
      b: 0x80,
    });
  });

  test("should return null for invalid formats", () => {
    expect(parseColorSpec("")).toBeNull();
    expect(parseColorSpec("invalid")).toBeNull();
    expect(parseColorSpec("rgb:")).toBeNull();
    expect(parseColorSpec("#12345")).toBeNull();
  });
});

// ── formatColorResponse tests ───────────────────────────

describe("formatColorResponse", () => {
  test("should format black", () => {
    expect(formatColorResponse(0, 0, 0)).toBe("rgb:0000/0000/0000");
  });

  test("should format white", () => {
    expect(formatColorResponse(255, 255, 255)).toBe("rgb:ffff/ffff/ffff");
  });

  test("should format mixed color", () => {
    expect(formatColorResponse(0x80, 0x40, 0xc0)).toBe("rgb:8080/4040/c0c0");
  });

  test("should roundtrip correctly", () => {
    const response = formatColorResponse(0xab, 0xcd, 0xef);
    const parsed = parseColorSpec(response);
    expect(parsed).toEqual({ type: "color", r: 0xab, g: 0xcd, b: 0xef });
  });
});

// ── OscColorHandler tests ───────────────────────────────

describe("OscColorHandler", () => {
  test("should set and get palette overlay entry", () => {
    const handler = new OscColorHandler();
    handler.setPaletteEntry(42, 0xff, 0x00, 0x80);
    const entry = handler.getPaletteEntry(42);
    expect(entry).toEqual({ r: 0xff, g: 0x00, b: 0x80 });
  });

  test("should return null for unset palette entry", () => {
    const handler = new OscColorHandler();
    expect(handler.getPaletteEntry(42)).toBeNull();
  });

  test("should reset single palette entry", () => {
    const handler = new OscColorHandler();
    handler.setPaletteEntry(42, 0xff, 0x00, 0x80);
    handler.resetPaletteEntry(42);
    expect(handler.getPaletteEntry(42)).toBeNull();
  });

  test("should reset all palette entries", () => {
    const handler = new OscColorHandler();
    handler.setPaletteEntry(0, 0xff, 0, 0);
    handler.setPaletteEntry(1, 0, 0xff, 0);
    handler.resetAllPaletteEntries();
    expect(handler.getPaletteEntry(0)).toBeNull();
    expect(handler.getPaletteEntry(1)).toBeNull();
  });

  test("should set and get default foreground override", () => {
    const handler = new OscColorHandler();
    handler.setForeground(0xff, 0x00, 0x80);
    expect(handler.getForeground()).toEqual({ r: 0xff, g: 0x00, b: 0x80 });
  });

  test("should reset foreground override", () => {
    const handler = new OscColorHandler();
    handler.setForeground(0xff, 0x00, 0x80);
    handler.resetForeground();
    expect(handler.getForeground()).toBeNull();
  });

  test("should set and get default background override", () => {
    const handler = new OscColorHandler();
    handler.setBackground(0x28, 0x2a, 0x36);
    expect(handler.getBackground()).toEqual({ r: 0x28, g: 0x2a, b: 0x36 });
  });

  test("should reset background override", () => {
    const handler = new OscColorHandler();
    handler.setBackground(0x28, 0x2a, 0x36);
    handler.resetBackground();
    expect(handler.getBackground()).toBeNull();
  });

  test("should set and get cursor color override", () => {
    const handler = new OscColorHandler();
    handler.setCursorColor(0x00, 0xff, 0x00);
    expect(handler.getCursorColor()).toEqual({ r: 0x00, g: 0xff, b: 0x00 });
  });

  test("should reset cursor color override", () => {
    const handler = new OscColorHandler();
    handler.setCursorColor(0x00, 0xff, 0x00);
    handler.resetCursorColor();
    expect(handler.getCursorColor()).toBeNull();
  });

  test("should parse OSC 4 set command", () => {
    const handler = new OscColorHandler();
    const responses: string[] = [];
    handler.handleOsc4("42;rgb:ff/00/80", (resp) => responses.push(resp));
    expect(handler.getPaletteEntry(42)).toEqual({ r: 0xff, g: 0x00, b: 0x80 });
    expect(responses).toEqual([]);
  });

  test("should parse OSC 4 query command", () => {
    const handler = new OscColorHandler();
    handler.setPaletteEntry(42, 0xff, 0x00, 0x80);
    const responses: string[] = [];
    handler.handleOsc4("42;?", (resp) => responses.push(resp));
    expect(responses.length).toBe(1);
    expect(responses[0]).toContain("4;42;rgb:ffff/0000/8080");
  });

  test("should parse OSC 4 chained pairs", () => {
    const handler = new OscColorHandler();
    const responses: string[] = [];
    handler.handleOsc4("10;rgb:ff/00/00;11;rgb:00/ff/00", (resp) => responses.push(resp));
    expect(handler.getPaletteEntry(10)).toEqual({ r: 0xff, g: 0x00, b: 0x00 });
    expect(handler.getPaletteEntry(11)).toEqual({ r: 0x00, g: 0xff, b: 0x00 });
  });

  test("should handle OSC 10/11/12 chaining (set fg, bg, cursor in one sequence)", () => {
    const handler = new OscColorHandler();
    const responses: string[] = [];
    // OSC 10 with 3 specs: sets fg (10), bg (11), cursor (12)
    handler.handleOscDefaultColor(10, "rgb:ff/00/00;rgb:00/ff/00;rgb:00/00/ff", (resp) => responses.push(resp));
    expect(handler.getForeground()).toEqual({ r: 0xff, g: 0x00, b: 0x00 });
    expect(handler.getBackground()).toEqual({ r: 0x00, g: 0xff, b: 0x00 });
    expect(handler.getCursorColor()).toEqual({ r: 0x00, g: 0x00, b: 0xff });
  });

  test("should handle OSC 10 query", () => {
    const handler = new OscColorHandler();
    handler.setForeground(0x40, 0xff, 0x40);
    const responses: string[] = [];
    handler.handleOscDefaultColor(10, "?", (resp) => responses.push(resp));
    expect(responses.length).toBe(1);
    expect(responses[0]).toContain("10;rgb:4040/ffff/4040");
  });
});
