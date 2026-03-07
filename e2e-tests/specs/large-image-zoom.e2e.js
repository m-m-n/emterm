/**
 * Large Image Zoom Test
 * Tests zoom with a large image (1080x1920) that requires fitLevel < 100%
 */

/** Helper to ensure image viewer is open */
async function ensureLargeViewerOpen(imagePath) {
	const isVisible = await browser.execute(() => {
		const overlay = document.querySelector(".image-viewer-overlay");
		return overlay?.classList.contains("visible") ?? false;
	});
	if (isVisible) return;

	const terminal = await $('[data-testid="terminal"]');
	await terminal.click();
	const command = `emterm image ${imagePath}`;
	for (const char of command) {
		await browser.keys([char]);
		await browser.pause(20);
	}
	await browser.keys(["Enter"]);
	await browser.waitUntil(
		async () => {
			return await browser.execute(() => {
				const overlay = document.querySelector(".image-viewer-overlay");
				return overlay?.classList.contains("visible") ?? false;
			});
		},
		{ timeout: 30000, timeoutMsg: "Image viewer did not open within 30 seconds", interval: 500 }
	);
}

describe("Large Image Zoom Test", () => {
	const imagePath = "/tmp/large-test.png";

	it("should display large image with correct fit level and test zoom operations", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.waitForDisplayed({ timeout: 10000 });
		await browser.pause(2000);

		await terminal.click();

		// Display the large image
		const command = `emterm image ${imagePath}`;
		for (const char of command) {
			await browser.keys([char]);
			await browser.pause(20);
		}
		await browser.keys(["Enter"]);

		// Wait for overlay to become visible
		await browser.waitUntil(
			async () => {
				const visible = await browser.execute(() => {
					const overlay = document.querySelector(".image-viewer-overlay");
					return overlay?.classList.contains("visible") ?? false;
				});
				return visible;
			},
			{ timeout: 30000, timeoutMsg: "Image viewer did not open within 30 seconds", interval: 500 }
		);

		await browser.saveScreenshot("./screenshots/large-01-initial.png");

		// Check for decode errors
		const canvasState = await browser.execute(() => {
			const canvas = document.querySelector(".image-viewer-canvas");
			if (!canvas) return { exists: false };
			const ctx = canvas.getContext("2d");
			const pixel = ctx.getImageData(0, 0, 1, 1).data;
			return {
				exists: true,
				width: canvas.width,
				height: canvas.height,
				topLeftRed: pixel[0],
				topLeftGreen: pixel[1],
				topLeftBlue: pixel[2],
			};
		});
		console.log("Canvas state:", JSON.stringify(canvasState));

		// Verify image viewer is visible
		const overlayInfo = await browser.execute(() => {
			const overlay = document.querySelector(".image-viewer-overlay");
			return overlay
				? { exists: true, visible: overlay.classList.contains("visible") }
				: { exists: false };
		});
		expect(overlayInfo.visible).toBe(true);

		// Get initial state
		const state = await browser.execute(() => {
			const canvas = document.querySelector(".image-viewer-canvas");
			const zoomLevel = document.querySelector(".viewer-zoom-level");
			return {
				zoomLevel: zoomLevel?.textContent || "",
				boundingRect: canvas?.getBoundingClientRect() || null,
			};
		});
		console.log("Large image state:", JSON.stringify(state, null, 2));

		// Verify fit level < 100% for large image
		const zoomValue = parseInt(state.zoomLevel) || 0;
		console.log(`Zoom value: ${zoomValue}%`);
		expect(zoomValue).toBeLessThan(100);
		expect(zoomValue).toBeGreaterThan(20);

		if (state.boundingRect) {
			console.log(`Bounding rect: ${state.boundingRect.width}x${state.boundingRect.height}`);
			expect(state.boundingRect.height).toBeGreaterThan(400);
		}

		// --- Zoom in beyond fit level ---
		const beforeZoom = zoomValue;
		for (let i = 0; i < 5; i++) {
			await browser.keys(["+"]);
			await browser.pause(100);
		}
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/large-02-zoomed-in.png");

		const afterZoomIn = await browser.execute(() => {
			const zoomLevel = document.querySelector(".viewer-zoom-level");
			const canvas = document.querySelector(".image-viewer-canvas");
			return {
				zoomValue: parseInt(zoomLevel?.textContent) || 0,
				boundingRect: canvas?.getBoundingClientRect() || null,
			};
		});
		console.log(`After zoom in: ${afterZoomIn.zoomValue}%`);
		expect(afterZoomIn.zoomValue).toBeGreaterThan(beforeZoom);

		// --- Zoom to 100% with '1' key ---
		await browser.keys(["1"]);
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/large-03-100-percent.png");

		const state100 = await browser.execute(() => {
			const canvas = document.querySelector(".image-viewer-canvas");
			const zoomLevel = document.querySelector(".viewer-zoom-level");
			return {
				zoomValue: parseInt(zoomLevel?.textContent) || 0,
				canvasWidth: canvas?.width || 0,
				canvasHeight: canvas?.height || 0,
				boundingRect: canvas?.getBoundingClientRect() || null,
			};
		});
		console.log(`At 100%: zoom=${state100.zoomValue}%, canvas=${state100.canvasWidth}x${state100.canvasHeight}`);
		expect(state100.zoomValue).toBe(100);

		if (state100.boundingRect) {
			const tolerance = 2;
			expect(Math.abs(state100.boundingRect.width - 1080)).toBeLessThanOrEqual(tolerance);
			expect(Math.abs(state100.boundingRect.height - 1920)).toBeLessThanOrEqual(tolerance);
		}

		// --- Zoom to maximum 400% ---
		for (let i = 0; i < 50; i++) {
			await browser.keys(["+"]);
			await browser.pause(50);
		}
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/large-04-max-zoom.png");

		const maxState = await browser.execute(() => {
			const canvas = document.querySelector(".image-viewer-canvas");
			const zoomLevel = document.querySelector(".viewer-zoom-level");
			return {
				zoomValue: parseInt(zoomLevel?.textContent) || 0,
				boundingRect: canvas?.getBoundingClientRect() || null,
			};
		});
		console.log(`Max zoom: ${maxState.zoomValue}%`);
		expect(maxState.zoomValue).toBe(400);
		if (maxState.boundingRect) {
			expect(maxState.boundingRect.width).toBeGreaterThan(3000);
		}
	});

	it("should close viewer with Escape", async () => {
		await ensureLargeViewerOpen(imagePath);

		await browser.keys(["Escape"]);
		await browser.pause(500);

		const overlayInfo = await browser.execute(() => {
			const overlay = document.querySelector(".image-viewer-overlay");
			return overlay?.classList.contains("visible") ?? false;
		});
		expect(overlayInfo).toBe(false);
	});
});
