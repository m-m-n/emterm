import { describe, test, expect } from "bun:test";
import { MuxTabGroup } from "./tab-group";
import type { MuxTab } from "../../tab-bar/types";

function makeMuxTab(overrides?: Partial<MuxTab>): MuxTab {
  return {
    id: "mux-1",
    type: "mux",
    title: "mux session",
    socketPath: "/tmp/emterm/mux-default.sock",
    sessionId: 1,
    expanded: false,
    windowNames: ["bash", "vim"],
    activeWindowIndex: 0,
    ...overrides,
  };
}

describe("MuxTabGroup", () => {
  test("starts in compact state by default", () => {
    const group = new MuxTabGroup(makeMuxTab());
    expect(group.state).toBe("compact");
  });

  test("starts expanded if tab is expanded", () => {
    const group = new MuxTabGroup(makeMuxTab({ expanded: true }));
    expect(group.state).toBe("expanded");
  });

  test("expand/compact toggle", () => {
    const group = new MuxTabGroup(makeMuxTab());
    group.expand();
    expect(group.state).toBe("expanded");
    group.compact();
    expect(group.state).toBe("compact");
  });

  test("toggle switches state", () => {
    const group = new MuxTabGroup(makeMuxTab());
    group.toggle();
    expect(group.state).toBe("expanded");
    group.toggle();
    expect(group.state).toBe("compact");
  });

  test("compact label shows window count", () => {
    const group = new MuxTabGroup(makeMuxTab({ windowNames: ["a", "b", "c"] }));
    expect(group.getCompactLabel()).toBe("mux (3)");
  });

  test("getWindowNames returns names", () => {
    const group = new MuxTabGroup(makeMuxTab({ windowNames: ["bash", "vim"] }));
    expect(group.getWindowNames()).toEqual(["bash", "vim"]);
  });

  test("setActiveWindow updates index", () => {
    const group = new MuxTabGroup(makeMuxTab({ windowNames: ["a", "b", "c"] }));
    group.setActiveWindow(2);
    expect(group.getActiveWindowIndex()).toBe(2);
  });

  test("setActiveWindow clamps to valid range", () => {
    const group = new MuxTabGroup(makeMuxTab({ windowNames: ["a", "b"] }));
    group.setActiveWindow(5); // out of range
    expect(group.getActiveWindowIndex()).toBe(0); // unchanged
  });

  test("updateWindowNames adjusts active index", () => {
    const group = new MuxTabGroup(makeMuxTab({ windowNames: ["a", "b", "c"], activeWindowIndex: 2 }));
    group.updateWindowNames(["a", "b"]); // removed "c"
    expect(group.getActiveWindowIndex()).toBe(1); // clamped to last
  });

  test("state change callback fires", () => {
    const group = new MuxTabGroup(makeMuxTab());
    const states: string[] = [];
    group.setOnStateChange((s) => states.push(s));
    group.expand();
    group.compact();
    expect(states).toEqual(["expanded", "compact"]);
  });

  test("expand when already expanded is no-op", () => {
    const group = new MuxTabGroup(makeMuxTab({ expanded: true }));
    const states: string[] = [];
    group.setOnStateChange((s) => states.push(s));
    group.expand(); // already expanded
    expect(states).toHaveLength(0);
  });
});
