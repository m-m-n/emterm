/**
 * Clean Exit E2E Test - Tests exit behavior without test interference
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
				// Also track pty_exit specific logs
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
		const initialState = await browser.execute(() => {
			return {
				sessionId: window.ptyClient?.getSessionId?.() || null,
				ptyExists: !!window.ptyClient,
			};
		});
		console.log("Initial state:", JSON.stringify(initialState, null, 2));
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

		// Get final state and captured logs
		const finalState = await browser.execute(() => {
			return {
				sessionId: window.ptyClient?.getSessionId?.() || null,
				ptyExists: !!window.ptyClient,
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

		console.log(
			"Final state:",
			JSON.stringify(
				{
					sessionId: finalState.sessionId,
					ptyExists: finalState.ptyExists,
				},
				null,
				2,
			),
		);

		console.log("=== PTY Exit Events ===");
		for (const evt of finalState.ptyExitEvents) {
			console.log(evt);
		}
		console.log("=== End PTY Exit Events ===");

		console.log("=== Relevant Logs ===");
		for (const log of finalState.allLogs.slice(-20)) {
			console.log(log);
		}
		console.log("=== End Relevant Logs ===");

		await browser.saveScreenshot("./screenshots/clean-exit-03-final.png");

		// Check if window is still open
		try {
			const title = await browser.getTitle();
			console.log("Window still open, title:", title);
		} catch (e) {
			console.log("Window closed:", e.message);
		}

		// Get terminal content
		try {
			const terminalText = await terminal.getText();
			console.log("Terminal content:", terminalText.slice(0, 300));
		} catch (e) {
			console.log("Could not get terminal text:", e.message);
		}
	});
});
