/**
 * Tests for the front matter tree builder (buildTree).
 *
 * Pure-function unit tests (no DOM). Covers SPEC.md TS-7 and task0003
 * Acceptance Criteria AC-1..AC-5. Ported from legacy/webview
 * `src/data-viewer/tree-builder.test.ts` and extended with the scalar-root
 * and depth-cap cases (AC-4, AC-5).
 */

import { describe, expect, test } from "bun:test";

import { buildTree } from "./tree-builder.ts";

describe("buildTree", () => {
  // AC-1: a nested object produces one node per key at every level, in
  // source order, with correct depth and path values.
  test("AC-1: flat object yields one node per key in source order", () => {
    const data = { name: "John", age: 30 };
    const nodes = buildTree(data);
    expect(nodes.length).toBe(2);
    expect(nodes[0]!.key).toBe("name");
    expect(nodes[0]!.depth).toBe(0);
    expect(nodes[0]!.path).toBe("name");
    expect(nodes[1]!.key).toBe("age");
    expect(nodes[1]!.depth).toBe(0);
    expect(nodes[1]!.path).toBe("age");
  });

  test("AC-1: nested object increments depth and dot-joins paths", () => {
    const data = { server: { host: "localhost", port: 8080 } };
    const nodes = buildTree(data);
    expect(nodes.length).toBe(3);
    expect(nodes[0]!.key).toBe("server");
    expect(nodes[0]!.depth).toBe(0);
    expect(nodes[0]!.path).toBe("server");
    expect(nodes[1]!.key).toBe("host");
    expect(nodes[1]!.depth).toBe(1);
    expect(nodes[1]!.path).toBe("server.host");
    expect(nodes[2]!.key).toBe("port");
    expect(nodes[2]!.depth).toBe(1);
    expect(nodes[2]!.path).toBe("server.port");
  });

  // AC-2: arrays produce `[i]`-keyed nodes; nested containers inside arrays
  // recurse with incremented depth.
  test("AC-2: array elements are keyed [i] with bracket paths", () => {
    const data = [1, 2, 3];
    const nodes = buildTree(data);
    expect(nodes.length).toBe(3);
    expect(nodes[0]!.key).toBe("[0]");
    expect(nodes[0]!.path).toBe("[0]");
    expect(nodes[0]!.depth).toBe(0);
    expect(nodes[1]!.key).toBe("[1]");
    expect(nodes[2]!.key).toBe("[2]");
  });

  test("AC-2: containers inside arrays recurse with incremented depth", () => {
    const data = { items: [{ name: "a" }, { name: "b" }] };
    const nodes = buildTree(data);
    // items (0), [0] (1), name (2), [1] (3), name (4)
    expect(nodes.length).toBe(5);
    expect(nodes[0]!.key).toBe("items");
    expect(nodes[0]!.depth).toBe(0);
    expect(nodes[0]!.hasChildren).toBe(true);
    expect(nodes[1]!.key).toBe("[0]");
    expect(nodes[1]!.depth).toBe(1);
    expect(nodes[1]!.path).toBe("items[0]");
    expect(nodes[1]!.hasChildren).toBe(true);
    expect(nodes[2]!.key).toBe("name");
    expect(nodes[2]!.depth).toBe(2);
    expect(nodes[2]!.path).toBe("items[0].name");
    expect(nodes[3]!.key).toBe("[1]");
    expect(nodes[3]!.depth).toBe(1);
    expect(nodes[3]!.path).toBe("items[1]");
    expect(nodes[4]!.key).toBe("name");
    expect(nodes[4]!.depth).toBe(2);
    expect(nodes[4]!.path).toBe("items[1].name");
  });

  // AC-3: leaf nodes report has-children false; container nodes true.
  test("AC-3: leaves report hasChildren false, containers true", () => {
    const data = { scalar: 1, obj: { a: 1 }, arr: [1], nil: null };
    const nodes = buildTree(data);
    // scalar(0) obj(0) a(1) arr(0) [0](1) nil(0)
    expect(nodes.length).toBe(6);
    expect(nodes[0]!.path).toBe("scalar");
    expect(nodes[0]!.hasChildren).toBe(false);
    expect(nodes[1]!.path).toBe("obj");
    expect(nodes[1]!.hasChildren).toBe(true);
    expect(nodes[2]!.path).toBe("obj.a");
    expect(nodes[2]!.hasChildren).toBe(false);
    expect(nodes[3]!.path).toBe("arr");
    expect(nodes[3]!.hasChildren).toBe(true);
    expect(nodes[4]!.path).toBe("arr[0]");
    expect(nodes[4]!.hasChildren).toBe(false);
    // null is a leaf, not a container.
    expect(nodes[5]!.path).toBe("nil");
    expect(nodes[5]!.hasChildren).toBe(false);
    expect(nodes[5]!.value).toBe(null);
  });

  // AC-4: input nested deeper than 128 levels is truncated at the cap
  // without error or stack overflow.
  test("AC-4: nesting deeper than the cap truncates at depth 128 without throwing", () => {
    // Build 200 wrapper levels around a leaf — far deeper than MAX_DEPTH.
    let data: Record<string, unknown> = { leaf: "value" };
    for (let i = 0; i < 200; i++) {
      data = { [`level${i}`]: data };
    }
    let nodes: ReturnType<typeof buildTree> = [];
    expect(() => {
      nodes = buildTree(data);
    }).not.toThrow();
    // Nodes are emitted at depths 0..127 only: exactly 128, capped.
    expect(nodes.length).toBe(128);
    expect(nodes[0]!.depth).toBe(0);
    expect(nodes[nodes.length - 1]!.depth).toBe(127);
    expect(nodes.every((n) => n.depth < 128)).toBe(true);
  });

  test("AC-4: within-cap deep nesting is fully expanded", () => {
    // 10 wrapper levels + 1 leaf = 11 nodes, deepest at depth 10.
    let data: Record<string, unknown> = { leaf: "value" };
    for (let i = 0; i < 10; i++) {
      data = { [`level${i}`]: data };
    }
    const nodes = buildTree(data);
    expect(nodes.length).toBe(11);
    expect(nodes[0]!.depth).toBe(0);
    expect(nodes[10]!.depth).toBe(10);
    expect(nodes[10]!.key).toBe("leaf");
  });

  // AC-5: scalar / null / empty-object roots yield the documented result
  // (an empty node list) without throwing.
  test("AC-5: scalar string root yields an empty list", () => {
    expect(buildTree("hello")).toEqual([]);
  });

  test("AC-5: number and boolean roots yield an empty list", () => {
    expect(buildTree(42)).toEqual([]);
    expect(buildTree(true)).toEqual([]);
  });

  test("AC-5: null root yields an empty list", () => {
    expect(buildTree(null)).toEqual([]);
  });

  test("AC-5: undefined root yields an empty list", () => {
    expect(buildTree(undefined)).toEqual([]);
  });

  test("AC-5: empty object and empty array roots yield an empty list", () => {
    expect(buildTree({})).toEqual([]);
    expect(buildTree([])).toEqual([]);
  });
});
