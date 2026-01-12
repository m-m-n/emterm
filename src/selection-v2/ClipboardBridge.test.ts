/**
 * Tests for ClipboardBridge
 */

import { describe, test, expect, beforeEach } from "bun:test";
import { ClipboardBridge } from "./ClipboardBridge";

describe("ClipboardBridge", () => {
	let clipboard: ClipboardBridge;

	beforeEach(() => {
		clipboard = new ClipboardBridge();
	});

	describe("isMultiLine", () => {
		test("should return false for single line", () => {
			expect(clipboard.isMultiLine("hello world")).toBe(false);
		});

		test("should return true for LF", () => {
			expect(clipboard.isMultiLine("line1\nline2")).toBe(true);
		});

		test("should return true for CR", () => {
			expect(clipboard.isMultiLine("line1\rline2")).toBe(true);
		});

		test("should return true for CRLF", () => {
			expect(clipboard.isMultiLine("line1\r\nline2")).toBe(true);
		});

		test("should return false for empty string", () => {
			expect(clipboard.isMultiLine("")).toBe(false);
		});
	});

	describe("countLines", () => {
		test("should return 1 for empty string", () => {
			expect(clipboard.countLines("")).toBe(1);
		});

		test("should return 1 for single line", () => {
			expect(clipboard.countLines("hello")).toBe(1);
		});

		test("should count LF separated lines", () => {
			expect(clipboard.countLines("line1\nline2\nline3")).toBe(3);
		});

		test("should count CR separated lines", () => {
			expect(clipboard.countLines("line1\rline2")).toBe(2);
		});

		test("should count CRLF as single separator", () => {
			expect(clipboard.countLines("line1\r\nline2\r\nline3")).toBe(3);
		});

		test("should handle trailing newline", () => {
			expect(clipboard.countLines("line1\nline2\n")).toBe(3);
		});
	});
});
