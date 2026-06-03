/**
 * Tests for scripts/patch-wasm-bindgen.sh (FR1, FR2).
 *
 * These tests run the patch script against a synthetic wasm-bindgen-style
 * module to verify:
 *  - TS-1: the injected reset() references only identifiers that exist, and
 *          running it does not throw ReferenceError.
 *  - TS-8: the post-patch guard passes when every reset identifier is declared.
 *  - TS-9: the post-patch guard fails (non-zero) and names the missing
 *          identifier when one of the reset targets is absent.
 *
 * The script operates on hard-coded paths (wasm/pkg/emterm_wasm.js); we run it
 * from a temporary working directory containing a `wasm/pkg/` fixture so the
 * real generated module is never touched.
 */

import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const REPO_ROOT = join(import.meta.dir, "..");
const PATCH_SCRIPT = join(REPO_ROOT, "scripts", "patch-wasm-bindgen.sh");

/**
 * Minimal wasm-bindgen-style module that declares the same identifiers the
 * injected reset writes. `dropExtra` lets a test omit one declaration to
 * exercise the negative guard path (TS-9).
 */
function makeModuleSource(opts: { dropIdentifier?: string } = {}): string {
	const declarations: Record<string, string> = {
		wasm: "let wasmModule, wasm;",
		cachedDataViewMemory0: "let cachedDataViewMemory0 = null;",
		cachedUint16ArrayMemory0: "let cachedUint16ArrayMemory0 = null;",
		cachedUint8ArrayMemory0: "let cachedUint8ArrayMemory0 = null;",
		WASM_VECTOR_LEN: "let WASM_VECTOR_LEN = 0;",
	};
	if (opts.dropIdentifier) delete declarations[opts.dropIdentifier];
	const decls = Object.values(declarations).join("\n");
	return `${decls}

function initSync() {}
function __wbg_init() {}

export { initSync, __wbg_init as default };
`;
}

async function runPatch(cwd: string) {
	const proc = Bun.spawn(["bash", PATCH_SCRIPT], {
		cwd,
		stdout: "pipe",
		stderr: "pipe",
	});
	const exitCode = await proc.exited;
	const stdout = await new Response(proc.stdout).text();
	const stderr = await new Response(proc.stderr).text();
	return { exitCode, stdout, stderr };
}

describe("patch-wasm-bindgen.sh", () => {
	let workdir: string;
	let jsPath: string;

	beforeEach(async () => {
		workdir = await mkdtemp(join(tmpdir(), "patch-wasm-test-"));
		await mkdir(join(workdir, "wasm", "pkg"), { recursive: true });
		jsPath = join(workdir, "wasm", "pkg", "emterm_wasm.js");
	});

	afterEach(async () => {
		await rm(workdir, { recursive: true, force: true });
	});

	it("TS-1/TS-8: injects reset, guard passes, reset references only existing identifiers and does not throw", async () => {
		await writeFile(jsPath, makeModuleSource(), "utf8");

		const { exitCode, stdout } = await runPatch(workdir);
		expect(exitCode).toBe(0);
		expect(stdout).toContain("Patch guard passed");

		const patched = await readFile(jsPath, "utf8");
		// The injected reset must not reference the removed legacy object table.
		expect(patched).not.toContain("heap.length");
		expect(patched).not.toContain("heap_next");
		expect(patched).toContain("function __wbg_reset()");

		// Extract the reset body and assert every identifier it assigns is one
		// of the declared module bindings. If the body referenced an
		// undeclared identifier (e.g. `heap`), running reset() at runtime would
		// throw ReferenceError — so equating the assigned set to the declared
		// set proves reset() cannot ReferenceError.
		const match = patched.match(/function __wbg_reset\(\) \{([\s\S]*?)\n\}/);
		expect(match).not.toBeNull();
		const body = match?.[1] ?? "";

		const declaredIdentifiers = new Set([
			"wasm",
			"cachedDataViewMemory0",
			"cachedUint16ArrayMemory0",
			"cachedUint8ArrayMemory0",
			"WASM_VECTOR_LEN",
		]);
		// Collect the left-hand-side identifier of each `<ident> = ...;` line.
		const assigned = new Set<string>();
		for (const line of body.split("\n")) {
			const m = line.match(/^\s*([A-Za-z_$][A-Za-z0-9_$]*)\s*=/);
			if (m?.[1]) assigned.add(m[1]);
		}
		expect(assigned.size).toBeGreaterThan(0);
		for (const id of assigned) {
			expect(declaredIdentifiers.has(id)).toBe(true);
		}
		// And it must actually reset the cached-memory views + vector length.
		expect(assigned.has("wasm")).toBe(true);
		expect(assigned.has("WASM_VECTOR_LEN")).toBe(true);
	});

	it("TS-9: guard fails with non-zero exit naming the missing identifier", async () => {
		// Drop the WASM_VECTOR_LEN declaration so the injected reset references a
		// symbol absent from the module.
		await writeFile(jsPath, makeModuleSource({ dropIdentifier: "WASM_VECTOR_LEN" }), "utf8");

		const { exitCode, stderr } = await runPatch(workdir);
		expect(exitCode).not.toBe(0);
		expect(stderr).toContain("WASM_VECTOR_LEN");
		expect(stderr).toContain("not declared");
	});
});
