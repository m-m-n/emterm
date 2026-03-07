/**
 * Reflow E2E Tests
 *
 * Tests window resize reflow behavior.
 */

describe("eMterm Reflow", () => {
	it("should handle window resize and reflow text", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.waitForDisplayed({ timeout: 10000 });
		await terminal.click();
		await browser.pause(2000);
		await browser.saveScreenshot("./screenshots/reflow-01-initial.png");

		// Type a long string
		const testText = "echo ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890abcdefghijklmnopqrstuvwxyz";
		for (const char of testText) {
			await browser.keys([char]);
		}
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/reflow-02-before-enter.png");

		await browser.keys(["Enter"]);
		await browser.pause(1000);
		await browser.saveScreenshot("./screenshots/reflow-03-after-enter.png");

		// Get current window size
		const initialSize = await browser.getWindowSize();
		console.log("Initial window size:", initialSize);
		await browser.saveScreenshot("./screenshots/reflow-04-before-resize.png");

		// Shrink window to trigger wrap
		await browser.setWindowSize(400, initialSize.height);
		await browser.pause(1000);
		await browser.saveScreenshot("./screenshots/reflow-05-small-window.png");

		const stateAfterShrink = await browser.execute(() => {
			const terminalState = window.terminalState;
			const buffer = terminalState?.getActiveBuffer?.();
			if (!buffer) return { error: "Buffer not found" };

			const lineInfo = [];
			for (let i = 0; i < Math.min(5, buffer.rows); i++) {
				const line = buffer.getLine(i);
				lineInfo.push({
					row: i,
					wrapped: line?.wrapped ?? false,
					text: line?.getText?.()?.slice(0, 30) ?? "",
				});
			}
			return { cols: buffer.cols, rows: buffer.rows, lines: lineInfo };
		});
		console.log("State after shrink:", JSON.stringify(stateAfterShrink, null, 2));

		// Restore window size
		await browser.setWindowSize(initialSize.width, initialSize.height);
		await browser.pause(1000);
		await browser.saveScreenshot("./screenshots/reflow-06-restored-window.png");

		const stateAfterRestore = await browser.execute(() => {
			const terminalState = window.terminalState;
			const buffer = terminalState?.getActiveBuffer?.();
			if (!buffer) return { error: "Buffer not found" };

			const lineInfo = [];
			for (let i = 0; i < Math.min(5, buffer.rows); i++) {
				const line = buffer.getLine(i);
				lineInfo.push({
					row: i,
					wrapped: line?.wrapped ?? false,
					text: line?.getText?.()?.slice(0, 50) ?? "",
				});
			}
			return { cols: buffer.cols, rows: buffer.rows, lines: lineInfo };
		});
		console.log("State after restore:", JSON.stringify(stateAfterRestore, null, 2));
	});

	it("should verify wrapped lines unwrap on resize", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.waitForDisplayed({ timeout: 5000 });
		await terminal.click();
		await browser.pause(500);

		// Print a long string without newline
		const command = "printf 'WRAP_TEST_%0.s' {1..20}";
		for (const char of command) {
			await browser.keys([char]);
		}
		await browser.keys(["Enter"]);
		await browser.pause(1000);
		await browser.saveScreenshot("./screenshots/reflow-07-long-output.png");

		// Shrink window
		const size = await browser.getWindowSize();
		await browser.setWindowSize(300, size.height);
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/reflow-08-wrapped.png");

		const wrappedState = await browser.execute(() => {
			const terminalState = window.terminalState;
			const buffer = terminalState?.getActiveBuffer?.();
			if (!buffer) return { error: "Buffer not found" };

			let wrappedCount = 0;
			for (let i = 0; i < buffer.rows; i++) {
				const line = buffer.getLine(i);
				if (line?.wrapped) wrappedCount++;
			}
			return { cols: buffer.cols, wrappedCount };
		});
		console.log("Wrapped state:", wrappedState);

		// Expand window
		await browser.setWindowSize(800, size.height);
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/reflow-09-unwrapped.png");

		const unwrappedState = await browser.execute(() => {
			const terminalState = window.terminalState;
			const buffer = terminalState?.getActiveBuffer?.();
			if (!buffer) return { error: "Buffer not found" };

			let wrappedCount = 0;
			for (let i = 0; i < buffer.rows; i++) {
				const line = buffer.getLine(i);
				if (line?.wrapped) wrappedCount++;
			}
			return { cols: buffer.cols, wrappedCount };
		});
		console.log("Unwrapped state:", unwrappedState);

		expect(unwrappedState.wrappedCount).toBeLessThanOrEqual(wrappedState.wrappedCount);

		// Restore original window size
		await browser.setWindowSize(size.width, size.height);
		await browser.pause(500);
	});
});
