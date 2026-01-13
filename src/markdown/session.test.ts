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
			manager.handleCommand("emterm", [
				"markdown",
				"begin",
				"id=test-session-1",
				"format=gfm",
			]);

			expect(manager.sessionCount).toBe(1);

			const session = manager.getSession("test-session-1");
			expect(session).toBeDefined();
			expect(session?.format).toBe("gfm");
			expect(session?.version).toBe(1);
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
		});

		test("should reject session without id", () => {
			const consoleSpy = mock(() => {});
			const originalWarn = console.warn;
			console.warn = consoleSpy;

			manager.handleCommand("emterm", ["markdown", "begin"]);

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
			]);

			const session = manager.getSession("full-params");
			expect(session?.format).toBe("gfm");
			expect(session?.version).toBe(2);
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

		test("should cleanup session after end (no DOM)", () => {
			// Temporarily suppress errors from fullscreen view in test environment
			const originalError = console.error;
			console.error = mock(() => {});

			try {
				manager.handleCommand("emterm", ["markdown", "end", "id=end-test"]);
			} catch {
				// Ignore DOM-related errors in test environment
			}

			// Session should be cleaned up regardless of fullscreen display success
			expect(manager.getSession("end-test")).toBeUndefined();
			expect(manager.sessionCount).toBe(0);

			console.error = originalError;
		});

		test("should warn for unknown session", () => {
			const consoleSpy = mock(() => {});
			const originalWarn = console.warn;
			console.warn = consoleSpy;

			manager.handleCommand("emterm", [
				"markdown",
				"end",
				"id=unknown",
			]);

			expect(consoleSpy).toHaveBeenCalled();

			console.warn = originalWarn;
		});

		// DOM-dependent tests are in integration.test.ts with proper DOM environment
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
			manager.handleCommand("other", [
				"markdown",
				"begin",
				"id=test",
			]);

			expect(manager.sessionCount).toBe(0);
		});

		test("should ignore non-markdown commands", () => {
			manager.handleCommand("emterm", [
				"image",
				"begin",
				"id=test",
			]);

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
		// Note: DOM-dependent fullscreen tests are in integration.test.ts
		// which has proper DOM environment setup (happy-dom)

		test("should process end command and cleanup session", () => {
			manager.handleCommand("emterm", [
				"markdown",
				"begin",
				"id=fullscreen-test",
			]);
			manager.handleCommand("emterm", [
				"markdown",
				"chunk",
				"id=fullscreen-test",
				"seq=0",
				"data=IyBUZXN0", // "# Test" in Base64
			]);

			// Suppress DOM errors in test environment
			const originalError = console.error;
			console.error = mock(() => {});

			try {
				manager.handleCommand("emterm", [
					"markdown",
					"end",
					"id=fullscreen-test",
				]);
			} catch {
				// Ignore DOM-related errors
			}

			// Session should be cleaned up
			expect(manager.getSession("fullscreen-test")).toBeUndefined();

			console.error = originalError;
		});
	});
});
