/**
 * WASM module loader.
 *
 * Loads and initializes the emterm WASM module.
 * Must be called before any WASM-backed functions are used.
 */

import init from "../../../wasm/pkg/emterm_wasm.js";

let initialized = false;

/**
 * Initialize the WASM module.
 *
 * Safe to call multiple times; subsequent calls are no-ops.
 * Must complete before any terminal processing begins.
 */
export async function initWasm(): Promise<void> {
	if (initialized) return;
	await init();
	initialized = true;
}
