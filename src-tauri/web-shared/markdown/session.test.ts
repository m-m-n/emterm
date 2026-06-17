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

		test("should accumulate large data without size limit", () => {
			// Create a large Base64 string (> 2MB decoded) — should succeed
			const largeContent = "x".repeat(3 * 1024 * 1024);
			const largeData = btoa(largeContent);

			manager.handleCommand("emterm", [
				"markdown",
				"chunk",
				"id=chunk-test",
				"seq=0",
				`data=${largeData}`,
			]);

			// Session should still exist (no size limit)
			const session = manager.getSession("chunk-test");
			expect(session).toBeDefined();
			expect(session?.chunks.size).toBe(1);
			expect(session?.chunks.get(0)).toBe(largeContent);
		});

		test("should update lastChunkAt on each chunk", () => {
			const before = Date.now();

			manager.handleCommand("emterm", [
				"markdown",
				"chunk",
				"id=chunk-test",
				"seq=0",
				"data=IyBIZWxsbw==",
			]);

			const session = manager.getSession("chunk-test");
			expect(session?.lastChunkAt).toBeGreaterThanOrEqual(before);
			expect(session?.lastChunkAt).toBeLessThanOrEqual(Date.now());
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
		test("should cleanup expired sessions based on lastChunkAt", async () => {
			// Create a session and manually set old lastChunkAt
			manager.handleCommand("emterm", ["markdown", "begin", "id=old-session"]);

			const session = manager.getSession("old-session");
			if (session) {
				// Manually age the session via lastChunkAt
				(session as any).lastChunkAt =
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

		test("should not cleanup session with recent chunk", () => {
			manager.handleCommand("emterm", ["markdown", "begin", "id=active-session"]);

			// Send a chunk to update lastChunkAt
			manager.handleCommand("emterm", [
				"markdown",
				"chunk",
				"id=active-session",
				"seq=0",
				"data=IyBIZWxsbw==",
			]);

			// Trigger cleanup
			manager.cleanupExpiredSessions();

			// Session should survive because lastChunkAt is recent
			expect(manager.getSession("active-session")).toBeDefined();
		});

		test("should cleanup session with old lastChunkAt", () => {
			manager.handleCommand("emterm", ["markdown", "begin", "id=stale-session"]);

			// Send a chunk, then manually age lastChunkAt
			manager.handleCommand("emterm", [
				"markdown",
				"chunk",
				"id=stale-session",
				"seq=0",
				"data=IyBIZWxsbw==",
			]);

			const session = manager.getSession("stale-session");
			if (session) {
				(session as any).lastChunkAt =
					Date.now() - MarkdownSessionManager.SESSION_TIMEOUT - 1000;
			}

			manager.cleanupExpiredSessions();

			expect(manager.getSession("stale-session")).toBeUndefined();
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

	describe("basedir handling", () => {
		test("should store basedir from begin params", () => {
			manager.handleCommand("emterm", [
				"markdown",
				"begin",
				"id=basedir-test",
				"format=gfm",
				"basedir=/home/user/docs",
			]);

			const session = manager.getSession("basedir-test");
			expect(session).toBeDefined();
			expect(session?.basedir).toBe("/home/user/docs");
		});

		test("should store undefined basedir when not provided", () => {
			manager.handleCommand("emterm", [
				"markdown",
				"begin",
				"id=no-basedir-test",
				"format=gfm",
			]);

			const session = manager.getSession("no-basedir-test");
			expect(session).toBeDefined();
			expect(session?.basedir).toBeUndefined();
		});

		test("should expose active basedir after end command", () => {
			manager.handleCommand("emterm", [
				"markdown",
				"begin",
				"id=active-basedir-test",
				"basedir=/home/user/project",
			]);
			manager.handleCommand("emterm", [
				"markdown",
				"chunk",
				"id=active-basedir-test",
				"seq=0",
				"data=IyBUZXN0",
			]);

			// Suppress DOM errors
			const originalError = console.error;
			console.error = mock(() => {});

			try {
				manager.handleCommand("emterm", [
					"markdown",
					"end",
					"id=active-basedir-test",
				]);
			} catch {
				// Ignore DOM-related errors
			}

			expect(manager.getActiveBasedir()).toBe("/home/user/project");

			console.error = originalError;
		});
	});

	describe("PTY write callback", () => {
		test("should accept and store PTY write callback", () => {
			const writeFn = mock((_data: string) => {});
			manager.setPtyWriteCallback(writeFn);

			// Callback should be stored (verified via fullscreen integration)
			expect(manager.getPtyWriteCallback()).toBe(writeFn);
		});

		test("should clear PTY write callback", () => {
			const writeFn = mock((_data: string) => {});
			manager.setPtyWriteCallback(writeFn);
			manager.setPtyWriteCallback(null);

			expect(manager.getPtyWriteCallback()).toBeNull();
		});
	});

	describe("image-response handling", () => {
		test("should handle single image-response", () => {
			// Set up a DOM container with a placeholder image
			const container = document.createElement("div");
			const img = document.createElement("img");
			img.setAttribute("data-request-id", "img-1");
			img.src = "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";
			container.appendChild(img);

			manager.setImageContainer(container);

			manager.handleCommand("emterm", [
				"markdown",
				"image-response",
				"request_id=img-1",
				"mime_type=image/png",
				"data=iVBORw0KGgo=",
			]);

			expect(img.src).toBe("data:image/png;base64,iVBORw0KGgo=");
		});

		test("should reject SVG MIME type to prevent XSS", () => {
			const container = document.createElement("div");
			const img = document.createElement("img");
			img.setAttribute("data-request-id", "img-10");
			container.appendChild(img);

			manager.setImageContainer(container);

			const consoleSpy = mock(() => {});
			const originalWarn = console.warn;
			console.warn = consoleSpy;

			manager.handleCommand("emterm", [
				"markdown",
				"image-response",
				"request_id=img-10",
				"mime_type=image/svg+xml",
				"data=PHN2Zz48L3N2Zz4=",
			]);

			// Image should be replaced with error indicator
			const errorEl = container.querySelector("[data-request-id='img-10']");
			expect(errorEl).not.toBeNull();
			expect(errorEl?.textContent).toContain("unsupported format");

			expect(consoleSpy).toHaveBeenCalled();
			console.warn = originalWarn;
		});

		test("should reject invalid request_id format", () => {
			const container = document.createElement("div");
			manager.setImageContainer(container);

			const consoleSpy = mock(() => {});
			const originalWarn = console.warn;
			console.warn = consoleSpy;

			manager.handleCommand("emterm", [
				"markdown",
				"image-response",
				"request_id=evil\"][onclick=\"alert(1)",
				"mime_type=image/png",
				"data=iVBORw0KGgo=",
			]);

			expect(consoleSpy).toHaveBeenCalled();
			console.warn = originalWarn;
		});

		test("should handle chunked image-response", () => {
			const container = document.createElement("div");
			const img = document.createElement("img");
			img.setAttribute("data-request-id", "img-2");
			container.appendChild(img);

			manager.setImageContainer(container);

			// Send chunk 0 of 2
			manager.handleCommand("emterm", [
				"markdown",
				"image-response",
				"request_id=img-2",
				"mime_type=image/png",
				"chunk_seq=0",
				"chunk_total=2",
				"data=iVBORw0K",
			]);

			// Image should not be set yet
			expect(img.src).not.toContain("data:image/png");

			// Send chunk 1 of 2 (final)
			manager.handleCommand("emterm", [
				"markdown",
				"image-response",
				"request_id=img-2",
				"chunk_seq=1",
				"chunk_total=2",
				"data=Ggo=",
			]);

			// Image should now be assembled
			expect(img.src).toBe("data:image/png;base64,iVBORw0KGgo=");
		});

		test("should ignore image-response with unknown request_id", () => {
			const container = document.createElement("div");
			manager.setImageContainer(container);

			const consoleSpy = mock(() => {});
			const originalWarn = console.warn;
			console.warn = consoleSpy;

			manager.handleCommand("emterm", [
				"markdown",
				"image-response",
				"request_id=img-99",
				"mime_type=image/png",
				"data=iVBORw0KGgo=",
			]);

			expect(consoleSpy).toHaveBeenCalled();
			console.warn = originalWarn;
		});
	});

	describe("image-error handling", () => {
		test("should handle image-error and show error on placeholder", () => {
			const container = document.createElement("div");
			const img = document.createElement("img");
			img.setAttribute("data-request-id", "img-100");
			container.appendChild(img);

			manager.setImageContainer(container);

			manager.handleCommand("emterm", [
				"markdown",
				"image-error",
				"request_id=img-100",
				"error=File not found",
			]);

			// Image should be replaced with error indicator
			const errorEl = container.querySelector("[data-request-id='img-100']");
			expect(errorEl).not.toBeNull();
			expect(errorEl?.textContent).toContain("File not found");
		});

		test("should ignore image-error with invalid request_id format", () => {
			const container = document.createElement("div");
			manager.setImageContainer(container);

			const consoleSpy = mock(() => {});
			const originalWarn = console.warn;
			console.warn = consoleSpy;

			manager.handleCommand("emterm", [
				"markdown",
				"image-error",
				"request_id=unknown-err",
				"error=Some error",
			]);

			expect(consoleSpy).toHaveBeenCalled();
			console.warn = originalWarn;
		});

		test("should ignore image-error with unknown but valid request_id", () => {
			const container = document.createElement("div");
			manager.setImageContainer(container);

			const consoleSpy = mock(() => {});
			const originalWarn = console.warn;
			console.warn = consoleSpy;

			manager.handleCommand("emterm", [
				"markdown",
				"image-error",
				"request_id=img-999",
				"error=Some error",
			]);

			expect(consoleSpy).toHaveBeenCalled();
			console.warn = originalWarn;
		});
	});

	describe("image request invalidation on navigation", () => {
		test("should clear pending image chunks on new begin", () => {
			const container = document.createElement("div");
			const img = document.createElement("img");
			img.setAttribute("data-request-id", "img-200");
			container.appendChild(img);

			manager.setImageContainer(container);

			// Start a chunked image transfer
			manager.handleCommand("emterm", [
				"markdown",
				"image-response",
				"request_id=img-200",
				"mime_type=image/png",
				"chunk_seq=0",
				"chunk_total=3",
				"data=chunk0data",
			]);

			// New navigation begins (new session)
			manager.handleCommand("emterm", [
				"markdown",
				"begin",
				"id=new-session",
				"basedir=/new/path",
			]);

			// Pending chunks should be cleared
			// Verify by sending remaining chunks - they should not assemble
			const consoleSpy = mock(() => {});
			const originalWarn = console.warn;
			console.warn = consoleSpy;

			manager.handleCommand("emterm", [
				"markdown",
				"image-response",
				"request_id=img-200",
				"chunk_seq=1",
				"chunk_total=3",
				"data=chunk1data",
			]);

			// The old placeholder is gone (new session cleared it)
			// So this should warn about unknown request_id
			expect(consoleSpy).toHaveBeenCalled();
			console.warn = originalWarn;
		});
	});
});
