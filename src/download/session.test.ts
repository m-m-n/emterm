import { describe, test, expect, beforeEach, afterEach, mock } from "bun:test";

// Track all invoke calls for verification
let invokeCalls: Array<{ command: string; args: Record<string, unknown> }> = [];
let invokeResults: Map<string, unknown> = new Map();
let invokeErrors: Map<string, Error> = new Map();

mock.module("@tauri-apps/api/core", () => ({
	invoke: async (command: string, args: Record<string, unknown> = {}) => {
		invokeCalls.push({ command, args });

		if (invokeErrors.has(command)) {
			throw invokeErrors.get(command);
		}

		if (invokeResults.has(command)) {
			return invokeResults.get(command);
		}

		// Default behaviors
		switch (command) {
			case "start_download_file":
				return { id: "handle-1", path: "/tmp/test-download.txt" };
			case "append_download_chunk":
				return undefined;
			case "finish_download_file":
				return "/tmp/test-download.txt";
			case "cancel_download_file":
				return undefined;
			default:
				return undefined;
		}
	},
}));

const { DownloadSessionManager } = await import("./session.ts");

describe("DownloadSessionManager", () => {
	let manager: DownloadSessionManager;

	beforeEach(() => {
		manager = new DownloadSessionManager();
		invokeCalls = [];
		invokeResults = new Map();
		invokeErrors = new Map();
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
		expect(session!.handleId).toBeNull();
		expect(session!.nextSeq).toBe(0);
	});

	test("begin invokes start_download_file IPC", async () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=ipc-test",
			"name=test.txt",
			"size=100",
		]);

		// Wait for async startDownload
		await new Promise((r) => setTimeout(r, 50));

		const startCalls = invokeCalls.filter(
			(c) => c.command === "start_download_file",
		);
		expect(startCalls.length).toBe(1);
		expect(startCalls[0].args.filename).toBe("test.txt");

		// Handle ID should be set after dialog confirms
		const session = manager.getSession("ipc-test");
		expect(session?.handleId).toBe("handle-1");
	});

	test("begin with user cancel on save dialog discards session", async () => {
		invokeResults.set("start_download_file", null);

		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=cancel-test",
			"name=test.txt",
			"size=100",
		]);

		await new Promise((r) => setTimeout(r, 50));

		expect(manager.sessionCount).toBe(0);
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

	test("chunk increments nextSeq and tracks receivedBytes", async () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=chunk-test",
			"name=data.bin",
			"size=10",
		]);
		await new Promise((r) => setTimeout(r, 50));

		manager.handleCommand("emterm", [
			"download",
			"chunk",
			"id=chunk-test",
			"seq=0",
			"data=SGVsbG8=",
		]);

		const session = manager.getSession("chunk-test");
		expect(session).toBeDefined();
		expect(session!.nextSeq).toBe(1);
		expect(session!.receivedBytes).toBe("SGVsbG8=".length);
	});

	test("chunk invokes append_download_chunk when handle is ready", async () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=append-test",
			"name=data.bin",
			"size=10",
		]);
		await new Promise((r) => setTimeout(r, 50));

		manager.handleCommand("emterm", [
			"download",
			"chunk",
			"id=append-test",
			"seq=0",
			"data=SGVsbG8=",
		]);
		await new Promise((r) => setTimeout(r, 50));

		const appendCalls = invokeCalls.filter(
			(c) => c.command === "append_download_chunk",
		);
		expect(appendCalls.length).toBe(1);
		expect(appendCalls[0].args.id).toBe("handle-1");
		expect(appendCalls[0].args.dataBase64).toBe("SGVsbG8=");
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

	test("chunk with out-of-order seq discards session", async () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=ooo",
			"name=test.txt",
			"size=100",
		]);
		await new Promise((r) => setTimeout(r, 50));

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

	test("out-of-order discard invokes cancel_download_file", async () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=ooo-cancel",
			"name=test.txt",
			"size=100",
		]);
		await new Promise((r) => setTimeout(r, 50));

		// Verify handle is set
		expect(manager.getSession("ooo-cancel")?.handleId).toBe("handle-1");

		manager.handleCommand("emterm", [
			"download",
			"chunk",
			"id=ooo-cancel",
			"seq=1",
			"data=test",
		]);
		await new Promise((r) => setTimeout(r, 50));

		const cancelCalls = invokeCalls.filter(
			(c) => c.command === "cancel_download_file",
		);
		expect(cancelCalls.length).toBe(1);
		expect(cancelCalls[0].args.id).toBe("handle-1");
	});

	test("multiple sequential chunks accumulate correctly", async () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=multi",
			"name=multi.txt",
			"size=100",
		]);
		await new Promise((r) => setTimeout(r, 50));

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
		expect(session!.nextSeq).toBe(3);
	});

	test("end invokes finish_download_file and removes session", async () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=end-test",
			"name=test.txt",
			"size=5",
		]);
		await new Promise((r) => setTimeout(r, 50));

		manager.handleCommand("emterm", [
			"download",
			"chunk",
			"id=end-test",
			"seq=0",
			"data=SGVsbG8=",
		]);
		manager.handleCommand("emterm", ["download", "end", "id=end-test"]);
		await new Promise((r) => setTimeout(r, 50));

		expect(manager.sessionCount).toBe(0);

		const finishCalls = invokeCalls.filter(
			(c) => c.command === "finish_download_file",
		);
		expect(finishCalls.length).toBe(1);
		expect(finishCalls[0].args.id).toBe("handle-1");
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

	test("cleanup removes expired sessions and invokes cancel", async () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=expired",
			"name=test.txt",
			"size=10",
		]);
		await new Promise((r) => setTimeout(r, 50));

		const session = manager.getSession("expired")!;
		session.lastChunkAt =
			Date.now() - DownloadSessionManager.SESSION_TIMEOUT - 1000;
		manager.cleanupExpiredSessions();
		expect(manager.sessionCount).toBe(0);

		await new Promise((r) => setTimeout(r, 50));

		const cancelCalls = invokeCalls.filter(
			(c) => c.command === "cancel_download_file",
		);
		expect(cancelCalls.length).toBe(1);
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

	test("no MAX_DOWNLOAD_SIZE limit - large sizes accepted", () => {
		const largeSize = 2 * 1024 * 1024 * 1024; // 2GB
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=large",
			"name=big.iso",
			`size=${largeSize}`,
		]);
		const session = manager.getSession("large");
		expect(session).toBeDefined();
		expect(session!.expectedSize).toBe(largeSize);
	});

	test("session does not store chunk data (no chunks Map)", () => {
		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=no-store",
			"name=test.txt",
			"size=100",
		]);
		const session = manager.getSession("no-store");
		expect(session).toBeDefined();
		// Verify no chunks property exists
		expect((session as Record<string, unknown>).chunks).toBeUndefined();
	});

	test("append_download_chunk error triggers cancel and discard", async () => {
		invokeErrors.set(
			"append_download_chunk",
			new Error("Disk full"),
		);

		manager.handleCommand("emterm", [
			"download",
			"begin",
			"id=error-test",
			"name=test.txt",
			"size=100",
		]);
		await new Promise((r) => setTimeout(r, 50));

		manager.handleCommand("emterm", [
			"download",
			"chunk",
			"id=error-test",
			"seq=0",
			"data=SGVsbG8=",
		]);
		await new Promise((r) => setTimeout(r, 100));

		expect(manager.sessionCount).toBe(0);

		const cancelCalls = invokeCalls.filter(
			(c) => c.command === "cancel_download_file",
		);
		expect(cancelCalls.length).toBe(1);
	});

	test("multiple concurrent sessions work independently", async () => {
		let handleCounter = 0;
		invokeResults.set("start_download_file", undefined); // clear default

		// Override to return different handles per call
		const originalInvokeCalls = invokeCalls;
		mock.module("@tauri-apps/api/core", () => ({
			invoke: async (command: string, args: Record<string, unknown> = {}) => {
				originalInvokeCalls.push({ command, args });
				if (command === "start_download_file") {
					handleCounter++;
					return { id: `handle-${handleCounter}`, path: `/tmp/file-${handleCounter}` };
				}
				return undefined;
			},
		}));

		// Re-import to pick up new mock
		const { DownloadSessionManager: NewManager } = await import("./session.ts");
		const mgr = new NewManager();

		mgr.handleCommand("emterm", [
			"download",
			"begin",
			"id=sess-a",
			"name=a.txt",
			"size=50",
		]);
		mgr.handleCommand("emterm", [
			"download",
			"begin",
			"id=sess-b",
			"name=b.txt",
			"size=100",
		]);

		expect(mgr.sessionCount).toBe(2);
		mgr.dispose();
	});
});
