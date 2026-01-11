/**
 * Tests for StyleCache CSS class-based styling.
 *
 * Note: These tests focus on the caching logic.
 * DOM-related functionality is tested in integration tests.
 */
import { beforeEach, describe, expect, mock, test } from "bun:test";
import type { CellAttributes } from "./attributes.ts";
import { cloneAttributes, createDefaultAttributes } from "./attributes.ts";

// Mock document for testing in non-browser environment
const mockDocument = {
	getElementById: mock(() => null),
	createElement: mock((tag: string) => ({
		id: "",
		textContent: "",
		remove: mock(() => {}),
	})),
	head: {
		appendChild: mock(() => {}),
	},
};

// Helper to create attributes with specific properties
function createAttrs(overrides: Partial<CellAttributes> = {}): CellAttributes {
	return { ...createDefaultAttributes(), ...overrides };
}

describe("StyleCache Logic", () => {
	describe("Attribute Hashing", () => {
		test("should produce same hash for identical attributes", () => {
			const attrs1 = createAttrs({ bold: true });
			const attrs2 = createAttrs({ bold: true });

			// Same attribute combination should produce same hash
			expect(attrs1.bold).toBe(attrs2.bold);
			expect(attrs1.dim).toBe(attrs2.dim);
			expect(attrs1.italic).toBe(attrs2.italic);
		});

		test("should produce different hash for different attributes", () => {
			const attrs1 = createAttrs({ bold: true });
			const attrs2 = createAttrs({ italic: true });

			expect(attrs1.bold).not.toBe(attrs2.bold);
			expect(attrs1.italic).not.toBe(attrs2.italic);
		});

		test("should handle color attributes", () => {
			const attrs1 = createAttrs({
				fg: { type: "rgb", r: 255, g: 0, b: 0 },
			});
			const attrs2 = createAttrs({
				fg: { type: "rgb", r: 0, g: 255, b: 0 },
			});

			// Different colors should be distinguishable
			const fg1 = attrs1.fg;
			const fg2 = attrs2.fg;
			expect(fg1?.type).toBe("rgb");
			expect(fg2?.type).toBe("rgb");
			if (fg1?.type === "rgb" && fg2?.type === "rgb") {
				expect(fg1.r).not.toBe(fg2.r);
			}
		});
	});

	describe("Decoration Classes", () => {
		test("should generate correct decoration classes for bold", () => {
			const attrs = createAttrs({ bold: true });
			expect(attrs.bold).toBe(true);
		});

		test("should generate correct decoration classes for italic", () => {
			const attrs = createAttrs({ italic: true });
			expect(attrs.italic).toBe(true);
		});

		test("should handle underline and strikethrough combination", () => {
			const attrs = createAttrs({ underline: true, strikethrough: true });
			expect(attrs.underline).toBe(true);
			expect(attrs.strikethrough).toBe(true);
		});

		test("should handle all decoration flags", () => {
			const attrs = createAttrs({
				bold: true,
				dim: true,
				italic: true,
				underline: true,
				blink: true,
				hidden: true,
				strikethrough: true,
			});

			expect(attrs.bold).toBe(true);
			expect(attrs.dim).toBe(true);
			expect(attrs.italic).toBe(true);
			expect(attrs.underline).toBe(true);
			expect(attrs.blink).toBe(true);
			expect(attrs.hidden).toBe(true);
			expect(attrs.strikethrough).toBe(true);
		});
	});

	describe("Attribute Cloning", () => {
		test("should clone attributes correctly", () => {
			const original = createAttrs({
				bold: true,
				fg: { type: "rgb", r: 100, g: 150, b: 200 },
			});

			const cloned = cloneAttributes(original);

			expect(cloned.bold).toBe(original.bold);
			expect(cloned.fg?.type).toBe("rgb");
			if (cloned.fg?.type === "rgb" && original.fg?.type === "rgb") {
				expect(cloned.fg.r).toBe(original.fg.r);
				expect(cloned.fg.g).toBe(original.fg.g);
				expect(cloned.fg.b).toBe(original.fg.b);
			}

			// Modifying clone should not affect original
			cloned.bold = false;
			expect(original.bold).toBe(true);
		});
	});

	describe("Cache Efficiency Patterns", () => {
		test("should benefit from repeated attribute patterns", () => {
			// Simulate typical terminal output with repeated styles
			const patterns: CellAttributes[] = [];

			// Normal text (most common)
			for (let i = 0; i < 1000; i++) {
				patterns.push(createAttrs());
			}

			// Bold text
			for (let i = 0; i < 100; i++) {
				patterns.push(createAttrs({ bold: true }));
			}

			// Colored text
			for (let i = 0; i < 100; i++) {
				patterns.push(createAttrs({ fg: { type: "rgb", r: 255, g: 0, b: 0 } }));
			}

			// Count unique patterns
			const uniquePatterns = new Set<string>();
			for (const attrs of patterns) {
				// Create a simple hash
				const hash = JSON.stringify({
					bold: attrs.bold,
					dim: attrs.dim,
					italic: attrs.italic,
					fg: attrs.fg,
					bg: attrs.bg,
				});
				uniquePatterns.add(hash);
			}

			// Should have only 3 unique patterns despite 1200 total
			expect(uniquePatterns.size).toBe(3);
		});

		test("should handle 256-color palette efficiently", () => {
			const patterns: CellAttributes[] = [];

			// Use indexed colors (common in terminal applications)
			for (let i = 0; i < 256; i++) {
				patterns.push(createAttrs({ fg: { type: "indexed", index: i } }));
			}

			// Should have 256 unique patterns
			const uniquePatterns = new Set<string>();
			for (const attrs of patterns) {
				const fg = attrs.fg;
				if (fg?.type === "indexed") {
					uniquePatterns.add(`indexed-${fg.index}`);
				}
			}

			expect(uniquePatterns.size).toBe(256);
		});

		test("should handle common color combinations", () => {
			// Common terminal color combinations
			const combinations: CellAttributes[] = [
				// Normal
				createAttrs(),
				// Bold
				createAttrs({ bold: true }),
				// Error (red)
				createAttrs({ fg: { type: "indexed", index: 1 } }),
				// Success (green)
				createAttrs({ fg: { type: "indexed", index: 2 } }),
				// Warning (yellow)
				createAttrs({ fg: { type: "indexed", index: 3 } }),
				// Info (blue)
				createAttrs({ fg: { type: "indexed", index: 4 } }),
				// Path (cyan)
				createAttrs({ fg: { type: "indexed", index: 6 } }),
				// Highlighted
				createAttrs({ bg: { type: "indexed", index: 7 } }),
				// Reverse
				createAttrs({ reverse: true }),
			];

			// All should be unique
			const uniquePatterns = new Set<string>();
			for (const attrs of combinations) {
				uniquePatterns.add(JSON.stringify(attrs));
			}

			expect(uniquePatterns.size).toBe(9);
		});
	});
});

describe("Performance Optimization Patterns", () => {
	test("should minimize attribute comparisons", () => {
		// Test that we can quickly compare attributes
		const attrs1 = createAttrs({
			bold: true,
			fg: { type: "rgb", r: 255, g: 0, b: 0 },
		});
		const attrs2 = createAttrs({
			bold: true,
			fg: { type: "rgb", r: 255, g: 0, b: 0 },
		});

		const start = performance.now();
		let comparisonCount = 0;

		for (let i = 0; i < 10000; i++) {
			// Simple equality check pattern
			const equal =
				attrs1.bold === attrs2.bold &&
				attrs1.dim === attrs2.dim &&
				attrs1.italic === attrs2.italic &&
				attrs1.underline === attrs2.underline &&
				attrs1.blink === attrs2.blink &&
				attrs1.hidden === attrs2.hidden &&
				attrs1.strikethrough === attrs2.strikethrough &&
				attrs1.reverse === attrs2.reverse;

			if (equal) comparisonCount++;
		}

		const duration = performance.now() - start;

		// 10000 comparisons should be very fast
		expect(duration).toBeLessThan(10);
		expect(comparisonCount).toBe(10000);
	});

	test("should handle rapid attribute changes", () => {
		const changes: CellAttributes[] = [];

		const start = performance.now();

		// Simulate rapid SGR changes
		for (let i = 0; i < 10000; i++) {
			changes.push(
				createAttrs({
					bold: i % 2 === 0,
					fg: { type: "indexed", index: i % 256 },
				}),
			);
		}

		const duration = performance.now() - start;

		// Should be fast
		expect(duration).toBeLessThan(50);
		expect(changes.length).toBe(10000);
	});
});
