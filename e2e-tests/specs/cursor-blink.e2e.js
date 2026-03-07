/**
 * Cursor Blink E2E Test
 *
 * Tests that cursor blinks correctly in Canvas mode.
 */

describe("Cursor Blink Test", () => {
	it("should blink cursor in canvas mode", async () => {
		// Wait for terminal to initialize
		await browser.pause(2000);

		// Check if terminal exists
		const terminal = await $('[data-testid="terminal"]');
		const exists = await terminal.isExisting();
		console.log("Terminal element exists:", exists);

		// Get renderer type
		const rendererType = await browser.execute(() => {
			const tr = window.terminalRenderer;
			if (!tr) return "unknown";
			// Check if it's a canvas renderer by looking for canvas element
			if (tr.canvas) return "canvas";
			return "dom";
		});
		console.log("Renderer type:", rendererType);

		// Get cursor state
		const cursorState = await browser.execute(() => {
			const ts = window.terminalState;
			const tr = window.terminalRenderer;
			if (!ts || !tr) return null;
			return {
				cursorCol: ts.cursorCol,
				cursorRow: ts.cursorRow,
				cursorVisible: ts.cursorVisible,
				cursorBlink: ts.cursorBlink,
				cursorStyle: ts.cursorStyle,
				rendererCursorBlinkVisible: tr.cursorBlinkVisible,
				cursorBlinkTimer: tr.cursorBlinkTimer !== null,
			};
		});
		console.log("Cursor state:", JSON.stringify(cursorState));

		// Take initial screenshot
		await browser.saveScreenshot("./screenshots/cursor-blink-01-initial.png");

		// Get cursor pixel color at initial state
		const getPixelAtCursor = async () => {
			return await browser.execute(() => {
				const ts = window.terminalState;
				const tr = window.terminalRenderer;
				if (!ts || !tr || !tr.canvas) return null;

				const ctx = tr.canvas.getContext("2d");
				const dpr = window.devicePixelRatio || 1;
				const x = Math.floor((ts.cursorCol * tr.charWidth + tr.charWidth / 2) * dpr);
				const y = Math.floor((ts.cursorRow * tr.charHeight + tr.charHeight / 2) * dpr);

				const imageData = ctx.getImageData(x, y, 1, 1);
				const [r, g, b, a] = imageData.data;
				return { r, g, b, a, x, y, cursorBlinkVisible: tr.cursorBlinkVisible };
			});
		};

		// Capture cursor pixel over several blink cycles
		const samples = [];
		for (let i = 0; i < 10; i++) {
			const pixel = await getPixelAtCursor();
			samples.push(pixel);
			console.log(`Sample ${i}: RGB(${pixel?.r}, ${pixel?.g}, ${pixel?.b}) blinkVisible=${pixel?.cursorBlinkVisible}`);
			await browser.pause(200);
		}

		// Take screenshot after sampling
		await browser.saveScreenshot("./screenshots/cursor-blink-02-after-samples.png");

		// Check if cursorBlinkVisible changed (should toggle between true/false)
		const blinkStates = samples.map((s) => s?.cursorBlinkVisible);
		const uniqueStates = [...new Set(blinkStates)];
		console.log("Unique blink states:", uniqueStates);

		// Check if pixel color changed (indicates actual blink on canvas)
		const colorSignatures = samples.map((s) => s ? `${s.r}-${s.g}-${s.b}` : null);
		const uniqueColors = [...new Set(colorSignatures)];
		console.log("Unique colors at cursor:", uniqueColors);

		// Assertions
		expect(cursorState).not.toBeNull();
		expect(cursorState.cursorBlinkTimer).toBe(true); // Timer should be running

		// cursorBlinkVisible should toggle between true and false
		expect(uniqueStates.length).toBe(2);
		expect(uniqueStates).toContain(true);
		expect(uniqueStates).toContain(false);

		// If canvas mode, colors should change (cursor drawn/not drawn)
		if (rendererType === "canvas") {
			console.log("Canvas mode: checking if pixel colors changed");
			expect(uniqueColors.length).toBeGreaterThan(1);
		}
	});

	it("should not leave cursor residue when pressing Enter", async () => {
		// Wait for terminal to initialize
		await browser.pause(1000);

		const terminal = await $('[data-testid="terminal"]');
		await terminal.click();
		await browser.pause(200);

		// Take screenshot before Enter presses
		await browser.saveScreenshot("./screenshots/cursor-blink-03-before-enter.png");

		// Get initial cursor position
		const initialPos = await browser.execute(() => {
			const ts = window.terminalState;
			return ts ? { col: ts.cursorCol, row: ts.cursorRow } : null;
		});
		console.log("Initial cursor position:", JSON.stringify(initialPos));

		// Press Enter multiple times
		for (let i = 0; i < 5; i++) {
			await browser.keys("Enter");
			await browser.pause(300);
		}

		// Wait for render to settle
		await browser.pause(500);

		// Take screenshot after Enter presses
		await browser.saveScreenshot("./screenshots/cursor-blink-04-after-enter.png");

		// Get final cursor position
		const finalPos = await browser.execute(() => {
			const ts = window.terminalState;
			return ts ? { col: ts.cursorCol, row: ts.cursorRow } : null;
		});
		console.log("Final cursor position:", JSON.stringify(finalPos));

		// Check pixel at initial cursor position (should be background color now)
		const pixelAtOldPos = await browser.execute((initialRow) => {
			const ts = window.terminalState;
			const tr = window.terminalRenderer;
			if (!ts || !tr || !tr.canvas) return null;

			const ctx = tr.canvas.getContext("2d");
			const dpr = window.devicePixelRatio || 1;
			const x = Math.floor((0 * tr.charWidth + tr.charWidth / 2) * dpr);
			const y = Math.floor((initialRow * tr.charHeight + tr.charHeight / 2) * dpr);

			const imageData = ctx.getImageData(x, y, 1, 1);
			const [r, g, b, a] = imageData.data;
			return { r, g, b, a, x, y };
		}, initialPos?.row || 0);
		console.log("Pixel at old cursor position:", JSON.stringify(pixelAtOldPos));

		// Check pixel at current cursor position (should be cursor color)
		const pixelAtNewPos = await browser.execute(() => {
			const ts = window.terminalState;
			const tr = window.terminalRenderer;
			if (!ts || !tr || !tr.canvas) return null;

			const ctx = tr.canvas.getContext("2d");
			const dpr = window.devicePixelRatio || 1;
			const x = Math.floor((ts.cursorCol * tr.charWidth + tr.charWidth / 2) * dpr);
			const y = Math.floor((ts.cursorRow * tr.charHeight + tr.charHeight / 2) * dpr);

			const imageData = ctx.getImageData(x, y, 1, 1);
			const [r, g, b, a] = imageData.data;
			return { r, g, b, a, x, y, cursorRow: ts.cursorRow, cursorCol: ts.cursorCol };
		});
		console.log("Pixel at new cursor position:", JSON.stringify(pixelAtNewPos));

		// The pixel at old position should NOT be cursor color (green #00FF00 or #008000)
		// Background is typically black (#000000) or dark
		const isGreen = (pixel) => pixel && pixel.g > 100 && pixel.r < 50 && pixel.b < 50;

		if (pixelAtOldPos && initialPos?.row !== finalPos?.row) {
			// Old position should not be green (cursor color)
			console.log("Is old position green (cursor residue)?", isGreen(pixelAtOldPos));
			expect(isGreen(pixelAtOldPos)).toBe(false);
		}
	});
});
