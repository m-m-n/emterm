import { describe, test, expect, beforeEach, afterEach, mock } from "bun:test";

// Mock Tauri API before importing session module
mock.module("@tauri-apps/api/core", () => ({
	invoke: async () => "/tmp/test-download.txt",
}));

// Import after mocks are set up
const { DownloadSessionManager } = await import("./session.ts");

describe("DownloadSessionManager", () => {
	let manager: DownloadSessionManager;

	beforeEach(() => {
		manager = new DownloadSessionManager();
	});

	afterEach(() => {
		manager.dispose();
	});

	test("handleCommand ignores non-emterm verb", () => {
		manager.handleCommand("other", ["download", "begin", "id=abc"]);
		expect(manager.sessionCount).toBe(0);
	});

	test("handleCommand ignores non-download type", () => {
		manager.handleCommand("emterm", ["markdown", "begin", "id=abc"]);
		expect(manager.sessionCount).toBe(0);
	});

	test("begin creates a new session", () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=test-uuid",
			"name=test.txt",
			"size=100",
			"version=1.0",
		]);
		expect(manager.sessionCount).toBe(1);
		const session = manager.getSession("test-uuid");
		expect(session).toBeDefined();
		expect(session!.filename).toBe("test.txt");
		expect(session!.expectedSize).toBe(100);
	});

	test("begin with missing id is ignored", () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"name=test.txt",
			"size=100",
		]);
		expect(manager.sessionCount).toBe(0);
	});

	test("begin with duplicate id is ignored", () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=dup",
			"name=first.txt",
			"size=50",
		]);
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=dup",
			"name=second.txt",
			"size=100",
		]);
		expect(manager.sessionCount).toBe(1);
		expect(manager.getSession("dup")!.filename).toBe("first.txt");
	});

	test("chunk accumulates data in session", () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=abc",
			"name=data.bin",
			"size=10",
		]);
		manager.handleCommand("emterm", [
			"download",
			"chunk",
			"id=abc",
			"seq=0",
			"data=SGVsbG8=",
		]);
		const session = manager.getSession("abc");
		expect(session).toBeDefined();
		expect(session!.chunks.size).toBe(1);
		expect(session!.chunks.get(0)).toBe("SGVsbG8=");
	});

	test("chunk with unknown id is silently ignored", () => {
		manager.handleCommand("emterm", [
			"download",
			"chunk",
			"id=unknown",
			"seq=0",
			"data=test",
		]);
		expect(manager.sessionCount).toBe(0);
	});

	test("chunk with out-of-order seq discards session", () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=ooo",
			"name=test.txt",
			"size=100",
		]);
		// Skip seq=0 and send seq=1
		manager.handleCommand("emterm", [
			"download",
			"chunk",
			"id=ooo",
			"seq=1",
			"data=test",
		]);
		expect(manager.sessionCount).toBe(0);
	});

	test("multiple sequential chunks accumulate correctly", () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=multi",
			"name=multi.txt",
			"size=100",
		]);
		manager.handleCommand("emterm", [
			"download",
			"chunk",
			"id=multi",
			"seq=0",
			"data=chunk0",
		]);
		manager.handleCommand("emterm", [
			"download",
			"chunk",
			"id=multi",
			"seq=1",
			"data=chunk1",
		]);
		manager.handleCommand("emterm", [
			"download",
			"chunk",
			"id=multi",
			"seq=2",
			"data=chunk2",
		]);
		const session = manager.getSession("multi");
		expect(session!.chunks.size).toBe(3);
	});

	test("end removes session", async () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=end-test",
			"name=test.txt",
			"size=5",
		]);
		manager.handleCommand("emterm", [
			"download",
			"chunk",
			"id=end-test",
			"seq=0",
			"data=SGVsbG8=",
		]);
		manager.handleCommand("emterm", ["download", "end", "id=end-test"]);
		// Wait for async handleEnd
		await new Promise((r) => setTimeout(r, 50));
		expect(manager.sessionCount).toBe(0);
	});

	test("end with unknown id is ignored", () => {
		manager.handleCommand("emterm", ["download", "end", "id=unknown"]);
		expect(manager.sessionCount).toBe(0);
	});

	test("max sessions limit is enforced", () => {
		for (let i = 0; i < 12; i++) {
			manager.handleCommand("emterm", [
				"download",
				"begin",
				`id=session-${i}`,
				"name=test.txt",
				"size=10",
			]);
		}
		expect(manager.sessionCount).toBe(DownloadSessionManager.MAX_SESSIONS);
	});

	test("cleanup removes expired sessions", () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=expired",
			"name=test.txt",
			"size=10",
		]);
		const session = manager.getSession("expired")!;
		session.lastChunkAt =
			Date.now() - DownloadSessionManager.SESSION_TIMEOUT - 1000;
		manager.cleanupExpiredSessions();
		expect(manager.sessionCount).toBe(0);
	});

	test("dispose cleans up all state", () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=dispose-test",
			"name=test.txt",
			"size=10",
		]);
		manager.dispose();
		expect(manager.sessionCount).toBe(0);
	});

	test("begin with default filename when name missing", () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=no-name",
			"size=100",
		]);
		const session = manager.getSession("no-name");
		expect(session!.filename).toBe("download");
	});

	test("begin with default size when size missing", () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=no-size",
			"name=test.txt",
		]);
		const session = manager.getSession("no-size");
		expect(session!.expectedSize).toBe(0);
	});

	test("sanitizeFilename strips semicolons", () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=semi",
			"name=evil;inject=val.txt",
			"size=10",
		]);
		const session = manager.getSession("semi");
		expect(session!.filename).toBe("evilinject=val.txt");
	});

	test("sanitizeFilename preserves legitimate double dots", () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=dots",
			"name=file..v2.txt",
			"size=10",
		]);
		const session = manager.getSession("dots");
		expect(session!.filename).toBe("file..v2.txt");
	});

	test("chunk accumulated size limit prevents size=0 bypass", () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=bypass",
			"name=test.txt",
			"size=0",
		]);
		// Simulate exceeding limit by manipulating receivedBytes
		const session = manager.getSession("bypass")!;
		session.receivedBytes = Math.ceil(
			(DownloadSessionManager.MAX_DOWNLOAD_SIZE * 4) / 3,
		);
		// Next chunk should trigger discard
		manager.handleCommand("emterm", [
			"download",
			"chunk",
			"id=bypass",
			"seq=0",
			"data=extra",
		]);
		expect(manager.sessionCount).toBe(0);
	});
});
