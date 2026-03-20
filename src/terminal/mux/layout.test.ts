import { describe, test, expect } from "bun:test";
import {
  calculateLayout,
  splitPane,
  removePane,
  getAllPaneIds,
  presetLayout,
  MIN_PANE_COLS,
  MIN_PANE_ROWS,
  type LayoutNode,
} from "./layout";

const CELL_W = 8;
const CELL_H = 16;

describe("calculateLayout", () => {
  test("single pane fills container", () => {
    const root: LayoutNode = { type: "leaf", paneId: 1 };
    const results = calculateLayout(root, 800, 480, CELL_W, CELL_H);
    expect(results).toHaveLength(1);
    expect(results[0]!.paneId).toBe(1);
    expect(results[0]!.rect).toEqual({ x: 0, y: 0, width: 800, height: 480 });
    expect(results[0]!.cols).toBe(100); // 800/8
    expect(results[0]!.rows).toBe(30); // 480/16
  });

  test("vertical split produces two panes side by side", () => {
    const root: LayoutNode = {
      type: "split",
      direction: "vertical",
      ratio: 0.5,
      first: { type: "leaf", paneId: 1 },
      second: { type: "leaf", paneId: 2 },
    };
    const results = calculateLayout(root, 800, 480, CELL_W, CELL_H);
    expect(results).toHaveLength(2);
    // First pane: left half
    expect(results[0]!.paneId).toBe(1);
    expect(results[0]!.rect.x).toBe(0);
    expect(results[0]!.rect.width).toBeLessThan(400);
    // Second pane: right half
    expect(results[1]!.paneId).toBe(2);
    expect(results[1]!.rect.x).toBeGreaterThan(399);
  });

  test("horizontal split produces two panes stacked", () => {
    const root: LayoutNode = {
      type: "split",
      direction: "horizontal",
      ratio: 0.5,
      first: { type: "leaf", paneId: 1 },
      second: { type: "leaf", paneId: 2 },
    };
    const results = calculateLayout(root, 800, 480, CELL_W, CELL_H);
    expect(results).toHaveLength(2);
    expect(results[0]!.rect.y).toBe(0);
    expect(results[1]!.rect.y).toBeGreaterThan(239);
  });

  test("minimum pane size enforced", () => {
    const root: LayoutNode = { type: "leaf", paneId: 1 };
    const results = calculateLayout(root, 10, 10, CELL_W, CELL_H);
    expect(results[0]!.cols).toBe(MIN_PANE_COLS);
    expect(results[0]!.rows).toBe(MIN_PANE_ROWS);
  });
});

describe("splitPane", () => {
  test("splits a single pane vertically", () => {
    const root: LayoutNode = { type: "leaf", paneId: 1 };
    const result = splitPane(root, 1, 2, "vertical", 800, 480, CELL_W, CELL_H);
    expect(result).not.toBeNull();
    expect(result!.type).toBe("split");
    if (result!.type === "split") {
      expect(result!.direction).toBe("vertical");
      expect(getAllPaneIds(result!)).toEqual([1, 2]);
    }
  });

  test("splits a single pane horizontally", () => {
    const root: LayoutNode = { type: "leaf", paneId: 1 };
    const result = splitPane(root, 1, 2, "horizontal", 800, 480, CELL_W, CELL_H);
    expect(result).not.toBeNull();
    if (result!.type === "split") {
      expect(result!.direction).toBe("horizontal");
    }
  });

  test("returns null when pane too small to split", () => {
    const root: LayoutNode = { type: "leaf", paneId: 1 };
    // Container too small for two panes side by side
    const result = splitPane(root, 1, 2, "vertical", 100, 480, CELL_W, CELL_H);
    expect(result).toBeNull();
  });

  test("returns null for non-existent pane", () => {
    const root: LayoutNode = { type: "leaf", paneId: 1 };
    const result = splitPane(root, 99, 2, "vertical", 800, 480, CELL_W, CELL_H);
    expect(result).toBeNull();
  });

  test("splits nested pane", () => {
    const root: LayoutNode = {
      type: "split",
      direction: "vertical",
      ratio: 0.5,
      first: { type: "leaf", paneId: 1 },
      second: { type: "leaf", paneId: 2 },
    };
    const result = splitPane(root, 2, 3, "horizontal", 800, 480, CELL_W, CELL_H);
    expect(result).not.toBeNull();
    expect(getAllPaneIds(result!)).toEqual([1, 2, 3]);
  });
});

describe("removePane", () => {
  test("removes from two-pane split, sibling takes over", () => {
    const root: LayoutNode = {
      type: "split",
      direction: "vertical",
      ratio: 0.5,
      first: { type: "leaf", paneId: 1 },
      second: { type: "leaf", paneId: 2 },
    };
    const result = removePane(root, 1);
    expect(result).not.toBeNull();
    expect(result!.type).toBe("leaf");
    if (result!.type === "leaf") {
      expect(result!.paneId).toBe(2);
    }
  });

  test("returns null when removing last pane", () => {
    const root: LayoutNode = { type: "leaf", paneId: 1 };
    expect(removePane(root, 1)).toBeNull();
  });

  test("removes nested pane", () => {
    // 1 | (2 / 3)
    const root: LayoutNode = {
      type: "split",
      direction: "vertical",
      ratio: 0.5,
      first: { type: "leaf", paneId: 1 },
      second: {
        type: "split",
        direction: "horizontal",
        ratio: 0.5,
        first: { type: "leaf", paneId: 2 },
        second: { type: "leaf", paneId: 3 },
      },
    };
    const result = removePane(root, 2);
    expect(result).not.toBeNull();
    expect(getAllPaneIds(result!)).toEqual([1, 3]);
  });

  test("returns unchanged tree for non-existent pane", () => {
    const root: LayoutNode = { type: "leaf", paneId: 1 };
    const result = removePane(root, 99);
    expect(result).toEqual(root);
  });
});

describe("getAllPaneIds", () => {
  test("single pane", () => {
    expect(getAllPaneIds({ type: "leaf", paneId: 1 })).toEqual([1]);
  });

  test("nested panes", () => {
    const root: LayoutNode = {
      type: "split",
      direction: "vertical",
      ratio: 0.5,
      first: { type: "leaf", paneId: 1 },
      second: {
        type: "split",
        direction: "horizontal",
        ratio: 0.5,
        first: { type: "leaf", paneId: 2 },
        second: { type: "leaf", paneId: 3 },
      },
    };
    expect(getAllPaneIds(root)).toEqual([1, 2, 3]);
  });
});

describe("presetLayout", () => {
  test("even-horizontal with 3 panes", () => {
    const root = presetLayout([1, 2, 3], "even-horizontal");
    expect(root).not.toBeNull();
    expect(getAllPaneIds(root!)).toEqual([1, 2, 3]);
  });

  test("even-vertical with 3 panes", () => {
    const root = presetLayout([1, 2, 3], "even-vertical");
    expect(root).not.toBeNull();
    expect(getAllPaneIds(root!)).toEqual([1, 2, 3]);
  });

  test("main-horizontal with 3 panes", () => {
    const root = presetLayout([1, 2, 3], "main-horizontal");
    expect(root).not.toBeNull();
    expect(getAllPaneIds(root!)).toEqual([1, 2, 3]);
    // First pane should be in the top half
    if (root!.type === "split") {
      expect(root!.direction).toBe("horizontal");
      expect(root!.first.type).toBe("leaf");
    }
  });

  test("main-vertical with 3 panes", () => {
    const root = presetLayout([1, 2, 3], "main-vertical");
    expect(root).not.toBeNull();
    if (root!.type === "split") {
      expect(root!.direction).toBe("vertical");
    }
  });

  test("tiled with 4 panes", () => {
    const root = presetLayout([1, 2, 3, 4], "tiled");
    expect(root).not.toBeNull();
    expect(getAllPaneIds(root!)).toEqual([1, 2, 3, 4]);
  });

  test("single pane returns leaf", () => {
    const root = presetLayout([1], "even-horizontal");
    expect(root).toEqual({ type: "leaf", paneId: 1 });
  });

  test("empty returns null", () => {
    expect(presetLayout([], "even-horizontal")).toBeNull();
  });
});
