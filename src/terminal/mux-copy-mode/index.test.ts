import { describe, test, expect } from "bun:test";
import { CopyModeManager } from "./index";
import { ViKeybinds } from "./vi-keybinds";

describe("CopyModeManager", () => {
  test("starts inactive", () => {
    const mgr = new CopyModeManager();
    expect(mgr.state).toBe("inactive");
    expect(mgr.isActive).toBe(false);
  });

  test("enter activates navigating mode", () => {
    const mgr = new CopyModeManager();
    mgr.enter();
    expect(mgr.state).toBe("navigating");
    expect(mgr.isActive).toBe(true);
  });

  test("exit returns to inactive", () => {
    const mgr = new CopyModeManager();
    mgr.enter();
    mgr.exit();
    expect(mgr.state).toBe("inactive");
  });

  test("startSelection transitions to selecting", () => {
    const mgr = new CopyModeManager();
    mgr.enter();
    mgr.startSelection();
    expect(mgr.state).toBe("selecting");
    expect(mgr.getSelection()).not.toBeNull();
  });

  test("moveCursor updates position", () => {
    const mgr = new CopyModeManager();
    mgr.enter();
    mgr.moveCursor(5, 3, 80, 24);
    expect(mgr.getCursor().col).toBe(5);
    expect(mgr.getCursor().row).toBe(3);
  });

  test("moveCursor clamps to bounds", () => {
    const mgr = new CopyModeManager();
    mgr.enter();
    mgr.moveCursor(-10, -10, 80, 24);
    expect(mgr.getCursor().col).toBe(0);
    expect(mgr.getCursor().row).toBe(0);
    mgr.moveCursor(100, 100, 80, 24);
    expect(mgr.getCursor().col).toBe(79);
    expect(mgr.getCursor().row).toBe(23);
  });

  test("yank returns selection and exits", () => {
    const mgr = new CopyModeManager();
    mgr.enter();
    mgr.startSelection();
    mgr.moveCursor(10, 2, 80, 24);
    const sel = mgr.yank();
    expect(sel).not.toBeNull();
    expect(sel!.endCol).toBe(10);
    expect(mgr.state).toBe("inactive");
  });

  test("yank in navigating mode returns null", () => {
    const mgr = new CopyModeManager();
    mgr.enter();
    expect(mgr.yank()).toBeNull();
  });

  test("state change callback fires", () => {
    const mgr = new CopyModeManager();
    const states: string[] = [];
    mgr.setOnStateChange((s) => states.push(s));
    mgr.enter();
    mgr.startSelection();
    mgr.exit();
    expect(states).toEqual(["navigating", "selecting", "inactive"]);
  });
});

/** Helper to create a KeyboardEvent for testing. */
function keyEvent(key: string, opts?: KeyboardEventInit): KeyboardEvent {
  return new KeyboardEvent("keydown", { key, ...opts });
}

describe("ViKeybinds", () => {
  test("h/j/k/l movement", () => {
    const mgr = new CopyModeManager();
    mgr.enter();
    mgr.moveCursor(40, 12, 80, 24); // center
    const vi = new ViKeybinds(mgr, 80, 24);

    vi.handleKeyEvent(keyEvent("h"));
    expect(mgr.getCursor().col).toBe(39);
    vi.handleKeyEvent(keyEvent("l"));
    expect(mgr.getCursor().col).toBe(40);
    vi.handleKeyEvent(keyEvent("j"));
    expect(mgr.getCursor().row).toBe(13);
    vi.handleKeyEvent(keyEvent("k"));
    expect(mgr.getCursor().row).toBe(12);
  });

  test("0 moves to start, $ moves to end", () => {
    const mgr = new CopyModeManager();
    mgr.enter();
    mgr.moveCursor(40, 0, 80, 24);
    const vi = new ViKeybinds(mgr, 80, 24);

    vi.handleKeyEvent(keyEvent("0"));
    expect(mgr.getCursor().col).toBe(0);
    vi.handleKeyEvent(keyEvent("$"));
    expect(mgr.getCursor().col).toBe(79);
  });

  test("v starts selection", () => {
    const mgr = new CopyModeManager();
    mgr.enter();
    const vi = new ViKeybinds(mgr, 80, 24);
    vi.handleKeyEvent(keyEvent("v"));
    expect(mgr.state).toBe("selecting");
  });

  test("y yanks and exits", () => {
    const mgr = new CopyModeManager();
    mgr.enter();
    const vi = new ViKeybinds(mgr, 80, 24);
    vi.handleKeyEvent(keyEvent("v"));
    vi.handleKeyEvent(keyEvent("l"));
    vi.handleKeyEvent(keyEvent("y"));
    expect(mgr.state).toBe("inactive");
  });

  test("q exits", () => {
    const mgr = new CopyModeManager();
    mgr.enter();
    const vi = new ViKeybinds(mgr, 80, 24);
    vi.handleKeyEvent(keyEvent("q"));
    expect(mgr.state).toBe("inactive");
  });

  test("Escape exits", () => {
    const mgr = new CopyModeManager();
    mgr.enter();
    const vi = new ViKeybinds(mgr, 80, 24);
    vi.handleKeyEvent(keyEvent("Escape"));
    expect(mgr.state).toBe("inactive");
  });

  test("Ctrl+C exits", () => {
    const mgr = new CopyModeManager();
    mgr.enter();
    const vi = new ViKeybinds(mgr, 80, 24);
    vi.handleKeyEvent(keyEvent("c", { ctrlKey: true }));
    expect(mgr.state).toBe("inactive");
  });

  test("not consumed when inactive", () => {
    const mgr = new CopyModeManager();
    const vi = new ViKeybinds(mgr, 80, 24);
    expect(vi.handleKeyEvent(keyEvent("h"))).toBe(false);
  });
});
