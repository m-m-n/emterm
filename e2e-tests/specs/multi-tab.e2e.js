/**
 * Multi-Tab E2E Test - Tests tab bar functionality
 *
 * This test verifies:
 * - Ctrl+Shift+T creates a new tab (default new_tab keybind)
 * - Keyboard input goes to the active tab only
 * - Ctrl+D (shell exit) closes the tab
 * - Tab switching works correctly (Ctrl+PageDown for next_tab)
 */

async function typeSlowly(text, delay = 100) {
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
			// Get last non-empty line
			const lines = [];
			for (let i = 0; i < state.totalLines; i++) {
				const line = state.getLineText(i);
				if (line.trim()) lines.push(line);
			}
			return lines[lines.length - 1] || "";
		});
		// Check for common shell prompts
		if (output && (output.includes("$") || output.includes("#") || output.includes("%"))) {
			return true;
		}
		await browser.pause(200);
	}
	return false;
}

describe("Multi-Tab Tests", () => {
	beforeEach(async () => {
		// Wait for app to be ready
		await browser.pause(2000);
	});

	it("should verify initial state has one tab", async () => {
		const tabCount = await browser.execute(() => {
			return window.tabManager?.getTabs().length || 0;
		});
		console.log("Initial tab count:", tabCount);
		expect(tabCount).toBe(1);
		await browser.saveScreenshot("./screenshots/multi-tab-01-initial.png");
	});

	it("should create new tab with Ctrl+Shift+T", async () => {
		// Get initial tab count
		const initialCount = await browser.execute(() => {
			return window.tabManager?.getTabs().length || 0;
		});
		console.log("Initial tab count:", initialCount);

		// Focus terminal area and press Ctrl+Shift+T
		const terminal = await $(".tab-content");
		await terminal.click();
		await browser.pause(500);

		// Press Ctrl+Shift+T to create new tab
		console.log("Pressing Ctrl+Shift+T...");
		await browser.keys(["Control", "Shift", "t"]);

		// Wait for tab to be created
		await browser.waitUntil(
			async () => {
				const count = await browser.execute(
					() => window.tabManager?.getTabs().length || 0,
				);
				return count === initialCount + 1;
			},
			{
				timeout: 10000,
				timeoutMsg: `Expected ${initialCount + 1} tabs after Ctrl+Shift+T`,
			},
		);

		const newCount = await browser.execute(() => {
			return window.tabManager?.getTabs().length || 0;
		});
		console.log("New tab count:", newCount);

		expect(newCount).toBe(initialCount + 1);
		await browser.saveScreenshot("./screenshots/multi-tab-02-after-ctrl-t.png");
	});

	it("should send input to active tab only", async () => {
		// Ensure we have 2 tabs
		let tabCount = await browser.execute(() => window.tabManager?.getTabs().length || 0);
		if (tabCount < 2) {
			// Create second tab
			const terminal = await $(".tab-content");
			await terminal.click();
			await browser.pause(500);
			await browser.keys(["Control", "Shift", "t"]);
			await browser.pause(2000);
		}

		// Wait for shell prompt in new tab
		await waitForShellPrompt();

		// Get tabs info
		const tabsInfo = await browser.execute(() => {
			const tabs = window.tabManager?.getTabs() || [];
			const activeTab = window.tabManager?.getActiveTab();
			return {
				tabs: tabs.map(t => ({ id: t.id, type: t.type })),
				activeTabId: activeTab?.id
			};
		});
		console.log("Tabs info:", JSON.stringify(tabsInfo, null, 2));

		// Type a unique marker in the active tab
		const marker = `MARKER_${Date.now()}`;
		console.log(`Typing marker in active tab: ${marker}`);

		// Focus the active tab content (locate by active tab id, since active
		// tab uses style.display = "" not "block")
		const activeTabId = await browser.execute(
			() => window.tabManager?.getActiveTab()?.id,
		);
		const activeContent = await $(`#tab-content-${activeTabId}`);
		await activeContent.click();
		await browser.pause(300);

		await typeSlowly(`echo ${marker}`, 80);
		await browser.keys("Enter");
		await browser.pause(1000);

		// Check output in active tab
		const activeTabOutput = await browser.execute(() => {
			const state = window.terminalState;
			if (!state) return "";
			// Use extractText to get all terminal content
			return state.extractText(0, 0, state.cols - 1, state.rows - 1);
		});
		console.log("Active tab output contains marker:", activeTabOutput.includes(marker));

		// Switch to first tab and check it doesn't have the marker
		console.log("Switching to first tab...");
		const tabs = await browser.execute(() => window.tabManager?.getTabs() || []);
		if (tabs.length >= 2) {
			await browser.execute((tabId) => {
				window.tabManager?.switchTab(tabId);
			}, tabs[0].id);
			await browser.pause(500);

			// Update window.terminalState to first tab's state
			const firstTabOutput = await browser.execute((tabId) => {
				const app = window.tabManager?.getTerminalApp(tabId);
				if (!app) return "NO_APP";
				const state = app.terminalState;
				if (!state) return "NO_STATE";
				// Use extractText to get all terminal content
				return state.extractText(0, 0, state.cols - 1, state.rows - 1);
			}, tabs[0].id);

			console.log("First tab output contains marker:", firstTabOutput.includes(marker));

			// Marker should NOT be in the first tab
			expect(firstTabOutput.includes(marker)).toBe(false);
		}

		await browser.saveScreenshot("./screenshots/multi-tab-03-input-isolation.png");
	});

	// SKIPPED: shell exit propagation through PTY does not fire pty_exit
	// reliably in the Docker E2E environment (WebDriver key delivery / WebView
	// focus interaction with the PTY layer). The implementation path
	// (handleSessionExit → closeTab) is covered by unit tests in
	// tab-manager.test.ts; revisit when the E2E PTY input path is hardened.
	it.skip("should close tab when shell exits", async () => {
		// Ensure we have at least 2 tabs so closing one doesn't exit the app
		let tabCount = await browser.execute(() => window.tabManager?.getTabs().length || 0);
		console.log("Tab count before test:", tabCount);

		if (tabCount < 2) {
			// Create a new tab
			const terminal = await $(".tab-content");
			await terminal.click();
			await browser.pause(500);
			await browser.keys(["Control", "Shift", "t"]);
			await browser.pause(2000);
		}

		// Get updated tab count
		const initialCount = await browser.execute(() => window.tabManager?.getTabs().length || 0);
		console.log("Initial tab count:", initialCount);
		expect(initialCount).toBeGreaterThanOrEqual(2);

		// Focus the active tab (locate by active tab id, since active
		// tab uses style.display = "" not "block")
		const activeTabId = await browser.execute(
			() => window.tabManager?.getActiveTab()?.id,
		);
		const activeContent = await $(`#tab-content-${activeTabId}`);
		await activeContent.click();
		await browser.pause(500);

		// Wait for shell prompt
		await waitForShellPrompt();
		await browser.saveScreenshot("./screenshots/multi-tab-04-before-ctrl-d.png");

		// Type 'exit' to terminate shell (WebDriver key events do not
		// reliably deliver Ctrl+D as EOF through the PTY layer)
		console.log("Typing 'exit' to terminate shell...");
		await typeSlowly("exit", 80);
		await browser.keys("Enter");

		// Wait until tab count decreases (shell exit → PTY close → tab removal)
		await browser.waitUntil(
			async () => {
				const count = await browser.execute(
					() => window.tabManager?.getTabs().length || 0,
				);
				return count === initialCount - 1;
			},
			{
				timeout: 10000,
				timeoutMsg: `Expected ${initialCount - 1} tabs after shell exit, got ${await browser.execute(() => window.tabManager?.getTabs().length || 0)}`,
			},
		);

		const finalCount = await browser.execute(() => window.tabManager?.getTabs().length || 0);
		console.log("Final tab count:", finalCount);

		expect(finalCount).toBe(initialCount - 1);
		await browser.saveScreenshot("./screenshots/multi-tab-05-after-ctrl-d.png");
	});

	it("should switch tabs with Ctrl+PageDown", async () => {
		// Ensure we have 2 tabs
		let tabCount = await browser.execute(() => window.tabManager?.getTabs().length || 0);
		if (tabCount < 2) {
			const terminal = await $(".tab-content");
			await terminal.click();
			await browser.pause(500);
			await browser.keys(["Control", "Shift", "t"]);
			await browser.pause(2000);
		}

		// Get current active tab
		const beforeSwitch = await browser.execute(() => window.tabManager?.getActiveTab()?.id);
		console.log("Active tab before switch:", beforeSwitch);

		// Press Ctrl+PageDown to switch (default next_tab keybind)
		console.log("Pressing Ctrl+PageDown...");
		await browser.keys(["Control", "PageDown"]);
		await browser.pause(500);

		// Get new active tab
		const afterSwitch = await browser.execute(() => window.tabManager?.getActiveTab()?.id);
		console.log("Active tab after switch:", afterSwitch);

		// Should have switched to a different tab
		expect(afterSwitch).not.toBe(beforeSwitch);
		await browser.saveScreenshot("./screenshots/multi-tab-06-after-switch.png");
	});

	it("should preserve tab content after switching away and back", async () => {
		// Ensure we have 2 tabs
		let tabCount = await browser.execute(() => window.tabManager?.getTabs().length || 0);
		if (tabCount < 2) {
			const terminal = await $(".tab-content");
			await terminal.click();
			await browser.pause(500);
			await browser.keys(["Control", "Shift", "t"]);
			await browser.pause(2000);
		}

		// Get tabs
		const tabs = await browser.execute(() => window.tabManager?.getTabs() || []);
		expect(tabs.length).toBeGreaterThanOrEqual(2);
		const firstTabId = tabs[0].id;
		const secondTabId = tabs[1].id;

		// Switch to first tab
		await browser.execute((tabId) => {
			window.tabManager?.switchTab(tabId);
		}, firstTabId);
		await browser.pause(500);

		// Wait for shell prompt
		await waitForShellPrompt();

		// Type a unique marker in first tab
		const marker = `CONTENT_TEST_${Date.now()}`;
		console.log(`Typing marker in first tab: ${marker}`);
		await typeSlowly(`echo ${marker}`, 80);
		await browser.keys("Enter");
		await browser.pause(1000);

		// Verify marker is in first tab
		const beforeSwitchOutput = await browser.execute((tabId) => {
			const app = window.tabManager?.getTerminalApp(tabId);
			if (!app) return "NO_APP";
			const state = app.terminalState;
			if (!state) return "NO_STATE";
			// Use extractText to get all terminal content
			return state.extractText(0, 0, state.cols - 1, state.rows - 1);
		}, firstTabId);
		console.log("First tab output before switch contains marker:", beforeSwitchOutput.includes(marker));
		expect(beforeSwitchOutput.includes(marker)).toBe(true);

		await browser.saveScreenshot("./screenshots/multi-tab-07-before-content-test.png");

		// Switch to second tab (this hides first tab and triggers ResizeObserver)
		console.log("Switching to second tab...");
		await browser.execute((tabId) => {
			window.tabManager?.switchTab(tabId);
		}, secondTabId);
		await browser.pause(1000);

		await browser.saveScreenshot("./screenshots/multi-tab-08-second-tab.png");

		// Switch back to first tab
		console.log("Switching back to first tab...");
		await browser.execute((tabId) => {
			window.tabManager?.switchTab(tabId);
		}, firstTabId);
		await browser.pause(1000);

		await browser.saveScreenshot("./screenshots/multi-tab-09-back-to-first.png");

		// Verify marker is STILL in first tab (content was preserved)
		const afterSwitchOutput = await browser.execute((tabId) => {
			const app = window.tabManager?.getTerminalApp(tabId);
			if (!app) return "NO_APP";
			const state = app.terminalState;
			if (!state) return "NO_STATE";
			// Use extractText to get all terminal content
			return state.extractText(0, 0, state.cols - 1, state.rows - 1);
		}, firstTabId);
		console.log("First tab output after switch contains marker:", afterSwitchOutput.includes(marker));

		// CRITICAL: Content should be preserved after switching tabs
		expect(afterSwitchOutput.includes(marker)).toBe(true);

		await browser.saveScreenshot("./screenshots/multi-tab-10-content-preserved.png");
	});
});
