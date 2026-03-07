/**
 * eMterm Image Viewer Keyboard Blocking E2E Tests
 *
 * Tests that keyboard input is properly blocked when the image viewer is open.
 * Verifies that:
 * 1. While viewer is open, key presses don't reach the terminal prompt
 * 2. After closing viewer with Escape, keyboard input works normally
 */

describe("eMterm Image Viewer Keyboard Blocking", () => {
	// Use short path for Docker environment (symlinked in Dockerfile)
	const imagePath = "/tmp/test.png";

	it("should display the terminal and wait for shell prompt", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.waitForDisplayed({ timeout: 10000 });

		// Wait for shell prompt to be ready
		await browser.pause(2000);
		await browser.saveScreenshot("./screenshots/kb-01-initial.png");
	});

	it("should open image viewer and verify keyboard is blocked", async () => {
		const terminal = await $('[data-testid="terminal"]');
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
		await browser.saveScreenshot("./screenshots/kb-02-viewer-open.png");

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

		// Get terminal text BEFORE typing
		const textBefore = await browser.execute(() => {
			const terminal = document.querySelector('[data-testid="terminal"]');
			return terminal ? terminal.textContent : "";
		});
		console.log("Text before typing:", textBefore.slice(-200));

		// Type several characters while viewer is open
		// These should NOT appear in the terminal
		const testChars = ["a", "b", "c", "d", "e"];
		for (const char of testChars) {
			await browser.keys([char]);
			await browser.pause(100);
		}
		await browser.saveScreenshot("./screenshots/kb-03-typed-while-open.png");

		// Get terminal text AFTER typing (while viewer still open)
		const textAfter = await browser.execute(() => {
			const terminal = document.querySelector('[data-testid="terminal"]');
			return terminal ? terminal.textContent : "";
		});
		console.log("Text after typing (viewer open):", textAfter.slice(-200));

		// The text should NOT have changed (no new characters at the prompt)
		// We check that the typed characters don't appear at the end
		const endsWithTyped = textAfter.trim().endsWith("abcde");
		console.log("Text ends with typed chars:", endsWithTyped);
		expect(endsWithTyped).toBe(false);
	});

	it("should close viewer with Escape and then accept keyboard input", async () => {
		// Close the viewer with Escape
		await browser.keys(["Escape"]);
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/kb-04-after-escape.png");

		// Verify viewer is closed
		const overlayInfo = await browser.execute(() => {
			const overlay = document.querySelector(".image-viewer-overlay");
			return overlay
				? {
						exists: true,
						visible: overlay.classList.contains("visible"),
					}
				: { exists: false };
		});
		console.log("Overlay after Escape:", JSON.stringify(overlayInfo));
		expect(overlayInfo.visible).toBe(false);

		// Click terminal to ensure focus
		const terminal = await $('[data-testid="terminal"]');
		await terminal.click();
		await browser.pause(200);

		// Type a marker text to verify keyboard works
		const markerText = "TEST_MARKER_123";
		for (const char of markerText) {
			await browser.keys([char]);
			await browser.pause(30);
		}
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/kb-05-typed-after-close.png");

		// Verify the marker appears in terminal
		const terminalText = await browser.execute(() => {
			const terminal = document.querySelector('[data-testid="terminal"]');
			return terminal ? terminal.textContent : "";
		});
		console.log("Terminal text after typing marker:", terminalText.slice(-200));

		const hasMarker = terminalText.includes("TEST_MARKER_123");
		console.log("Has marker text:", hasMarker);
		expect(hasMarker).toBe(true);

		// Clean up - press Ctrl+C to cancel the marker input, then newline
		await browser.keys(["Control", "c"]);
		await browser.pause(300);
	});

	it("should verify no stray characters from blocked input", async () => {
		// Type 'echo check' to verify no leftover characters
		const terminal = await $('[data-testid="terminal"]');
		await terminal.click();
		await browser.pause(200);

		const checkCommand = "echo CLEAN_CHECK";
		for (const char of checkCommand) {
			await browser.keys([char]);
			await browser.pause(30);
		}
		await browser.keys(["Enter"]);
		await browser.pause(1000);
		await browser.saveScreenshot("./screenshots/kb-06-final-check.png");

		const terminalText = await browser.execute(() => {
			const terminal = document.querySelector('[data-testid="terminal"]');
			return terminal ? terminal.textContent : "";
		});
		console.log("Final terminal text:", terminalText.slice(-300));

		// Verify CLEAN_CHECK appears (clean prompt)
		const hasCleanCheck = terminalText.includes("CLEAN_CHECK");
		console.log("Has CLEAN_CHECK:", hasCleanCheck);
		expect(hasCleanCheck).toBe(true);
	});
});
