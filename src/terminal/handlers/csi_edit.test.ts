/**
 * Tests for CSI edit (insert/delete) handlers.
 */

import { describe, expect, test, beforeEach } from "bun:test";
import { TerminalState } from "../state.ts";
import {
  handleInsertLines,
  handleDeleteLines,
  handleInsertCharacters,
  handleDeleteCharacters,
} from "./csi_edit.ts";
import type { TerminalStateAccessor } from "./types.ts";

describe("csi_edit handlers", () => {
  let state: TerminalState;

  beforeEach(() => {
    state = new TerminalState(80, 24);
  });

  // Helper to get accessor - TerminalState implements TerminalStateAccessor
  function getAccessor(): TerminalStateAccessor {
    return state;
  }

  describe("handleInsertLines", () => {
    test("should insert blank lines at cursor row", () => {
      // Fill first few lines with single-char content
      for (let i = 0; i < 5; i++) {
        state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: i + 1, col: 1 } } });
        state.processAction({ type: "Print", value: String(i) });
      }
      // Position cursor at row 1 (0-indexed)
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 2, col: 1 } } });

      handleInsertLines(getAccessor(), 2);

      const buffer = state.getActiveBuffer();
      // Row 0 should still have "0"
      expect(buffer.getCell(0, 0).char).toBe("0");
      // Row 1 and 2 should now be blank (inserted)
      expect(buffer.getCell(0, 1).char).toBe(" ");
      expect(buffer.getCell(0, 2).char).toBe(" ");
      // Original row 1 ("1") should now be at row 3
      expect(buffer.getCell(0, 3).char).toBe("1");
    });

    test("should default to 1 line", () => {
      state.processAction({ type: "Print", value: "A" });
      state.processAction({ type: "Execute", value: 0x0a }); // LF
      state.processAction({ type: "Print", value: "B" });
      // Position at row 0
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 1 } } });

      handleInsertLines(getAccessor(), undefined);

      const buffer = state.getActiveBuffer();
      // Row 0 should be blank
      expect(buffer.getCell(0, 0).char).toBe(" ");
      // Row 1 should have A (was at row 0)
      expect(buffer.getCell(0, 1).char).toBe("A");
    });
  });

  describe("handleDeleteLines", () => {
    test("should delete lines at cursor row and shift content up", () => {
      // Fill first few lines
      for (let i = 0; i < 5; i++) {
        state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: i + 1, col: 1 } } });
        state.processAction({ type: "Print", value: `${i}` });
      }
      // Position cursor at row 1 (0-indexed)
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 2, col: 1 } } });

      handleDeleteLines(getAccessor(), 2);

      const buffer = state.getActiveBuffer();
      // Row 0 should still have 0
      expect(buffer.getCell(0, 0).char).toBe("0");
      // Row 1 should now have what was at row 3 (which was "3")
      expect(buffer.getCell(0, 1).char).toBe("3");
      // Row 2 should have what was at row 4
      expect(buffer.getCell(0, 2).char).toBe("4");
    });

    test("should default to 1 line", () => {
      // Position at row 0
      state.processAction({ type: "Print", value: "A" });
      // Move to row 1
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 2, col: 1 } } });
      state.processAction({ type: "Print", value: "B" });
      // Position cursor back at row 0
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 1 } } });

      handleDeleteLines(getAccessor(), undefined);

      const buffer = state.getActiveBuffer();
      // Row 0 should now have B (was at row 1)
      expect(buffer.getCell(0, 0).char).toBe("B");
    });
  });

  describe("handleInsertCharacters", () => {
    test("should insert blank characters and shift content right", () => {
      // Write ABCDEF
      for (const char of "ABCDEF") {
        state.processAction({ type: "Print", value: char });
      }
      // Position cursor at col 2 (where C is)
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 3 } } });

      handleInsertCharacters(getAccessor(), 2);

      const buffer = state.getActiveBuffer();
      // A and B should remain
      expect(buffer.getCell(0, 0).char).toBe("A");
      expect(buffer.getCell(1, 0).char).toBe("B");
      // 2 blanks inserted at col 2 and 3
      expect(buffer.getCell(2, 0).char).toBe(" ");
      expect(buffer.getCell(3, 0).char).toBe(" ");
      // C, D, E, F shifted right
      expect(buffer.getCell(4, 0).char).toBe("C");
      expect(buffer.getCell(5, 0).char).toBe("D");
    });

    test("should default to 1 character", () => {
      for (const char of "ABC") {
        state.processAction({ type: "Print", value: char });
      }
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 2 } } });

      handleInsertCharacters(getAccessor(), undefined);

      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(0, 0).char).toBe("A");
      expect(buffer.getCell(1, 0).char).toBe(" "); // Inserted
      expect(buffer.getCell(2, 0).char).toBe("B"); // Shifted
    });
  });

  describe("handleDeleteCharacters", () => {
    test("should delete characters and shift content left", () => {
      for (const char of "ABCDEF") {
        state.processAction({ type: "Print", value: char });
      }
      // Position cursor at col 2 (where C is)
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 3 } } });

      handleDeleteCharacters(getAccessor(), 2);

      const buffer = state.getActiveBuffer();
      // A and B should remain
      expect(buffer.getCell(0, 0).char).toBe("A");
      expect(buffer.getCell(1, 0).char).toBe("B");
      // E and F shifted left to col 2 and 3
      expect(buffer.getCell(2, 0).char).toBe("E");
      expect(buffer.getCell(3, 0).char).toBe("F");
      // Rest should be blank
      expect(buffer.getCell(4, 0).char).toBe(" ");
    });

    test("should default to 1 character", () => {
      for (const char of "ABC") {
        state.processAction({ type: "Print", value: char });
      }
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 2 } } });

      handleDeleteCharacters(getAccessor(), undefined);

      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(0, 0).char).toBe("A");
      expect(buffer.getCell(1, 0).char).toBe("C"); // Shifted left
      expect(buffer.getCell(2, 0).char).toBe(" "); // Blank at end
    });
  });
});
