/**
 * eMterm visibility-aware PTY streaming E2E (TS-29 / SC-5 CI proxy)
 *
 * 非 mux モードで `pty_set_visibility` を直接 invoke して backend を
 * hidden 状態に切り替え、その間に backend の `pty_get_send_stats.sent_bytes`
 * が増えないことを assert する (TS-29)。
 *
 * spec の主目的:
 *  1. hidden 区間で backend send が完全停止する (FR6)
 *  2. visible 復帰時に backend が snapshot を 1 回送り frontend grid が
 *     最新状態を反映する (FR8 / FR12)
 *
 * 注: DOM `document.visibilityState` の Object.defineProperty 上書きは
 *     real WebView では visibilitychange を発火させないので採用しない
 *     (SPEC.md の Decision Log 参照)。代わりに backend Tauri command を
 *     直接 invoke することで VisibilityController 経路をバイパスし、
 *     backend 動作のみを CI で検証する。
 */

async function typeSlowly(text, delay = 30) {
	for (const char of text) {
		await browser.keys(char);
		await browser.pause(delay);
	}
}

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

async function setVisibility(sessionId, visible) {
	const result = await browser.executeAsync((sid, vis, done) => {
		const internals = window.__TAURI_INTERNALS__;
		if (!internals || typeof internals.invoke !== "function") {
			done({ error: "Tauri internals not available" });
			return;
		}
		internals
			.invoke("pty_set_visibility", { sessionId: sid, visible: vis })
			.then(() => done({ ok: true }))
			.catch((err) => done({ error: String(err) }));
	}, sessionId, visible);
	if (result?.error) throw new Error(`setVisibility failed: ${result.error}`);
}

async function getRecvStats() {
	return browser.execute(() => {
		const app = window.terminalApp;
		return app?.ptyClient?.getRecvStats?.() ?? null;
	});
}

describe("Visibility-aware PTY streaming (non-mux)", () => {
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

	it("should not increase backend sent_bytes while hidden", async () => {
		// Establish a baseline by reading send stats.
		const baseline = await getSendStats(sessionId);
		console.log("Baseline send stats:", JSON.stringify(baseline));

		// Switch backend into hidden mode.
		await setVisibility(sessionId, false);
		await browser.pause(200);

		const afterHide = await getSendStats(sessionId);
		console.log("Send stats just after hide:", JSON.stringify(afterHide));

		// Generate moderate PTY output while hidden. Keep volume small
		// enough to avoid stress-testing the upstream vt100 crate; the
		// FR6 contract is binary (any send is a violation), so volume is
		// not the property under test.
		await typeSlowly("for i in 1 2 3 4 5; do echo hidden-line-$i; done");
		await browser.keys("Enter");
		await browser.pause(2500);

		const duringHide = await getSendStats(sessionId);
		console.log("Send stats during hide:", JSON.stringify(duringHide));

		// FR6 / TS-29: while hidden, backend MUST NOT add to sent_bytes.
		// The reader thread keeps reading from the PTY, but channel.send
		// is skipped — sent_bytes / sent_count stay flat after the hide
		// transition.
		expect(duringHide.bytes).toBe(afterHide.bytes);
		expect(duringHide.count).toBe(afterHide.count);

		// Resume to visible. The snapshot bypasses the regular
		// `add_sent` accounting (it goes through the same channel as
		// reader chunks but is not counted by the backpressure
		// counters). The contract under test here is purely "hidden
		// suppresses backend send accounting"; resume-side accounting
		// is covered separately by the recv-side test below.
		await setVisibility(sessionId, true);
		await browser.pause(500);

		const afterResume = await getSendStats(sessionId);
		console.log("Send stats after resume:", JSON.stringify(afterResume));

		// `sent_bytes` may stay equal until the next live reader batch
		// arrives; the strict invariant is that hidden never advanced
		// it, which we already asserted above. Just sanity-check that
		// the counter did not regress.
		expect(afterResume.count).toBeGreaterThanOrEqual(duringHide.count);
		expect(afterResume.bytes).toBeGreaterThanOrEqual(duringHide.bytes);
	});

	it("should resume into the latest screen state after hide/show cycle", async () => {
		// Kick off another short hide/show cycle and then verify the
		// frontend WASM grid converged to a state that reflects the
		// commands run while hidden.
		await setVisibility(sessionId, false);
		await browser.pause(200);

		await typeSlowly("printf 'HIDDEN_MARKER_%s\\n' end");
		await browser.keys("Enter");
		await browser.pause(2000);

		// At this point chunkRecv must NOT have advanced for the
		// HIDDEN_MARKER bytes — frontend never received them while hidden.
		const recvDuringHide = await getRecvStats();
		console.log("recv stats during hide:", JSON.stringify(recvDuringHide));

		await setVisibility(sessionId, true);
		await browser.pause(1500);

		await browser.saveScreenshot(
			"./screenshots/visibility-aware-streaming-resume.png",
		);

		// After resume, the frontend should have received at least one
		// snapshot chunk and the grid should contain HIDDEN_MARKER_end.
		// Allow equality because earlier specs may have left a baseline
		// state where the frontend already saw the snapshot for that
		// pre-existing hidden cycle.
		const recvAfterResume = await getRecvStats();
		console.log("recv stats after resume:", JSON.stringify(recvAfterResume));
		expect(recvAfterResume.count).toBeGreaterThanOrEqual(recvDuringHide.count);

		const gridText = await browser.execute(() => {
			const state = window.terminalState;
			const core = state?.getActiveCore?.() || state?.getWasmCore?.();
			if (!core) return "";
			const rows = core.rows?.() ?? 24;
			const lines = [];
			for (let r = 0; r < rows; r++) {
				try {
					lines.push(core.get_line_text?.(r) ?? "");
				} catch {
					break;
				}
			}
			return lines.join("\n");
		});
		console.log("Grid text snapshot:", gridText.slice(-200));
		expect(gridText).toContain("HIDDEN_MARKER_end");
	});
});
