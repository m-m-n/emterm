/**
 * eMterm E2E Tests
 *
 * 現在の問題をチェック:
 * 1. SSH接続時の描画
 * 2. Ctrl+D でウィンドウが閉じるか
 * 3. 基本的なターミナル操作
 */

describe("eMterm Terminal", () => {
	it("should display the terminal window", async () => {
		// ウィンドウが表示されるか
		const title = await browser.getTitle();
		console.log("Window title:", title);
		expect(title).toBeTruthy();

		// スクリーンショットを撮影
		await browser.saveScreenshot("./screenshots/01-initial.png");
	});

	it("should have terminal element", async () => {
		// ターミナル要素が存在するか
		const terminal = await $("#terminal");
		const isDisplayed = await terminal.isDisplayed();
		console.log("Terminal element displayed:", isDisplayed);
		expect(isDisplayed).toBe(true);

		await browser.saveScreenshot("./screenshots/02-terminal-element.png");
	});

	it("should accept keyboard input", async () => {
		// ターミナルにフォーカス
		const terminal = await $("#terminal");
		await terminal.click();

		// エコーコマンドを入力
		await browser.keys(["e", "c", "h", "o", " ", "H", "e", "l", "l", "o"]);
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/03-after-typing-echo.png");

		// Enter を押して実行
		await browser.keys(["Enter"]);
		await browser.pause(1000);
		await browser.saveScreenshot("./screenshots/04-after-enter.png");

		// 出力を確認
		const terminalText = await terminal.getText();
		console.log("Terminal text after echo:", terminalText.slice(0, 200));
	});

	it("should check console logs for events", async () => {
		// ブラウザのコンソールログを取得
		const logs = await browser.getLogs("browser");
		console.log("=== Console Logs ===");
		for (const log of logs) {
			console.log(`[${log.level}] ${log.message}`);
		}
	});

	it("should test Ctrl+D behavior", async () => {
		const terminal = await $("#terminal");
		await terminal.click();

		// 現在の状態をスクリーンショット
		await browser.saveScreenshot("./screenshots/05-before-ctrl-d.png");

		// Ctrl+D を送信
		console.log("Sending Ctrl+D...");
		await browser.keys(["Control", "d"]);

		// 少し待つ
		await browser.pause(2000);

		// ウィンドウがまだ開いているか確認
		try {
			const stillOpen = await browser.getTitle();
			console.log("Window still open after Ctrl+D, title:", stillOpen);
			await browser.saveScreenshot("./screenshots/06-after-ctrl-d.png");
		} catch (e) {
			console.log("Window closed after Ctrl+D (expected behavior)");
		}
	});
});

describe("eMterm SSH Test", () => {
	it("should test SSH-like alternate buffer behavior", async () => {
		const terminal = await $("#terminal");
		await terminal.click();

		// 代替バッファに切り替えるコマンド（例: less や vim）
		// less は代替バッファを使用する
		console.log("Testing alternate buffer with less command...");

		await browser.keys([
			"l",
			"e",
			"s",
			"s",
			" ",
			"/",
			"e",
			"t",
			"c",
			"/",
			"p",
			"a",
			"s",
			"s",
			"w",
			"d",
		]);
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/07-before-less.png");

		await browser.keys(["Enter"]);
		await browser.pause(2000);
		await browser.saveScreenshot("./screenshots/08-in-less.png");

		// less の内容を確認
		const terminalText = await terminal.getText();
		console.log("Terminal text in less:", terminalText.slice(0, 300));

		// q で less を終了
		await browser.keys(["q"]);
		await browser.pause(1000);
		await browser.saveScreenshot("./screenshots/09-after-less.png");
	});

	it("should get JavaScript console state", async () => {
		// JavaScript を実行してターミナル状態を取得
		const state = await browser.execute(() => {
			// グローバル変数からターミナル状態を取得
			const terminalState = window.terminalState;
			const ptyClient = window.ptyClient;

			return {
				terminalStateExists: !!terminalState,
				ptyClientExists: !!ptyClient,
				sessionId: ptyClient?.getSessionId?.() || null,
				isAlternateBuffer: terminalState?.isAlternateBuffer || false,
			};
		});

		console.log("Terminal state:", JSON.stringify(state, null, 2));
	});
});
