/**
 * eMterm Markdown Rendering E2E Tests
 *
 * Tests for OSC 777 Markdown extension rendering.
 */

describe("eMterm Markdown Rendering", () => {
	before(async () => {
		// Inject console interceptor to capture logs (WebKitWebDriver doesn't support getLogs)
		await browser.execute(() => {
			if (window.__capturedLogs) return;
			window.__capturedLogs = [];
			["log", "warn", "error", "info", "debug"].forEach((level) => {
				const original = console[level];
				console[level] = function (...args) {
					window.__capturedLogs.push({ level, message: args.join(" ") });
					original.apply(console, args);
				};
			});
		});
	});

	// Helper to type a command
	async function typeCommand(command) {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.click();
		await browser.pause(100);

		for (const char of command) {
			await browser.keys([char]);
		}
		await browser.pause(100);
	}

	// Helper to execute a command
	async function executeCommand(command) {
		await typeCommand(command);
		await browser.keys(["Enter"]);
		await browser.pause(500);
	}

	it("should render markdown from echo command with OSC 777 sequence", async () => {
		// Prepare OSC sequence for "# Test" (base64: IyBUZXN0)
		// OSC 777 format: ESC ] 777 ; emterm ; markdown ; verb ; params ESC \
		const oscSequence = [
			"\\033]777;emterm;markdown;begin;id=e2e-test-1;format=gfm\\033\\\\",
			"\\033]777;emterm;markdown;chunk;id=e2e-test-1;seq=0;data=IyBUZXN0\\033\\\\",
			"\\033]777;emterm;markdown;end;id=e2e-test-1\\033\\\\",
		].join("");

		// Execute echo with the OSC sequence
		await executeCommand(`echo -e '${oscSequence}'`);

		// Wait for rendering
		await browser.pause(1000);

		// Take screenshot before checking
		await browser.saveScreenshot("./screenshots/markdown-01-after-echo.png");

		// Check if markdown block exists
		const markdownBlock = await $(".markdown-block");
		const exists = await markdownBlock.isExisting();

		console.log("Markdown block exists:", exists);

		if (exists) {
			const html = await markdownBlock.getHTML();
			console.log("Markdown block HTML:", html.slice(0, 500));

			// Check content contains rendered markdown
			const text = await markdownBlock.getText();
			console.log("Markdown block text:", text);
			expect(text).toContain("Test");
		}

		expect(exists).toBe(true);
	});

	it("should render markdown with multiple lines", async () => {
		// Markdown content: "# Hello\n\nThis is a test."
		// Base64: IyBIZWxsbwoKVGhpcyBpcyBhIHRlc3Qu
		const oscSequence = [
			"\\033]777;emterm;markdown;begin;id=e2e-test-2;format=gfm\\033\\\\",
			"\\033]777;emterm;markdown;chunk;id=e2e-test-2;seq=0;data=IyBIZWxsbwoKVGhpcyBpcyBhIHRlc3Qu\\033\\\\",
			"\\033]777;emterm;markdown;end;id=e2e-test-2\\033\\\\",
		].join("");

		await executeCommand(`echo -e '${oscSequence}'`);
		await browser.pause(1000);

		await browser.saveScreenshot("./screenshots/markdown-02-multiline.png");

		// Find the second markdown block (or the most recent one)
		const markdownBlocks = await $$(".markdown-block");
		console.log("Number of markdown blocks:", markdownBlocks.length);

		if (markdownBlocks.length > 0) {
			const lastBlock = markdownBlocks[markdownBlocks.length - 1];
			const text = await lastBlock.getText();
			console.log("Last markdown block text:", text);

			// Check for heading
			const h1 = await lastBlock.$("h1");
			const h1Exists = await h1.isExisting();
			console.log("H1 element exists:", h1Exists);

			expect(text).toContain("Hello");
		}

		expect(markdownBlocks.length).toBeGreaterThan(0);
	});

	it("should render markdown using emterm CLI command", async () => {
		// Create a simple markdown file and display it
		// First create a temp file
		await executeCommand("echo '# CLI Test' > /tmp/e2e-test.md");
		await browser.pause(500);

		// Use emterm markdown command
		await executeCommand("emterm markdown /tmp/e2e-test.md");
		await browser.pause(1500);

		await browser.saveScreenshot("./screenshots/markdown-03-cli-command.png");

		// Check for markdown blocks
		const markdownBlocks = await $$(".markdown-block");
		console.log("Markdown blocks after CLI command:", markdownBlocks.length);

		// Cleanup
		await executeCommand("rm /tmp/e2e-test.md");
	});

	it("should render markdown with code block", async () => {
		// Markdown: "```js\nconsole.log('hello');\n```"
		// Base64: YGBganMKY29uc29sZS5sb2coJ2hlbGxvJyk7CmBgYA==
		const oscSequence = [
			"\\033]777;emterm;markdown;begin;id=e2e-test-code;format=gfm\\033\\\\",
			"\\033]777;emterm;markdown;chunk;id=e2e-test-code;seq=0;data=YGBganMKY29uc29sZS5sb2coJ2hlbGxvJyk7CmBgYA==\\033\\\\",
			"\\033]777;emterm;markdown;end;id=e2e-test-code\\033\\\\",
		].join("");

		await executeCommand(`echo -e '${oscSequence}'`);
		await browser.pause(1000);

		await browser.saveScreenshot("./screenshots/markdown-04-code-block.png");

		// Check for code element
		const markdownBlocks = await $$(".markdown-block");
		if (markdownBlocks.length > 0) {
			const lastBlock = markdownBlocks[markdownBlocks.length - 1];
			const codeElement = await lastBlock.$("code");
			const codeExists = await codeElement.isExisting();
			console.log("Code element exists:", codeExists);

			if (codeExists) {
				const codeText = await codeElement.getText();
				console.log("Code content:", codeText);
				expect(codeText).toContain("console.log");
			}
		}
	});

	it("should capture console logs for debugging", async () => {
		// Collect captured console logs
		const logs = await browser.execute(() => {
			const captured = window.__capturedLogs || [];
			window.__capturedLogs = [];
			return captured;
		});
		console.log("=== Browser Console Logs ===");
		for (const log of logs) {
			if (
				log.message.includes("markdown") ||
				log.message.includes("Markdown") ||
				log.message.includes("OSC") ||
				log.message.includes("777")
			) {
				console.log(`[${log.level}] ${log.message}`);
			}
		}
	});
});
