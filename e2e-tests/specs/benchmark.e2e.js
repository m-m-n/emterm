/**
 * Terminal Performance Benchmark E2E Test
 *
 * Measures throughput and rendering performance by running `seq 1 N`
 * and timing how long until the shell prompt returns.
 *
 * Usage:
 *   ./scripts/run-e2e-docker.sh test benchmark.e2e.js
 *
 * Results are printed to stdout in a parseable format:
 *   BENCHMARK_RESULT: { lines, wallTimeMs, ... }
 */

// Number of lines to output (configurable via env, default 1M)
const BENCHMARK_LINES = parseInt(process.env.BENCHMARK_LINES || "1000000", 10);

// Max time to wait for command completion (ms)
const BENCHMARK_TIMEOUT_MS = parseInt(
	process.env.BENCHMARK_TIMEOUT || "600000",
	10,
);

async function typeSlowly(text, delay = 50) {
	for (const char of text) {
		await browser.keys(char);
		await browser.pause(delay);
	}
}

/**
 * Wait for window.terminalState to exist with rows > 0.
 */
async function waitForTerminalReady(timeoutMs = 30000, pollMs = 500) {
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		const ready = await browser.execute(() => {
			return !!(window.terminalState && window.terminalState.rows > 0);
		});
		if (ready) return true;
		await browser.pause(pollMs);
	}
	return false;
}

/**
 * Click on the terminal to focus it.
 */
async function focusTerminal() {
	let el = await $("canvas");
	if (await el.isExisting()) {
		await el.click();
		return;
	}
	el = await $(".terminal-root");
	if (await el.isExisting()) {
		await el.click();
		return;
	}
	el = await $(".tab-content");
	if (await el.isExisting()) {
		await el.click();
		return;
	}
	throw new Error("No terminal element found to focus");
}

/**
 * Poll terminal buffer for shell prompt (line ending with "$" or "#").
 */
async function waitForPrompt(timeoutMs = BENCHMARK_TIMEOUT_MS, pollMs = 500) {
	const deadline = Date.now() + timeoutMs;

	while (Date.now() < deadline) {
		const found = await browser.execute(() => {
			const state = window.terminalState;
			if (!state) return false;
			const buffer = state.getActiveBuffer();
			if (!buffer) return false;
			const rows = state.rows;
			for (let r = rows - 1; r >= Math.max(0, rows - 5); r--) {
				const line = buffer.getLine(r);
				if (!line) continue;
				let text = "";
				for (let c = 0; c < line.length; c++) {
					const cell = line.getCell(c);
					if (cell) text += cell.char;
				}
				text = text.trimEnd();
				if (text.length > 0 && (text.endsWith("$") || text.endsWith("#"))) {
					return true;
				}
			}
			return false;
		});

		if (found) return true;
		await browser.pause(pollMs);
	}

	return false;
}

/**
 * Get the last non-empty line text from the terminal.
 */
async function getLastLineText() {
	return browser.execute(() => {
		const state = window.terminalState;
		if (!state) return "";
		const buffer = state.getActiveBuffer();
		const rows = state.rows;
		for (let r = rows - 1; r >= 0; r--) {
			const line = buffer.getLine(r);
			let text = "";
			for (let c = 0; c < line.length; c++) {
				text += line.getCell(c).char;
			}
			text = text.trimEnd();
			if (text.length > 0) return text;
		}
		return "";
	});
}

/**
 * Get terminal metrics snapshot.
 */
async function getTerminalMetrics() {
	return browser.execute(() => {
		const state = window.terminalState;
		if (!state)
			return { cols: 0, rows: 0, scrollbackLength: 0, cursorRow: 0 };
		return {
			cols: state.cols,
			rows: state.rows,
			scrollbackLength: state.getScrollbackLength(),
			cursorRow: state.cursorRow,
		};
	});
}

describe("Terminal Performance Benchmark", function () {
	this.timeout(BENCHMARK_TIMEOUT_MS + 30000);

	it(`should benchmark seq 1 ${BENCHMARK_LINES}`, async function () {
		this.timeout(BENCHMARK_TIMEOUT_MS + 30000);

		// Wait for terminal to initialize
		await waitForTerminalReady(30000, 500);
		await focusTerminal();
		await browser.pause(3000);

		// Type the command
		const command = `bash -c 'time seq 1 ${BENCHMARK_LINES}'`;
		console.log(`Typing command: ${command}`);
		await typeSlowly(command);
		await browser.pause(300);
		await browser.saveScreenshot("./screenshots/bench-01-typed.png");

		// Record start time and press Enter
		const startMs = Date.now();
		console.log(`Starting benchmark at ${new Date(startMs).toISOString()}`);
		await browser.keys("Enter");

		// Poll for completion with progress reporting
		let lastReport = startMs;
		let completed = false;
		const progressInterval = 5000;

		while (Date.now() - startMs < BENCHMARK_TIMEOUT_MS) {
			const now = Date.now();

			if (now - lastReport >= progressInterval) {
				const metrics = await getTerminalMetrics();
				const elapsed = ((now - startMs) / 1000).toFixed(1);
				console.log(
					`  [${elapsed}s] scrollback: ${metrics.scrollbackLength}, cursor row: ${metrics.cursorRow}`,
				);
				lastReport = now;
			}

			const found = await browser.execute(() => {
				const state = window.terminalState;
				if (!state) return false;
				const buffer = state.getActiveBuffer();
				const rows = state.rows;
				for (let r = rows - 1; r >= Math.max(0, rows - 3); r--) {
					const line = buffer.getLine(r);
					let text = "";
					for (let c = 0; c < line.length; c++) {
						text += line.getCell(c).char;
					}
					text = text.trimEnd();
					if (text.length > 0 && (text.endsWith("$") || text.endsWith("#"))) {
						return true;
					}
				}
				return false;
			});

			if (found) {
				completed = true;
				break;
			}

			await browser.pause(500);
		}

		const endMs = Date.now();
		const wallTimeMs = endMs - startMs;
		const wallTimeSec = (wallTimeMs / 1000).toFixed(2);

		await browser.saveScreenshot("./screenshots/bench-02-complete.png");
		const finalMetrics = await getTerminalMetrics();
		const lastLine = await getLastLineText();

		const result = {
			lines: BENCHMARK_LINES,
			wallTimeMs,
			wallTimeSec: parseFloat(wallTimeSec),
			completed,
			scrollbackLength: finalMetrics.scrollbackLength,
			terminalSize: `${finalMetrics.cols}x${finalMetrics.rows}`,
			lastLine: lastLine.slice(0, 80),
			linesPerSecond: completed
				? Math.round(BENCHMARK_LINES / (wallTimeMs / 1000))
				: null,
		};

		console.log("=".repeat(60));
		console.log("BENCHMARK_RESULT:", JSON.stringify(result));
		console.log("=".repeat(60));
		console.log(`  Lines:          ${result.lines.toLocaleString()}`);
		console.log(`  Wall time:      ${result.wallTimeSec}s`);
		console.log(`  Completed:      ${result.completed}`);
		console.log(`  Lines/sec:      ${result.linesPerSecond?.toLocaleString() ?? "N/A"}`);
		console.log(`  Scrollback:     ${result.scrollbackLength.toLocaleString()}`);
		console.log(`  Terminal size:  ${result.terminalSize}`);
		console.log("=".repeat(60));

		expect(completed).toBe(true);
	});

	it("should verify terminal is still responsive after benchmark", async () => {
		await focusTerminal();
		await browser.pause(500);

		await typeSlowly("echo RESPONSIVE_CHECK");
		await browser.keys("Enter");
		await browser.pause(2000);

		const lastLine = await getLastLineText();
		console.log("Responsiveness check - last line:", lastLine);
		await browser.saveScreenshot("./screenshots/bench-03-responsive.png");

		const output = await browser.execute(() => {
			const state = window.terminalState;
			const buffer = state.getActiveBuffer();
			const rows = state.rows;
			for (let r = rows - 1; r >= 0; r--) {
				const line = buffer.getLine(r);
				let text = "";
				for (let c = 0; c < line.length; c++) {
					text += line.getCell(c).char;
				}
				if (text.includes("RESPONSIVE_CHECK") && !text.includes("echo")) {
					return text.trim();
				}
			}
			return null;
		});

		console.log("Echo output:", output);
		expect(output).toContain("RESPONSIVE_CHECK");
	});
});
