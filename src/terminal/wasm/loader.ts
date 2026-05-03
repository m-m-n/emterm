/**
 * WASM module loader.
 *
 * Loads and initializes the emterm WASM module.
 * Must be called before any WASM-backed functions are used.
 */

import init, { reset as wasmReset, type InitOutput } from "../../../wasm/pkg/emterm_wasm.js";

let initialized = false;
let initPromise: Promise<void> | null = null;
let reinitPromise: Promise<void> | null = null;
/** Captured InitOutput from the most recent successful init(). wasm-bindgen
 *  exposes `memory: WebAssembly.Memory` only on this module-level object —
 *  individual TerminalCore instances do NOT carry a `memory` field. The
 *  diagnostic heartbeat reads heap size via getWasmMemoryBytes() below. */
let wasmInitOutput: InitOutput | null = null;

/**
 * Initialize the WASM module.
 *
 * Safe to call multiple times; concurrent calls share the same promise.
 * Must complete before any terminal processing begins.
 */
export async function initWasm(): Promise<void> {
	if (initialized) return;
	if (initPromise) return initPromise;
	initPromise = init().then((out) => { wasmInitOutput = out; initialized = true; });
	try {
		await initPromise;
	} finally {
		initPromise = null;
	}
}

/** Current size of the WASM linear-memory ArrayBuffer in bytes, or -1 if WASM
 *  has not been initialized. Used by the diagnostic heartbeat. WebAssembly
 *  memory only ever grows, so a rising value across heartbeats indicates
 *  either legitimate growth or a leak in the parser/grid. */
export function getWasmMemoryBytes(): number {
	return wasmInitOutput?.memory?.buffer.byteLength ?? -1;
}

/**
 * Force re-initialization of the WASM module.
 *
 * Used when the WASM engine itself is corrupted (e.g., after long idle
 * causing linear memory corruption). Resets the wasm-bindgen internal
 * singleton so that init() creates a fresh WebAssembly.Instance with
 * new linear memory. Concurrent calls are deduplicated.
 */
export async function reinitWasm(): Promise<void> {
	if (reinitPromise) return reinitPromise;
	reinitPromise = (async () => {
		initialized = false;
		// Destructive: clears the wasm-bindgen singleton and heap.
		// If init() fails after this, WASM is left unusable (no rollback).
		wasmReset();
		wasmInitOutput = await init();
		initialized = true;
	})();
	try {
		await reinitPromise;
	} finally {
		reinitPromise = null;
	}
}
