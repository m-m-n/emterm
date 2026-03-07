/**
 * SSH E2E Test
 */

/**
 * Type text character by character with delay to avoid WebDriver key input issues
 */
async function typeSlowly(text, delay = 50) {
	for (const char of text) {
		await browser.keys(char);
		await browser.pause(delay);
	}
}

describe("SSH Connection Test", () => {
	it("should connect to ssh laser5.net and display output", async () => {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.click();

		// 初期状態をスクリーンショット
		await browser.saveScreenshot("./screenshots/ssh-01-initial.png");

		// ssh laser5.net を入力（一文字ずつ）
		console.log("Typing ssh laser5.net (character by character)...");
		await typeSlowly("ssh laser5.net", 100);
		await browser.pause(500);
		await browser.saveScreenshot("./screenshots/ssh-02-typed.png");

		// Enter を押して実行
		console.log("Pressing Enter...");
		await browser.keys("Enter");

		// SSH接続を待つ（5秒）
		console.log("Waiting for SSH connection...");
		await browser.pause(5000);
		await browser.saveScreenshot("./screenshots/ssh-03-connecting.png");

		// さらに待つ（認証など）
		await browser.pause(5000);
		await browser.saveScreenshot("./screenshots/ssh-04-connected.png");

		// ターミナルの内容を取得
		const terminalText = await terminal.getText();
		console.log("=== Terminal content after SSH ===");
		console.log(terminalText);
		console.log("=== End of terminal content ===");

		// JavaScript状態を取得
		const state = await browser.execute(() => {
			const terminalState = window.terminalState;
			const ptyClient = window.ptyClient;

			return {
				terminalStateExists: !!terminalState,
				ptyClientExists: !!ptyClient,
				sessionId: ptyClient?.getSessionId?.() || null,
				isAlternateBuffer: terminalState?.isAlternateBuffer || false,
				cols: terminalState?.cols || 0,
				rows: terminalState?.rows || 0,
			};
		});
		console.log("Terminal state:", JSON.stringify(state, null, 2));

		// exitで終了
		console.log("Sending exit command...");
		await typeSlowly("exit", 100);
		await browser.pause(500);
		await browser.keys("Enter");
		await browser.pause(2000);
		await browser.saveScreenshot("./screenshots/ssh-05-after-exit.png");
	});
});
