/**
 * Keyboard Debug E2E Test
 */

async function typeSlowly(text, delay = 100) {
	for (const char of text) {
		await browser.keys(char);
		await browser.pause(delay);
	}
}

describe("Keyboard Debug Test", () => {
	it("should capture keyboard event logs", async () => {
		const terminal = await $("#terminal");
		await terminal.click();
		await browser.pause(500);

		// Set up console log capture
		console.log("Setting up console capture...");
		await browser.execute(() => {
			window.consoleLogs = [];
			const originalLog = console.log;
			console.log = (...args) => {
				window.consoleLogs.push(args.join(" "));
				originalLog.apply(console, args);
			};
		});

		// Type a few characters
		console.log('Typing "ab"...');
		await typeSlowly("ab", 200);
		await browser.pause(500);

		// Press Enter
		console.log("Pressing Enter...");
		await browser.keys("Enter");
		await browser.pause(500);

		// Get captured logs
		const logs = await browser.execute(() => {
			return window.consoleLogs.filter((log) =>
				log.includes("[handleKeyDown]"),
			);
		});

		console.log("=== Captured keyboard logs ===");
		for (const log of logs) {
			console.log(log);
		}
		console.log("=== End of logs ===");

		await browser.saveScreenshot("./screenshots/keyboard-debug-01.png");

		// Check terminal content
		const terminalText = await terminal.getText();
		console.log("Terminal content:", terminalText.slice(0, 100));
	});

	it("should test exit command with log capture", async () => {
		const terminal = await $("#terminal");
		await terminal.click();
		await browser.pause(500);

		// Re-setup console capture (in case it was lost)
		await browser.execute(() => {
			if (!window.consoleLogs) {
				window.consoleLogs = [];
				const originalLog = console.log;
				console.log = (...args) => {
					window.consoleLogs.push(args.join(" "));
					originalLog.apply(console, args);
				};
			}
			window.consoleLogs = []; // Clear previous logs
		});

		// Get initial session ID
		const initialSession = await browser.execute(() => {
			return window.ptyClient?.getSessionId?.() || null;
		});
		console.log("Initial session ID:", initialSession);

		// Type exit
		console.log("Typing exit...");
		await typeSlowly("exit", 150);
		await browser.pause(300);

		// Get logs after typing
		const logsAfterType = await browser.execute(() => {
			return window.consoleLogs.filter((log) =>
				log.includes("[handleKeyDown]"),
			);
		});
		console.log('Logs after typing "exit":', logsAfterType.length, "entries");

		// Press Enter
		console.log("Pressing Enter...");
		await browser.keys("Enter");
		await browser.pause(500);

		// Get all logs including Enter
		const allLogs = await browser.execute(() => {
			return window.consoleLogs.filter((log) =>
				log.includes("[handleKeyDown]"),
			);
		});

		console.log("=== All keyboard logs ===");
		for (const log of allLogs) {
			console.log(log);
		}
		console.log("=== End of logs ===");

		// Wait for potential exit
		await browser.pause(2000);

		// Check session ID after
		const finalSession = await browser.execute(() => {
			return window.ptyClient?.getSessionId?.() || null;
		});
		console.log("Final session ID:", finalSession);
		console.log(
			"Session cleared:",
			initialSession !== null && finalSession === null,
		);

		await browser.saveScreenshot(
			"./screenshots/keyboard-debug-02-after-exit.png",
		);

		// Get terminal content
		const terminalText = await terminal.getText();
		console.log("Terminal content:", terminalText.slice(0, 200));
	});
});
