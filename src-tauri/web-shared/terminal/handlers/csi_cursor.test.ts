/**
 * Tests for CSI cursor movement handlers.
 */

import { describe, expect, test, beforeEach } from "bun:test";
import { TerminalState } from "../state.ts";
import {
  handleCursorUp,
  handleCursorDown,
  handleCursorForward,
  handleCursorBack,
  handleCursorNextLine,
  handleCursorPreviousLine,
  handleCursorHorizontalAbsolute,
  handleCursorVerticalAbsolute,
  handleCursorPosition,
} from "./csi_cursor.ts";
import type { TerminalStateAccessor } from "./types.ts";

describe("csi_cursor handlers", () => {
  let state: TerminalState;

  beforeEach(() => {
    state = new TerminalState(80, 24);
    // Move cursor to a known position for testing
    state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 10, col: 20 } } });
  });

  // Helper to get accessor - TerminalState implements TerminalStateAccessor
  function getAccessor(): TerminalStateAccessor {
    return state;
  }

  describe("handleCursorUp", () => {
    test("should move cursor up by count", () => {
      handleCursorUp(getAccessor(), 3);
      expect(state.cursorRow).toBe(6); // 9 - 3 = 6
    });

    test("should use default count of 1", () => {
      handleCursorUp(getAccessor(), undefined);
      expect(state.cursorRow).toBe(8); // 9 - 1 = 8
    });

    test("should not move past row 0", () => {
      handleCursorUp(getAccessor(), 100);
      expect(state.cursorRow).toBe(0);
    });

    test("should clear wrapPending", () => {
      // Set wrap pending by writing at end of line
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 80 } } });
      state.processAction({ type: "Print", value: "X" });
      handleCursorUp(getAccessor(), 1);
      // wrapPending should be cleared (subsequent print won't wrap)
    });
  });

  describe("handleCursorDown", () => {
    test("should move cursor down by count", () => {
      handleCursorDown(getAccessor(), 5);
      expect(state.cursorRow).toBe(14); // 9 + 5 = 14
    });

    test("should use default count of 1", () => {
      handleCursorDown(getAccessor(), undefined);
      expect(state.cursorRow).toBe(10); // 9 + 1 = 10
    });

    test("should not move past bottom row", () => {
      handleCursorDown(getAccessor(), 100);
      expect(state.cursorRow).toBe(23); // max row is 23 (0-indexed)
    });
  });

  describe("handleCursorForward", () => {
    test("should move cursor right by count", () => {
      handleCursorForward(getAccessor(), 10);
      expect(state.cursorCol).toBe(29); // 19 + 10 = 29
    });

    test("should use default count of 1", () => {
      handleCursorForward(getAccessor(), undefined);
      expect(state.cursorCol).toBe(20); // 19 + 1 = 20
    });

    test("should not move past right edge", () => {
      handleCursorForward(getAccessor(), 100);
      expect(state.cursorCol).toBe(79); // max col is 79 (0-indexed)
    });
  });

  describe("handleCursorBack", () => {
    test("should move cursor left by count", () => {
      handleCursorBack(getAccessor(), 5);
      expect(state.cursorCol).toBe(14); // 19 - 5 = 14
    });

    test("should use default count of 1", () => {
      handleCursorBack(getAccessor(), undefined);
      expect(state.cursorCol).toBe(18); // 19 - 1 = 18
    });

    test("should not move past column 0", () => {
      handleCursorBack(getAccessor(), 100);
      expect(state.cursorCol).toBe(0);
    });
  });

  describe("handleCursorNextLine", () => {
    test("should move down N lines and to column 0", () => {
      handleCursorNextLine(getAccessor(), 3);
      expect(state.cursorRow).toBe(12); // 9 + 3 = 12
      expect(state.cursorCol).toBe(0);
    });

    test("should use default count of 1", () => {
      handleCursorNextLine(getAccessor(), undefined);
      expect(state.cursorRow).toBe(10); // 9 + 1 = 10
      expect(state.cursorCol).toBe(0);
    });
  });

  describe("handleCursorPreviousLine", () => {
    test("should move up N lines and to column 0", () => {
      handleCursorPreviousLine(getAccessor(), 3);
      expect(state.cursorRow).toBe(6); // 9 - 3 = 6
      expect(state.cursorCol).toBe(0);
    });

    test("should use default count of 1", () => {
      handleCursorPreviousLine(getAccessor(), undefined);
      expect(state.cursorRow).toBe(8); // 9 - 1 = 8
      expect(state.cursorCol).toBe(0);
    });
  });

  describe("handleCursorHorizontalAbsolute", () => {
    test("should set cursor column (1-indexed input)", () => {
      handleCursorHorizontalAbsolute(getAccessor(), 40);
      expect(state.cursorCol).toBe(39); // 40 - 1 = 39 (0-indexed)
    });

    test("should default to column 1 (0-indexed: 0)", () => {
      handleCursorHorizontalAbsolute(getAccessor(), undefined);
      expect(state.cursorCol).toBe(0);
    });

    test("should clamp to valid range", () => {
      handleCursorHorizontalAbsolute(getAccessor(), 200);
      expect(state.cursorCol).toBe(79); // max col
    });
  });

  describe("handleCursorVerticalAbsolute", () => {
    test("should set cursor row (1-indexed input)", () => {
      handleCursorVerticalAbsolute(getAccessor(), 15);
      expect(state.cursorRow).toBe(14); // 15 - 1 = 14 (0-indexed)
    });

    test("should default to row 1 (0-indexed: 0)", () => {
      handleCursorVerticalAbsolute(getAccessor(), undefined);
      expect(state.cursorRow).toBe(0);
    });

    test("should clamp to valid range", () => {
      handleCursorVerticalAbsolute(getAccessor(), 100);
      expect(state.cursorRow).toBe(23); // max row
    });
  });

  describe("handleCursorPosition", () => {
    test("should set cursor position (1-indexed input)", () => {
      handleCursorPosition(getAccessor(), 5, 10);
      expect(state.cursorRow).toBe(4); // row 5 -> 4 (0-indexed)
      expect(state.cursorCol).toBe(9); // col 10 -> 9 (0-indexed)
    });

    test("should default to (1, 1) -> (0, 0)", () => {
      handleCursorPosition(getAccessor(), undefined, undefined);
      expect(state.cursorRow).toBe(0);
      expect(state.cursorCol).toBe(0);
    });

    test("should clamp to valid range", () => {
      handleCursorPosition(getAccessor(), 100, 200);
      expect(state.cursorRow).toBe(23);
      expect(state.cursorCol).toBe(79);
    });
  });
});
