/**
 * Tests for OSC sequence handlers.
 */

import { describe, expect, test, beforeEach } from "bun:test";
import { TerminalState } from "../state.ts";
import {
  handleOscDispatch,
  handleSetTitle,
  handleSetIconName,
  handleSetTitleAndIcon,
  handleSetWorkingDirectory,
  handleHyperlink,
} from "./osc_handlers.ts";
import type { TerminalStateAccessor } from "./types.ts";

describe("osc_handlers", () => {
  let state: TerminalState;

  beforeEach(() => {
    state = new TerminalState(80, 24);
  });

  // Helper to get accessor - TerminalState implements TerminalStateAccessor
  function getAccessor(): TerminalStateAccessor {
    return state;
  }

  describe("handleSetTitle", () => {
    test("should set window title", () => {
      handleSetTitle(getAccessor(), "My Terminal");

      expect(state.title).toBe("My Terminal");
    });
  });

  describe("handleSetIconName", () => {
    test("should set icon name", () => {
      handleSetIconName(getAccessor(), "term");

      expect(state.iconName).toBe("term");
    });
  });

  describe("handleSetTitleAndIcon", () => {
    test("should set both title and icon name", () => {
      handleSetTitleAndIcon(getAccessor(), "My Terminal");

      expect(state.title).toBe("My Terminal");
      expect(state.iconName).toBe("My Terminal");
    });
  });

  describe("handleSetWorkingDirectory", () => {
    test("should set working directory", () => {
      handleSetWorkingDirectory(getAccessor(), "/home/user/projects");

      expect(state.workingDirectory).toBe("/home/user/projects");
    });
  });

  describe("handleHyperlink", () => {
    test("should set active hyperlink", () => {
      handleHyperlink(getAccessor(), "id=123", "https://example.com");

      expect(state.activeHyperlink).not.toBeNull();
      expect(state.activeHyperlink!.uri).toBe("https://example.com");
      expect(state.activeHyperlink!.params).toBe("id=123");
    });

    test("should clear hyperlink when URI is empty", () => {
      // First set a hyperlink
      handleHyperlink(getAccessor(), "id=123", "https://example.com");
      expect(state.activeHyperlink).not.toBeNull();

      // Clear it
      handleHyperlink(getAccessor(), "", "");

      expect(state.activeHyperlink).toBeNull();
    });
  });

  describe("handleOscDispatch", () => {
    test("should dispatch SetTitle action", () => {
      handleOscDispatch(getAccessor(), { action: "SetTitle", data: "Test Title" });

      expect(state.title).toBe("Test Title");
    });

    test("should dispatch SetIconName action", () => {
      handleOscDispatch(getAccessor(), { action: "SetIconName", data: "icon" });

      expect(state.iconName).toBe("icon");
    });

    test("should dispatch SetTitleAndIcon action", () => {
      handleOscDispatch(getAccessor(), { action: "SetTitleAndIcon", data: "Both" });

      expect(state.title).toBe("Both");
      expect(state.iconName).toBe("Both");
    });

    test("should dispatch SetWorkingDirectory action", () => {
      handleOscDispatch(getAccessor(), { action: "SetWorkingDirectory", data: "/tmp" });

      expect(state.workingDirectory).toBe("/tmp");
    });

    test("should dispatch Hyperlink action", () => {
      handleOscDispatch(getAccessor(), { action: "Hyperlink", params: "", uri: "https://test.com" });

      expect(state.activeHyperlink!.uri).toBe("https://test.com");
    });

    test("should handle Unknown action gracefully", () => {
      // Should not throw
      handleOscDispatch(getAccessor(), { action: "Unknown", ps: 999, data: "unknown" });
    });

    test("should handle SetColorPalette (no-op)", () => {
      // Should not throw
      handleOscDispatch(getAccessor(), { action: "SetColorPalette", index: 0, color: "#ffffff" });
    });
  });
});
