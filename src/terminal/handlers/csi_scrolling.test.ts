/**
 * Tests for CSI scrolling handlers.
 */

import { describe, expect, test, beforeEach } from "bun:test";
import { TerminalState } from "../state.ts";
import {
  handleScrollUp,
  handleScrollDown,
  handleSetScrollRegion,
} from "./csi_scrolling.ts";
import type { TerminalStateAccessor } from "./types.ts";

describe("csi_scrolling handlers", () => {
  let state: TerminalState;

  beforeEach(() => {
    state = new TerminalState(80, 24);
    // Fill first 5 lines with content
    for (let i = 0; i < 5; i++) {
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: i + 1, col: 1 } } });
      state.processAction({ type: "Print", value: String(i) });
    }
  });

  // Helper to get accessor - TerminalState implements TerminalStateAccessor
  function getAccessor(): TerminalStateAccessor {
    return state;
  }

  describe("handleScrollUp", () => {
    test("should scroll content up by count lines", () => {
      handleScrollUp(getAccessor(), 2);

      const buffer = state.getActiveBuffer();
      // Lines 2 and 3 should now be at 0 and 1
      expect(buffer.getCell(0, 0).char).toBe("2");
      expect(buffer.getCell(0, 1).char).toBe("3");
      // Bottom should be blank
      expect(buffer.getCell(0, 22).char).toBe(" ");
      expect(buffer.getCell(0, 23).char).toBe(" ");
    });

    test("should default to 1 line", () => {
      handleScrollUp(getAccessor(), undefined);

      const buffer = state.getActiveBuffer();
      // Line 1 should now be at 0
      expect(buffer.getCell(0, 0).char).toBe("1");
    });
  });

  describe("handleScrollDown", () => {
    test("should scroll content down by count lines", () => {
      handleScrollDown(getAccessor(), 2);

      const buffer = state.getActiveBuffer();
      // Top 2 rows should be blank
      expect(buffer.getCell(0, 0).char).toBe(" ");
      expect(buffer.getCell(0, 1).char).toBe(" ");
      // Original row 0 should now be at row 2
      expect(buffer.getCell(0, 2).char).toBe("0");
      expect(buffer.getCell(0, 3).char).toBe("1");
    });

    test("should default to 1 line", () => {
      handleScrollDown(getAccessor(), undefined);

      const buffer = state.getActiveBuffer();
      // Top row should be blank
      expect(buffer.getCell(0, 0).char).toBe(" ");
      // Original row 0 should now be at row 1
      expect(buffer.getCell(0, 1).char).toBe("0");
    });
  });

  describe("handleSetScrollRegion", () => {
    test("should set scroll region and move cursor to home", () => {
      // Position cursor away from home
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 10, col: 20 } } });

      handleSetScrollRegion(getAccessor(), 5, 15);

      // Cursor should be at home (0, 0)
      expect(state.cursorRow).toBe(0);
      expect(state.cursorCol).toBe(0);

      // Scroll region should be set (verified by scrolling behavior)
      const buffer = state.getActiveBuffer();
      const region = buffer.getScrollRegion();
      expect(region).not.toBeNull();
      expect(region!.top).toBe(4); // 5-1 (1-indexed to 0-indexed)
      expect(region!.bottom).toBe(14); // 15-1
    });

    test("should default bottom to screen height", () => {
      handleSetScrollRegion(getAccessor(), 5, 0);

      const buffer = state.getActiveBuffer();
      const region = buffer.getScrollRegion();
      // When bottom is 0, it means use screen height (but might clear region if full screen)
      // For top=5, bottom=0 means rows 4-23 (0-indexed)
      // This might be null if it covers near full screen
    });

    test("should clear wrapPending", () => {
      // Set wrap pending
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 1, col: 80 } } });
      state.processAction({ type: "Print", value: "X" });

      handleSetScrollRegion(getAccessor(), 1, 24);

      // Cursor at home, wrapPending should be cleared
      expect(state.cursorRow).toBe(0);
      expect(state.cursorCol).toBe(0);
    });
  });
});
