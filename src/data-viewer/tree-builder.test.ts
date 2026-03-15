import { describe, test, expect } from "bun:test";
import { buildTree } from "./tree-builder.ts";

describe("buildTree", () => {
  test("builds tree from flat object", () => {
    const data = { name: "John", age: 30 };
    const nodes = buildTree(data);
    expect(nodes.length).toBe(2);
    expect(nodes[0]!.key).toBe("name");
    expect(nodes[0]!.depth).toBe(0);
    expect(nodes[0]!.hasChildren).toBe(false);
    expect(nodes[1]!.key).toBe("age");
  });

  test("builds tree from nested object", () => {
    const data = { server: { host: "localhost", port: 8080 } };
    const nodes = buildTree(data);
    expect(nodes.length).toBe(3);
    expect(nodes[0]!.key).toBe("server");
    expect(nodes[0]!.depth).toBe(0);
    expect(nodes[0]!.hasChildren).toBe(true);
    expect(nodes[1]!.key).toBe("host");
    expect(nodes[1]!.depth).toBe(1);
    expect(nodes[2]!.key).toBe("port");
    expect(nodes[2]!.depth).toBe(1);
  });

  test("builds tree from array", () => {
    const data = [1, 2, 3];
    const nodes = buildTree(data);
    expect(nodes.length).toBe(3);
    expect(nodes[0]!.key).toBe("[0]");
    expect(nodes[1]!.key).toBe("[1]");
    expect(nodes[2]!.key).toBe("[2]");
  });

  test("builds tree from nested array", () => {
    const data = { items: [{ name: "a" }, { name: "b" }] };
    const nodes = buildTree(data);
    // items (0), [0] (1), name (2), [1] (3), name (4)
    expect(nodes.length).toBe(5);
    expect(nodes[0]!.key).toBe("items");
    expect(nodes[0]!.hasChildren).toBe(true);
    expect(nodes[1]!.key).toBe("[0]");
    expect(nodes[1]!.depth).toBe(1);
    expect(nodes[2]!.key).toBe("name");
    expect(nodes[2]!.depth).toBe(2);
  });

  test("handles empty object", () => {
    const nodes = buildTree({});
    expect(nodes.length).toBe(0);
  });

  test("handles empty array", () => {
    const nodes = buildTree([]);
    expect(nodes.length).toBe(0);
  });

  test("handles null values", () => {
    const data = { key: null };
    const nodes = buildTree(data);
    expect(nodes.length).toBe(1);
    expect(nodes[0]!.hasChildren).toBe(false);
    expect(nodes[0]!.value).toBe(null);
  });

  test("handles deeply nested structure", () => {
    let data: Record<string, unknown> = { leaf: "value" };
    for (let i = 0; i < 10; i++) {
      data = { [`level${i}`]: data };
    }
    const nodes = buildTree(data);
    // 10 levels of nesting + 1 leaf = 11 nodes
    expect(nodes.length).toBe(11);
    expect(nodes[0]!.depth).toBe(0);
    expect(nodes[10]!.depth).toBe(10);
  });

  test("handles primitive root (non-object)", () => {
    const nodes = buildTree("hello");
    expect(nodes.length).toBe(0);
  });

  test("sets correct paths", () => {
    const data = { server: { host: "localhost" } };
    const nodes = buildTree(data);
    expect(nodes[0]!.path).toBe("server");
    expect(nodes[1]!.path).toBe("server.host");
  });
});
