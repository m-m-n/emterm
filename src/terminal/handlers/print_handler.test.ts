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

  describe("emoji grapheme cluster buffering", () => {
    test("should place single emoji with width 2 and placeholder", () => {
      // 📁 is U+1F4C1, Emoji_Presentation=Yes
      // Emit as surrogate pair characters via processAction
      state.processAction({ type: "Print", value: "📁" });
      state.flushGraphemeBuffer();

      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(0, 0).char).toBe("📁");
      expect(buffer.getCell(0, 0).width).toBe(2);
      expect(buffer.getCell(1, 0).width).toBe(0); // placeholder
      expect(state.cursorCol).toBe(2);
    });

    test("should buffer ZWJ sequence as single cell with width 2", () => {
      // 👨‍👩‍👧 = 👨 (U+1F468) + ZWJ (U+200D) + 👩 (U+1F469) + ZWJ (U+200D) + 👧 (U+1F467)
      const codepoints = [0x1F468, 0x200D, 0x1F469, 0x200D, 0x1F467];
      for (const cp of codepoints) {
        state.processAction({ type: "Print", value: String.fromCodePoint(cp) });
      }
      // Flush by sending an ASCII character
      state.processAction({ type: "Print", value: "A" });

      const buffer = state.getActiveBuffer();
      // ZWJ sequence should be in cell 0 as a single cluster
      expect(buffer.getCell(0, 0).char).toBe("👨\u200D👩\u200D👧");
      expect(buffer.getCell(0, 0).width).toBe(2);
      expect(buffer.getCell(1, 0).width).toBe(0); // placeholder
      // "A" should be at col 2
      expect(buffer.getCell(2, 0).char).toBe("A");
      expect(state.cursorCol).toBe(3);
    });

    test("should buffer Regional Indicator pair as single cell with width 2", () => {
      // 🇯🇵 = U+1F1EF (J) + U+1F1F5 (P)
      state.processAction({ type: "Print", value: String.fromCodePoint(0x1F1EF) });
      state.processAction({ type: "Print", value: String.fromCodePoint(0x1F1F5) });

      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(0, 0).char).toBe("🇯🇵");
      expect(buffer.getCell(0, 0).width).toBe(2);
      expect(buffer.getCell(1, 0).width).toBe(0); // placeholder
      expect(state.cursorCol).toBe(2);
    });

    test("should buffer skin tone modified emoji as single cell with width 2", () => {
      // 👋🏻 = U+1F44B + U+1F3FB
      state.processAction({ type: "Print", value: String.fromCodePoint(0x1F44B) });
      state.processAction({ type: "Print", value: String.fromCodePoint(0x1F3FB) });
      state.flushGraphemeBuffer();

      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(0, 0).char).toBe("👋🏻");
      expect(buffer.getCell(0, 0).width).toBe(2);
      expect(state.cursorCol).toBe(2);
    });

    test("should handle emoji + U+FE0F with width 2", () => {
      // ☀️ = ☀ (U+2600) + FE0F
      state.processAction({ type: "Print", value: String.fromCodePoint(0x2600) });
      state.processAction({ type: "Print", value: String.fromCodePoint(0xFE0F) });
      state.flushGraphemeBuffer();

      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(0, 0).char).toBe("☀\uFE0F");
      expect(buffer.getCell(0, 0).width).toBe(2);
      expect(state.cursorCol).toBe(2);
    });

    test("should handle emoji + U+FE0E with width 1", () => {
      // ☀︎ = ☀ (U+2600) + FE0E
      state.processAction({ type: "Print", value: String.fromCodePoint(0x2600) });
      state.processAction({ type: "Print", value: String.fromCodePoint(0xFE0E) });
      state.flushGraphemeBuffer();

      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(0, 0).char).toBe("☀\uFE0E");
      expect(buffer.getCell(0, 0).width).toBe(1);
      expect(state.cursorCol).toBe(1);
    });

    test("should flush buffer when ASCII follows emoji", () => {
      state.processAction({ type: "Print", value: "📁" });
      state.processAction({ type: "Print", value: "A" });

      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(0, 0).char).toBe("📁");
      expect(buffer.getCell(0, 0).width).toBe(2);
      expect(buffer.getCell(2, 0).char).toBe("A");
      expect(state.cursorCol).toBe(3);
    });

    test("should flush buffer on non-Print action", () => {
      state.processAction({ type: "Print", value: "📁" });
      // Execute a line feed (non-Print action)
      state.processAction({ type: "Execute", value: 10 }); // LF

      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(0, 0).char).toBe("📁");
      expect(buffer.getCell(0, 0).width).toBe(2);
    });

    test("should handle mixed text 'Hello📁World' correctly", () => {
      for (const char of "Hello") {
        state.processAction({ type: "Print", value: char });
      }
      state.processAction({ type: "Print", value: "📁" });
      for (const char of "World") {
        state.processAction({ type: "Print", value: char });
      }

      const buffer = state.getActiveBuffer();
      // H(0) e(1) l(2) l(3) o(4) 📁(5,6) W(7) o(8) r(9) l(10) d(11)
      expect(buffer.getCell(0, 0).char).toBe("H");
      expect(buffer.getCell(4, 0).char).toBe("o");
      expect(buffer.getCell(5, 0).char).toBe("📁");
      expect(buffer.getCell(5, 0).width).toBe(2);
      expect(buffer.getCell(6, 0).width).toBe(0); // placeholder
      expect(buffer.getCell(7, 0).char).toBe("W");
      expect(buffer.getCell(11, 0).char).toBe("d");
      expect(state.cursorCol).toBe(12);
    });

    test("should handle lone ZWJ gracefully", () => {
      // ZWJ not preceded by emoji should be flushed as zero-width
      state.processAction({ type: "Print", value: "\u200D" });
      state.processAction({ type: "Print", value: "A" });

      const buffer = state.getActiveBuffer();
      // ZWJ is zero-width, so "A" should be at col 0
      expect(buffer.getCell(0, 0).char).toBe("A");
    });

    test("should handle lone Regional Indicator", () => {
      // Single RI (not a pair) should be flushed when next non-RI arrives
      state.processAction({ type: "Print", value: String.fromCodePoint(0x1F1EF) });
      state.processAction({ type: "Print", value: "A" });

      const buffer = state.getActiveBuffer();
      // Single RI should be placed as width 2 (it is Emoji_Presentation=Yes)
      expect(buffer.getCell(0, 0).width).toBe(2);
      expect(buffer.getCell(2, 0).char).toBe("A");
    });

    test("should handle emoji at end of line with proper wrap", () => {
      // Position cursor at col 79 (last column)
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 80 } } });

      // Print emoji (width 2) - should wrap to next line
      state.processAction({ type: "Print", value: "📁" });
      state.flushGraphemeBuffer();

      const buffer = state.getActiveBuffer();
      expect(buffer.getCell(0, 1).char).toBe("📁");
      expect(buffer.getCell(0, 1).width).toBe(2);
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
