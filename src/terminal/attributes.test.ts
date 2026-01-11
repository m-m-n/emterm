/**
 * Tests for cell attributes.
 */
import { describe, expect, test } from "bun:test";
import {
	applySgrAttr,
	applySgrAttrs,
	attributesEqual,
	type CellAttributes,
	cloneAttributes,
	createDefaultAttributes,
	DEFAULT_ATTRIBUTES,
	getEffectiveBackground,
	getEffectiveForeground,
	type SgrAttr,
} from "./attributes.ts";
import { DEFAULT_BACKGROUND, DEFAULT_FOREGROUND } from "./colors.ts";

describe("DEFAULT_ATTRIBUTES", () => {
	test("has default values", () => {
		expect(DEFAULT_ATTRIBUTES.bold).toBe(false);
		expect(DEFAULT_ATTRIBUTES.dim).toBe(false);
		expect(DEFAULT_ATTRIBUTES.italic).toBe(false);
		expect(DEFAULT_ATTRIBUTES.underline).toBe(false);
		expect(DEFAULT_ATTRIBUTES.blink).toBe(false);
		expect(DEFAULT_ATTRIBUTES.reverse).toBe(false);
		expect(DEFAULT_ATTRIBUTES.hidden).toBe(false);
		expect(DEFAULT_ATTRIBUTES.strikethrough).toBe(false);
		expect(DEFAULT_ATTRIBUTES.fg).toBeNull();
		expect(DEFAULT_ATTRIBUTES.bg).toBeNull();
	});
});

describe("createDefaultAttributes", () => {
	test("creates new object with default values", () => {
		const attrs = createDefaultAttributes();
		expect(attrs).not.toBe(DEFAULT_ATTRIBUTES);
		expect(attrs.bold).toBe(false);
		expect(attrs.fg).toBeNull();
	});
});

describe("attributesEqual", () => {
	test("returns true for same default attributes", () => {
		const a = createDefaultAttributes();
		const b = createDefaultAttributes();
		expect(attributesEqual(a, b)).toBe(true);
	});

	test("returns false for different bold", () => {
		const a = createDefaultAttributes();
		const b = createDefaultAttributes();
		b.bold = true;
		expect(attributesEqual(a, b)).toBe(false);
	});

	test("returns false for different foreground color", () => {
		const a = createDefaultAttributes();
		const b = createDefaultAttributes();
		b.fg = { type: "indexed", index: 1 };
		expect(attributesEqual(a, b)).toBe(false);
	});

	test("returns true for same indexed colors", () => {
		const a = createDefaultAttributes();
		const b = createDefaultAttributes();
		a.fg = { type: "indexed", index: 1 };
		b.fg = { type: "indexed", index: 1 };
		expect(attributesEqual(a, b)).toBe(true);
	});

	test("returns false for different indexed colors", () => {
		const a = createDefaultAttributes();
		const b = createDefaultAttributes();
		a.fg = { type: "indexed", index: 1 };
		b.fg = { type: "indexed", index: 2 };
		expect(attributesEqual(a, b)).toBe(false);
	});

	test("returns true for same RGB colors", () => {
		const a = createDefaultAttributes();
		const b = createDefaultAttributes();
		a.fg = { type: "rgb", r: 255, g: 0, b: 0 };
		b.fg = { type: "rgb", r: 255, g: 0, b: 0 };
		expect(attributesEqual(a, b)).toBe(true);
	});

	test("returns false for different RGB colors", () => {
		const a = createDefaultAttributes();
		const b = createDefaultAttributes();
		a.fg = { type: "rgb", r: 255, g: 0, b: 0 };
		b.fg = { type: "rgb", r: 0, g: 255, b: 0 };
		expect(attributesEqual(a, b)).toBe(false);
	});
});

describe("cloneAttributes", () => {
	test("creates independent copy", () => {
		const original = createDefaultAttributes();
		original.bold = true;
		original.fg = { type: "indexed", index: 5 };

		const cloned = cloneAttributes(original);
		expect(cloned).not.toBe(original);
		expect(cloned.bold).toBe(true);
		expect(cloned.fg).toEqual({ type: "indexed", index: 5 });

		// Modify original should not affect clone
		original.bold = false;
		expect(cloned.bold).toBe(true);
	});

	test("deep clones color objects", () => {
		const original = createDefaultAttributes();
		original.fg = { type: "rgb", r: 100, g: 150, b: 200 };

		const cloned = cloneAttributes(original);
		expect(cloned.fg).not.toBe(original.fg);
		expect(cloned.fg).toEqual(original.fg);
	});
});

describe("applySgrAttr", () => {
	test("Reset clears all attributes", () => {
		const attrs = createDefaultAttributes();
		attrs.bold = true;
		attrs.italic = true;
		attrs.fg = { type: "rgb", r: 255, g: 0, b: 0 };

		applySgrAttr(attrs, { attr: "Reset" });

		expect(attrs.bold).toBe(false);
		expect(attrs.italic).toBe(false);
		expect(attrs.fg).toBeNull();
	});

	test("Bold sets bold flag", () => {
		const attrs = createDefaultAttributes();
		applySgrAttr(attrs, { attr: "Bold" });
		expect(attrs.bold).toBe(true);
	});

	test("Dim sets dim flag", () => {
		const attrs = createDefaultAttributes();
		applySgrAttr(attrs, { attr: "Dim" });
		expect(attrs.dim).toBe(true);
	});

	test("Italic sets italic flag", () => {
		const attrs = createDefaultAttributes();
		applySgrAttr(attrs, { attr: "Italic" });
		expect(attrs.italic).toBe(true);
	});

	test("Underline sets underline flag", () => {
		const attrs = createDefaultAttributes();
		applySgrAttr(attrs, { attr: "Underline" });
		expect(attrs.underline).toBe(true);
	});

	test("Reverse sets reverse flag", () => {
		const attrs = createDefaultAttributes();
		applySgrAttr(attrs, { attr: "Reverse" });
		expect(attrs.reverse).toBe(true);
	});

	test("NormalIntensity clears bold and dim", () => {
		const attrs = createDefaultAttributes();
		attrs.bold = true;
		attrs.dim = true;
		applySgrAttr(attrs, { attr: "NormalIntensity" });
		expect(attrs.bold).toBe(false);
		expect(attrs.dim).toBe(false);
	});

	test("NotItalic clears italic", () => {
		const attrs = createDefaultAttributes();
		attrs.italic = true;
		applySgrAttr(attrs, { attr: "NotItalic" });
		expect(attrs.italic).toBe(false);
	});

	test("Foreground sets fg color", () => {
		const attrs = createDefaultAttributes();
		applySgrAttr(attrs, {
			attr: "Foreground",
			value: { type: "Standard", value: 1 },
		});
		expect(attrs.fg).not.toBeNull();
		expect(attrs.fg?.type).toBe("rgb");
	});

	test("Background sets bg color", () => {
		const attrs = createDefaultAttributes();
		applySgrAttr(attrs, {
			attr: "Background",
			value: { type: "Rgb", value: { r: 128, g: 64, b: 32 } },
		});
		expect(attrs.bg).toEqual({ type: "rgb", r: 128, g: 64, b: 32 });
	});

	test("DefaultForeground clears fg", () => {
		const attrs = createDefaultAttributes();
		attrs.fg = { type: "rgb", r: 255, g: 0, b: 0 };
		applySgrAttr(attrs, { attr: "DefaultForeground" });
		expect(attrs.fg).toBeNull();
	});

	test("DefaultBackground clears bg", () => {
		const attrs = createDefaultAttributes();
		attrs.bg = { type: "rgb", r: 0, g: 255, b: 0 };
		applySgrAttr(attrs, { attr: "DefaultBackground" });
		expect(attrs.bg).toBeNull();
	});
});

describe("applySgrAttrs", () => {
	test("applies multiple attributes in order", () => {
		const attrs = createDefaultAttributes();
		applySgrAttrs(attrs, [
			{ attr: "Bold" },
			{ attr: "Italic" },
			{ attr: "Foreground", value: { type: "Standard", value: 1 } },
		]);

		expect(attrs.bold).toBe(true);
		expect(attrs.italic).toBe(true);
		expect(attrs.fg).not.toBeNull();
	});

	test("reset followed by style works correctly", () => {
		const attrs = createDefaultAttributes();
		attrs.underline = true;

		applySgrAttrs(attrs, [{ attr: "Reset" }, { attr: "Bold" }]);

		expect(attrs.underline).toBe(false);
		expect(attrs.bold).toBe(true);
	});
});

describe("getEffectiveForeground", () => {
	test("returns default foreground when fg is null", () => {
		const attrs = createDefaultAttributes();
		expect(getEffectiveForeground(attrs)).toEqual(DEFAULT_FOREGROUND);
	});

	test("returns fg color when set", () => {
		const attrs = createDefaultAttributes();
		attrs.fg = { type: "rgb", r: 255, g: 128, b: 64 };
		expect(getEffectiveForeground(attrs)).toEqual({ r: 255, g: 128, b: 64 });
	});

	test("returns bg color when reverse is set", () => {
		const attrs = createDefaultAttributes();
		attrs.reverse = true;
		attrs.bg = { type: "rgb", r: 0, g: 128, b: 255 };
		expect(getEffectiveForeground(attrs)).toEqual({ r: 0, g: 128, b: 255 });
	});

	test("returns default background when reverse and bg is null", () => {
		const attrs = createDefaultAttributes();
		attrs.reverse = true;
		expect(getEffectiveForeground(attrs)).toEqual(DEFAULT_BACKGROUND);
	});
});

describe("getEffectiveBackground", () => {
	test("returns null when bg is null", () => {
		const attrs = createDefaultAttributes();
		expect(getEffectiveBackground(attrs)).toBeNull();
	});

	test("returns bg color when set", () => {
		const attrs = createDefaultAttributes();
		attrs.bg = { type: "rgb", r: 0, g: 64, b: 128 };
		expect(getEffectiveBackground(attrs)).toEqual({ r: 0, g: 64, b: 128 });
	});

	test("returns fg color when reverse is set", () => {
		const attrs = createDefaultAttributes();
		attrs.reverse = true;
		attrs.fg = { type: "rgb", r: 255, g: 0, b: 0 };
		expect(getEffectiveBackground(attrs)).toEqual({ r: 255, g: 0, b: 0 });
	});

	test("returns default foreground when reverse and fg is null", () => {
		const attrs = createDefaultAttributes();
		attrs.reverse = true;
		expect(getEffectiveBackground(attrs)).toEqual(DEFAULT_FOREGROUND);
	});
});
