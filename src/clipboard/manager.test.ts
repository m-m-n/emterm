import { beforeEach, describe, expect, mock, test } from "bun:test";
import { ClipboardManager } from "./manager";

describe("ClipboardManager", () => {
	let manager: ClipboardManager;
	let mockClipboard: {
		writeText: ReturnType<typeof mock>;
		readText: ReturnType<typeof mock>;
	};

	beforeEach(() => {
		// Mock navigator.clipboard
		mockClipboard = {
			writeText: mock(() => Promise.resolve()),
			readText: mock(() => Promise.resolve("")),
		};

		// @ts-expect-error - Mock global navigator.clipboard
		globalThis.navigator = {
			clipboard: mockClipboard,
		};

		manager = new ClipboardManager();
	});

	describe("copyToClipboard", () => {
		test("writes text to clipboard", async () => {
			const text = "Hello, World!";
			await manager.copyToClipboard(text);

			expect(mockClipboard.writeText).toHaveBeenCalledTimes(1);
			expect(mockClipboard.writeText).toHaveBeenCalledWith(text);
		});

		test("handles empty string", async () => {
			await manager.copyToClipboard("");

			expect(mockClipboard.writeText).toHaveBeenCalledWith("");
		});

		test("handles multi-line text", async () => {
			const text = "Line 1\nLine 2\nLine 3";
			await manager.copyToClipboard(text);

			expect(mockClipboard.writeText).toHaveBeenCalledWith(text);
		});

		test("handles Unicode characters", async () => {
			const text = "日本語 🎉";
			await manager.copyToClipboard(text);

			expect(mockClipboard.writeText).toHaveBeenCalledWith(text);
		});

		test("returns true on success", async () => {
			const result = await manager.copyToClipboard("test");
			expect(result).toBe(true);
		});

		test("returns false on failure", async () => {
			mockClipboard.writeText = mock(() =>
				Promise.reject(new Error("Permission denied")),
			);

			const result = await manager.copyToClipboard("test");
			expect(result).toBe(false);
		});

		test("logs error on failure", async () => {
			const consoleError = mock(() => {});
			const originalError = console.error;
			console.error = consoleError;

			mockClipboard.writeText = mock(() =>
				Promise.reject(new Error("Permission denied")),
			);

			await manager.copyToClipboard("test");

			expect(consoleError).toHaveBeenCalled();

			console.error = originalError;
		});
	});

	describe("pasteFromClipboard", () => {
		test("reads text from clipboard", async () => {
			mockClipboard.readText = mock(() => Promise.resolve("Clipboard content"));

			const text = await manager.pasteFromClipboard();

			expect(mockClipboard.readText).toHaveBeenCalledTimes(1);
			expect(text).toBe("Clipboard content");
		});

		test("handles empty clipboard", async () => {
			mockClipboard.readText = mock(() => Promise.resolve(""));

			const text = await manager.pasteFromClipboard();

			expect(text).toBe("");
		});

		test("handles multi-line clipboard content", async () => {
			const content = "Line 1\nLine 2\nLine 3";
			mockClipboard.readText = mock(() => Promise.resolve(content));

			const text = await manager.pasteFromClipboard();

			expect(text).toBe(content);
		});

		test("returns empty string on failure", async () => {
			mockClipboard.readText = mock(() =>
				Promise.reject(new Error("Permission denied")),
			);

			const text = await manager.pasteFromClipboard();

			expect(text).toBe("");
		});

		test("logs error on failure", async () => {
			const consoleError = mock(() => {});
			const originalError = console.error;
			console.error = consoleError;

			mockClipboard.readText = mock(() =>
				Promise.reject(new Error("Permission denied")),
			);

			await manager.pasteFromClipboard();

			expect(consoleError).toHaveBeenCalled();

			console.error = originalError;
		});
	});

	describe("hasNewlines", () => {
		test("returns false for single-line text", () => {
			expect(manager.hasNewlines("Hello, World!")).toBe(false);
		});

		test("returns true for text with LF", () => {
			expect(manager.hasNewlines("Line 1\nLine 2")).toBe(true);
		});

		test("returns true for text with CRLF", () => {
			expect(manager.hasNewlines("Line 1\r\nLine 2")).toBe(true);
		});

		test("returns true for text with CR", () => {
			expect(manager.hasNewlines("Line 1\rLine 2")).toBe(true);
		});

		test("returns false for empty string", () => {
			expect(manager.hasNewlines("")).toBe(false);
		});

		test("returns true for newline at end", () => {
			expect(manager.hasNewlines("Text\n")).toBe(true);
		});

		test("returns true for newline at beginning", () => {
			expect(manager.hasNewlines("\nText")).toBe(true);
		});
	});

	describe("countLines", () => {
		test("counts single line", () => {
			expect(manager.countLines("Hello")).toBe(1);
		});

		test("counts multiple lines with LF", () => {
			expect(manager.countLines("Line 1\nLine 2\nLine 3")).toBe(3);
		});

		test("counts multiple lines with CRLF", () => {
			expect(manager.countLines("Line 1\r\nLine 2\r\nLine 3")).toBe(3);
		});

		test("counts empty string as 1 line", () => {
			expect(manager.countLines("")).toBe(1);
		});

		test("counts trailing newline correctly", () => {
			expect(manager.countLines("Line 1\nLine 2\n")).toBe(3);
		});

		test("counts leading newline correctly", () => {
			expect(manager.countLines("\nLine 1\nLine 2")).toBe(3);
		});

		test("counts only newlines", () => {
			expect(manager.countLines("\n\n\n")).toBe(4);
		});

		test("handles mixed newline types", () => {
			expect(manager.countLines("Line 1\nLine 2\r\nLine 3\rLine 4")).toBe(4);
		});
	});
});
