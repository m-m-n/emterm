/**
 * Throughput regression bench for visible-mode PTY streaming (TS-25 / NFR1).
 *
 * The visibility-aware streaming work adds a single `is_visible()` atomic
 * load on the reader hot path. NFR1 requires that visible-mode throughput
 * is not measurably worse than the pre-change baseline (±5 %).
 *
 * This spec drives a fixed-size payload through the PTY in visible mode
 * and measures wall-clock time + reader-side `pty_get_send_stats.bytes`
 * throughput. It captures the metric to the spec output for human review.
 * Hard-failing on a numeric threshold inside CI requires a stored
 * baseline that is not part of this repository, so this spec only
 * reports — manual review compares the printed `bytes/sec` against the
 * baseline recorded in `doc/tasks/visibility-aware-pty-streaming/perf-results.md`.
 */

async function getSessionId() {
	return browser.execute(() => {
		const app = window.terminalApp;
		return app?.ptyClient?.getSessionId?.() ?? app?.pty?.getSessionId?.() ?? null;
	});
}

async function getSendStats(sessionId) {
	const result = await browser.executeAsync((sid, done) => {
		const internals = window.__TAURI_INTERNALS__;
		if (!internals?.invoke) {
			done({ error: "no internals" });
			return;
		}
		internals
			.invoke("pty_get_send_stats", { sessionId: sid })
			.then((r) => done({ count: r[0], bytes: r[1] }))
			.catch((e) => done({ error: String(e) }));
	}, sessionId);
	if (result?.error) throw new Error(result.error);
	return result;
}

async function typeSlowly(text, delay = 30) {
	for (const char of text) {
		await browser.keys(char);
		await browser.pause(delay);
	}
}

describe("Visible-mode throughput bench (TS-25 / NFR1)", () => {
	it("should sustain visible streaming without obvious regression", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.waitForExist({ timeout: 30000 });
		await browser.pause(3000);
		await terminal.click();
		await browser.pause(500);

		const sessionId = await getSessionId();
		expect(sessionId).toBeTruthy();

		const before = await getSendStats(sessionId);
		console.log("[BENCH] before:", JSON.stringify(before));

		const t0 = Date.now();
		// Drive a bounded payload sized to fit comfortably within Xvfb
		// scrollback. ~50 lines × ~80 chars/line = ~4 KB on the channel.
		await typeSlowly("for i in $(seq 1 40); do echo bench-line-$i; done");
		await browser.keys("Enter");
		// Give the reader plenty of time to flush all chunks at this size.
		await browser.pause(3000);
		const elapsedMs = Date.now() - t0;

		const after = await getSendStats(sessionId);
		console.log("[BENCH] after:", JSON.stringify(after));
		console.log(`[BENCH] elapsed_ms=${elapsedMs}`);

		const deltaBytes = after.bytes - before.bytes;
		const deltaCount = after.count - before.count;
		const bytesPerSec = elapsedMs > 0 ? Math.round((deltaBytes * 1000) / elapsedMs) : 0;
		console.log(`[BENCH] deltaBytes=${deltaBytes} deltaCount=${deltaCount} bytes/sec=${bytesPerSec}`);

		// Sanity floor: the workload must produce at least one chunk.
		// Tighter regression detection is done by manual review of the
		// printed bytes/sec against the recorded baseline.
		expect(deltaCount).toBeGreaterThan(0);
		expect(deltaBytes).toBeGreaterThan(0);
	});
});
