/**
 * Tests for ClipboardBridge
 */

import { describe, test, expect, beforeEach, afterEach, mock } from "bun:test";
import { ClipboardBridge } from "./ClipboardBridge";
import {
	_resetPlatformCacheForTests,
	_setPlatformCacheForTests,
} from "../platform";

// Mock the Tauri core invoke used by writePrimary / readPrimary so the
// Linux-path tests can exercise the actual code without a Tauri runtime.
const mockInvoke = mock(() => Promise.resolve(""));

mock.module("@tauri-apps/api/core", () => ({
	invoke: mockInvoke,
}));

describe("ClipboardBridge", () => {
	let clipboard: ClipboardBridge;

	beforeEach(() => {
		mockInvoke.mockClear();
		mockInvoke.mockImplementation(() => Promise.resolve(""));
		clipboard = new ClipboardBridge();
	});

	afterEach(() => {
		_resetPlatformCacheForTests();
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

	describe("PRIMARY selection (non-Linux short-circuit)", () => {
		test("writePrimary returns false without touching Tauri on non-Linux", async () => {
			_setPlatformCacheForTests("windows");
			const result = await clipboard.writePrimary("hello");
			expect(result).toBe(false);
			expect(mockInvoke).not.toHaveBeenCalled();
		});

		test("writePrimary returns false before platform is resolved", async () => {
			_resetPlatformCacheForTests();
			const result = await clipboard.writePrimary("hello");
			expect(result).toBe(false);
			expect(mockInvoke).not.toHaveBeenCalled();
		});

		test("readPrimary returns empty string without touching Tauri on non-Linux", async () => {
			_setPlatformCacheForTests("windows");
			const result = await clipboard.readPrimary();
			expect(result).toBe("");
			expect(mockInvoke).not.toHaveBeenCalled();
		});

		test("readPrimary returns empty string before platform is resolved", async () => {
			_resetPlatformCacheForTests();
			const result = await clipboard.readPrimary();
			expect(result).toBe("");
			expect(mockInvoke).not.toHaveBeenCalled();
		});
	});

	describe("PRIMARY selection (Linux mocked path)", () => {
		test("writePrimary invokes clipboard_write_primary on Linux", async () => {
			_setPlatformCacheForTests("linux");
			mockInvoke.mockImplementation(() => Promise.resolve(""));

			const result = await clipboard.writePrimary("hello");

			expect(result).toBe(true);
			expect(mockInvoke).toHaveBeenCalledTimes(1);
			expect(mockInvoke).toHaveBeenCalledWith("clipboard_write_primary", {
				text: "hello",
			});
		});

		test("writePrimary returns false and logs on Linux when invoke throws", async () => {
			_setPlatformCacheForTests("linux");
			mockInvoke.mockImplementation(() =>
				Promise.reject(new Error("backend unavailable")),
			);

			const consoleWarn = mock(() => {});
			const originalWarn = console.warn;
			console.warn = consoleWarn;

			const result = await clipboard.writePrimary("hello");

			expect(result).toBe(false);
			expect(consoleWarn).toHaveBeenCalled();

			console.warn = originalWarn;
		});

		test("readPrimary returns the invoke result on Linux", async () => {
			_setPlatformCacheForTests("linux");
			mockInvoke.mockImplementation(() => Promise.resolve("primary content"));

			const result = await clipboard.readPrimary();

			expect(result).toBe("primary content");
			expect(mockInvoke).toHaveBeenCalledWith("clipboard_read_primary");
		});

		test("readPrimary returns empty string when PRIMARY is genuinely empty", async () => {
			_setPlatformCacheForTests("linux");
			mockInvoke.mockImplementation(() => Promise.resolve(""));

			const result = await clipboard.readPrimary();

			// "" is the genuine-empty signal — callers may safely fall back to CLIPBOARD.
			expect(result).toBe("");
		});

		test("readPrimary returns null and logs on Linux when invoke throws", async () => {
			_setPlatformCacheForTests("linux");
			mockInvoke.mockImplementation(() =>
				Promise.reject(new Error("backend unavailable")),
			);

			const consoleWarn = mock(() => {});
			const originalWarn = console.warn;
			console.warn = consoleWarn;

			const result = await clipboard.readPrimary();

			// null = read error; callers must NOT fall back to CLIPBOARD on null.
			expect(result).toBeNull();
			expect(consoleWarn).toHaveBeenCalled();

			console.warn = originalWarn;
		});
	});
});
