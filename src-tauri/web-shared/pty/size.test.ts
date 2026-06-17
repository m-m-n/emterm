/**
 * Tests for terminal size calculation utilities.
 */

import { afterEach, beforeEach, describe, expect, it, mock } from "bun:test";
import { calculateTerminalSize, measureCharacterSize } from "./size";

// Mock getComputedStyle
const originalGetComputedStyle = globalThis.getComputedStyle;

describe("calculateTerminalSize", () => {
	beforeEach(() => {
		// Mock getComputedStyle to return zero padding (default terminal style)
		globalThis.getComputedStyle = mock(() => ({
			paddingLeft: "0px",
			paddingRight: "0px",
			paddingTop: "0px",
			paddingBottom: "0px",
		})) as unknown as typeof getComputedStyle;
	});

	afterEach(() => {
		globalThis.getComputedStyle = originalGetComputedStyle;
	});

	it("should calculate correct columns and rows", () => {
		const container = {
			clientWidth: 800, // Full width available (no padding)
			clientHeight: 400, // Full height available (no padding)
		} as HTMLElement;

		const result = calculateTerminalSize(container, 10, 20);

		// 800 / 10 = 80 cols, 400 / 20 = 20 rows
		expect(result.cols).toBe(80);
		expect(result.rows).toBe(20);
	});

	it("should return at least 1 column and 1 row", () => {
		const container = {
			clientWidth: 5, // Less than one character width
			clientHeight: 8, // Less than one character height
		} as HTMLElement;

		const result = calculateTerminalSize(container, 10, 20);

		expect(result.cols).toBe(1);
		expect(result.rows).toBe(1);
	});

	it("should floor fractional values", () => {
		const container = {
			clientWidth: 115, // 115 / 10 = 11.5 -> 11
			clientHeight: 67, // 67 / 20 = 3.35 -> 3
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

	it("should account for non-zero padding if CSS changes", () => {
		globalThis.getComputedStyle = mock(() => ({
			paddingLeft: "10px",
			paddingRight: "10px",
			paddingTop: "5px",
			paddingBottom: "5px",
		})) as unknown as typeof getComputedStyle;

		const container = {
			clientWidth: 820, // 820 - 20 (padding) = 800 available
			clientHeight: 410, // 410 - 10 (padding) = 400 available
		} as HTMLElement;

		const result = calculateTerminalSize(container, 10, 20);

		// 800 / 10 = 80 cols, 400 / 20 = 20 rows
		expect(result.cols).toBe(80);
		expect(result.rows).toBe(20);
	});
});

describe("measureCharacterSize", () => {
	const originalCreateElement = document.createElement.bind(document);

	beforeEach(() => {
		// Mock getComputedStyle for measureCharacterSize tests
		globalThis.getComputedStyle = mock((el: Element) => {
			const htmlEl = el as HTMLElement;
			return {
				fontFamily: htmlEl.style.fontFamily || "monospace",
				fontSize: htmlEl.style.fontSize || "14px",
				paddingLeft: "0px",
				paddingRight: "0px",
				paddingTop: "0px",
				paddingBottom: "0px",
			};
		}) as unknown as typeof getComputedStyle;

		// Mock canvas for text measurement
		const mockCanvas = {
			getContext: mock(() => ({
				measureText: mock(() => ({
					width: 8.4,
					fontBoundingBoxAscent: 12,
					fontBoundingBoxDescent: 3,
				})),
				font: "",
			})),
		};
		globalThis.document.createElement = mock((tagName: string) => {
			if (tagName === "canvas") {
				return mockCanvas as unknown as HTMLCanvasElement;
			}
			return originalCreateElement.call(document, tagName);
		}) as typeof document.createElement;
	});

	afterEach(() => {
		globalThis.getComputedStyle = originalGetComputedStyle;
		globalThis.document.createElement = originalCreateElement;
	});

	it("should return width and height from font metrics", () => {
		// Create a mock container element
		const container = originalCreateElement("div");
		container.id = "terminal";
		container.style.fontFamily = "monospace";
		container.style.fontSize = "14px";

		const result = measureCharacterSize(container);

		// Check that we get reasonable values
		expect(result.width).toBeGreaterThan(0);
		// Height should be ascent + descent from font metrics (12 + 3 = 15)
		expect(result.height).toBe(15);
	});

	it("should use fallback when canvas unavailable", () => {
		// Override createElement to return canvas without getContext
		globalThis.document.createElement = mock((tagName: string) => {
			if (tagName === "canvas") {
				return { getContext: () => null } as unknown as HTMLCanvasElement;
			}
			return originalCreateElement.call(document, tagName);
		}) as typeof document.createElement;

		const container = originalCreateElement("div");
		container.style.fontFamily = "monospace";
		container.style.fontSize = "14px";

		const result = measureCharacterSize(container);

		// Should return valid fallback dimensions (height = fontSize when canvas unavailable)
		expect(result.width).toBeGreaterThan(0);
		expect(result.height).toBe(14);
	});
});
