/**
 * eMterm microtask-data-flow E2E (TS-12 / NFR1).
 *
 * Verifies that the WASM-parsing data path keeps draining when
 * `requestAnimationFrame` is stalled (simulating WebKitGTK's behaviour
 * when the page is hidden / occluded). This is the central NFR1
 * verification for the microtask-driven-pty-flow feature.
 *
 * Strategy:
 *  - Patch globalThis.requestAnimationFrame with a stub that records
 *    every call but never invokes the supplied callback.
 *  - Drive an UNBOUNDED `yes` producer so the backend reader keeps
 *    pushing chunks into the frontend.
 *  - Sample `pty_get_send_stats(sessionId)` at regular intervals across
 *    a multi-second window. `pty_get_send_stats` returns
 *    (sent_count, sent_bytes); `sent_bytes` keeps growing only if the
 *    reader is NOT blocked in `wait_for_drain`. If the frontend stopped
 *    acking (e.g. the rAF stall blocked the data path), `in_flight`
 *    would hit `HIGH_WATER_BYTES` (8 MiB) and the reader would block —
 *    `sent_bytes` would plateau within ~1 second.
 *  - Continued monotonic growth therefore proves the microtask-driven
 *    scheduler is consuming and acking even with rAF stalled.
 *  - Restore real rAF and Ctrl+C the producer so subsequent specs see a
 *    clean terminal.
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
		if (!internals || typeof internals.invoke !== "function") {
			done({ error: "Tauri internals not available" });
			return;
		}
		internals
			.invoke("pty_get_send_stats", { sessionId: sid })
			.then((res) => done({ count: res[0], bytes: res[1] }))
			.catch((err) => done({ error: String(err) }));
	}, sessionId);
	if (result?.error) throw new Error(`getSendStats failed: ${result.error}`);
	return result;
}

async function typeSlowly(text, delay = 30) {
	for (const char of text) {
		await browser.keys(char);
		await browser.pause(delay);
	}
}

/**
 * Install the rAF stall stub and start capturing console.warn lines so
 * we can grep for `backpressure stalled` later.
 *
 * Mirrors the stub from `visibility-raf-heartbeat.e2e.js`: records every
 * requestAnimationFrame call AND queues the callbacks (without invoking
 * them). The queued cbs are drained explicitly during teardown to
 * unblock any controllers that depend on rAF for recovery.
 */
async function installRafStall() {
	await browser.execute(() => {
		const w = /** @type {any} */ (window);
		w.__rafStallScheduledCount = 0;
		w.__rafStallQueuedCbs = [];
		w.__rafStallOriginal = w.requestAnimationFrame.bind(w);
		w.requestAnimationFrame = function (cb) {
			w.__rafStallScheduledCount++;
			if (typeof cb === "function") {
				w.__rafStallQueuedCbs.push(cb);
			}
			return ++w.__rafStallNextHandle || (w.__rafStallNextHandle = 1);
		};
		w.__rafStallWarnLines = [];
		w.__rafStallOriginalWarn = console.warn.bind(console);
		console.warn = function (...args) {
			try {
				const line = args.map((a) => (typeof a === "string" ? a : String(a))).join(" ");
				w.__rafStallWarnLines.push(line);
			} catch {
				/* ignore */
			}
			return w.__rafStallOriginalWarn.apply(null, args);
		};
	});
}

async function drainRafStallQueue() {
	await browser.execute(() => {
		const w = /** @type {any} */ (window);
		const cbs = Array.isArray(w.__rafStallQueuedCbs) ? w.__rafStallQueuedCbs : [];
		w.__rafStallQueuedCbs = [];
		const ts = typeof performance !== "undefined" ? performance.now() : Date.now();
		for (const cb of cbs) {
			try {
				cb(ts);
			} catch {
				/* ignore */
			}
		}
	});
}

async function restoreRafStall() {
	await browser.execute(() => {
		const w = /** @type {any} */ (window);
		if (typeof w.__rafStallOriginal === "function") {
			w.requestAnimationFrame = w.__rafStallOriginal;
		}
		if (typeof w.__rafStallOriginalWarn === "function") {
			console.warn = w.__rafStallOriginalWarn;
		}
	});
}

async function getCapturedWarnLines() {
	return browser.execute(() => {
		const w = /** @type {any} */ (window);
		return Array.isArray(w.__rafStallWarnLines) ? w.__rafStallWarnLines.slice() : [];
	});
}

async function getRafStallScheduledCount() {
	return browser.execute(() => {
		const w = /** @type {any} */ (window);
		return typeof w.__rafStallScheduledCount === "number" ? w.__rafStallScheduledCount : 0;
	});
}

describe("microtask-driven PTY flow keeps draining under rAF stall (TS-12 / NFR1)", () => {
	let sessionId = null;

	before(async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.waitForExist({ timeout: 30000 });
		await browser.pause(3000);
		await terminal.click();
		await browser.pause(500);
		sessionId = await getSessionId();
		console.log("Session id:", sessionId);
		expect(sessionId).toBeTruthy();
	});

	it("TS-12: sent_bytes keeps increasing while requestAnimationFrame is stalled", async () => {
		// Use an UNBOUNDED producer so the workload outlives the
		// observation window.
		await typeSlowly("yes mt-payload");
		await browser.pause(300);

		// Install stall stub BEFORE Enter so any in-flight rAF callbacks
		// scheduled during input echo do not slip through after press.
		await installRafStall();
		await browser.pause(100);

		// Start the workload.
		await browser.keys("Enter");

		// Let the producer ramp up so chunks reach the backend reader.
		await browser.pause(1500);

		const stubCallsAfterRamp = await getRafStallScheduledCount();
		console.log("rAF stub scheduledCount after ramp:", stubCallsAfterRamp);
		expect(stubCallsAfterRamp).toBeGreaterThan(0);

		// Sample sent_bytes across an observation window. Under the
		// microtask scheduler the data path keeps acking, so sent_bytes
		// must keep growing across samples even though rAF is stalled.
		const samples = [];
		const SAMPLE_COUNT = 8;
		const SAMPLE_INTERVAL_MS = 500;
		for (let i = 0; i < SAMPLE_COUNT; i++) {
			const stats = await getSendStats(sessionId);
			samples.push(stats);
			await browser.pause(SAMPLE_INTERVAL_MS);
		}
		console.log("Send stats samples:", JSON.stringify(samples));

		const first = samples[0];
		const last = samples[samples.length - 1];

		// Sanity: the producer reached the backend reader.
		expect(first.bytes).toBeGreaterThan(0);

		// Core invariant: sent_bytes more than doubled across the
		// 4-second observation window. If the frontend stopped acking,
		// `in_flight` would hit HIGH_WATER_BYTES (8 MiB) within ~1s and
		// `sent_bytes` would plateau. Continued monotonic growth proves
		// the microtask-driven data path is consuming and acking under
		// rAF stall.
		const ratio = last.bytes / Math.max(first.bytes, 1);
		console.log(
			`First sample bytes=${first.bytes}, last=${last.bytes}, ratio=${ratio.toFixed(2)}`,
		);
		expect(last.bytes).toBeGreaterThan(first.bytes * 2);

		// `backpressure stalled` warn lines must NOT appear during the
		// rAF-stall window. Their presence would indicate the reader
		// blocked in wait_for_drain for an extended period.
		const warnLines = await getCapturedWarnLines();
		const backpressureWarns = warnLines.filter((l) =>
			l.toLowerCase().includes("backpressure stalled"),
		);
		console.log(`backpressure-stalled warn lines: ${backpressureWarns.length}`);
		for (const l of backpressureWarns) console.log("  ", l);
		expect(backpressureWarns.length).toBe(0);
	});

	after(async () => {
		// Restore real rAF + drain queued callbacks so any controller
		// that depends on rAF (e.g. visibility heartbeat) can recover
		// before the next spec runs.
		await restoreRafStall();
		await drainRafStallQueue();

		// Ctrl+C the producer to leave the terminal clean for any
		// subsequent specs in the same session.
		await browser.keys(["Control", "c"]);
		await browser.pause(200);

		await browser.saveScreenshot(
			"./screenshots/microtask-data-flow-after.png",
		);
	});
});
