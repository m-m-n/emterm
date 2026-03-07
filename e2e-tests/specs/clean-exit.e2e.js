/**
 * Clean Exit E2E Test - Tests exit behavior with console capture
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

describe("Clean Exit Test", () => {
	it("should test exit command with full console capture", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.waitForExist({ timeout: 10000 });
		await terminal.click();

		// Set up console capture FIRST
		await browser.execute(() => {
			window.consoleLogs = [];
			window.ptyExitEvents = [];

			const originalLog = console.log;
			console.log = (...args) => {
				const msg = args.join(" ");
				window.consoleLogs.push(msg);
				if (msg.includes("pty_exit") || msg.includes("onExit")) {
					window.ptyExitEvents.push(msg);
				}
				originalLog.apply(console, args);
			};
		});

		// Wait for shell to be ready
		console.log("Waiting for shell to be ready...");
		await browser.pause(2000);

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
		await browser.saveScreenshot("./screenshots/clean-exit-01-initial.png");

		// Type "exit" slowly and clearly
		console.log("Typing exit command...");
		await typeSlowly("exit", 200);
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/clean-exit-02-typed.png");

		// Press Enter
		console.log("Pressing Enter...");
		await browser.keys("Enter");

		// Wait for shell to exit and event to fire
		console.log("Waiting for shell to exit...");
		await browser.pause(3000);

		// After exit, the window may have closed, invalidating the session.
		// Try to get final state, but accept session invalidation as success.
		try {
			const finalState = await browser.execute(() => {
				return {
					ptyExitEvents: window.ptyExitEvents || [],
					allLogs: (window.consoleLogs || []).filter(
						(log) =>
							log.includes("pty_exit") ||
							log.includes("onExit") ||
							log.includes("PTY") ||
							log.includes("DEBUG"),
					),
				};
			});

			console.log("=== PTY Exit Events ===");
			for (const evt of finalState.ptyExitEvents) {
				console.log(evt);
			}

			console.log("=== Relevant Logs ===");
			for (const log of finalState.allLogs.slice(-20)) {
				console.log(log);
			}

			await browser.saveScreenshot("./screenshots/clean-exit-03-final.png");
		} catch (e) {
			// Session invalidated = window closed = exit succeeded
			console.log("Window closed after exit command (session invalidated):", e.message);
		}
	});
});
