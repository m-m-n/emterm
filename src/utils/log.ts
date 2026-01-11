/**
 * Debug logging utility - outputs to backend stderr.
 */
import { invoke } from "@tauri-apps/api/core";

export function log(message: string): void {
	invoke("debug_log", { message }).catch(() => {});
	console.log(message);
}
