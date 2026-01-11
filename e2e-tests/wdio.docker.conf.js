/**
 * WebdriverIO configuration for Docker environment
 * Uses full paths and longer timeouts for container environment
 */

import { spawn, spawnSync } from "child_process";
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
		console.log("Building Tauri application...");
		const build = spawnSync(
			"bun",
			["tauri", "build", "--debug", "--no-bundle"],
			{
				cwd: projectRoot,
				stdio: "inherit",
				env: {
					...process.env,
					PATH: `/root/.cargo/bin:/root/.bun/bin:${process.env.PATH}`,
				},
			},
		);

		if (build.status !== 0) {
			throw new Error(`Build failed with code ${build.status}`);
		}
		console.log("Build completed.");
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

	afterSession: async () => {
		if (tauriDriver) {
			console.log("Stopping tauri-driver...");
			tauriDriver.kill();
			tauriDriver = null;
		}
	},
};
