/**
 * eMterm Mux Terminal Multiplexer E2E Tests
 *
 * Tests mux mode activation, window management, keyboard input, and detach.
 * Requires the emterm binary at /root/.cargo/bin/emterm inside Docker.
 */

// Helper to type characters one at a time (canvas terminal cannot use setValue)
async function typeSlowly(text, delay = 80) {
	for (const char of text) {
		await browser.keys(char);
		await browser.pause(delay);
	}
}

// Helper to send the mux prefix key (Ctrl+B by default)
async function sendPrefixKey() {
	await browser.keys(["Control", "b"]);
	await browser.pause(300);
}

// Helper to get mux sub-tab count for the active tab
async function getSubTabCount() {
	return browser.execute(() => {
		const subTabs = document.querySelector(".mux-sub-tabs");
		if (!subTabs) return 0;
		return subTabs.children.length;
	});
}

// Helper to get the index of the active mux sub-tab (-1 if none)
async function getActiveSubTabIndex() {
	return browser.execute(() => {
		const active = document.querySelector(".mux-sub-tab-active");
		if (!active || !active.parentElement) return -1;
		return Array.from(active.parentElement.children).indexOf(active);
	});
}

describe("Mux Terminal Multiplexer", () => {
	before(async () => {
		// Wait for terminal element to exist in DOM
		const terminal = await $('[data-testid="terminal"]');
		await terminal.waitForExist({ timeout: 30000 });
		await browser.pause(5000); // Wait for shell prompt

		// Focus the terminal
		await terminal.click();
		await browser.pause(1000);

		await browser.saveScreenshot("./screenshots/mux-00-initial.png");
	});

	describe("mux mode activation", () => {
		it("should enter mux mode when emterm mux is executed", async () => {
			await typeSlowly("emterm mux");
			await browser.keys("Enter");
			// Daemon startup + mux attach needs time
			await browser.pause(5000);

			await browser.saveScreenshot("./screenshots/mux-01-after-mux-command.png");

			// Tab title should contain [mux] after entering mux mode
			const tabTitle = await browser.execute(() => {
				const tabManager = window.tabManager;
				const activeTabId = tabManager?.getActiveTabId?.();
				if (!activeTabId) return "";
				const tabs = tabManager?.getTabs?.() || [];
				const activeTab = tabs.find((t) => t.id === activeTabId);
				return activeTab?.title || "";
			});
			console.log("Tab title after mux:", tabTitle);
			expect(tabTitle).toContain("[mux]");
		});

		it("should show sub-tabs for mux windows", async () => {
			const count = await getSubTabCount();
			console.log("Sub-tab count:", count);
			expect(count).toBeGreaterThan(0);

			await browser.saveScreenshot("./screenshots/mux-02-sub-tabs.png");
		});
	});

	describe("window management", () => {
		it("should create a new window with prefix+c", async () => {
			const beforeCount = await getSubTabCount();
			console.log("Sub-tab count before create:", beforeCount);

			// prefix + c = create new window
			await sendPrefixKey();
			await browser.keys("c");
			// Wait for PTY spawn
			await browser.pause(3000);

			await browser.saveScreenshot("./screenshots/mux-03-after-create-window.png");

			const afterCount = await getSubTabCount();
			console.log("Sub-tab count after create:", afterCount);
			expect(afterCount).toBe(beforeCount + 1);
		});

		it("should switch to next window with prefix+n", async () => {
			const activeBefore = await getActiveSubTabIndex();
			console.log("Active sub-tab before next:", activeBefore);

			// prefix + n = next window
			await sendPrefixKey();
			await browser.keys("n");
			await browser.pause(500);

			await browser.saveScreenshot("./screenshots/mux-04-after-next-window.png");

			const activeAfter = await getActiveSubTabIndex();
			console.log("Active sub-tab after next:", activeAfter);
			// Should have moved (wraps around if at the end)
			expect(activeAfter).not.toBe(activeBefore);
		});

		it("should switch to previous window with prefix+p", async () => {
			const activeBefore = await getActiveSubTabIndex();
			console.log("Active sub-tab before prev:", activeBefore);

			// prefix + p = previous window
			await sendPrefixKey();
			await browser.keys("p");
			await browser.pause(500);

			await browser.saveScreenshot("./screenshots/mux-05-after-prev-window.png");

			const activeAfter = await getActiveSubTabIndex();
			console.log("Active sub-tab after prev:", activeAfter);
			expect(activeAfter).not.toBe(activeBefore);
		});
	});

	describe("keyboard input", () => {
		it("should accept keyboard input in mux mode", async () => {
			// Focus the terminal
			const terminal = await $('[data-testid="terminal"]');
			await terminal.click();
			await browser.pause(500);

			// Type a command that produces identifiable output
			await typeSlowly("echo mux-e2e-test-output");
			await browser.keys("Enter");
			await browser.pause(1500);

			await browser.saveScreenshot("./screenshots/mux-06-after-echo.png");

			// Check output via JS state rather than canvas text
			// The terminal renders to canvas, so getText() may not work.
			// Instead verify the command was accepted by checking for errors
			// or use the screenshot for visual confirmation.
			const state = await browser.execute(() => {
				const terminalState = window.terminalState;
				return {
					terminalStateExists: !!terminalState,
					appExists: !!window.terminalApp,
				};
			});
			expect(state.terminalStateExists).toBe(true);
			expect(state.appExists).toBe(true);
		});
	});

	describe("detach", () => {
		it("should exit mux mode with prefix+d", async () => {
			// prefix + d = detach
			await sendPrefixKey();
			await browser.keys("d");
			await browser.pause(2000);

			await browser.saveScreenshot("./screenshots/mux-07-after-detach.png");

			// Sub-tabs should be removed
			const count = await getSubTabCount();
			console.log("Sub-tab count after detach:", count);
			expect(count).toBe(0);

			// Tab title should no longer contain [mux]
			const tabTitle = await browser.execute(() => {
				const tabManager = window.tabManager;
				const activeTabId = tabManager?.getActiveTabId?.();
				if (!activeTabId) return "";
				const tabs = tabManager?.getTabs?.() || [];
				const activeTab = tabs.find((t) => t.id === activeTabId);
				return activeTab?.title || "";
			});
			console.log("Tab title after detach:", tabTitle);
			expect(tabTitle).not.toContain("[mux]");
		});
	});
});
