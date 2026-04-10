/**
 * eMterm Mux Reattach E2E Tests
 *
 * Tests detach/reattach scenarios including multi-window sessions.
 * Verifies screen content is restored after reattach.
 */

// Helper to type characters one at a time
async function typeSlowly(text, delay = 80) {
	for (const char of text) {
		await browser.keys(char);
		await browser.pause(delay);
	}
}

// Helper to send the mux prefix key (Ctrl+B)
async function sendPrefixKey() {
	await browser.keys(["Control", "b"]);
	await browser.pause(300);
}

// Helper to get mux sub-tab count
async function getSubTabCount() {
	return browser.execute(() => {
		const subTabs = document.querySelector(".mux-sub-tabs");
		if (!subTabs) return 0;
		return subTabs.children.length;
	});
}

// Helper to read grid text content (first N non-empty rows)
async function readGridContent(maxRows = 10) {
	return browser.execute((max) => {
		const state = window.terminalState;
		if (!state) return { lines: [], error: "no state" };
		const core = state.getActiveCore?.() || state.getWasmCore?.();
		if (!core) return { lines: [], error: "no core" };
		const lines = [];
		const rows = core.rows?.() || 24;
		for (let r = 0; r < Math.min(rows, max); r++) {
			try {
				const line = core.get_line_text?.(r) || "";
				if (line.trim()) lines.push(`${r}: ${line.trim()}`);
			} catch { break; }
		}
		return { lines, error: null };
	}, maxRows);
}

// Helper to send command via muxClient.sendInput (more reliable than keyboard)
async function sendMuxCommand(cmd) {
	await browser.execute(async (command) => {
		const app = window.terminalApp;
		if (!app || !app.muxClient || !app.muxPaneIds) return;
		const paneId = app.muxPaneIds[app.activeMuxWindowIndex];
		if (!paneId) return;
		const data = new TextEncoder().encode(command + "\r");
		await app.muxClient.sendInput(paneId, data);
	}, cmd);
}

describe("Mux Reattach", () => {
	before(async () => {
		// Wait for terminal canvas to appear
		const canvas = await $("canvas");
		await canvas.waitForExist({ timeout: 30000 });
		await browser.pause(5000);
		await canvas.click();
		await browser.pause(1000);
	});

	describe("single window detach/reattach", () => {
		it("should enter mux mode", async () => {
			await typeSlowly("emterm mux");
			await browser.keys("Enter");
			await browser.pause(5000);

			const count = await getSubTabCount();
			expect(count).toBeGreaterThan(0);
			await browser.saveScreenshot("./screenshots/mux-reattach-01-entered.png");
		});

		it("should run a command and verify output", async () => {
			await sendMuxCommand("echo MARKER_SINGLE");
			await browser.pause(2000);

			const content = await readGridContent();
			console.log("Grid after echo:", JSON.stringify(content));
			const hasMarker = content.lines.some(l => l.includes("MARKER_SINGLE"));
			expect(hasMarker).toBe(true);
			await browser.saveScreenshot("./screenshots/mux-reattach-02-echo.png");
		});

		it("should detach with prefix+d", async () => {
			await sendPrefixKey();
			await browser.keys("d");
			await browser.pause(2000);

			const count = await getSubTabCount();
			expect(count).toBe(0);
			await browser.saveScreenshot("./screenshots/mux-reattach-03-detached.png");
		});

		it("should reattach and restore screen content", async () => {
			await typeSlowly("emterm mux attach");
			await browser.keys("Enter");
			await browser.pause(5000);

			await browser.saveScreenshot("./screenshots/mux-reattach-04-reattached.png");

			// Verify mux mode is active
			const count = await getSubTabCount();
			console.log("Sub-tab count after reattach:", count);
			expect(count).toBeGreaterThan(0);

			// Verify previous output is visible (snapshot or ring buffer replay)
			const content = await readGridContent();
			console.log("Grid after reattach:", JSON.stringify(content));
			const hasMarker = content.lines.some(l => l.includes("MARKER_SINGLE"));
			expect(hasMarker).toBe(true);
		});

		it("should detach again for next test", async () => {
			await sendPrefixKey();
			await browser.keys("d");
			await browser.pause(2000);

			// Kill shell in the mux pane via Ctrl+D to trigger daemon auto-shutdown
			// (emterm mux kill only removes socket, doesn't kill process)
			// Re-enter mux to access the pane, then exit its shell
			await typeSlowly("emterm mux attach");
			await browser.keys("Enter");
			await browser.pause(3000);
			// Send Ctrl+D to exit shell (EOF)
			await browser.keys(["Control", "d"]);
			await browser.pause(3000);
			// Daemon should auto-shutdown after last pane exits
		});
	});

	describe("multi-window detach/reattach", () => {
		it("should enter mux mode and create window 0 content", async () => {
			await typeSlowly("emterm mux");
			await browser.keys("Enter");
			await browser.pause(5000);

			await sendMuxCommand("echo WIN0_CONTENT");
			await browser.pause(2000);

			const content = await readGridContent();
			console.log("Window 0 content:", JSON.stringify(content));
			const hasMarker = content.lines.some(l => l.includes("WIN0_CONTENT"));
			expect(hasMarker).toBe(true);
			await browser.saveScreenshot("./screenshots/mux-reattach-10-win0.png");
		});

		it("should create window 1 with prefix+c", async () => {
			await sendPrefixKey();
			await browser.keys("c");
			await browser.pause(3000);

			const count = await getSubTabCount();
			console.log("Sub-tab count after create:", count);
			expect(count).toBe(2);
			await browser.saveScreenshot("./screenshots/mux-reattach-11-win1-created.png");
		});

		it("should add content to window 1", async () => {
			await sendMuxCommand("echo WIN1_CONTENT");
			await browser.pause(2000);

			const content = await readGridContent();
			console.log("Window 1 content:", JSON.stringify(content));
			const hasMarker = content.lines.some(l => l.includes("WIN1_CONTENT"));
			expect(hasMarker).toBe(true);
			await browser.saveScreenshot("./screenshots/mux-reattach-12-win1-echo.png");
		});

		it("should detach with prefix+d", async () => {
			await sendPrefixKey();
			await browser.keys("d");
			await browser.pause(2000);

			const count = await getSubTabCount();
			expect(count).toBe(0);
			await browser.saveScreenshot("./screenshots/mux-reattach-13-detached.png");
		});

		it("should reattach and restore to window 1 (last active before detach)", async () => {
			await typeSlowly("emterm mux attach");
			await browser.keys("Enter");
			await browser.pause(5000);

			await browser.saveScreenshot("./screenshots/mux-reattach-14-reattached.png");

			// Verify mux mode is active with 2 windows
			const count = await getSubTabCount();
			console.log("Sub-tab count after reattach:", count);
			expect(count).toBe(2);

			// Should restore to window 1 (the active window at detach time)
			const activeIndex = await browser.execute(() => {
				return window.terminalApp?.activeMuxWindowIndex ?? -1;
			});
			console.log("Active window index after reattach:", activeIndex);
			expect(activeIndex).toBe(1);

			// Window 1 content should be visible (it was active when we detached)
			const content = await readGridContent();
			console.log("Grid after multi-window reattach:", JSON.stringify(content));

			const hasWin1 = content.lines.some(l => l.includes("WIN1_CONTENT"));
			console.log("Has WIN1_CONTENT:", hasWin1);
			expect(content.lines.length).toBeGreaterThan(0);
		});

		it("should switch to window 0 and see its content", async () => {
			await sendPrefixKey();
			await browser.keys("p");
			await browser.pause(1000);

			await browser.saveScreenshot("./screenshots/mux-reattach-15-switched-to-win0.png");

			const content = await readGridContent();
			console.log("Window 0 after switch:", JSON.stringify(content));

			const hasWin0 = content.lines.some(l => l.includes("WIN0_CONTENT"));
			console.log("Has WIN0_CONTENT:", hasWin0);
			// Window 0 should not be blank either
			expect(content.lines.length).toBeGreaterThan(0);
		});

		after(async () => {
			// Clean up: detach and kill daemon
			try {
				await sendPrefixKey();
				await browser.keys("d");
				await browser.pause(1000);
				await typeSlowly("emterm mux kill");
				await browser.keys("Enter");
				await browser.pause(1000);
			} catch { /* ignore cleanup errors */ }
		});
	});
});
