import { describe, test, expect } from "bun:test";
import { PrefixKeyHandler, type MuxAction } from "./prefix-key";

function makeKeyEvent(key: string, ctrlKey = false): KeyboardEvent {
  return new KeyboardEvent("keydown", { key, ctrlKey });
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

  test("prefix + % dispatches split-vertical", () => {
    const handler = new PrefixKeyHandler();
    const actions: MuxAction[] = [];
    handler.setOnAction((a) => actions.push(a));

    handler.handleKeyEvent(makeKeyEvent("b", true));
    handler.handleKeyEvent(makeKeyEvent("%"));

    expect(actions).toHaveLength(1);
    expect(actions[0]!.type).toBe("split-vertical");
    expect(handler.state).toBe("idle");
  });

  test('prefix + " dispatches split-horizontal', () => {
    const handler = new PrefixKeyHandler();
    const actions: MuxAction[] = [];
    handler.setOnAction((a) => actions.push(a));

    handler.handleKeyEvent(makeKeyEvent("b", true));
    handler.handleKeyEvent(makeKeyEvent('"'));

    expect(actions[0]!.type).toBe("split-horizontal");
  });

  test("prefix + d dispatches detach", () => {
    const handler = new PrefixKeyHandler();
    const actions: MuxAction[] = [];
    handler.setOnAction((a) => actions.push(a));

    handler.handleKeyEvent(makeKeyEvent("b", true));
    handler.handleKeyEvent(makeKeyEvent("d"));

    expect(actions[0]!.type).toBe("detach");
  });

  test("prefix + z dispatches zoom-toggle", () => {
    const handler = new PrefixKeyHandler();
    const actions: MuxAction[] = [];
    handler.setOnAction((a) => actions.push(a));

    handler.handleKeyEvent(makeKeyEvent("b", true));
    handler.handleKeyEvent(makeKeyEvent("z"));

    expect(actions[0]!.type).toBe("zoom-toggle");
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
      { key: "%", expected: "split-vertical" },
      { key: '"', expected: "split-horizontal" },
      { key: "o", expected: "next-pane" },
      { key: "x", expected: "close-pane" },
      { key: "z", expected: "zoom-toggle" },
      { key: "d", expected: "detach" },
      { key: "c", expected: "new-window" },
      { key: "n", expected: "next-window" },
      { key: "p", expected: "prev-window" },
      { key: ",", expected: "rename-window" },
      { key: "[", expected: "copy-mode" },
      { key: "]", expected: "paste" },
    ];

    for (const { key, expected } of bindings) {
      actions.length = 0;
      handler.handleKeyEvent(makeKeyEvent("b", true));
      handler.handleKeyEvent(makeKeyEvent(key));
      expect(actions).toHaveLength(1);
      expect(actions[0]!.type).toBe(expected);
    }
  });
});
