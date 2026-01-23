/**
 * Tests for ESC sequence handlers.
 */

import { describe, expect, test, beforeEach } from "bun:test";
import { TerminalState } from "../state.ts";
import {
  handleEscDispatch,
  handleSaveCursor,
  handleRestoreCursor,
  handleIndex,
  handleNextLine,
  handleReverseIndex,
  handleHorizontalTabSet,
  handleResetToInitialState,
  handleSetG0CharSet,
  handleSetG1CharSet,
} from "./esc_handlers.ts";
import type { TerminalStateAccessor } from "./types.ts";

describe("esc_handlers", () => {
  let state: TerminalState;

  beforeEach(() => {
    state = new TerminalState(80, 24);
  });

  // Helper to get accessor - TerminalState implements TerminalStateAccessor
  function getAccessor(): TerminalStateAccessor {
    return state;
  }

  describe("handleSaveCursor / handleRestoreCursor", () => {
    test("should save and restore cursor position", () => {
      // Move to specific position
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 10, col: 20 } } });
      expect(state.cursorRow).toBe(9);
      expect(state.cursorCol).toBe(19);

      // Save cursor
      handleSaveCursor(getAccessor());

      // Move to different position
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 1 } } });
      expect(state.cursorRow).toBe(0);
      expect(state.cursorCol).toBe(0);

      // Restore cursor
      handleRestoreCursor(getAccessor());
      expect(state.cursorRow).toBe(9);
      expect(state.cursorCol).toBe(19);
    });

    test("should clear wrapPending on restore", () => {
      // Set wrap pending
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 80 } } });
      state.processAction({ type: "Print", value: "X" });

      handleSaveCursor(getAccessor());
      handleRestoreCursor(getAccessor());

      // wrapPending should be cleared
    });
  });

  describe("handleIndex", () => {
    test("should move cursor down within screen", () => {
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 5, col: 10 } } });

      handleIndex(getAccessor());

      expect(state.cursorRow).toBe(5); // Was 4, now 5
    });

    test("should scroll buffer when at bottom", () => {
      // Move to last row
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 24, col: 1 } } });
      // Put some content
      state.processAction({ type: "Print", value: "A" });
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 1 } } });
      state.processAction({ type: "Print", value: "B" });
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 24, col: 1 } } });

      handleIndex(getAccessor());

      // Content should scroll, cursor stays at bottom
      expect(state.cursorRow).toBe(23);
    });
  });

  describe("handleNextLine", () => {
    test("should move to column 0 of next line", () => {
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 5, col: 40 } } });

      handleNextLine(getAccessor());

      expect(state.cursorRow).toBe(5); // Was 4, now 5
      expect(state.cursorCol).toBe(0);
    });
  });

  describe("handleReverseIndex", () => {
    test("should move cursor up within screen", () => {
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 5, col: 10 } } });

      handleReverseIndex(getAccessor());

      expect(state.cursorRow).toBe(3); // Was 4, now 3
    });

    test("should scroll down when at top", () => {
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 1 } } });
      state.processAction({ type: "Print", value: "A" });

      handleReverseIndex(getAccessor());

      // Should scroll down, cursor stays at 0
      expect(state.cursorRow).toBe(0);
      // Content at row 0 should have moved to row 1
      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(0, 1).char).toBe("A");
    });
  });

  describe("handleHorizontalTabSet", () => {
    test("should set tab stop at current column", () => {
      // Move to column 15
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 16 } } });

      handleHorizontalTabSet(getAccessor());

      // Tab from column 10 should go to column 15
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 11 } } });
      state.processAction({ type: "Execute", value: 0x09 }); // HT

      expect(state.cursorCol).toBe(15);
    });
  });

  describe("handleResetToInitialState", () => {
    test("should reset terminal to initial state", () => {
      // Make some changes
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 10, col: 20 } } });
      state.processAction({ type: "Csi", value: { action: "SetMode", data: [1049] } }); // Alt buffer
      state.processAction({ type: "Print", value: "A" });

      handleResetToInitialState(getAccessor());

      expect(state.cursorRow).toBe(0);
      expect(state.cursorCol).toBe(0);
      expect(state.isAlternateBuffer).toBe(false);
    });
  });

  describe("handleSetG0CharSet / handleSetG1CharSet", () => {
    test("should set G0 charset to DecLineDrawing", () => {
      handleSetG0CharSet(getAccessor(), "DecLineDrawing");

      // Print a line drawing character
      state.processAction({ type: "Print", value: "q" }); // Should become horizontal line

      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(0, 0).char).toBe("\u2500"); // Box drawing horizontal
    });

    test("should set G1 charset", () => {
      handleSetG1CharSet(getAccessor(), "DecLineDrawing");

      // Need to shift to G1 first (SO)
      state.processAction({ type: "Execute", value: 0x0e }); // SO
      state.processAction({ type: "Print", value: "q" });

      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(0, 0).char).toBe("\u2500");
    });
  });

  describe("handleEscDispatch", () => {
    test("should dispatch SaveCursor action", () => {
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 5, col: 10 } } });

      handleEscDispatch(getAccessor(), { action: "SaveCursor" });
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 1 } } });
      handleEscDispatch(getAccessor(), { action: "RestoreCursor" });

      expect(state.cursorRow).toBe(4);
      expect(state.cursorCol).toBe(9);
    });

    test("should handle Unknown action gracefully", () => {
      // Should not throw
      handleEscDispatch(getAccessor(), { action: "Unknown", data: 0x42 });
    });
  });
});
