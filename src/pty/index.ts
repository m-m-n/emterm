/**
 * PTY module - Terminal pseudo-terminal functionality.
 *
 * This module provides everything needed to interact with PTY sessions
 * from the frontend.
 */

export { PtyClient } from "./client";
export {
	HEALTH_CHECK_MS,
	HIDE_DEBOUNCE_MS,
	VisibilityController,
} from "./visibility-controller";
export { type KeyMapping, keyEventToBytes, shouldHandleKey } from "./keyboard";
export {
	type CharacterSize,
	calculateTerminalSize,
	measureCharacterSize,
	observeContainerResize,
	type TerminalSize,
} from "./size";
