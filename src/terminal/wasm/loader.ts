/**
 * WASM module loader.
 *
 * Loads and initializes the emterm WASM module.
 * Must be called before any WASM-backed functions are used.
 */

import init, { reset as wasmReset } from "../../../wasm/pkg/emterm_wasm.js";

let initialized = false;
let initPromise: Promise<void> | null = null;
let reinitPromise: Promise<void> | null = null;

/**
 * Initialize the WASM module.
 *
 * Safe to call multiple times; concurrent calls share the same promise.
 * Must complete before any terminal processing begins.
 */
export async function initWasm(): Promise<void> {
	if (initialized) return;
	if (initPromise) return initPromise;
	initPromise = init().then(() => { initialized = true; });
	try {
		await initPromise;
	} finally {
		initPromise = null;
	}
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
		await init();
		initialized = true;
	})();
	try {
		await reinitPromise;
	} finally {
		reinitPromise = null;
	}
}
