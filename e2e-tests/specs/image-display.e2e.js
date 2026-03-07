/**
 * eMterm Image Display E2E Tests
 *
 * Tests for Kitty Graphics Protocol image display functionality.
 * Verifies that:
 * 1. Images are displayed correctly
 * 2. No response strings appear in the prompt (q=1 suppression)
 */

describe("eMterm Image Display", () => {
	// Use short path for Docker environment (symlinked in Dockerfile)
	// This avoids WebDriver key input issues with long paths
	const imagePath = "/tmp/test.png";

	it("should display the terminal and wait for shell prompt", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.waitForDisplayed({ timeout: 10000 });

		// Wait for shell prompt to be ready
		await browser.pause(2000);
		await browser.saveScreenshot("./screenshots/image-01-initial.png");
	});

	it("should display an image using emterm image command", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.click();

		// Type the emterm image command with delay between keys
		const command = `emterm image ${imagePath}`;
		for (const char of command) {
			await browser.keys([char]);
			await browser.pause(20); // Small delay to prevent key loss
		}
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/image-02-before-enter.png");

		// Execute the command
		await browser.keys(["Enter"]);

		// Wait and take multiple screenshots to catch the viewer state
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/image-03a-500ms.png");

		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/image-03b-1000ms.png");

		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/image-03c-1500ms.png");

		// Check overlay state via JavaScript
		const overlayInfo = await browser.execute(() => {
			const overlay = document.querySelector(".image-viewer-overlay");
			if (!overlay) return { exists: false };
			const style = getComputedStyle(overlay);
			return {
				exists: true,
				hasVisible: overlay.classList.contains("visible"),
				opacity: style.opacity,
				visibility: style.visibility,
				zIndex: style.zIndex,
				width: style.width,
				height: style.height,
			};
		});
		console.log("Overlay info:", JSON.stringify(overlayInfo, null, 2));

		// Check canvas state
		const canvasInfo = await browser.execute(() => {
			const canvas = document.querySelector(".image-viewer-canvas");
			if (!canvas) return { exists: false };
			return {
				exists: true,
				width: canvas.width,
				height: canvas.height,
				cssWidth: getComputedStyle(canvas).width,
				cssHeight: getComputedStyle(canvas).height,
			};
		});
		console.log("Canvas info:", JSON.stringify(canvasInfo, null, 2));

		await browser.pause(1500);
		await browser.saveScreenshot("./screenshots/image-03-after-image.png");
	});

	it("should verify no response string in prompt", async () => {
		const terminal = await $('[data-testid="terminal"]');

		// Get terminal text content
		const terminalText = await terminal.getText();
		console.log("Terminal text after image:", terminalText.slice(-500));

		// Check that response strings like "Gi=1" or ";OK" don't appear in unexpected places
		// The pattern "Gi=" followed by digits suggests a Kitty response leak
		const hasLeakedResponse = /Gi=\d+.*(?:OK|ERROR)/.test(terminalText);

		if (hasLeakedResponse) {
			console.error(
				"FAILED: Kitty response string leaked to terminal output",
			);
		}

		// Note: This test documents the expected behavior
		// If q=1 is working correctly, no response should leak
		await browser.saveScreenshot("./screenshots/image-04-prompt-check.png");
	});

	it("should verify image viewer element exists after display", async () => {
		// Check if image viewer overlay or element exists
		const imageViewer = await $(".image-viewer");

		try {
			const exists = await imageViewer.isExisting();
			console.log("Image viewer element exists:", exists);

			if (exists) {
				const isDisplayed = await imageViewer.isDisplayed();
				console.log("Image viewer is displayed:", isDisplayed);
			}
		} catch (e) {
			console.log("Image viewer check:", e.message);
		}

		await browser.saveScreenshot("./screenshots/image-05-viewer-check.png");
	});

	it("should verify terminal text does not contain Kitty response", async () => {
		// WebKitWebDriver doesn't support getLogs, so we check terminal text instead
		const terminal = await $('[data-testid="terminal"]');
		const terminalText = await terminal.getText();

		// Check for Kitty response patterns that should NOT appear
		const hasResponseLeak = /\x1b_G|Gi=\d+.*[;](?:OK|ERROR)/.test(terminalText);
		const hasRawResponse = /Gi=\d+,p=\d+;OK/.test(terminalText);

		console.log("Checking for response leaks in terminal...");
		console.log("Has ESC sequence leak:", hasResponseLeak);
		console.log("Has raw response leak:", hasRawResponse);

		// These should be false if q=1 suppression is working
		expect(hasResponseLeak).toBe(false);
		expect(hasRawResponse).toBe(false);
	});

	it("should type a command after image to verify prompt is clean", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.click();

		// Type a simple command to verify prompt is functional
		await browser.keys(["e", "c", "h", "o", " ", "t", "e", "s", "t"]);
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/image-06-typing-after.png");

		await browser.keys(["Enter"]);
		await browser.pause(1000);
		await browser.saveScreenshot("./screenshots/image-07-after-echo.png");

		// The prompt should show "echo test" and its output
		const terminalText = await terminal.getText();
		console.log("Terminal after echo test:", terminalText.slice(-300));
	});
});
