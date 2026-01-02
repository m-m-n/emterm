/**
 * Tests for terminal modes management.
 */
import { describe, it, expect } from "bun:test";
import {
  createDefaultModes,
  cloneModes,
  setDecPrivateMode,
  DECPrivateMode,
  type TerminalModes,
} from "./modes.ts";

describe("createDefaultModes", () => {
  it("should create modes with default values", () => {
    const modes = createDefaultModes();

    expect(modes.cursorKeys).toBe("normal");
    expect(modes.column132).toBe(false);
    expect(modes.reverseScreen).toBe(false);
    expect(modes.originMode).toBe(false);
    expect(modes.autoWrap).toBe(true);
    expect(modes.cursorBlink).toBe(true);
    expect(modes.cursorVisible).toBe(true);
    expect(modes.mouseTracking).toBe("none");
    expect(modes.mouseEncoding).toBe("default");
    expect(modes.focusTracking).toBe(false);
    expect(modes.bracketedPaste).toBe(false);
  });
});

describe("cloneModes", () => {
  it("should create an independent copy", () => {
    const original = createDefaultModes();
    original.autoWrap = false;
    original.cursorVisible = false;

    const cloned = cloneModes(original);

    expect(cloned.autoWrap).toBe(false);
    expect(cloned.cursorVisible).toBe(false);

    // Modify original and verify clone is unaffected
    original.autoWrap = true;
    expect(cloned.autoWrap).toBe(false);
  });
});

describe("setDecPrivateMode", () => {
  describe("DECCKM (1) - Cursor Keys Mode", () => {
    it("should set cursor keys to application mode", () => {
      const modes = createDefaultModes();
      const result = setDecPrivateMode(modes, DECPrivateMode.DECCKM, true);

      expect(result.changed).toBe(true);
      expect(modes.cursorKeys).toBe("application");
    });

    it("should set cursor keys to normal mode", () => {
      const modes = createDefaultModes();
      modes.cursorKeys = "application";
      const result = setDecPrivateMode(modes, DECPrivateMode.DECCKM, false);

      expect(result.changed).toBe(true);
      expect(modes.cursorKeys).toBe("normal");
    });
  });

  describe("DECAWM (7) - Auto Wrap Mode", () => {
    it("should enable auto wrap", () => {
      const modes = createDefaultModes();
      modes.autoWrap = false;
      const result = setDecPrivateMode(modes, DECPrivateMode.DECAWM, true);

      expect(result.changed).toBe(true);
      expect(modes.autoWrap).toBe(true);
    });

    it("should disable auto wrap", () => {
      const modes = createDefaultModes();
      const result = setDecPrivateMode(modes, DECPrivateMode.DECAWM, false);

      expect(result.changed).toBe(true);
      expect(modes.autoWrap).toBe(false);
    });
  });

  describe("DECTCEM (25) - Cursor Visibility", () => {
    it("should show cursor", () => {
      const modes = createDefaultModes();
      modes.cursorVisible = false;
      const result = setDecPrivateMode(modes, DECPrivateMode.DECTCEM, true);

      expect(result.changed).toBe(true);
      expect(modes.cursorVisible).toBe(true);
    });

    it("should hide cursor", () => {
      const modes = createDefaultModes();
      const result = setDecPrivateMode(modes, DECPrivateMode.DECTCEM, false);

      expect(result.changed).toBe(true);
      expect(modes.cursorVisible).toBe(false);
    });

    it("should not report change if already in desired state", () => {
      const modes = createDefaultModes();
      // cursorVisible is true by default
      const result = setDecPrivateMode(modes, DECPrivateMode.DECTCEM, true);

      expect(result.changed).toBe(false);
    });
  });

  describe("Alternate Buffer (47, 1047, 1049)", () => {
    it("should return switchToAlt action for mode 47 enable", () => {
      const modes = createDefaultModes();
      const result = setDecPrivateMode(modes, DECPrivateMode.XTERM_ALTBUF_47, true);

      expect(result.action).toBe("switchToAlt");
    });

    it("should return switchToMain action for mode 47 disable", () => {
      const modes = createDefaultModes();
      const result = setDecPrivateMode(modes, DECPrivateMode.XTERM_ALTBUF_47, false);

      expect(result.action).toBe("switchToMain");
    });

    it("should return saveAndSwitchToAlt action for mode 1049 enable", () => {
      const modes = createDefaultModes();
      const result = setDecPrivateMode(modes, DECPrivateMode.XTERM_ALTBUF_1049, true);

      expect(result.action).toBe("saveAndSwitchToAlt");
    });

    it("should return switchToMain action for mode 1049 disable", () => {
      const modes = createDefaultModes();
      const result = setDecPrivateMode(modes, DECPrivateMode.XTERM_ALTBUF_1049, false);

      expect(result.action).toBe("switchToMain");
    });
  });

  describe("Cursor Save/Restore (1048)", () => {
    it("should return saveCursor action on enable", () => {
      const modes = createDefaultModes();
      const result = setDecPrivateMode(modes, DECPrivateMode.XTERM_SAVE, true);

      expect(result.action).toBe("saveCursor");
    });

    it("should return restoreCursor action on disable", () => {
      const modes = createDefaultModes();
      const result = setDecPrivateMode(modes, DECPrivateMode.XTERM_SAVE, false);

      expect(result.action).toBe("restoreCursor");
    });
  });

  describe("Mouse Tracking (1000, 1002, 1003)", () => {
    it("should enable X10 mouse tracking", () => {
      const modes = createDefaultModes();
      const result = setDecPrivateMode(modes, DECPrivateMode.X10_MOUSE, true);

      expect(result.changed).toBe(true);
      expect(modes.mouseTracking).toBe("x10");
    });

    it("should disable X10 mouse tracking", () => {
      const modes = createDefaultModes();
      modes.mouseTracking = "x10";
      const result = setDecPrivateMode(modes, DECPrivateMode.X10_MOUSE, false);

      expect(result.changed).toBe(true);
      expect(modes.mouseTracking).toBe("none");
    });

    it("should enable button event mouse tracking", () => {
      const modes = createDefaultModes();
      const result = setDecPrivateMode(modes, DECPrivateMode.BTN_EVENT_MOUSE, true);

      expect(result.changed).toBe(true);
      expect(modes.mouseTracking).toBe("button");
    });

    it("should enable any event mouse tracking", () => {
      const modes = createDefaultModes();
      const result = setDecPrivateMode(modes, DECPrivateMode.ANY_EVENT_MOUSE, true);

      expect(result.changed).toBe(true);
      expect(modes.mouseTracking).toBe("any");
    });
  });

  describe("Mouse Encoding (1005, 1006)", () => {
    it("should enable UTF-8 mouse encoding", () => {
      const modes = createDefaultModes();
      const result = setDecPrivateMode(modes, DECPrivateMode.UTF8_MOUSE, true);

      expect(result.changed).toBe(true);
      expect(modes.mouseEncoding).toBe("utf8");
    });

    it("should enable SGR mouse encoding", () => {
      const modes = createDefaultModes();
      const result = setDecPrivateMode(modes, DECPrivateMode.SGR_MOUSE, true);

      expect(result.changed).toBe(true);
      expect(modes.mouseEncoding).toBe("sgr");
    });
  });

  describe("Bracketed Paste (2004)", () => {
    it("should enable bracketed paste mode", () => {
      const modes = createDefaultModes();
      const result = setDecPrivateMode(modes, DECPrivateMode.BRACKETED_PASTE, true);

      expect(result.changed).toBe(true);
      expect(modes.bracketedPaste).toBe(true);
    });

    it("should disable bracketed paste mode", () => {
      const modes = createDefaultModes();
      modes.bracketedPaste = true;
      const result = setDecPrivateMode(modes, DECPrivateMode.BRACKETED_PASTE, false);

      expect(result.changed).toBe(true);
      expect(modes.bracketedPaste).toBe(false);
    });
  });

  describe("Focus Tracking (1004)", () => {
    it("should enable focus tracking", () => {
      const modes = createDefaultModes();
      const result = setDecPrivateMode(modes, DECPrivateMode.FOCUS_TRACKING, true);

      expect(result.changed).toBe(true);
      expect(modes.focusTracking).toBe(true);
    });
  });
});
