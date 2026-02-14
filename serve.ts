// Development server for eMterm frontend

import { resolve } from "node:path";

const SRC_ROOT = resolve("./src");

// Build the main bundle with dependencies resolved
async function buildBundle() {
	const result = await Bun.build({
		entrypoints: ["./src/main.ts"],
		outdir: "./dist",
		target: "browser",
		format: "esm",
		minify: false,
		sourcemap: "inline",
	});

	if (!result.success) {
		console.error("Build failed:", result.logs);
		throw new Error("Build failed");
	}

	return result;
}

// Initial build
await buildBundle();

const server = Bun.serve({
	port: 5173,
	async fetch(req) {
		const url = new URL(req.url);
		let path = url.pathname;

		if (path === "/") {
			path = "/index.html";
		}

		// Serve built bundle - always rebuild in dev mode
		if (path === "/main.js") {
			await buildBundle();
			const file = Bun.file("./dist/main.js");
			if (await file.exists()) {
				return new Response(file, {
					headers: {
						"Content-Type": "application/javascript",
						"Cache-Control": "no-cache, no-store, must-revalidate",
					},
				});
			}
		}

		// Serve WASM files from wasm/pkg/ (allowlist to prevent path traversal)
		if (path.endsWith(".wasm")) {
			const wasmName = path.split("/").pop()!;
			if (wasmName === "emterm_wasm_bg.wasm") {
				const wasmFile = Bun.file(`./wasm/pkg/${wasmName}`);
				if (await wasmFile.exists()) {
					return new Response(wasmFile, {
						headers: {
							"Content-Type": "application/wasm",
							"Cache-Control": "no-cache, no-store, must-revalidate",
						},
					});
				}
			}
		}

		// Serve static files from src (with path traversal protection)
		const filePath = resolve(`./src${path}`);
		if (!filePath.startsWith(SRC_ROOT)) {
			return new Response("Forbidden", { status: 403 });
		}
		const file = Bun.file(filePath);

		if (await file.exists()) {
			const contentType = getContentType(path);

			// For TypeScript files, redirect to the bundle
			if (path.endsWith(".ts")) {
				// Rebuild on TS file request (hot reload)
				await buildBundle();
				const bundleFile = Bun.file("./dist/main.js");
				return new Response(bundleFile, {
					headers: {
						"Content-Type": "application/javascript",
						"Cache-Control": "no-cache, no-store, must-revalidate",
					},
				});
			}

			return new Response(file, {
				headers: {
					"Content-Type": contentType,
					"Cache-Control": "no-cache, no-store, must-revalidate",
				},
			});
		}

		return new Response("Not Found", { status: 404 });
	},
});

function getContentType(path: string): string {
	if (path.endsWith(".html")) return "text/html";
	if (path.endsWith(".css")) return "text/css";
	if (path.endsWith(".js")) return "application/javascript";
	if (path.endsWith(".ts")) return "application/javascript";
	if (path.endsWith(".json")) return "application/json";
	if (path.endsWith(".wasm")) return "application/wasm";
	return "application/octet-stream";
}

console.log(`Development server running at http://localhost:${server.port}`);
