/**
 * Tests for OSC 9 notification and progress bar handler.
 */
import { describe, it, expect } from "bun:test";
import {
  parseOsc9,
  type Osc9Action,
  type ProgressState,
} from "./osc-notification";

describe("parseOsc9", () => {
  describe("notification messages", () => {
    it("parses simple notification message", () => {
      const result = parseOsc9("Hello, World!");
      expect(result).toEqual({
        type: "notification",
        message: "Hello, World!",
      });
    });

    it("parses empty message as notification", () => {
      const result = parseOsc9("");
      expect(result).toEqual({
        type: "notification",
        message: "",
      });
    });

    it("parses message with semicolons (non-progress)", () => {
      const result = parseOsc9("Build done; 42 tests passed");
      expect(result).toEqual({
        type: "notification",
        message: "Build done; 42 tests passed",
      });
    });
  });

  describe("progress bar (OSC 9;4)", () => {
    it("parses progress set (state=1, 50%)", () => {
      const result = parseOsc9("4;1;50");
      expect(result).toEqual({
        type: "progress",
        state: 1,
        percentage: 50,
      });
    });

    it("parses progress remove (state=0)", () => {
      const result = parseOsc9("4;0;0");
      expect(result).toEqual({
        type: "progress",
        state: 0,
        percentage: 0,
      });
    });

    it("parses progress error (state=4)", () => {
      const result = parseOsc9("4;4;100");
      expect(result).toEqual({
        type: "progress",
        state: 4,
        percentage: 100,
      });
    });

    it("parses progress indeterminate (state=3, no percentage)", () => {
      const result = parseOsc9("4;3");
      expect(result).toEqual({
        type: "progress",
        state: 3,
        percentage: -1,
      });
    });

    it("clamps percentage to 0-100", () => {
      const over = parseOsc9("4;1;150");
      expect(over).toEqual({
        type: "progress",
        state: 1,
        percentage: 100,
      });

      const under = parseOsc9("4;1;-10");
      expect(under).toEqual({
        type: "progress",
        state: 1,
        percentage: 0,
      });
    });

    it("returns null for invalid state", () => {
      const result = parseOsc9("4;5;50");
      expect(result).toBeNull();
    });

    it("returns null for non-numeric state", () => {
      const result = parseOsc9("4;abc;50");
      expect(result).toBeNull();
    });
  });
});

describe("ProgressState", () => {
  it("state 0 means remove", () => {
    const result = parseOsc9("4;0;0") as { type: "progress"; state: ProgressState };
    expect(result.state).toBe(0); // Remove
  });

  it("state 1 means normal", () => {
    const result = parseOsc9("4;1;50") as { type: "progress"; state: ProgressState };
    expect(result.state).toBe(1); // Normal
  });

  it("state 2 means paused", () => {
    const result = parseOsc9("4;2;75") as { type: "progress"; state: ProgressState };
    expect(result.state).toBe(2); // Paused
  });

  it("state 3 means indeterminate", () => {
    const result = parseOsc9("4;3") as { type: "progress"; state: ProgressState };
    expect(result.state).toBe(3); // Indeterminate
  });

  it("state 4 means error", () => {
    const result = parseOsc9("4;4;100") as { type: "progress"; state: ProgressState };
    expect(result.state).toBe(4); // Error
  });
});
