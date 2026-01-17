/**
 * eMterm Image Viewer Zoom E2E Tests
 *
 * Tests zoom functionality to verify:
 * 1. Initial display fits the image to viewport
 * 2. Zoom operations change canvas dimensions
 * 3. Can zoom beyond fit level up to 400%
 * 4. Canvas size reflects zoom percentage based on original image size
 */

describe("eMterm Image Viewer Zoom", () => {
	// Use short path for Docker environment (symlinked in Dockerfile)
	const imagePath = "/tmp/test.png";

	it("should display the terminal and wait for shell prompt", async () => {
		const terminal = await $("#terminal");
		await terminal.waitForDisplayed({ timeout: 10000 });

		// Wait for shell prompt to be ready
		await browser.pause(2000);
		await browser.saveScreenshot("./screenshots/zoom-01-initial.png");
	});

	it("should open image viewer and verify initial fit level", async () => {
		const terminal = await $("#terminal");
		await terminal.click();

		// Type the emterm image command
		const command = `emterm image ${imagePath}`;
		for (const char of command) {
			await browser.keys([char]);
			await browser.pause(20);
		}
		await browser.keys(["Enter"]);

		// Wait for image viewer to open
		await browser.pause(2000);
		await browser.saveScreenshot("./screenshots/zoom-02-viewer-open.png");

		// Verify image viewer is visible
		const overlayInfo = await browser.execute(() => {
			const overlay = document.querySelector(".image-viewer-overlay");
			return overlay
				? {
						exists: true,
						visible: overlay.classList.contains("visible"),
					}
				: { exists: false };
		});
		console.log("Overlay info:", JSON.stringify(overlayInfo));
		expect(overlayInfo.visible).toBe(true);

		// Get initial canvas state and zoom info
		const initialState = await browser.execute(() => {
			const canvas = document.querySelector(".image-viewer-canvas");
			const zoomLevel = document.querySelector(".viewer-zoom-level");
			const infoDisplay = document.querySelector(".image-viewer-info");
			// Parse transform: translate(0px, 0px) scale(scaleX, scaleY)
			const transform = canvas?.style.transform || "";
			// Match both scale(x, y) and scale(x) formats
			const scaleMatch = transform.match(/scale\(([\d.]+)(?:,\s*([\d.]+))?\)/);
			const scaleX = scaleMatch ? parseFloat(scaleMatch[1]) : 1;
			const scaleY = scaleMatch && scaleMatch[2] ? parseFloat(scaleMatch[2]) : scaleX;
			return {
				transform: transform,
				scaleX: scaleX,
				scaleY: scaleY,
				canvasWidth: canvas?.width || 0, // Internal canvas resolution
				canvasHeight: canvas?.height || 0,
				zoomLevel: zoomLevel?.textContent || "not found",
				infoText: infoDisplay?.textContent || "not found",
			};
		});
		console.log("Initial state:", JSON.stringify(initialState, null, 2));

		// Initial zoom should be fit level (likely 100% for 128x128 test image)
		expect(initialState.zoomLevel).toMatch(/\d+%/);
	});

	it("should zoom in with + key and increase scale", async () => {
		// Get scale before zoom
		const beforeZoom = await browser.execute(() => {
			const canvas = document.querySelector(".image-viewer-canvas");
			const zoomLevel = document.querySelector(".viewer-zoom-level");
			const transform = canvas?.style.transform || "";
			// Match both scale(x, y) and scale(x) formats
			const scaleMatch = transform.match(/scale\(([\d.]+)(?:,\s*([\d.]+))?\)/);
			const scaleX = scaleMatch ? parseFloat(scaleMatch[1]) : 1;
			return {
				transform: transform,
				scaleX: scaleX,
				zoomLevel: zoomLevel?.textContent || "",
				zoomValue: parseInt(zoomLevel?.textContent) || 0,
			};
		});
		console.log("Before zoom in:", JSON.stringify(beforeZoom));

		// Press + key multiple times to zoom in
		for (let i = 0; i < 5; i++) {
			await browser.keys(["+"]);
			await browser.pause(100);
		}
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/zoom-03-zoomed-in.png");

		// Get scale after zoom
		const afterZoom = await browser.execute(() => {
			const canvas = document.querySelector(".image-viewer-canvas");
			const zoomLevel = document.querySelector(".viewer-zoom-level");
			const transform = canvas?.style.transform || "";
			// Match both scale(x, y) and scale(x) formats
			const scaleMatch = transform.match(/scale\(([\d.]+)(?:,\s*([\d.]+))?\)/);
			const scaleX = scaleMatch ? parseFloat(scaleMatch[1]) : 1;
			return {
				transform: transform,
				scaleX: scaleX,
				zoomLevel: zoomLevel?.textContent || "",
				zoomValue: parseInt(zoomLevel?.textContent) || 0,
			};
		});
		console.log("After zoom in:", JSON.stringify(afterZoom));

		// Verify zoom level increased (each + press adds 10%)
		expect(afterZoom.zoomValue).toBeGreaterThan(beforeZoom.zoomValue);

		// Verify scale increased
		expect(afterZoom.scaleX).toBeGreaterThan(beforeZoom.scaleX);
	});

	it("should zoom to maximum 400% with repeated + key presses", async () => {
		// Press + key many times to reach maximum
		for (let i = 0; i < 50; i++) {
			await browser.keys(["+"]);
			await browser.pause(50);
		}
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/zoom-04-max-zoom.png");

		// Get max zoom state
		const maxState = await browser.execute(() => {
			const canvas = document.querySelector(".image-viewer-canvas");
			const zoomLevel = document.querySelector(".viewer-zoom-level");
			const transform = canvas?.style.transform || "";
			// Match both scale(x, y) and scale(x) formats
			const scaleMatch = transform.match(/scale\(([\d.]+)(?:,\s*([\d.]+))?\)/);
			const scaleX = scaleMatch ? parseFloat(scaleMatch[1]) : 1;
			return {
				transform: transform,
				scaleX: scaleX,
				zoomLevel: zoomLevel?.textContent || "",
				zoomValue: parseInt(zoomLevel?.textContent) || 0,
				// Get bounding rect to verify visual size
				boundingRect: canvas?.getBoundingClientRect() || null,
				canvasWidth: canvas?.width || 0,
				canvasHeight: canvas?.height || 0,
			};
		});
		console.log("Max zoom state:", JSON.stringify(maxState));

		// Verify we can reach 400% (max zoom)
		expect(maxState.zoomValue).toBe(400);

		// Verify visual size is 4x the original (128 * 4 = 512)
		// The scale includes correction factor, so check visual dimensions instead
		if (maxState.boundingRect) {
			// At 400% zoom, 128x128 image should be ~512x512 visually
			expect(maxState.boundingRect.width).toBeGreaterThanOrEqual(500);
			expect(maxState.boundingRect.height).toBeGreaterThanOrEqual(500);
		}
	});

	it("should zoom out with - key and decrease scale", async () => {
		// Press 0 to reset to fit level first
		await browser.keys(["0"]);
		await browser.pause(200);

		// Get scale at fit level
		const atFitLevel = await browser.execute(() => {
			const canvas = document.querySelector(".image-viewer-canvas");
			const zoomLevel = document.querySelector(".viewer-zoom-level");
			const transform = canvas?.style.transform || "";
			// Match both scale(x, y) and scale(x) formats
			const scaleMatch = transform.match(/scale\(([\d.]+)(?:,\s*([\d.]+))?\)/);
			const scaleX = scaleMatch ? parseFloat(scaleMatch[1]) : 1;
			return {
				scaleX: scaleX,
				zoomValue: parseInt(zoomLevel?.textContent) || 0,
			};
		});
		console.log("At fit level:", JSON.stringify(atFitLevel));

		// Press - key multiple times to zoom out
		for (let i = 0; i < 5; i++) {
			await browser.keys(["-"]);
			await browser.pause(100);
		}
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/zoom-05-zoomed-out.png");

		// Get scale after zoom out
		const afterZoomOut = await browser.execute(() => {
			const canvas = document.querySelector(".image-viewer-canvas");
			const zoomLevel = document.querySelector(".viewer-zoom-level");
			const transform = canvas?.style.transform || "";
			// Match both scale(x, y) and scale(x) formats
			const scaleMatch = transform.match(/scale\(([\d.]+)(?:,\s*([\d.]+))?\)/);
			const scaleX = scaleMatch ? parseFloat(scaleMatch[1]) : 1;
			return {
				scaleX: scaleX,
				zoomValue: parseInt(zoomLevel?.textContent) || 0,
			};
		});
		console.log("After zoom out:", JSON.stringify(afterZoomOut));

		// Verify zoom level decreased
		expect(afterZoomOut.zoomValue).toBeLessThan(atFitLevel.zoomValue);
		// Verify scale decreased
		expect(afterZoomOut.scaleX).toBeLessThan(atFitLevel.scaleX);
	});

	it("should zoom to minimum 25% with repeated - key presses", async () => {
		// Press - key many times to reach minimum
		for (let i = 0; i < 50; i++) {
			await browser.keys(["-"]);
			await browser.pause(50);
		}
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/zoom-06-min-zoom.png");

		// Get min zoom state
		const minState = await browser.execute(() => {
			const canvas = document.querySelector(".image-viewer-canvas");
			const zoomLevel = document.querySelector(".viewer-zoom-level");
			const transform = canvas?.style.transform || "";
			// Match both scale(x, y) and scale(x) formats
			const scaleMatch = transform.match(/scale\(([\d.]+)(?:,\s*([\d.]+))?\)/);
			const scaleX = scaleMatch ? parseFloat(scaleMatch[1]) : 1;
			return {
				transform: transform,
				scaleX: scaleX,
				zoomLevel: zoomLevel?.textContent || "",
				zoomValue: parseInt(zoomLevel?.textContent) || 0,
				// Get bounding rect to verify visual size
				boundingRect: canvas?.getBoundingClientRect() || null,
				canvasWidth: canvas?.width || 0,
			};
		});
		console.log("Min zoom state:", JSON.stringify(minState));

		// Verify we reached minimum 25%
		expect(minState.zoomValue).toBe(25);

		// Verify visual size is 0.25x the original (128 * 0.25 = 32)
		// The scale includes correction factor, so check visual dimensions instead
		if (minState.boundingRect) {
			// At 25% zoom, 128x128 image should be ~32x32 visually
			expect(minState.boundingRect.width).toBeLessThanOrEqual(50);
			expect(minState.boundingRect.height).toBeLessThanOrEqual(50);
		}
	});

	it("should reset zoom with 0 key", async () => {
		// First zoom in
		for (let i = 0; i < 10; i++) {
			await browser.keys(["+"]);
			await browser.pause(50);
		}
		await browser.pause(200);

		const beforeReset = await browser.execute(() => {
			const zoomLevel = document.querySelector(".viewer-zoom-level");
			return parseInt(zoomLevel?.textContent) || 0;
		});
		console.log("Before reset:", beforeReset);

		// Press 0 to reset
		await browser.keys(["0"]);
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/zoom-07-reset.png");

		const afterReset = await browser.execute(() => {
			const zoomLevel = document.querySelector(".viewer-zoom-level");
			return parseInt(zoomLevel?.textContent) || 0;
		});
		console.log("After reset:", afterReset);

		// Should reset to fit level (100% for 128x128 in a large viewport)
		expect(afterReset).toBe(100);
	});

	it("should close viewer with Escape", async () => {
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
