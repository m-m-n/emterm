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
			return {
				lineElementCount: tr.lineElements?.length || 0,
				lastRowHashSize: tr.lastRowHash?.size || 0,
				useOptimizedRendering: tr.useOptimizedRendering,
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

	async function getDOMContent(row) {
		return await browser.execute((r) => {
			const terminal = document.getElementById("terminal");
			const lines = terminal?.querySelectorAll(".terminal-line");
			if (!lines || r >= lines.length) return null;
			return lines[r].textContent;
		}, row);
	}

	it("should verify initial render works", async () => {
		const terminal = await $("#terminal");
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
			const domContent = await getDOMContent(i);
			console.log(`Row ${i} - Buffer: "${bufContent}" | DOM: "${domContent}"`);
		}

		await browser.saveScreenshot("./screenshots/render-debug-01-initial.png");
	});

	it("should track render after single keystroke", async () => {
		const terminal = await $("#terminal");
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

		// Get DOM content before
		const cursorRow = beforeState?.cursorRow || 0;
		const domBefore = await getDOMContent(cursorRow);
		console.log("DOM content before:", domBefore);

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
		const domAfter = await getDOMContent(cursorRow);
		console.log("Buffer content:", bufAfter);
		console.log("DOM content:", domAfter);
		console.log("Match:", bufAfter === domAfter);

		await browser.saveScreenshot(
			"./screenshots/render-debug-02-after-keystroke.png",
		);
	});

	it("should verify render with optimized mode disabled", async () => {
		const terminal = await $("#terminal");
		await terminal.click();
		await browser.pause(500);

		// Disable optimized rendering
		await browser.execute(() => {
			if (window.terminalRenderer) {
				window.terminalRenderer.setOptimizedRendering(false);
			}
		});

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
		console.log('Pressing "x" with optimized mode disabled...');
		await browser.keys("x");
		await browser.pause(500);

		// Get logs
		const logs = await captureRenderLogs();
		console.log("=== Render logs (non-optimized mode) ===");
		for (const log of logs) {
			console.log(log);
		}

		// Compare buffer vs DOM
		const bufAfter = await getBufferContent(cursorRow);
		const domAfter = await getDOMContent(cursorRow);
		console.log("Buffer content:", bufAfter);
		console.log("DOM content:", domAfter);
		console.log("Match:", bufAfter === domAfter);

		await browser.saveScreenshot(
			"./screenshots/render-debug-03-non-optimized.png",
		);

		// Re-enable optimized rendering
		await browser.execute(() => {
			if (window.terminalRenderer) {
				window.terminalRenderer.setOptimizedRendering(true);
			}
		});
	});

	it("should test hash cache invalidation", async () => {
		const terminal = await $("#terminal");
		await terminal.click();
		await browser.pause(500);

		// Get current state
		const state = await getTerminalState();
		const cursorRow = state?.cursorRow || 0;

		// Clear hash cache manually
		await browser.execute(() => {
			if (window.terminalRenderer) {
				window.terminalRenderer.lastRowHash?.clear();
			}
		});

		// Clear logs
		await browser.execute(() => {
			window.consoleLogs = [];
		});

		// Type a character
		console.log('Pressing "z" after cache clear...');
		await browser.keys("z");
		await browser.pause(500);

		// Get logs
		const logs = await captureRenderLogs();
		console.log("=== Render logs (after cache clear) ===");
		for (const log of logs) {
			console.log(log);
		}

		// Compare
		const bufAfter = await getBufferContent(cursorRow);
		const domAfter = await getDOMContent(cursorRow);
		console.log("Buffer content:", bufAfter);
		console.log("DOM content:", domAfter);
		console.log("Match:", bufAfter === domAfter);

		await browser.saveScreenshot(
			"./screenshots/render-debug-04-cache-cleared.png",
		);
	});

	it("should trace full render cycle", async () => {
		const terminal = await $("#terminal");
		await terminal.click();
		await browser.pause(500);

		// Inject detailed render logging
		await browser.execute(() => {
			const renderer = window.terminalRenderer;
			if (!renderer) return;

			// Patch render method
			const originalRender = renderer.render.bind(renderer);
			renderer.render = function () {
				console.log("[TRACE] render() called");
				console.log("[TRACE] pendingState:", !!this.pendingState);
				console.log("[TRACE] lineElements:", this.lineElements?.length);
				const result = originalRender();
				console.log("[TRACE] render() completed");
				return result;
			};

			// Patch renderLineOptimized
			const originalRenderLine = renderer.renderLineOptimized.bind(renderer);
			renderer.renderLineOptimized = function (rowIndex, line) {
				const contentHash = this.computeLineHash(line);
				const lastHash = this.lastRowHash.get(rowIndex);
				console.log(
					`[TRACE] renderLineOptimized row=${rowIndex} hash=${contentHash.slice(0, 50)}... lastHash=${lastHash?.slice(0, 50)}... same=${lastHash === contentHash}`,
				);
				return originalRenderLine(rowIndex, line);
			};
		});

		// Clear logs
		await browser.execute(() => {
			window.consoleLogs = [];
		});

		// Type
		console.log('Pressing "t" with trace logging...');
		await browser.keys("t");
		await browser.pause(500);

		// Get all trace logs
		const logs = await browser.execute(() => {
			return (
				window.consoleLogs?.filter(
					(log) =>
						log.includes("[TRACE]") ||
						log.includes("[render]") ||
						log.includes("[scheduleRender]"),
				) || []
			);
		});

		console.log("=== Trace logs ===");
		for (const log of logs) {
			console.log(log);
		}

		await browser.saveScreenshot("./screenshots/render-debug-05-trace.png");
	});
});
