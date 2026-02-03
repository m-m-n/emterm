/**
 * Tests for terminal color palette and utilities.
 */
import { describe, expect, it } from "bun:test";
import {
	brightColorToRgb,
	COLOR_SCHEME_PRESETS,
	DEFAULT_BACKGROUND,
	DEFAULT_FOREGROUND,
	getColorSchemePreset,
	indexToCSS,
	indexToRgb,
	PALETTE_16,
	PALETTE_256,
	type Rgb,
	rgbToCSS,
	type SgrColor,
	sgrColorToRgb,
	standardColorToRgb,
} from "./colors.ts";

describe("PALETTE_16", () => {
	it("should have 16 colors", () => {
		expect(PALETTE_16.length).toBe(16);
	});

	it("should have black as first color", () => {
		expect(PALETTE_16[0]).toEqual({ r: 0, g: 0, b: 0 });
	});

	it("should have WezTerm white as last standard color", () => {
		// WezTerm white: #dedacf
		expect(PALETTE_16[7]).toEqual({ r: 0xde, g: 0xda, b: 0xcf });
	});

	it("should have bright white as last color", () => {
		expect(PALETTE_16[15]).toEqual({ r: 255, g: 255, b: 255 });
	});

	it("should have WezTerm red color", () => {
		// WezTerm red: #ff0000
		expect(PALETTE_16[1]).toEqual({ r: 0xff, g: 0x00, b: 0x00 });
	});

	it("should have WezTerm green color", () => {
		// WezTerm green: #00dd00
		expect(PALETTE_16[2]).toEqual({ r: 0x00, g: 0xdd, b: 0x00 });
	});

	it("should have WezTerm yellow color", () => {
		// WezTerm yellow: #eeee00
		expect(PALETTE_16[3]).toEqual({ r: 0xee, g: 0xee, b: 0x00 });
	});

	it("should have WezTerm blue color", () => {
		// WezTerm blue: #4040ff
		expect(PALETTE_16[4]).toEqual({ r: 0x40, g: 0x40, b: 0xff });
	});

	it("should have WezTerm magenta color", () => {
		// WezTerm magenta: #ff00ff
		expect(PALETTE_16[5]).toEqual({ r: 0xff, g: 0x00, b: 0xff });
	});

	it("should have WezTerm cyan color", () => {
		// WezTerm cyan: #00dddd
		expect(PALETTE_16[6]).toEqual({ r: 0x00, g: 0xdd, b: 0xdd });
	});

	it("should have WezTerm bright black color", () => {
		// WezTerm bright black: #555555
		expect(PALETTE_16[8]).toEqual({ r: 0x55, g: 0x55, b: 0x55 });
	});

	it("should have WezTerm bright red color", () => {
		// WezTerm bright red: #ff6060
		expect(PALETTE_16[9]).toEqual({ r: 0xff, g: 0x60, b: 0x60 });
	});

	it("should have WezTerm bright green color", () => {
		// WezTerm bright green: #60ff60
		expect(PALETTE_16[10]).toEqual({ r: 0x60, g: 0xff, b: 0x60 });
	});

	it("should have WezTerm bright yellow color", () => {
		// WezTerm bright yellow: #ffff60
		expect(PALETTE_16[11]).toEqual({ r: 0xff, g: 0xff, b: 0x60 });
	});

	it("should have WezTerm bright blue color", () => {
		// WezTerm bright blue: #6060ff
		expect(PALETTE_16[12]).toEqual({ r: 0x60, g: 0x60, b: 0xff });
	});

	it("should have WezTerm bright magenta color", () => {
		// WezTerm bright magenta: #ff60ff
		expect(PALETTE_16[13]).toEqual({ r: 0xff, g: 0x60, b: 0xff });
	});

	it("should have WezTerm bright cyan color", () => {
		// WezTerm bright cyan: #60ffff
		expect(PALETTE_16[14]).toEqual({ r: 0x60, g: 0xff, b: 0xff });
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
	it("should be WezTerm bright green", () => {
		// WezTerm foreground: #40ff40
		expect(DEFAULT_FOREGROUND).toEqual({ r: 0x40, g: 0xff, b: 0x40 });
	});
});

describe("DEFAULT_BACKGROUND", () => {
	it("should be black", () => {
		expect(DEFAULT_BACKGROUND).toEqual({ r: 0, g: 0, b: 0 });
	});
});

describe("COLOR_SCHEME_PRESETS", () => {
	it("should have 6 presets", () => {
		expect(COLOR_SCHEME_PRESETS.length).toBe(6);
	});

	it("should include all required presets", () => {
		const names = COLOR_SCHEME_PRESETS.map((p) => p.name);
		expect(names).toContain("emterm");
		expect(names).toContain("solarized-dark");
		expect(names).toContain("solarized-light");
		expect(names).toContain("monokai");
		expect(names).toContain("dracula");
		expect(names).toContain("nord");
	});

	it("should have 16 ANSI colors in each preset", () => {
		for (const preset of COLOR_SCHEME_PRESETS) {
			expect(preset.ansiColors.length).toBe(16);
		}
	});

	it("emterm preset should use default foreground", () => {
		const emterm = COLOR_SCHEME_PRESETS.find((p) => p.name === "emterm");
		expect(emterm?.foreground).toEqual(DEFAULT_FOREGROUND);
	});
});

describe("getColorSchemePreset", () => {
	it("should return preset for valid name", () => {
		const preset = getColorSchemePreset("emterm");
		expect(preset).toBeDefined();
		expect(preset?.name).toBe("emterm");
	});

	it("should return undefined for unknown name", () => {
		const preset = getColorSchemePreset("unknown-scheme");
		expect(preset).toBeUndefined();
	});

	it("should return all presets by name", () => {
		expect(getColorSchemePreset("emterm")).toBeDefined();
		expect(getColorSchemePreset("solarized-dark")).toBeDefined();
		expect(getColorSchemePreset("solarized-light")).toBeDefined();
		expect(getColorSchemePreset("monokai")).toBeDefined();
		expect(getColorSchemePreset("dracula")).toBeDefined();
		expect(getColorSchemePreset("nord")).toBeDefined();
	});
});

// ============================================================
// Hex Conversion Utilities (Phase 1)
// ============================================================

import { hexToRgb, rgbToHex, validateHexColor } from "./colors.ts";

describe("hexToRgb", () => {
	it("parses valid #RRGGBB format", () => {
		expect(hexToRgb("#ff0000")).toEqual({ r: 255, g: 0, b: 0 });
		expect(hexToRgb("#00ff00")).toEqual({ r: 0, g: 255, b: 0 });
		expect(hexToRgb("#0000ff")).toEqual({ r: 0, g: 0, b: 255 });
		expect(hexToRgb("#ffffff")).toEqual({ r: 255, g: 255, b: 255 });
		expect(hexToRgb("#000000")).toEqual({ r: 0, g: 0, b: 0 });
	});

	it("parses uppercase hex", () => {
		expect(hexToRgb("#FF0000")).toEqual({ r: 255, g: 0, b: 0 });
		expect(hexToRgb("#AABBCC")).toEqual({ r: 170, g: 187, b: 204 });
	});

	it("parses mixed case hex", () => {
		expect(hexToRgb("#AaBbCc")).toEqual({ r: 170, g: 187, b: 204 });
	});

	it("returns null for invalid formats", () => {
		expect(hexToRgb("")).toBeNull();
		expect(hexToRgb("ff0000")).toBeNull();
		expect(hexToRgb("#ff000")).toBeNull();
		expect(hexToRgb("#ff00000")).toBeNull();
		expect(hexToRgb("#gggggg")).toBeNull();
		expect(hexToRgb("red")).toBeNull();
		expect(hexToRgb("#fff")).toBeNull();
	});
});

describe("rgbToHex", () => {
	it("formats Rgb to lowercase #rrggbb", () => {
		expect(rgbToHex({ r: 255, g: 0, b: 0 })).toBe("#ff0000");
		expect(rgbToHex({ r: 0, g: 255, b: 0 })).toBe("#00ff00");
		expect(rgbToHex({ r: 0, g: 0, b: 255 })).toBe("#0000ff");
		expect(rgbToHex({ r: 255, g: 255, b: 255 })).toBe("#ffffff");
		expect(rgbToHex({ r: 0, g: 0, b: 0 })).toBe("#000000");
	});

	it("pads single-digit hex values with zero", () => {
		expect(rgbToHex({ r: 1, g: 2, b: 3 })).toBe("#010203");
		expect(rgbToHex({ r: 15, g: 15, b: 15 })).toBe("#0f0f0f");
	});

	it("handles specific color values", () => {
		expect(rgbToHex({ r: 170, g: 187, b: 204 })).toBe("#aabbcc");
		expect(rgbToHex({ r: 64, g: 255, b: 64 })).toBe("#40ff40");
	});
});

describe("hex conversion round-trip", () => {
	it("rgbToHex(hexToRgb(hex)) returns original hex (lowercase)", () => {
		const testCases = [
			"#ff0000",
			"#00ff00",
			"#0000ff",
			"#ffffff",
			"#000000",
			"#aabbcc",
			"#123456",
			"#fedcba",
		];

		for (const hex of testCases) {
			const rgb = hexToRgb(hex);
			expect(rgb).not.toBeNull();
			expect(rgbToHex(rgb!)).toBe(hex.toLowerCase());
		}
	});

	it("hexToRgb(rgbToHex(rgb)) returns original rgb", () => {
		const testCases: Rgb[] = [
			{ r: 255, g: 0, b: 0 },
			{ r: 0, g: 255, b: 0 },
			{ r: 0, g: 0, b: 255 },
			{ r: 170, g: 187, b: 204 },
			{ r: 0, g: 0, b: 0 },
			{ r: 255, g: 255, b: 255 },
		];

		for (const rgb of testCases) {
			const hex = rgbToHex(rgb);
			const result = hexToRgb(hex);
			expect(result).toEqual(rgb);
		}
	});
});

describe("validateHexColor", () => {
	it("accepts valid #RRGGBB format", () => {
		expect(validateHexColor("#ff0000")).toBe(true);
		expect(validateHexColor("#00ff00")).toBe(true);
		expect(validateHexColor("#0000ff")).toBe(true);
		expect(validateHexColor("#aabbcc")).toBe(true);
		expect(validateHexColor("#AABBCC")).toBe(true);
		expect(validateHexColor("#123456")).toBe(true);
	});

	it("rejects invalid formats", () => {
		expect(validateHexColor("")).toBe(false);
		expect(validateHexColor("ff0000")).toBe(false);
		expect(validateHexColor("#ff000")).toBe(false);
		expect(validateHexColor("#ff00000")).toBe(false);
		expect(validateHexColor("#gggggg")).toBe(false);
		expect(validateHexColor("red")).toBe(false);
		expect(validateHexColor("#fff")).toBe(false);
		expect(validateHexColor("#f00")).toBe(false);
	});
});
