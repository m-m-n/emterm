/**
 * E2E Test: Viewer open + Tab switch keyboard input
 *
 * Tests that when ImageViewer is open in Tab A and user switches to Tab B,
 * keyboard input (including Backspace, Ctrl+C) works correctly in Tab B.
 *
 * This is a regression test for a bug where DisplayModeController's
 * keyboard handler blocked all keys even when the viewer's tab was hidden.
 */

async function typeSlowly(text, delay = 150) {
	for (const char of text) {
		await browser.keys(char);
		await browser.pause(delay);
	}
}

async function waitForShellPrompt(timeout = 5000) {
	const startTime = Date.now();
	while (Date.now() - startTime < timeout) {
		const output = await browser.execute(() => {
			const state = window.terminalState;
			if (!state) return "";
			const lines = [];
			for (let i = 0; i < state.totalLines; i++) {
				const line = state.getLineText(i);
				if (line.trim()) lines.push(line);
			}
			return lines[lines.length - 1] || "";
		});
		if (output && (output.includes("$") || output.includes("#") || output.includes("%"))) {
			return true;
		}
		await browser.pause(200);
	}
	return false;
}

// Get terminal output for a specific tab
async function getTabOutput(tabId) {
	return await browser.execute((id) => {
		const app = window.tabManager?.getTerminalApp(id);
		if (!app) return "NO_APP";
		const state = app.terminalState;
		if (!state) return "NO_STATE";
		return state.extractText(0, 0, state.cols - 1, state.rows - 1);
	}, tabId);
}

describe("Viewer + Tab Switch Keyboard Input", () => {
	const imagePath = "/tmp/test.png";

	beforeEach(async () => {
		// Wait for app to be ready
		await browser.pause(2000);
	});

	it("should have working keyboard in Tab B when viewer is open in Tab A", async () => {
		// Step 1: Verify initial state
		const initialTabCount = await browser.execute(() => {
			return window.tabManager?.getTabs().length || 0;
		});
		console.log("Initial tab count:", initialTabCount);
		expect(initialTabCount).toBe(1);

		// Wait for shell prompt in Tab A
		await waitForShellPrompt();
		await browser.saveScreenshot("./screenshots/viewer-tab-01-initial.png");

		// Step 2: Create Tab B first (before opening viewer)
		console.log("Creating Tab B with Ctrl+T...");
		const terminal = await $(".tab-content");
		await terminal.click();
		await browser.pause(500);
		await browser.keys(["Control", "t"]);
		await browser.pause(2000);

		// Verify we have 2 tabs
		const tabCount = await browser.execute(() => {
			return window.tabManager?.getTabs().length || 0;
		});
		console.log("Tab count:", tabCount);
		expect(tabCount).toBe(2);
		await browser.saveScreenshot("./screenshots/viewer-tab-02-tab-b-created.png");

		// Get tab IDs
		const tabs = await browser.execute(() => window.tabManager?.getTabs() || []);
		const tabAId = tabs[0].id;
		const tabBId = tabs[1].id;
		console.log("Tab A ID:", tabAId, "Tab B ID:", tabBId);

		// Step 3: Switch back to Tab A and open image viewer
		console.log("Switching to Tab A to open viewer...");
		await browser.execute((tabId) => {
			window.tabManager?.switchTab(tabId);
		}, tabAId);
		await browser.pause(500);

		// Wait for shell prompt in Tab A
		await waitForShellPrompt();

		// Focus Tab A content
		const tabAContent = await $(".tab-content[style*='display: block']");
		await tabAContent.click();
		await browser.pause(300);

		console.log("Opening image viewer in Tab A...");
		await typeSlowly(`emterm image ${imagePath}`);
		await browser.keys("Enter");
		await browser.pause(2000);

		// Verify viewer is open
		const viewerOpen = await browser.execute(() => {
			const overlay = document.querySelector(".image-viewer-overlay");
			return overlay?.classList.contains("visible") || false;
		});
		console.log("Viewer open in Tab A:", viewerOpen);
		expect(viewerOpen).toBe(true);
		await browser.saveScreenshot("./screenshots/viewer-tab-03-viewer-open.png");

		// Step 4: Switch to Tab B (with viewer still open in Tab A)
		console.log("Switching to Tab B (viewer still open in Tab A)...");
		await browser.execute((tabId) => {
			window.tabManager?.switchTab(tabId);
		}, tabBId);
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/viewer-tab-04-switched-to-tab-b.png");

		// Wait for shell prompt in Tab B
		await waitForShellPrompt();

		// Focus Tab B content
		const activeContent = await $(".tab-content[style*='display: block']");
		await activeContent.click();
		await browser.pause(300);

		// Step 5: Type some text in Tab B
		console.log("Typing 'hello' in Tab B...");
		await typeSlowly("hello");
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/viewer-tab-05-typed-hello.png");

		// Verify 'hello' appears (using tab-specific output)
		const afterHello = await getTabOutput(tabBId);
		console.log("Tab B output:", afterHello.slice(-100));
		console.log("Tab B contains 'hello':", afterHello.includes("hello"));
		expect(afterHello.includes("hello")).toBe(true);

		// Step 6: Test Backspace - delete 'o' to get 'hell'
		console.log("Testing Backspace...");
		await browser.keys("Backspace");
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/viewer-tab-06-after-backspace.png");

		// Verify 'hello' is now 'hell' (last char deleted)
		const afterBackspace = await getTabOutput(tabBId);
		console.log("After backspace output:", afterBackspace.slice(-100));

		// Look for the current input line - should have 'hell' but not 'hello' at the end
		const lines = afterBackspace.split("\n");
		const lastNonEmptyLine = lines.filter(l => l.trim()).pop() || "";
		console.log("Last non-empty line:", lastNonEmptyLine);

		// The line should contain 'hell' but not end with 'hello'
		// (Backspace should have removed the 'o')
		const backspaceWorked = lastNonEmptyLine.includes("hell") && !lastNonEmptyLine.endsWith("hello");
		console.log("Backspace worked:", backspaceWorked);
		expect(backspaceWorked).toBe(true);

		// Step 7: Test Ctrl+C - should cancel current input
		console.log("Testing Ctrl+C...");
		await browser.keys(["Control", "c"]);
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/viewer-tab-07-after-ctrl-c.png");

		// After Ctrl+C, we should get a new prompt
		// Type a marker to verify we're at a fresh prompt
		const marker = `MARKER_${Date.now()}`;
		console.log(`Typing marker: ${marker}`);
		await typeSlowly(`echo ${marker}`);
		await browser.keys("Enter");
		await browser.pause(1000);
		await browser.saveScreenshot("./screenshots/viewer-tab-08-marker-echo.png");

		// Verify marker appears (Ctrl+C worked and we got a new prompt)
		const afterMarker = await getTabOutput(tabBId);
		console.log("Marker appears in output:", afterMarker.includes(marker));
		expect(afterMarker.includes(marker)).toBe(true);

		// Step 8: Verify Tab A's viewer is still open
		console.log("Switching back to Tab A to verify viewer is still open...");
		await browser.execute((tabId) => {
			window.tabManager?.switchTab(tabId);
		}, tabAId);
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/viewer-tab-09-back-to-tab-a.png");

		const viewerStillOpen = await browser.execute(() => {
			const overlay = document.querySelector(".image-viewer-overlay");
			return overlay?.classList.contains("visible") || false;
		});
		console.log("Viewer still open in Tab A:", viewerStillOpen);
		expect(viewerStillOpen).toBe(true);

		// Step 9: Close viewer with Escape
		console.log("Closing viewer with Escape...");
		await browser.keys("Escape");
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/viewer-tab-10-viewer-closed.png");

		const viewerClosed = await browser.execute(() => {
			const overlay = document.querySelector(".image-viewer-overlay");
			return !overlay?.classList.contains("visible");
		});
		console.log("Viewer closed:", viewerClosed);
		expect(viewerClosed).toBe(true);
	});
});
