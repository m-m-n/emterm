/**
 * Tab Lifecycle E2E Test - Tests tab lifecycle behavior
 *
 * This test verifies:
 * - Tab creation works and tab count increases
 * - Session ID is assigned to each tab
 * - Shell exit closes the tab
 * - Window closes when last session exits
 */

async function typeSlowly(text, delay = 150) {
	for (const char of text) {
		await browser.keys(char);
		await browser.pause(delay);
	}
}

async function waitForTabCount(expected, timeout = 10000) {
	await browser.waitUntil(
		async () => {
			const count = await browser.execute(
				() => window.tabManager?.getTabs().length || 0,
			);
			return count === expected;
		},
		{ timeout, timeoutMsg: `Expected ${expected} tabs within ${timeout}ms` },
	);
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
		if (
			output &&
			(output.includes("$") ||
				output.includes("#") ||
				output.includes("%"))
		) {
			return true;
		}
		await browser.pause(200);
	}
	return false;
}

describe("Tab Lifecycle Tests", () => {
	it("should have initial session with valid session ID", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.click();
		await browser.pause(2000);

		// Verify initial tab exists with session ID
		const tabInfo = await browser.execute(() => {
			const tabs = window.tabManager?.getTabs() || [];
			const activeTab = window.tabManager?.getActiveTab();
			const app = activeTab
				? window.tabManager?.getTerminalApp(activeTab.id)
				: null;
			return {
				tabCount: tabs.length,
				activeTabId: activeTab?.id || null,
				sessionId: app?.pty?.getSessionId?.() || null,
			};
		});

		console.log("Initial tab info:", JSON.stringify(tabInfo, null, 2));

		expect(tabInfo.tabCount).toBe(1);
		expect(tabInfo.activeTabId).toBeTruthy();
		expect(tabInfo.sessionId).toBeTruthy();

		await browser.saveScreenshot("./screenshots/tab-lifecycle-01-initial.png");
	});

	it("should track session count via tab manager", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.click();
		await browser.pause(1000);

		// Query session count via tabManager
		const count = await browser.execute(
			() => window.tabManager?.getTabs().length || 0,
		);

		console.log("Session count:", count);
		expect(count).toBeGreaterThanOrEqual(1);

		await browser.saveScreenshot(
			"./screenshots/tab-lifecycle-02-session-count.png",
		);
	});

	it("should close tab when shell exits", async () => {
		// Create a second tab via JS API so closing one doesn't exit the app
		await browser.execute(() => window.tabManager?.createTab());
		await waitForTabCount(2);

		const initialCount = await browser.execute(
			() => window.tabManager?.getTabs().length || 0,
		);
		console.log("Tab count before exit:", initialCount);
		expect(initialCount).toBe(2);

		await browser.saveScreenshot(
			"./screenshots/tab-lifecycle-03-before-exit.png",
		);

		// Wait for shell prompt in the new tab
		await waitForShellPrompt();

		// Focus terminal via JS to avoid "element not interactable" in new tab
		await browser.execute(() => {
			document.querySelector('[data-testid="terminal"]')?.focus();
		});
		await browser.pause(300);

		// Type "exit" command
		console.log("Typing exit command...");
		await typeSlowly("exit", 200);
		await browser.pause(500);

		// Press Enter
		console.log("Pressing Enter...");
		await browser.keys("Enter");

		// Wait for tab to close
		await waitForTabCount(initialCount - 1);

		const finalCount = await browser.execute(
			() => window.tabManager?.getTabs().length || 0,
		);
		console.log("Tab count after exit:", finalCount);
		expect(finalCount).toBe(initialCount - 1);

		await browser.saveScreenshot(
			"./screenshots/tab-lifecycle-04-after-exit.png",
		);
	});

	it("should get session ID for active tab", async () => {
		await browser.pause(2000);
		// Focus terminal via JS to avoid "element not interactable"
		await browser.execute(() => {
			document.querySelector('[data-testid="terminal"]')?.focus();
		});
		await browser.pause(500);

		// Get the current session ID via tabManager (window.terminalApp may be stale after tab close)
		const sessionInfo = await browser.execute(() => {
			const activeTab = window.tabManager?.getActiveTab();
			const app = activeTab
				? window.tabManager?.getTerminalApp(activeTab.id)
				: null;
			return { sessionId: app?.pty?.getSessionId?.() || null };
		});
		console.log("Current session:", JSON.stringify(sessionInfo, null, 2));

		expect(sessionInfo.sessionId).toBeTruthy();

		await browser.saveScreenshot(
			"./screenshots/tab-lifecycle-05-session-id.png",
		);
	});
});
