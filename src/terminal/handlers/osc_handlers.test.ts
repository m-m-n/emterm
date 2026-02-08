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
  handleFoldCommand,
  handleSemanticPrompt,
  _getPendingFoldBegins,
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

    test("should dispatch EmtermExtension fold verb", () => {
      const accessor = getAccessor();
      handleOscDispatch(accessor, {
        action: "EmtermExtension",
        data: { verb: "emterm", params: ["fold", "begin", "Test Label"] },
      });
      // Pending fold should be stored
      const pending = _getPendingFoldBegins().get(accessor);
      expect(pending).not.toBeUndefined();
      expect(pending!.label).toBe("Test Label");
    });
  });

  describe("handleFoldCommand", () => {
    test("T2-1: fold;begin creates pending fold", () => {
      const accessor = getAccessor();
      handleFoldCommand(accessor, ["begin", "Build Output"]);

      const pending = _getPendingFoldBegins().get(accessor);
      expect(pending).not.toBeUndefined();
      expect(pending!.label).toBe("Build Output");
    });

    test("T2-2: fold;end completes region", () => {
      const accessor = getAccessor();
      // Simulate begin at current cursor position (row 0)
      handleFoldCommand(accessor, ["begin", "Build Output"]);

      // Move cursor down to simulate output (col=0, row=10)
      state.cursor.moveTo(0, 10);

      // End fold
      handleFoldCommand(accessor, ["end"]);

      // Pending should be cleared
      expect(_getPendingFoldBegins().get(accessor)).toBeUndefined();

      // Fold region should be registered (startLine=0, endLine=10)
      const foldManager = state.getFoldManager();
      const region = foldManager.getRegionAtLine(0);
      expect(region).not.toBeNull();
      expect(region!.source).toBe("custom");
      expect(region!.label).toBe("Build Output");
    });

    test("T2-3: fold;end without begin is silently ignored", () => {
      const accessor = getAccessor();
      // Should not throw
      expect(() => handleFoldCommand(accessor, ["end"])).not.toThrow();

      const foldManager = state.getFoldManager();
      expect(foldManager.getCollapsedRegions().length).toBe(0);
    });

    test("T2-4: consecutive begin discards previous", () => {
      const accessor = getAccessor();
      handleFoldCommand(accessor, ["begin", "First"]);
      handleFoldCommand(accessor, ["begin", "Second"]);

      const pending = _getPendingFoldBegins().get(accessor);
      expect(pending!.label).toBe("Second");
    });

    test("fold;begin with empty label uses fallback", () => {
      const accessor = getAccessor();
      handleFoldCommand(accessor, ["begin", ""]);

      const pending = _getPendingFoldBegins().get(accessor);
      expect(pending!.label).toBe("...");
    });

    test("fold commands ignored in alternate buffer", () => {
      const accessor = getAccessor();
      state.switchToAlternateBuffer(false);

      handleFoldCommand(accessor, ["begin", "Test"]);
      expect(_getPendingFoldBegins().get(accessor)).toBeUndefined();
    });
  });

  describe("handleSemanticPrompt - fold region detection", () => {
    test("T2-5: D marker creates fold region from C→D pair", () => {
      const accessor = getAccessor();

      // Simulate: A (prompt), B (command), C (output start), D (output end)
      // cursor.moveTo(col, row)
      handleSemanticPrompt(accessor, "A", null);  // row=0, line=0
      handleSemanticPrompt(accessor, "B", null);  // row=0, line=0
      state.cursor.moveTo(0, 1);
      handleSemanticPrompt(accessor, "C", null);  // row=1, line=1
      state.cursor.moveTo(0, 10);
      handleSemanticPrompt(accessor, "D", 0);     // row=10, line=10

      const foldManager = state.getFoldManager();
      // C marker at line 1, D at line 10
      const region = foldManager.getRegionAtLine(1);
      expect(region).not.toBeNull();
      expect(region!.source).toBe("osc133");
      expect(region!.exitCode).toBe(0);
    });

    test("T2-7: C without D does not create region", () => {
      const accessor = getAccessor();

      handleSemanticPrompt(accessor, "A", null);
      handleSemanticPrompt(accessor, "B", null);
      state.cursor.moveTo(1, 0);
      handleSemanticPrompt(accessor, "C", null);

      // No D marker
      const foldManager = state.getFoldManager();
      // Region at C line should not exist (no D to close it)
      // getRegionAtLine checks FoldManager, which only has registered regions
      expect(foldManager.getCollapsedRegions().length).toBe(0);
    });

    test("TS-18: D marker without preceding C is ignored", () => {
      const accessor = getAccessor();

      // Send D marker without any prior C marker
      handleSemanticPrompt(accessor, "D", 0);

      const foldManager = state.getFoldManager();
      expect(foldManager.getCollapsedRegions().length).toBe(0);
    });

    test("D marker in alternate buffer is ignored for folding", () => {
      const accessor = getAccessor();

      handleSemanticPrompt(accessor, "C", null);
      state.switchToAlternateBuffer(false);
      state.cursor.moveTo(10, 0);
      // D marker in alternate buffer should not trigger fold registration
      handleSemanticPrompt(accessor, "D", 0);

      state.switchToPrimaryBuffer(false);
      const foldManager = state.getFoldManager();
      expect(foldManager.getCollapsedRegions().length).toBe(0);
    });
  });
});
