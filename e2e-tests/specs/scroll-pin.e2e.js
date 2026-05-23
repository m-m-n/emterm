/**
 * Pin viewport when scrolled up — E2E spec (TS-7).
 *
 * Verifies that while the user has scrolled up (scrollOffset > 0) and new
 * PTY output causes the scrollback to grow, the visible absolute rows do
 * not shift on screen. See:
 *   - doc/tasks/pin-viewport-when-scrolled-up/SPEC.md (FR1, US1)
 *   - doc/tasks/pin-viewport-when-scrolled-up/VERIFICATION.md TS-7
 *
 * Strategy:
 *   1. Wait for terminal to be ready.
 *   2. Fill scrollback with `seq` so we have many distinct lines.
 *   3. Programmatically call renderer.setScrollOffset(N) to scroll up.
 *   4. Sample the text of a fixed display row via getVisibleLines.
 *   5. Trigger another `seq` burst by writing straight to PtyClient,
 *      bypassing the keyboard handler (which would otherwise call
 *      onExitScrollback() and reset our pin — that is FR4 by design).
 *   6. Sample the same display row again.
 *   7. Assert the two samples are equal — pin worked.
 *
 * The spec relies on the global window.terminalApp exposed by main.ts.
 */

const PIN_DISPLAY_ROW = 5;

async function getTerminalContext() {
	return browser.execute(() => {
		const app = window.terminalApp;
		if (!app) return { error: "terminalApp missing" };
		const renderer = app.renderer ?? app.getRenderer?.();
		const state = app.state ?? app.getState?.();
		if (!renderer || !state) return { error: "renderer/state missing" };
		return {
			ok: true,
			rows: state.rows,
			scrollbackLength: state.getScrollbackLength(),
			scrollOffset: renderer.getScrollOffset(),
		};
	});
}

async function sampleRowText(displayRow) {
	return browser.execute((row) => {
		const app = window.terminalApp;
		const renderer = app.renderer ?? app.getRenderer?.();
		const state = app.state ?? app.getState?.();
		if (!renderer || !state) return { error: "no renderer/state" };

		const scrollOffset = renderer.getScrollOffset();
		const scrollbackLength = state.getScrollbackLength();
		const buffer = state.getActiveBuffer();
		const visibleRows = state.rows;

		const startIndex = Math.max(0, scrollbackLength - scrollOffset);
		const lineIndex = startIndex + row;

		let text = "";
		if (lineIndex < scrollbackLength) {
			const line = state.getScrollbackLine(lineIndex);
			text = line ? line.getText() : "";
		} else if (lineIndex - scrollbackLength < visibleRows) {
			const line = buffer.getLine(lineIndex - scrollbackLength);
			text = line ? line.getText() : "";
		}

		return {
			ok: true,
			scrollOffset,
			scrollbackLength,
			text: (text || "").trimEnd(),
		};
	}, displayRow);
}

async function setScrollOffset(offset) {
	return browser.execute((off) => {
		const app = window.terminalApp;
		const renderer = app.renderer ?? app.getRenderer?.();
		const state = app.state ?? app.getState?.();
		if (!renderer || !state) return { error: "no renderer/state" };
		renderer.setScrollOffset(off);
		renderer.forceRender(state);
		return { ok: true, scrollOffset: renderer.getScrollOffset() };
	}, offset);
}

async function typeCommand(cmd) {
	for (const ch of cmd) {
		await browser.keys(ch);
	}
	await browser.keys(["Enter"]);
}

/**
 * Write directly to the PTY, bypassing the keyboard handler.
 *
 * The keyboard handler in src/terminal-app/handlers/keyboard.ts calls
 * onExitScrollback() on every key event (FR4: User-initiated scroll
 * unchanged), which calls setScrollOffset(0) and would defeat the pin we
 * are trying to verify here. Writing straight to PtyClient.write avoids
 * that path and faithfully simulates "PTY output arrives while user is
 * scrolled up", which is the scenario FR1 covers.
 */
async function ptyWrite(cmd) {
	return browser.execute((c) => {
		const app = window.terminalApp;
		if (!app) return { error: "terminalApp missing" };
		const pty = app.pty ?? app.getPtyClient?.() ?? window.ptyClient;
		if (!pty) return { error: "ptyClient missing" };
		// PtyClient.write accepts string | Uint8Array.
		return pty
			.write(`${c}\n`)
			.then(() => ({ ok: true }))
			.catch((err) => ({ error: String(err) }));
	}, cmd);
}

describe("Pin viewport when scrolled up (TS-7)", () => {
	before(async () => {
		const terminal = await $(".tab-content");
		await terminal.waitForExist({ timeout: 30000 });
		await browser.pause(2000);
		await terminal.click();
		await browser.pause(500);
	});

	it("keeps the visible row text stable across a PTY burst while scrolled up", async () => {
		// 1. Fill scrollback with a recognizable `seq` burst.
		await typeCommand("seq 1 400");
		await browser.pause(2500);

		const ctxAfterFill = await getTerminalContext();
		console.log("After fill:", JSON.stringify(ctxAfterFill));
		expect(ctxAfterFill.ok).toBe(true);
		expect(ctxAfterFill.scrollbackLength).toBeGreaterThan(50);

		// 2. Scroll up programmatically (avoids relying on key event focus).
		const pinOffset = Math.min(40, Math.max(20, ctxAfterFill.scrollbackLength - ctxAfterFill.rows - 1));
		const scrolled = await setScrollOffset(pinOffset);
		console.log("After setScrollOffset:", JSON.stringify(scrolled));
		expect(scrolled.scrollOffset).toBe(pinOffset);

		await browser.pause(300);
		await browser.saveScreenshot("./screenshots/scroll-pin-before-burst.png");

		// 3. Sample the pinned row before the next burst.
		const before = await sampleRowText(PIN_DISPLAY_ROW);
		console.log("Pinned row before burst:", JSON.stringify(before));
		expect(before.ok).toBe(true);
		expect(before.text.length).toBeGreaterThan(0);

		// 4. Trigger a fresh burst that grows scrollback under us.
		//    Write straight to PTY so the keyboard handler's
		//    onExitScrollback() (FR4) does not reset our pin.
		const wrote = await ptyWrite("seq 1 200");
		console.log("ptyWrite result:", JSON.stringify(wrote));
		expect(wrote.ok).toBe(true);
		await browser.pause(3000);

		const ctxAfterBurst = await getTerminalContext();
		console.log("After burst:", JSON.stringify(ctxAfterBurst));
		// scrollOffset must have been adjusted upward by the pin logic so
		// the user-visible top row stays put.
		expect(ctxAfterBurst.scrollbackLength).toBeGreaterThan(ctxAfterFill.scrollbackLength);
		expect(ctxAfterBurst.scrollOffset).toBeGreaterThan(pinOffset);

		await browser.saveScreenshot("./screenshots/scroll-pin-after-burst.png");

		// 5. Sample the same display row after the burst — must match.
		const after = await sampleRowText(PIN_DISPLAY_ROW);
		console.log("Pinned row after burst:", JSON.stringify(after));
		expect(after.ok).toBe(true);
		expect(after.text).toBe(before.text);
	});
});
