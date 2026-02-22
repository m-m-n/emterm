/**
 * treemd color diagnostic - dump cell attributes at corruption boundary
 */
describe("treemd Color Diagnostic", () => {
	it("should dump cell colors around corruption boundary", async () => {
		await browser.waitUntil(
			async () => {
				const ready = await browser.execute(() => {
					return !!window.terminalState && window.terminalState.cols > 0;
				});
				return ready;
			},
			{ timeout: 30000, timeoutMsg: "terminalState not ready" },
		);
		await browser.pause(3000);

		const command = "treemd /app/e2e-tests/fixtures/treemd-test.md";
		for (const char of command) {
			await browser.keys([char]);
		}
		await browser.pause(500);
		await browser.keys(["Enter"]);
		await browser.pause(8000);

		await browser.saveScreenshot("./screenshots/kitty-treemd-diag.png");

		// Dump first 5 rows, full width cell attributes
		const diag = await browser.execute(() => {
			const state = window.terminalState;
			if (!state) return { error: "no state" };
			const buffer = state.getActiveBuffer();
			if (!buffer) return { error: "no buffer" };

			const result = {
				cols: buffer.cols,
				rows: buffer.rows,
				isAlt: state.isAlternateBuffer,
				modes: {
					reverseScreen: state.getModes().reverseScreen,
					autoWrap: state.getModes().autoWrap,
				},
				rowData: [],
			};

			// Dump rows 0-4 (title, panel headers, content start)
			for (let row = 0; row < Math.min(6, buffer.rows); row++) {
				const line = buffer.getLine(row);
				if (!line) continue;
				const cells = [];
				for (let col = 0; col < buffer.cols; col++) {
					const cell = line.getCell(col);
					if (!cell) continue;
					const ch = cell.char;
					const bg = cell.attrs.bg;
					const fg = cell.attrs.fg;
					// Only record cells where bg is non-null (has explicit color)
					if (bg || (ch && ch !== " " && ch !== "\x00")) {
						cells.push({
							col,
							char: ch || "",
							fg: fg ? JSON.parse(JSON.stringify(fg)) : null,
							bg: bg ? JSON.parse(JSON.stringify(bg)) : null,
							reverse: cell.attrs.reverse || false,
						});
					}
				}
				result.rowData.push({ row, cells });
			}

			return result;
		});

		console.log("=== DIAGNOSTIC ===");
		console.log("cols:", diag.cols, "rows:", diag.rows, "isAlt:", diag.isAlt);
		console.log("modes:", JSON.stringify(diag.modes));

		if (diag.rowData) {
			for (const rd of diag.rowData) {
				console.log(`\n--- Row ${rd.row} ---`);
				for (const c of rd.cells) {
					const bgStr = c.bg
						? c.bg.type === "indexed"
							? `idx:${c.bg.index}`
							: c.bg.type === "rgb"
								? `rgb(${c.bg.r},${c.bg.g},${c.bg.b})`
								: c.bg.type
						: "-";
					const fgStr = c.fg
						? c.fg.type === "indexed"
							? `idx:${c.fg.index}`
							: c.fg.type === "rgb"
								? `rgb(${c.fg.r},${c.fg.g},${c.fg.b})`
								: c.fg.type
						: "-";
					console.log(
						`  col=${c.col} char='${c.char}' fg=${fgStr} bg=${bgStr} rev=${c.reverse}`,
					);
				}
			}
		}

		// Quit treemd
		await browser.keys(["q"]);
		await browser.pause(1000);
	});
});
