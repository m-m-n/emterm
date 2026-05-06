/**
 * eMterm visibility-raf-heartbeat E2E (E2E-1 / E2E-2 / E2E-3).
 *
 * Verifies that VisibilityController detects a stalled
 * requestAnimationFrame loop and propagates `pty_set_visibility(false)`
 * to the backend within a few seconds, then immediately recovers
 * `pty_set_visibility(true)` once rAF resumes.
 *
 * Strategy:
 *  - Patch globalThis.requestAnimationFrame with a stub that records
 *    every call but never invokes the supplied callback. The
 *    VisibilityController binds rAF lazily through globalThis, so the
 *    stub is observed even though it is installed after construction.
 *  - Drive an UNBOUNDED `yes` producer so backend `sent_bytes` would
 *    keep growing if the hidden short-circuit failed.
 *  - Poll the captured warn buffer for `[DIAG-IDLE] visibility→hidden
 *    ... reason=raf-stall`; once it appears, snapshot `sent_bytes` as
 *    the *post-detection baseline* and verify the delta over a 3 s
 *    sample window stays at noise level. This decouples the assertion
 *    from the (variable) detection latency before the hidden flip.
 *  - Restore the original rAF and poll for `[DIAG-IDLE]
 *    visibility→visible` to assert resume within ~2 s.
 *  - Finally Ctrl+C the producer to leave the terminal clean.
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
 * we can grep for `[DIAG-IDLE] reason=raf-stall`. The stub records
 * every requestAnimationFrame call AND queues the callbacks (without
 * invoking them) — this mimics WebKit's background-throttle behaviour
 * where the cbs survive cancel() and are delivered after wake. The
 * queued cbs are drained by `drainRafStallQueue()` once we restore the
 * real rAF, simulating the wake delivery and unblocking the
 * controller's `rafAlive=true` recovery path.
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
			// Return a fake handle but never invoke the cb synchronously.
			return ++w.__rafStallNextHandle || (w.__rafStallNextHandle = 1);
		};
		// Capture warn lines in a buffer for assertion.
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

/**
 * Fire any rAF callbacks the stub queued while it was active. The
 * controller's recovery path requires *one* rAF callback to actually
 * execute so it can flip `rafAlive=true` and re-evaluate. WebKit
 * delivers these queued cbs after the tab wakes; in the test
 * environment we drain them explicitly.
 */
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

/**
 * Poll the captured warn buffer until a line matching `predicate`
 * appears, or `timeoutMs` elapses. Returns the matching line(s).
 *
 * `bufferKey` selects which captured buffer to scan
 * (`__rafStallWarnLines` for the stall window, `__rafStallResumeLines`
 * for the resume window).
 */
async function waitForWarnLine(predicateSrc, timeoutMs, bufferKey = "__rafStallWarnLines", intervalMs = 250) {
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		const lines = await browser.execute(
			(key, src) => {
				const w = /** @type {any} */ (window);
				const buf = Array.isArray(w[key]) ? w[key] : [];
				const fn = new Function("l", `return (${src})(l);`);
				return buf.filter(fn);
			},
			bufferKey,
			predicateSrc,
		);
		if (lines.length > 0) return lines;
		await browser.pause(intervalMs);
	}
	return [];
}

describe("VisibilityController rAF heartbeat (E2E-1 / E2E-2 / E2E-3)", () => {
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

	it("E2E-1 / E2E-3: rAF stall flips hidden; sent_bytes stays flat AFTER detection fires", async () => {
		// Use an UNBOUNDED producer so the workload keeps generating
		// past the detection latency window. We can then sample
		// `sent_bytes` AFTER the hidden flip and verify the hidden
		// short-circuit suppresses further growth.
		await typeSlowly("yes hb-payload");
		await browser.pause(300);

		// Install stall stub BEFORE Enter so any in-flight rAF queued
		// by the controller doesn't slip through after press.
		await installRafStall();
		await browser.pause(100);

		// Start the workload.
		await browser.keys("Enter");

		// Poll for `[DIAG-IDLE] visibility→hidden ... reason=...raf-stall`.
		// Detection latency is bounded by HEALTH_CHECK_MS (10s) +
		// HIDE_DEBOUNCE_MS (1s) + slack. 18s gives generous headroom.
		const stallHidden = await waitForWarnLine(
			"(l) => l.includes('[DIAG-IDLE] visibility→hidden') && l.includes('reason=') && l.includes('raf-stall')",
			18000,
			"__rafStallWarnLines",
		);
		console.log(`rAF-stall hidden lines (${stallHidden.length}):`);
		for (const l of stallHidden) console.log("  ", l);
		expect(stallHidden.length).toBeGreaterThan(0);

		// Post-detection baseline: this is the snapshot AFTER the
		// hidden flip propagated. Any growth past this point reflects
		// a failure of the hidden short-circuit.
		const baseline = await getSendStats(sessionId);
		console.log("Post-detection baseline send stats:", JSON.stringify(baseline));

		// Sample window: keep the producer running while hidden and
		// confirm sent_bytes stays effectively flat.
		await browser.pause(3000);

		const after = await getSendStats(sessionId);
		console.log("After-window send stats:", JSON.stringify(after));

		// E2E-3: Allow a small allowance (1 KiB) to absorb any
		// in-flight residue still on the IPC channel between the flip
		// and our baseline read.
		const allowance = 1024;
		const delta = after.bytes - baseline.bytes;
		console.log(`Hidden-window byte delta: ${delta} (allowance ${allowance})`);
		expect(delta).toBeLessThanOrEqual(allowance);

		// Sanity: the stub was indeed invoked (proof the lazy default
		// in VisibilityController observed the patched global).
		const stubCalls = await getRafStallScheduledCount();
		console.log("rAF stub scheduledCount:", stubCalls);
		expect(stubCalls).toBeGreaterThan(0);
	});

	it("E2E-2: restoring rAF resumes visibility within ~2s", async () => {
		// Step 1: Restore the real rAF AND restore the original
		// console.warn (this drops the stall-capture wrapper).
		await restoreRafStall();

		// Step 2: NOW install a fresh resume-capture wrapper on top of
		// the just-restored original console.warn so we only see lines
		// emitted AFTER this point. Order matters — doing this BEFORE
		// `restoreRafStall()` would let `restoreRafStall()` clobber
		// our wrapper.
		await browser.execute(() => {
			const w = /** @type {any} */ (window);
			w.__rafStallResumeLines = [];
			w.__rafStallResumeOrigWarn = console.warn.bind(console);
			console.warn = function (...args) {
				try {
					const line = args.map((a) => (typeof a === "string" ? a : String(a))).join(" ");
					w.__rafStallResumeLines.push(line);
				} catch {
					/* ignore */
				}
				return w.__rafStallResumeOrigWarn.apply(null, args);
			};
		});

		// Step 3: Drain the queued rAF callbacks captured by the
		// stall stub. While hidden the controller has
		// `lastNotified !== true` and therefore does NOT schedule new
		// rAFs, so recovery requires at least one PREVIOUSLY-queued cb
		// to fire (matching WebKit's "queued cbs delivered after wake"
		// behaviour).
		await drainRafStallQueue();

		// Poll for `[DIAG-IDLE] visibility→visible` in the resume
		// buffer. After the drained cb fires, the controller flips
		// `rafAlive=true` and re-evaluates, dispatching visible.
		const visibleLines = await waitForWarnLine(
			"(l) => l.includes('[DIAG-IDLE] visibility→visible')",
			2000,
			"__rafStallResumeLines",
			100,
		);
		console.log(`Resume visible lines (${visibleLines.length}):`);
		for (const l of visibleLines) console.log("  ", l);
		expect(visibleLines.length).toBeGreaterThan(0);

		// Restore the original warn binding.
		await browser.execute(() => {
			const w = /** @type {any} */ (window);
			if (typeof w.__rafStallResumeOrigWarn === "function") {
				console.warn = w.__rafStallResumeOrigWarn;
			}
		});

		// Stop the unbounded producer so the terminal is clean for
		// any subsequent specs in the same session.
		await browser.keys(["Control", "c"]);
		await browser.pause(200);

		await browser.saveScreenshot(
			"./screenshots/visibility-raf-heartbeat-after-resume.png",
		);
	});
});
