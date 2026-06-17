/**
 * Tests for CSI mode handlers.
 */

import { describe, expect, test, beforeEach } from "bun:test";
import { TerminalState } from "../state.ts";
import { handleSetMode } from "./csi_modes.ts";
import type { TerminalStateAccessor } from "./types.ts";

describe("csi_modes handlers", () => {
  let state: TerminalState;

  beforeEach(() => {
    state = new TerminalState(80, 24);
  });

  // Helper to get accessor - TerminalState implements TerminalStateAccessor
  function getAccessor(): TerminalStateAccessor {
    return state;
  }

  describe("handleSetMode", () => {
    test("should enable cursor visibility (mode 25)", () => {
      // First disable it
      handleSetMode(getAccessor(), [25], false);
      expect(state.cursorVisible).toBe(false);

      // Then enable it
      handleSetMode(getAccessor(), [25], true);
      expect(state.cursorVisible).toBe(true);
    });

    test("should disable auto wrap (mode 7)", () => {
      // Auto wrap is enabled by default
      expect(state.getModes().autoWrap).toBe(true);

      handleSetMode(getAccessor(), [7], false);
      expect(state.getModes().autoWrap).toBe(false);
    });

    test("should switch to alternate buffer (mode 1049)", () => {
      expect(state.isAlternateBuffer).toBe(false);

      handleSetMode(getAccessor(), [1049], true);
      expect(state.isAlternateBuffer).toBe(true);
    });

    test("should switch back to primary buffer (mode 1049 reset)", () => {
      handleSetMode(getAccessor(), [1049], true);
      expect(state.isAlternateBuffer).toBe(true);

      handleSetMode(getAccessor(), [1049], false);
      expect(state.isAlternateBuffer).toBe(false);
    });

    test("should save cursor when switching to alt buffer with mode 1049", () => {
      // Position cursor at specific location
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 10, col: 20 } } });
      expect(state.cursorRow).toBe(9);
      expect(state.cursorCol).toBe(19);

      // Switch to alt buffer (saves cursor)
      handleSetMode(getAccessor(), [1049], true);

      // Alt buffer starts at 0,0
      // Note: behavior depends on implementation

      // Switch back (restores cursor)
      handleSetMode(getAccessor(), [1049], false);
      expect(state.cursorRow).toBe(9);
      expect(state.cursorCol).toBe(19);
    });

    test("should enable mouse tracking (mode 1000)", () => {
      handleSetMode(getAccessor(), [1000], true);
      expect(state.getModes().mouseTracking).toBe("x10");
    });

    test("should enable SGR mouse encoding (mode 1006)", () => {
      handleSetMode(getAccessor(), [1006], true);
      expect(state.getModes().mouseEncoding).toBe("sgr");
    });

    test("should enable bracketed paste (mode 2004)", () => {
      handleSetMode(getAccessor(), [2004], true);
      expect(state.getModes().bracketedPaste).toBe(true);
    });

    test("should handle multiple modes in one call", () => {
      handleSetMode(getAccessor(), [1000, 1006], true);
      expect(state.getModes().mouseTracking).toBe("x10");
      expect(state.getModes().mouseEncoding).toBe("sgr");
    });
  });
});
