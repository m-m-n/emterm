/**
 * Exit Debug E2E Test - Tests exit with Ctrl+D and longer wait time
 *
 * The "exit" command closes the Tauri window, which invalidates the
 * WebDriver session. Post-exit checks use try/catch to handle this.
 */

async function typeSlowly(text, delay = 150) {
	for (const char of text) {
		await browser.keys(char);
		await browser.pause(delay);
	}
}

describe("Exit Debug Test", () => {
	it("should test exit command with long wait", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.waitForExist({ timeout: 10000 });
		await terminal.click();

		// Wait for shell to be ready
		console.log("Waiting for shell to be ready...");
		await browser.pause(3000);

		// Get initial session ID
		const initialSessionId = await browser.execute(() => {
			const tabs = window.tabManager?.getTabs() || [];
			const terminalTab = tabs.find((t) => t.type === "terminal");
			if (!terminalTab) return null;
			const app = window.tabManager?.getTerminalApp(terminalTab.id);
			return app?.pty?.getSessionId() || null;
		});
		console.log("Initial sessionId:", initialSessionId);
		expect(initialSessionId).not.toBeNull();
		await browser.saveScreenshot("./screenshots/exit-debug-01-initial.png");

		// Type "exit" slowly and clearly
		console.log("Typing exit command...");
		await typeSlowly("exit", 200);
		await browser.pause(1000);
		await browser.saveScreenshot("./screenshots/exit-debug-02-typed.png");

		// Press Enter
		console.log("Pressing Enter...");
		await browser.keys("Enter");
		await browser.pause(500);

		// After exit + Enter, the window may close at any point.
		// All subsequent operations must handle session invalidation.
		try {
			// Send Ctrl+D via WebDriver keys
			console.log("Sending Ctrl+D via WebDriver keys...");
			await browser.keys(["Control", "d"]);
			await browser.pause(1000);

			// Wait and poll for shell exit
			console.log("Waiting for shell to exit...");
			for (let i = 0; i < 5; i++) {
				await browser.pause(1000);
				const sessionId = await browser.execute(() => {
					const tabs = window.tabManager?.getTabs() || [];
					const terminalTab = tabs.find((t) => t.type === "terminal");
					if (!terminalTab) return null;
					const app = window.tabManager?.getTerminalApp(terminalTab.id);
					return app?.pty?.getSessionId() || null;
				});
				console.log(`After ${i + 1}s: sessionId = ${sessionId}`);
				if (sessionId === null) {
					console.log("Session cleared - shell exited!");
					break;
				}
			}

			await browser.saveScreenshot("./screenshots/exit-debug-03-after-wait.png");

			const title = await browser.getTitle();
			console.log("Window still open, title:", title);
		} catch (e) {
			// Session invalidated = window closed = exit succeeded
			console.log("Window closed after exit (session invalidated):", e.message);
		}
	});
});
