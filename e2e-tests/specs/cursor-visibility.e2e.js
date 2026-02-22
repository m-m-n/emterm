/**
 * E2E test: Cursor visibility (DECTCEM CSI ?25l / ?25h)
 *
 * Tests each layer of the cursor visibility pipeline:
 * - Layer 0-4: Direct WASM core processing and TS mode sync
 * - Layer 5: PTY flow with printf
 * - Layer 6: Canvas pixel verification
 * - Layer 7: tput smcup/civis via PTY
 * - Layer 8: Real TUI program (top) cursor hiding regression test
 * - Layer 9: Full alt buffer + cursor hide sequence via PTY
 */

describe("Cursor Visibility", () => {
	before(async () => {
		await browser.waitUntil(
			async () => {
				const ready = await browser.execute(() => {
					return !!(window.terminalState && window.terminalApp);
				});
				return ready;
			},
			{ timeout: 15000, timeoutMsg: "Terminal app did not initialize" },
		);
		await browser.pause(3000);
	});

	it("Layer 0: initial state should have cursor visible", async () => {
		const state = await browser.execute(() => {
			const ts = window.terminalState;
			const core = ts.getActiveCore();
			return {
				tsVisible: ts.cursorVisible,
				tsModes: ts.modes.cursorVisible,
				wasmBit: core.get_mode(2),
			};
		});
		console.log("Layer 0 - Initial:", JSON.stringify(state));
		expect(state.tsVisible).toBe(true);
		expect(state.tsModes).toBe(true);
		expect(state.wasmBit).toBe(true);
	});

	it("Layer 1: WASM core should process CSI ?25l correctly", async () => {
		const result = await browser.execute(() => {
			const ts = window.terminalState;
			const core = ts.getActiveCore();

			const before = core.get_mode(2);
			const data = new Uint8Array([0x1b, 0x5b, 0x3f, 0x32, 0x35, 0x6c]);
			const consumed = core.process_pty_data(data);
			const after = core.get_mode(2);
			const actions = core.take_mode_actions();

			return { before, after, consumed, actionsLength: actions.length };
		});
		console.log("Layer 1 - WASM core:", JSON.stringify(result));
		expect(result.before).toBe(true);
		expect(result.after).toBe(false);
	});

	it("Layer 2: syncModesFromWasm should propagate to TS", async () => {
		const result = await browser.execute(() => {
			const ts = window.terminalState;
			const beforeSync = ts.modes.cursorVisible;
			ts.syncModesFromWasm();
			const afterSync = ts.modes.cursorVisible;
			return { beforeSync, afterSync, getter: ts.cursorVisible };
		});
		console.log("Layer 2 - Sync:", JSON.stringify(result));
		expect(result.afterSync).toBe(false);
	});

	it("Layer 3: renderer should update prevCursorVisible after render", async () => {
		await browser.execute(() => {
			const ts = window.terminalState;
			const r = window.terminalRenderer;
			r.forceRender(ts);
		});
		await browser.pause(200);

		const after = await browser.execute(() => {
			const r = window.terminalRenderer;
			return { prevCursorVisible: r.prevCursorVisible };
		});
		console.log("Layer 3 - After render:", JSON.stringify(after));
		expect(after.prevCursorVisible).toBe(false);
	});

	it("Layer 4: restore cursor with CSI ?25h", async () => {
		const result = await browser.execute(() => {
			const ts = window.terminalState;
			const core = ts.getActiveCore();
			const data = new Uint8Array([0x1b, 0x5b, 0x3f, 0x32, 0x35, 0x68]);
			core.process_pty_data(data);
			core.take_mode_actions();
			ts.syncModesFromWasm();
			return { wasmBit: core.get_mode(2), tsVisible: ts.cursorVisible };
		});
		console.log("Layer 4 - Restore:", JSON.stringify(result));
		expect(result.tsVisible).toBe(true);
	});

	it("Layer 5: PTY flow - send printf via pty.write()", async () => {
		const initial = await browser.execute(() => {
			return {
				cursorVisible: window.terminalState.cursorVisible,
				useAlternate: window.terminalState.useAlternate,
			};
		});
		console.log("Layer 5 - Initial:", JSON.stringify(initial));

		await browser.execute(() => {
			const encoder = new TextEncoder();
			window.terminalApp.pty.write(encoder.encode("printf '\\e[?25l'\n"));
		});
		await browser.pause(2000);

		const result = await browser.execute(() => {
			const ts = window.terminalState;
			const core = ts.getActiveCore();
			return {
				tsVisible: ts.cursorVisible,
				tsModes: ts.modes.cursorVisible,
				wasmBit: core.get_mode(2),
			};
		});
		console.log("Layer 5 - After printf CSI?25l:", JSON.stringify(result));

		await browser.execute(() => {
			const encoder = new TextEncoder();
			window.terminalApp.pty.write(encoder.encode("printf '\\e[?25h'\n"));
		});
		await browser.pause(1000);

		expect(result.tsVisible).toBe(false);
	});

	it("Layer 6: Pixel check - cursor not drawn when invisible", async () => {
		// Ensure cursor visible with blink stopped for clean pixel check
		await browser.execute(() => {
			const ts = window.terminalState;
			const r = window.terminalRenderer;
			const core = ts.getActiveCore();
			const showData = new Uint8Array([0x1b, 0x5b, 0x3f, 0x32, 0x35, 0x68]);
			core.process_pty_data(showData);
			core.take_mode_actions();
			ts.syncModesFromWasm();
			r.stopCursorBlink();
			r.cursorBlinkVisible = true;
			r.forceRender(ts);
		});
		await browser.pause(100);

		const visiblePixel = await browser.execute(() => {
			const ts = window.terminalState;
			const r = window.terminalRenderer;
			const cx = Math.floor(ts.cursorCol * r.charWidth) + Math.floor(r.charWidth / 2);
			const cy = Math.floor(ts.cursorRow * r.charHeight) + Math.floor(r.charHeight / 2);
			const pixel = r.ctx.getImageData(cx, cy, 1, 1).data;
			return {
				g: pixel[1],
				cursorVisible: ts.cursorVisible,
				blinkVisible: r.cursorBlinkVisible,
			};
		});
		console.log("Layer 6a - Cursor visible pixel:", JSON.stringify(visiblePixel));

		// Hide cursor and check pixel
		await browser.execute(() => {
			const ts = window.terminalState;
			const r = window.terminalRenderer;
			const core = ts.getActiveCore();
			const hideData = new Uint8Array([0x1b, 0x5b, 0x3f, 0x32, 0x35, 0x6c]);
			core.process_pty_data(hideData);
			core.take_mode_actions();
			ts.syncModesFromWasm();
			r.forceRender(ts);
		});
		await browser.pause(100);

		const hiddenPixel = await browser.execute(() => {
			const ts = window.terminalState;
			const r = window.terminalRenderer;
			const cx = Math.floor(ts.cursorCol * r.charWidth) + Math.floor(r.charWidth / 2);
			const cy = Math.floor(ts.cursorRow * r.charHeight) + Math.floor(r.charHeight / 2);
			const pixel = r.ctx.getImageData(cx, cy, 1, 1).data;
			return { g: pixel[1], cursorVisible: ts.cursorVisible };
		});
		console.log("Layer 6b - Cursor hidden pixel:", JSON.stringify(hiddenPixel));

		expect(visiblePixel.cursorVisible).toBe(true);
		expect(visiblePixel.g).toBeGreaterThan(100);
		expect(hiddenPixel.cursorVisible).toBe(false);
		expect(hiddenPixel.g).toBeLessThan(50);

		// Restore cursor for subsequent tests
		await browser.execute(() => {
			const ts = window.terminalState;
			const r = window.terminalRenderer;
			const core = ts.getActiveCore();
			core.process_pty_data(new Uint8Array([0x1b, 0x5b, 0x3f, 0x32, 0x35, 0x68]));
			core.take_mode_actions();
			ts.syncModesFromWasm();
			r.startCursorBlink();
		});
	});

	it("Layer 7: tput smcup/civis via PTY triggers mode changes", async () => {
		await browser.execute(() => {
			const encoder = new TextEncoder();
			window.terminalApp.pty.write(encoder.encode("tput smcup\n"));
		});
		await browser.pause(2000);

		const afterSmcup = await browser.execute(() => {
			return { useAlternate: window.terminalState.useAlternate };
		});
		console.log("Layer 7 - After tput smcup:", JSON.stringify(afterSmcup));

		await browser.execute(() => {
			const encoder = new TextEncoder();
			window.terminalApp.pty.write(encoder.encode("tput civis\n"));
		});
		await browser.pause(2000);

		const afterCivis = await browser.execute(() => {
			const ts = window.terminalState;
			return { tsVisible: ts.cursorVisible, wasmBit: ts.getActiveCore().get_mode(2) };
		});
		console.log("Layer 7 - After tput civis:", JSON.stringify(afterCivis));

		await browser.execute(() => {
			const encoder = new TextEncoder();
			window.terminalApp.pty.write(encoder.encode("tput cnorm; tput rmcup\n"));
		});
		await browser.pause(1000);

		const afterRestore = await browser.execute(() => {
			return {
				useAlternate: window.terminalState.useAlternate,
				cursorVisible: window.terminalState.cursorVisible,
			};
		});
		console.log("Layer 7 - After restore:", JSON.stringify(afterRestore));

		expect(afterSmcup.useAlternate).toBe(true);
		expect(afterCivis.tsVisible).toBe(false);
		expect(afterRestore.useAlternate).toBe(false);
		expect(afterRestore.cursorVisible).toBe(true);
	});

	it("Layer 8: top command hides cursor via CSI ?25l", async () => {
		// Regression test: top sends CSI ?1h (DECCKM) + CSI ?25l (civis) in
		// the same PTY chunk. Previously, setDecPrivateMode's syncModesToWasm
		// overwrote the WASM cursorVisible bit before syncModesFromWasm could
		// read it, causing cursor to remain visible during TUI programs.
		await browser.execute(() => {
			const encoder = new TextEncoder();
			window.terminalApp.pty.write(encoder.encode("top\n"));
		});

		// Wait for top to start and send its initialization sequences
		await browser.waitUntil(
			async () => {
				const visible = await browser.execute(() => {
					return window.terminalState.cursorVisible;
				});
				return !visible;
			},
			{ timeout: 10000, timeoutMsg: "top did not hide cursor within 10s" },
		);

		const state = await browser.execute(() => {
			const ts = window.terminalState;
			return {
				cursorVisible: ts.cursorVisible,
				wasmBit: ts.getActiveCore().get_mode(2),
			};
		});
		console.log("Layer 8 - During top:", JSON.stringify(state));

		// Quit top
		await browser.execute(() => {
			const encoder = new TextEncoder();
			window.terminalApp.pty.write(encoder.encode("q"));
		});
		await browser.pause(2000);

		const afterQuit = await browser.execute(() => {
			return { cursorVisible: window.terminalState.cursorVisible };
		});
		console.log("Layer 8 - After quit:", JSON.stringify(afterQuit));

		expect(state.cursorVisible).toBe(false);
		expect(state.wasmBit).toBe(false);
		expect(afterQuit.cursorVisible).toBe(true);
	});

	it("Layer 9: PTY flow - full top-like sequence (alt buffer + hide)", async () => {
		await browser.execute(() => {
			const encoder = new TextEncoder();
			const cmd = encoder.encode("printf '\\e[?1049h\\e[?25l\\e[H\\e[2Jhello from alt'\n");
			window.terminalApp.pty.write(cmd);
		});
		await browser.pause(2000);

		const afterAlt = await browser.execute(() => {
			const ts = window.terminalState;
			return {
				tsVisible: ts.cursorVisible,
				wasmBit: ts.getActiveCore().get_mode(2),
				useAlternate: ts.useAlternate,
			};
		});
		console.log("Layer 9 - After alt+hide:", JSON.stringify(afterAlt));

		await browser.execute(() => {
			const encoder = new TextEncoder();
			window.terminalApp.pty.write(encoder.encode("printf '\\e[?25h\\e[?1049l'\n"));
		});
		await browser.pause(1000);

		const afterRestore = await browser.execute(() => {
			return {
				cursorVisible: window.terminalState.cursorVisible,
				useAlternate: window.terminalState.useAlternate,
			};
		});
		console.log("Layer 9 - After restore:", JSON.stringify(afterRestore));

		expect(afterAlt.tsVisible).toBe(false);
		expect(afterAlt.useAlternate).toBe(true);
		expect(afterRestore.cursorVisible).toBe(true);
	});
});
