/**
 * Tests for print handler.
 */

import { describe, expect, test, beforeEach } from "bun:test";
import { TerminalState } from "../state.ts";
import {
  handlePrintDispatch,
  translateCharacter,
  translateLineDrawing,
} from "./print_handler.ts";
import type { TerminalStateAccessor } from "./types.ts";

describe("print_handler", () => {
  let state: TerminalState;

  beforeEach(() => {
    state = new TerminalState(80, 24);
  });

  // Helper to get accessor - TerminalState implements TerminalStateAccessor
  function getAccessor(): TerminalStateAccessor {
    return state;
  }

  describe("handlePrintDispatch", () => {
    test("should print ASCII character at cursor", () => {
      handlePrintDispatch(getAccessor(), "A");

      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(0, 0).char).toBe("A");
      expect(state.cursorCol).toBe(1);
    });

    test("should advance cursor after printing", () => {
      handlePrintDispatch(getAccessor(), "A");
      handlePrintDispatch(getAccessor(), "B");
      handlePrintDispatch(getAccessor(), "C");

      expect(state.cursorCol).toBe(3);
      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(0, 0).char).toBe("A");
      expect(buffer.getCell(1, 0).char).toBe("B");
      expect(buffer.getCell(2, 0).char).toBe("C");
    });

    test("should handle wide character (CJK)", () => {
      handlePrintDispatch(getAccessor(), "\u4e2d"); // Chinese character (width 2)

      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(0, 0).char).toBe("\u4e2d");
      expect(buffer.getCell(0, 0).width).toBe(2);
      // Placeholder at next cell
      expect(buffer.getCell(1, 0).width).toBe(0);
      // Cursor advances by 2
      expect(state.cursorCol).toBe(2);
    });

    test("should wrap at end of line with autoWrap", () => {
      // Position near end of line
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 80 } } });

      handlePrintDispatch(getAccessor(), "X");
      handlePrintDispatch(getAccessor(), "Y");

      // Y should be on next line
      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(79, 0).char).toBe("X");
      expect(buffer.getCell(0, 1).char).toBe("Y");
    });

    test("should not wrap at end of line without autoWrap", () => {
      // Disable auto wrap
      state.processAction({ type: "Csi", value: { action: "ResetMode", data: [7] } });
      // Position at end of line
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 80 } } });

      handlePrintDispatch(getAccessor(), "X");
      handlePrintDispatch(getAccessor(), "Y");

      // Both should overwrite last cell
      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(79, 0).char).toBe("Y");
      expect(buffer.getCell(0, 1).char).toBe(" "); // No wrap
    });

    test("should handle wide character wrap at line end", () => {
      // Position at col 79 (1-indexed) = col 78 (0-indexed), leaving 2 cells (78, 79)
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 79 } } });

      // Wide character needs 2 cells, fits at col 78 and 79
      handlePrintDispatch(getAccessor(), "\u4e2d");

      const buffer = state.getActiveBuffer();
      // Wide char placed at col 78 with placeholder at col 79
      expect(buffer.getCell(78, 0).char).toBe("\u4e2d");
      expect(buffer.getCell(79, 0).width).toBe(0); // Placeholder
      // wrapPending should be set
    });
  });

  describe("translateCharacter", () => {
    test("should return character unchanged for ASCII charset", () => {
      const result = translateCharacter(getAccessor(), "q");
      expect(result).toBe("q");
    });

    test("should translate for DecLineDrawing charset", () => {
      getAccessor().g0CharSet = "DecLineDrawing";
      const result = translateCharacter(getAccessor(), "q");
      expect(result).toBe("\u2500"); // Horizontal line
    });
  });

  describe("translateLineDrawing", () => {
    test("should translate lowercase letters to box drawing", () => {
      expect(translateLineDrawing("j")).toBe("\u2518"); // Lower right corner
      expect(translateLineDrawing("k")).toBe("\u2510"); // Upper right corner
      expect(translateLineDrawing("l")).toBe("\u250C"); // Upper left corner
      expect(translateLineDrawing("m")).toBe("\u2514"); // Lower left corner
      expect(translateLineDrawing("n")).toBe("\u253C"); // Crossing lines
      expect(translateLineDrawing("q")).toBe("\u2500"); // Horizontal line
      expect(translateLineDrawing("t")).toBe("\u251C"); // Left tee
      expect(translateLineDrawing("u")).toBe("\u2524"); // Right tee
      expect(translateLineDrawing("v")).toBe("\u2534"); // Bottom tee
      expect(translateLineDrawing("w")).toBe("\u252C"); // Top tee
      expect(translateLineDrawing("x")).toBe("\u2502"); // Vertical line
    });

    test("should return unchanged for non-translatable characters", () => {
      expect(translateLineDrawing("A")).toBe("A");
      expect(translateLineDrawing("1")).toBe("1");
      expect(translateLineDrawing("!")).toBe("!");
    });
  });

  describe("fast path", () => {
    test("should use fast path for simple ASCII", () => {
      // Multiple ASCII characters in sequence should be fast
      for (const char of "Hello, World!") {
        handlePrintDispatch(getAccessor(), char);
      }

      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(0, 0).char).toBe("H");
      expect(buffer.getCell(6, 0).char).toBe(" ");
      expect(buffer.getCell(12, 0).char).toBe("!");
    });
  });
});
