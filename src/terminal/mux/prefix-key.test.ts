import { describe, test, expect } from "bun:test";
import { PrefixKeyHandler, type MuxAction } from "./prefix-key";

function makeKeyEvent(
  key: string,
  ctrlKey = false,
  shiftKey = false,
  altKey = false,
): KeyboardEvent {
  return new KeyboardEvent("keydown", { key, ctrlKey, shiftKey, altKey });
}

describe("PrefixKeyHandler", () => {
  test("starts in idle state", () => {
    const handler = new PrefixKeyHandler();
    expect(handler.state).toBe("idle");
  });

  test("Ctrl+b enters waiting state", () => {
    const handler = new PrefixKeyHandler();
    const consumed = handler.handleKeyEvent(makeKeyEvent("b", true));
    expect(consumed).toBe(true);
    expect(handler.state).toBe("waiting");
  });

  test("non-prefix key in idle is not consumed", () => {
    const handler = new PrefixKeyHandler();
    const consumed = handler.handleKeyEvent(makeKeyEvent("a"));
    expect(consumed).toBe(false);
    expect(handler.state).toBe("idle");
  });

  test("prefix + d dispatches detach", () => {
    const handler = new PrefixKeyHandler();
    const actions: MuxAction[] = [];
    handler.setOnAction((a) => actions.push(a));

    handler.handleKeyEvent(makeKeyEvent("b", true));
    handler.handleKeyEvent(makeKeyEvent("d"));

    expect(actions[0]!.type).toBe("detach");
  });

  test("double prefix sends passthrough", () => {
    const handler = new PrefixKeyHandler();
    const actions: MuxAction[] = [];
    handler.setOnAction((a) => actions.push(a));

    handler.handleKeyEvent(makeKeyEvent("b", true));
    handler.handleKeyEvent(makeKeyEvent("b", true));

    expect(actions[0]!.type).toBe("prefix-passthrough");
  });

  test("unknown key after prefix is consumed but no action", () => {
    const handler = new PrefixKeyHandler();
    const actions: MuxAction[] = [];
    handler.setOnAction((a) => actions.push(a));

    handler.handleKeyEvent(makeKeyEvent("b", true));
    const consumed = handler.handleKeyEvent(makeKeyEvent("q"));

    expect(consumed).toBe(true);
    expect(actions).toHaveLength(0);
    expect(handler.state).toBe("idle");
  });

  test("reset returns to idle", () => {
    const handler = new PrefixKeyHandler();
    handler.handleKeyEvent(makeKeyEvent("b", true));
    expect(handler.state).toBe("waiting");
    handler.reset();
    expect(handler.state).toBe("idle");
  });

  test("all tmux-compatible bindings are present", () => {
    const handler = new PrefixKeyHandler();
    const actions: MuxAction[] = [];
    handler.setOnAction((a) => actions.push(a));

    const bindings = [
      { key: "d", expected: "detach" },
      { key: "c", expected: "new-window" },
      { key: "n", expected: "next-window" },
      { key: "p", expected: "prev-window" },
      { key: ",", expected: "rename-window" },
    ];

    for (const { key, expected } of bindings) {
      actions.length = 0;
      handler.handleKeyEvent(makeKeyEvent("b", true));
      handler.handleKeyEvent(makeKeyEvent(key));
      expect(actions).toHaveLength(1);
      expect(actions[0]!.type).toBe(expected);
    }
  });

  test("removed tmux action keys dispatch no action and return to idle", () => {
    const handler = new PrefixKeyHandler();
    const actions: MuxAction[] = [];
    handler.setOnAction((a) => actions.push(a));

    const removedKeys = ["%", '"', "o", ";", "x", "z", "[", "]"];
    for (const key of removedKeys) {
      actions.length = 0;
      handler.handleKeyEvent(makeKeyEvent("b", true));
      const consumed = handler.handleKeyEvent(makeKeyEvent(key));
      expect(consumed).toBe(true);
      expect(actions).toHaveLength(0);
      expect(handler.state).toBe("idle");
    }
  });

  // --- New tests for keybind string support ---

  test("custom prefix keybind string (Ctrl+Z)", () => {
    const handler = new PrefixKeyHandler("Ctrl+Z");
    const consumed = handler.handleKeyEvent(makeKeyEvent("z", true));
    expect(consumed).toBe(true);
    expect(handler.state).toBe("waiting");

    // Old prefix should not activate
    const handler2 = new PrefixKeyHandler("Ctrl+Z");
    const consumed2 = handler2.handleKeyEvent(makeKeyEvent("b", true));
    expect(consumed2).toBe(false);
    expect(handler2.state).toBe("idle");
  });

  test("custom action bindings override defaults", () => {
    const handler = new PrefixKeyHandler("Ctrl+B", {
      "new-window": "w",
    });
    const actions: MuxAction[] = [];
    handler.setOnAction((a) => actions.push(a));

    handler.handleKeyEvent(makeKeyEvent("b", true));
    handler.handleKeyEvent(makeKeyEvent("w"));

    expect(actions).toHaveLength(1);
    expect(actions[0]!.type).toBe("new-window");
  });

  test("action binding with modifier (Ctrl+N for next-window)", () => {
    const handler = new PrefixKeyHandler("Ctrl+B", {
      "next-window": "Ctrl+N",
    });
    const actions: MuxAction[] = [];
    handler.setOnAction((a) => actions.push(a));

    handler.handleKeyEvent(makeKeyEvent("b", true));
    handler.handleKeyEvent(makeKeyEvent("n", true));

    expect(actions).toHaveLength(1);
    expect(actions[0]!.type).toBe("next-window");
  });

  test("modifier action binding does not match without modifier", () => {
    const handler = new PrefixKeyHandler("Ctrl+B", {
      "next-window": "Ctrl+N",
    });
    const actions: MuxAction[] = [];
    handler.setOnAction((a) => actions.push(a));

    handler.handleKeyEvent(makeKeyEvent("b", true));
    // Press "n" without Ctrl -- should not match "Ctrl+N" binding
    handler.handleKeyEvent(makeKeyEvent("n"));

    // The default binding for "n" was overridden, and bare "n" doesn't match "Ctrl+N"
    expect(actions).toHaveLength(0);
  });

  test("empty custom binding string is ignored", () => {
    const handler = new PrefixKeyHandler("Ctrl+B", {
      "new-window": "",
    });
    const actions: MuxAction[] = [];
    handler.setOnAction((a) => actions.push(a));

    // Default "c" binding for new-window should still work
    handler.handleKeyEvent(makeKeyEvent("b", true));
    handler.handleKeyEvent(makeKeyEvent("c"));

    expect(actions).toHaveLength(1);
    expect(actions[0]!.type).toBe("new-window");
  });

  test("double custom prefix sends passthrough", () => {
    const handler = new PrefixKeyHandler("Ctrl+Z");
    const actions: MuxAction[] = [];
    handler.setOnAction((a) => actions.push(a));

    handler.handleKeyEvent(makeKeyEvent("z", true));
    handler.handleKeyEvent(makeKeyEvent("z", true));

    expect(actions[0]!.type).toBe("prefix-passthrough");
  });
});
