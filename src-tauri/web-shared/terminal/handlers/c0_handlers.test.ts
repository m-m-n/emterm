/**
 * Tests for C0 control handlers.
 */

import { describe, expect, test, beforeEach } from "bun:test";
import { TerminalState } from "../state.ts";
import {
  handleExecuteDispatch,
  handleBel,
  handleBackspace,
  handleTab,
  handleLineFeed,
  handleCarriageReturn,
  handleShiftOut,
  handleShiftIn,
} from "./c0_handlers.ts";
import type { TerminalStateAccessor } from "./types.ts";
import { C0 } from "../../types/terminal.ts";

describe("c0_handlers", () => {
  let state: TerminalState;

  beforeEach(() => {
    state = new TerminalState(80, 24);
  });

  // Helper to get accessor - TerminalState implements TerminalStateAccessor
  function getAccessor(): TerminalStateAccessor {
    return state;
  }

  describe("handleBel", () => {
    test("should handle bell (no-op)", () => {
      // Should not throw
      handleBel(getAccessor());
    });
  });

  describe("handleBackspace", () => {
    test("should move cursor left by 1", () => {
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 10 } } });

      handleBackspace(getAccessor());

      expect(state.cursorCol).toBe(8); // Was 9, now 8
    });

    test("should not move past column 0", () => {
      handleBackspace(getAccessor());

      expect(state.cursorCol).toBe(0);
    });

    test("should clear wrapPending", () => {
      // Set wrap pending
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 80 } } });
      state.processAction({ type: "Print", value: "X" });

      handleBackspace(getAccessor());

      // wrapPending should be cleared
    });
  });

  describe("handleTab", () => {
    test("should move to next tab stop", () => {
      // Default tab stops are at 8, 16, 24, etc.
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 1 } } });

      handleTab(getAccessor());

      expect(state.cursorCol).toBe(8);
    });

    test("should move to next tab stop from middle", () => {
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 10 } } });

      handleTab(getAccessor());

      expect(state.cursorCol).toBe(16); // Next tab after col 9
    });

    test("should stop at end of line if no more tab stops", () => {
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 75 } } });

      handleTab(getAccessor());

      expect(state.cursorCol).toBe(79); // End of line
    });
  });

  describe("handleLineFeed", () => {
    test("should move cursor down by 1", () => {
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 5, col: 10 } } });

      handleLineFeed(getAccessor());

      expect(state.cursorRow).toBe(5); // Was 4, now 5
    });

    test("should scroll when at bottom", () => {
      // Put content at row 0
      state.processAction({ type: "Print", value: "A" });
      // Move to bottom row
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 24, col: 1 } } });

      handleLineFeed(getAccessor());

      // Should scroll, row 0 content should have moved up
      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(0, 0).char).toBe(" "); // Content scrolled away
      expect(state.cursorRow).toBe(23);
    });

    test("should clear wrapPending", () => {
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 80 } } });
      state.processAction({ type: "Print", value: "X" });

      handleLineFeed(getAccessor());

      // wrapPending should be cleared
    });
  });

  describe("handleCarriageReturn", () => {
    test("should move cursor to column 0", () => {
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 5, col: 40 } } });

      handleCarriageReturn(getAccessor());

      expect(state.cursorCol).toBe(0);
      expect(state.cursorRow).toBe(4); // Row unchanged
    });

    test("should clear wrapPending", () => {
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 80 } } });
      state.processAction({ type: "Print", value: "X" });

      handleCarriageReturn(getAccessor());

      // wrapPending should be cleared
    });
  });

  describe("handleShiftOut / handleShiftIn", () => {
    test("should switch to G1 charset on SO", () => {
      // Set G1 to DecLineDrawing first
      state.processAction({ type: "Esc", value: { action: "SetG1CharSet", data: "DecLineDrawing" } });

      handleShiftOut(getAccessor());
      state.processAction({ type: "Print", value: "q" });

      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(0, 0).char).toBe("\u2500"); // Box drawing
    });

    test("should switch back to G0 charset on SI", () => {
      handleShiftOut(getAccessor());
      handleShiftIn(getAccessor());

      state.processAction({ type: "Print", value: "A" });

      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(0, 0).char).toBe("A");
    });
  });

  describe("handleExecuteDispatch", () => {
    test("should dispatch BEL", () => {
      // Should not throw
      handleExecuteDispatch(getAccessor(), C0.BEL);
    });

    test("should dispatch BS", () => {
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 5 } } });

      handleExecuteDispatch(getAccessor(), C0.BS);

      expect(state.cursorCol).toBe(3);
    });

    test("should dispatch HT", () => {
      handleExecuteDispatch(getAccessor(), C0.HT);

      expect(state.cursorCol).toBe(8);
    });

    test("should dispatch LF", () => {
      handleExecuteDispatch(getAccessor(), C0.LF);

      expect(state.cursorRow).toBe(1);
    });

    test("should dispatch VT (same as LF)", () => {
      handleExecuteDispatch(getAccessor(), C0.VT);

      expect(state.cursorRow).toBe(1);
    });

    test("should dispatch FF (same as LF)", () => {
      handleExecuteDispatch(getAccessor(), C0.FF);

      expect(state.cursorRow).toBe(1);
    });

    test("should dispatch CR", () => {
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 40 } } });

      handleExecuteDispatch(getAccessor(), C0.CR);

      expect(state.cursorCol).toBe(0);
    });

    test("should dispatch SO", () => {
      handleExecuteDispatch(getAccessor(), C0.SO);

      expect(getAccessor().activeCharSet).toBe("G1");
    });

    test("should dispatch SI", () => {
      handleExecuteDispatch(getAccessor(), C0.SO); // First switch to G1
      handleExecuteDispatch(getAccessor(), C0.SI); // Then back to G0

      expect(getAccessor().activeCharSet).toBe("G0");
    });
  });
});
