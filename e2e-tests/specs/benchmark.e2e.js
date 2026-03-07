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

		// Step 1: Record start time in shell variable
		await typeSlowly("S=$(date +%s%3N)");
		await browser.keys("Enter");
		await browser.pause(500);

		// Step 2: Run time seq (use bash -c for time builtin; Docker shell is sh)
		const timeCmd = `bash -c 'time seq 1 ${BENCHMARK_LINES}' 2>&1`;
		console.log(`Typing command: ${timeCmd}`);
		await typeSlowly(timeCmd);
		await browser.pause(300);
		await browser.saveScreenshot("./screenshots/bench-01-typed.png");

		const jsStartMs = Date.now();
		console.log(`Starting benchmark at ${new Date(jsStartMs).toISOString()}`);
		await browser.keys("Enter");

		// Wait for prompt to reappear (command completed)
		let lastReport = jsStartMs;
		let completed = false;
		const progressInterval = 5000;
		await browser.pause(2000);

		while (Date.now() - jsStartMs < BENCHMARK_TIMEOUT_MS) {
			const now = Date.now();

			if (now - lastReport >= progressInterval) {
				const metrics = await getTerminalMetrics();
				const elapsed = ((now - jsStartMs) / 1000).toFixed(1);
				console.log(
					`  [${elapsed}s] scrollback: ${metrics.scrollbackLength}, cursor row: ${metrics.cursorRow}`,
				);
				lastReport = now;
			}

			const promptFound = await browser.execute(() => {
				const state = window.terminalState;
				if (!state) return false;
				if (state.getScrollbackLength() === 0) return false;
				const buffer = state.getActiveBuffer();
				const rows = state.rows;
				for (let r = rows - 1; r >= Math.max(0, rows - 3); r--) {
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

			if (promptFound) {
				completed = true;
				break;
			}

			await browser.pause(500);
		}

		// Step 3: Record end time and output wall time
		if (completed) {
			await typeSlowly("E=$(date +%s%3N);echo BM:$((E-S))");
			await browser.keys("Enter");
			await browser.pause(2000);
		}

		// Parse BM: and time real from terminal buffer
		let shellTimeMs = null;
		let timeRealSec = null;

		if (completed) {
			const parseResult = await browser.execute(() => {
				const state = window.terminalState;
				if (!state) return null;
				const buffer = state.getActiveBuffer();
				const rows = state.rows;
				let bmMs = null;
				let realTime = null;
				for (let r = rows - 1; r >= 0; r--) {
					const line = buffer.getLine(r);
					if (!line) continue;
					let text = "";
					for (let c = 0; c < line.length; c++) {
						const cell = line.getCell(c);
						if (cell) text += cell.char;
					}
					const trimmed = text.trim();
					const bmMatch = trimmed.match(/^BM:(\d+)$/);
					if (bmMatch) bmMs = parseInt(bmMatch[1], 10);
					// bash time format: "real\t0m4.123s"
					const realMatch = trimmed.match(/^real\s+(\d+)m([\d.]+)s$/);
					if (realMatch) {
						realTime = parseFloat(realMatch[1]) * 60 + parseFloat(realMatch[2]);
					}
				}
				return { bmMs, realTime };
			});

			if (parseResult) {
				shellTimeMs = parseResult.bmMs;
				timeRealSec = parseResult.realTime;
			}
		}

		const jsEndMs = Date.now();
		await browser.saveScreenshot("./screenshots/bench-02-complete.png");
		const finalMetrics = await getTerminalMetrics();
		const lastLine = await getLastLineText();

		// Shell time (date before/after seq) = PTY execution time including backpressure
		const shellTimeSec = shellTimeMs !== null ? shellTimeMs / 1000 : null;
		// JS wall time = rendering complete time (from Enter to marker detection)
		const jsWallMs = jsEndMs - jsStartMs;
		const jsWallSec = jsWallMs / 1000;
		// Rendering delay = JS wall time - shell time
		const delaySec = shellTimeSec !== null
			? parseFloat((jsWallSec - shellTimeSec).toFixed(3))
			: null;

		const result = {
			lines: BENCHMARK_LINES,
			shellTimeMs,
			shellTimeSec,
			timeRealSec,
			jsWallSec: parseFloat(jsWallSec.toFixed(2)),
			delaySec,
			completed,
			scrollbackLength: finalMetrics.scrollbackLength,
			terminalSize: `${finalMetrics.cols}x${finalMetrics.rows}`,
			lastLine: lastLine.slice(0, 80),
			linesPerSecond: shellTimeSec
				? Math.round(BENCHMARK_LINES / shellTimeSec)
				: null,
		};

		console.log("=".repeat(60));
		console.log("BENCHMARK_RESULT:", JSON.stringify(result));
		console.log("=".repeat(60));
		console.log(`  Lines:          ${result.lines.toLocaleString()}`);
		console.log(`  Shell time:     ${shellTimeSec !== null ? shellTimeSec + "s" : "N/A"} (date)`);
		console.log(`  time real:      ${timeRealSec !== null ? timeRealSec + "s" : "N/A"}`);
		console.log(`  JS wall time:   ${result.jsWallSec}s (Enter → marker detected)`);
		console.log(`  Delay:          ${delaySec !== null ? delaySec + "s" : "N/A"} (JS wall - shell)`);
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
