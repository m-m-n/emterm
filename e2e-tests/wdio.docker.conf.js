/**
 * WebdriverIO configuration for Docker environment
 * Uses full paths and longer timeouts for container environment
 */

import { spawn } from "child_process";
import { dirname, resolve } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const projectRoot = resolve(__dirname, "..");
const appPath = resolve(projectRoot, "src-tauri/target/debug/emterm");

// Full paths for Docker environment
const TAURI_DRIVER = "/root/.cargo/bin/tauri-driver";
const WEBKIT_DRIVER = "/usr/bin/WebKitWebDriver";

// tauri-driver process
let tauriDriver;

export const config = {
	specs: ["./specs/**/*.e2e.js"],
	exclude: [
		"./specs/*-debug.e2e.js",
		"./specs/*-diag.e2e.js",
		"./specs/*-capture.e2e.js",
		"./specs/benchmark.e2e.js",
	],
	maxInstances: 1,

	capabilities: [
		{
			"tauri:options": {
				application: appPath,
			},
		},
	],

	hostname: "localhost",
	port: 4444,
	path: "/",

	framework: "mocha",
	mochaOpts: {
		ui: "bdd",
		timeout: 180000, // 3 minutes for Docker
	},

	reporters: ["spec"],
	logLevel: "info",

	connectionRetryTimeout: 180000,
	connectionRetryCount: 5,

	onPrepare: async () => {
		// Build is done separately via: docker compose run --rm build
		console.log("Using pre-built binary at:", appPath);
	},

	beforeSession: async () => {
		console.log("Starting tauri-driver at", TAURI_DRIVER);

		tauriDriver = spawn(TAURI_DRIVER, ["--native-driver", WEBKIT_DRIVER], {
			stdio: ["ignore", "pipe", "pipe"],
			env: { ...process.env, DISPLAY: ":99" },
		});

		tauriDriver.stdout.on("data", (data) => {
			console.log(`[tauri-driver stdout] ${data}`);
		});

		tauriDriver.stderr.on("data", (data) => {
			console.error(`[tauri-driver stderr] ${data}`);
		});

		tauriDriver.on("error", (err) => {
			console.error("[tauri-driver] Failed to start:", err);
		});

		tauriDriver.on("exit", (code) => {
			console.log(`[tauri-driver] Exited with code ${code}`);
		});

		// Longer wait for Docker environment
		console.log("Waiting 5 seconds for tauri-driver to start...");
		await new Promise((resolve) => setTimeout(resolve, 5000));
		console.log("tauri-driver should be ready now");
	},

	before: async () => {
		// Wait for app initialization (WASM load, PTY spawn, DOM ready)
		console.log("Waiting for app to initialize...");
		try {
			await browser.waitUntil(
				async () => {
					return await browser.execute(() => {
						return (
							!!window.terminalState &&
							window.terminalState.cols > 0 &&
							!!window.tabManager &&
							window.tabManager.getTabs().length > 0
						);
					});
				},
				{
					timeout: 30000,
					interval: 500,
					timeoutMsg:
						"App did not initialize within 30s (terminalState/tabManager not ready)",
				},
			);
			console.log("App initialized successfully");
		} catch (e) {
			console.error("App initialization failed:", e.message);
			throw e;
		}
	},

	afterSession: async () => {
		if (tauriDriver) {
			console.log("Stopping tauri-driver...");
			tauriDriver.kill();
			tauriDriver = null;
		}
	},
};
