/**
 * Tests for OSC 22 mouse cursor shape handler with push/pop stack.
 */
import { describe, it, expect } from "bun:test";
import {
  parseOsc22,
  CursorShapeStack,
  VALID_CURSOR_SHAPES,
  type Osc22Action,
} from "./osc-cursor-shape";

describe("parseOsc22", () => {
  it("parses simple set cursor", () => {
    expect(parseOsc22("pointer")).toEqual({
      type: "set",
      shape: "pointer",
    });
  });

  it("parses push cursor with > prefix", () => {
    expect(parseOsc22(">text")).toEqual({
      type: "push",
      shape: "text",
    });
  });

  it("parses pop cursor with <", () => {
    expect(parseOsc22("<")).toEqual({
      type: "pop",
    });
  });

  it("parses empty string as reset to default", () => {
    expect(parseOsc22("")).toEqual({
      type: "set",
      shape: "default",
    });
  });

  it("returns null for unknown cursor shape (set)", () => {
    expect(parseOsc22("unknown_shape")).toBeNull();
  });

  it("returns null for unknown cursor shape (push)", () => {
    expect(parseOsc22(">unknown_shape")).toBeNull();
  });

  it("accepts all valid CSS cursor shapes", () => {
    for (const shape of VALID_CURSOR_SHAPES) {
      const result = parseOsc22(shape);
      expect(result).toEqual({ type: "set", shape });
    }
  });
});

describe("CursorShapeStack", () => {
  it("starts with default cursor", () => {
    const stack = new CursorShapeStack();
    expect(stack.current()).toBe("default");
  });

  it("set changes current cursor", () => {
    const stack = new CursorShapeStack();
    stack.set("pointer");
    expect(stack.current()).toBe("pointer");
  });

  it("push saves current and sets new", () => {
    const stack = new CursorShapeStack();
    stack.set("text");
    stack.push("pointer");
    expect(stack.current()).toBe("pointer");
  });

  it("pop restores previous cursor", () => {
    const stack = new CursorShapeStack();
    stack.set("text");
    stack.push("pointer");
    stack.pop();
    expect(stack.current()).toBe("text");
  });

  it("pop on empty stack does nothing (stays at default)", () => {
    const stack = new CursorShapeStack();
    stack.pop();
    expect(stack.current()).toBe("default");
  });

  it("pop on empty stack after set keeps set value", () => {
    const stack = new CursorShapeStack();
    stack.set("crosshair");
    stack.pop();
    // Pop with empty stack should not change current
    expect(stack.current()).toBe("crosshair");
  });

  it("handles multiple push/pop operations", () => {
    const stack = new CursorShapeStack();
    stack.push("pointer");    // stack: [default]
    stack.push("text");       // stack: [default, pointer]
    stack.push("crosshair");  // stack: [default, pointer, text]
    expect(stack.current()).toBe("crosshair");

    stack.pop(); // restore text
    expect(stack.current()).toBe("text");

    stack.pop(); // restore pointer
    expect(stack.current()).toBe("pointer");

    stack.pop(); // restore default
    expect(stack.current()).toBe("default");
  });

  it("enforces max depth of 10", () => {
    const stack = new CursorShapeStack();
    // Push 10 times (fills stack)
    for (let i = 0; i < 10; i++) {
      stack.push("pointer");
    }
    // 11th push should still work (oldest entry dropped)
    stack.push("text");
    expect(stack.current()).toBe("text");
    // Stack depth should be capped at 10
    expect(stack.depth()).toBe(10);
  });

  it("reset clears stack and sets default", () => {
    const stack = new CursorShapeStack();
    stack.push("pointer");
    stack.push("text");
    stack.reset();
    expect(stack.current()).toBe("default");
    expect(stack.depth()).toBe(0);
  });
});
