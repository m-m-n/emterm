/**
 * Multi-Tab E2E Test - Tests tab bar functionality
 *
 * This test verifies:
 * - Ctrl+T creates a new tab
 * - Keyboard input goes to the active tab only
 * - Ctrl+D (shell exit) closes the tab
 * - Tab switching works correctly
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

	it("should create new tab with Ctrl+T", async () => {
		// Get initial tab count
		const initialCount = await browser.execute(() => {
			return window.tabManager?.getTabs().length || 0;
		});
		console.log("Initial tab count:", initialCount);

		// Focus terminal area and press Ctrl+T
		const terminal = await $(".tab-content");
		await terminal.click();
		await browser.pause(500);

		// Press Ctrl+T to create new tab
		console.log("Pressing Ctrl+T...");
		await browser.keys(["Control", "t"]);
		await browser.pause(2000); // Wait for tab creation and PTY spawn

		// Verify tab was created
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
			await browser.keys(["Control", "t"]);
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

		// Focus the active tab content
		const activeContent = await $(".tab-content[style*='display: block']");
		await activeContent.click();
		await browser.pause(300);

		await typeSlowly(`echo ${marker}`, 80);
		await browser.keys("Enter");
		await browser.pause(1000);

		// Check output in active tab
		const activeTabOutput = await browser.execute(() => {
			const state = window.terminalState;
			if (!state) return "";
			const lines = [];
			for (let i = 0; i < state.totalLines; i++) {
				lines.push(state.getLineText(i));
			}
			return lines.join("\n");
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
				const lines = [];
				for (let i = 0; i < state.totalLines; i++) {
					lines.push(state.getLineText(i));
				}
				return lines.join("\n");
			}, tabs[0].id);

			console.log("First tab output contains marker:", firstTabOutput.includes(marker));

			// Marker should NOT be in the first tab
			expect(firstTabOutput.includes(marker)).toBe(false);
		}

		await browser.saveScreenshot("./screenshots/multi-tab-03-input-isolation.png");
	});

	it("should close tab when shell exits with Ctrl+D", async () => {
		// Ensure we have at least 2 tabs so closing one doesn't exit the app
		let tabCount = await browser.execute(() => window.tabManager?.getTabs().length || 0);
		console.log("Tab count before test:", tabCount);

		if (tabCount < 2) {
			// Create a new tab
			const terminal = await $(".tab-content");
			await terminal.click();
			await browser.pause(500);
			await browser.keys(["Control", "t"]);
			await browser.pause(2000);
		}

		// Get updated tab count
		const initialCount = await browser.execute(() => window.tabManager?.getTabs().length || 0);
		console.log("Initial tab count:", initialCount);
		expect(initialCount).toBeGreaterThanOrEqual(2);

		// Focus the active tab
		const activeContent = await $(".tab-content[style*='display: block']");
		await activeContent.click();
		await browser.pause(500);

		// Wait for shell prompt
		await waitForShellPrompt();
		await browser.saveScreenshot("./screenshots/multi-tab-04-before-ctrl-d.png");

		// Press Ctrl+D to exit shell
		console.log("Pressing Ctrl+D...");
		await browser.keys(["Control", "d"]);
		await browser.pause(2000);

		// Verify tab was closed
		const finalCount = await browser.execute(() => window.tabManager?.getTabs().length || 0);
		console.log("Final tab count:", finalCount);

		expect(finalCount).toBe(initialCount - 1);
		await browser.saveScreenshot("./screenshots/multi-tab-05-after-ctrl-d.png");
	});

	it("should switch tabs with Ctrl+Tab", async () => {
		// Ensure we have 2 tabs
		let tabCount = await browser.execute(() => window.tabManager?.getTabs().length || 0);
		if (tabCount < 2) {
			const terminal = await $(".tab-content");
			await terminal.click();
			await browser.pause(500);
			await browser.keys(["Control", "t"]);
			await browser.pause(2000);
		}

		// Get current active tab
		const beforeSwitch = await browser.execute(() => window.tabManager?.getActiveTab()?.id);
		console.log("Active tab before switch:", beforeSwitch);

		// Press Ctrl+Tab to switch
		console.log("Pressing Ctrl+Tab...");
		await browser.keys(["Control", "Tab"]);
		await browser.pause(500);

		// Get new active tab
		const afterSwitch = await browser.execute(() => window.tabManager?.getActiveTab()?.id);
		console.log("Active tab after switch:", afterSwitch);

		// Should have switched to a different tab
		expect(afterSwitch).not.toBe(beforeSwitch);
		await browser.saveScreenshot("./screenshots/multi-tab-06-after-switch.png");
	});
});
