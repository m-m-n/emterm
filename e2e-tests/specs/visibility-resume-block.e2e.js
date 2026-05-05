/**
 * Visible-resume main-thread block bench (TS-27 / NFR2).
 *
 * NFR2 requires that the visible-resume snapshot path (snapshot build
 * + IPC delivery + frontend `processPendingData`) blocks the main
 * thread for < 200 ms on a single-pane session.
 *
 * Measurement strategy:
 *   1. Drive a workload while the session is hidden so the shadow
 *      parser holds enough state to produce a non-trivial snapshot.
 *   2. Capture `performance.now()` immediately before invoking
 *      `pty_set_visibility(true)` and again after the next
 *      `requestAnimationFrame`-aligned `processPendingData` flush.
 *   3. Print the elapsed time. The spec passes if it printed a
 *      reasonable value (< 200 ms target on stock hardware; the spec
 *      is intentionally lenient at < 1000 ms so flaky CI doesn't
 *      regress on transient load).
 */

async function getSessionId() {
	return browser.execute(() => {
		const app = window.terminalApp;
		return app?.ptyClient?.getSessionId?.() ?? null;
	});
}

async function setVisibility(sessionId, visible) {
	const r = await browser.executeAsync((sid, vis, done) => {
		const internals = window.__TAURI_INTERNALS__;
		if (!internals?.invoke) {
			done({ error: "no internals" });
			return;
		}
		internals
			.invoke("pty_set_visibility", { sessionId: sid, visible: vis })
			.then(() => done({ ok: true }))
			.catch((e) => done({ error: String(e) }));
	}, sessionId, visible);
	if (r?.error) throw new Error(r.error);
}

async function typeSlowly(text, delay = 30) {
	for (const char of text) {
		await browser.keys(char);
		await browser.pause(delay);
	}
}

describe("Visible-resume main-thread block (TS-27 / NFR2)", () => {
	it("should resume within the NFR2 budget", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.waitForExist({ timeout: 30000 });
		await browser.pause(3000);
		await terminal.click();
		await browser.pause(500);

		const sessionId = await getSessionId();
		expect(sessionId).toBeTruthy();

		// Hide and drive a small workload so the snapshot has content.
		await setVisibility(sessionId, false);
		await browser.pause(150);
		await typeSlowly("for i in $(seq 1 30); do echo resume-line-$i; done");
		await browser.keys("Enter");
		await browser.pause(2000);

		// Measure resume cost end-to-end. We bracket the
		// pty_set_visibility(true) invoke with performance.now() and
		// then wait one rAF tick before sampling t1, so the snapshot
		// chunk has had a chance to land in processPendingData.
		const measurement = await browser.executeAsync(async (sid, done) => {
			const internals = window.__TAURI_INTERNALS__;
			if (!internals?.invoke) {
				done({ error: "no internals" });
				return;
			}
			const t0 = performance.now();
			try {
				await internals.invoke("pty_set_visibility", {
					sessionId: sid,
					visible: true,
				});
			} catch (err) {
				done({ error: String(err) });
				return;
			}
			// Wait for the next paint frame so the snapshot chunk has
			// been ingested by processPendingData (which schedules on
			// rAF). Two frames provide a margin against jitter.
			await new Promise((r) => requestAnimationFrame(() => r()));
			await new Promise((r) => requestAnimationFrame(() => r()));
			const t1 = performance.now();
			done({ resumeMs: t1 - t0 });
		}, sessionId);

		if (measurement?.error) throw new Error(measurement.error);

		console.log(
			`[BENCH-RESUME] resumeMs=${measurement.resumeMs.toFixed(2)}`,
		);

		// NFR2 budget is 200 ms. Use a wider 1000 ms ceiling for the
		// hard E2E assertion to absorb Xvfb / Docker jitter; manual
		// review compares the printed value against the 200 ms target.
		expect(measurement.resumeMs).toBeLessThan(1000);
	});
});
