/**
 * Tests for terminal color palette and utilities.
 */
import { describe, it, expect } from "bun:test";
import {
  PALETTE_16,
  PALETTE_256,
  indexToRgb,
  standardColorToRgb,
  brightColorToRgb,
  rgbToCSS,
  indexToCSS,
  sgrColorToRgb,
  DEFAULT_FOREGROUND,
  DEFAULT_BACKGROUND,
  type Rgb,
  type SgrColor,
} from "./colors.ts";

describe("PALETTE_16", () => {
  it("should have 16 colors", () => {
    expect(PALETTE_16.length).toBe(16);
  });

  it("should have black as first color", () => {
    expect(PALETTE_16[0]).toEqual({ r: 0, g: 0, b: 0 });
  });

  it("should have white as last standard color", () => {
    expect(PALETTE_16[7]).toEqual({ r: 229, g: 229, b: 229 });
  });

  it("should have bright white as last color", () => {
    expect(PALETTE_16[15]).toEqual({ r: 255, g: 255, b: 255 });
  });
});

describe("PALETTE_256", () => {
  it("should have 256 colors", () => {
    expect(PALETTE_256.length).toBe(256);
  });

  it("should have first 16 colors matching PALETTE_16", () => {
    for (let i = 0; i < 16; i++) {
      expect(PALETTE_256[i]).toEqual(PALETTE_16[i]);
    }
  });

  it("should have color cube starting at index 16", () => {
    // Index 16 is 0,0,0 in the color cube (black)
    expect(PALETTE_256[16]).toEqual({ r: 0, g: 0, b: 0 });
  });

  it("should have correct color cube values", () => {
    // Index 231 is the last color cube entry (5,5,5 = white)
    expect(PALETTE_256[231]).toEqual({ r: 255, g: 255, b: 255 });
  });

  it("should have grayscale ramp starting at index 232", () => {
    // Index 232 is the darkest gray (8, 8, 8)
    expect(PALETTE_256[232]).toEqual({ r: 8, g: 8, b: 8 });
  });

  it("should have grayscale ramp ending at index 255", () => {
    // Index 255 is the lightest gray (238, 238, 238)
    expect(PALETTE_256[255]).toEqual({ r: 238, g: 238, b: 238 });
  });
});

describe("indexToRgb", () => {
  it("should return correct color for standard colors", () => {
    expect(indexToRgb(0)).toEqual(PALETTE_16[0]);
    expect(indexToRgb(1)).toEqual(PALETTE_16[1]);
    expect(indexToRgb(15)).toEqual(PALETTE_16[15]);
  });

  it("should return correct color for color cube", () => {
    expect(indexToRgb(16)).toEqual({ r: 0, g: 0, b: 0 });
    expect(indexToRgb(196)).toEqual({ r: 255, g: 0, b: 0 }); // Bright red
  });

  it("should return black for invalid indices", () => {
    expect(indexToRgb(-1)).toEqual({ r: 0, g: 0, b: 0 });
    expect(indexToRgb(256)).toEqual({ r: 0, g: 0, b: 0 });
  });
});

describe("standardColorToRgb", () => {
  it("should return correct standard colors", () => {
    expect(standardColorToRgb(0)).toEqual(PALETTE_16[0]); // Black
    expect(standardColorToRgb(1)).toEqual(PALETTE_16[1]); // Red
    expect(standardColorToRgb(7)).toEqual(PALETTE_16[7]); // White
  });

  it("should return black for invalid indices", () => {
    expect(standardColorToRgb(-1)).toEqual({ r: 0, g: 0, b: 0 });
    expect(standardColorToRgb(8)).toEqual({ r: 0, g: 0, b: 0 });
  });
});

describe("brightColorToRgb", () => {
  it("should return correct bright colors", () => {
    expect(brightColorToRgb(0)).toEqual(PALETTE_16[8]); // Bright black
    expect(brightColorToRgb(1)).toEqual(PALETTE_16[9]); // Bright red
    expect(brightColorToRgb(7)).toEqual(PALETTE_16[15]); // Bright white
  });

  it("should return black for invalid indices", () => {
    expect(brightColorToRgb(-1)).toEqual({ r: 0, g: 0, b: 0 });
    expect(brightColorToRgb(8)).toEqual({ r: 0, g: 0, b: 0 });
  });
});

describe("rgbToCSS", () => {
  it("should format RGB as CSS string", () => {
    expect(rgbToCSS({ r: 255, g: 0, b: 0 })).toBe("rgb(255, 0, 0)");
    expect(rgbToCSS({ r: 0, g: 255, b: 0 })).toBe("rgb(0, 255, 0)");
    expect(rgbToCSS({ r: 0, g: 0, b: 255 })).toBe("rgb(0, 0, 255)");
    expect(rgbToCSS({ r: 128, g: 64, b: 32 })).toBe("rgb(128, 64, 32)");
  });
});

describe("indexToCSS", () => {
  it("should convert index directly to CSS", () => {
    expect(indexToCSS(0)).toBe(rgbToCSS(PALETTE_16[0]!));
    expect(indexToCSS(196)).toBe("rgb(255, 0, 0)");
  });
});

describe("sgrColorToRgb", () => {
  it("should handle Standard colors", () => {
    const color: SgrColor = { type: "Standard", value: 1 };
    expect(sgrColorToRgb(color)).toEqual(PALETTE_16[1]);
  });

  it("should handle Bright colors", () => {
    const color: SgrColor = { type: "Bright", value: 1 };
    expect(sgrColorToRgb(color)).toEqual(PALETTE_16[9]);
  });

  it("should handle Indexed colors", () => {
    const color: SgrColor = { type: "Indexed", value: 196 };
    expect(sgrColorToRgb(color)).toEqual({ r: 255, g: 0, b: 0 });
  });

  it("should handle RGB colors", () => {
    const color: SgrColor = { type: "Rgb", value: { r: 123, g: 45, b: 67 } };
    expect(sgrColorToRgb(color)).toEqual({ r: 123, g: 45, b: 67 });
  });
});

describe("DEFAULT_FOREGROUND", () => {
  it("should be light gray", () => {
    expect(DEFAULT_FOREGROUND).toEqual({ r: 229, g: 229, b: 229 });
  });
});

describe("DEFAULT_BACKGROUND", () => {
  it("should be black", () => {
    expect(DEFAULT_BACKGROUND).toEqual({ r: 0, g: 0, b: 0 });
  });
});
