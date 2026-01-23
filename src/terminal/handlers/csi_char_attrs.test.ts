/**
 * Tests for CSI character attributes (SGR) handler.
 */

import { describe, expect, test, beforeEach } from "bun:test";
import { TerminalState } from "../state.ts";
import { handleSgr } from "./csi_char_attrs.ts";
import type { TerminalStateAccessor } from "./types.ts";

describe("csi_char_attrs handlers", () => {
  let state: TerminalState;

  beforeEach(() => {
    state = new TerminalState(80, 24);
  });

  // Helper to get accessor - TerminalState implements TerminalStateAccessor
  function getAccessor(): TerminalStateAccessor {
    return state;
  }

  describe("handleSgr", () => {
    test("should apply bold attribute (SGR 1)", () => {
      handleSgr(getAccessor(), [1]);

      expect(state["cursor"].attrs.bold).toBe(true);
    });

    test("should apply italic attribute (SGR 3)", () => {
      handleSgr(getAccessor(), [3]);

      expect(state["cursor"].attrs.italic).toBe(true);
    });

    test("should apply underline attribute (SGR 4)", () => {
      handleSgr(getAccessor(), [4]);

      expect(state["cursor"].attrs.underline).toBe(true);
    });

    test("should reset all attributes (SGR 0)", () => {
      // First set some attributes
      handleSgr(getAccessor(), [1, 3, 4]);
      expect(state["cursor"].attrs.bold).toBe(true);
      expect(state["cursor"].attrs.italic).toBe(true);
      expect(state["cursor"].attrs.underline).toBe(true);

      // Reset
      handleSgr(getAccessor(), [0]);

      expect(state["cursor"].attrs.bold).toBe(false);
      expect(state["cursor"].attrs.italic).toBe(false);
      expect(state["cursor"].attrs.underline).toBe(false);
    });

    test("should apply foreground color (SGR 30-37)", () => {
      handleSgr(getAccessor(), [31]); // Red

      expect(state["cursor"].attrs.fg).not.toBeNull();
    });

    test("should apply background color (SGR 40-47)", () => {
      handleSgr(getAccessor(), [44]); // Blue background

      expect(state["cursor"].attrs.bg).not.toBeNull();
    });

    test("should apply 256 color foreground (SGR 38;5;n)", () => {
      handleSgr(getAccessor(), [38, 5, 196]); // Bright red

      expect(state["cursor"].attrs.fg).not.toBeNull();
    });

    test("should apply true color foreground (SGR 38;2;r;g;b)", () => {
      handleSgr(getAccessor(), [38, 2, 255, 128, 0]); // Orange

      const fg = state["cursor"].attrs.fg;
      expect(fg).not.toBeNull();
      if (fg && fg.type === "rgb") {
        expect(fg.r).toBe(255);
        expect(fg.g).toBe(128);
        expect(fg.b).toBe(0);
      }
    });

    test("should handle empty parameter array", () => {
      // Empty array is equivalent to SGR 0 (reset)
      handleSgr(getAccessor(), [1]); // Set bold
      handleSgr(getAccessor(), []); // Should reset

      expect(state["cursor"].attrs.bold).toBe(false);
    });

    test("should apply multiple attributes in sequence", () => {
      handleSgr(getAccessor(), [1, 4, 31]); // Bold, underline, red fg

      expect(state["cursor"].attrs.bold).toBe(true);
      expect(state["cursor"].attrs.underline).toBe(true);
      expect(state["cursor"].attrs.fg).not.toBeNull();
    });
  });
});
