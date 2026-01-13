/**
 * Tests for MarkdownSessionManager.
 */
import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import { MarkdownSessionManager } from "./session.ts";

describe("MarkdownSessionManager", () => {
	let manager: MarkdownSessionManager;

	beforeEach(() => {
		manager = new MarkdownSessionManager();
	});

	afterEach(() => {
		manager.dispose();
	});

	describe("handleBegin", () => {
		test("should create new session with valid params", () => {
			const result = manager.handleCommand("emterm", [
				"markdown",
				"begin",
				"id=test-session-1",
				"format=gfm",
			]);

			expect(result).toBeNull(); // begin returns null
			expect(manager.sessionCount).toBe(1);

			const session = manager.getSession("test-session-1");
			expect(session).toBeDefined();
			expect(session?.format).toBe("gfm");
			expect(session?.version).toBe(1);
			expect(session?.render).toBe("block");
		});

		test("should create session with default values", () => {
			manager.handleCommand("emterm", [
				"markdown",
				"begin",
				"id=default-session",
			]);

			const session = manager.getSession("default-session");
			expect(session).toBeDefined();
			expect(session?.format).toBe("commonmark");
			expect(session?.version).toBe(1);
			expect(session?.render).toBe("block");
		});

		test("should reject session without id", () => {
			const consoleSpy = mock(() => {});
			const originalWarn = console.warn;
			console.warn = consoleSpy;

			const result = manager.handleCommand("emterm", ["markdown", "begin"]);

			expect(result).toBeNull();
			expect(manager.sessionCount).toBe(0);
			expect(consoleSpy).toHaveBeenCalled();

			console.warn = originalWarn;
		});

		test("should reject when max sessions reached", () => {
			// Create MAX_SESSIONS sessions
			for (let i = 0; i < MarkdownSessionManager.MAX_SESSIONS; i++) {
				manager.handleCommand("emterm", [
					"markdown",
					"begin",
					`id=session-${i}`,
				]);
			}

			expect(manager.sessionCount).toBe(MarkdownSessionManager.MAX_SESSIONS);

			const consoleSpy = mock(() => {});
			const originalWarn = console.warn;
			console.warn = consoleSpy;

			// Try to create one more
			manager.handleCommand("emterm", [
				"markdown",
				"begin",
				"id=overflow-session",
			]);

			expect(manager.sessionCount).toBe(MarkdownSessionManager.MAX_SESSIONS);
			expect(consoleSpy).toHaveBeenCalled();

			console.warn = originalWarn;
		});

		test("should use optional parameters", () => {
			manager.handleCommand("emterm", [
				"markdown",
				"begin",
				"id=full-params",
				"format=gfm",
				"version=2",
				"render=inline",
			]);

			const session = manager.getSession("full-params");
			expect(session?.format).toBe("gfm");
			expect(session?.version).toBe(2);
			expect(session?.render).toBe("inline");
		});
	});

	describe("handleChunk", () => {
		beforeEach(() => {
			manager.handleCommand("emterm", ["markdown", "begin", "id=chunk-test"]);
		});

		test("should append decoded data to session", () => {
			// "# Hello" in Base64 is "IyBIZWxsbw=="
			manager.handleCommand("emterm", [
				"markdown",
				"chunk",
				"id=chunk-test",
				"seq=0",
				"data=IyBIZWxsbw==",
			]);

			const session = manager.getSession("chunk-test");
			expect(session?.chunks.size).toBe(1);
			expect(session?.chunks.get(0)).toBe("# Hello");
			expect(session?.dataSize).toBe(7);
		});

		test("should reject chunk for unknown session", () => {
			const consoleSpy = mock(() => {});
			const originalWarn = console.warn;
			console.warn = consoleSpy;

			manager.handleCommand("emterm", [
				"markdown",
				"chunk",
				"id=unknown-session",
				"seq=0",
				"data=SGVsbG8=",
			]);

			expect(consoleSpy).toHaveBeenCalled();
			console.warn = originalWarn;
		});

		test("should reject invalid Base64 data", () => {
			const consoleSpy = mock(() => {});
			const originalWarn = console.warn;
			console.warn = consoleSpy;

			manager.handleCommand("emterm", [
				"markdown",
				"chunk",
				"id=chunk-test",
				"seq=0",
				"data=!!!invalid-base64!!!",
			]);

			const session = manager.getSession("chunk-test");
			expect(session?.chunks.size).toBe(0);
			expect(consoleSpy).toHaveBeenCalled();

			console.warn = originalWarn;
		});

		test("should enforce size limit", () => {
			// Create a large Base64 string (> 2MB decoded)
			const largeData = btoa(
				"x".repeat(MarkdownSessionManager.MAX_SESSION_SIZE + 1),
			);

			const consoleSpy = mock(() => {});
			const originalWarn = console.warn;
			console.warn = consoleSpy;

			manager.handleCommand("emterm", [
				"markdown",
				"chunk",
				"id=chunk-test",
				"seq=0",
				`data=${largeData}`,
			]);

			// Session should be deleted due to size limit
			expect(manager.getSession("chunk-test")).toBeUndefined();
			expect(consoleSpy).toHaveBeenCalled();

			console.warn = originalWarn;
		});

		test("should accumulate multiple chunks", () => {
			manager.handleCommand("emterm", [
				"markdown",
				"chunk",
				"id=chunk-test",
				"seq=0",
				"data=IyBI", // "# H"
			]);
			manager.handleCommand("emterm", [
				"markdown",
				"chunk",
				"id=chunk-test",
				"seq=1",
				"data=ZWxsbw==", // "ello"
			]);

			const session = manager.getSession("chunk-test");
			expect(session?.chunks.size).toBe(2);
			expect(session?.chunks.get(0)).toBe("# H");
			expect(session?.chunks.get(1)).toBe("ello");
		});
	});

	describe("handleEnd", () => {
		beforeEach(() => {
			manager.handleCommand("emterm", ["markdown", "begin", "id=end-test"]);
			manager.handleCommand("emterm", [
				"markdown",
				"chunk",
				"id=end-test",
				"seq=0",
				"data=IyBIZWxsbw==", // "# Hello"
			]);
		});

		test("should assemble chunks in order", () => {
			// Add another chunk out of order
			manager.handleCommand("emterm", [
				"markdown",
				"chunk",
				"id=end-test",
				"seq=2",
				"data=IQ==", // "!"
			]);
			manager.handleCommand("emterm", [
				"markdown",
				"chunk",
				"id=end-test",
				"seq=1",
				"data=V29ybGQ=", // "World"
			]);

			const result = manager.handleCommand("emterm", [
				"markdown",
				"end",
				"id=end-test",
			]);

			expect(result).not.toBeNull();
			expect(result?.id).toBe("end-test");
			// Chunks should be assembled in order: "# Hello" + "World" + "!"
			// The HTML will contain rendered markdown
			expect(result?.html).toBeDefined();
		});

		test("should cleanup session after end", () => {
			manager.handleCommand("emterm", ["markdown", "end", "id=end-test"]);

			expect(manager.getSession("end-test")).toBeUndefined();
			expect(manager.sessionCount).toBe(0);
		});

		test("should return null for unknown session", () => {
			const consoleSpy = mock(() => {});
			const originalWarn = console.warn;
			console.warn = consoleSpy;

			const result = manager.handleCommand("emterm", [
				"markdown",
				"end",
				"id=unknown",
			]);

			expect(result).toBeNull();
			expect(consoleSpy).toHaveBeenCalled();

			console.warn = originalWarn;
		});
	});

	describe("timeout", () => {
		test("should cleanup expired sessions", async () => {
			// Create a session and manually set old timestamp
			manager.handleCommand("emterm", ["markdown", "begin", "id=old-session"]);

			const session = manager.getSession("old-session");
			if (session) {
				// Manually age the session
				(session as any).createdAt =
					Date.now() - MarkdownSessionManager.SESSION_TIMEOUT - 1000;
			}

			// Trigger cleanup
			manager.cleanupExpiredSessions();

			expect(manager.getSession("old-session")).toBeUndefined();
		});

		test("should not cleanup fresh sessions", () => {
			manager.handleCommand("emterm", [
				"markdown",
				"begin",
				"id=fresh-session",
			]);

			manager.cleanupExpiredSessions();

			expect(manager.getSession("fresh-session")).toBeDefined();
		});
	});

	describe("non-markdown commands", () => {
		test("should ignore non-emterm verbs", () => {
			const result = manager.handleCommand("other", [
				"markdown",
				"begin",
				"id=test",
			]);

			expect(result).toBeNull();
			expect(manager.sessionCount).toBe(0);
		});

		test("should ignore non-markdown commands", () => {
			const result = manager.handleCommand("emterm", [
				"image",
				"begin",
				"id=test",
			]);

			expect(result).toBeNull();
			expect(manager.sessionCount).toBe(0);
		});
	});

	describe("dispose", () => {
		test("should cleanup all sessions", () => {
			manager.handleCommand("emterm", ["markdown", "begin", "id=session-1"]);
			manager.handleCommand("emterm", ["markdown", "begin", "id=session-2"]);

			expect(manager.sessionCount).toBe(2);

			manager.dispose();

			expect(manager.sessionCount).toBe(0);
		});
	});

	describe("fullscreen mode", () => {
		test("should accept render=fullscreen in handleBegin", () => {
			manager.handleCommand("emterm", [
				"markdown",
				"begin",
				"id=fullscreen-test",
				"render=fullscreen",
			]);

			const session = manager.getSession("fullscreen-test");
			expect(session).toBeDefined();
			expect(session?.render).toBe("fullscreen");
		});

		test("should return null for fullscreen mode in handleEnd", () => {
			manager.handleCommand("emterm", [
				"markdown",
				"begin",
				"id=fullscreen-test",
				"render=fullscreen",
			]);
			manager.handleCommand("emterm", [
				"markdown",
				"chunk",
				"id=fullscreen-test",
				"seq=0",
				"data=IyBUZXN0", // "# Test" in Base64
			]);

			const result = manager.handleCommand("emterm", [
				"markdown",
				"end",
				"id=fullscreen-test",
			]);

			// For fullscreen, handleEnd returns null (fullscreen handles its own display)
			expect(result).toBeNull();
		});

		test("should show fullscreen overlay for render=fullscreen", () => {
			manager.handleCommand("emterm", [
				"markdown",
				"begin",
				"id=fullscreen-show-test",
				"render=fullscreen",
			]);
			manager.handleCommand("emterm", [
				"markdown",
				"chunk",
				"id=fullscreen-show-test",
				"seq=0",
				"data=IyBIZWxsbw==", // "# Hello" in Base64
			]);
			manager.handleCommand("emterm", [
				"markdown",
				"end",
				"id=fullscreen-show-test",
			]);

			// Check if fullscreen overlay is shown
			const overlay = document.querySelector(".markdown-fullscreen-overlay");
			expect(overlay).not.toBeNull();

			// Clean up overlay
			overlay?.remove();
		});

		test("should not affect existing inline/block modes", () => {
			// Block mode
			manager.handleCommand("emterm", [
				"markdown",
				"begin",
				"id=block-test",
				"render=block",
			]);
			manager.handleCommand("emterm", [
				"markdown",
				"chunk",
				"id=block-test",
				"seq=0",
				"data=IyBCbG9jaw==", // "# Block" in Base64
			]);
			const blockResult = manager.handleCommand("emterm", [
				"markdown",
				"end",
				"id=block-test",
			]);

			// Block mode returns MarkdownBlock
			expect(blockResult).not.toBeNull();
			expect(blockResult?.id).toBe("block-test");

			// Inline mode
			manager.handleCommand("emterm", [
				"markdown",
				"begin",
				"id=inline-test",
				"render=inline",
			]);
			manager.handleCommand("emterm", [
				"markdown",
				"chunk",
				"id=inline-test",
				"seq=0",
				"data=SW5saW5l", // "Inline" in Base64
			]);
			const inlineResult = manager.handleCommand("emterm", [
				"markdown",
				"end",
				"id=inline-test",
			]);

			// Inline mode returns MarkdownBlock
			expect(inlineResult).not.toBeNull();
			expect(inlineResult?.id).toBe("inline-test");
		});
	});
});
