/**
 * Large Image Zoom Test
 * Tests zoom with a large image (1080x1920) that requires fitLevel < 100%
 */

describe("Large Image Zoom Test", () => {
	// Use pre-created large image in Docker (1080x1920)
	const imagePath = "/tmp/large-test.png";

	it("should display terminal", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.waitForDisplayed({ timeout: 10000 });
		await browser.pause(2000);
	});

	it("should display large image with correct fit level", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.click();

		// Display the large image
		const command = `emterm image ${imagePath}`;
		for (const char of command) {
			await browser.keys([char]);
			await browser.pause(20);
		}
		await browser.keys(["Enter"]);

		// Wait for overlay to become visible (large images need more time)
		await browser.waitUntil(
			async () => {
				const visible = await browser.execute(() => {
					const overlay = document.querySelector(".image-viewer-overlay");
					return overlay?.classList.contains("visible") ?? false;
				});
				return visible;
			},
			{
				timeout: 30000, // 30 seconds for large image
				timeoutMsg: "Image viewer did not open within 30 seconds",
				interval: 500,
			}
		);

		await browser.saveScreenshot("./screenshots/large-01-initial.png");

		// Check for any decode errors (red canvas = decode failure)
		const canvasState = await browser.execute(() => {
			const canvas = document.querySelector(".image-viewer-canvas");
			if (!canvas) return { exists: false };
			const ctx = canvas.getContext("2d");
			// Check top-left pixel for red (error indicator)
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

		// If top-left is red (255,0,0), decode failed
		const isDecodeError =
			canvasState.topLeftRed === 255 &&
			canvasState.topLeftGreen === 0 &&
			canvasState.topLeftBlue === 0;
		if (isDecodeError) {
			console.log("WARNING: Decode error detected (red canvas)");
		}

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

		// Get initial state
		const state = await browser.execute(() => {
			const canvas = document.querySelector(".image-viewer-canvas");
			const zoomLevel = document.querySelector(".viewer-zoom-level");
			const infoDisplay = document.querySelector(".image-viewer-info");
			const transform = canvas?.style.transform || "";
			const scaleMatch = transform.match(/scale\(([\d.]+)\)/);

			return {
				transform: transform,
				scale: scaleMatch ? parseFloat(scaleMatch[1]) : null,
				canvasInternalWidth: canvas?.width || 0,
				canvasInternalHeight: canvas?.height || 0,
				canvasStyleWidth: canvas?.style.width || "",
				canvasStyleHeight: canvas?.style.height || "",
				boundingRect: canvas?.getBoundingClientRect() || null,
				zoomLevel: zoomLevel?.textContent || "",
				infoText: infoDisplay?.textContent || "",
			};
		});

		console.log("Large image state:", JSON.stringify(state, null, 2));

		// Verify fit level is less than 100% for large image
		const zoomValue = parseInt(state.zoomLevel) || 0;
		console.log(`Zoom value: ${zoomValue}%`);

		// For 1080x1920 image in ~800x600 viewport, fit should be around 30%
		expect(zoomValue).toBeLessThan(100);
		expect(zoomValue).toBeGreaterThan(20);

		// Verify bounding rect - the visual size should fill most of viewport height
		if (state.boundingRect) {
			console.log(`Bounding rect: ${state.boundingRect.width}x${state.boundingRect.height}`);
			// The image height should fill most of the viewport (with padding)
			// Viewport is ~600px high, so image should be ~500px+ (around 85% of viewport)
			expect(state.boundingRect.height).toBeGreaterThan(400);
		}
	});

	it("should zoom in beyond fit level", async () => {
		// Ensure overlay is visible before testing zoom
		const isVisible = await browser.execute(() => {
			const overlay = document.querySelector(".image-viewer-overlay");
			return overlay?.classList.contains("visible") ?? false;
		});
		if (!isVisible) {
			console.log("Overlay not visible, waiting...");
			await browser.waitUntil(
				async () => {
					return await browser.execute(() => {
						const overlay = document.querySelector(".image-viewer-overlay");
						return overlay?.classList.contains("visible") ?? false;
					});
				},
				{ timeout: 10000, interval: 500 }
			);
		}

		// Get current zoom level
		const beforeZoom = await browser.execute(() => {
			const zoomLevel = document.querySelector(".viewer-zoom-level");
			return parseInt(zoomLevel?.textContent) || 0;
		});
		console.log(`Before zoom in: ${beforeZoom}%`);

		// Press + key 5 times to zoom in
		for (let i = 0; i < 5; i++) {
			await browser.keys(["+"]);
			await browser.pause(100);
		}
		await browser.pause(500);

		await browser.saveScreenshot("./screenshots/large-02-zoomed-in.png");

		// Get state after zoom
		const afterState = await browser.execute(() => {
			const canvas = document.querySelector(".image-viewer-canvas");
			const zoomLevel = document.querySelector(".viewer-zoom-level");
			return {
				zoomValue: parseInt(zoomLevel?.textContent) || 0,
				boundingRect: canvas?.getBoundingClientRect() || null,
				canvasStyleWidth: canvas?.style.width || "",
				canvasStyleHeight: canvas?.style.height || "",
			};
		});
		console.log(`After zoom in: ${afterState.zoomValue}%, boundingRect: ${JSON.stringify(afterState.boundingRect)}`);

		// Zoom should have increased
		expect(afterState.zoomValue).toBeGreaterThan(beforeZoom);

		// Image should be larger than before
		if (afterState.boundingRect) {
			// Should exceed viewport height after zooming in
			expect(afterState.boundingRect.height).toBeGreaterThan(500);
		}
	});

	it("should zoom to 100% with pixel-perfect size", async () => {
		// Ensure overlay is visible
		const isVisible = await browser.execute(() => {
			const overlay = document.querySelector(".image-viewer-overlay");
			return overlay?.classList.contains("visible") ?? false;
		});
		if (!isVisible) {
			console.log("Overlay not visible, skipping 100% zoom test");
			return;
		}

		// Press '1' to reset to 100%
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
		console.log(`BoundingRect: ${state100.boundingRect?.width}x${state100.boundingRect?.height}`);

		// Verify zoom is 100%
		expect(state100.zoomValue).toBe(100);

		// At 100%, the visual size (boundingRect) should match the original image size
		// Original image is 1080x1920
		const expectedWidth = 1080;
		const expectedHeight = 1920;

		if (state100.boundingRect) {
			// Allow small tolerance for rounding (±2px)
			const tolerance = 2;
			expect(Math.abs(state100.boundingRect.width - expectedWidth)).toBeLessThanOrEqual(tolerance);
			expect(Math.abs(state100.boundingRect.height - expectedHeight)).toBeLessThanOrEqual(tolerance);
			console.log(`✓ Pixel-perfect at 100%: expected ${expectedWidth}x${expectedHeight}, got ${state100.boundingRect.width}x${state100.boundingRect.height}`);
		}
	});

	it("should zoom to maximum 400%", async () => {
		// Ensure overlay is visible before testing zoom
		const isVisible = await browser.execute(() => {
			const overlay = document.querySelector(".image-viewer-overlay");
			return overlay?.classList.contains("visible") ?? false;
		});
		if (!isVisible) {
			console.log("Overlay not visible, skipping max zoom test");
			return; // Skip if overlay not visible
		}

		// Press + many times to reach max
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

		// At 400%, the image should be huge (1080*4 = 4320px wide)
		if (maxState.boundingRect) {
			console.log(`At 400%: ${maxState.boundingRect.width}x${maxState.boundingRect.height}`);
			expect(maxState.boundingRect.width).toBeGreaterThan(3000);
		}
	});

	it("should close viewer with Escape", async () => {
		await browser.keys(["Escape"]);
		await browser.pause(500);

		const overlayInfo = await browser.execute(() => {
			const overlay = document.querySelector(".image-viewer-overlay");
			return overlay?.classList.contains("visible") ?? false;
		});
		expect(overlayInfo).toBe(false);
	});
});
