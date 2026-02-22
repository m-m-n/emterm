/**
 * Kitty Protocol Compatibility E2E Test with treemd
 *
 * treemd (ratatui/crossterm-based TUI Markdown viewer) が
 * eMterm の Kitty プロトコルサポートを正しく検出できるか検証する。
 *
 * 検証項目:
 * - treemd が alternate buffer で正常起動する
 * - "No interactive elements" 警告が表示されない
 * - 赤背景（capability detection 失敗の兆候）が表示されない
 * - Markdown コンテンツが正しく表示される
 * - q キーで正常終了し primary buffer に復帰する
 */

describe("Kitty Protocol - treemd Compatibility", () => {
	it("should initialize terminal and launch treemd", async () => {
		// window.terminalState が利用可能になるまで待機
		await browser.waitUntil(
			async () => {
				const ready = await browser.execute(() => {
					return !!window.terminalState && window.terminalState.cols > 0;
				});
				return ready;
			},
			{ timeout: 30000, timeoutMsg: "terminalState not ready within 30s" },
		);

		// シェルプロンプトが表示されるまで追加待機
		await browser.pause(3000);

		await browser.saveScreenshot(
			"./screenshots/kitty-treemd-01-initial.png",
		);

		// treemd コマンドを入力 (body にフォーカスしてキー送信)
		const command = "treemd /app/e2e-tests/fixtures/treemd-test.md";
		for (const char of command) {
			await browser.keys([char]);
		}
		await browser.pause(500);
		await browser.keys(["Enter"]);

		// Kitty capability detection + 描画を待機
		await browser.pause(8000);

		await browser.saveScreenshot(
			"./screenshots/kitty-treemd-02-running.png",
		);

		// alternate buffer に切り替わっているか確認
		const isAltBuffer = await browser.execute(() => {
			return window.terminalState?.isAlternateBuffer ?? false;
		});
		console.log("isAlternateBuffer:", isAltBuffer);
		expect(isAltBuffer).toBe(true);
	});

	it("should not show 'No interactive elements' warning", async () => {
		// 画面全体のテキストを取得
		const text = await browser.execute(() => {
			const state = window.terminalState;
			if (!state) return "";
			const buffer = state.getActiveBuffer();
			if (!buffer) return "";
			return state.extractText(0, 0, buffer.cols - 1, buffer.rows - 1);
		});
		console.log(
			"Screen text (first 500 chars):",
			text.slice(0, 500),
		);

		// 警告テキストが含まれないことを検証
		expect(text).not.toContain("No interactive elements");
		expect(text).not.toContain("\u26a0");
	});

	it("should not have red background cells (capability detection failure)", async () => {
		// 全セルの背景色を走査して赤背景の有無を確認
		const result = await browser.execute(() => {
			const state = window.terminalState;
			if (!state) return { error: "no state" };
			const buffer = state.getActiveBuffer();
			if (!buffer) return { error: "no buffer" };

			let redCellCount = 0;
			const redCells = [];

			for (let row = 0; row < buffer.rows; row++) {
				const line = buffer.getLine(row);
				if (!line) continue;
				for (let col = 0; col < buffer.cols; col++) {
					const cell = line.getCell(col);
					if (!cell || !cell.attrs || !cell.attrs.bg) continue;

					const bg = cell.attrs.bg;
					let isRed = false;

					if (bg.type === "indexed" && (bg.index === 1 || bg.index === 9)) {
						isRed = true;
					} else if (
						bg.type === "rgb" &&
						bg.r > 200 &&
						bg.g < 80 &&
						bg.b < 80
					) {
						isRed = true;
					}

					if (isRed) {
						redCellCount++;
						if (redCells.length < 5) {
							redCells.push({
								col,
								row,
								bg: JSON.parse(JSON.stringify(bg)),
							});
						}
					}
				}
			}

			return { redCellCount, redCells };
		});

		console.log("Red background scan result:", JSON.stringify(result));

		if (result.error) {
			console.warn("Could not scan cells:", result.error);
		} else {
			expect(result.redCellCount).toBe(0);
		}
	});

	it("should display markdown content correctly", async () => {
		const text = await browser.execute(() => {
			const state = window.terminalState;
			if (!state) return "";
			const buffer = state.getActiveBuffer();
			if (!buffer) return "";
			return state.extractText(0, 0, buffer.cols - 1, buffer.rows - 1);
		});

		console.log("Content check text (first 500 chars):", text.slice(0, 500));

		// treemd がコンテンツを正しく表示しているか
		expect(text).toContain("Test Document");
		expect(text).toContain("Section One");
	});

	it("should exit treemd and return to primary buffer", async () => {
		// q キーで treemd を終了
		await browser.keys(["q"]);
		await browser.pause(2000);

		await browser.saveScreenshot(
			"./screenshots/kitty-treemd-03-after-quit.png",
		);

		// primary buffer に復帰しているか確認
		const isAltBuffer = await browser.execute(() => {
			return window.terminalState?.isAlternateBuffer ?? false;
		});
		console.log("isAlternateBuffer after quit:", isAltBuffer);
		expect(isAltBuffer).toBe(false);
	});
});
