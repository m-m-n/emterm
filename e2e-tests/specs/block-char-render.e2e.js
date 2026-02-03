/**
 * E2E test: Block character rendering verification
 *
 * Tests that block drawing characters (U+2580-U+259F) render without gaps.
 * This is to verify the Claude Code logo displays correctly.
 */

async function typeCommand(cmd) {
	for (const char of cmd) {
		await browser.keys([char]);
		await browser.pause(10);
	}
}

describe("Block Character Rendering", () => {
	beforeEach(async () => {
		// Wait for terminal to be ready
		await browser.pause(3000);
	});

	it("should render Claude Code logo block characters", async () => {
		// Type the Claude Code logo echo commands
		await typeCommand('echo " ▐▛███▜▌   Claude Code v2.1.29"');
		await browser.keys(["Enter"]);
		await browser.pause(500);

		await typeCommand('echo "▝▜█████▛▘  Opus 4.5 · Claude Max"');
		await browser.keys(["Enter"]);
		await browser.pause(500);

		await typeCommand('echo "  ▘▘ ▝▝    ~/AI/claude"');
		await browser.keys(["Enter"]);
		await browser.pause(1000);

		// Take screenshot for visual verification
		await browser.saveScreenshot(
			"./screenshots/block-char-render-01-logo.png"
		);

		console.log("Screenshot saved: block-char-render-01-logo.png");
		console.log("Please visually verify that block characters have no gaps.");
	});

	it("should render full block characters seamlessly", async () => {
		// Test with a line of full blocks
		await typeCommand('echo "████████████████████████████████"');
		await browser.keys(["Enter"]);
		await browser.pause(1000);

		// Take screenshot
		await browser.saveScreenshot(
			"./screenshots/block-char-render-02-fullblocks.png"
		);

		console.log("Screenshot saved: block-char-render-02-fullblocks.png");
		console.log("Full block line should appear as a solid bar with no gaps.");
	});
});
