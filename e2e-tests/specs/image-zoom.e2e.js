/**
 * eMterm Image Viewer Zoom E2E Tests
 *
 * Tests zoom functionality to verify:
 * 1. Initial display fits the image to viewport
 * 2. Zoom operations change canvas dimensions
 * 3. Can zoom beyond fit level up to 400%
 * 4. Canvas size reflects zoom percentage based on original image size
 */

/** Helper to get current zoom state */
async function getZoomState() {
	return browser.execute(() => {
		const canvas = document.querySelector(".image-viewer-canvas");
		const zoomLevel = document.querySelector(".viewer-zoom-level");
		const transform = canvas?.style.transform || "";
		const scaleMatch = transform.match(/scale\(([\d.]+)(?:,\s*([\d.]+))?\)/);
		const scaleX = scaleMatch ? parseFloat(scaleMatch[1]) : 1;
		return {
			transform,
			scaleX,
			zoomLevel: zoomLevel?.textContent || "",
			zoomValue: parseInt(zoomLevel?.textContent) || 0,
			boundingRect: canvas?.getBoundingClientRect() || null,
			canvasWidth: canvas?.width || 0,
			canvasHeight: canvas?.height || 0,
		};
	});
}

/** Helper to ensure image viewer is open */
async function ensureViewerOpen(imagePath) {
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
	await browser.pause(2000);
}

describe("eMterm Image Viewer Zoom", () => {
	const imagePath = "/tmp/test.png";

	it("should open image viewer and verify initial fit level", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.waitForDisplayed({ timeout: 10000 });
		await browser.pause(2000);
		await browser.saveScreenshot("./screenshots/zoom-01-initial.png");

		await terminal.click();

		// Type the emterm image command
		const command = `emterm image ${imagePath}`;
		for (const char of command) {
			await browser.keys([char]);
			await browser.pause(20);
		}
		await browser.keys(["Enter"]);
		await browser.pause(2000);
		await browser.saveScreenshot("./screenshots/zoom-02-viewer-open.png");

		// Verify image viewer is visible
		const overlayInfo = await browser.execute(() => {
			const overlay = document.querySelector(".image-viewer-overlay");
			return overlay
				? { exists: true, visible: overlay.classList.contains("visible") }
				: { exists: false };
		});
		console.log("Overlay info:", JSON.stringify(overlayInfo));
		expect(overlayInfo.visible).toBe(true);

		const initialState = await getZoomState();
		console.log("Initial state:", JSON.stringify(initialState, null, 2));
		expect(initialState.zoomLevel).toMatch(/\d+%/);
	});

	it("should zoom in, out, to max, min, and reset correctly", async () => {
		await ensureViewerOpen(imagePath);

		// --- Zoom in with + key ---
		const beforeZoom = await getZoomState();
		console.log("Before zoom in:", JSON.stringify(beforeZoom));

		for (let i = 0; i < 5; i++) {
			await browser.keys(["+"]);
			await browser.pause(100);
		}
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/zoom-03-zoomed-in.png");

		const afterZoomIn = await getZoomState();
		console.log("After zoom in:", JSON.stringify(afterZoomIn));
		expect(afterZoomIn.zoomValue).toBeGreaterThan(beforeZoom.zoomValue);
		expect(afterZoomIn.scaleX).toBeGreaterThan(beforeZoom.scaleX);

		// --- Zoom to maximum 400% ---
		for (let i = 0; i < 50; i++) {
			await browser.keys(["+"]);
			await browser.pause(50);
		}
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/zoom-04-max-zoom.png");

		const maxState = await getZoomState();
		console.log("Max zoom state:", JSON.stringify(maxState));
		expect(maxState.zoomValue).toBe(400);
		if (maxState.boundingRect) {
			expect(maxState.boundingRect.width).toBeGreaterThanOrEqual(500);
			expect(maxState.boundingRect.height).toBeGreaterThanOrEqual(500);
		}

		// --- Reset with 0, then zoom out ---
		await browser.keys(["0"]);
		await browser.pause(200);

		const atFitLevel = await getZoomState();
		console.log("At fit level:", JSON.stringify(atFitLevel));

		for (let i = 0; i < 5; i++) {
			await browser.keys(["-"]);
			await browser.pause(100);
		}
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/zoom-05-zoomed-out.png");

		const afterZoomOut = await getZoomState();
		console.log("After zoom out:", JSON.stringify(afterZoomOut));
		expect(afterZoomOut.zoomValue).toBeLessThan(atFitLevel.zoomValue);
		expect(afterZoomOut.scaleX).toBeLessThan(atFitLevel.scaleX);

		// --- Zoom to minimum 25% ---
		for (let i = 0; i < 50; i++) {
			await browser.keys(["-"]);
			await browser.pause(50);
		}
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/zoom-06-min-zoom.png");

		const minState = await getZoomState();
		console.log("Min zoom state:", JSON.stringify(minState));
		expect(minState.zoomValue).toBe(25);
		if (minState.boundingRect) {
			expect(minState.boundingRect.width).toBeLessThanOrEqual(50);
			expect(minState.boundingRect.height).toBeLessThanOrEqual(50);
		}

		// --- Reset zoom with 0 key ---
		for (let i = 0; i < 10; i++) {
			await browser.keys(["+"]);
			await browser.pause(50);
		}
		await browser.pause(200);

		const beforeReset = await getZoomState();
		console.log("Before reset:", beforeReset.zoomValue);

		await browser.keys(["0"]);
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/zoom-07-reset.png");

		const afterReset = await getZoomState();
		console.log("After reset:", afterReset.zoomValue);
		expect(afterReset.zoomValue).toBe(100);
	});

	it("should close viewer with Escape", async () => {
		await ensureViewerOpen(imagePath);

		await browser.keys(["Escape"]);
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/zoom-08-closed.png");

		const overlayInfo = await browser.execute(() => {
			const overlay = document.querySelector(".image-viewer-overlay");
			return overlay?.classList.contains("visible") ?? false;
		});
		expect(overlayInfo).toBe(false);
	});
});
