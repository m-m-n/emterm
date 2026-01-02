/**
 * Tests for Phase 5: Mode and Buffer Management
 */
import { describe, it, expect } from "bun:test";
import { TerminalState } from "./state.ts";

describe("TerminalState Phase 5: Mode and Buffer Management", () => {
  describe("Cursor Visibility (DECTCEM)", () => {
    it("should show cursor by default", () => {
      const state = new TerminalState(80, 24);
      expect(state.cursorVisible).toBe(true);
    });

    it("should hide cursor with CSI ?25l", () => {
      const state = new TerminalState(80, 24);
      state.processAction({
        type: "Csi",
        value: { action: "ResetMode", data: [25] },
      });
      expect(state.cursorVisible).toBe(false);
    });

    it("should show cursor with CSI ?25h", () => {
      const state = new TerminalState(80, 24);
      // First hide it
      state.processAction({
        type: "Csi",
        value: { action: "ResetMode", data: [25] },
      });
      // Then show it
      state.processAction({
        type: "Csi",
        value: { action: "SetMode", data: [25] },
      });
      expect(state.cursorVisible).toBe(true);
    });
  });

  describe("Alternate Buffer", () => {
    it("should start with primary buffer", () => {
      const state = new TerminalState(80, 24);
      expect(state.isAlternateBuffer).toBe(false);
    });

    it("should switch to alternate buffer with mode 47", () => {
      const state = new TerminalState(80, 24);

      // Write to primary buffer
      state.processAction({ type: "Print", value: "Hello" });

      // Switch to alternate buffer
      state.processAction({
        type: "Csi",
        value: { action: "SetMode", data: [47] },
      });

      expect(state.isAlternateBuffer).toBe(true);
    });

    it("should preserve main buffer content when switching", () => {
      const state = new TerminalState(80, 24);

      // Write to primary buffer (char by char like Rust parser does)
      for (const c of "Primary") {
        state.processAction({ type: "Print", value: c });
      }

      // Switch to alternate buffer
      state.processAction({
        type: "Csi",
        value: { action: "SetMode", data: [47] },
      });

      // Write to alternate buffer
      for (const c of "Alternate") {
        state.processAction({ type: "Print", value: c });
      }

      // Switch back to primary
      state.processAction({
        type: "Csi",
        value: { action: "ResetMode", data: [47] },
      });

      expect(state.isAlternateBuffer).toBe(false);

      // Verify primary buffer content is restored
      const buffer = state.getActiveBuffer();
      const line = buffer.getLine(0);
      expect(line.getCell(0).char).toBe("P");
      expect(line.getCell(1).char).toBe("r");
      expect(line.getCell(2).char).toBe("i");
    });

    it("should switch to alternate buffer with mode 1049 and save cursor", () => {
      const state = new TerminalState(80, 24);

      // Move cursor
      state.processAction({
        type: "Csi",
        value: { action: "CursorPosition", data: { row: 10, col: 20 } },
      });

      // Switch to alternate with 1049 (saves cursor)
      state.processAction({
        type: "Csi",
        value: { action: "SetMode", data: [1049] },
      });

      expect(state.isAlternateBuffer).toBe(true);
      // Cursor is reset in alternate buffer
      expect(state.cursorCol).toBe(0);
      expect(state.cursorRow).toBe(0);

      // Switch back - cursor should be restored
      state.processAction({
        type: "Csi",
        value: { action: "ResetMode", data: [1049] },
      });

      expect(state.cursorCol).toBe(19); // 20 - 1 (0-indexed)
      expect(state.cursorRow).toBe(9);  // 10 - 1 (0-indexed)
    });
  });

  describe("Cursor Save/Restore (ESC 7 / ESC 8)", () => {
    it("should save and restore cursor position", () => {
      const state = new TerminalState(80, 24);

      // Move cursor
      state.processAction({
        type: "Csi",
        value: { action: "CursorPosition", data: { row: 5, col: 10 } },
      });

      // Save cursor (ESC 7)
      state.processAction({
        type: "Esc",
        value: { action: "SaveCursor" },
      });

      // Move cursor elsewhere
      state.processAction({
        type: "Csi",
        value: { action: "CursorPosition", data: { row: 1, col: 1 } },
      });

      expect(state.cursorRow).toBe(0);
      expect(state.cursorCol).toBe(0);

      // Restore cursor (ESC 8)
      state.processAction({
        type: "Esc",
        value: { action: "RestoreCursor" },
      });

      expect(state.cursorRow).toBe(4); // 5 - 1
      expect(state.cursorCol).toBe(9); // 10 - 1
    });
  });

  describe("Auto Wrap Mode (DECAWM)", () => {
    it("should wrap by default", () => {
      const state = new TerminalState(10, 5);
      const modes = state.getModes();
      expect(modes.autoWrap).toBe(true);
    });

    it("should not wrap when disabled", () => {
      const state = new TerminalState(10, 5);

      // Disable auto wrap
      state.processAction({
        type: "Csi",
        value: { action: "ResetMode", data: [7] },
      });

      // Print text that exceeds line width
      for (let i = 0; i < 15; i++) {
        state.processAction({ type: "Print", value: "X" });
      }

      // Should stay on first line, cursor at end
      expect(state.cursorRow).toBe(0);
    });

    it("should wrap when enabled", () => {
      const state = new TerminalState(10, 5);

      // Print text that exceeds line width
      for (let i = 0; i < 12; i++) {
        state.processAction({ type: "Print", value: "X" });
      }

      // Should have wrapped to second line
      expect(state.cursorRow).toBe(1);
      expect(state.cursorCol).toBe(2); // 12 - 10 = 2
    });
  });

  describe("Bracketed Paste Mode", () => {
    it("should enable bracketed paste", () => {
      const state = new TerminalState(80, 24);

      state.processAction({
        type: "Csi",
        value: { action: "SetMode", data: [2004] },
      });

      const modes = state.getModes();
      expect(modes.bracketedPaste).toBe(true);
    });

    it("should disable bracketed paste", () => {
      const state = new TerminalState(80, 24);

      // Enable first
      state.processAction({
        type: "Csi",
        value: { action: "SetMode", data: [2004] },
      });

      // Then disable
      state.processAction({
        type: "Csi",
        value: { action: "ResetMode", data: [2004] },
      });

      const modes = state.getModes();
      expect(modes.bracketedPaste).toBe(false);
    });
  });

  describe("ESC c - Reset to Initial State", () => {
    it("should reset terminal to initial state", () => {
      const state = new TerminalState(80, 24);

      // Make some changes
      state.processAction({ type: "Print", value: "Hello" });
      state.processAction({
        type: "Csi",
        value: { action: "ResetMode", data: [25] }, // Hide cursor
      });
      state.processAction({
        type: "Csi",
        value: { action: "SetMode", data: [1049] }, // Alt buffer
      });

      // Reset
      state.processAction({
        type: "Esc",
        value: { action: "ResetToInitialState" },
      });

      // Verify reset
      expect(state.cursorCol).toBe(0);
      expect(state.cursorRow).toBe(0);
      expect(state.cursorVisible).toBe(true);
      expect(state.isAlternateBuffer).toBe(false);

      const modes = state.getModes();
      expect(modes.autoWrap).toBe(true);
      expect(modes.bracketedPaste).toBe(false);
    });
  });

  describe("ESC D - Index", () => {
    it("should move cursor down", () => {
      const state = new TerminalState(80, 24);

      state.processAction({
        type: "Esc",
        value: { action: "Index" },
      });

      expect(state.cursorRow).toBe(1);
    });

    it("should scroll at bottom margin", () => {
      const state = new TerminalState(80, 5);

      // Move to bottom
      state.processAction({
        type: "Csi",
        value: { action: "CursorPosition", data: { row: 5, col: 1 } },
      });

      // Print something on line 5
      state.processAction({ type: "Print", value: "Bottom" });

      // Move cursor back to column 1
      state.processAction({ type: "Execute", value: 0x0d }); // CR

      // Index (should scroll)
      state.processAction({
        type: "Esc",
        value: { action: "Index" },
      });

      // Cursor should stay at bottom row
      expect(state.cursorRow).toBe(4); // 0-indexed
    });
  });

  describe("ESC E - Next Line", () => {
    it("should move to column 0 of next line", () => {
      const state = new TerminalState(80, 24);

      // Move to some column (char by char)
      for (const c of "Hello") {
        state.processAction({ type: "Print", value: c });
      }
      expect(state.cursorCol).toBe(5);

      // Next line
      state.processAction({
        type: "Esc",
        value: { action: "NextLine" },
      });

      expect(state.cursorRow).toBe(1);
      expect(state.cursorCol).toBe(0);
    });
  });

  describe("ESC H - Horizontal Tab Set", () => {
    it("should set tab stop at current column", () => {
      const state = new TerminalState(80, 24);

      // Move to column 15
      state.processAction({
        type: "Csi",
        value: { action: "CursorHorizontalAbsolute", data: 15 },
      });

      // Set tab stop
      state.processAction({
        type: "Esc",
        value: { action: "HorizontalTabSet" },
      });

      // Go to column 0
      state.processAction({ type: "Execute", value: 0x0d }); // CR

      // Tab should go to column 8 first (default), then 14 (our new stop)
      state.processAction({ type: "Execute", value: 0x09 }); // HT
      expect(state.cursorCol).toBe(8);

      state.processAction({ type: "Execute", value: 0x09 }); // HT
      expect(state.cursorCol).toBe(14); // 0-indexed from column 15
    });
  });

  describe("ESC M - Reverse Index", () => {
    it("should move cursor up", () => {
      const state = new TerminalState(80, 24);

      // Move down first
      state.processAction({
        type: "Csi",
        value: { action: "CursorPosition", data: { row: 5, col: 1 } },
      });

      // Reverse index
      state.processAction({
        type: "Esc",
        value: { action: "ReverseIndex" },
      });

      expect(state.cursorRow).toBe(3); // 5-1-1 = 3 (0-indexed)
    });

    it("should scroll down at top margin", () => {
      const state = new TerminalState(80, 5);

      // Put content on first line (char by char)
      for (const c of "First") {
        state.processAction({ type: "Print", value: c });
      }

      // Move to start
      state.processAction({
        type: "Csi",
        value: { action: "CursorPosition", data: { row: 1, col: 1 } },
      });

      // Reverse index at top (should scroll down)
      state.processAction({
        type: "Esc",
        value: { action: "ReverseIndex" },
      });

      // Cursor should stay at row 0
      expect(state.cursorRow).toBe(0);

      // Content should have moved down
      const buffer = state.getActiveBuffer();
      // Line 0 should now be empty
      expect(buffer.getLine(0).getCell(0).char).toBe(" ");
      // Line 1 should have the old content
      expect(buffer.getLine(1).getCell(0).char).toBe("F");
    });
  });

  describe("Character Set Selection (ESC ( / ESC ))", () => {
    it("should set G0 character set to DEC Line Drawing", () => {
      const state = new TerminalState(80, 24);

      // Set G0 to DEC Line Drawing
      state.processAction({
        type: "Esc",
        value: { action: "SetG0CharSet", data: "DecLineDrawing" },
      });

      // Print 'q' which should become horizontal line in DEC line drawing
      state.processAction({ type: "Print", value: "q" });

      const buffer = state.getActiveBuffer();
      const cell = buffer.getLine(0).getCell(0);
      // Should be horizontal line character
      expect(cell.char).toBe("\u2500");
    });

    it("should switch to G1 with SO and back to G0 with SI", () => {
      const state = new TerminalState(80, 24);

      // Set G1 to DEC Line Drawing
      state.processAction({
        type: "Esc",
        value: { action: "SetG1CharSet", data: "DecLineDrawing" },
      });

      // Print 'q' - should be normal
      state.processAction({ type: "Print", value: "q" });

      // Switch to G1 (SO)
      state.processAction({ type: "Execute", value: 0x0e }); // SO

      // Print 'q' - should be line drawing
      state.processAction({ type: "Print", value: "q" });

      // Switch back to G0 (SI)
      state.processAction({ type: "Execute", value: 0x0f }); // SI

      // Print 'q' - should be normal again
      state.processAction({ type: "Print", value: "q" });

      const buffer = state.getActiveBuffer();
      expect(buffer.getLine(0).getCell(0).char).toBe("q"); // Normal
      expect(buffer.getLine(0).getCell(1).char).toBe("\u2500"); // Line drawing
      expect(buffer.getLine(0).getCell(2).char).toBe("q"); // Normal
    });
  });
});
