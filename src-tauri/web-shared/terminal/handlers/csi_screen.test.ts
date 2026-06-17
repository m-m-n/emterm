/**
 * Tests for CSI screen erase handlers.
 */

import { describe, expect, test, beforeEach } from "bun:test";
import { TerminalState } from "../state.ts";
import {
  handleEraseInDisplay,
  handleEraseInLine,
  handleEraseCharacters,
} from "./csi_screen.ts";
import type { TerminalStateAccessor } from "./types.ts";
import type { EraseMode } from "../../types/terminal.ts";

describe("csi_screen handlers", () => {
  let state: TerminalState;

  beforeEach(() => {
    state = new TerminalState(80, 24);
    // Fill screen with some content
    for (let i = 0; i < 10; i++) {
      state.processAction({ type: "Print", value: `Line ${i}` });
      state.processAction({ type: "Execute", value: 0x0a }); // LF
    }
    // Position cursor
    state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 5, col: 10 } } });
  });

  // Helper to get accessor - TerminalState implements TerminalStateAccessor
  function getAccessor(): TerminalStateAccessor {
    return state;
  }

  describe("handleEraseInDisplay", () => {
    test("should erase from cursor to end of screen (Below)", () => {
      handleEraseInDisplay(getAccessor(), "Below");

      const buffer = state.getActiveBuffer();
      // Current line from cursor should be cleared
      expect(buffer.getCell(9, 4).char).toBe(" ");
      expect(buffer.getCell(10, 4).char).toBe(" ");
      // Lines below should be cleared
      expect(buffer.getCell(0, 5).char).toBe(" ");
      // Lines above should remain - each char is in separate cell
      // "Line 0" starts at col 0, so 'L' is at col 0
      expect(buffer.getCell(0, 0).char).not.toBe(" ");
    });

    test("should erase from start of screen to cursor (Above)", () => {
      handleEraseInDisplay(getAccessor(), "Above");

      const buffer = state.getActiveBuffer();
      // Lines above should be cleared
      expect(buffer.getCell(0, 0).char).toBe(" ");
      // Current line to cursor should be cleared
      expect(buffer.getCell(0, 4).char).toBe(" ");
      // Lines below remain (next test will position differently)
    });

    test("should erase entire screen (All)", () => {
      handleEraseInDisplay(getAccessor(), "All");

      const buffer = state.getActiveBuffer();
      // All lines should be cleared
      for (let row = 0; row < 24; row++) {
        expect(buffer.getCell(0, row).char).toBe(" ");
      }
    });

    test("should handle Scrollback (currently same as All)", () => {
      handleEraseInDisplay(getAccessor(), "Scrollback");

      const buffer = state.getActiveBuffer();
      // All lines should be cleared (scrollback not implemented)
      expect(buffer.getCell(0, 0).char).toBe(" ");
    });
  });

  describe("handleEraseInLine", () => {
    test("should erase from cursor to end of line (Below)", () => {
      handleEraseInLine(getAccessor(), "Below");

      const buffer = state.getActiveBuffer();
      // From cursor to end of line should be cleared
      expect(buffer.getCell(9, 4).char).toBe(" ");
      expect(buffer.getCell(79, 4).char).toBe(" ");
      // Before cursor should remain
      // Note: Line 5 was "Line 4" initially, starting at col 0
    });

    test("should erase from start of line to cursor (Above)", () => {
      handleEraseInLine(getAccessor(), "Above");

      const buffer = state.getActiveBuffer();
      // From start of line to cursor (inclusive) should be cleared
      expect(buffer.getCell(0, 4).char).toBe(" ");
      expect(buffer.getCell(9, 4).char).toBe(" ");
    });

    test("should erase entire line (All)", () => {
      handleEraseInLine(getAccessor(), "All");

      const buffer = state.getActiveBuffer();
      // Entire line should be cleared
      for (let col = 0; col < 80; col++) {
        expect(buffer.getCell(col, 4).char).toBe(" ");
      }
    });
  });

  describe("handleEraseCharacters", () => {
    test("should erase N characters at cursor without shifting", () => {
      // Create fresh state with specific content
      state = new TerminalState(80, 24);
      // Position at col 0, row 0
      for (const char of "ABCDEFGH") {
        state.processAction({ type: "Print", value: char });
      }
      // Move cursor to col 2 (where 'C' is)
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 3 } } });

      handleEraseCharacters(getAccessor(), 3);

      const buffer = state.getActiveBuffer();
      // 3 characters should be erased starting at col 2
      expect(buffer.getCell(2, 0).char).toBe(" ");
      expect(buffer.getCell(3, 0).char).toBe(" ");
      expect(buffer.getCell(4, 0).char).toBe(" ");
      // Surrounding characters should remain
      expect(buffer.getCell(1, 0).char).toBe("B");
      expect(buffer.getCell(5, 0).char).toBe("F");
    });

    test("should default to 1 character", () => {
      state = new TerminalState(80, 24);
      for (const char of "ABCDEFGH") {
        state.processAction({ type: "Print", value: char });
      }
      // Move cursor to col 2
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 3 } } });

      handleEraseCharacters(getAccessor(), undefined);

      const buffer = state.getActiveBuffer();
      // Only 1 character should be erased at col 2
      expect(buffer.getCell(2, 0).char).toBe(" ");
      expect(buffer.getCell(3, 0).char).toBe("D");
    });
  });
});
