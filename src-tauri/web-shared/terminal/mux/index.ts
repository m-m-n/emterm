/**
 * Mux multiplexer client module.
 *
 * Manages the GUI-side state for multiplexer mode:
 * - IPC connection to daemon via Tauri bridge commands
 * - Per-pane Canvas + WASM instance lifecycle
 * - Mode switching (normal ↔ mux)
 * - Snapshot/restore for detach/reattach
 */

export { MuxClient, type MuxConnectionState } from "./mux-client";
export { MuxTabGroup, type MuxTabGroupState } from "./tab-group";
export { validateSocketPath, parseMuxOsc } from "./mux-client";
