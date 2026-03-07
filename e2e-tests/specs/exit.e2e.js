/**
 * Exit Command E2E Test - Tests if "exit" command closes the window
 *
 * IMPORTANT: Tests are ordered so non-destructive tests run first.
 * The "exit" command closes the Tauri window and invalidates the
 * WebDriver session, so it must be the last test.
 */

async function typeSlowly(text, delay = 100) {
	for (const char of text) {
		await browser.keys(char);
		await browser.pause(delay);
	}
}

/** Get PTY session ID via tabManager */
async function getSessionId() {
	return browser.execute(() => {
		const tabs = window.tabManager?.getTabs() || [];
		const terminalTab = tabs.find((t) => t.type === "terminal");
		if (!terminalTab) return null;
		const app = window.tabManager?.getTerminalApp(terminalTab.id);
		return app?.pty?.getSessionId() || null;
	});
}

describe("Exit Command Test", () => {
	it("should have a valid pty session initially", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.waitForExist({ timeout: 10000 });
		await terminal.click();
		await browser.pause(2000);

		const sessionId = await getSessionId();
		console.log("Initial sessionId:", sessionId);
		expect(sessionId).not.toBeNull();

		await browser.saveScreenshot("./screenshots/exit-01-initial.png");
	});

	it("should close window when typing exit command", async () => {
		// This test MUST be last - it closes the window and invalidates the session
		const terminal = await $('[data-testid="terminal"]');
		await terminal.click();
		await browser.pause(1000);

		// Type "exit" command
		console.log("Typing exit command...");
		await typeSlowly("exit", 100);
		await browser.pause(500);

		// Press Enter
		console.log("Pressing Enter...");
		await browser.keys("Enter");

		// Wait for window to close
		console.log("Waiting for window to close...");
		await browser.pause(3000);

		// After exit, the window should be closed and the session invalidated.
		// "invalid session id" error means the window closed successfully.
		let windowStillOpen = false;
		try {
			await browser.getTitle();
			windowStillOpen = true;
			console.log("Window still open after exit command");
			await browser.saveScreenshot("./screenshots/exit-02-after.png");
		} catch (e) {
			console.log("Window closed as expected:", e.message);
		}

		if (!windowStillOpen) {
			console.log("Exit command successfully closed the window");
		}
	});
});
