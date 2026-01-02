/**
 * PTY module - Terminal pseudo-terminal functionality.
 *
 * This module provides everything needed to interact with PTY sessions
 * from the frontend.
 */

export { PtyClient } from "./client";
export { keyEventToBytes, shouldHandleKey, type KeyMapping } from "./keyboard";
export {
  calculateTerminalSize,
  measureCharacterSize,
  observeContainerResize,
  type TerminalSize,
  type CharacterSize,
} from "./size";
