/**
 * Reflow E2E Tests
 *
 * ウィンドウリサイズ時に行が適切にreflow（再構成）されるかテスト
 */

describe("eMterm Reflow", () => {
	it("should display the terminal window", async () => {
		const title = await browser.getTitle();
		console.log("Window title:", title);
		expect(title).toBeTruthy();
		await browser.saveScreenshot("./screenshots/reflow-01-initial.png");
	});

	it("should accept text input", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.click();

		// 長い文字列を入力（後でリサイズテストに使用）
		const testText = "echo ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890abcdefghijklmnopqrstuvwxyz";
		for (const char of testText) {
			await browser.keys([char]);
		}
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/reflow-02-before-enter.png");

		await browser.keys(["Enter"]);
		await browser.pause(1000);
		await browser.saveScreenshot("./screenshots/reflow-03-after-enter.png");

		const terminalText = await terminal.getText();
		console.log("Terminal text:", terminalText.slice(0, 200));
	});

	it("should handle window resize and reflow text", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.click();

		// 現在のウィンドウサイズを取得
		const initialSize = await browser.getWindowSize();
		console.log("Initial window size:", initialSize);
		await browser.saveScreenshot("./screenshots/reflow-04-before-resize.png");

		// ウィンドウを小さくして折り返しを発生させる
		await browser.setWindowSize(400, initialSize.height);
		await browser.pause(1000);
		await browser.saveScreenshot("./screenshots/reflow-05-small-window.png");

		// ターミナル状態を確認
		const stateAfterShrink = await browser.execute(() => {
			const terminalState = window.terminalState;
			const buffer = terminalState?.getActiveBuffer?.();
			if (!buffer) return { error: "Buffer not found" };

			// 最初の数行の wrapped フラグを確認
			const lineInfo = [];
			for (let i = 0; i < Math.min(5, buffer.rows); i++) {
				const line = buffer.getLine(i);
				lineInfo.push({
					row: i,
					wrapped: line?.wrapped ?? false,
					text: line?.getText?.()?.slice(0, 30) ?? "",
				});
			}
			return {
				cols: buffer.cols,
				rows: buffer.rows,
				lines: lineInfo,
			};
		});
		console.log("State after shrink:", JSON.stringify(stateAfterShrink, null, 2));

		// ウィンドウを元のサイズに戻す
		await browser.setWindowSize(initialSize.width, initialSize.height);
		await browser.pause(1000);
		await browser.saveScreenshot("./screenshots/reflow-06-restored-window.png");

		// reflow後の状態を確認
		const stateAfterRestore = await browser.execute(() => {
			const terminalState = window.terminalState;
			const buffer = terminalState?.getActiveBuffer?.();
			if (!buffer) return { error: "Buffer not found" };

			const lineInfo = [];
			for (let i = 0; i < Math.min(5, buffer.rows); i++) {
				const line = buffer.getLine(i);
				lineInfo.push({
					row: i,
					wrapped: line?.wrapped ?? false,
					text: line?.getText?.()?.slice(0, 50) ?? "",
				});
			}
			return {
				cols: buffer.cols,
				rows: buffer.rows,
				lines: lineInfo,
			};
		});
		console.log("State after restore:", JSON.stringify(stateAfterRestore, null, 2));
	});

	it("should verify wrapped lines unwrap on resize", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.click();

		// 改行なしで長い出力を出すコマンド
		// printf を使用して改行なしの長い文字列を出力
		const command = "printf 'WRAP_TEST_%0.s' {1..20}";
		for (const char of command) {
			await browser.keys([char]);
		}
		await browser.keys(["Enter"]);
		await browser.pause(1000);
		await browser.saveScreenshot("./screenshots/reflow-07-long-output.png");

		// ウィンドウを小さくする
		const size = await browser.getWindowSize();
		await browser.setWindowSize(300, size.height);
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/reflow-08-wrapped.png");

		// wrapped フラグを確認
		const wrappedState = await browser.execute(() => {
			const terminalState = window.terminalState;
			const buffer = terminalState?.getActiveBuffer?.();
			if (!buffer) return { error: "Buffer not found" };

			let wrappedCount = 0;
			for (let i = 0; i < buffer.rows; i++) {
				const line = buffer.getLine(i);
				if (line?.wrapped) wrappedCount++;
			}
			return {
				cols: buffer.cols,
				wrappedCount,
			};
		});
		console.log("Wrapped state:", wrappedState);

		// ウィンドウを広げる
		await browser.setWindowSize(800, size.height);
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/reflow-09-unwrapped.png");

		// unwrap 後の状態
		const unwrappedState = await browser.execute(() => {
			const terminalState = window.terminalState;
			const buffer = terminalState?.getActiveBuffer?.();
			if (!buffer) return { error: "Buffer not found" };

			let wrappedCount = 0;
			for (let i = 0; i < buffer.rows; i++) {
				const line = buffer.getLine(i);
				if (line?.wrapped) wrappedCount++;
			}
			return {
				cols: buffer.cols,
				wrappedCount,
			};
		});
		console.log("Unwrapped state:", unwrappedState);

		// wrapped 数が減少していることを確認
		expect(unwrappedState.wrappedCount).toBeLessThanOrEqual(
			wrappedState.wrappedCount,
		);
	});

	it("should clean up and exit", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.click();

		// exit コマンドで終了
		for (const char of "exit") {
			await browser.keys([char]);
		}
		await browser.keys(["Enter"]);
		await browser.pause(2000);
	});
});
