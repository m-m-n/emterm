import { spawn, spawnSync } from "child_process";
import { dirname, resolve } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const projectRoot = resolve(__dirname, "..");
const appPath = resolve(projectRoot, "src-tauri/target/debug/emterm");

// tauri-driver プロセス
let tauriDriver;

export const config = {
	// テストファイル
	specs: ["./specs/**/*.e2e.js"],

	// 並列実行なし（1つずつ）
	maxInstances: 1,

	// Tauri WebView capabilities (Tauri 2.x format)
	capabilities: [
		{
			"tauri:options": {
				application: appPath,
			},
		},
	],

	// tauri-driver への接続
	hostname: "localhost",
	port: 4444,
	path: "/",

	// テストフレームワーク
	framework: "mocha",
	mochaOpts: {
		ui: "bdd",
		timeout: 120000, // 2分（アプリ起動を考慮）
	},

	// レポーター
	reporters: ["spec"],

	// ログレベル
	logLevel: "info",

	// 接続リトライ
	connectionRetryTimeout: 120000,
	connectionRetryCount: 3,

	// テスト前にアプリをビルド
	onPrepare: async () => {
		console.log("Building Tauri application...");
		const build = spawnSync(
			"bun",
			["tauri", "build", "--debug", "--no-bundle"],
			{
				cwd: projectRoot,
				stdio: "inherit",
			},
		);

		if (build.status !== 0) {
			throw new Error(`Build failed with code ${build.status}`);
		}
		console.log("Build completed.");
	},

	// セッション開始前に tauri-driver を起動
	beforeSession: async () => {
		console.log("Starting tauri-driver...");
		tauriDriver = spawn(
			"tauri-driver",
			["--native-driver", "/usr/bin/WebKitWebDriver"],
			{
				stdio: ["ignore", "pipe", "pipe"],
			},
		);

		tauriDriver.stdout.on("data", (data) => {
			console.log(`[tauri-driver] ${data}`);
		});

		tauriDriver.stderr.on("data", (data) => {
			console.error(`[tauri-driver] ${data}`);
		});

		// tauri-driver の起動を待つ
		await new Promise((resolve) => setTimeout(resolve, 3000));
	},

	// セッション終了後に tauri-driver を停止
	afterSession: async () => {
		if (tauriDriver) {
			console.log("Stopping tauri-driver...");
			tauriDriver.kill();
			tauriDriver = null;
		}
	},
};
