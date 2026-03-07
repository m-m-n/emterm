/**
 * Startup Render Debug E2E Test
 *
 * Captures the full render flow from startup to detect rendering issues.
 */

describe("Startup Render Test", () => {
	it("should capture render flow on startup", async () => {
		// Wait for terminal to initialize
		await browser.pause(2000);

		// Check if terminal exists
		const terminal = await $('[data-testid="terminal"]');
		const exists = await terminal.isExisting();
		console.log("Terminal element exists:", exists);

		// Get terminal state
		const state = await browser.execute(() => {
			const ts = window.terminalState;
			if (!ts) return null;
			return {
				cols: ts.cols,
				rows: ts.rows,
				cursorCol: ts.cursorCol,
				cursorRow: ts.cursorRow,
			};
		});
		console.log("Terminal state:", JSON.stringify(state));

		// Get renderer state
		const rendererState = await browser.execute(() => {
			const tr = window.terminalRenderer;
			if (!tr) return null;
			const canvas = document.querySelector("canvas");
			return {
				canvasExists: !!canvas,
				canvasWidth: canvas?.width || 0,
				canvasHeight: canvas?.height || 0,
				charWidth: tr["charWidth"] || 0,
				charHeight: tr["charHeight"] || 0,
			};
		});
		console.log("Renderer state:", JSON.stringify(rendererState));

		// Verify canvas exists (Canvas renderer, no DOM line elements)
		const canvasExists = await browser.execute(() => {
			return !!document.querySelector("canvas");
		});
		console.log("Canvas exists:", canvasExists);

		// Get first 5 lines content from buffer
		for (let i = 0; i < 5; i++) {
			const content = await browser.execute((row) => {
				const ts = window.terminalState;
				if (!ts) return null;
				const buffer = ts.getActiveBuffer?.();
				if (!buffer) return null;

				const line = buffer.getLine(row);
				let bufferText = "";
				for (let j = 0; j < line.length; j++) {
					bufferText += line.getCell(j).char;
				}

				return bufferText.trim();
			}, i);
			console.log(`Row ${i}: buffer="${content}"`);
		}

		// Take screenshot
		await browser.saveScreenshot("./screenshots/startup-01-initial.png");

		// Now type a character and check render
		console.log('=== Typing character "a" ===');
		await terminal.click();
		await browser.pause(200);
		await browser.keys("a");
		await browser.pause(500);

		// Check state after typing
		const afterTyping = await browser.execute(() => {
			const ts = window.terminalState;
			if (!ts) return null;
			return {
				cursorCol: ts.cursorCol,
				cursorRow: ts.cursorRow,
				dirtyRows: ts.getDirtyRows?.() || [],
			};
		});
		console.log('After typing "a":', JSON.stringify(afterTyping));

		// Check content at cursor row
		const cursorRow = afterTyping?.cursorRow || 0;
		const rowContent = await browser.execute((row) => {
			const ts = window.terminalState;
			if (!ts) return null;
			const buffer = ts.getActiveBuffer?.();
			if (!buffer) return null;

			const line = buffer.getLine(row);
			let bufferText = "";
			for (let j = 0; j < line.length; j++) {
				bufferText += line.getCell(j).char;
			}

			return bufferText.trim();
		}, cursorRow);
		console.log(`Cursor row ${cursorRow}: buffer="${rowContent}"`);

		await browser.saveScreenshot("./screenshots/startup-02-after-a.png");

		// Press Enter
		console.log("=== Pressing Enter ===");
		await browser.keys("Enter");
		await browser.pause(1000);

		const afterEnter = await browser.execute(() => {
			const ts = window.terminalState;
			if (!ts) return null;
			return {
				cursorCol: ts.cursorCol,
				cursorRow: ts.cursorRow,
			};
		});
		console.log("After Enter:", JSON.stringify(afterEnter));

		await browser.saveScreenshot("./screenshots/startup-03-after-enter.png");

		// Verify terminal is responsive
		expect(state).not.toBeNull();
		expect(canvasExists).toBe(true);
	});
});
