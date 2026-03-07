/**
 * Render Debug E2E Test
 *
 * Investigates the rendering issue where keystrokes are processed
 * but the display doesn't update.
 */

describe("Render Debug Test", () => {
	async function captureRenderLogs() {
		return await browser.execute(() => {
			return (
				window.consoleLogs?.filter(
					(log) =>
						log.includes("[scheduleRender]") ||
						log.includes("[render]") ||
						log.includes("[flushPendingTerminalActions]"),
				) || []
			);
		});
	}

	async function getTerminalState() {
		return await browser.execute(() => {
			const ts = window.terminalState;
			if (!ts) return null;
			return {
				cols: ts.cols,
				rows: ts.rows,
				cursorCol: ts.cursorCol,
				cursorRow: ts.cursorRow,
				dirtyRows: ts.getDirtyRows?.() || [],
			};
		});
	}

	async function getRendererState() {
		return await browser.execute(() => {
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
	}

	async function getBufferContent(row) {
		return await browser.execute((r) => {
			const ts = window.terminalState;
			if (!ts) return null;
			const buffer = ts.getActiveBuffer?.();
			if (!buffer) return null;
			const line = buffer.getLine(r);
			let text = "";
			for (let i = 0; i < line.length; i++) {
				text += line.getCell(i).char;
			}
			return text.trim();
		}, row);
	}

	async function getCanvasContent(row) {
		return await browser.execute((r) => {
			const ts = window.terminalState;
			if (!ts) return null;
			const buffer = ts.getActiveBuffer?.();
			if (!buffer) return null;
			const line = buffer.getLine(r);
			let text = "";
			for (let i = 0; i < line.length; i++) {
				text += line.getCell(i).char;
			}
			return text.trim();
		}, row);
	}

	it("should verify initial render works", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.click();
		await browser.pause(1000);

		// Set up console log capture
		await browser.execute(() => {
			window.consoleLogs = [];
			const originalLog = console.log;
			console.log = (...args) => {
				window.consoleLogs.push(args.join(" "));
				originalLog.apply(console, args);
			};
		});

		// Get initial states
		const termState = await getTerminalState();
		const rendererState = await getRendererState();

		console.log("=== Initial State ===");
		console.log("Terminal State:", JSON.stringify(termState));
		console.log("Renderer State:", JSON.stringify(rendererState));

		// Get first few lines content
		for (let i = 0; i < 5; i++) {
			const bufContent = await getBufferContent(i);
			const domContent = await getCanvasContent(i);
			console.log(`Row ${i} - Buffer: "${bufContent}" | Canvas: "${domContent}"`);
		}

		await browser.saveScreenshot("./screenshots/render-debug-01-initial.png");
	});

	it("should track render after single keystroke", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.click();
		await browser.pause(500);

		// Clear logs
		await browser.execute(() => {
			window.consoleLogs = [];
		});

		// Get cursor position before keystroke
		const beforeState = await getTerminalState();
		console.log("=== Before keystroke ===");
		console.log("Cursor:", beforeState?.cursorCol, beforeState?.cursorRow);

		// Get Canvas content before
		const cursorRow = beforeState?.cursorRow || 0;
		const domBefore = await getCanvasContent(cursorRow);
		console.log("Canvas content before:", domBefore);

		// Type a single character
		console.log('Pressing "a"...');
		await browser.keys("a");
		await browser.pause(500);

		// Get logs
		const logs = await captureRenderLogs();
		console.log("=== Render logs ===");
		for (const log of logs) {
			console.log(log);
		}

		// Get state after keystroke
		const afterState = await getTerminalState();
		console.log("=== After keystroke ===");
		console.log("Cursor:", afterState?.cursorCol, afterState?.cursorRow);
		console.log("Dirty rows:", afterState?.dirtyRows);

		// Compare buffer vs DOM
		const bufAfter = await getBufferContent(cursorRow);
		const domAfter = await getCanvasContent(cursorRow);
		console.log("Buffer content:", bufAfter);
		console.log("Canvas content:", domAfter);
		console.log("Match:", bufAfter === domAfter);

		await browser.saveScreenshot(
			"./screenshots/render-debug-02-after-keystroke.png",
		);
	});

	it("should verify render after forceRender", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.click();
		await browser.pause(500);

		// Clear logs
		await browser.execute(() => {
			window.consoleLogs = [];
		});

		// Force render
		await browser.execute(() => {
			if (window.terminalRenderer && window.terminalState) {
				window.terminalRenderer.forceRender(window.terminalState);
			}
		});

		await browser.pause(300);

		// Get state
		const beforeState = await getTerminalState();
		const cursorRow = beforeState?.cursorRow || 0;

		// Type a character
		console.log('Pressing "x" after forceRender...');
		await browser.keys("x");
		await browser.pause(500);

		// Get logs
		const logs = await captureRenderLogs();
		console.log("=== Render logs (after forceRender) ===");
		for (const log of logs) {
			console.log(log);
		}

		// Compare buffer content
		const bufAfter = await getBufferContent(cursorRow);
		const canvasAfter = await getCanvasContent(cursorRow);
		console.log("Buffer content:", bufAfter);
		console.log("Canvas content:", canvasAfter);
		console.log("Match:", bufAfter === canvasAfter);

		await browser.saveScreenshot(
			"./screenshots/render-debug-03-force-render.png",
		);
	});

	it("should test forceRender after typing", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.click();
		await browser.pause(500);

		// Get current state
		const state = await getTerminalState();
		const cursorRow = state?.cursorRow || 0;

		// Clear logs
		await browser.execute(() => {
			window.consoleLogs = [];
		});

		// Type a character
		console.log('Pressing "z" then forceRender...');
		await browser.keys("z");
		await browser.pause(500);

		// Force full re-render
		await browser.execute(() => {
			if (window.terminalRenderer && window.terminalState) {
				window.terminalRenderer.forceRender(window.terminalState);
			}
		});
		await browser.pause(200);

		// Get logs
		const logs = await captureRenderLogs();
		console.log("=== Render logs (after forceRender) ===");
		for (const log of logs) {
			console.log(log);
		}

		// Compare
		const bufAfter = await getBufferContent(cursorRow);
		const canvasAfter = await getCanvasContent(cursorRow);
		console.log("Buffer content:", bufAfter);
		console.log("Canvas content:", canvasAfter);
		console.log("Match:", bufAfter === canvasAfter);

		await browser.saveScreenshot(
			"./screenshots/render-debug-04-force-render.png",
		);
	});

	it("should verify canvas pixel content after typing", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.click();
		await browser.pause(500);

		// Type a character
		console.log('Pressing "t" and checking canvas pixels...');
		await browser.keys("t");
		await browser.pause(500);

		// Check that the canvas has non-empty pixel data at the cursor row
		const pixelCheck = await browser.execute(() => {
			const canvas = document.querySelector("canvas");
			if (!canvas) return { error: "No canvas" };
			const ctx = canvas.getContext("2d");
			if (!ctx) return { error: "No context" };

			const renderer = window.terminalRenderer;
			const ts = window.terminalState;
			if (!renderer || !ts) return { error: "No renderer or state" };

			const charWidth = renderer["charWidth"];
			const charHeight = renderer["charHeight"];
			const dpr = renderer["dpr"] || 1;

			// Sample pixel at cursor row center
			const row = ts.cursorRow;
			const x = Math.floor(charWidth * dpr / 2);
			const y = Math.floor((row * charHeight + charHeight / 2) * dpr);
			const pixel = ctx.getImageData(x, y, 1, 1).data;

			return {
				row,
				sampleX: x,
				sampleY: y,
				r: pixel[0], g: pixel[1], b: pixel[2], a: pixel[3],
				hasContent: pixel[0] > 0 || pixel[1] > 0 || pixel[2] > 0,
			};
		});

		console.log("Pixel check:", JSON.stringify(pixelCheck));
		await browser.saveScreenshot("./screenshots/render-debug-05-pixel.png");
	});
});
